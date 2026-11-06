//! P18: the control bus and the agents' operations.
//!
//! Ports lib/{control,managerControl,deliveryControl,deliveryRuns}.ts
//! (inventory-frontend.md §5/1 and Appendix A6). Agents drive Hangar over
//! MCP: core's server relays each call as a control request, the shell hands
//! it here as `Msg::Request`, and the operation runs on tokio against the
//! port (port.rs), never against `Model`. It answers through the request's
//! `Reply`; core times it out on its own side, with the per-operation
//! timeouts of §5/1.0 (control.rs: 20 s, 60 s for `model_*`, `preview_*` and
//! `project_*`, 120 s for `worker_*`, 35 min for `workspace_create`).
//!
//! Every message an operation refuses with is English read by a model, and
//! says what to do next. The tests hold them word for word, as the TS ones
//! did: agents read them, and some match on them.
//!
//! - `port.rs`: what an operation reaches (`AppPort`, `ModelCmd`, the hooks
//!   P8, P15, P16, P19 and P20 install);
//! - `types.rs`: the values the engines share;
//! - `run.rs`: the dispatch, the gate, which workspace and pane a request
//!   means; `paths.rs`: paths as agents give them;
//! - `panes.rs`, `preview.rs`, `kit_tabs.rs`, `workspace.rs`, `manager.rs`,
//!   `delivery.rs` (+ `delivery_text.rs`): the operations;
//! - `writes.rs`: the model writes, carried out on the UI thread;
//! - `live.rs`: the port over the app's `Services`;
//! - `remote.rs`: the requests of a node's agents, run against the node's
//!   workspaces and answered on the node (`Msg::Node`).

mod board_relay;
pub mod delivery;
pub mod delivery_text;
mod engines;
mod kit_tabs;
mod live;
mod manager;
mod panes;
mod paths;
pub mod port;
mod preview;
pub mod remote;
mod run;
pub mod types;
mod workspace;
mod writes;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use iced::{Subscription, Task};
use serde_json::Value;

use hangar_i18n::{Key, Locale, args, t_with};

use crate::message::{Intent, Message};
use crate::services::{Channel, ControlRequest, CoreEvent, Services, inbox::Answer};
use crate::shell::Notice;
use crate::state::live::Delivering;
use crate::state::roles::READY_TO_LAND_LABEL;
use crate::state::{Model, StoreOutcome, Workspace, WsId};

#[allow(unused_imports)]
pub use port::{AppPort, CmdOut, Hook, ModelCmd, Port};
#[allow(unused_imports)]
pub use run::{ASLEEP, DISABLED};

/// The package's UI state; lives in `App.pkg.control`.
#[derive(Default)]
pub struct State {
    /// The hooks installed and the deliveries' state, shared with the port.
    shared: Arc<live::Shared>,
    /// The port the operations run against, made on the first request.
    port: Option<Port>,
    /// P19's runtime is installed on the bus.
    engines: bool,
}

impl State {
    /// The port the operations run against, made on first use: what
    /// `mobile::desk`'s calls take, as the agents' requests do.
    pub fn port(&mut self, sv: &Services) -> Port {
        self.port
            .get_or_insert_with(|| {
                Arc::new(live::LivePort::new(sv.clone(), self.shared.clone())) as Port
            })
            .clone()
    }

    /// P19's runtime starts on hydration and leaves its handle in
    /// `Model.live.orchestration`: from then on it answers `worker_*`, the
    /// briefing, the projects, and hears the deliveries' events.
    fn sync_engines(&mut self, m: &Model) {
        if self.engines {
            return;
        }
        if let Some(handle) = m.live.orchestration.handle() {
            self.shared
                .install(Hook::Orchestration(Arc::new(handle.clone())));
            self.engines = true;
        }
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("port", &self.port.is_some())
            .finish()
    }
}

/// The package's shared read-model; lives in `Model.live.control`.
///
/// The deliveries running (`deliveryRuns.ts`, and deliveryControl's
/// `deliveringIn`): « Fusionner » shows its spinner from it, and hibernation
/// keeps their workspaces awake. The snapshot's `delivering` is meant to be
/// made from `delivering()`.
#[derive(Default)]
pub struct Live {
    /// How many holders each task's delivery has, by workspace: the person's
    /// landing and the engine's own run of it end separately.
    runs: HashMap<WsId, HashMap<String, u32>>,
    /// The port the operations run against, once another package's handler
    /// is installed on the bus (`Msg::Install`): what the window's own calls
    /// to `mobile::desk` take (the chat view).
    port: Option<Port>,
}

