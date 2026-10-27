//! A workspace's orchestrator: the Claude Code pane pinned to plan on the
//! board, hand tasks to workers and land what they deliver.
//!
//! This module holds what the orchestrator and its workers are told: the role
//! each one is given in its system prompt, the state an orchestrator is handed
//! again after a compaction or a restart, and the events that wake it.
//! Workers themselves are started by the window (see lib/control.ts), and
//! deliveries land through delivery.rs.

/// The role section and the state a pane's mod adds to its session, or None
/// for a pane with no part in an orchestration.
pub fn briefing(_pane_id: &str) -> Option<serde_json::Value> {
    None
}
