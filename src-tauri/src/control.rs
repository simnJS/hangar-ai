//! Agents driving the panes: listing, creating, closing, restarting them,
//! typing into them and reading them back.
//!
//! The panes are not Rust's to touch. They live in the window's store — their
//! names, their agents, the split tree, the sessions to resume — and a pane
//! created behind the window's back would be a PTY nothing draws. So the HTTP
//! API only relays: each request goes to the main window as a
//! `control:request` event, the window carries it out against its store and
//! answers through `control_reply`, and the API hands that answer back to the
//! agent that asked.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

/// What the window knows how to do. Anything else is refused here, before the
/// window is bothered with it.
pub const OPERATIONS: &[&str] = &[
    "workspace_list",
    "pane_list",
    "pane_create",
    "pane_close",
    "pane_restart",
    "pane_send",
    "pane_read",
    "pane_focus",
    "workspace_window",
];

/// Long enough for a window that is busy repainting eight streaming panes;
/// short enough that an agent whose window is gone hears about it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);

type Answer = Result<Value, String>;

fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<Answer>>> {
    static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<Answer>>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

#[derive(Clone, Serialize)]
struct ControlRequest {
    id: String,
    op: String,
    args: Value,
}

#[derive(Debug, PartialEq)]
pub enum ControlError {
    /// The window looked at the request and said no: a pane that does not
    /// exist, a full workspace, the feature turned off. The caller's to fix.
    Refused(String),
    /// Nobody answered, which is the app's state rather than the request's.
    Unavailable(String),
}

/// Books a request and returns its id with the receiving half of its answer.
fn book() -> (String, oneshot::Receiver<Answer>) {
    let id = uuid::Uuid::new_v4().to_string();
    let (sender, receiver) = oneshot::channel();
    pending().lock().unwrap().insert(id.clone(), sender);
    (id, receiver)
}

fn forget(id: &str) {
    pending().lock().unwrap().remove(id);
}

async fn wait(
    id: &str,
    receiver: oneshot::Receiver<Answer>,
    limit: Duration,
) -> Result<Value, ControlError> {
    match tokio::time::timeout(limit, receiver).await {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(message))) => Err(ControlError::Refused(message)),
        Ok(Err(_)) => Err(ControlError::Unavailable(
            "the window dropped the request".to_string(),
        )),
        Err(_) => {
            forget(id);
            Err(ControlError::Unavailable(
                "Hangar's window did not answer in time — is it still open and responsive?"
                    .to_string(),
            ))
        }
    }
}

/// Sends `op` to the window and waits for what it did.
pub async fn ask(app: &AppHandle, op: &str, args: Value) -> Result<Value, ControlError> {
    if !OPERATIONS.contains(&op) {
        return Err(ControlError::Refused(format!("unknown operation '{op}'")));
    }
    let (id, receiver) = book();
    let request = ControlRequest {
        id: id.clone(),
        op: op.to_string(),
        args,
    };
    // Every window hears it; only the main one, which owns the store, answers.
    if let Err(err) = app.emit("control:request", request) {
        forget(&id);
        return Err(ControlError::Unavailable(format!(
            "could not reach the window: {err}"
        )));
    }
    wait(&id, receiver, ANSWER_TIMEOUT).await
}

/// The window's answer to a `control:request`. An id nobody waits for any
/// more — the request timed out — is dropped.
#[tauri::command]
pub fn control_reply(id: String, ok: bool, value: Option<Value>, error: Option<String>) {
    let Some(sender) = pending().lock().unwrap().remove(&id) else {
        return;
    };
    let answer = if ok {
        Ok(value.unwrap_or(Value::Null))
    } else {
        Err(error.unwrap_or_else(|| "the window refused without saying why".to_string()))
    };
    let _ = sender.send(answer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn a_reply_reaches_the_request_it_answers() {
        let (id, receiver) = book();
        control_reply(id.clone(), true, Some(json!({ "pane": "Ava" })), None);
        assert_eq!(
            wait(&id, receiver, Duration::from_secs(1)).await,
            Ok(json!({ "pane": "Ava" }))
        );
    }

    #[tokio::test]
    async fn a_refusal_carries_the_window_s_reason() {
        let (id, receiver) = book();
        control_reply(id.clone(), false, None, Some("no pane named Zed".into()));
        assert_eq!(
            wait(&id, receiver, Duration::from_secs(1)).await,
            Err(ControlError::Refused("no pane named Zed".into()))
        );
    }

    #[tokio::test]
    async fn a_silent_window_times_out_and_a_late_reply_is_dropped() {
        let (id, receiver) = book();
        let outcome = wait(&id, receiver, Duration::from_millis(20)).await;
        assert!(matches!(outcome, Err(ControlError::Unavailable(_))));
        assert!(!pending().lock().unwrap().contains_key(&id));
        // Must not panic, and must not resurrect anything.
        control_reply(id.clone(), true, None, None);
        assert!(!pending().lock().unwrap().contains_key(&id));
    }
}
