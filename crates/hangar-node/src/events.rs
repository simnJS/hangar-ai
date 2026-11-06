//! Core's events (hangar-core's host.rs) as the JSON `GET /node/events`
//! sends: `{"event": <the name the window listened to>, …}`, with what the
//! event carries beside it.
#![cfg(target_os = "linux")]

use hangar_core::Event;
use serde_json::{json, Value};

pub fn to_json(event: &Event) -> Value {
    let name = event.name();
    match event {
        Event::BoardChanged { cwd } => json!({ "event": name, "cwd": cwd }),
        Event::BridgeEvent { pane_id, event } => {
            json!({ "event": name, "paneId": pane_id, "payload": event })
        }
        Event::ControlRequest(request) | Event::ControlInternal(request) => {
            json!({ "event": name, "payload": request })
        }
        Event::BridgeQueue(queue) => json!({ "event": name, "payload": queue }),
        Event::GitCloneProgress(progress) => json!({ "event": name, "payload": progress }),
        Event::MemoryChanged
        | Event::RemoteChanged
        | Event::KitsChanged
        | Event::AccountsChanged => json!({ "event": name }),
        // The desktop-only events, there when another crate of the
        // workspace builds core with `desktop`: a node never sends them.
        #[allow(unreachable_patterns)]
        _ => json!({ "event": name }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_carry_their_name_and_payload() {
        assert_eq!(
            to_json(&Event::BoardChanged { cwd: "/w".into() }),
            json!({ "event": "board:changed", "cwd": "/w" })
        );
        assert_eq!(
            to_json(&Event::MemoryChanged),
            json!({ "event": "memory:changed" })
        );
        assert_eq!(
            to_json(&Event::BridgeEvent {
                pane_id: "p1".into(),
                event: json!({ "kind": "stop" })
            }),
            json!({ "event": "bridge:event", "paneId": "p1", "payload": { "kind": "stop" } })
        );
    }
}
