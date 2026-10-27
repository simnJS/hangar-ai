//! A workspace's orchestrator: the Claude Code pane pinned to plan on the
//! board, hand tasks to workers and land what they deliver.
//!
//! This module holds what the orchestrator and its workers are told: the role
//! each one is given in its system prompt, the state an orchestrator is handed
//! again after a compaction or a restart, and the events that wake it.
//! Workers themselves are started by the window (see lib/control.ts), and
//! deliveries land through delivery.rs.
//!
//! Roles live in the window's store, and the board, the plan and the events
//! are read beside them, so the window writes the briefing
//! (src/lib/orchestratorBrief.ts) and this side only asks for it and checks
//! what comes back before a pane's mod builds a system prompt out of it.

use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::control::{self, ControlError};

/// A window that answers at all answers in milliseconds: the briefing is two
/// small file reads. A pane's session waits on it as it starts.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(8);

/// Far above anything the window writes; only a runaway is cut. The context is
/// handed back after every compaction, so it must never grow with the session.
const MAX_SYSTEM_CHARS: usize = 24_000;
const MAX_CONTEXT_CHARS: usize = 32_000;

/// The role section and the state a pane's mod adds to its session, or None
/// for a pane with no part in an orchestration.
pub async fn briefing(app: &AppHandle, pane_id: &str) -> Result<Option<Value>, ControlError> {
    let answer = control::ask_internal(
        app,
        "orchestrator_briefing",
        json!({ "paneId": pane_id }),
        ANSWER_TIMEOUT,
    )
    .await?;
    Ok(checked(&answer))
}

/// The window's answer in the one shape the mod relies on —
/// `{ role, system, context }` — or None, which is also what a pane with no
/// role is answered.
fn checked(answer: &Value) -> Option<Value> {
    let role = answer.get("role")?.as_str()?;
    if role != "orchestrator" && role != "worker" {
        return None;
    }
    let system = answer.get("system")?.as_str()?;
    if system.trim().is_empty() {
        return None;
    }
    let context = answer.get("context").and_then(Value::as_str).unwrap_or("");
    Some(json!({
        "role": role,
        "system": clip(system, MAX_SYSTEM_CHARS),
        "context": clip(context, MAX_CONTEXT_CHARS),
    }))
}

/// At most `max` characters, cut on a character boundary.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_briefing_keeps_its_three_fields() {
        let answer = json!({
            "role": "orchestrator",
            "system": "You coordinate.",
            "context": "Board: empty.",
            "extra": 1
        });
        assert_eq!(
            checked(&answer),
            Some(json!({
                "role": "orchestrator",
                "system": "You coordinate.",
                "context": "Board: empty."
            }))
        );
    }

    #[test]
    fn a_pane_without_a_role_gets_none() {
        for answer in [
            Value::Null,
            json!({}),
            json!({ "role": "reviewer", "system": "x", "context": "" }),
            json!({ "role": "worker", "system": "   ", "context": "" }),
            json!({ "role": "worker", "context": "y" }),
            json!({ "role": 3, "system": "x" }),
        ] {
            assert_eq!(checked(&answer), None, "{answer}");
        }
    }

    #[test]
    fn a_missing_context_is_an_empty_one() {
        let briefing = checked(&json!({ "role": "worker", "system": "Do the task." })).unwrap();
        assert_eq!(briefing["context"], "");
    }

    #[test]
    fn a_runaway_text_is_cut_on_a_character() {
        assert_eq!(clip("héllo", 2), "hé…");
        assert_eq!(clip("héllo", 5), "héllo");
        assert_eq!(clip("", 3), "");
        let long = "é".repeat(MAX_CONTEXT_CHARS + 10);
        let answer = json!({ "role": "worker", "system": "s", "context": long });
        let briefing = checked(&answer).unwrap();
        assert_eq!(
            briefing["context"].as_str().unwrap().chars().count(),
            MAX_CONTEXT_CHARS + 1
        );
    }
}
