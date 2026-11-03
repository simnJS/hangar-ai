//! P12: Board.
//!
//! Ports Board*.tsx, TaskDialog.tsx,
//! lib/{board,boardDrag,boardFormat,paneTask}.ts. Read inventory-frontend.md
//! §2.3/1 and crates/hangar-app/PACKAGES.md.
//!
//! What other packages read is [`Live`] (the boards cache, `cache.rs`) and the
//! pure helpers of `model`, `format` and `pane_task`. What this package draws
//! is the board of the active workspace, full (`view`) or in the dock beside
//! the terminals, and the task dialog (`task_dialog`), an overlay.
//!
//! # How a frame comes about
//!
//! `update` is the only place that writes: a change to a task goes to
//! hangar-core off the UI thread (`actions`), and the board is read again
//! after it, so what is drawn is always what the file says. The cards and
//! columns are drawn from `Live` by `view`, `card` and `filter`; `look` has
//! the surfaces, `hit` the one custom widget (a surface pressed with its
//! position, which the drag and the dock's grip need).

pub mod actions;
pub mod agent_pane;
pub mod cache;
mod card;
mod dock;
pub mod drag;
mod filter;
pub mod format;
mod hit;
mod look;
pub mod model;
pub mod pane_task;
mod select;
#[cfg(test)]
mod snapshot;
pub mod task_dialog;
mod view;

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use hangar_i18n::{Key, Translator, args};
use iced::widget::{Id, Space};
use iced::{Element, Point, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{BoardVariant, Overlay, Region, View};
use crate::state::{Model, Patch, Settings, StoreAction};

use actions::{After, Draft, Edit, Errors, Mutation};
pub use cache::Live;
// `model::Task` is not re-exported here: this module already names iced's.
#[allow(unused_imports)]
pub use model::{Blocked, Column, TaskComment};
use model::{Task as BoardTask, blocked_move_refusal};
use select::Filters;

/// Which filter of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Label,
    Assignee,
    State,
}

/// What the pointer does while a card or the dock's grip is held: the board
/// follows it from the window's events, since it may leave the widget that
/// took the press.
#[derive(Debug, Clone, Copy)]
pub enum Pointer {
    Moved(Point),
    Released,
    /// Escape, or the window losing the focus.
    Cancelled,
}

/// The package's UI state; lives in `App.pkg.board`.
/// Owns region `main` in the board view and region `dock`.
#[derive(Debug)]
pub struct State {
    filters: Filters,
    open_filter: Option<FilterKind>,
    /// What is typed in the add field of each column, not made a card yet.
    drafts: HashMap<Column, String>,
    /// The column whose add field was asked for.
    adding: Option<Column>,
    /// The dock's folded sections. Finished work, and work nobody started,
    /// are the least useful things to keep in view next to the agents.
    folded: Vec<Column>,
    /// Dock only: the filter row is one click away rather than always there.
    filter_bar: bool,
    /// Asked for a first task from the empty board: show the columns anyway.
    composing: bool,
    /// Why a move was refused, shown for a few seconds.
    toast: Option<(u64, String)>,
    toasts: u64,
    press: Option<drag::Press>,
    grip: Option<dock::Grip>,
    dialog: Option<task_dialog::Dialog>,
    /// What the add field had when "with details" was asked for.
    seed: Option<(Column, String)>,
}

impl Default for State {
    fn default() -> Self {
        State {
            filters: Filters::default(),
            open_filter: None,
            drafts: HashMap::new(),
            adding: None,
            folded: vec![Column::Todo, Column::Done],
            filter_bar: false,
            composing: false,
            toast: None,
            toasts: 0,
            press: None,
            grip: None,
            dialog: None,
            seed: None,
        }
    }
}

