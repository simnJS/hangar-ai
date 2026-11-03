//! A task in a dialog (`TaskDialog.tsx`): read and edited as it is made, or
//! written new.
//!
//! Opening a task shows the board's version of it, and every edit goes to the
//! board when the field is left (the web app saved on blur; iced's fields have
//! no blur, so a field is saved when anything else in the dialog is used, when
//! Enter is pressed in a one-line field, and when the dialog closes). What an
//! agent changes meanwhile (the title it renames, the column it moves the
//! task to) replaces what is on screen, as it did.
//!
//! `Overlay::Task { task: None }` is the same dialog for a task that does not
//! exist yet; the board's own "add" fields make a card from a title alone, and
//! this is the way to give it a description, a priority and the files its
//! worker may change at once.

use hangar_core::board::Task as BoardTask;
use hangar_i18n::{Key, Translator, args};
use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use hangar_ui::button;
use hangar_ui::icon::{Icon, icon};
use hangar_ui::input::{NumberField, NumberMsg, number_input};
use hangar_ui::layer;
use hangar_ui::tip;
use iced::widget::{
    Space, checkbox, column, container, pick_list, row, rule, scrollable, text, text_editor,
    text_input,
};
use iced::{Alignment, Element, Length, Padding, Task};

use crate::message::{Intent, Message};
use crate::state::Model;

use super::actions::{self, After, Draft, Edit, Errors, Mutation};
use super::card::assignee_view;
use super::format::{DateStyle, short_task_id, when};
use super::look;
use super::model::{Column, MAX_BLOCK_REASON, Task as T, index_by_id, recheck_problem};
use super::view::View;
use super::{Msg as BoardMsg, State, card_id};

/// The thread scrolls to its newest message.
const THREAD: &str = "board-task-thread";
const TITLE_FIELD: &str = "board-task-title";
const REASON_FIELD: &str = "board-task-reason";

/// A dialog's open task, and everything typed that is not on the board yet.
#[derive(Debug)]
pub struct Dialog {
    pub cwd: String,
    /// The task, or `None` for one being written.
    pub task: Option<String>,
    title: String,
    description: text_editor::Content,
    /// The paths and globs the task's worker may change, one per line.
    owns: text_editor::Content,
    priority: NumberField,
    /// The column of a task being written.
    column: Column,
    /// The dependencies of a task being written.
    depends_on: Vec<String>,
    /// What the task said last time it was read: edits are saved against it,
    /// and a change that is not ours replaces what is typed.
    seen: Seen,
    comment: String,
    blocking: bool,
    reason: String,
    recheck: String,
    human_only: bool,
    note: String,
    error: Option<String>,
    confirm_delete: bool,
    /// How many comments the thread showed, to follow a new one.
    comments: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Seen {
    title: String,
    description: String,
    owns: String,
}

impl Seen {
    fn of(task: &BoardTask) -> Seen {
        Seen {
            title: task.title.clone(),
            description: task.description.clone(),
            owns: task.owns.join("\n"),
        }
    }
}

/// The paths of a "one per line" field, trimmed, without the empty lines.
fn patterns(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What an editor holds, with the line endings of the board's file.
fn text_of(content: &text_editor::Content) -> String {
    content.text().replace("\r\n", "\n")
}

fn priority_field(value: u8) -> NumberField {
    NumberField::new(f64::from(value), 0.0, 9.0, 1.0)
}

impl Dialog {
    /// The dialog for `task` of the board in `cwd`, or for a new one.
    pub fn open(cwd: &str, task: Option<&str>, tasks: &[T]) -> Dialog {
        let found = task.and_then(|id| tasks.iter().find(|candidate| candidate.id == id));
        let seen = found.map(Seen::of).unwrap_or_default();
        Dialog {
            cwd: cwd.to_owned(),
            task: task.map(str::to_owned),
            title: seen.title.clone(),
            description: text_editor::Content::with_text(&seen.description),
            owns: text_editor::Content::with_text(&seen.owns),
            priority: priority_field(found.map_or(1, |task| task.priority)),
            column: Column::Todo,
            depends_on: Vec::new(),
            comments: found.map_or(0, |task| task.comments.len()),
            seen,
            comment: String::new(),
            blocking: false,
            reason: String::new(),
            recheck: String::new(),
            human_only: false,
            note: String::new(),
            error: None,
            confirm_delete: false,
        }
    }

    pub fn is_new(&self) -> bool {
        self.task.is_none()
    }

    /// A task being written starts from what was typed in a column's add
    /// field: its title, and the column.
    pub fn seed(&mut self, column: Column, title: String) {
        self.column = column;
        self.title = title.trim().to_owned();
    }

    /// Follows the task as the board has it now: what is not ours replaces
    /// what is typed. Answers whether the thread grew.
    pub fn follow(&mut self, task: &BoardTask) -> bool {
        let now = Seen::of(task);
        if now.title != self.seen.title {
            self.title = now.title.clone();
        }
        if now.description != self.seen.description {
            self.description = text_editor::Content::with_text(&now.description);
        }
        if now.owns != self.seen.owns {
            self.owns = text_editor::Content::with_text(&now.owns);
        }
        self.seen = now;
        if (self.priority.value() - f64::from(task.priority)).abs() > f64::EPSILON
            && self.priority.is_valid()
        {
            self.priority.set(f64::from(task.priority));
        }
        let grew = task.comments.len() != self.comments;
        self.comments = task.comments.len();
        grew
    }

    /// What was typed and not saved yet, as the edits that save it.
    fn pending(&mut self) -> Vec<Edit> {
        if self.is_new() {
            return Vec::new();
        }
        let mut edits = Vec::new();
        if self.title != self.seen.title {
            edits.push(Edit::Title(self.title.clone()));
            self.seen.title = self.title.clone();
        }
        let description = text_of(&self.description);
        if description != self.seen.description {
            edits.push(Edit::Description(description.clone()));
            self.seen.description = description;
        }
        let owns = patterns(&text_of(&self.owns));
        if owns.join("\n") != self.seen.owns {
            self.seen.owns = owns.join("\n");
            edits.push(Edit::Owns(owns));
        }
        edits
    }

    /// The block form and the unblock note, back to rest.
    fn finish_block(&mut self) {
        self.blocking = false;
        self.reason.clear();
        self.recheck.clear();
        self.human_only = false;
        self.note.clear();
        self.error = None;
    }

    fn draft(&self) -> Draft {
        Draft {
            title: self.title.trim().to_owned(),
            description: text_of(&self.description),
            column: self.column,
            priority: self.priority.value().round().clamp(0.0, 9.0) as u8,
            owns: patterns(&text_of(&self.owns)),
            depends_on: self.depends_on.clone(),
        }
    }
}

/// Messages of the dialog, routed as `Msg::Dialog`.
#[derive(Debug, Clone)]
pub enum Msg {
    Title(String),
    SaveTitle,
    Description(text_editor::Action),
    Owns(text_editor::Action),
    Column(Column),
    Priority(NumberMsg),
    Release,
    StartBlock,
    CancelBlock,
    Reason(String),
    Recheck(String),
    HumanOnly(bool),
    ConfirmBlock,
    Note(String),
    Unblock,
    Comment(String),
    Send,
    AskDelete,
    CancelDelete,
    ConfirmDelete,
    Create,
    AddDependency(Dependency),
    RemoveDependency(String),
    Close,
}

impl Msg {
    /// Typing in a field is not leaving it.
    fn is_typing(&self) -> bool {
        matches!(
            self,
            Msg::Title(_)
                | Msg::Description(_)
                | Msg::Owns(_)
                | Msg::Reason(_)
                | Msg::Recheck(_)
                | Msg::Note(_)
                | Msg::Comment(_)
        )
    }
}

/// A task a dependency can be added from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub id: String,
    label: String,
}

impl std::fmt::Display for Dependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn task_named<'a>(m: &'a Model, cwd: &str, id: &str) -> Option<&'a T> {
    m.live
        .board
        .tasks(cwd)?
        .iter()
        .find(|candidate| candidate.id == id)
}