impl std::fmt::Debug for Live {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live")
            .field("runs", &self.runs)
            .field("port", &self.port.is_some())
            .finish()
    }
}

impl Live {
    /// The port, for `mobile::desk`'s calls: `None` until a handler is
    /// installed on the bus, Hangar Mobile's among them.
    pub fn port(&self) -> Option<Port> {
        self.port.clone()
    }

    /// Whether a delivery of this workspace is running (`isDelivering`).
    pub fn is_delivering(&self, ws: &str) -> bool {
        self.runs.get(ws).is_some_and(|tasks| !tasks.is_empty())
    }

    /// Whether this task's delivery is running (`useDelivering`).
    pub fn task_delivering(&self, ws: &str, task: &str) -> bool {
        self.runs
            .get(ws)
            .is_some_and(|tasks| tasks.contains_key(task))
    }

    /// The deliveries running, for the snapshot (`Snapshot::delivering`).
    pub fn delivering(&self) -> Delivering {
        self.runs
            .iter()
            .filter(|(_, tasks)| !tasks.is_empty())
            .map(|(ws, tasks)| (ws.clone(), tasks.keys().cloned().collect()))
            .collect()
    }

    fn begin(&mut self, ws: WsId, task: String) {
        *self.runs.entry(ws).or_default().entry(task).or_default() += 1;
    }

    fn end(&mut self, ws: &str, task: &str) {
        let Some(tasks) = self.runs.get_mut(ws) else {
            return;
        };
        if let Some(count) = tasks.get_mut(task) {
            *count -= 1;
            if *count == 0 {
                tasks.remove(task);
            }
        }
        if tasks.is_empty() {
            self.runs.remove(ws);
        }
    }
}

/// The package's messages, routed by the shell as `Message::Control(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A control request from core (an agent's, a briefing, a phone's call).
    /// Before hydration the shell answers it itself and it never comes here.
    Request(ControlRequest),
    /// A model write from an engine (`AppPort::apply`), answered on the UI
    /// thread once applied.
    Model(ModelCmd, Answer<Result<StoreOutcome, String>>),
    /// Another package's handler for its part of the bus, sent once it has
    /// hydrated (port.rs, "For P19 and P20").
    Install(Hook),
    /// A control request of a node's core (an agent of the node, or a
    /// briefing it asks), forwarded on its events (`nodes::follow_events`):
    /// answered on that node.
    Node(NodeRequest),
}

/// A request a node forwarded, with what to answer it with.
#[derive(Debug, Clone)]
pub struct NodeRequest {
    pub node: String,
    pub id: String,
    pub op: String,
    pub args: Value,
    pub channel: Channel,
}

/// Said to a node's agent before the state is in.
const STARTING: &str =
    "Hangar.AI is still starting on the person's computer. Try again in a moment.";

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    st.sync_engines(m);
    match msg {
        Msg::Request(request) => {
            let port = st.port(sv);
            // On tokio, never here: an operation waits on the viewer, on git,
            // on the person (a clone's consent), for up to 35 minutes.
            Task::future(async move {
                handle(request, port).await;
                Message::Noop
            })
        }
        Msg::Model(cmd, answer) => writes::carry_out(m, cmd, answer),
        Msg::Install(hook) => {
            st.shared.install(hook);
            m.live.control.port = Some(st.port(sv));
            Task::none()
        }
        Msg::Node(request) => {
            let ready = m.live.hydrated;
            // A worker a local workspace sent to this node is that
            // workspace's; anything else of the node is the node's own.
            let port: Port = match remote::away_workspace(m.state(), &request.node, &request.args) {
                Some(ws) => Arc::new(remote::AwayPort::new(st.port(sv), ws)),
                None => Arc::new(remote::RemotePort::new(st.port(sv), request.node.clone())),
            };
            Task::future(async move {
                let answer = match remote::refused(&request.op) {
                    _ if !ready => Err(STARTING.to_string()),
                    Some(refusal) => Err(refusal.to_string()),
                    None => answer(&request.op, request.args, request.channel, &port).await,
                };
                if let Err(error) =
                    crate::nodes::engine::control_reply(&request.node, &request.id, answer).await
                {
                    eprintln!(
                        "the answer to {} did not reach node {}: {error}",
                        request.op, request.node
                    );
                }
                Message::Noop
            })
        }
    }
}

