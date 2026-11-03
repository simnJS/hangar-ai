//! P9: Inspector.
//!
//! The column on the right: everything about the focused pane — its session,
//! context and cost, the board task it is on, the prompts waiting for it, the
//! files it touched and what it did lately. Ports `Inspector.tsx`,
//! `InspectorSection.tsx`, `InspectorPanels.tsx`, `InspectorFiles.tsx` and
//! `lib/{inspectorFiles,inspectorFormat}.ts` (inventory-frontend.md §2.2).
//!
//! It reads only. What the pane reported, the plan's limits and the board
//! come from other packages and are read in one place, `sources`; git's view
//! of the working tree is polled here (`poll`), for each window that shows the
//! column, only while its Files section is unfolded and the app is in front.
//!
//! Each window has its own column: it speaks about the workspace that window
//! draws (`Ctx::workspace`, `Model::window_workspace`) and is on screen or not
//! as the frame placed it (`Panels::of`), docked by its setting or as a drawer
//! over the terminals.
//!
//! The width, the folded sections and whether the column is open are
//! settings and change through `Model::apply`. While the grip is held, the
//! width it has reached goes to `live.signals.panel_drag`, which the shell's
//! frame (`Panels::of`) draws the column at; the setting is written once, when
//! the grip is let go, as the TS did, instead of a save and a snapshot for
//! every move of the pointer. Escape, or the app going to the back, drops the
//! drag and the column keeps the width it had. The widest it may be is what
//! the frame placed for the window the grip was pressed in.

mod files;
pub mod files_model;
pub mod format;
mod header;
pub mod poll;
mod sections;
pub mod sources;
mod view;
mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::keyboard::{self, key::Named};
use iced::{Subscription, Task, event, time, window};

use hangar_core::git::Changes;

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::side_panels::{INSPECTOR_MAX, INSPECTOR_MIN, Panels};
use crate::shell::{Drawer, Focus, Region, View};
use crate::state::types::Settings;
use crate::state::{Model, PaneId, PanelDrag, Patch, StoreAction, WsId};

pub use view::view;

/// How far an arrow key resizes the column (`KEY_STEP`).
const KEY_STEP: i16 = 16;
/// Ticks every half minute — so "1 h 12" is never a minute behind — and only
/// while something on screen counts time.
const CLOCK: std::time::Duration = std::time::Duration::from_secs(30);

/// The sections, in the order the column lists them. The ids are what
/// `settings.inspectorCollapsed` stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionId {
    Session,
    Terminal,
    Context,
    Task,
    Queue,
    Files,
    Activity,
}

impl SectionId {
    pub fn as_str(self) -> &'static str {
        match self {
            SectionId::Session => "session",
            SectionId::Terminal => "terminal",
            SectionId::Context => "context",
            SectionId::Task => "task",
            SectionId::Queue => "queue",
            SectionId::Files => "files",
            SectionId::Activity => "activity",
        }
    }
}

