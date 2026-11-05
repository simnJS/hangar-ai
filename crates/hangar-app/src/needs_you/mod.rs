//! P6: Needs you (`NeedsYou.tsx`, `lib/needsYou.ts`).
//!
//! « Besoin de toi » in the toolbar: a count of what waits for the person, and
//! the list behind it. The button is only there while the list has something
//! in it, and the list only exists once a workspace has an orchestrator or a
//! worker.
//!
//! Each row says what waits and where, and offers the one thing to do about
//! it (go to the agent, answer the orchestrator, merge the delivery) plus the
//! board, where the task says the rest.
//!
//! The boards come from the board package's shared cache (`board::Live`,
//! `useBoardsOf`), which follows core's change events itself.
//!
//! The list is worked out here, off the interface's draw: a beat of the clock
//! (and each press of the button) computes it, keeps it for the button to
//! draw, and tells the shell how long it is (`live.signals.needs_you`), which
//! is what makes `view.needsYou` a live command and what lets the shell leave
//! the panel open: with a count of zero it closes it.

pub mod model;
mod view;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::{Element, Event, Subscription, Task, window};

use hangar_i18n::{Key, Translator, args};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::panes_logic::state_glyph_state;
use crate::services::{CoreEvent, Services};
use crate::shell::{Notification, Region};
use crate::state::folders::sidebar_order;
use crate::state::live::GlyphState;
use crate::state::roles::orchestrator_of;

use crate::state::Model;

use model::{BoardSource, NeedsYouItem, NoticeMemory, is_orchestrated, watched_boards};

/// How often the list is worked out again while there is anything to watch:
/// a pane that starts waiting, a task that gets its label, a delivery that
/// ends. The reads are of memory only, so the beat can be short, and the
/// button and the `view.needsYou` command are never more than this behind.
const BEAT: Duration = Duration::from_millis(400);

/// How long a press on « Land » says it is landing before the delivery itself
/// does (`control::Live::task_delivering`): the engine reads the task's title
/// before it registers the run. Past it, a refusal that never says so must not
/// leave the button dead.
const LAND_GRACE: Duration = Duration::from_secs(10);

/// The package's UI state; lives in `App.pkg.needs_you`.
#[derive(Debug)]
pub struct State {
    /// The list as last worked out; the button draws this.
    items: Vec<NeedsYouItem>,
    /// What « Fusionner » last failed with, by item key.
    failures: HashMap<String, String>,
    /// The deliveries asked to land, and when, until the engine's own run of
    /// them shows.
    landing: HashMap<String, Instant>,
    /// What the notices have already told.
    memory: NoticeMemory,
    /// The window has the system's focus (`document.hasFocus()`).
    focused: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            items: Vec::new(),
            failures: HashMap::new(),
            landing: HashMap::new(),
            memory: NoticeMemory::default(),
            focused: true,
        }
    }
}

impl State {
    /// A row that left the list takes its failure and its landing with it: the
    /// delivery that comes back under the same task is another one.
    fn forget_gone(&mut self, items: &[NeedsYouItem]) {
        let live: HashSet<&str> = items.iter().map(NeedsYouItem::key).collect();
        self.failures.retain(|key, _| live.contains(key.as_str()));
        self.landing.retain(|key, _| live.contains(key.as_str()));
    }

    /// Whether a delivery is being landed right now: its run is going
    /// (`useDelivering`), or it was asked a moment ago and the engine has not
    /// registered it yet.
    pub(crate) fn is_landing(
        &self,
        m: &Model,
        item: &model::ItemBase,
        task: &str,
        now: Instant,
    ) -> bool {
        m.live.control.task_delivering(&item.workspace_id, task)
            || self
                .landing
                .get(&item.key)
                .is_some_and(|since| now.saturating_duration_since(*since) < LAND_GRACE)
    }

    /// The list as last worked out.
    pub(crate) fn items(&self) -> &[NeedsYouItem] {
        &self.items
    }