/// One request, answered (inventory A6, "Async shape", step 3).
async fn handle(request: ControlRequest, port: Port) {
    let answer = answer(&request.op, request.args.clone(), request.channel, &port).await;
    request.reply.send(answer);
}

async fn answer(op: &str, args: Value, channel: Channel, port: &Port) -> Result<Value, String> {
    match channel {
        Channel::Agent => run::run(op, args, port, run::Gate::Agent).await,
        // Hangar's own requests: never gated by the setting.
        Channel::Internal if op == "orchestrator_briefing" => match port.orchestration() {
            Some(engine) => engine.briefing(port.clone(), args).await,
            None => Err("The orchestration engine is not running yet.".into()),
        },
        Channel::Internal if op.starts_with("remote_") => match port.mobile() {
            Some(mobile) => mobile.remote(port.clone(), op.to_string(), args).await,
            None => Err("Hangar Mobile is not ready in this window yet.".into()),
        },
        Channel::Internal => Err(run::unknown(op)),
    }
}

/// An operation run as the person, from the phone (`runRemote`): the same
/// operations, never refused for the setting. P20's `pane_read` and
/// `pane_send` go through here.
pub async fn run_as_person(op: &str, args: Value, port: &Port) -> Result<Value, String> {
    run::run(op, args, port, run::Gate::Person).await
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = (st, m);
    Subscription::none()
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. « Fusionner » (`Intent::Land`) is ours: the delivery
/// runs for as long as it takes, and says so (App.tsx's `landItem`).
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    st.sync_engines(m);
    let Intent::Land { ws, task } = intent else {
        return None;
    };
    let workspace = m.workspace(ws)?.clone();
    // A workspace on a node lands there.
    let port = match &workspace.node {
        Some(node) => Arc::new(remote::RemotePort::new(st.port(sv), node.clone())) as Port,
        None => st.port(sv),
    };
    let landing = land(workspace, task.clone(), port, m.locale());
    Some(Task::future(landing).then(|said| Task::batch(said.into_iter().map(Task::done))))
}

/// The key « Besoin de toi » gives a delivery's row (`needs_you::model`), so
/// a failure reaches that row.
fn land_key(workspace: &Workspace, task: &str) -> String {
    format!("task:{}:{task}:{READY_TO_LAND_LABEL}", workspace.cwd)
}

/// Lands a delivery for the person, and what is then said: a notice either
/// way, and on a failure the row's own word for it (`LandFailed`), as the
/// TS's `landItem` threw back to its row. The notice is said too: a landing
/// refused because its worker moved past it leaves the list, and its row
/// with it.
async fn land(workspace: Workspace, task: String, port: Port, locale: Locale) -> Vec<Message> {
    let title = match port.delivery() {
        Some(delivery) => delivery
            .board_load(&workspace.cwd)
            .await
            .ok()
            .and_then(|board| board.tasks.into_iter().find(|entry| entry.id == task))
            .map_or_else(|| task.clone(), |entry| entry.title),
        None => task.clone(),
    };
    let mut run = delivery::begin_delivery(&port, &workspace.id, &task).await;
    let landed = delivery::land_delivery(&workspace, &task, &port).await;
    run.end().await;
    match landed {
        Ok(()) => vec![Message::Intent(Intent::Notice(Notice::Agent(
            t_with(
                locale,
                Key::NeedsYouLanded,
                &args!("title" => title.as_str()),
            )
            .into_owned(),
        )))],
        Err(error) => vec![
            Message::Intent(Intent::Notice(Notice::Problem(
                t_with(
                    locale,
                    Key::NeedsYouLandFailed,
                    &args!("error" => error.as_str()),
                )
                .into_owned(),
            ))),
            Message::NeedsYou(crate::needs_you::Msg::LandFailed {
                key: land_key(&workspace, &task),
                error,
            }),
        ],
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    // The first events after hydration come before any request: the runtime
    // is on the bus by the time an agent asks.
    st.sync_engines(m);
    let _ = (sv, event);
    None
}