/// Saves what is pending, as mutations.
fn save(cwd: &str, id: &str, edits: Vec<Edit>) -> Task<Message> {
    Task::batch(edits.into_iter().map(|edit| {
        actions::run(
            cwd,
            Mutation::Update {
                id: id.to_owned(),
                edit,
            },
            Errors::Swallow,
            After::Nothing,
        )
    }))
}

/// A task for the dialog to open on, when it is opened from `Intent::OpenTask`.
pub fn opened(st: &mut State, m: &Model, cwd: &str, task: Option<&str>) -> Task<Message> {
    if st
        .dialog
        .as_ref()
        .is_some_and(|open| open.cwd == cwd && open.task.as_deref() == task)
    {
        return Task::none();
    }
    let tasks = m.live.board.tasks(cwd).unwrap_or(&[]);
    let dialog = Dialog::open(cwd, task, tasks);
    let thread = dialog.comments > 0;
    st.dialog = Some(dialog);
    if thread {
        iced::widget::operation::snap_to_end(THREAD)
    } else {
        Task::none()
    }
}

/// The dialog is gone: what was typed is saved, and the keyboard goes back to
/// the card that opened it. That card is re-made when an agent moves the task,
/// and a deleted task has none: the column's add field takes it then.
pub fn closed(st: &mut State, m: &Model) -> Task<Message> {
    let Some(mut dialog) = st.dialog.take() else {
        return Task::none();
    };
    let Some(id) = dialog.task.clone() else {
        return Task::none();
    };
    let edits = dialog.pending();
    let save_edits = save(&dialog.cwd, &id, edits);
    let focus = match task_named(m, &dialog.cwd, &id) {
        Some(task) => iced::widget::operation::focus(card_id(&task.id)),
        None => Task::none(),
    };
    Task::batch([save_edits, focus])
}

pub fn update(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    let tr = Translator::new(m.locale());
    let Some(dialog) = st.dialog.as_mut() else {
        return Task::none();
    };
    let cwd = dialog.cwd.clone();
    let id = dialog.task.clone();

    // Using anything else is leaving the field that was typed in.
    let flush = if msg.is_typing() {
        Task::none()
    } else {
        match &id {
            Some(id) => save(&cwd, id, dialog.pending()),
            None => Task::none(),
        }
    };

    let act = match msg {
        Msg::Title(text) => {
            dialog.title = text;
            Task::none()
        }
        // Saved with everything else that was typed, above.
        Msg::SaveTitle => Task::none(),
        Msg::Description(action) => {
            dialog.description.perform(action);
            Task::none()
        }
        Msg::Owns(action) => {
            dialog.owns.perform(action);
            Task::none()
        }
        Msg::Column(column) => match &id {
            Some(id) => mutate(&cwd, id, Edit::Column(column), Errors::Swallow),
            None => {
                dialog.column = column;
                Task::none()
            }
        },
        Msg::Priority(change) => match dialog.priority.update(change) {
            Some(value) => match &id {
                Some(id) => mutate(&cwd, id, Edit::Priority(value as u8), Errors::Swallow),
                None => Task::none(),
            },
            None => Task::none(),
        },
        Msg::Release => match &id {
            Some(id) => mutate(&cwd, id, Edit::Release, Errors::Swallow),
            None => Task::none(),
        },
        Msg::StartBlock => {
            dialog.blocking = true;
            focus_soon(REASON_FIELD)
        }
        Msg::CancelBlock => {
            dialog.blocking = false;
            dialog.error = None;
            Task::none()
        }
        Msg::Reason(text) => {
            dialog.reason = text.chars().take(MAX_BLOCK_REASON).collect();
            Task::none()
        }
        Msg::Recheck(text) => {
            dialog.recheck = text;
            Task::none()
        }
        Msg::HumanOnly(on) => {
            dialog.human_only = on;
            Task::none()
        }
        Msg::ConfirmBlock => {
            if dialog.reason.trim().is_empty() {
                dialog.error = Some(tr.t(Key::BoardBlockReasonRequired).into_owned());
                Task::none()
            } else if let Some(problem) =
                recheck_problem(&dialog.recheck, &hangar_core::board::today())
            {
                dialog.error = Some(tr.t(problem.message_key()).into_owned());
                Task::none()
            } else if let Some(id) = &id {
                dialog.error = None;
                mutate(
                    &cwd,
                    id,
                    Edit::Block {
                        reason: dialog.reason.clone(),
                        recheck: dialog.recheck.clone(),
                        human_only: dialog.human_only,
                    },
                    Errors::ToDialog,
                )
            } else {
                Task::none()
            }
        }
        Msg::Note(text) => {
            dialog.note = text;
            Task::none()
        }
        Msg::Unblock => match &id {
            Some(id) => mutate(
                &cwd,
                id,
                Edit::Unblock {
                    note: dialog.note.clone(),
                },
                Errors::ToDialog,
            ),
            None => Task::none(),
        },
        Msg::Comment(text) => {
            dialog.comment = text;
            Task::none()
        }
        Msg::Send => {
            let text = dialog.comment.trim().to_owned();
            match (&id, text.is_empty()) {
                (Some(id), false) => {
                    dialog.comment.clear();
                    actions::run(
                        &cwd,
                        Mutation::Comment {
                            id: id.clone(),
                            text,
                        },
                        Errors::Swallow,
                        After::Nothing,
                    )
                }
                _ => Task::none(),
            }
        }
        Msg::AskDelete => {
            dialog.confirm_delete = true;
            Task::none()
        }
        Msg::CancelDelete => {
            dialog.confirm_delete = false;
            Task::none()
        }
        Msg::ConfirmDelete => match &id {
            Some(id) => actions::run(
                &cwd,
                Mutation::Delete { id: id.clone() },
                Errors::Swallow,
                After::CloseAlways,
            ),
            None => Task::none(),
        },
        Msg::Create => {
            let draft = dialog.draft();
            if draft.title.is_empty() {
                Task::none()
            } else {
                dialog.error = None;
                actions::run(
                    &cwd,
                    Mutation::Create(draft),
                    Errors::ToDialog,
                    After::CloseOnOk,
                )
            }
        }
        Msg::AddDependency(choice) => match &id {
            Some(id) => {
                let mut ids = task_named(m, &cwd, id)
                    .map(|task| task.depends_on.clone())
                    .unwrap_or_default();
                if !ids.contains(&choice.id) {
                    ids.push(choice.id);
                }
                mutate(&cwd, id, Edit::DependsOn(ids), Errors::Swallow)
            }
            None => {
                if !dialog.depends_on.contains(&choice.id) {
                    dialog.depends_on.push(choice.id);
                }
                Task::none()
            }
        },
        Msg::RemoveDependency(dep) => match &id {
            Some(id) => {
                let mut ids = task_named(m, &cwd, id)
                    .map(|task| task.depends_on.clone())
                    .unwrap_or_default();
                ids.retain(|candidate| candidate != &dep);
                mutate(&cwd, id, Edit::DependsOn(ids), Errors::Swallow)
            }
            None => {
                dialog.depends_on.retain(|candidate| candidate != &dep);
                Task::none()
            }
        },
        Msg::Close => Task::done(Message::Intent(Intent::CloseOverlay)),
    };
    Task::batch([flush, act])
}