/// The package's UI state; lives in `App.pkg.inspector`.
#[derive(Debug, Default)]
pub struct State {
    /// The grip, while it is held.
    drag: Option<Drag>,
    /// The app is not in front (another application has the focus, or its
    /// windows are minimised): git is not asked, as the web app did not while
    /// its page was hidden.
    away: bool,
    /// The windows that have the system's focus, which is how `away` is told
    /// from one window handing the focus to another.
    focused: HashSet<window::Id>,
    /// git's last answer for each folder some window looks at, until the next
    /// one or a failure.
    changes: HashMap<String, Arc<Changes>>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    /// Where the pointer was when the grip was pressed.
    from_x: f32,
    /// The width then, as drawn.
    from_width: f64,
    /// The widest the room let the column be in that window then, in points.
    max: f32,
    /// The width the pointer has reached, which the release saves.
    to: f64,
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.inspector`. The inspector keeps nothing others need.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Inspector(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A section's title was pressed: fold it or open it.
    Toggle(SectionId),
    /// The header's cross. Carries the pane to give the keyboard back to.
    Close(WsId, Option<PaneId>),
    OpenBoard,
    OpenSessions(WsId, PaneId),
    OpenMcpServers(WsId),
    OpenDiff(WsId, PaneId),
    /// The × of a queued prompt.
    CancelQueued(PaneId, String),
    /// The grip was pressed at this x. `max` is the widest its window's room
    /// lets the column be (`Panels::of`).
    GripPressed {
        x: f32,
        max: f32,
    },
    GripMoved(f32),
    GripReleased,
    /// Escape while the grip is held: the width stays as it was.
    GripCancelled,
    /// A double click on the grip.
    ResetWidth,
    /// An arrow key while the column has the keyboard: left grows it.
    Nudge(i16),
    /// git answered for this key (poll.rs).
    Changes(poll::PollKey, Result<Arc<Changes>, String>),
    /// A window of the app gained or lost the system's focus.
    Focus(window::Id, bool),
    /// The half-minute clock: nothing to do but be drawn again.
    Clock,
}

/// Whether window `w` shows the column: docked by its setting while the room
/// leaves the terminals theirs, otherwise opened as a drawer over them
/// (`Panels::of`). A column the setting calls open but the room turned into a
/// closed drawer is not on screen, and nothing about it is worth polling or
/// counting.
pub fn shown_in(m: &Model, w: window::Id) -> bool {
    Panels::of(m, w).inspector.shown
}

/// The windows that show the column now, in a fixed order.
fn windows_showing(m: &Model) -> Vec<window::Id> {
    let mut windows: Vec<window::Id> = m
        .live
        .windows
        .keys()
        .copied()
        .filter(|w| shown_in(m, *w))
        .collect();
    windows.sort();
    windows
}

/// The widest the focused window lets the column be, in points: the frame's
/// answer, never over the setting's own bound.
fn focused_max(m: &Model) -> f32 {
    m.focused_window()
        .map_or(INSPECTOR_MAX, |w| Panels::of(m, w).inspector.max)
}

/// The width the settings can hold, in interface points (`clampInspector`),
/// and no more than `max`, the room the window has: `Math.min(INSPECTOR_MAX,
/// maxWidth)` of the TS. The minimum wins over a room too small for it.
fn clamp_width(width: f64, max: f32) -> f64 {
    let max = f64::from(max).min(f64::from(INSPECTOR_MAX));
    width.min(max).max(f64::from(INSPECTOR_MIN)).round()
}

/// The width the column is drawn at, which is what a drag or a nudge starts
/// from: the saved width, in bounds, cut to the room (`shown` of the TS).
fn drawn_width(saved: f64, max: f32) -> f64 {
    clamp_width(saved, INSPECTOR_MAX).min(f64::from(max))
}

/// The width a drag has reached: the column grows leftwards, into the work
/// area, by as far as the pointer went that way from where it was pressed.
fn dragged_width(from_width: f64, from_x: f32, x: f32, max: f32) -> f64 {
    clamp_width(from_width + f64::from(from_x - x), max)
}

/// A section id added to the folded ones, or taken out if it is there.
fn fold(collapsed: &mut Vec<String>, id: &str) {
    match collapsed.iter().position(|folded| folded == id) {
        Some(at) => {
            collapsed.remove(at);
        }
        None => collapsed.push(id.to_string()),
    }
}

/// The column is drawn at `width` while the grip is held, with no setting
/// written.
fn show_drag(m: &mut Model, width: f64) {
    m.live.signals.panel_drag = Some(PanelDrag {
        panel: Drawer::Inspector,
        width: width as f32,
    });
}

/// The column goes back to being drawn at its setting. Another panel's held
/// grip is not the inspector's to let go.
fn end_drag(st: &mut State, m: &mut Model) {
    st.drag = None;
    if m.live
        .signals
        .panel_drag
        .is_some_and(|drag| drag.panel == Drawer::Inspector)
    {
        m.live.signals.panel_drag = None;
    }
}

fn set_width(m: &mut Model, width: f64, max: f32) {
    let width = clamp_width(width, max);
    if (m.settings().inspector_width - width).abs() < 0.5 {
        return;
    }
    m.apply(StoreAction::UpdateSettings(Patch::new(
        move |settings: &mut Settings| settings.inspector_width = width,
    )));
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::CancelQueued(pane, id) => {
            // The queue lives in the bridge: the pane hears the change and
            // the list follows, so there is nothing to undo here and a
            // failure has nobody to tell.
            let host = sv.core().host().clone();
            Task::perform(
                // Started when the task runs, inside the runtime.
                async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        hangar_core::bridge::bridge_cancel(&host, pane, Some(id));
                    })
                    .await;
                },
                |()| Message::Noop,
            )
        }
        other => reduce(st, m, other),
    }
}

