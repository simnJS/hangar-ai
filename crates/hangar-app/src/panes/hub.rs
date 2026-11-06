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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use iced::Subscription;
use iced::futures::stream;
use tokio::sync::mpsc;

use crate::message::Message;
use crate::panes::Msg;
use crate::panes::pty_link::{HubEvent, HubSender, PaneLink, Waker};
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

    /// The rows of the screen, the history left out.
    fn screen_text(&self, pane: &PaneId) -> Option<Vec<String>> {
        let link = self.get(pane)?;
        let rows = link.lock().screen_text();
        Some(rows)
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

/// The channel every pane's sink sends on, its receiving half, which the
/// subscription takes once, and the flag that keeps the panes' wake-ups for
/// output to one at a time ([`Waker`]).
#[derive(Debug, Clone)]
pub struct HubChannel {
    sender: HubSender,
    receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<HubEvent>>>>,
    awake: Arc<AtomicBool>,
}

impl Default for HubChannel {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        HubChannel {
            sender,
            receiver: Arc::new(Mutex::new(Some(receiver))),
            awake: Arc::new(AtomicBool::new(false)),
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
    /// What a pane's sink is given.
    pub fn waker(&self) -> Waker {
        Waker {
            sender: self.sender.clone(),
            awake: self.awake.clone(),
        }
    }

    /// The receiving half, for a test to read what the sinks sent.
    #[cfg(test)]
    pub fn take_receiver(&self) -> Option<mpsc::UnboundedReceiver<HubEvent>> {
        self.receiver.lock().ok().and_then(|mut slot| slot.take())
    }

    /// Every pane was quiet for a frame: the next chunk sends a wake-up.
    pub fn rest(&self) {
        self.awake.store(false, Ordering::Release);
    }

    /// Output came in between the window's last look and its rest: it goes
    /// on looking every frame, and no wake-up is needed.
    pub fn stay_awake(&self) {
        self.awake.store(true, Ordering::Release);
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

#[cfg(test)]
mod tests {
    use hangar_core::pty::PtyManager;
    use hangar_term::{Config, Terminal};

    use super::*;

    fn hub_with(id: &str) -> (Hub, Arc<PaneLink>) {
        let hub = Hub::default();
        let link = PaneLink::new(
            id.into(),
            Terminal::new(Config::default(), 40, 10),
            crate::panes::pty_link::PtyBackend::Local(Arc::new(PtyManager::default())),
        );
        hub.insert(link.clone());
        (hub, link)
    }

    #[test]
    fn the_engines_read_what_a_pane_shows() {
        let (hub, link) = hub_with("a");
        link.lock().feed(b"first\r\nsecond\r\n\r\n");
        assert!(hub.has(&"a".to_string()));
        assert!(!hub.has(&"b".to_string()));
        assert_eq!(
            hub.read_lines(&"a".to_string(), 2),
            Some(vec!["first".to_string(), "second".to_string()])
        );
        assert_eq!(hub.read_lines(&"b".to_string(), 2), None);
    }

    #[test]
    fn the_screen_is_what_the_program_drew_and_not_what_scrolled_off_it() {
        let (hub, link) = hub_with("a");
        let question = "Quick safety check: trust this folder?";
        link.lock().feed(format!("{question}\r\n").as_bytes());
        let rows = hub.screen_text(&"a".to_string()).expect("a screen");
        assert_eq!(rows.len(), 10, "one string per row of the grid");
        assert_eq!(rows[0], question);
        // Twenty lines later the question is history: read_lines still finds
        // it, which is what made a long-gone question look asked again.
        let mut output = String::new();
        for line in 0..20 {
            output.push_str(&format!("line {line}\r\n"));
        }
        link.lock().feed(output.as_bytes());
        let rows = hub.screen_text(&"a".to_string()).expect("a screen");
        assert!(
            !rows.iter().any(|row| row.contains("safety check")),
            "{rows:?}"
        );
        let lines = hub.read_lines(&"a".to_string(), 60).expect("lines");
        assert!(lines.iter().any(|line| line.contains("safety check")));
        // And a pane scrolled back is still read where the program draws.
        link.lock()
            .feed(b"Quick safety check: trust this folder?\r\n");
        link.lock()
            .perform(hangar_term::Action::Scroll(hangar_term::ScrollTo::Top));
        let rows = hub.screen_text(&"a".to_string()).expect("a screen");
        assert!(rows.iter().any(|row| row.contains("trust this folder")));
        assert_eq!(hub.screen_text(&"b".to_string()), None);
    }

    #[test]
    fn the_cursor_mode_follows_the_program() {
        let (hub, link) = hub_with("a");
        assert!(!hub.application_cursor(&"a".to_string()));
        link.lock().feed(b"\x1b[?1h");
        assert!(hub.application_cursor(&"a".to_string()));
    }

    #[test]
    fn a_pane_gone_takes_no_paste() {
        let (hub, _link) = hub_with("a");
        assert!(hub.paste(&"a".to_string(), "x"));
        hub.remove("a");
        assert!(!hub.paste(&"a".to_string(), "x"));
        assert!(!hub.has(&"a".to_string()));
    }
}
