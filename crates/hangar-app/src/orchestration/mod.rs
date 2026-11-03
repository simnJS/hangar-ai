//! P19: the orchestration engine.
//!
//! Ports lib/{workerControl,orchestratorEvents,orchestratorBrief,managerEvents,
//! projects}.ts (inventory-frontend.md §5/1.3, 1.5–1.7 and Appendix A6):
//!
//! - `worker`: the `worker_*` ops — a worker started on a task in a git
//!   worktree of its own (or the workspace's folder without git), listed,
//!   stopped — and the person's « Arrêter les workers »;
//! - `events`: what an orchestrator is told between its turns, batched;
//! - `manager_feed`: what the project manager is told waits for the person,
//!   and the phone's push beside it;
//! - `brief` and `brief_text`: the roles and the state a session is briefed
//!   with, at start and after every compaction;
//! - `projects`: every project summarized, for the manager;
//! - `runtime`: the one tokio task that owns the engines and drives them with
//!   the clock, and `OrchestrationHandle`, the way in for everyone else;
//! - `live`: the ports over the running app; `port`: the shapes shared with
//!   P18's control bus until its own land.
//!
//! The package never touches `Model` from the runtime: it reads
//! `Services::snapshot()` and writes through the inbox, as `Msg::Model`,
//! which `update` applies on the UI thread (PACKAGES.md, rule 6). It draws
//! nothing.

pub mod brief;
pub mod brief_text;
pub mod events;
pub mod live;
pub mod manager_feed;
pub mod port;
pub mod projects;
pub mod roles;
pub mod runtime;
pub mod text;
pub mod worker;

#[cfg(test)]
pub(crate) mod testkit;

use iced::futures::stream;
use iced::{Subscription, Task};

use hangar_i18n::{Key, args, t_with, tn};

pub use runtime::{EngineMsg, OrchestrationHandle};

use self::port::NewPane;
use crate::message::{Intent, Message};
use crate::services::inbox::Answer;
use crate::services::{CoreEvent, Services};
use crate::shell::{Notice, Region};
use crate::state::live::BridgeEvent;
use crate::state::types::{Pane, PaneId, PaneRole, WsId};
use crate::state::{Model, PanePatch, StoreAction, StoreOutcome};

/// The package's UI state; lives in `App.pkg.orchestration`: the runtime,
/// once started on hydration.
#[derive(Default)]
pub struct State {
    runtime: Option<OrchestrationHandle>,
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").field("running", &self.runtime.is_some()).finish()
    }
}

/// What other packages read; lives in `Model.live.orchestration`: the way
/// into the runtime (P18's `worker_*` ops, the briefing and deliveries' events;
/// P20's journals, projects and push).
#[derive(Default)]
pub struct Live {
    handle: Option<OrchestrationHandle>,
}

impl Live {
    /// The runtime, once the state is in.
    pub fn handle(&self) -> Option<&OrchestrationHandle> {
        self.handle.as_ref()
    }
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live").field("running", &self.handle.is_some()).finish()
    }
}

/// A model write the runtime asks for, applied by `update` on the UI thread.
/// Stands in for P18's `ModelCmd` (live.rs says how they meet).
#[derive(Debug, Clone)]
pub enum ModelWrite {
    /// A worker's pane, its role, task, model and ports set before its shell
    /// starts.
    AddPane { ws: WsId, pane: NewPane },
    ClosePane { ws: WsId, pane: PaneId },
    /// Its panes come up, as a visited workspace's do (`openWorkspace`).
    OpenWorkspace(WsId),
}

/// The package's messages, routed by the shell as `Message::Orchestration(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The state is in: the runtime starts.
    Start,
    Model(ModelWrite, Answer<StoreOutcome>),
    /// A pane was pinned or unpinned as orchestrator: its running session is
    /// told (`announceRole`). Sent after the pin, so the snapshot has it.
    AnnounceRole {
        ws: WsId,
        pane: PaneId,
        previous: Option<PaneRole>,
    },
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Start => {
            if st.runtime.is_none() && m.live.hydrated {
                let handle = runtime::start(sv.clone());
                m.live.orchestration.handle = Some(handle.clone());
                st.runtime = Some(handle);
            }
            Task::none()
        }
        Msg::Model(write, answer) => {
            answer.send(apply(m, write));
            Task::none()
        }
        Msg::AnnounceRole { ws, pane, previous } => match &st.runtime {
            Some(handle) => {
                let handle = handle.clone();
                Task::perform(
                    async move { handle.announce_role(&ws, &pane, previous).await },
                    |_| Message::Noop,
                )
            }
            None => Task::none(),
        },
    }
}

