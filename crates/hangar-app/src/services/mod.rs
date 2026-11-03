//! What a package reaches outside the model: hangar-core, the terminals and
//! the preview (behind traits their packages implement), the inbox back into
//! `update`, the latest snapshot, the clipboard and the file dialogs.
//!
//! One `Services` exists, made at boot and handed by reference to every
//! `update` and `on_intent`. It is `Clone` and `Send`, so an engine takes a
//! clone onto its tokio task.

pub mod clipboard;
pub mod core;
pub mod dialogs;
pub mod events;
pub mod inbox;
pub mod notify;
pub mod preview;
pub mod terminals;

use std::sync::Arc;

use tokio::sync::watch;

pub use self::core::CoreHandles;
#[allow(unused_imports)]
pub use events::{Channel, ControlRequest, CoreEvent, Reply};
pub use inbox::Inbox;
pub use preview::{PreviewAccess, PreviewRequest};
pub use terminals::TerminalAccess;

use crate::message::Message;
use crate::state::Snapshot;

#[derive(Clone)]
pub struct Services {
    core: CoreHandles,
    terminals: Arc<dyn TerminalAccess>,
    preview: Arc<dyn PreviewAccess>,
    inbox: Inbox,
    snapshot: watch::Receiver<Arc<Snapshot>>,
}

impl Services {
    pub fn new(
        core: CoreHandles,
        terminals: Arc<dyn TerminalAccess>,
        preview: Arc<dyn PreviewAccess>,
        inbox: Inbox,
        snapshot: watch::Receiver<Arc<Snapshot>>,
    ) -> Self {
        Services {
            core,
            terminals,
            preview,
            inbox,
            snapshot,
        }
    }

    /// The running hangar-core: `core().host()`, `core().pty()`,
    /// `core().core().voice()`…
    pub fn core(&self) -> &CoreHandles {
        &self.core
    }

    /// The panes' terminals (P5's hub).
    pub fn terminals(&self) -> &Arc<dyn TerminalAccess> {
        &self.terminals
    }

    /// The workspaces' previews (P15's store).
    pub fn preview(&self) -> &Arc<dyn PreviewAccess> {
        &self.preview
    }

    /// The way back into `update` from a tokio task.
    pub fn inbox(&self) -> &Inbox {
        &self.inbox
    }

    /// Queues a message for `update` (`inbox().send`).
    pub fn send(&self, message: Message) -> bool {
        self.inbox.send(message)
    }

    /// The model as it stood after the last update that changed it.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.borrow().clone()
    }

    /// A receiver that wakes on every new snapshot, for an engine that waits
    /// on changes (`changed().await`).
    pub fn snapshots(&self) -> watch::Receiver<Arc<Snapshot>> {
        self.snapshot.clone()
    }
}