/// The package's messages, routed by the shell as `Message::Board(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    // Reading
    /// The folders of the workspaces, which the cache keeps the boards of.
    Follow(Vec<String>),
    Loaded(String, Option<Arc<Vec<BoardTask>>>),
    /// A recheck day may have passed: read again.
    Reload,
    /// The clock of cards in progress ticked: nothing to do but draw.
    Redraw,

    // Writing
    Mutated {
        cwd: String,
        errors: Errors,
        after: After,
        result: Result<(), String>,
    },

    // The bars
    OpenFilter(Option<FilterKind>),
    Filter(FilterKind, String),
    ClearFilters,
    ToggleFilterBar,
    OpenMcp,
    Expand,
    HideDock,

    // Columns
    StartAdding(Column),
    FocusAdd(Column),
    FocusField(&'static str),
    Draft(Column, String),
    Submit(Column),
    ClearDraft(Column),
    AddDetails(Column),
    Fold(Column),

    // Cards
    Press(String, Point),
    Pointer(Pointer),
    Enter(Column),
    Leave(Column),
    OpenTask(String),
    FocusAgent {
        ws: String,
        pane: String,
        from_dialog: bool,
    },
    ToastGone(u64),

    // The dock's grip
    GripPress {
        at: Point,
        width: f32,
        max: f32,
    },
    GripReset,
    GripKey {
        widen: bool,
    },

    Dialog(task_dialog::Msg),
}

impl Msg {
    /// Whether it is a press somewhere else than the add field a card is being
    /// typed in. The web app made the card when the field lost the keyboard;
    /// iced's fields have no such event, so this is when it is made.
    fn leaves_the_add_field(&self) -> bool {
        matches!(
            self,
            Msg::Press(..)
                | Msg::OpenTask(_)
                | Msg::Fold(_)
                | Msg::OpenFilter(_)
                | Msg::Filter(..)
                | Msg::ClearFilters
                | Msg::ToggleFilterBar
                | Msg::OpenMcp
                | Msg::Expand
                | Msg::HideDock
                | Msg::StartAdding(_)
                | Msg::Draft(..)
                | Msg::GripPress { .. }
                | Msg::Dialog(_)
        )
    }

    /// The column whose draft this message is about, which it leaves be.
    fn keeps_draft_of(&self) -> Option<Column> {
        match self {
            Msg::StartAdding(column) | Msg::Draft(column, _) => Some(*column),
            _ => None,
        }
    }
}

/// The identity of a column's add field, for the keyboard to go to.
fn add_id(column: Column) -> Id {
    Id::from(format!("board-add-{}", column.id()))
}

/// The identity of a task's card.
fn card_id(task: &str) -> Id {
    Id::from(format!("board-card-{task}"))
}

/// The folder of the board the window shows.
fn active_cwd(m: &Model) -> Option<String> {
    m.active_workspace().map(|ws| ws.cwd.clone())
}

/// Whether a board is on screen: the full view, or the dock beside the
/// terminals.
fn board_shown(m: &Model) -> bool {
    match m.ui.view {
        View::Board => true,
        View::Terminals => m.settings().board_dock_open,
        _ => false,
    }
}

fn task_named<'a>(m: &'a Model, cwd: &str, id: &str) -> Option<&'a BoardTask> {
    m.live
        .board
        .tasks(cwd)?
        .iter()
        .find(|candidate| candidate.id == id)
}

/// Reads every board that is not being read already.
fn read_all(m: &mut Model) -> Task<Message> {
    let folders = m.live.board.request_all();
    Task::batch(folders.iter().map(|cwd| actions::read(cwd)))
}

/// Reads one board, unless a read of it is out (it will repeat).
fn read_one(m: &mut Model, cwd: &str) -> Task<Message> {
    if m.live.board.request(cwd) {
        actions::read(cwd)
    } else {
        Task::none()
    }
}

