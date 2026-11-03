//! The way back into the model from off the UI thread.
//!
//! The engines (P18–P20) and anything else running on tokio never touch
//! `Model`: they send a `Message` here (`Services::send`), and the shell's
//! subscription hands it to `update` like any other (PACKAGES.md, rule 6).

use std::sync::{Arc, Mutex};

use iced::Subscription;
use iced::futures::stream;
use tokio::sync::mpsc;

use crate::message::Message;

/// The sending half, cloned into whatever needs it.
#[derive(Clone, Debug)]
pub struct Inbox(mpsc::UnboundedSender<Message>);

impl Inbox {
    /// Queues `message` for `update`. Fails only once the app is closing.
    pub fn send(&self, message: Message) -> bool {
        self.0.send(message).is_ok()
    }
}

/// The receiving half, taken once by the subscription.
#[derive(Clone)]
pub struct InboxReceiver(Arc<Mutex<Option<mpsc::UnboundedReceiver<Message>>>>);

// The subscription's identity: there is one inbox.
impl std::hash::Hash for InboxReceiver {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "hangar inbox".hash(state);
    }
}

pub fn channel() -> (Inbox, InboxReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Inbox(tx), InboxReceiver(Arc::new(Mutex::new(Some(rx)))))
}

/// Every message sent to the inbox, in order.
pub fn subscription(receiver: &InboxReceiver) -> Subscription<Message> {
    Subscription::run_with(receiver.clone(), |receiver| {
        let rx = receiver.0.lock().ok().and_then(|mut slot| slot.take());
        stream::unfold(rx, |rx| async move {
            let mut rx = rx?;
            let message = rx.recv().await?;
            Some((message, Some(rx)))
        })
    })
}