/// The intent a message stands for, when it is a request to another package.
fn request(msg: &Msg) -> Option<Intent> {
    Some(match msg.clone() {
        Msg::OpenBoard => Intent::SetView(View::Board),
        Msg::OpenSessions(ws, pane) => Intent::OpenSessions { ws, pane },
        Msg::OpenMcpServers(ws) => Intent::OpenMcpServers(ws),
        Msg::OpenDiff(ws, pane) => Intent::OpenDiff {
            ws,
            pane: Some(pane),
        },
        _ => return None,
    })
}

/// Everything a message does that needs no service, so that it can be
/// tested on a bare model.
fn reduce(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    if let Some(asked) = request(&msg) {
        return intent(asked);
    }
    match msg {
        Msg::Toggle(section) => {
            // Read from the model as it stands, not from the render that drew
            // the title: two folds before a redraw must not lose one.
            let id = section.as_str();
            m.apply(StoreAction::UpdateSettings(Patch::new(
                move |settings: &mut Settings| fold(&mut settings.inspector_collapsed, id),
            )));
            Task::none()
        }
        Msg::Close(ws, pane) => {
            if m.ui.drawer == Some(Drawer::Inspector) {
                m.ui.drawer = None;
            } else {
                m.apply(StoreAction::UpdateSettings(Patch::new(
                    |settings: &mut Settings| settings.inspector_open = false,
                )));
            }
            // The keyboard goes back where it was working: the focused pane.
            match pane {
                Some(pane) => intent(Intent::FocusPane {
                    ws,
                    pane,
                    terminal: true,
                }),
                None => Task::none(),
            }
        }
        Msg::GripPressed { x, max } => {
            let from_width = drawn_width(m.settings().inspector_width, max);
            st.drag = Some(Drag {
                from_x: x,
                from_width,
                max,
                to: from_width,
            });
            Task::none()
        }
        Msg::GripMoved(x) => {
            if let Some(drag) = st.drag.as_mut() {
                drag.to = dragged_width(drag.from_width, drag.from_x, x, drag.max);
                let to = drag.to;
                show_drag(m, to);
            }
            Task::none()
        }
        Msg::GripReleased => {
            let held = st.drag;
            end_drag(st, m);
            // A release where the press was writes nothing (`set_width`
            // skips an unchanged width).
            if let Some(drag) = held {
                set_width(m, drag.to, drag.max);
            }
            Task::none()
        }
        Msg::GripCancelled => {
            end_drag(st, m);
            Task::none()
        }
        Msg::ResetWidth => {
            end_drag(st, m);
            set_width(m, Settings::default().inspector_width, INSPECTOR_MAX);
            Task::none()
        }
        Msg::Nudge(delta) => {
            let max = focused_max(m);
            let from = drawn_width(m.settings().inspector_width, max);
            set_width(m, from + f64::from(delta), max);
            Task::none()
        }
        Msg::Changes(key, answer) => {
            // An answer for a look that is no longer a current one (the pane
            // or its folder changed meanwhile) is dropped.
            if poll_keys(st, m).contains(&key) {
                match answer {
                    Ok(changes) => {
                        st.changes.insert(key.folder, changes);
                    }
                    Err(_) => {
                        st.changes.remove(&key.folder);
                    }
                }
            }
            // What no window looks at any more is let go.
            let looked_at = looks(m);
            st.changes
                .retain(|folder, _| looked_at.iter().any(|look| &look.folder == folder));
            Task::none()
        }
        Msg::Focus(window, focused) => {
            if focused {
                st.focused.insert(window);
                st.away = false;
            } else {
                st.focused.remove(&window);
                // One window handing the focus to another is not leaving.
                if st.focused.is_empty() {
                    st.away = true;
                    end_drag(st, m);
                }
            }
            Task::none()
        }
        Msg::Clock
        | Msg::CancelQueued(..)
        | Msg::OpenBoard
        | Msg::OpenSessions(..)
        | Msg::OpenMcpServers(..)
        | Msg::OpenDiff(..) => Task::none(),
    }
}

fn intent(intent: Intent) -> Task<Message> {
    Task::done(Message::Intent(intent))
}