/// Makes a card of what was typed in the add fields, but the one about to be
/// typed in.
fn flush_drafts(st: &mut State, m: &Model, keep: Option<Column>) -> Task<Message> {
    let Some(cwd) = active_cwd(m) else {
        return Task::none();
    };
    let mut made = Vec::new();
    for column in Column::ALL {
        if Some(column) == keep {
            continue;
        }
        if let Some(draft) = st.drafts.remove(&column) {
            let title = draft.trim();
            if !title.is_empty() {
                made.push(actions::run(
                    &cwd,
                    Mutation::Create(Draft::titled(title, column)),
                    Errors::Swallow,
                    After::Nothing,
                ));
            }
        }
    }
    Task::batch(made)
}

fn toast(st: &mut State, words: String) -> Task<Message> {
    st.toasts += 1;
    let seq = st.toasts;
    st.toast = Some((seq, words));
    Task::perform(
        async { tokio::time::sleep(Duration::from_secs(6)).await },
        move |()| Message::Board(Msg::ToastGone(seq)),
    )
}

/// Moves a card to a column, unless it is blocked: the person has to lift the
/// block first, and is told so.
fn move_task(st: &mut State, m: &Model, id: &str, to: Column) -> Task<Message> {
    let Some(cwd) = active_cwd(m) else {
        return Task::none();
    };
    let Some(task) = task_named(m, &cwd, id) else {
        return Task::none();
    };
    if Column::of(task) == Some(to) {
        return Task::none();
    }
    if let Some(reason) = blocked_move_refusal(task, to) {
        // Never clear the state silently: the task has to be unblocked first.
        let words = Translator::new(m.locale())
            .t_with(
                Key::BoardBlockedStays,
                &args!("title" => task.title.as_str(), "reason" => reason),
            )
            .into_owned();
        return toast(st, words);
    }
    actions::run(
        &cwd,
        Mutation::Update {
            id: id.to_owned(),
            edit: Edit::Column(to),
        },
        Errors::Swallow,
        After::Nothing,
    )
}

fn set_dock_width(m: &mut Model, width: f32) {
    m.apply(StoreAction::UpdateSettings(Patch::new(
        move |settings: &mut Settings| settings.board_dock_width = f64::from(width),
    )));
}

/// The widest the dock may be in the window that has the focus.
fn dock_room(m: &Model) -> f32 {
    let window =
        m.ui.focused_window
            .and_then(|window| m.ui.body_width.get(&window))
            .copied()
            .unwrap_or(crate::shell::windows::MAIN_SIZE.width);
    dock::room(m, window)
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let abandoned = if msg.leaves_the_add_field() {
        flush_drafts(st, m, msg.keeps_draft_of())
    } else {
        Task::none()
    };
    Task::batch([abandoned, handle(st, m, msg)])
}

