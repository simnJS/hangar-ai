//! Every pane's terminal, for the code that is not a view: the engines' side
//! (`TerminalAccess`, inventory §8.2) and the channel the reader threads wake
//! the window through (`lib/terminalRegistry.ts` and the window half of
//! `lib/ptyBus.ts`).
//!
//! The web app reached a terminal of another window by asking for it
//! (`terminal:ask`, 1.5 s timeout). Here one daemon holds every terminal, so
//! the hub answers directly, from any thread: it shares each pane's
//! [`PaneLink`] with the panes' state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use iced::Subscription;
use iced::futures::stream;
use tokio::sync::mpsc;

use crate::message::Message;
use crate::panes::Msg;
use crate::panes::pty_link::{HubEvent, HubSender, PaneLink};
use crate::services::TerminalAccess;
use crate::state::PaneId;

/// The terminals by pane id.
#[derive(Debug, Default)]
pub struct Hub {
    panes: RwLock<HashMap<PaneId, Arc<PaneLink>>>,
}

impl Hub {
    pub fn insert(&self, link: Arc<PaneLink>) {
        self.panes
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(link.id.clone(), link);
    }

    pub fn remove(&self, pane: &str) {
        self.panes
            .write()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(pane);
    }

    fn get(&self, pane: &str) -> Option<Arc<PaneLink>> {
        self.panes
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(pane)
            .cloned()
    }
}

impl TerminalAccess for Hub {
    fn has(&self, pane: &PaneId) -> bool {
        self.get(pane).is_some()
    }

    /// The last `n` lines, wrapped rows joined and the blank rows below the
    /// text dropped (`readLines` of lib/windows.ts).
    fn read_lines(&self, pane: &PaneId, n: usize) -> Option<Vec<String>> {
        let link = self.get(pane)?;
        let lines = link.lock().read_lines(n);
        Some(lines)
    }

    /// As a paste: bracketed when the program asked for it, line breaks as
    /// Enter sends them.
    fn paste(&self, pane: &PaneId, text: &str) -> bool {
        let Some(link) = self.get(pane) else {
            return false;
        };
        let bytes = link.lock().paste_bytes(text);
        link.write(bytes);
        true
    }

    fn application_cursor(&self, pane: &PaneId) -> bool {
        self.get(pane)
            .is_some_and(|link| link.lock().application_cursor())
    }
}

/// The channel every pane's sink sends on, and its receiving half, which the
/// subscription takes once.
#[derive(Debug, Clone)]
pub struct HubChannel {
    sender: HubSender,
    receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<HubEvent>>>>,
}

impl Default for HubChannel {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        HubChannel {
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
        }
    }
}

// The subscription's identity: there is one channel.
impl std::hash::Hash for HubChannel {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "hangar panes hub".hash(state);
    }
}

impl HubChannel {
    pub fn sender(&self) -> HubSender {
        self.sender.clone()
    }

    /// Every event the panes' reader threads send, in batches: whatever has
    /// piled up since the last message goes in the next one, so a burst over
    /// sixteen panes is one update and one frame, not one per chunk.
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::run_with(self.clone(), |channel| {
            let receiver = channel
                .receiver
                .lock()
                .ok()
                .and_then(|mut slot| slot.take());
            stream::unfold(receiver, |receiver| async move {
                let mut receiver = receiver?;
                let first = receiver.recv().await?;
                let mut batch = vec![first];
                while let Ok(more) = receiver.try_recv() {
                    batch.push(more);
                }
                Some((Message::Panes(Msg::Hub(batch)), Some(receiver)))
            })
        })
    }
}