fn mutate(cwd: &str, id: &str, edit: Edit, errors: Errors) -> Task<Message> {
    actions::run(
        cwd,
        Mutation::Update {
            id: id.to_owned(),
            edit,
        },
        errors,
        After::Nothing,
    )
}

/// The focus to a field that is on screen after this message's view, which
/// the operation would otherwise run before.
pub fn focus_soon(id: &'static str) -> Task<Message> {
    Task::perform(
        async { tokio::time::sleep(std::time::Duration::from_millis(30)).await },
        move |()| Message::Board(BoardMsg::FocusField(id)),
    )
}

/// A change of the board answered: clear the forms it settled, say why it was
/// refused, and close the dialog when it was asked to.
pub fn mutated(
    st: &mut State,
    errors: Errors,
    after: After,
    result: &Result<(), String>,
) -> Task<Message> {
    let mut close = false;
    if let Some(dialog) = st.dialog.as_mut() {
        match result {
            Ok(()) => {
                if errors == Errors::ToDialog {
                    dialog.finish_block();
                }
                close = after != After::Nothing;
            }
            Err(error) => {
                if errors == Errors::ToDialog {
                    dialog.error = Some(error.clone());
                }
                close = after == After::CloseAlways;
            }
        }
    }
    if close {
        Task::done(Message::Intent(Intent::CloseOverlay))
    } else {
        Task::none()
    }
}

// ---- The view --------------------------------------------------------------------------------

fn label<'a>(t: &'a Theme, words: impl Into<String>, hint: Option<String>) -> Element<'a, Message> {
    let mut line = row![
        text(words.into())
            .size(font_size::CAPTION)
            .font(fonts::ui_semibold())
            .style(style::text::muted(t))
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);
    if let Some(hint) = hint {
        line = line.push(
            text(hint)
                .size(font_size::CAPTION)
                .style(style::text::subtle(t)),
        );
    }
    line.into()
}

fn field<'a>(t: &'a Theme, placeholder: &str, value: &str) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .size(font_size::BODY)
        .padding([6.0, space::LG])
        .style(style::text_input::field(t))
}

fn editor<'a>(
    t: &'a Theme,
    content: &'a text_editor::Content,
    min_height: f32,
    on_action: impl Fn(text_editor::Action) -> Message + 'a,
) -> Element<'a, Message> {
    text_editor(content)
        .on_action(on_action)
        .size(font_size::BODY)
        .padding([6.0, space::LG])
        .min_height(min_height)
        .style(style::text_editor::field(t))
        .into()
}

/// A field with the keymap asked first (`keymap::guard`): a shortcut it takes
/// is spent before the field sees it, so Ctrl+Shift+Enter in the title splits
/// a pane behind the dialog and does not also save the task.
fn guarded<'a>(v: &View<'a>, field: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    crate::keymap::guard(v.ctx, field)
}

fn stack_of<'a>(parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column(parts).spacing(space::SM).into()
}

fn msg(inner: Msg) -> Message {
    Message::Board(BoardMsg::Dialog(inner))
}

/// `Overlay::Task`: the dialog over the window.
pub fn view<'a>(
    st: &'a State,
    ctx: crate::ctx::Ctx<'a>,
    cwd: &'a str,
    task: Option<&'a str>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let page = Space::new().width(Length::Fill).height(Length::Fill);
    let Some(dialog) = st
        .dialog
        .as_ref()
        .filter(|open| open.cwd == cwd && open.task.as_deref() == task)
    else {
        return page.into();
    };
    let v = View::new(st, ctx, cwd, false);
    let found = task.and_then(|id| v.all.iter().find(|candidate| candidate.id == id));
    let content = match (task, found) {
        (Some(_), Some(found)) => edit_form(&v, dialog, found),
        // The task is gone (an agent deleted it): the board closes the dialog
        // with the next read; until then there is nothing to show.
        (Some(_), None) => return page.into(),
        (None, _) => create_form(&v, dialog),
    };
    layer::modal(page, Some(content))
        .veil(style::container::scrim(t))
        .on_dismiss(msg(Msg::Close))
        .into()
}