fn handle(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    match msg {
        // ---- Reading ----------------------------------------------------
        Msg::Follow(folders) => {
            let fresh = m.live.board.follow(&folders);
            m.live.version += 1;
            Task::batch(fresh.iter().map(|cwd| read_one(m, cwd)))
        }
        Msg::Loaded(cwd, tasks) => {
            let again = m.live.board.answer(&cwd, tasks);
            m.live.version += 1;
            let repeat = if again {
                actions::read(&cwd)
            } else {
                Task::none()
            };
            Task::batch([repeat, follow_dialog(st, m, &cwd)])
        }
        Msg::Reload => read_all(m),
        Msg::Redraw => Task::none(),

        // ---- Writing ----------------------------------------------------
        Msg::Mutated {
            cwd,
            errors,
            after,
            result,
        } => {
            let dialog = task_dialog::mutated(st, errors, after, &result);
            Task::batch([dialog, read_one(m, &cwd)])
        }

        // ---- The bars ---------------------------------------------------
        Msg::OpenFilter(kind) => {
            st.open_filter = match kind {
                Some(kind) if st.open_filter != Some(kind) => Some(kind),
                _ => None,
            };
            Task::none()
        }
        Msg::Filter(kind, value) => {
            match kind {
                FilterKind::Label => st.filters.label = value,
                FilterKind::Assignee => st.filters.assignee = value,
                FilterKind::State => st.filters.state = value,
            }
            st.open_filter = None;
            Task::none()
        }
        Msg::ClearFilters => {
            st.filters.clear();
            Task::none()
        }
        Msg::ToggleFilterBar => {
            st.filter_bar = !st.filter_bar;
            Task::none()
        }
        Msg::OpenMcp => Task::done(Message::Intent(Intent::OpenMcpInstall)),
        Msg::Expand => Task::done(Message::Intent(Intent::SetView(View::Board))),
        Msg::HideDock => Task::done(Message::Intent(Intent::CloseDock)),

        // ---- Columns ----------------------------------------------------
        Msg::StartAdding(column) => {
            // Puts the cursor in a column's add field, unfolding it if need be.
            st.composing = true;
            st.adding = Some(column);
            st.folded.retain(|folded| *folded != column);
            // The field is on screen after this message's view.
            Task::perform(
                async { tokio::time::sleep(Duration::from_millis(30)).await },
                move |()| Message::Board(Msg::FocusAdd(column)),
            )
        }
        Msg::FocusAdd(column) => iced::widget::operation::focus(add_id(column)),
        Msg::FocusField(id) => iced::widget::operation::focus(id),
        Msg::Draft(column, text) => {
            st.drafts.insert(column, text);
            Task::none()
        }
        Msg::Submit(column) => {
            let title = st.drafts.remove(&column).unwrap_or_default();
            st.adding = None;
            let (Some(cwd), false) = (active_cwd(m), title.trim().is_empty()) else {
                return Task::none();
            };
            actions::run(
                &cwd,
                Mutation::Create(Draft::titled(&title, column)),
                Errors::Swallow,
                After::Nothing,
            )
        }
        Msg::ClearDraft(column) => {
            st.drafts.remove(&column);
            Task::none()
        }
        Msg::AddDetails(column) => {
            let title = st.drafts.remove(&column).unwrap_or_default();
            st.adding = None;
            let Some(cwd) = active_cwd(m) else {
                return Task::none();
            };
            st.seed = Some((column, title));
            Task::done(Message::Intent(Intent::OpenTask { cwd, task: None }))
        }
        Msg::Fold(column) => {
            if st.folded.contains(&column) {
                st.folded.retain(|folded| *folded != column);
            } else {
                st.folded.push(column);
            }
            Task::none()
        }

        // ---- Cards ------------------------------------------------------
        Msg::Press(id, at) => {
            let Some(cwd) = active_cwd(m) else {
                return Task::none();
            };
            let Some(column) = task_named(m, &cwd, &id).and_then(Column::of) else {
                return Task::none();
            };
            // A new press ends any drag in progress.
            st.press = Some(drag::Press::new(id, column, at));
            st.adding = None;
            Task::none()
        }
        Msg::Pointer(pointer) => pointer_event(st, m, pointer),
        Msg::Enter(column) => {
            if let Some(press) = &mut st.press {
                press.enter(column);
            }
            Task::none()
        }
        Msg::Leave(column) => {
            if let Some(press) = &mut st.press {
                press.leave(column);
            }
            Task::none()
        }
        Msg::OpenTask(id) => match active_cwd(m) {
            Some(cwd) => Task::done(Message::Intent(Intent::OpenTask {
                cwd,
                task: Some(id),
            })),
            None => Task::none(),
        },
        Msg::FocusAgent {
            ws,
            pane,
            from_dialog,
        } => {
            let go = Task::done(Message::Intent(Intent::FocusPane {
                ws,
                pane,
                terminal: true,
            }));
            if from_dialog {
                Task::batch([Task::done(Message::Intent(Intent::CloseOverlay)), go])
            } else {
                go
            }
        }
        Msg::ToastGone(seq) => {
            if st.toast.as_ref().is_some_and(|(shown, _)| *shown == seq) {
                st.toast = None;
            }
            Task::none()
        }

        // ---- The dock's grip -------------------------------------------
        Msg::GripPress { at, width, max } => {
            st.grip = Some(dock::Grip::new(at, width, max));
            Task::none()
        }
        Msg::GripReset => {
            st.grip = None;
            set_dock_width(m, dock::default_width());
            Task::none()
        }
        Msg::GripKey { widen } => {
            let width = m.settings().board_dock_width as f32;
            set_dock_width(m, dock::key_width(width, dock_room(m), widen));
            Task::none()
        }

        Msg::Dialog(inner) => task_dialog::update(st, m, inner),
    }
}