    pub(crate) fn failure(&self, key: &str) -> Option<&str> {
        self.failures.get(key).map(String::as_str)
    }
}

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.needs_you`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::NeedsYou(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The button was pressed, or the `view.needsYou` shortcut.
    Toggle,
    /// Escape, Tab or a press elsewhere.
    Close,
    /// The beat: the list is worked out again.
    Refresh,
    /// A row's action, once the panel is gone.
    GoTo {
        ws: String,
        pane: String,
    },
    OpenBoard(String),
    Land(LandRequest),
    /// A landing was refused or failed: whoever lands deliveries answers with
    /// this, so the row can say why (`needsYou.landFailed`).
    LandFailed {
        key: String,
        error: String,
    },
    Focus(bool),
}

/// The identity of a delivery row, and what landing it needs.
#[derive(Debug, Clone)]
pub struct LandRequest {
    pub key: String,
    pub ws: String,
    pub task: String,
}

/// The panes whose agent is in a dialog, asking for a permission or an answer
/// (`paneGlyphState` is `waiting`).
fn waiting_panes(m: &Model) -> HashSet<String> {
    m.state()
        .workspaces
        .iter()
        .flat_map(|ws| ws.panes.iter())
        .filter(|pane| state_glyph_state(m, &pane.id) == GlyphState::Waiting)
        .map(|pane| pane.id.clone())
        .collect()
}

/// What the person has to look at, worked out from the state, the panes'
/// activity and the boards.
struct Look {
    items: Vec<NeedsYouItem>,
    /// The watched folders whose board has been read: the notices take a
    /// board's first read as its baseline.
    loaded: HashSet<String>,
}

fn look(m: &Model, waiting: &HashSet<String>, boards: &impl BoardSource) -> Look {
    let state = m.state();
    let workspaces = sidebar_order(&state.workspaces, &state.folders);
    let items = model::needs_you_items(&workspaces, waiting, boards);
    let loaded = watched_boards(&workspaces)
        .into_iter()
        .filter(|cwd| boards.tasks_in(cwd).is_some())
        .collect();
    Look { items, loaded }
}

/// Works the list out again from the model as it is now, keeps it, and tells
/// the shell its length. Runs on the beat and before a press on the button is
/// obeyed: the shell closes the panel after every message while the count is
/// zero, so a count that lagged would shut it as it opened.
fn refresh(st: &mut State, m: &mut Model) -> Task<Message> {
    let seen = look(m, &waiting_panes(m), &m.live.board);
    settle(st, m, seen)
}

fn settle(st: &mut State, m: &mut Model, seen: Look) -> Task<Message> {
    m.live.signals.needs_you = seen.items.len();
    st.forget_gone(&seen.items);
    // A delivery whose run has shown no longer needs the grace: the run says
    // when it ends.
    st.landing.retain(|key, _| {
        !seen.items.iter().any(|item| {
            item.key() == key
                && matches!(item, NeedsYouItem::Land { base, task, .. }
                    if m.live.control.task_delivering(&base.workspace_id, &task.id))
        })
    });
    let tasks = notices(st, m, &seen);
    st.items = seen.items;
    tasks
}

fn watched(m: &Model) -> Vec<String> {
    let state = m.state();
    watched_boards(&sidebar_order(&state.workspaces, &state.folders))
}

/// The button, or the `view.needsYou` shortcut's twin: the list first, so the
/// shell's count is the truth when it looks after this message.
fn toggle(st: &mut State, m: &mut Model) -> Task<Message> {
    let tasks = refresh(st, m);
    m.ui.needs_you_open = !m.ui.needs_you_open;
    tasks
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Toggle => toggle(st, m),
        Msg::Close => {
            m.ui.needs_you_open = false;
            Task::none()
        }
        Msg::Refresh => refresh(st, m),
        Msg::GoTo { ws, pane } => {
            m.ui.needs_you_open = false;
            Task::done(Message::Intent(Intent::FocusPane {
                ws,
                pane,
                terminal: true,
            }))
        }
        Msg::OpenBoard(ws) => {
            m.ui.needs_you_open = false;
            Task::done(Message::Intent(Intent::OpenBoard(ws)))
        }
        Msg::Land(item) => {
            st.failures.remove(&item.key);
            st.landing.insert(item.key, Instant::now());
            Task::done(Message::Intent(Intent::Land {
                ws: item.ws,
                task: item.task,
            }))
        }
        Msg::LandFailed { key, error } => {
            st.landing.remove(&key);
            // Its failure can arrive after the row left, a landing refused once
            // its worker moved past it: that one is not kept.
            if st.items.iter().any(|item| item.key() == key) {
                st.failures.insert(key, error);
            }
            Task::none()
        }
        Msg::Focus(on) => {
            st.focused = on;
            Task::none()
        }
    }
}

/// A desktop notification for each new question and each delivery ready to
/// land, through the same plumbing and the same settings as the panes' own:
/// off with « Notifier », and kept for when the window is not in front unless
/// asked otherwise. A click lands on the workspace's orchestrator.
fn notices(st: &mut State, m: &Model, seen: &Look) -> Task<Message> {
    let (memory, fresh) = model::notice_delta(&st.memory, &seen.items, &seen.loaded);
    st.memory = memory;
    let settings = m.settings();
    if fresh.is_empty() || !settings.notify_on_idle {
        return Task::none();
    }
    if settings.notify_only_when_away && st.focused {
        return Task::none();
    }
    let tr = Translator::new(m.locale());
    Task::batch(fresh.iter().filter_map(|item| {
        let (title, kind) = match item {
            NeedsYouItem::Question { task, .. } => (&task.title, Key::NeedsYouNotifyQuestion),
            NeedsYouItem::Land { task, .. } => (&task.title, Key::NeedsYouNotifyLand),
            NeedsYouItem::Waiting { .. } => return None,
        };
        let ws = m.workspace(&item.base().workspace_id)?;
        let target = orchestrator_of(ws).or_else(|| ws.panes.first());
        Some(Task::done(Message::Intent(Intent::Notify(Notification {
            title: tr
                .t_with(
                    Key::NeedsYouNotifyTitle,
                    &args!("workspace" => item.base().workspace_name.as_str()),
                )
                .into_owned(),
            body: tr
                .t_with(kind, &args!("title" => title.as_str()))
                .into_owned(),
            target: target.map(|pane| (ws.id.clone(), pane.id.clone())),
        }))))
    }))
}

/// The beat, while there is anything to watch (a workspace that orchestrates,
/// a list on screen, a count the shell still holds), and the window's focus
/// for the notices. Someone who never pins an orchestrator pays for neither.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let focus = iced::event::listen_with(|event, _status, _window| match event {
        Event::Window(window::Event::Focused) => Some(Message::NeedsYou(Msg::Focus(true))),
        Event::Window(window::Event::Unfocused) => Some(Message::NeedsYou(Msg::Focus(false))),
        _ => None,
    });
    // `watched(m)` is not empty: without the sidebar's sort, as this runs
    // after every message.
    let watching = m.state().workspaces.iter().any(is_orchestrated)
        || !st.items.is_empty()
        || !st.landing.is_empty()
        || m.live.signals.needs_you != 0;
    if !watching {
        return focus;
    }
    Subscription::batch([
        focus,
        iced::time::every(BEAT).map(|_| Message::NeedsYou(Msg::Refresh)),
    ])
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(_st: &State, _m: &Model, _region: Region) -> Option<Task<Message>> {
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _intent: &Intent,
) -> Option<Task<Message>> {
    None
}

/// Nothing: the board package reads a board again when core says it was
/// written, and the next beat sees it.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}

/// The toolbar's « Besoin de toi » button and its panel (embedded by the
/// toolbar).
pub fn button<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    view::entry(st, ctx)
}

#[cfg(test)]
mod interaction;
#[cfg(test)]
mod tests;
