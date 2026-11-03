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
pub mod cache;
mod card;
mod dock;
pub mod drag;
mod filter;
pub mod format;
mod hit;
#[cfg(test)]
mod interaction;
mod look;
pub mod model;
pub mod pane_task;
mod select;
#[cfg(test)]
mod snapshot;
pub mod task_dialog;
mod view;

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use hangar_i18n::{Key, Translator, args};
use iced::widget::{Id, Space};
use iced::{Element, Point, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{BoardVariant, Drawer, Overlay, Panels, Region, View};
use crate::state::{Model, PanelDrag, Patch, Settings, StoreAction, Workspace};

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
    /// What is typed in the add field of each column of each board, not made a
    /// card yet. Keyed by the board's folder as well as the column: two
    /// windows can draw two boards at once, and a draft belongs to the board
    /// it was typed on.
    drafts: HashMap<(String, Column), String>,
    /// The board and column whose add field was asked for.
    adding: Option<(String, Column)>,
    /// The dock's folded sections. Finished work, and work nobody started,
    /// are the least useful things to keep in view next to the agents.
    folded: Vec<Column>,
    /// Dock only: the filter row is one click away rather than always there.
    filter_bar: bool,
    /// The boards (by folder) a first task was asked for on, from the empty
    /// board: they show the columns anyway.
    composing: HashSet<String>,
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
            composing: HashSet::new(),
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

    // Columns. Every message that writes, or reads what is written, carries the
    // folder of the board it is about: the window that sent it draws the
    // board of its own workspace, which is not the main window's active one
    // when a workspace is detached.
    StartAdding(String, Column),
    FocusAdd(Column),
    FocusField(&'static str),
    Draft(String, Column, String),
    Submit(String, Column),
    ClearDraft(String, Column),
    AddDetails(String, Column),
    Fold(Column),

    // Cards
    /// `(cwd, task, position)`.
    Press(String, String, Point),
    Pointer(Pointer),
    Enter(Column),
    Leave(Column),
    /// `(cwd, task)`.
    OpenTask(String, String),
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
    /// An arrow key on the grip, with the widest the window allows (the
    /// window that has the grip knows it; the message carries it).
    GripKey {
        widen: bool,
        width: f32,
        max: f32,
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
                | Msg::OpenTask(..)
                | Msg::Fold(_)
                | Msg::OpenFilter(_)
                | Msg::Filter(..)
                | Msg::ClearFilters
                | Msg::ToggleFilterBar
                | Msg::OpenMcp
                | Msg::Expand
                | Msg::HideDock
                | Msg::StartAdding(..)
                | Msg::Draft(..)
                | Msg::GripPress { .. }
                | Msg::Dialog(_)
        )
    }

    /// The column whose draft this message is about, which it leaves be.
    fn keeps_draft_of(&self) -> Option<(&str, Column)> {
        match self {
            Msg::StartAdding(cwd, column) | Msg::Draft(cwd, column, _) => {
                Some((cwd.as_str(), *column))
            }
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

/// The folder of the board the window that has the keyboard shows. Only what
/// has no window of its own to ask (the F6 landing) reads this; a message
/// carries the folder of the board it was sent from.
fn focused_cwd(m: &Model) -> Option<String> {
    let workspace = match m.focused_window() {
        Some(window) => m.window_workspace(window),
        None => m.active_workspace().filter(|ws| ws.detached != Some(true)),
    };
    workspace.map(|ws| ws.cwd.clone())
}

/// The workspaces whose board is on screen now, one per window that draws
/// one: the full view, or the dock (beside the terminals or as an open
/// drawer), as the shell placed it for that window.
fn boards_on_screen(m: &Model) -> Vec<&Workspace> {
    if m.live.windows.is_empty() {
        // Before the first window opens, the one the model describes.
        let shown = match m.ui.view {
            View::Board => true,
            View::Terminals => m.settings().board_dock_open,
            _ => false,
        };
        return m.active_workspace().filter(|_| shown).into_iter().collect();
    }
    m.live
        .windows
        .keys()
        .filter(|window| m.view_in(**window) == View::Board || Panels::of(m, **window).dock.shown)
        .filter_map(|window| m.window_workspace(*window))
        .collect()
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
fn flush_drafts(st: &mut State, keep: Option<(&str, Column)>) -> Task<Message> {
    // Each draft is made on the board it was typed on, which may not be the
    // one the message that abandoned it is about.
    let mut abandoned: Vec<(String, Column)> = st
        .drafts
        .keys()
        .filter(|(cwd, column)| keep != Some((cwd.as_str(), *column)))
        .cloned()
        .collect();
    let rank = |column: &Column| Column::ALL.iter().position(|c| c == column);
    abandoned.sort_by(|a, b| a.0.cmp(&b.0).then(rank(&a.1).cmp(&rank(&b.1))));
    let mut made = Vec::new();
    for (cwd, column) in abandoned {
        let Some(draft) = st.drafts.remove(&(cwd.clone(), column)) else {
            continue;
        };
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

/// Moves a card of the board in `cwd` to a column, unless it is blocked: the
/// person has to lift the block first, and is told so.
fn move_task(st: &mut State, m: &Model, cwd: &str, id: &str, to: Column) -> Task<Message> {
    let Some(task) = task_named(m, cwd, id) else {
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
        cwd,
        Mutation::Update {
            id: id.to_owned(),
            edit: Edit::Column(to),
        },
        Errors::Swallow,
        After::Nothing,
    )
}

/// The grip is let go of, or dropped: the dock goes back to being drawn at
/// its setting. Another panel's held grip is not the board's to let go.
fn end_grip(st: &mut State, m: &mut Model) {
    st.grip = None;
    if m.live
        .signals
        .panel_drag
        .is_some_and(|drag| drag.panel == Drawer::Dock)
    {
        m.live.signals.panel_drag = None;
    }
}

fn set_dock_width(m: &mut Model, width: f32) {
    if (m.settings().board_dock_width - f64::from(width)).abs() < 0.5 {
        return;
    }
    m.apply(StoreAction::UpdateSettings(Patch::new(
        move |settings: &mut Settings| settings.board_dock_width = f64::from(width),
    )));
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let abandoned = if msg.leaves_the_add_field() {
        // The field that was asked for is left, unless it is the next one.
        if !matches!(msg, Msg::StartAdding(..) | Msg::Draft(..)) {
            st.adding = None;
        }
        flush_drafts(st, msg.keeps_draft_of())
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
        Msg::StartAdding(cwd, column) => {
            // Puts the cursor in a column's add field, unfolding it if need be.
            st.composing.insert(cwd.clone());
            st.adding = Some((cwd, column));
            st.folded.retain(|folded| *folded != column);
            // The field is on screen after this message's view.
            Task::perform(
                async { tokio::time::sleep(Duration::from_millis(30)).await },
                move |()| Message::Board(Msg::FocusAdd(column)),
            )
        }
        Msg::FocusAdd(column) => iced::widget::operation::focus(add_id(column)),
        Msg::FocusField(id) => iced::widget::operation::focus(id),
        Msg::Draft(cwd, column, text) => {
            st.drafts.insert((cwd, column), text);
            Task::none()
        }
        Msg::Submit(cwd, column) => {
            let title = st.drafts.remove(&(cwd.clone(), column)).unwrap_or_default();
            st.adding = None;
            if title.trim().is_empty() {
                return Task::none();
            }
            actions::run(
                &cwd,
                Mutation::Create(Draft::titled(&title, column)),
                Errors::Swallow,
                After::Nothing,
            )
        }
        Msg::ClearDraft(cwd, column) => {
            st.drafts.remove(&(cwd, column));
            Task::none()
        }
        Msg::AddDetails(cwd, column) => {
            let title = st.drafts.remove(&(cwd.clone(), column)).unwrap_or_default();
            st.adding = None;
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
        Msg::Press(cwd, id, at) => {
            let Some(column) = task_named(m, &cwd, &id).and_then(Column::of) else {
                return Task::none();
            };
            // A new press ends any drag in progress.
            st.press = Some(drag::Press::new(id, column, at).on(&cwd));
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
        Msg::OpenTask(cwd, id) => Task::done(Message::Intent(Intent::OpenTask {
            cwd,
            task: Some(id),
        })),
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
            end_grip(st, m);
            set_dock_width(m, dock::default_width());
            Task::none()
        }
        Msg::GripKey { widen, width, max } => {
            set_dock_width(m, dock::key_width(width, max, widen));
            Task::none()
        }

        Msg::Dialog(inner) => task_dialog::update(st, m, inner),
    }
}

/// The pointer, while a card or the grip is held.
fn pointer_event(st: &mut State, m: &mut Model, pointer: Pointer) -> Task<Message> {
    if let Some(grip) = &mut st.grip {
        // The width is saved once, when the pointer is let go, as the TS did:
        // every move saved the settings and republished the snapshot.
        // Meanwhile the frame draws the dock at the width the pointer has
        // reached (`live.signals.panel_drag`, read by `Panels::of`). Escape
        // or the window losing the focus drops it, and the panel keeps the
        // width it had.
        match pointer {
            Pointer::Moved(at) => {
                grip.moved_to(at.x);
                m.live.signals.panel_drag = Some(PanelDrag {
                    panel: Drawer::Dock,
                    width: grip.pending(),
                });
            }
            Pointer::Released => {
                let width = grip.pending();
                end_grip(st, m);
                set_dock_width(m, width);
            }
            Pointer::Cancelled => end_grip(st, m),
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
            // The board the card was pressed on, which may not be the main
            // window's.
            let cwd = press.cwd.clone();
            match press.release() {
                drag::Ended::Click(id) => handle(st, m, Msg::OpenTask(cwd, id)),
                drag::Ended::Drop { task, to } => move_task(st, m, &cwd, &task, to),
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
    if clock_running(m) {
        all.push(iced::time::every(Duration::from_secs(30)).map(|_| Message::Board(Msg::Redraw)));
    }
    if st.press.is_some() || st.grip.is_some() {
        all.push(iced::event::listen_with(pointer));
    }
    Subscription::batch(all)
}

/// Whether a task of a board on screen, in any window, is in progress with a
/// clock.
fn clock_running(m: &Model) -> bool {
    boards_on_screen(m).into_iter().any(|workspace| {
        m.live.board.tasks(&workspace.cwd).is_some_and(|tasks| {
            tasks
                .iter()
                .any(|task| task.column == "doing" && task.doing_since.is_some())
        })
    })
}

/// What takes the focus when F6 reaches region `Main` (the board view) or
/// `Dock`: the first card, else the first column's add field.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    if !matches!(region, Region::Main | Region::Dock) {
        return None;
    }
    let cwd = focused_cwd(m)?;
    let tasks = m.live.board.tasks(&cwd)?;
    if tasks.is_empty() && !st.composing.contains(&cwd) {
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
    react(st, m, intent)
}

fn react(st: &mut State, m: &mut Model, intent: &Intent) -> Option<Task<Message>> {
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
            end_grip(st, m);
            st.adding = None;
            st.composing.clear();
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
    // The workspace of this window: a detached one draws its own board, not
    // the main window's active one.
    let Some(workspace) = ctx.workspace() else {
        return Space::new().into();
    };
    match variant {
        BoardVariant::Full => view::board(st, ctx, &workspace.cwd, None),
        BoardVariant::Dock { width, .. } => {
            // The room the shell placed the panel in, for this window.
            let max = ctx.panels().dock.max;
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

#[cfg(test)]
mod tests {
    use super::snapshot::{CWD, SITE, model, tasks, two_windows};
    use super::*;
    use hangar_i18n::Locale;

    const BLOCKED: &str = "5b3c0000-aaaa-bbbb-cccc-dddddddddddd";
    const FREE: &str = "1a2b0000-aaaa-bbbb-cccc-dddddddddddd";
    const DOING: &str = "3d4e0000-aaaa-bbbb-cccc-dddddddddddd";

    fn setup() -> (State, Model) {
        (
            State::default(),
            model(Locale::En, "graphite", Some(tasks())),
        )
    }

    /// A message whose task nobody needs.
    fn step(st: &mut State, m: &mut Model, msg: Msg) {
        drop(handle(st, m, msg));
    }

    fn drag(st: &mut State, m: &mut Model, id: &str, to: Column) -> Task<Message> {
        step(
            st,
            m,
            Msg::Press(CWD.into(), id.into(), Point::new(10.0, 10.0)),
        );
        step(st, m, Msg::Pointer(Pointer::Moved(Point::new(80.0, 10.0))));
        step(st, m, Msg::Enter(to));
        handle(st, m, Msg::Pointer(Pointer::Released))
    }

    #[test]
    fn what_was_typed_becomes_a_card_when_the_person_goes_elsewhere() {
        let (mut st, _m) = setup();
        st.drafts
            .insert((CWD.into(), Column::Todo), "  Fix the thing ".into());
        st.drafts.insert((CWD.into(), Column::Done), "   ".into());

        // Typing in another column leaves the first one's field.
        let typing = Msg::Draft(CWD.into(), Column::Doing, "x".into());
        assert!(typing.leaves_the_add_field());
        assert_eq!(typing.keeps_draft_of(), Some((CWD, Column::Doing)));
        let made = flush_drafts(&mut st, typing.keeps_draft_of());
        // One card, from the one that had words; both drafts are gone.
        assert_eq!(made.units(), 1);
        assert!(st.drafts.is_empty());
    }

    #[test]
    fn typing_in_a_field_does_not_make_a_card_of_it() {
        let (mut st, _m) = setup();
        st.drafts.insert((CWD.into(), Column::Todo), "Fix".into());
        let made = flush_drafts(&mut st, Some((CWD, Column::Todo)));
        assert_eq!(made.units(), 0);
        assert_eq!(
            st.drafts
                .get(&(CWD.to_owned(), Column::Todo))
                .map(String::as_str),
            Some("Fix")
        );
        assert!(!Msg::Submit(CWD.into(), Column::Todo).leaves_the_add_field());
        assert!(!Msg::Pointer(Pointer::Released).leaves_the_add_field());
    }

    #[test]
    fn submitting_makes_the_card_and_empties_the_field() {
        let (mut st, mut m) = setup();
        st.drafts.insert((CWD.into(), Column::Todo), "Fix".into());
        st.adding = Some((CWD.into(), Column::Todo));
        assert_eq!(
            handle(&mut st, &mut m, Msg::Submit(CWD.into(), Column::Todo)).units(),
            1
        );
        assert!(st.drafts.is_empty() && st.adding.is_none());
        // Nothing typed: nothing made.
        assert_eq!(
            handle(&mut st, &mut m, Msg::Submit(CWD.into(), Column::Todo)).units(),
            0
        );
    }

    #[test]
    fn a_card_dropped_in_another_column_moves() {
        let (mut st, mut m) = setup();
        let after = drag(&mut st, &mut m, FREE, Column::Review);
        assert_eq!(after.units(), 1);
        assert!(st.toast.is_none() && st.press.is_none());
    }

    #[test]
    fn a_blocked_card_dropped_out_of_todo_stays_and_says_why() {
        let (mut st, mut m) = setup();
        drop(drag(&mut st, &mut m, BLOCKED, Column::Doing));
        let (_, words) = st.toast.as_ref().expect("a message");
        assert!(words.contains("Release 1.13"));
        assert!(words.contains("Waiting for the npm release"));
    }

    #[test]
    fn a_blocked_card_dropped_in_todo_is_not_refused() {
        let (mut st, mut m) = setup();
        // Dropped where it already is: nothing to do, nothing to say.
        step(
            &mut st,
            &mut m,
            Msg::Press(CWD.into(), BLOCKED.into(), Point::new(1.0, 1.0)),
        );
        step(
            &mut st,
            &mut m,
            Msg::Pointer(Pointer::Moved(Point::new(90.0, 1.0))),
        );
        step(&mut st, &mut m, Msg::Enter(Column::Todo));
        step(&mut st, &mut m, Msg::Pointer(Pointer::Released));
        assert!(st.toast.is_none());
    }

    #[test]
    fn a_press_that_barely_moves_is_a_click_that_opens_the_task() {
        let (mut st, mut m) = setup();
        step(
            &mut st,
            &mut m,
            Msg::Press(CWD.into(), DOING.into(), Point::new(5.0, 5.0)),
        );
        step(
            &mut st,
            &mut m,
            Msg::Pointer(Pointer::Moved(Point::new(7.0, 6.0))),
        );
        let opened = handle(&mut st, &mut m, Msg::Pointer(Pointer::Released));
        assert_eq!(opened.units(), 1);
        assert!(st.press.is_none());
    }

    #[test]
    fn escape_ends_a_drag_without_moving_anything() {
        let (mut st, mut m) = setup();
        step(
            &mut st,
            &mut m,
            Msg::Press(CWD.into(), FREE.into(), Point::new(5.0, 5.0)),
        );
        step(
            &mut st,
            &mut m,
            Msg::Pointer(Pointer::Moved(Point::new(90.0, 5.0))),
        );
        step(&mut st, &mut m, Msg::Enter(Column::Done));
        let cancelled = handle(&mut st, &mut m, Msg::Pointer(Pointer::Cancelled));
        assert_eq!(cancelled.units(), 0);
        assert!(st.press.is_none());
    }

    #[test]
    fn a_press_on_a_task_that_is_gone_starts_nothing() {
        let (mut st, mut m) = setup();
        step(
            &mut st,
            &mut m,
            Msg::Press(CWD.into(), "gone".into(), Point::ORIGIN),
        );
        assert!(st.press.is_none());
    }

    #[test]
    fn the_filters_pick_clear_and_toggle() {
        let (mut st, mut m) = setup();
        step(&mut st, &mut m, Msg::OpenFilter(Some(FilterKind::Label)));
        assert_eq!(st.open_filter, Some(FilterKind::Label));
        // The same button again closes its list.
        step(&mut st, &mut m, Msg::OpenFilter(Some(FilterKind::Label)));
        assert_eq!(st.open_filter, None);

        step(&mut st, &mut m, Msg::OpenFilter(Some(FilterKind::Label)));
        step(&mut st, &mut m, Msg::Filter(FilterKind::Label, "ui".into()));
        assert_eq!(st.filters.label, "ui");
        assert_eq!(st.open_filter, None);
        assert!(st.filters.active());

        step(&mut st, &mut m, Msg::ClearFilters);
        assert!(!st.filters.active());
    }

    #[test]
    fn a_section_of_the_dock_folds_and_unfolds() {
        let (mut st, mut m) = setup();
        assert!(st.folded.contains(&Column::Todo));
        step(&mut st, &mut m, Msg::Fold(Column::Todo));
        assert!(!st.folded.contains(&Column::Todo));
        step(&mut st, &mut m, Msg::Fold(Column::Todo));
        assert!(st.folded.contains(&Column::Todo));
    }

    #[test]
    fn asking_to_add_unfolds_the_column_and_shows_the_columns() {
        let (mut st, mut m) = setup();
        let focus = handle(&mut st, &mut m, Msg::StartAdding(CWD.into(), Column::Done));
        assert!(st.composing.contains(CWD) && st.adding == Some((CWD.into(), Column::Done)));
        assert!(!st.folded.contains(&Column::Done));
        // The keyboard goes to the field once it is drawn.
        assert_eq!(focus.units(), 1);
    }

    #[test]
    fn the_cache_follows_the_workspaces_and_answers_reads() {
        let mut st = State::default();
        let mut m = model(Locale::En, "graphite", None);
        // The model knows its folder but has not read it.
        assert!(!m.live.board.is_loaded(CWD));
        let read = handle(
            &mut st,
            &mut m,
            Msg::Follow(vec![CWD.into(), "/other".into()]),
        );
        // Only the folder that was not held yet is read: `/other`.
        assert_eq!(read.units(), 1);
        assert_eq!(m.live.board.folders(), vec!["/other", CWD]);

        let again = handle(
            &mut st,
            &mut m,
            Msg::Loaded(CWD.into(), Some(Arc::new(tasks()))),
        );
        assert_eq!(again.units(), 0);
        assert_eq!(m.live.board.tasks(CWD).map(<[BoardTask]>::len), Some(7));
    }

    #[test]
    fn a_change_event_reads_every_board_once() {
        let (mut st, mut m) = setup();
        let event_reads = read_all(&mut m);
        assert_eq!(event_reads.units(), 1);
        // A read is out: a second event waits for it instead of piling on.
        assert_eq!(read_all(&mut m).units(), 0);
        let _ = &mut st;
    }

    #[test]
    fn opening_a_task_and_closing_its_overlay() {
        let (mut st, mut m) = setup();
        let open = Intent::OpenTask {
            cwd: CWD.into(),
            task: Some(DOING.into()),
        };
        // The shell pushes the overlay first, then offers the intent.
        m.ui.overlays.push(Overlay::Task {
            cwd: CWD.into(),
            task: Some(DOING.into()),
        });
        assert!(react(&mut st, &mut m, &open).is_some());
        assert!(st.dialog.is_some());

        // Still open while its overlay is on the stack.
        react(&mut st, &mut m, &Intent::CloseOverlay);
        assert!(st.dialog.is_some());

        m.ui.overlays.pop();
        assert!(react(&mut st, &mut m, &Intent::CloseOverlay).is_some());
        assert!(st.dialog.is_none());
    }

    #[test]
    fn closing_the_dialog_saves_what_was_typed() {
        let (mut st, mut m) = setup();
        let open = Intent::OpenTask {
            cwd: CWD.into(),
            task: Some(DOING.into()),
        };
        react(&mut st, &mut m, &open);
        let dialog = st.dialog.as_mut().expect("open");
        dialog.seed(Column::Todo, String::new());
        step(
            &mut st,
            &mut m,
            Msg::Dialog(task_dialog::Msg::Title("Renamed".into())),
        );
        // One save (the title), and the keyboard back to the card.
        let closed = task_dialog::closed(&mut st, &m);
        assert_eq!(closed.units(), 2);
        assert!(st.dialog.is_none());
    }

    #[test]
    fn a_new_task_takes_what_the_add_field_held() {
        let (mut st, mut m) = setup();
        st.drafts
            .insert((CWD.into(), Column::Doing), "Typed in the board".into());
        step(&mut st, &mut m, Msg::AddDetails(CWD.into(), Column::Doing));
        assert!(st.drafts.is_empty());
        let new = Intent::OpenTask {
            cwd: CWD.into(),
            task: None,
        };
        react(&mut st, &mut m, &new);
        let dialog = st.dialog.as_ref().expect("open");
        assert!(dialog.is_new());
    }

    #[test]
    fn an_open_dialog_closes_when_an_agent_deletes_the_task() {
        let (mut st, mut m) = setup();
        react(
            &mut st,
            &mut m,
            &Intent::OpenTask {
                cwd: CWD.into(),
                task: Some(DOING.into()),
            },
        );
        let without: Vec<BoardTask> = tasks().into_iter().filter(|t| t.id != DOING).collect();
        step(
            &mut st,
            &mut m,
            Msg::Loaded(CWD.into(), Some(Arc::new(without))),
        );
        assert!(st.dialog.is_none());
    }

    #[test]
    fn leaving_the_board_ends_what_was_in_progress() {
        let (mut st, mut m) = setup();
        step(
            &mut st,
            &mut m,
            Msg::Press(CWD.into(), FREE.into(), Point::ORIGIN),
        );
        st.open_filter = Some(FilterKind::State);
        st.composing.insert(CWD.into());
        react(&mut st, &mut m, &Intent::SetView(View::Terminals));
        assert!(st.press.is_none() && st.open_filter.is_none() && st.composing.is_empty());
    }

    #[test]
    fn f6_lands_on_the_first_card_of_the_first_column_that_has_one() {
        let (st, mut m) = setup();
        assert!(focus_entry(&st, &m, Region::Main).is_some());
        assert!(focus_entry(&st, &m, Region::Dock).is_some());
        assert!(focus_entry(&st, &m, Region::Navigator).is_none());
        // An empty board has nothing to land on.
        let empty = model(Locale::En, "graphite", Some(Vec::new()));
        assert!(focus_entry(&st, &empty, Region::Main).is_none());
        m.live.version += 1;
    }

    #[test]
    fn the_clock_runs_only_while_a_task_is_in_progress_with_one() {
        let (_, mut m) = setup();
        // No board is on screen: nothing to redraw.
        assert!(!clock_running(&m));
        m.ui.view = View::Board;
        assert!(clock_running(&m));
        let mut idle = model(Locale::En, "graphite", Some(vec![model::test_task("a")]));
        idle.ui.view = View::Board;
        assert!(!clock_running(&idle));
    }

    // ---- Two windows ------------------------------------------------------

    /// What `actions::run` was asked for, without running it.
    fn asked() -> Vec<(String, Mutation)> {
        actions::take_asked()
    }

    #[test]
    fn a_card_added_in_a_detached_window_lands_on_that_workspaces_board() {
        // The main window's active workspace is on `CWD`; the person is in the
        // detached `site` window, whose board is on `SITE`.
        let (mut m, _main, _site) = two_windows();
        let mut st = State::default();
        asked();
        step(&mut st, &mut m, Msg::StartAdding(SITE.into(), Column::Todo));
        step(
            &mut st,
            &mut m,
            Msg::Draft(SITE.into(), Column::Todo, "Fix the header".into()),
        );
        step(&mut st, &mut m, Msg::Submit(SITE.into(), Column::Todo));
        let written = asked();
        assert_eq!(written.len(), 1, "{written:?}");
        assert_eq!(written[0].0, SITE);
        assert!(matches!(
            &written[0].1,
            Mutation::Create(draft) if draft.title == "Fix the header" && draft.column == Column::Todo
        ));
    }

    #[test]
    fn a_draft_is_made_on_the_board_it_was_typed_on() {
        // Typed in the detached window, abandoned by a press in the main one.
        let (mut m, _main, _site) = two_windows();
        let mut st = State::default();
        asked();
        step(
            &mut st,
            &mut m,
            Msg::Draft(SITE.into(), Column::Todo, "From the site".into()),
        );
        let elsewhere = Msg::Press(CWD.into(), FREE.into(), Point::ORIGIN);
        assert!(elsewhere.leaves_the_add_field());
        drop(update_without_services(&mut st, &mut m, elsewhere));
        let written = asked();
        assert_eq!(written.len(), 1, "{written:?}");
        assert_eq!(written[0].0, SITE);
    }

    /// `update` without a `Services`, for the messages that do not use one.
    fn update_without_services(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
        let abandoned = flush_drafts(st, msg.keeps_draft_of());
        Task::batch([abandoned, handle(st, m, msg)])
    }

    #[test]
    fn a_card_dropped_in_a_detached_window_moves_on_that_workspaces_board() {
        let (mut m, _main, _site) = two_windows();
        let mut st = State::default();
        asked();
        step(
            &mut st,
            &mut m,
            Msg::Press(SITE.into(), "site-task".into(), Point::new(5.0, 5.0)),
        );
        step(
            &mut st,
            &mut m,
            Msg::Pointer(Pointer::Moved(Point::new(90.0, 5.0))),
        );
        step(&mut st, &mut m, Msg::Enter(Column::Review));
        step(&mut st, &mut m, Msg::Pointer(Pointer::Released));
        let written = asked();
        assert_eq!(written.len(), 1, "{written:?}");
        assert_eq!(written[0].0, SITE);
        assert!(matches!(
            &written[0].1,
            Mutation::Update { id, edit: Edit::Column(Column::Review) } if id == "site-task"
        ));
    }

    #[test]
    fn a_click_on_a_card_of_a_detached_window_opens_the_task_of_its_board() {
        let (mut m, _main, _site) = two_windows();
        let mut st = State::default();
        step(
            &mut st,
            &mut m,
            Msg::Press(SITE.into(), "site-task".into(), Point::new(5.0, 5.0)),
        );
        assert_eq!(
            st.press.as_ref().map(|press| press.cwd.as_str()),
            Some(SITE)
        );
        let opened = handle(&mut st, &mut m, Msg::Pointer(Pointer::Released));
        assert_eq!(opened.units(), 1);
    }

    #[test]
    fn the_keyboard_lands_on_the_board_of_the_window_that_has_it() {
        let (mut m, main, site) = two_windows();
        let st = State::default();
        // The detached window has the keyboard: its board has one card.
        assert_eq!(focused_cwd(&m).as_deref(), Some(SITE));
        assert!(focus_entry(&st, &m, Region::Main).is_some());
        // The main window has it: the active workspace's board.
        m.ui.focused_window = Some(main);
        assert_eq!(focused_cwd(&m).as_deref(), Some(CWD));
        let _ = site;
    }

    #[test]
    fn the_clock_runs_for_a_board_on_screen_in_any_window() {
        use crate::state::Patch;
        let (mut m, main, site) = two_windows();
        // Only the site's board has a task in progress with a clock.
        let mut doing = model::test_task("site-task");
        doing.column = "doing".into();
        doing.doing_since = Some(1);
        m.live.board.request(SITE);
        m.live.board.answer(SITE, Some(Arc::new(vec![doing])));
        let idle: Vec<BoardTask> = vec![model::test_task("idle")];
        m.live.board.request(CWD);
        m.live.board.answer(CWD, Some(Arc::new(idle)));
        m.apply(StoreAction::UpdateSettings(Patch::new(
            |settings: &mut Settings| settings.board_dock_open = true,
        )));
        // Every window shows its terminals with the dock beside them.
        assert!(boards_on_screen(&m).iter().any(|ws| ws.cwd == SITE));
        assert!(clock_running(&m), "the site's dock is on screen");

        // The site window shows the memory view: its board is not drawn.
        m.ui.focused_window = Some(site);
        m.ui.view = View::Memory;
        assert!(!boards_on_screen(&m).iter().any(|ws| ws.cwd == SITE));
        assert!(!clock_running(&m));

        // The main window on the full board: its workspace's board is drawn,
        // and the site window (back on its terminals, dock open) draws its own.
        m.ui.focused_window = Some(main);
        m.ui.view = View::Board;
        let drawn: Vec<&str> = boards_on_screen(&m)
            .iter()
            .map(|ws| ws.cwd.as_str())
            .collect();
        assert!(drawn.contains(&CWD) && drawn.contains(&SITE), "{drawn:?}");
    }

    #[test]
    fn the_dock_as_a_drawer_counts_as_on_screen() {
        use crate::shell::Drawer;
        use crate::state::Patch;
        let (mut m, main, site) = two_windows();
        m.apply(StoreAction::UpdateSettings(Patch::new(
            |settings: &mut Settings| settings.board_dock_open = false,
        )));
        // The dock is closed: nothing is drawn.
        assert!(boards_on_screen(&m).is_empty());
        // Too narrow for a dock beside the terminals: the shell makes it a
        // drawer, shown only while the person has opened it.
        m.ui.body_width.insert(site, 700.0);
        m.ui.drawer = Some(Drawer::Dock);
        assert!(
            Panels::of(&m, site).dock.drawer,
            "the room leaves only a drawer"
        );
        // The old rule read the setting alone and drew no clock for it.
        assert!(boards_on_screen(&m).iter().any(|ws| ws.cwd == SITE));
    }

    #[test]
    fn the_grip_saves_the_width_once_when_it_is_let_go() {
        let (mut st, mut m) = setup();
        let saved = |m: &Model| m.settings().board_dock_width;
        let before = saved(&m);
        let revision = m.revision();
        step(
            &mut st,
            &mut m,
            Msg::GripPress {
                at: Point::new(1000.0, 50.0),
                width: before as f32,
                max: 800.0,
            },
        );
        for x in [990.0, 960.0, 940.0] {
            step(
                &mut st,
                &mut m,
                Msg::Pointer(Pointer::Moved(Point::new(x, 50.0))),
            );
        }
        // Nothing is written while the pointer travels, but the frame draws
        // the dock where the pointer is.
        assert_eq!(saved(&m), before);
        assert_eq!(m.revision(), revision);
        assert_eq!(held(&m), Some(before as f32 + 60.0));
        let main = iced::window::Id::unique();
        m.live.windows.insert(main, crate::shell::WindowKind::Main);
        assert_eq!(Panels::of(&m, main).dock.width, before as f32 + 60.0);
        step(&mut st, &mut m, Msg::Pointer(Pointer::Released));
        assert_eq!(saved(&m), before + 60.0);
        assert!(st.grip.is_none());
        assert_eq!(held(&m), None);
        assert_eq!(m.revision(), revision + 1);
    }

    /// The width the frame is told to draw the dock at while its grip is
    /// held.
    fn held(m: &Model) -> Option<f32> {
        m.live
            .signals
            .panel_drag
            .filter(|drag| drag.panel == Drawer::Dock)
            .map(|drag| drag.width)
    }

    #[test]
    fn leaving_the_board_drops_a_held_grip() {
        let (mut st, mut m) = setup();
        let before = m.settings().board_dock_width;
        step(
            &mut st,
            &mut m,
            Msg::GripPress {
                at: Point::new(1000.0, 50.0),
                width: before as f32,
                max: 800.0,
            },
        );
        step(
            &mut st,
            &mut m,
            Msg::Pointer(Pointer::Moved(Point::new(900.0, 50.0))),
        );
        assert!(held(&m).is_some());
        let _ = react(&mut st, &mut m, &Intent::CloseDock);
        assert!(st.grip.is_none());
        assert_eq!(held(&m), None);
        assert_eq!(m.settings().board_dock_width, before);
    }

    #[test]
    fn letting_go_leaves_another_panel_s_held_grip_alone() {
        let (mut st, mut m) = setup();
        let inspector = PanelDrag {
            panel: Drawer::Inspector,
            width: 300.0,
        };
        m.live.signals.panel_drag = Some(inspector);
        step(&mut st, &mut m, Msg::GripReset);
        step(&mut st, &mut m, Msg::Pointer(Pointer::Cancelled));
        assert_eq!(m.live.signals.panel_drag, Some(inspector));
    }

    #[test]
    fn a_cancelled_grip_leaves_the_width_as_it_was() {
        let (mut st, mut m) = setup();
        let before = m.settings().board_dock_width;
        step(
            &mut st,
            &mut m,
            Msg::GripPress {
                at: Point::new(1000.0, 50.0),
                width: before as f32,
                max: 800.0,
            },
        );
        step(
            &mut st,
            &mut m,
            Msg::Pointer(Pointer::Moved(Point::new(900.0, 50.0))),
        );
        assert_eq!(held(&m), Some(before as f32 + 100.0));
        step(&mut st, &mut m, Msg::Pointer(Pointer::Cancelled));
        assert_eq!(m.settings().board_dock_width, before);
        assert!(st.grip.is_none());
        assert_eq!(held(&m), None);
    }

    #[test]
    fn a_grip_released_where_it_began_writes_nothing() {
        let (mut st, mut m) = setup();
        let revision = m.revision();
        let width = m.settings().board_dock_width as f32;
        step(
            &mut st,
            &mut m,
            Msg::GripPress {
                at: Point::new(1000.0, 50.0),
                width,
                max: 800.0,
            },
        );
        step(&mut st, &mut m, Msg::Pointer(Pointer::Released));
        assert_eq!(m.revision(), revision);
    }

    #[test]
    fn the_arrow_keys_widen_and_narrow_within_the_room_the_message_carries() {
        let (mut st, mut m) = setup();
        let before = m.settings().board_dock_width as f32;
        step(
            &mut st,
            &mut m,
            Msg::GripKey {
                widen: true,
                width: before,
                max: before + 10.0,
            },
        );
        // The window leaves only ten more points.
        assert_eq!(m.settings().board_dock_width as f32, before + 10.0);
    }
}
