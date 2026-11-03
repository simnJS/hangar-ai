//! Orchestration roles (`src/lib/orchestration.ts`, tests in
//! orchestration.test.ts): `ORCHESTRATION_DEFAULTS` (the `Default` of
//! `OrchestrationSettings`), `orchestrationOf` (lenient), `orchestratorOf`,
//! `workersOf`, `managerOf`, `hqOf`, and the board labels.
//!
//! TODO(P0b): port the module with its tests.

use super::types::{Pane, PaneRole, Workspace};

/// The board label of a task that waits for the person.
pub const NEEDS_YOU_LABEL: &str = "needs-you";
/// The board label of a delivery that passed and waits to land.
pub const READY_TO_LAND_LABEL: &str = "ready-to-land";

/// The workspace's orchestrator pane, if it has one (`orchestratorOf`).
pub fn orchestrator_of(ws: &Workspace) -> Option<&Pane> {
    ws.panes
        .iter()
        .find(|pane| pane.role == Some(PaneRole::Orchestrator))
}