fn shell<'a>(
    t: &'a Theme,
    head: Element<'a, Message>,
    body: Element<'a, Message>,
    foot: Element<'a, Message>,
) -> Element<'a, Message> {
    let line = || rule::horizontal(1).style(style::rule::hairline(t));
    container(column![
        head,
        line(),
        container(
            scrollable(container(body).padding(Padding {
                top: space::XXL,
                right: space::XXL,
                bottom: 18.0,
                left: space::XXL,
            }))
            .style(style::scrollable::default(t)),
        )
        .max_height(look::DIALOG_BODY_MAX),
        line(),
        foot,
    ])
    .width(Length::Fill)
    .max_width(look::DIALOG_WIDTH)
    .style(look::dialog(t))
    .into()
}

fn head<'a>(
    v: &View<'a>,
    column: Option<Column>,
    title: String,
    id: Option<&str>,
) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let mut line = row![].spacing(9.0).align_y(Alignment::Center);
    if let Some(column) = column {
        line = line.push(
            container(Space::new())
                .width(look::MARK_LARGE)
                .height(look::MARK_LARGE)
                .style(look::mark(t, column, look::MARK_LARGE)),
        );
    }
    let mut heading = row![text(title).size(font_size::BODY).font(fonts::ui_semibold())]
        .spacing(space::MD)
        .align_y(Alignment::Center);
    if let Some(id) = id {
        heading = heading.push(tip::over(
            t,
            text(format!("#{}", short_task_id(id)))
                .size(font_size::CAPTION)
                .font(fonts::mono())
                .style(style::text::subtle(t)),
            id.to_owned(),
        ));
    }
    line = line.push(heading);
    if let Some(column) = column {
        line = line.push(
            text(format!("· {}", tr.t(column.label_key())))
                .size(font_size::UI)
                .style(style::text::subtle(t)),
        );
    }
    line = line.push(Space::new().width(Length::Fill)).push(
        button::icon_button(t, Icon::Close)
            .tip(tr.t(Key::TaskClose))
            .on_press(msg(Msg::Close)),
    );
    container(line)
        .padding(Padding {
            top: 0.0,
            right: space::MD,
            bottom: 0.0,
            left: space::XXL,
        })
        .center_y(48.0)
        .into()
}

/// Title and description, then the grid of column, priority and assignee.
fn fields<'a>(
    v: &View<'a>,
    dialog: &'a Dialog,
    column: Column,
    blocked: bool,
    assignee: Option<Element<'a, Message>>,
) -> Vec<Element<'a, Message>> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let mut parts: Vec<Element<'a, Message>> = Vec::new();

    parts.push(stack_of(vec![
        label(t, tr.t(Key::TaskName), None),
        guarded(
            v,
            field(t, "", &dialog.title)
                .id(TITLE_FIELD)
                .font(fonts::ui_semibold())
                .on_input(|text| msg(Msg::Title(text)))
                .on_submit(msg(if dialog.is_new() {
                    Msg::Create
                } else {
                    Msg::SaveTitle
                })),
        ),
    ]));
    parts.push(stack_of(vec![
        label(
            t,
            tr.t(Key::TaskDescription),
            Some(tr.t(Key::TaskDescriptionHint).into_owned()),
        ),
        guarded(
            v,
            editor(t, &dialog.description, 92.0, |action| {
                msg(Msg::Description(action))
            }),
        ),
    ]));

    // While a task is blocked it stays in To do: the others are not offered.
    let choices: Vec<ColumnChoice> = Column::ALL
        .into_iter()
        .filter(|candidate| !blocked || *candidate == Column::Todo)
        .map(|candidate| ColumnChoice {
            column: candidate,
            label: tr.t(candidate.label_key()).into_owned(),
        })
        .collect();
    let selected = choices
        .iter()
        .find(|choice| choice.column == column)
        .cloned();
    let column_field = stack_of(vec![
        label(t, tr.t(Key::TaskColumn), None),
        hangar_ui::input::select(t, choices, selected, |choice| {
            msg(Msg::Column(choice.column))
        }),
    ]);
    let priority_value = dialog.priority.value().round() as u8;
    let priority_field = stack_of(vec![
        label(t, tr.t(Key::TaskPriority), None),
        row![
            container(guarded(
                v,
                number_input(t, &dialog.priority, |change| { msg(Msg::Priority(change)) }),
            ))
            .width(120.0),
            super::card::priority(v, priority_value),
        ]
        .spacing(space::LG)
        .align_y(Alignment::Center)
        .into(),
    ]);
    let mut grid = row![
        container(column_field).width(Length::FillPortion(10)),
        container(priority_field).width(Length::FillPortion(12)),
    ]
    .spacing(space::XL);
    if let Some(who) = assignee {
        grid = grid.push(container(who).width(Length::FillPortion(13)));
    }
    parts.push(grid.into());
    parts
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnChoice {
    column: Column,
    label: String,
}

impl std::fmt::Display for ColumnChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Files the task's worker may change, one per line.
fn owns_field<'a>(v: &View<'a>, dialog: &'a Dialog) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    stack_of(vec![
        label(
            t,
            tr.t(Key::TaskOwns),
            Some(tr.t(Key::TaskOwnsHint).into_owned()),
        ),
        guarded(
            v,
            editor(t, &dialog.owns, 56.0, |action| msg(Msg::Owns(action))),
        ),
    ])
}

