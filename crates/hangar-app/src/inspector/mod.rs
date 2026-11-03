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
//! of the working tree is polled here (`poll`), only while the section is
//! unfolded and the window is in front.
//!
//! The width, the folded sections and whether the column is open are
//! settings and change through `Model::apply`; the grip writes the width as
//! it is dragged, so the shell's frame (which sizes the column from the
//! setting) follows the pointer.

mod files;
pub mod files_model;
pub mod format;
mod header;
pub mod poll;
mod sections;
pub mod sources;
mod view;
mod widgets;

use std::sync::Arc;

use iced::keyboard::{self, key::Named};
use iced::{Subscription, Task, event, time, window};

use hangar_core::git::Changes;

use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::side_panels::{INSPECTOR_MAX, INSPECTOR_MIN};
use crate::shell::{Drawer, Focus, Region, View};
use crate::state::types::Settings;
use crate::state::{Model, PaneId, Patch, StoreAction, WsId};

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
    /// The window is not in front (another application has the focus, or it
    /// is minimised): git is not asked, as the web app did not while its
    /// page was hidden.
    away: bool,
    /// git's last answer for a folder, until the next one or a failure.
    changes: Option<(String, Arc<Changes>)>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    /// Where the pointer was when the grip was pressed.
    from_x: f32,
    /// The width then.
    from_width: f64,
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
    GripPressed(f32),
    GripMoved(f32),
    GripReleased,
    /// A double click on the grip.
    ResetWidth,
    /// An arrow key while the column has the keyboard: left grows it.
    Nudge(i16),
    /// git answered for this key (poll.rs).
    Changes(poll::PollKey, Result<Arc<Changes>, String>),
    /// The window went to the back or came to the front.
    Away(bool),
    /// The half-minute clock: nothing to do but be drawn again.
    Clock,
}

/// Whether the column is on screen: docked by its setting, or opened as a
/// drawer over the terminals.
pub fn shown(m: &Model) -> bool {
    m.settings().inspector_open || m.ui.drawer == Some(Drawer::Inspector)
}

/// The width the settings can hold, in interface points (`clampInspector`).
fn clamp_width(width: f64) -> f64 {
    width
        .round()
        .clamp(f64::from(INSPECTOR_MIN), f64::from(INSPECTOR_MAX))
}

/// The width a drag has reached: the column grows leftwards, into the work
/// area, by as far as the pointer went that way from where it was pressed.
fn dragged_width(from_width: f64, from_x: f32, x: f32) -> f64 {
    clamp_width(from_width + f64::from(from_x - x))
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

fn set_width(m: &mut Model, width: f64) {
    let width = clamp_width(width);
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
        Msg::GripPressed(x) => {
            st.drag = Some(Drag {
                from_x: x,
                from_width: m.settings().inspector_width,
            });
            Task::none()
        }
        Msg::GripMoved(x) => {
            if let Some(drag) = st.drag {
                set_width(m, dragged_width(drag.from_width, drag.from_x, x));
            }
            Task::none()
        }
        Msg::GripReleased => {
            st.drag = None;
            Task::none()
        }
        Msg::ResetWidth => {
            st.drag = None;
            set_width(m, Settings::default().inspector_width);
            Task::none()
        }
        Msg::Nudge(delta) => {
            set_width(m, m.settings().inspector_width + f64::from(delta));
            Task::none()
        }
        Msg::Changes(key, answer) => {
            // An answer for a look that is no longer the current one (the
            // pane or its folder changed meanwhile) is dropped.
            if poll_key(st, m).as_ref() == Some(&key) {
                st.changes = answer.ok().map(|changes| (key.folder, changes));
            }
            Task::none()
        }
        Msg::Away(away) => {
            st.away = away;
            if away {
                st.drag = None;
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

/// The subscription of git's look at the pane's folder, or `None` when
/// nothing should be asked: the column is hidden, the Files section is
/// folded (it shows nothing that needs git), the window is away, or no pane
/// is in focus.
fn poll_key(st: &State, m: &Model) -> Option<poll::PollKey> {
    if st.away || !shown(m) {
        return None;
    }
    if m.settings()
        .inspector_collapsed
        .iter()
        .any(|id| id == SectionId::Files.as_str())
    {
        return None;
    }
    let ws = m.active_workspace()?;
    let (pane, _) = sources::focused_pane(m, ws)?;
    let insight = sources::insight(m, &pane.id);
    Some(poll::PollKey {
        pane: pane.id.clone(),
        folder: pane.cwd.clone().unwrap_or_else(|| ws.cwd.clone()),
        trigger: poll::trigger(&insight.edited_files, insight.turn_started_at),
    })
}

/// The look itself: `git_changes` against HEAD.
fn look(key: &poll::PollKey) -> impl iced::futures::Stream<Item = Message> + use<> {
    use iced::futures::StreamExt;
    poll::stream(key.clone(), poll::Timing::LIVE, |folder| async move {
        hangar_core::git::git_changes(folder, None).await
    })
    .map(|(key, answer)| Message::Inspector(Msg::Changes(key, answer.map(Arc::new))))
}

/// Focus coming and going, for the polling.
fn window_focus(event: iced::Event, _status: event::Status, _id: window::Id) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::Focused) => Some(Message::Inspector(Msg::Away(false))),
        iced::Event::Window(window::Event::Unfocused) => Some(Message::Inspector(Msg::Away(true))),
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
    if let Some(key) = poll_key(st, m) {
        all.push(Subscription::run_with(key, look));
    }
    if sections::counts_time(m) {
        all.push(time::every(CLOCK).map(|_| Message::Inspector(Msg::Clock)));
    }
    if shown(m) && m.ui.focus == Some(Focus::Region(Region::Inspector)) {
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
    (region == Region::Inspector && shown(m)).then(Task::none)
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
            if !shown(m) {
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
