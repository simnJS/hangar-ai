//! The way back into the model from off the UI thread.
//!
//! The engines (P18–P20) and anything else running on tokio never touch
//! `Model`: they send a `Message` here (`Services::send`), and the shell's
//! subscription hands it to `update` like any other (PACKAGES.md, rule 6).

use std::sync::{Arc, Mutex};

use iced::Subscription;
use iced::futures::stream;
use tokio::sync::{mpsc, oneshot};

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

/// The answer to a message sent through the inbox, for the task that waits
/// on it: an engine sends `Message::Control(control::Msg::Model(cmd, answer))`
/// and awaits the receiver; `update` answers once it has applied the command.
/// The first `send` wins, so it can ride in a `Message` (which is cloned) and
/// be answered from wherever the handling ends.
pub struct Answer<T>(Arc<Mutex<Option<oneshot::Sender<T>>>>);

impl<T> Answer<T> {
    /// An answer and the receiver that waits for it. A receiver whose answer
    /// was dropped unsent gets an error: the app went away, or nobody handled
    /// the message.
    pub fn channel() -> (Answer<T>, oneshot::Receiver<T>) {
        let (tx, rx) = oneshot::channel();
        (Answer(Arc::new(Mutex::new(Some(tx)))), rx)
    }

    /// Answers, if nothing answered before. Returns whether this one did.
    pub fn send(&self, value: T) -> bool {
        let sender = self
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        sender.is_some_and(|sender| sender.send(value).is_ok())
    }
}

impl<T> Clone for Answer<T> {
    fn clone(&self) -> Self {
        Answer(self.0.clone())
    }
}

impl<T> std::fmt::Debug for Answer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answer(..)")
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