/// The pointer, while a card or the grip is held.
fn pointer_event(st: &mut State, m: &mut Model, pointer: Pointer) -> Task<Message> {
    if let Some(grip) = &st.grip {
        match pointer {
            Pointer::Moved(at) => set_dock_width(m, grip.width_at(at.x)),
            Pointer::Released | Pointer::Cancelled => st.grip = None,
        }
        return Task::none();
    }
    match pointer {
        Pointer::Moved(at) => {
            if let Some(press) = &mut st.press {
                press.pointer_moved(at);
            }
            Task::none()
        }
        Pointer::Cancelled => {
            st.press = None;
            Task::none()
        }
        Pointer::Released => {
            let Some(press) = st.press.take() else {
                return Task::none();
            };
            match press.release() {
                drag::Ended::Click(id) => handle(st, m, Msg::OpenTask(id)),
                drag::Ended::Drop { task, to } => move_task(st, m, &task, to),
                drag::Ended::Nothing => Task::none(),
            }
        }
    }
}

/// A board was read: the dialog open on it follows what the board says now,
/// or is done with if the task is gone.
fn follow_dialog(st: &mut State, m: &Model, cwd: &str) -> Task<Message> {
    let Some(dialog) = st.dialog.as_mut().filter(|dialog| dialog.cwd == cwd) else {
        return Task::none();
    };
    let Some(id) = dialog.task.clone() else {
        return Task::none();
    };
    match task_named(m, cwd, &id) {
        Some(task) => {
            if dialog.follow(task) {
                iced::widget::operation::snap_to_end("board-task-thread")
            } else {
                Task::none()
            }
        }
        // An agent deleted it: nothing is left to show.
        None => {
            st.dialog = None;
            Task::done(Message::Intent(Intent::CloseOverlay))
        }
    }
}

/// The window's events while a card or the grip is held.
fn pointer(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    use iced::keyboard::{self, Key, key::Named};
    use iced::{Event, mouse, window};
    let pointer = match event {
        Event::Mouse(mouse::Event::CursorMoved { position }) => Pointer::Moved(position),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Pointer::Released,
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        })
        | Event::Window(window::Event::Unfocused) => Pointer::Cancelled,
        _ => return None,
    };
    Some(Message::Board(Msg::Pointer(pointer)))
}

/// The folders to hold the boards of: a subscription of its own, restarted
/// whenever a workspace comes or goes, which says so once.
#[derive(Hash)]
struct Folders(Vec<String>);

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut all = vec![Subscription::run_with(
        Folders(cache::folders_of(m.state())),
        |folders| {
            let folders = folders.0.clone();
            iced::futures::stream::once(async move { Message::Board(Msg::Follow(folders)) })
        },
    )];
    // A recheck day that passes unblocks its task when the board is next
    // read (board.rs): reading again now and then is what shows that.
    if m.live.board.has_rechecks() {
        all.push(iced::time::every(Duration::from_secs(60)).map(|_| Message::Board(Msg::Reload)));
    }
    // Cards in progress carry a clock, drawn to the minute.
    if board_shown(m) && clock_running(m) {
        all.push(iced::time::every(Duration::from_secs(30)).map(|_| Message::Board(Msg::Redraw)));
    }
    if st.press.is_some() || st.grip.is_some() {
        all.push(iced::event::listen_with(pointer));
    }
    Subscription::batch(all)
}

