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
//! Boards are read here, for the workspaces that orchestrate, and again
//! whenever core says a board was written.
//!
//! TODO(P12): the board cache is to be P12's (`board::Live`), read by this
//! package, the inspector and the orchestration engine. It is an empty struct
//! today, so this keeps its own copy of the boards it watches; replacing
//! `State::boards` with a read of P12's is the whole change.

pub mod model;
mod view;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use iced::futures::{Stream, StreamExt, stream};
use iced::{Element, Event, Subscription, Task, window};

use hangar_core::board::Task as BoardTask;
use hangar_i18n::{Key, Translator, args};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::navigator::model::sidebar_order;
use crate::navigator::reads;
use crate::services::{CoreEvent, Services};
use crate::shell::{Notification, Region};
use crate::state::roles::orchestrator_of;

use crate::state::{AgentActivity, Model};

use model::{Boards, NeedsYouItem, NoticeMemory, is_orchestrated, watched_boards};

/// How long « Land » says it is landing before it lets the person try again:
/// the delivery's own state is `control::Live`'s, which does not exist yet, and
/// a refusal that never says so must not leave the button dead for good.
const LAND_PATIENCE: Duration = Duration::from_secs(45);

/// The package's UI state; lives in `App.pkg.needs_you`.
#[derive(Debug)]
pub struct State {
    /// The tasks of each board watched, once read.
    boards: Boards,
    /// What « Fusionner » last failed with, by item key.
    failures: HashMap<String, String>,
    /// The deliveries asked to land, and when.
    landing: HashMap<String, Instant>,
    /// What the notices have already told.
    memory: NoticeMemory,
    /// The window has the system's focus (`document.hasFocus()`).
    focused: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            boards: Boards::new(),
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

    /// Whether a delivery is being landed right now, from here.
    pub(crate) fn is_landing(&self, key: &str, now: Instant) -> bool {
        self.landing
            .get(key)
            .is_some_and(|since| now.saturating_duration_since(*since) < LAND_PATIENCE)
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
    /// These boards have to be read (the set watched changed).
    Load(Vec<String>),
    Loaded(String, Vec<BoardTask>),
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

fn items_of(st: &State, m: &Model) -> Vec<NeedsYouItem> {
    let state = m.state();
    let workspaces = sidebar_order(&state.workspaces, &state.folders);
    if !workspaces.iter().any(|ws| is_orchestrated(ws)) {
        return Vec::new();
    }
    let waiting = workspaces
        .iter()
        .flat_map(|ws| ws.panes.iter())
        .filter(|pane| reads::activity(m, &pane.id) == AgentActivity::Waiting)
        .map(|pane| pane.id.clone())
        .collect();
    model::needs_you_items(&workspaces, &waiting, &st.boards)
}

fn watched(m: &Model) -> Vec<String> {
    let state = m.state();
    watched_boards(&sidebar_order(&state.workspaces, &state.folders))
}

fn load(cwds: Vec<String>) -> Task<Message> {
    Task::batch(cwds.into_iter().map(|cwd| {
        Task::perform(
            async move {
                let for_read = cwd.clone();
                let board =
                    tokio::task::spawn_blocking(move || hangar_core::board_load(for_read)).await;
                (cwd, board.map(|board| board.tasks).ok())
            },
            |(cwd, tasks)| match tasks {
                // No board yet, or a read that failed: nothing to show rather
                // than an error.
                Some(tasks) => Message::NeedsYou(Msg::Loaded(cwd, tasks)),
                None => Message::Noop,
            },
        )
    }))
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Toggle => {
            m.ui.needs_you_open = !m.ui.needs_you_open;
            Task::none()
        }
        Msg::Close => {
            m.ui.needs_you_open = false;
            Task::none()
        }
        Msg::Load(cwds) => load(cwds),
        Msg::Loaded(cwd, tasks) => {
            st.boards.insert(cwd, tasks);
            let items = items_of(st, m);
            st.forget_gone(&items);
            notices(st, m, &items)
        }
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
            if items_of(st, m).iter().any(|item| item.key() == key) {
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
fn notices(st: &mut State, m: &Model, items: &[NeedsYouItem]) -> Task<Message> {
    let loaded: HashSet<String> = watched(m)
        .into_iter()
        .filter(|cwd| st.boards.contains_key(cwd))
        .collect();
    let (memory, fresh) = model::notice_delta(&st.memory, items, &loaded);
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

/// The boards to read are those of the workspaces that orchestrate: a
/// subscription for each set of them, which reads them when the set changes
/// (a workspace starts to orchestrate, or stops), and the window's focus for
/// the notices.
pub fn subscription(_st: &State, m: &Model) -> Subscription<Message> {
    let focus = iced::event::listen_with(|event, _status, _window| match event {
        Event::Window(window::Event::Focused) => Some(Message::NeedsYou(Msg::Focus(true))),
        Event::Window(window::Event::Unfocused) => Some(Message::NeedsYou(Msg::Focus(false))),
        _ => None,
    });
    let cwds = watched(m);
    if cwds.is_empty() {
        return focus;
    }
    Subscription::batch([focus, Subscription::run_with(cwds, read_once)])
}

// `Subscription::run_with` hands the builder a reference to its data, which
// is the `Vec` the subscription is identified by.
#[allow(clippy::ptr_arg)]
fn read_once(cwds: &Vec<String>) -> impl Stream<Item = Message> + use<> {
    let cwds = cwds.clone();
    stream::once(async move { Message::NeedsYou(Msg::Load(cwds)) }).chain(stream::pending())
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

/// A board written by an agent or a worker is read again.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    _sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    match event {
        CoreEvent::BoardChanged { .. } => {
            let cwds: Vec<String> = watched(m)
                .into_iter()
                .filter(|cwd| st.boards.contains_key(cwd))
                .collect();
            (!cwds.is_empty()).then(|| load(cwds))
        }
        _ => None,
    }
}

/// The toolbar's « Besoin de toi » button and its panel (embedded by the
/// toolbar).
pub fn button<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    view::entry(st, ctx)
}

#[cfg(test)]
mod tests;