/// git's look at the focused pane's folder, in the window `w` that shows the
/// column; `None` when that window has no pane in focus.
fn look_in(m: &Model, w: window::Id) -> Option<poll::PollKey> {
    let ws = m.window_workspace(w)?;
    let (pane, _) = sources::focused_pane(m, ws)?;
    let insight = sources::insight(m, &pane.id);
    Some(poll::PollKey {
        pane: pane.id.clone(),
        folder: pane.cwd.clone().unwrap_or_else(|| ws.cwd.clone()),
        trigger: poll::trigger(&insight.edited_files, insight.turn_started_at),
    })
}

/// What every window that shows the column looks at, whether or not git is
/// being asked now.
fn looks(m: &Model) -> Vec<poll::PollKey> {
    let mut looks: Vec<poll::PollKey> = Vec::new();
    for window in windows_showing(m) {
        if let Some(look) = look_in(m, window)
            && !looks.contains(&look)
        {
            looks.push(look);
        }
    }
    looks
}

/// The subscriptions of git's look at each pane's folder: one for each window
/// that shows the column, none while the app is away or the Files section is
/// folded (it shows nothing that needs git).
fn poll_keys(st: &State, m: &Model) -> Vec<poll::PollKey> {
    let folded = m
        .settings()
        .inspector_collapsed
        .iter()
        .any(|id| id == SectionId::Files.as_str());
    if st.away || folded {
        return Vec::new();
    }
    looks(m)
}

/// The look itself: `git_changes` against HEAD.
fn look(key: &poll::PollKey) -> impl iced::futures::Stream<Item = Message> + use<> {
    use iced::futures::StreamExt;
    poll::stream(key.clone(), poll::Timing::LIVE, |folder| async move {
        hangar_core::git::git_changes(folder, None).await
    })
    .map(|(key, answer)| Message::Inspector(Msg::Changes(key, answer.map(Arc::new))))
}

/// Focus coming and going, for the polling. A window that closes has the
/// focus no more, whether or not it said so first.
fn window_focus(event: iced::Event, _status: event::Status, id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Focused) => {
            Some(Message::Inspector(Msg::Focus(id, true)))
        }
        iced::Event::Window(window::Event::Unfocused | window::Event::Closed) => {
            Some(Message::Inspector(Msg::Focus(id, false)))
        }
        _ => None,
    }
}

/// The arrows resize the column while it has the keyboard (the web app's
/// separator took them when it had the focus: iced has no focusable
/// separator, so the region stands for it).
fn arrows(event: iced::Event, status: event::Status, _id: window::Id) -> Option<Message> {
    if status == event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    if !modifiers.is_empty() {
        return None;
    }
    match key {
        keyboard::Key::Named(Named::ArrowLeft) => Some(Message::Inspector(Msg::Nudge(KEY_STEP))),
        keyboard::Key::Named(Named::ArrowRight) => Some(Message::Inspector(Msg::Nudge(-KEY_STEP))),
        _ => None,
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut all = vec![event::listen_with(window_focus)];
    for key in poll_keys(st, m) {
        all.push(Subscription::run_with(key, look));
    }
    if windows_showing(m)
        .into_iter()
        .any(|w| sections::counts_time(m, w))
    {
        all.push(time::every(CLOCK).map(|_| Message::Inspector(Msg::Clock)));
    }
    if focused_shows(m) && m.ui.focus == Some(Focus::Region(Region::Inspector)) {
        all.push(event::listen_with(arrows));
    }
    Subscription::batch(all)
}

/// What takes the focus when F6 reaches region `Inspector`. The column has
/// nothing to focus inside (its buttons are for the pointer, iced's take no
/// keyboard focus), so entering the region is the whole of it: the shell
/// marks the region focused, and the arrows then resize.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = st;
    (region == Region::Inspector && focused_shows(m)).then(Task::none)
}

/// Whether the window the person is in shows the column.
fn focused_shows(m: &Model) -> bool {
    m.focused_window().is_some_and(|w| shown_in(m, w))
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (st, sv);
    match intent {
        // "Focus the inspector" shows it first when it is hidden. Setting is
        // idempotent, so it does not matter that the shell may also have
        // opened it.
        Intent::FocusInspector => {
            if !focused_shows(m) {
                m.apply(StoreAction::UpdateSettings(Patch::new(
                    |settings: &mut Settings| settings.inspector_open = true,
                )));
            }
            Some(Task::none())
        }
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it. The inspector
/// reads what other packages keep, and the shell draws it again after every
/// message, so there is nothing to do.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}

#[cfg(test)]
pub(crate) mod snapshot;
#[cfg(test)]
mod tests;