/// One of the runtime's writes, through the store.
fn apply(m: &mut Model, write: ModelWrite) -> StoreOutcome {
    match write {
        ModelWrite::AddPane { ws, pane } => {
            let init = pane.init.map(|init| {
                PanePatch::new(move |target: &mut Pane| {
                    target.role = Some(init.role);
                    target.task_id = Some(init.task_id.clone());
                    target.model = Some(init.model.clone());
                    target.port_base = Some(init.port_base);
                    if init.shared_folder.is_some() {
                        target.shared_folder = init.shared_folder;
                    }
                })
            });
            m.apply(StoreAction::AddPane {
                ws,
                agent: Some(pane.agent),
                near: pane.near,
                dir: None,
                cwd: Some(pane.cwd),
                name: Some(pane.name),
                init,
            })
        }
        ModelWrite::ClosePane { ws, pane } => m.apply(StoreAction::ClosePane { ws, pane }),
        ModelWrite::OpenWorkspace(ws) => {
            // What the shell's `ActivateWorkspace` does to the opened set,
            // without making the workspace the active one. P18's planned
            // `ModelCmd::OpenWorkspace` replaces this.
            if !m.ui.opened_ids.contains(&ws) {
                m.ui.opened_ids.push(ws);
            }
            StoreOutcome::default()
        }
    }
}

/// Once hydrated, one message starts the runtime; the subscription is gone
/// as soon as it has.
pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    if m.live.hydrated && st.runtime.is_none() {
        Subscription::run(|| stream::once(async { Message::Orchestration(Msg::Start) }))
    } else {
        Subscription::none()
    }
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    None
}

/// The intents this package acts on: the person's « Arrêter les workers »,
/// and a pin or an unpin, whose session is told.
pub fn on_intent(st: &mut State, m: &mut Model, sv: &Services, intent: &Intent) -> Option<Task<Message>> {
    // Before hydration there is no runtime, and nothing to stop or tell.
    st.runtime.as_ref()?;
    match intent {
        Intent::StopWorkers(ws) => {
            let handle = st.runtime.clone()?;
            let name = m.workspace(ws)?.name.clone();
            let locale = m.locale();
            let ws = ws.clone();
            Some(Task::perform(
                async move { handle.stop_all_workers(&ws).await },
                move |stopped| {
                    let text = if stopped > 0 {
                        tn(locale, Key::OrchStopped, stopped, &args!("workspace" => &name))
                    } else {
                        t_with(locale, Key::OrchStoppedNone, &args!("workspace" => &name))
                    };
                    Message::Intent(Intent::Notice(Notice::Agent(text.into_owned())))
                },
            ))
        }
        Intent::PinOrchestrator { ws, pane, pinned } => {
            // The snapshot is still the model before this update: whether the
            // pane was the orchestrator before the pin or the unpin.
            let before = sv.snapshot();
            let was = before
                .state
                .workspaces
                .iter()
                .find(|entry| entry.id == *ws)
                .and_then(|entry| entry.panes.iter().find(|p| p.id == *pane))
                .and_then(|p| p.role);
            let previous = match (pinned, was) {
                (true, Some(PaneRole::Orchestrator)) => return None,
                (true, _) => None,
                (false, Some(PaneRole::Orchestrator)) => Some(PaneRole::Orchestrator),
                (false, _) => return None,
            };
            Some(Task::done(Message::Orchestration(Msg::AnnounceRole {
                ws: ws.clone(),
                pane: pane.clone(),
                previous,
            })))
        }
        _ => None,
    }
}

/// The core events the engines watch: bridge reports, a pane's shell ending,
/// a board written.
pub fn on_event(st: &mut State, m: &mut Model, sv: &Services, event: &CoreEvent) -> Option<Task<Message>> {
    let handle = st.runtime.as_ref()?;
    match event {
        CoreEvent::Bridge { pane, event } => {
            // A kind this version does not know (a newer mod) is ignored, as
            // the TS ignored it.
            if let Some(event) = BridgeEvent::parse(event) {
                handle.send(EngineMsg::Bridge { pane: pane.clone(), event });
            }
        }
        CoreEvent::PtyExit { pane } => handle.send(EngineMsg::PtyExit(pane.clone())),
        CoreEvent::BoardChanged { .. } => handle.send(EngineMsg::BoardChanged),
        _ => {}
    }
    None
}
