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
//! - `live.rs`: the port over the app's `Services`.

pub mod delivery;
pub mod delivery_text;
mod kit_tabs;
mod live;
mod manager;
mod panes;
mod paths;
pub mod port;
mod preview;
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

use hangar_i18n::{Key, args, t_with};

use crate::message::{Intent, Message};
use crate::services::{Channel, ControlRequest, CoreEvent, Services, inbox::Answer};
use crate::shell::{Notice, Region};
use crate::state::live::Delivering;
use crate::state::{Model, StoreOutcome, WsId};

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
}

impl State {
    fn port(&mut self, sv: &Services) -> Port {
        self.port
            .get_or_insert_with(|| {
                Arc::new(live::LivePort::new(sv.clone(), self.shared.clone())) as Port
            })
            .clone()
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
#[derive(Debug, Default)]
pub struct Live {
    /// How many holders each task's delivery has, by workspace: the person's
    /// landing and the engine's own run of it end separately.
    runs: HashMap<WsId, HashMap<String, u32>>,
}

impl Live {
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
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
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
            Task::none()
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

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
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
    let Intent::Land { ws, task } = intent else {
        return None;
    };
    let workspace = m.workspace(ws)?.clone();
    let port = st.port(sv);
    let locale = m.locale();
    let task = task.clone();
    Some(Task::future(async move {
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
        let notice = match landed {
            Ok(()) => Notice::Agent(
                t_with(
                    locale,
                    Key::NeedsYouLanded,
                    &args!("title" => title.as_str()),
                )
                .into_owned(),
            ),
            Err(error) => Notice::Problem(
                t_with(
                    locale,
                    Key::NeedsYouLandFailed,
                    &args!("error" => error.as_str()),
                )
                .into_owned(),
            ),
        };
        Message::Intent(Intent::Notice(notice))
    }))
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}