fn edit_form<'a>(v: &View<'a>, dialog: &'a Dialog, task: &'a T) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let column = Column::of(task).unwrap_or(Column::Todo);

    let who: Element<'a, Message> = {
        let body: Element<'a, Message> = match &task.assignee {
            Some(assignee) => row![
                assignee_view(v, assignee, column == Column::Done, true),
                button::secondary(t, tr.t(Key::TaskRelease))
                    .small()
                    .on_press(msg(Msg::Release)),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center)
            .into(),
            None => text(tr.t(Key::TaskFree))
                .size(font_size::UI)
                .style(style::text::subtle(t))
                .into(),
        };
        stack_of(vec![
            label(t, tr.t(Key::TaskAssignee), None),
            container(body).center_y(look::FIELD_HEIGHT).into(),
        ])
    };

    let mut body = fields(v, dialog, column, task.blocked.is_some(), Some(who));

    if task.blocked.is_some() || column == Column::Todo || column == Column::Doing {
        body.push(block_section(v, dialog, task));
    }
    body.push(owns_field(v, dialog));
    if !task.labels.is_empty() {
        body.push(
            row(task.labels.iter().map(|text_label| {
                container(
                    text(text_label.as_str())
                        .size(font_size::MICRO)
                        .font(fonts::mono()),
                )
                .padding([0.0, space::SM])
                .style(look::chip(t))
                .into()
            }))
            .spacing(space::XS)
            .wrap()
            .into(),
        );
    }
    body.push(dependencies(v, dialog, Some(task)));
    body.push(thread(v, dialog, task));

    let foot = row![
        text(
            tr.t_with(
                Key::TaskCreatedAt,
                &args!("date" => when(task.created_at, tr.locale(), DateStyle::Short).as_str()),
            )
            .into_owned()
        )
        .size(font_size::CAPTION)
        .style(style::text::subtle(t)),
        Space::new().width(Length::Fill),
    ]
    .push(if dialog.confirm_delete {
        Element::from(
            row![
                button::ghost(t, tr.t(Key::CreateCancel)).on_press(msg(Msg::CancelDelete)),
                button::danger(t, tr.t(Key::TaskDeleteConfirm)).on_press(msg(Msg::ConfirmDelete)),
            ]
            .spacing(space::MD),
        )
    } else {
        button::ghost(t, tr.t(Key::TaskDelete))
            .icon(Icon::Trash)
            .on_press(msg(Msg::AskDelete))
            .into()
    })
    .push(button::primary(t, tr.t(Key::TaskClose)).on_press(msg(Msg::Close)))
    .spacing(space::MD)
    .align_y(Alignment::Center);

    shell(
        t,
        head(
            v,
            Some(column),
            tr.t(Key::TaskTitle).into_owned(),
            Some(&task.id),
        ),
        stack_column(body),
        container(foot).padding([space::LG, 14.0]).into(),
    )
}

fn stack_column<'a>(parts: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column(parts).spacing(14.0).into()
}

fn create_form<'a>(v: &View<'a>, dialog: &'a Dialog) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let mut body = fields(v, dialog, dialog.column, false, None);
    body.push(owns_field(v, dialog));
    body.push(dependencies(v, dialog, None));
    if let Some(error) = &dialog.error {
        body.push(
            text(error.as_str())
                .size(font_size::UI)
                .style(style::text::danger(t))
                .into(),
        );
    }
    let can_create = !dialog.title.trim().is_empty();
    let foot = row![
        Space::new().width(Length::Fill),
        button::ghost(t, tr.t(Key::CreateCancel)).on_press(msg(Msg::Close)),
        button::primary(t, tr.t(Key::TaskCreate))
            .on_press_maybe(can_create.then(|| msg(Msg::Create))),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);
    shell(
        t,
        head(
            v,
            Some(dialog.column),
            tr.t(Key::TaskNew).into_owned(),
            None,
        ),
        stack_column(body),
        container(foot).padding([space::LG, 14.0]).into(),
    )
}

/// Why a task is on hold, and the way to lift it; or the way to put one on
/// hold. Errors the board gives are shown here and nowhere else.
fn block_section<'a>(v: &View<'a>, dialog: &'a Dialog, task: &'a T) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let mut parts: Vec<Element<'a, Message>> = Vec::new();

    if let Some(blocked) = &task.blocked {
        parts.push(
            row![
                icon(Icon::Pause, 12.0).color(t.colors.fg_muted),
                text(
                    tr.t_with(
                        Key::BoardBlockedFor,
                        &args!("reason" => blocked.reason.as_str())
                    )
                    .into_owned()
                )
                .size(font_size::BODY)
                .width(Length::Fill),
            ]
            .spacing(space::SM)
            .into(),
        );
        let mut since = tr
            .t_with(
                Key::BoardBlockedSince,
                &args!("by" => blocked.by.as_str(),
                       "date" => when(blocked.since, tr.locale(), DateStyle::Short).as_str()),
            )
            .into_owned();
        if let Some(day) = &blocked.recheck {
            since.push_str(&format!(
                " · {}",
                tr.t_with(Key::BoardBlockedRecheck, &args!("date" => day.as_str()))
            ));
        }
        if blocked.human_only {
            since.push_str(&format!(" · {}", tr.t(Key::BoardBlockedPerson)));
        }
        parts.push(
            text(since)
                .size(font_size::CAPTION)
                .style(style::text::subtle(t))
                .into(),
        );
        parts.push(
            row![
                guarded(
                    v,
                    field(t, &tr.t(Key::BoardUnblockNote), &dialog.note)
                        .on_input(|text| msg(Msg::Note(text)))
                        .on_submit(msg(Msg::Unblock)),
                ),
                button::secondary(t, tr.t(Key::BoardUnblock)).on_press(msg(Msg::Unblock)),
            ]
            .spacing(space::MD)
            .into(),
        );
    } else if dialog.blocking {
        parts.push(label(
            t,
            tr.t(Key::BoardBlockReason),
            Some(tr.t(Key::BoardBlockHint).into_owned()),
        ));
        parts.push(guarded(
            v,
            field(t, "", &dialog.reason)
                .id(REASON_FIELD)
                .on_input(|text| msg(Msg::Reason(text)))
                .on_submit(msg(Msg::ConfirmBlock)),
        ));
        parts.push(label(t, tr.t(Key::BoardBlockRecheck), None));
        parts.push(guarded(
            v,
            field(t, "YYYY-MM-DD", &dialog.recheck)
                .on_input(|text| msg(Msg::Recheck(text)))
                .on_submit(msg(Msg::ConfirmBlock)),
        ));
        parts.push(
            checkbox(dialog.human_only)
                .label(tr.t(Key::BoardBlockHumanOnly))
                .on_toggle(|on| msg(Msg::HumanOnly(on)))
                .size(16)
                .text_size(font_size::BODY)
                .style(style::checkbox::default(t))
                .into(),
        );
        parts.push(
            row![
                button::ghost(t, tr.t(Key::CreateCancel)).on_press(msg(Msg::CancelBlock)),
                button::secondary(t, tr.t(Key::BoardBlockConfirm)).on_press(msg(Msg::ConfirmBlock)),
            ]
            .spacing(space::MD)
            .into(),
        );
    } else {
        parts.push(
            button::secondary(t, tr.t(Key::BoardBlock))
                .icon(Icon::Pause)
                .small()
                .on_press(msg(Msg::StartBlock))
                .into(),
        );
    }
    if let Some(error) = &dialog.error {
        parts.push(
            text(error.as_str())
                .size(font_size::BODY)
                .style(style::text::danger(t))
                .into(),
        );
    }
    column(parts).spacing(space::MD).into()
}