/// Whether a task of the board on screen is in progress with a clock.
fn clock_running(m: &Model) -> bool {
    let Some(workspace) = m.active_workspace() else {
        return false;
    };
    m.live.board.tasks(&workspace.cwd).is_some_and(|tasks| {
        tasks
            .iter()
            .any(|task| task.column == "doing" && task.doing_since.is_some())
    })
}

/// What takes the focus when F6 reaches region `Main` (the board view) or
/// `Dock`: the first card, else the first column's add field.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    if !matches!(region, Region::Main | Region::Dock) {
        return None;
    }
    let cwd = active_cwd(m)?;
    let tasks = m.live.board.tasks(&cwd)?;
    if tasks.is_empty() && !st.composing {
        return None;
    }
    let dock = region == Region::Dock;
    let order = if dock {
        Column::DOCK_ORDER
    } else {
        Column::ALL
    };
    let shown = select::shown(tasks, &st.filters);
    for column in order {
        if dock && st.folded.contains(&column) {
            continue;
        }
        if let Some(first) = select::column_items(&shown, column).first() {
            return Some(iced::widget::operation::focus(card_id(&first.id)));
        }
    }
    Some(iced::widget::operation::focus(add_id(order[0])))
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        Intent::OpenTask { cwd, task } | Intent::OpenOverlay(Overlay::Task { cwd, task }) => {
            if task.is_none()
                && let Some((column, title)) = st.seed.take()
                && st.dialog.is_none()
            {
                // The add field's text and column go to the form.
                let opened = task_dialog::opened(st, m, cwd, None);
                if let Some(dialog) = &mut st.dialog {
                    dialog.seed(column, title);
                }
                return Some(opened);
            }
            Some(task_dialog::opened(st, m, cwd, task.as_deref()))
        }
        // The dialog is closed by the shell (Escape, its scrim) or by itself;
        // either way what was typed is saved, and the keyboard goes back.
        Intent::CloseOverlay | Intent::ClosePopovers(_)
            if st.dialog.is_some()
                && !m
                    .ui
                    .overlays
                    .iter()
                    .any(|overlay| matches!(overlay, Overlay::Task { .. })) =>
        {
            Some(task_dialog::closed(st, m))
        }
        // The board leaves the screen: a press, a menu and a message about a
        // move do not outlive it.
        Intent::SetView(_) | Intent::OpenBoard(_) | Intent::ToggleDock | Intent::CloseDock => {
            st.open_filter = None;
            st.press = None;
            st.grip = None;
            st.adding = None;
            st.composing = false;
            st.toast = None;
            None
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        // Agents mutate the board through the HTTP API; core says so, and the
        // boards are read again. The payload names the caller's folder, which
        // is not compared against ours: worktree sharing makes several paths
        // alias one board, so an agent writing from a linked worktree carries
        // a path this window has never heard of.
        CoreEvent::BoardChanged { .. } => Some(read_all(m)),
        _ => None,
    }
}

/// Region `main` in the board view (`BoardVariant::Full`), or region `dock`
/// beside the terminals (`BoardVariant::Dock`).
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, variant: BoardVariant) -> Element<'a, Message> {
    let Some(workspace) = ctx.model.active_workspace() else {
        return Space::new().into();
    };
    match variant {
        BoardVariant::Full => view::board(st, ctx, &workspace.cwd, None),
        BoardVariant::Dock { width, .. } => {
            let max = dock::room(ctx.model, ctx.metrics.body_width);
            view::board(st, ctx, &workspace.cwd, Some((width, max)))
        }
    }
}

/// `Overlay::Task`: a board task, new (`task` is `None`) or opened.
pub fn task_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    cwd: &'a str,
    task: Option<&'a str>,
) -> Element<'a, Message> {
    task_dialog::view(st, ctx, cwd, task)
}
