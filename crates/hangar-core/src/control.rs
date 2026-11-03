//! Agents driving the panes: listing, creating, closing, restarting them,
//! typing into them and reading them back — and showing the user a 3D model,
//! or any other file, in the window's preview, which lives in the same store.
//!
//! The panes are not Rust's to touch. They live in the window's store — their
//! names, their agents, the split tree, the sessions to resume — and a pane
//! created behind the window's back would be a PTY nothing draws. So the HTTP
//! API only relays: each request goes to the window as an
//! `Event::ControlRequest` (the Tauri window's `control:request`), the window
//! carries it out against its store and answers through `control_reply`, and
//! the API hands that answer back to the agent that asked.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::host::{Event, Host};

/// What the window knows how to do. Anything else is refused here, before the
/// window is bothered with it.
pub const OPERATIONS: &[&str] = &[
    "workspace_list",
    // a repository to a workspace and its orchestrator (lib/workspaceFromGit)
    "workspace_create",
    "workspace_set_check",
    "pane_list",
    "pane_create",
    "pane_close",
    "pane_restart",
    "pane_send",
    "pane_read",
    "pane_focus",
    "workspace_window",
    "model_show",
    "model_snapshot",
    "model_close",
    "preview_show",
    "preview_close",
    // a kit's tabs (lib/control.ts)
    "kit_tab_list",
    "kit_tab_open",
    "kit_tab_close",
    // orchestration: workers (lib/workerControl.ts)
    "worker_spawn",
    "worker_list",
    "worker_stop",
    // orchestration: delivery (lib/deliveryControl.ts)
    "delivery_submit",
    "delivery_status",
    // orchestration: the project manager (lib/managerControl.ts)
    "project_list",
    "project_status",
    "project_send",
];

/// Long enough for a window that is busy repainting eight streaming panes;
/// short enough that an agent whose window is gone hears about it.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);

/// The 3D preview's operations wait on the viewer, and a model's first load
/// — parsing a few hundred megabytes of FBX, uploading it to the GPU — takes
/// longer than anything a pane does; a snapshot of a model still loading waits
/// for that load first. The window stops waiting on the viewer before this —
/// 45 s for a show, 40 s for a snapshot, in src/lib/control.ts — and answers
/// with what it has, so this only runs out when the window itself has stopped
/// answering. preview_show opens a model as model_show does, and is given as
/// long.
const MODEL_ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// Starting a worker checks out a worktree of the whole repository, and
/// stopping one waits for its processes to let go of that worktree before
/// removing it: on a large repository, longer than a pane operation.
const WORKER_ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

/// project_list and project_status read every project's board: longer than a
/// pane operation, far short of a worktree.
const PROJECT_ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// workspace_create waits for the person to decide (3 minutes, in
/// src/lib/workspaceFromGit.ts), then for a clone, which git_clone.rs gives up
/// on after 30 minutes. The window answers before this does; it only runs out
/// when the window has stopped answering.
const WORKSPACE_CREATE_ANSWER_TIMEOUT: Duration = Duration::from_secs(35 * 60);

fn answer_timeout(op: &str) -> Duration {
    if op == "workspace_create" {
        WORKSPACE_CREATE_ANSWER_TIMEOUT
    } else if op.starts_with("model_") || op.starts_with("preview_") {
        MODEL_ANSWER_TIMEOUT
    } else if op.starts_with("worker_") {
        WORKER_ANSWER_TIMEOUT
    } else if op.starts_with("project_") {
        PROJECT_ANSWER_TIMEOUT
    } else {
        ANSWER_TIMEOUT
    }
}

type Answer = Result<Value, String>;

fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<Answer>>> {
    static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<Answer>>>> = OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// One request as the window receives it, in `Event::ControlRequest` or
/// `Event::ControlInternal`; its answer comes back through `control_reply`.
#[derive(Debug, Clone, Serialize)]
pub struct ControlRequest {
    pub id: String,
    pub op: String,
    pub args: Value,
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
pub async fn ask(host: &Host, op: &str, args: Value) -> Result<Value, ControlError> {
    if !OPERATIONS.contains(&op) {
        return Err(ControlError::Refused(format!("unknown operation '{op}'")));
    }
    let (id, receiver) = book();
    let request = ControlRequest {
        id: id.clone(),
        op: op.to_string(),
        args,
    };
    // Every subscriber hears it; the window, which owns the store, answers
    // once. Nobody subscribed is no window to wait for.
    if let Err(err) = host.emit(Event::ControlRequest(request)) {
        forget(&id);
        return Err(ControlError::Unavailable(format!(
            "could not reach the window: {err}"
        )));
    }
    wait(&id, receiver, answer_timeout(op)).await
}

/// Asks the window one of Hangar's own questions — a pane's orchestration
/// briefing (orchestrator.rs), a phone's call (remote/) — which no agent may
/// ask. It goes out on an event of its own, never as an operation: no tool
/// offers it, the HTTP control route refuses it, and the window answers it
/// whether or not pane control is on. The answer comes back through
/// `control_reply` like any other.
pub async fn ask_internal(
    host: &Host,
    op: &str,
    args: Value,
    limit: Duration,
) -> Result<Value, ControlError> {
    ask_internal_via(|request| emit_internal(host, request), op, args, limit).await
}

/// How an internal request reaches the window: on its own event.
pub(crate) fn emit_internal(host: &Host, request: ControlRequest) -> Result<(), String> {
    host.emit(Event::ControlInternal(request))
        .map_err(|err| err.to_string())
}

/// `ask_internal`, with the request handed to `send` rather than emitted. The
/// mobile server goes through here (remote/), so that its tests can stand in
/// for the window, answering through `control_reply` as the window does.
pub(crate) async fn ask_internal_via(
    send: impl FnOnce(ControlRequest) -> Result<(), String>,
    op: &str,
    args: Value,
    limit: Duration,
) -> Result<Value, ControlError> {
    let (id, receiver) = book();
    let request = ControlRequest {
        id: id.clone(),
        op: op.to_string(),
        args,
    };
    if let Err(err) = send(request) {
        forget(&id);
        return Err(ControlError::Unavailable(format!(
            "could not reach the window: {err}"
        )));
    }
    wait(&id, receiver, limit).await
}

/// The window's answer to a control request, internal or not. An id nobody
/// waits for any more — the request timed out — is dropped.
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

    #[tokio::test]
    async fn an_internal_request_is_answered_through_control_reply() {
        let answer = ask_internal_via(
            |request| {
                assert_eq!(request.op, "remote_state");
                assert_eq!(request.args, json!({ "pane": "Ava" }));
                control_reply(request.id, true, Some(json!({ "panes": 1 })), None);
                Ok(())
            },
            "remote_state",
            json!({ "pane": "Ava" }),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(answer, Ok(json!({ "panes": 1 })));
    }

    #[tokio::test]
    async fn a_window_that_cannot_be_reached_is_unavailable_at_once() {
        let mut booked = String::new();
        let answer = ask_internal_via(
            |request| {
                booked = request.id;
                Err("no window".to_string())
            },
            "remote_state",
            json!({}),
            Duration::from_secs(30),
        )
        .await;
        assert!(matches!(answer, Err(ControlError::Unavailable(_))));
        assert!(!pending().lock().unwrap().contains_key(&booked));
    }

    fn host() -> Host {
        Host::with_paths(crate::paths::Paths::rooted(&std::env::temp_dir()))
    }

    #[tokio::test]
    async fn a_request_reaches_the_window_on_the_bus_and_comes_back_answered() {
        let host = host();
        let mut events = host.subscribe();
        let window = tokio::spawn(async move {
            match events.recv().await.unwrap() {
                Event::ControlRequest(request) => {
                    assert_eq!(request.op, "pane_list");
                    control_reply(request.id, true, Some(json!(["Ava"])), None);
                }
                other => panic!("unexpected {other:?}"),
            }
        });
        assert_eq!(ask(&host, "pane_list", json!({})).await, Ok(json!(["Ava"])));
        window.await.unwrap();
    }

    #[tokio::test]
    async fn an_internal_request_goes_out_on_its_own_event() {
        let host = host();
        let mut events = host.subscribe();
        let window = tokio::spawn(async move {
            match events.recv().await.unwrap() {
                Event::ControlInternal(request) => {
                    assert_eq!(request.op, "orchestrator_briefing");
                    control_reply(request.id, false, None, Some("no role".into()));
                }
                other => panic!("unexpected {other:?}"),
            }
        });
        assert_eq!(
            ask_internal(
                &host,
                "orchestrator_briefing",
                json!({}),
                Duration::from_secs(1)
            )
            .await,
            Err(ControlError::Refused("no role".into()))
        );
        window.await.unwrap();
    }

    /// No window subscribed: the agent hears so at once, rather than after
    /// the twenty seconds a window that does not answer is given.
    #[tokio::test]
    async fn a_request_nobody_listens_for_is_unavailable_at_once() {
        let outcome = ask(&host(), "pane_list", json!({})).await;
        assert!(matches!(outcome, Err(ControlError::Unavailable(_))));
    }

    #[test]
    fn the_preview_is_given_longer_than_the_panes() {
        for op in [
            "model_show",
            "model_snapshot",
            "model_close",
            "preview_show",
            "preview_close",
        ] {
            assert!(OPERATIONS.contains(&op), "{op} never reaches the window");
            assert_eq!(answer_timeout(op), Duration::from_secs(60), "{op}");
        }
        assert_eq!(answer_timeout("pane_read"), Duration::from_secs(20));
    }

    #[test]
    fn a_kit_s_tabs_reach_the_window_and_wait_for_nothing_it_loads() {
        for op in ["kit_tab_list", "kit_tab_open", "kit_tab_close"] {
            assert!(OPERATIONS.contains(&op), "{op} never reaches the window");
            assert_eq!(answer_timeout(op), Duration::from_secs(20), "{op}");
        }
    }

    #[test]
    fn the_project_manager_reading_every_board_is_given_time() {
        for op in ["project_list", "project_status", "project_send"] {
            assert!(OPERATIONS.contains(&op), "{op} never reaches the window");
            assert_eq!(answer_timeout(op), Duration::from_secs(60), "{op}");
        }
    }

    #[test]
    fn a_repository_to_a_workspace_waits_for_the_person_and_the_clone() {
        for op in ["workspace_create", "workspace_set_check"] {
            assert!(OPERATIONS.contains(&op), "{op} never reaches the window");
        }
        assert_eq!(
            answer_timeout("workspace_create"),
            Duration::from_secs(35 * 60)
        );
        // Setting a field is as quick as a pane operation.
        assert_eq!(
            answer_timeout("workspace_set_check"),
            Duration::from_secs(20)
        );
        assert_eq!(answer_timeout("workspace_list"), Duration::from_secs(20));
    }

    #[test]
    fn a_worker_s_worktree_is_given_time_to_check_out() {
        for op in ["worker_spawn", "worker_list", "worker_stop"] {
            assert!(OPERATIONS.contains(&op), "{op} never reaches the window");
            assert_eq!(answer_timeout(op), Duration::from_secs(120), "{op}");
        }
    }
}
