//! The shapes the engine shares with the control bus (P18): who calls an op,
//! what an op tells the person on screen, and what a worker's pane is added
//! with.
//!
//! `Caller` and the announcements are P18's (`control::types`), re-exported
//! here so the engine's code names them where it always did; there is one of
//! each. A new pane stays described here as data (`NewPane`, `WorkerInit`),
//! which the engine's tests read back, and becomes P18's
//! `ModelCmd::AddPane` in live.rs.

use serde::Serialize;

pub use crate::control::types::{Caller, ControlEvent as Announcement};
use crate::state::types::{AgentId, PaneId, PaneRole};

/// What a worker pane is created with, before its shell starts: its role,
/// its task, its model and its ports, which its terminal needs from the first
/// spawn (`HANGAR_PORT_BASE`). The TS patched the pane in the same tick as it
/// added it; here it rides in the add itself (`StoreAction::AddPane{init}`,
/// inventory A6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerInit {
    pub role: PaneRole,
    pub task_id: String,
    pub model: String,
    pub port_base: u32,
    /// A worker in the workspace's own folder (no git): absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_folder: Option<bool>,
    /// The node a worker sent to one runs on: absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

/// A pane an engine asks the model to add (`addPane(ws, {agent, cwd, name,
/// near})` plus the worker's init).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewPane {
    pub agent: AgentId,
    pub cwd: String,
    pub name: String,
    pub near: Option<PaneId>,
    #[serde(skip)]
    pub init: Option<WorkerInit>,
}
