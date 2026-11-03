//! P18: the control bus and the agents' operations.
//!
//! Ports lib/{control,managerControl,deliveryControl,deliveryRuns}.ts
//! (inventory-frontend.md §5/1 and Appendix A6). Agents drive Hangar over
//! MCP: core's server relays each call as a control request, the shell hands
//! it here as `Msg::Request`, and the operation runs on tokio against the
//! port (port.rs), never against `Model`. It answers through the request's
//! `Reply`; core times it out on its own side, with the per-operation
//! timeouts of §5/1.0 (control.rs).
//!
//! Every message an operation refuses with is English read by a model, and
//! says what to do next. The tests hold them word for word, as the TS ones
//! did: agents read them, and some match on them.
//!
//! - `port.rs`: what an operation reaches (`AppPort`, `ModelCmd`, the hooks
//!   P8, P15, P16, P19 and P20 install);
//! - `types.rs`: the values the engines share;
//! - `run.rs`: the dispatch, the gate, which workspace and pane a request
//!   means;
//! - `panes.rs`, `preview.rs`, `kit_tabs.rs`, `workspace.rs`, `manager.rs`,
//!   `delivery.rs` (+ `delivery_text.rs`): the operations;
//! - `writes.rs`: the model writes, carried out on the UI thread;
//! - `live.rs`: the port over the app's `Services`.

pub mod delivery;
pub mod port;
pub mod types;
mod writes;

use std::collections::HashMap;

use iced::{Subscription, Task};

use crate::message::{Intent, Message};
use crate::services::{ControlRequest, CoreEvent, Services, inbox::Answer};
use crate::shell::Region;
use crate::state::live::Delivering;
use crate::state::{Model, StoreOutcome, WsId};

#[allow(unused_imports)]
pub use port::{AppPort, CmdOut, Hook, ModelCmd, Port};

/// The package's UI state; lives in `App.pkg.control`.
#[derive(Default)]
pub struct State {
    /// The port the operations run against, made on the first request.
    port: Option<Port>,
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
/// The deliveries running (`deliveryRuns.ts`): « Fusionner » shows its
/// spinner from it, hibernation keeps their workspaces awake, and the
/// snapshot's `delivering` should be made from it.
#[derive(Debug, Default)]
pub struct Live {
    /// How many holders each task's delivery has, by workspace.
    runs: HashMap<WsId, HashMap<String, u32>>,
}

impl Live {
    /// Whether a delivery of this workspace is running (`isDelivering`).
    pub fn is_delivering(&self, ws: &str) -> bool {
        self.runs.get(ws).is_some_and(|tasks| !tasks.is_empty())
    }

    /// Whether this task's delivery is running (`useDelivering`).
    pub fn task_delivering(&self, ws: &str, task: &str) -> bool {
        self.runs.get(ws).is_some_and(|tasks| tasks.contains_key(task))
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

/// Until the operations land, an agent's request is answered at once rather
/// than left to time out.
const NOT_YET: &str = "pane control is not available in this build of Hangar yet";

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let _ = (&st.port, sv);
    match msg {
        Msg::Request(request) => {
            request.reply.send(Err(NOT_YET.into()));
            Task::none()
        }
        Msg::Model(cmd, answer) => writes::carry_out(m, cmd, answer),
        Msg::Install(_hook) => Task::none(),
    }
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
/// package acted on it.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, intent);
    None
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
