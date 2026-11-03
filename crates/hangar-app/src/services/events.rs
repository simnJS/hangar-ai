//! What hangar-core tells the window (inventory §1.9), as messages.
//!
//! Core sends its events on a broadcast bus (`hangar_core::Host::subscribe`,
//! docs/native/core-api.md, "Events"). One subscription of the shell reads it
//! and turns each event into a message:
//!
//! - a control request (an agent's, or one of Hangar's own: an orchestrator's
//!   briefing, a phone's call) becomes `Message::Control(control::Msg::Request)`
//!   with a `Reply` that answers core once (P18 handles it, inventory A6);
//! - everything else becomes `Message::Core(CoreEvent)`, which the shell
//!   handles where it is its own (a notification clicked) and offers to every
//!   package's `on_event`.
//!
//! A pane's output and its shell's exit are not on the bus: each pane's
//! `PtySink` gets them (P5). `CoreEvent::PtyExit` is for P5 to send through
//! the inbox when a pane's shell ends by itself, since the orchestration
//! engine listened for it (inventory §1.9, `pty:exit`).

use std::sync::{Arc, Mutex};

use iced::Subscription;
use iced::futures::stream;
use serde_json::Value;
use tokio::sync::broadcast;

use hangar_core::host::Event;

use crate::message::Message;
use crate::state::{PaneId, live::QueuedPrompt};

/// One variant per backend event of inventory §1.9 but the control requests,
/// which go to `Message::Control`.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// A pane's shell ended by itself (`pty:exit`). Sent by P5, not by core.
    PtyExit {
        pane: PaneId,
    },
    /// A report of a pane's hangar-bridge mod, as the mod sent it;
    /// `state::BridgeEvent::parse` reads it (P4 owns its meaning).
    Bridge {
        pane: PaneId,
        event: Value,
    },
    /// What is queued for a pane, after it changed.
    BridgeQueue {
        pane: PaneId,
        pending: Vec<QueuedPrompt>,
    },
    /// A board was written by an agent or a worker; its workspace's folder.
    BoardChanged {
        cwd: String,
    },
    MemoryChanged,
    KitsChanged,
    AccountsChanged,
    GitCloneProgress(hangar_core::git_clone::CloneProgress),
    VoicePhase(hangar_core::voice::Phase),
    VoiceLevel(hangar_core::voice::Level),
    VoiceText(hangar_core::voice::Transcript),
    VoiceDownload(hangar_core::voice::models::Progress),
    RemoteChanged,
    RemoteShared(hangar_core::remote::Share),
    /// The person clicked a notification that names a pane.
    NotificationActivated {
        workspace: String,
        pane: PaneId,
    },
}

/// Who asks: an agent through the API (`control:request`, gated by
/// `agentPaneControl`), or Hangar itself (`control:internal`: briefings and
/// the phone's `remote_*` calls, answered whether or not pane control is on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Agent,
    Internal,
}

/// A request for the window to act (inventory A6), with its answer channel.
#[derive(Debug, Clone)]
pub struct ControlRequest {
    pub id: String,
    pub op: String,
    pub args: Value,
    pub channel: Channel,
    pub reply: Reply,
}

/// The answer to a control request: the first `send` wins, later ones do
/// nothing, so a handler can answer from wherever it ends without counting.
/// A request never answered times out on core's side (control.rs).
///
/// TODO(core): inventory A6 proposed an in-process oneshot
/// (`control::install(tx)`); core kept its `pending` map, so this answers
/// through `control::control_reply`, which is the same from here.
#[derive(Clone)]
pub struct Reply {
    id: String,
    sent: Arc<Mutex<bool>>,
}

impl Reply {
    pub fn new(id: String) -> Self {
        Reply {
            id,
            sent: Arc::new(Mutex::new(false)),
        }
    }

    /// Answers, if nothing answered before. Returns whether this one did.
    pub fn send(&self, answer: Result<Value, String>) -> bool {
        let mut sent = self
            .sent
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if *sent {
            return false;
        }
        *sent = true;
        let id = self.id.clone();
        match answer {
            Ok(value) => hangar_core::control::control_reply(id, true, Some(value), None),
            Err(error) => hangar_core::control::control_reply(id, false, None, Some(error)),
        }
        true
    }
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply").field("id", &self.id).finish()
    }
}

/// The message a core event becomes.
pub fn message(event: Event) -> Message {
    let core = |event| Message::Core(event);
    match event {
        Event::ControlRequest(request) => control(request, Channel::Agent),
        Event::ControlInternal(request) => control(request, Channel::Internal),
        Event::BoardChanged { cwd } => core(CoreEvent::BoardChanged { cwd }),
        Event::MemoryChanged => core(CoreEvent::MemoryChanged),
        Event::BridgeEvent { pane_id, event } => core(CoreEvent::Bridge {
            pane: pane_id,
            event,
        }),
        Event::BridgeQueue(queue) => core(CoreEvent::BridgeQueue {
            pane: queue.pane_id,
            pending: queue.pending,
        }),
        Event::VoicePhase(phase) => core(CoreEvent::VoicePhase(phase)),
        Event::VoiceText(transcript) => core(CoreEvent::VoiceText(transcript)),
        Event::VoiceLevel(level) => core(CoreEvent::VoiceLevel(level)),
        Event::VoiceDownload(progress) => core(CoreEvent::VoiceDownload(progress)),
        Event::RemoteChanged => core(CoreEvent::RemoteChanged),
        Event::RemoteShared(share) => core(CoreEvent::RemoteShared(share)),
        Event::KitsChanged => core(CoreEvent::KitsChanged),
        Event::AccountsChanged => core(CoreEvent::AccountsChanged),
        Event::GitCloneProgress(progress) => core(CoreEvent::GitCloneProgress(progress)),
        Event::NotificationActivated(activated) => core(CoreEvent::NotificationActivated {
            workspace: activated.workspace_id,
            pane: activated.pane_id,
        }),
    }
}

fn control(request: hangar_core::control::ControlRequest, channel: Channel) -> Message {
    let reply = Reply::new(request.id.clone());
    Message::Control(crate::control::Msg::Request(ControlRequest {
        id: request.id,
        op: request.op,
        args: request.args,
        channel,
        reply,
    }))
}

/// The bus, subscribed to before `Core::start` so nothing it sends at start is
/// missed (core-api.md), handed to the one subscription that reads it.
#[derive(Clone)]
pub struct Bus(Arc<Mutex<Option<broadcast::Receiver<Event>>>>);

impl Bus {
    pub fn new(receiver: broadcast::Receiver<Event>) -> Self {
        Bus(Arc::new(Mutex::new(Some(receiver))))
    }
}

// The subscription's identity: there is one bus.
impl std::hash::Hash for Bus {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "hangar-core bus".hash(state);
    }
}

/// Every core event, as a message.
pub fn subscription(bus: &Bus) -> Subscription<Message> {
    Subscription::run_with(bus.clone(), |bus| {
        let receiver = bus.0.lock().ok().and_then(|mut slot| slot.take());
        stream::unfold(receiver, |receiver| async move {
            let mut receiver = receiver?;
            loop {
                match receiver.recv().await {
                    Ok(event) => return Some((message(event), Some(receiver))),
                    // A change notification lost is a reload to do anyway; a
                    // control request lost times out on the side that asked.
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        eprintln!("the window fell behind core's events: {missed} lost");
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
    })
}
