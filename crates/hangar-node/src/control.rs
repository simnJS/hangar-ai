//! The orchestration of a node's workspaces, driven by the desktop
//! (docs/native/nodes.md, "Orchestration on a node").
//!
//! An agent of the node asks core's API for a control operation
//! (`worker_spawn`, `pane_send`, …) or core asks one of its own (a pane's
//! briefing): core emits `Event::ControlRequest` / `Event::ControlInternal`
//! and waits for `control_reply`, as it waits for the window on the desktop.
//! A node has no window: the desktop that drives it follows `GET
//! /node/events?control=1`, where the requests go out with their id, runs
//! them against the remote workspace and answers with `POST
//! /node/control/{id}/reply`.
//!
//! No desktop following the events with `control=1`: nobody will answer,
//! and the request is refused at once with [`UNAVAILABLE`] rather than left
//! to run out core's time. The desktop going away while a request it was
//! sent is under way does the same to that request: a desktop that comes
//! back never hears of it.
#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use hangar_core::{Event, Host};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::error::RecvError;

use crate::api::{error, Shared};

/// What an agent of the node is told when no desktop drives the node.
pub const UNAVAILABLE: &str = "Unavailable: the desktop driving this node is not connected. \
Orchestration on a node (workers, panes, deliveries) runs through the desktop Hangar that drives it: \
ask the person to open Hangar.AI on their computer and connect this node (Settings › Nodes), then try again.";

/// Longer than core waits for any operation (35 minutes for a
/// `workspace_create`): a request still booked after that was dropped by
/// core, and its entry goes.
const FORGET_AFTER: Duration = Duration::from_secs(40 * 60);

/// The desktops following the node's control requests, and the requests
/// sent to them that are not answered yet.
#[derive(Debug, Default)]
pub struct Drivers {
    state: Mutex<DriverState>,
}

#[derive(Debug, Default)]
struct DriverState {
    connected: usize,
    pending: HashMap<String, Instant>,
}

/// Held for as long as one desktop follows the control requests.
pub struct Driver {
    drivers: Arc<Drivers>,
}

impl Drop for Driver {
    fn drop(&mut self) {
        for id in self.drivers.detach() {
            hangar_core::control::control_reply(id, false, None, Some(UNAVAILABLE.into()));
        }
    }
}

impl Drivers {
    fn lock(&self) -> std::sync::MutexGuard<'_, DriverState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// One more desktop follows the requests.
    pub fn attach(self: &Arc<Self>) -> Driver {
        self.lock().connected += 1;
        Driver {
            drivers: self.clone(),
        }
    }

    /// One desktop less: when it was the last, the requests still waiting
    /// on an answer are handed back, to be refused.
    fn detach(&self) -> Vec<String> {
        let mut state = self.lock();
        state.connected = state.connected.saturating_sub(1);
        if state.connected > 0 {
            return Vec::new();
        }
        state.pending.drain().map(|(id, _)| id).collect()
    }

    pub fn connected(&self) -> usize {
        self.lock().connected
    }

    /// A request core just emitted: booked when a desktop will hear of it,
    /// `false` when none follows.
    fn book(&self, id: &str) -> bool {
        let mut state = self.lock();
        if state.connected == 0 {
            return false;
        }
        let now = Instant::now();
        state
            .pending
            .retain(|_, since| now.duration_since(*since) < FORGET_AFTER);
        state.pending.insert(id.to_owned(), now);
        true
    }

    /// The desktop answered request `id`.
    fn answered(&self, id: &str) -> bool {
        self.lock().pending.remove(id).is_some()
    }
}

/// Watches core's control requests for as long as the node runs: those no
/// desktop will hear of are refused at once.
pub fn watch(host: &Host, drivers: Arc<Drivers>) {
    let mut receiver = host.subscribe();
    tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(Event::ControlRequest(request) | Event::ControlInternal(request)) => {
                    if !drivers.book(&request.id) {
                        hangar_core::control::control_reply(
                            request.id,
                            false,
                            None,
                            Some(UNAVAILABLE.into()),
                        );
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    });
}

/// `POST /node/control/{id}/reply`'s body: what the window's
/// `control_reply` takes.
#[derive(Deserialize)]
pub struct ReplyBody {
    ok: bool,
    #[serde(default)]
    value: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

/// The desktop's answer to request `id`, handed to core as the window's.
/// One core no longer waits for (it timed out) is dropped, as core drops it.
pub async fn reply(
    State(node): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ReplyBody>,
) -> Response {
    if id.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "a reply names its request");
    }
    let waited = node.drivers.answered(&id);
    hangar_core::control::control_reply(id, body.ok, body.value, body.error);
    Json(json!({ "ok": true, "waited": waited })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_booked_only_while_a_desktop_follows() {
        let drivers = Arc::new(Drivers::default());
        assert!(!drivers.book("r1"), "nobody follows yet");
        let first = drivers.attach();
        let second = drivers.attach();
        assert!(drivers.book("r2"));
        assert!(drivers.book("r3"));
        assert!(drivers.answered("r2"));
        assert!(!drivers.answered("r2"), "answered once");
        drop(first);
        assert_eq!(drivers.connected(), 1);
        assert!(
            drivers.lock().pending.contains_key("r3"),
            "one still follows"
        );
        drop(second);
        assert_eq!(drivers.connected(), 0);
        assert!(drivers.lock().pending.is_empty(), "handed back, refused");
        assert!(!drivers.book("r4"));
    }
}
