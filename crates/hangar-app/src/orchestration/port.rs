//! The shapes the engine shares with the control bus (P18): who calls an op,
//! what a model write from an engine carries, what an op tells the person on
//! screen.
//!
//! P18 owns these in `control/port.rs` and `control/types.rs` (inventory A6,
//! WP0: `Caller`, `ControlEvent`, `ModelCmd`). They were not written when this
//! package was, so the engine defines the few it needs here, under the same
//! names and with the same fields; when the two packages meet, these become
//! re-exports of P18's, or P18's are built from them.

use serde::Serialize;

use crate::state::types::{AgentId, PaneId, PaneRole};

/// Who calls a control op (`Caller` of lib/control.ts): from the agent's
/// `HANGAR_PANE_ID`/`HANGAR_PANE_NAME`, with its folder as a fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caller {
    pub pane_id: Option<PaneId>,
    pub pane_name: Option<String>,
    pub cwd: Option<String>,
}

/// The two kinds of `ControlEvent` the worker ops announce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceKind {
    Created,
    Closed,
}

/// A line on screen about a pane an op opened or closed (`ControlEvent`):
/// `control.created` / `control.closed`, with `by` (an agent's name, or the
/// person: `None`), the pane and its workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub kind: AnnounceKind,
    pub by: Option<String>,
    pub pane: String,
    pub workspace: String,
}

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