/// The tasks this one waits on, with the way to add and to drop one. A
/// dependency that no longer exists is not listed (and so is not droppable
/// here): the card still says the task is blocked by "a deleted task".
fn dependencies<'a>(v: &View<'a>, dialog: &'a Dialog, task: Option<&'a T>) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let ids: &[String] = match task {
        Some(task) => &task.depends_on,
        None => &dialog.depends_on,
    };
    let own = task.map(|task| task.id.as_str());
    let by_id = index_by_id(v.all);

    let rows = ids
        .iter()
        .filter_map(|dep| by_id.get(dep.as_str()).copied())
        .map(|dep| {
            let done = dep.column == "done";
            let mark = if done {
                icon(Icon::Check, 11.0).color(t.colors.success)
            } else {
                icon(Icon::Lock, 11.0).color(t.colors.danger)
            };
            let title = text(dep.title.as_str())
                .size(font_size::UI)
                .wrapping(text::Wrapping::None)
                .style(look::ink(if done {
                    t.colors.fg_muted
                } else {
                    t.colors.fg
                }))
                .width(Length::Fill);
            let column_name = Column::of(dep)
                .map(|column| tr.t(column.label_key()).into_owned())
                .unwrap_or_default();
            let id = dep.id.clone();
            container(
                row![
                    mark,
                    text(format!("#{}", short_task_id(&dep.id)))
                        .size(font_size::LABEL)
                        .font(fonts::mono())
                        .style(style::text::subtle(t)),
                    title,
                    text(column_name)
                        .size(font_size::CAPTION)
                        .style(style::text::subtle(t)),
                    button::icon_button(t, Icon::Close)
                        .small()
                        .icon_size(10.0)
                        .tip(tr.t(Key::TaskRemoveDependency))
                        .on_press(msg(Msg::RemoveDependency(id))),
                ]
                .spacing(space::MD)
                .align_y(Alignment::Center),
            )
            .padding([0.0, space::XL])
            .center_y(look::FIELD_HEIGHT)
            .into()
        });

    // Tasks that could be added: not this one, not already listed.
    let candidates: Vec<Dependency> = v
        .all
        .iter()
        .filter(|other| Some(other.id.as_str()) != own && !ids.contains(&other.id))
        .map(|other| Dependency {
            id: other.id.clone(),
            label: format!("#{} · {}", short_task_id(&other.id), other.title),
        })
        .collect();

    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    let listed: Vec<Element<'a, Message>> = rows.collect();
    if !listed.is_empty() || !candidates.is_empty() {
        parts.push(label(t, tr.t(Key::TaskDependencies), None));
    }
    if !listed.is_empty() {
        let mut list = column![];
        for (n, listed_row) in listed.into_iter().enumerate() {
            if n > 0 {
                list = list.push(rule::horizontal(1).style(style::rule::hairline(t)));
            }
            list = list.push(listed_row);
        }
        parts.push(container(list).style(look::deps(t)).into());
    }
    if !candidates.is_empty() {
        parts.push(
            pick_list(candidates, None::<Dependency>, |choice| {
                msg(Msg::AddDependency(choice))
            })
            .placeholder(tr.t(Key::TaskAddDependency).into_owned())
            .text_size(font_size::UI)
            .padding([5.0, space::LG])
            .width(Length::Fill)
            .style(style::pick_list::field(t))
            .menu_style(style::pick_list::menu(t))
            .into(),
        );
    }
    column(parts).spacing(space::SM).into()
}

/// The messages between the people and agents working on the task, the
/// newest at the bottom, and the way to add one.
fn thread<'a>(v: &View<'a>, dialog: &'a Dialog, task: &'a T) -> Element<'a, Message> {
    let t = v.t();
    let tr = v.ctx.i18n;
    let heading = label(
        t,
        tr.t(Key::TaskThread),
        Some(tr.t(Key::TaskThreadHint).into_owned()),
    );

    let messages: Element<'a, Message> = if task.comments.is_empty() {
        container(
            text(tr.t(Key::TaskNoComments))
                .size(font_size::UI)
                .style(style::text::subtle(t)),
        )
        .padding(space::XL)
        .width(Length::Fill)
        .style(style::container::well(t))
        .into()
    } else {
        let mut list = column![];
        for (n, entry) in task.comments.iter().enumerate() {
            if n > 0 {
                list = list.push(rule::horizontal(1).style(style::rule::hairline(t)));
            }
            list = list.push(
                container(
                    column![
                        row![
                            text(entry.author.as_str())
                                .size(font_size::UI)
                                .font(fonts::ui_semibold()),
                            Space::new().width(Length::Fill),
                            text(when(entry.created_at, tr.locale(), DateStyle::Short))
                                .size(font_size::LABEL)
                                .font(fonts::mono())
                                .style(style::text::subtle(t)),
                        ]
                        .align_y(Alignment::Center),
                        text(entry.text.as_str())
                            .size(font_size::UI_LG)
                            .wrapping(text::Wrapping::WordOrGlyph),
                    ]
                    .spacing(3.0),
                )
                .padding(Padding {
                    top: 9.0,
                    right: space::XL,
                    bottom: space::LG,
                    left: space::XL,
                }),
            );
        }
        container(
            scrollable(list.width(Length::Fill))
                .id(THREAD)
                .style(style::scrollable::default(t)),
        )
        .max_height(look::THREAD_MAX_HEIGHT)
        .style(style::container::well(t))
        .into()
    };

    let compose = row![
        guarded(
            v,
            field(t, &tr.t(Key::TaskWriteComment), &dialog.comment)
                .on_input(|text| msg(Msg::Comment(text)))
                .on_submit(msg(Msg::Send)),
        ),
        button::secondary(t, tr.t(Key::TaskSend))
            .on_press_maybe((!dialog.comment.trim().is_empty()).then(|| msg(Msg::Send))),
    ]
    .spacing(space::MD);

    column![
        rule::horizontal(1).style(style::rule::hairline(t)),
        Space::new().height(space::XS),
        heading,
        messages,
        compose
    ]
    .spacing(space::MD)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::model::test_task;

    #[test]
    fn splits_the_files_a_task_owns_one_per_line() {
        assert_eq!(
            patterns("  src/**\n\n docs/ \r\nREADME.md"),
            vec!["src/**", "docs/", "README.md"]
        );
        assert!(patterns(" \n ").is_empty());
    }

    #[test]
    fn opens_on_the_task_as_the_board_has_it() {
        let mut task = test_task("a");
        task.title = "Fix it".into();
        task.description = "Why".into();
        task.owns = vec!["src/".into(), "docs/".into()];
        task.priority = 3;
        let dialog = Dialog::open("/repo", Some("a"), &[task]);
        assert_eq!(dialog.title, "Fix it");
        assert_eq!(dialog.priority.value(), 3.0);
        assert_eq!(dialog.seen.owns, "src/\ndocs/");
        assert!(!dialog.is_new());
        assert!(Dialog::open("/repo", None, &[]).is_new());
    }

    #[test]
    fn saves_only_what_changed_and_once() {
        let mut task = test_task("a");
        task.title = "Old".into();
        let mut dialog = Dialog::open("/repo", Some("a"), &[task]);
        assert!(dialog.pending().is_empty());

        dialog.title = "New".into();
        dialog
            .owns
            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                std::sync::Arc::new("src/**".into()),
            )));
        let edits = dialog.pending();
        assert_eq!(
            edits,
            vec![Edit::Title("New".into()), Edit::Owns(vec!["src/**".into()])]
        );
        // Saved: nothing left to save.
        assert!(dialog.pending().is_empty());
    }

    #[test]
    fn a_change_that_is_not_ours_replaces_what_is_typed() {
        let task = test_task("a");
        let mut dialog = Dialog::open("/repo", Some("a"), std::slice::from_ref(&task));
        dialog.title = "half typed".into();

        let mut renamed = task.clone();
        renamed.title = "Renamed by an agent".into();
        dialog.follow(&renamed);
        assert_eq!(dialog.title, "Renamed by an agent");

        // Nothing changed on the board: what is typed stays.
        dialog.title = "typing again".into();
        dialog.follow(&renamed);
        assert_eq!(dialog.title, "typing again");
    }

    #[test]
    fn follows_the_thread_as_it_grows() {
        let task = test_task("a");
        let mut dialog = Dialog::open("/repo", Some("a"), std::slice::from_ref(&task));
        assert!(!dialog.follow(&task));
        let mut talked = task;
        talked.comments.push(hangar_core::board::Comment {
            id: "c".into(),
            author: "Ava".into(),
            text: "hi".into(),
            created_at: 1,
        });
        assert!(dialog.follow(&talked));
        assert!(!dialog.follow(&talked));
    }

    #[test]
    fn a_new_task_is_drafted_from_the_form() {
        let mut dialog = Dialog::open("/repo", None, &[]);
        dialog.title = "  Ship  ".into();
        dialog.column = Column::Doing;
        dialog.depends_on = vec!["x".into()];
        let draft = dialog.draft();
        assert_eq!(draft.title, "Ship");
        assert_eq!(draft.column, Column::Doing);
        assert_eq!(draft.priority, 1);
        assert_eq!(draft.depends_on, vec!["x".to_owned()]);
        // Nothing is saved against a task that does not exist yet.
        assert!(dialog.pending().is_empty());
    }

    #[test]
    fn a_block_settles_the_forms_and_a_refusal_stays_on_screen() {
        let mut st = State::default();
        let mut dialog = Dialog::open("/repo", Some("a"), &[test_task("a")]);
        dialog.blocking = true;
        dialog.reason = "release".into();
        st.dialog = Some(dialog);

        let refused: Result<(), String> = Err("it is a refusal".into());
        let _ = mutated(&mut st, Errors::ToDialog, After::Nothing, &refused);
        let open = st.dialog.as_ref().expect("still open");
        assert_eq!(open.error.as_deref(), Some("it is a refusal"));
        assert!(open.blocking);

        let _ = mutated(&mut st, Errors::ToDialog, After::Nothing, &Ok(()));
        let open = st.dialog.as_ref().expect("still open");
        assert!(open.error.is_none() && !open.blocking && open.reason.is_empty());
    }

    // ---- What the buttons do -----------------------------------------------

    use crate::board::snapshot::{CWD, model, tasks};
    use hangar_i18n::Locale;

    const DOING: &str = "3d4e0000-aaaa-bbbb-cccc-dddddddddddd";

    /// A dialog open on a task of the sample board, and the model it is on.
    fn open_on(id: Option<&str>) -> (State, Model) {
        let m = model(Locale::En, "graphite", Some(tasks()));
        let st = State {
            dialog: Some(Dialog::open(
                CWD,
                id,
                m.live.board.tasks(CWD).unwrap_or(&[]),
            )),
            ..State::default()
        };
        (st, m)
    }

    fn send(st: &mut State, m: &mut Model, msg: Msg) -> usize {
        update(st, m, msg).units()
    }

    fn dialog(st: &State) -> &Dialog {
        st.dialog.as_ref().expect("an open dialog")
    }

    #[test]
    fn a_block_needs_a_reason_and_a_day_that_is_not_past() {
        let (mut st, mut m) = open_on(Some(DOING));
        send(&mut st, &mut m, Msg::StartBlock);
        assert!(dialog(&st).blocking);

        // Nothing said: the form says what is missing, and writes nothing.
        send(&mut st, &mut m, Msg::Reason("   ".into()));
        assert_eq!(send(&mut st, &mut m, Msg::ConfirmBlock), 0);
        assert_eq!(dialog(&st).error.as_deref(), Some("Say what it waits for."));

        send(&mut st, &mut m, Msg::Reason("the npm release".into()));
        send(&mut st, &mut m, Msg::Recheck("2000-01-01".into()));
        assert_eq!(send(&mut st, &mut m, Msg::ConfirmBlock), 0);
        assert_eq!(dialog(&st).error.as_deref(), Some("That day has passed."));

        send(&mut st, &mut m, Msg::Recheck("next week".into()));
        assert_eq!(send(&mut st, &mut m, Msg::ConfirmBlock), 0);
        assert_eq!(
            dialog(&st).error.as_deref(),
            Some("Write the day as YYYY-MM-DD.")
        );

        // A good one goes to the board, and the error is gone.
        send(&mut st, &mut m, Msg::Recheck("2999-01-01".into()));
        assert_eq!(send(&mut st, &mut m, Msg::ConfirmBlock), 1);
        assert!(dialog(&st).error.is_none());
    }

    #[test]
    fn the_reason_stops_at_what_the_board_keeps() {
        let (mut st, mut m) = open_on(Some(DOING));
        send(
            &mut st,
            &mut m,
            Msg::Reason("x".repeat(MAX_BLOCK_REASON + 40)),
        );
        assert_eq!(dialog(&st).reason.chars().count(), MAX_BLOCK_REASON);
    }

    #[test]
    fn cancelling_a_block_closes_the_form_and_its_error() {
        let (mut st, mut m) = open_on(Some(DOING));
        send(&mut st, &mut m, Msg::StartBlock);
        send(&mut st, &mut m, Msg::ConfirmBlock);
        assert!(dialog(&st).error.is_some());
        send(&mut st, &mut m, Msg::CancelBlock);
        assert!(!dialog(&st).blocking && dialog(&st).error.is_none());
    }

    #[test]
    fn a_comment_is_sent_trimmed_and_a_blank_one_is_not() {
        let (mut st, mut m) = open_on(Some(DOING));
        send(&mut st, &mut m, Msg::Comment("   ".into()));
        assert_eq!(send(&mut st, &mut m, Msg::Send), 0);

        send(&mut st, &mut m, Msg::Comment("  a word ".into()));
        assert_eq!(send(&mut st, &mut m, Msg::Send), 1);
        assert!(dialog(&st).comment.is_empty());
    }

    #[test]
    fn delete_asks_twice() {
        let (mut st, mut m) = open_on(Some(DOING));
        assert_eq!(send(&mut st, &mut m, Msg::AskDelete), 0);
        assert!(dialog(&st).confirm_delete);
        send(&mut st, &mut m, Msg::CancelDelete);
        assert!(!dialog(&st).confirm_delete);
        send(&mut st, &mut m, Msg::AskDelete);
        assert_eq!(send(&mut st, &mut m, Msg::ConfirmDelete), 1);
    }

    #[test]
    fn leaving_a_field_saves_it_and_typing_in_it_does_not() {
        let (mut st, mut m) = open_on(Some(DOING));
        assert_eq!(send(&mut st, &mut m, Msg::Title("Renamed".into())), 0);
        // Using the release button leaves the title field: it is saved,
        // then the release goes.
        assert_eq!(send(&mut st, &mut m, Msg::Release), 2);
        // Nothing is left to save.
        assert_eq!(send(&mut st, &mut m, Msg::Release), 1);
    }

    #[test]
    fn the_priority_stepper_saves_each_step() {
        let (mut st, mut m) = open_on(Some(DOING));
        assert_eq!(send(&mut st, &mut m, Msg::Priority(NumberMsg::Step(1))), 1);
        assert_eq!(dialog(&st).priority.value(), 2.0);
        // Typing half a number saves nothing until it reads as one.
        assert_eq!(
            send(
                &mut st,
                &mut m,
                Msg::Priority(NumberMsg::Input(String::new()))
            ),
            0
        );
    }

    #[test]
    fn writing_a_task_changes_the_form_not_the_board() {
        let (mut st, mut m) = open_on(None);
        assert_eq!(send(&mut st, &mut m, Msg::Column(Column::Review)), 0);
        assert_eq!(dialog(&st).column, Column::Review);

        let other = Dependency {
            id: "x".into(),
            label: "#x".into(),
        };
        send(&mut st, &mut m, Msg::AddDependency(other.clone()));
        send(&mut st, &mut m, Msg::AddDependency(other));
        assert_eq!(dialog(&st).depends_on, vec!["x".to_owned()]);
        send(&mut st, &mut m, Msg::RemoveDependency("x".into()));
        assert!(dialog(&st).depends_on.is_empty());

        // No title: no task. With one: it goes to the board.
        assert_eq!(send(&mut st, &mut m, Msg::Create), 0);
        send(&mut st, &mut m, Msg::Title("A new one".into()));
        assert_eq!(send(&mut st, &mut m, Msg::Create), 1);
    }

    #[test]
    fn a_dependency_of_an_open_task_goes_to_the_board() {
        let (mut st, mut m) = open_on(Some(DOING));
        let dep = Dependency {
            id: "5b3c0000-aaaa-bbbb-cccc-dddddddddddd".into(),
            label: "#5b3c".into(),
        };
        assert_eq!(send(&mut st, &mut m, Msg::AddDependency(dep)), 1);
        assert_eq!(
            send(&mut st, &mut m, Msg::RemoveDependency("gone".into())),
            1
        );
    }

    #[test]
    fn closing_the_dialog_asks_the_shell_to_close_the_overlay() {
        let (mut st, mut m) = open_on(Some(DOING));
        assert_eq!(send(&mut st, &mut m, Msg::Close), 1);
    }

    #[test]
    fn a_finished_write_closes_the_dialog_when_it_was_asked_to() {
        let (mut st, _) = open_on(None);
        // A refusal of a new task stays on screen, with its reason.
        let refused: Result<(), String> = Err("no".into());
        assert_eq!(
            mutated(&mut st, Errors::ToDialog, After::CloseOnOk, &refused).units(),
            0
        );
        assert_eq!(dialog(&st).error.as_deref(), Some("no"));
        assert_eq!(
            mutated(&mut st, Errors::ToDialog, After::CloseOnOk, &Ok(())).units(),
            1
        );
        // A delete closes it whatever the board answered.
        assert_eq!(
            mutated(&mut st, Errors::Swallow, After::CloseAlways, &refused).units(),
            1
        );
    }
}
