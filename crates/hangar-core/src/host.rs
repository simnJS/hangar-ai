//! What the backend reaches of the app around it, which Tauri's `AppHandle`
//! used to be: the folders it keeps its files in (paths.rs), and a way to tell
//! the window that something changed.
//!
//! The window learnt of every change through an event, `app.emit("…")`, and
//! still does: each one is an `Event` here, sent on a broadcast bus. The app
//! subscribes once and draws from what arrives; nothing waits on it to.
//!
//! Two kinds of traffic stay off the bus on purpose:
//!
//! - a pane's output, which is most of what the backend ever says and is only
//!   ever for that pane: pty.rs hands it to the sink the pane was spawned
//!   with, so it never queues behind a board reload, and a flood of it never
//!   pushes the rarer events out of the bus;
//! - what Tauri itself did with a window (focus it, close it, ask the person
//!   for a file): the app's own business, listed in docs/native/core-api.md.
//!
//! Tauri's state — the PTY manager, the dictation, Discord, Hangar Mobile —
//! is not here either. `Core` (lib.rs) holds those, and a function that used
//! one is given it as an argument, as it was given `State<…>`.

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::broadcast;

use crate::paths::Paths;

/// How many events the bus keeps for a subscriber that has not read them.
/// Generous: the busiest are the dictation's meter (sixteen a second while a
/// recording is open) and a download's progress (five), and a window that
/// falls this far behind has stalled. A subscriber that does is told it
/// lagged (`RecvError::Lagged`) and loses the oldest: a change notification
/// lost is a reload to do anyway, and a control request lost times out on the
/// side that asked (control.rs), as a window that never answered did.
const BUS_CAPACITY: usize = 1024;

/// Everything the backend told the window, by the name the window listened
/// to (`Event::name`).
///
/// The dictation's, Hangar Mobile's share and the notification's click carry
/// types of modules only the `desktop` feature compiles, and exist with it
/// only: nothing else could send them.
#[derive(Debug, Clone)]
pub enum Event {
    /// `board:changed`: a board was written by an agent or a worker; the
    /// workspace's folder, as the board is keyed (board.rs).
    BoardChanged { cwd: String },
    /// `memory:changed`: the memory, which is global, was written.
    MemoryChanged,
    /// `control:request`: an agent asks the window to act on its panes
    /// (control.rs). Answered with `control::control_reply`, once.
    ControlRequest(crate::control::ControlRequest),
    /// `control:internal`: one of Hangar's own questions — an orchestrator's
    /// briefing, a phone's call (`remote_<op>`). Answered the same way.
    ControlInternal(crate::control::ControlRequest),
    /// `bridge:event`: a report of a pane's hangar-bridge mod, as the mod
    /// sent it (bridge.rs); the window owns the meaning of each `kind`.
    BridgeEvent { pane_id: String, event: Value },
    /// `bridge:queue`: what is queued for a pane, after it changed.
    BridgeQueue(crate::bridge::QueueChanged),
    /// `voice:phase`: the dictation's state.
    #[cfg(feature = "desktop")]
    VoicePhase(crate::voice::Phase),
    /// `voice:text`: what was said, for the pane that has the focus.
    #[cfg(feature = "desktop")]
    VoiceText(crate::voice::Transcript),
    /// `voice:level`: the microphone's level, while recording.
    #[cfg(feature = "desktop")]
    VoiceLevel(crate::voice::Level),
    /// `voice:download`: a local model's download, as it goes.
    #[cfg(feature = "desktop")]
    VoiceDownload(crate::voice::models::Progress),
    /// `remote:changed`: a phone was paired or revoked.
    RemoteChanged,
    /// `remote:shared`: a file was shared with the phones.
    #[cfg(feature = "desktop")]
    RemoteShared(crate::remote::Share),
    /// `kits:changed`: the list `kits_list` returns may have changed.
    KitsChanged,
    /// `accounts:changed`: a Claude account was added, renamed, removed,
    /// logged in or out.
    AccountsChanged,
    /// `git-clone:progress`: a clone started by `git_clone`, as it goes.
    GitCloneProgress(crate::git_clone::CloneProgress),
    /// `notification-activated`: the person clicked a notification that names
    /// a pane (Windows; notify.rs). The window brings itself, or the
    /// workspace's own window, forward and reveals the pane.
    #[cfg(feature = "desktop")]
    NotificationActivated(crate::notify::Activated),
}

impl Event {
    /// The name the Tauri window listened for, for logs and for anything that
    /// still speaks to a page in those terms.
    pub fn name(&self) -> &'static str {
        match self {
            Event::BoardChanged { .. } => "board:changed",
            Event::MemoryChanged => "memory:changed",
            Event::ControlRequest(_) => "control:request",
            Event::ControlInternal(_) => "control:internal",
            Event::BridgeEvent { .. } => "bridge:event",
            Event::BridgeQueue(_) => "bridge:queue",
            #[cfg(feature = "desktop")]
            Event::VoicePhase(_) => "voice:phase",
            #[cfg(feature = "desktop")]
            Event::VoiceText(_) => "voice:text",
            #[cfg(feature = "desktop")]
            Event::VoiceLevel(_) => "voice:level",
            #[cfg(feature = "desktop")]
            Event::VoiceDownload(_) => "voice:download",
            Event::RemoteChanged => "remote:changed",
            #[cfg(feature = "desktop")]
            Event::RemoteShared(_) => "remote:shared",
            Event::KitsChanged => "kits:changed",
            Event::AccountsChanged => "accounts:changed",
            Event::GitCloneProgress(_) => "git-clone:progress",
            #[cfg(feature = "desktop")]
            Event::NotificationActivated(_) => "notification-activated",
        }
    }
}

/// An event sent while nothing was subscribed to the bus: the window is gone,
/// or never came.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoListener;

impl std::fmt::Display for NoListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("nothing is listening to Hangar's events")
    }
}

impl std::error::Error for NoListener {}

/// The app around the backend, as the backend sees it. Cloned freely: every
/// clone is the same folders and the same bus.
#[derive(Clone)]
pub struct Host {
    inner: Arc<Inner>,
}

struct Inner {
    paths: Paths,
    events: broadcast::Sender<Event>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    /// The installed app's folders, the ones Tauri used (paths.rs), unless
    /// `HANGAR_DATA_ROOT` names another place (`Paths::from_env`).
    pub fn new() -> Self {
        Self::with_paths(Paths::from_env())
    }

    /// Folders of the caller's choosing: tests, mostly.
    pub fn with_paths(paths: Paths) -> Self {
        let (events, _) = broadcast::channel(BUS_CAPACITY);
        Self {
            inner: Arc::new(Inner { paths, events }),
        }
    }

    pub fn paths(&self) -> &Paths {
        &self.inner.paths
    }

    /// Every event sent from now on. Subscribe before calling what sends them
    /// (`Core::start` included): the bus does not keep what nobody heard.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Sends `event` to every subscriber. Fails only when there is none, which
    /// is the one way Tauri's `emit` could fail in practice, the window gone:
    /// most callers ignore it, the ones that wait for an answer give up at
    /// once instead of waiting it out (control.rs).
    pub fn emit(&self, event: Event) -> Result<(), NoListener> {
        self.inner
            .events
            .send(event)
            .map(|_| ())
            .map_err(|_| NoListener)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_subscriber_hears_an_event() {
        let host = Host::with_paths(Paths::rooted(&std::env::temp_dir()));
        let mut first = host.subscribe();
        let mut second = host.clone().subscribe();
        host.emit(Event::BoardChanged {
            cwd: "/work/app".into(),
        })
        .unwrap();
        for receiver in [&mut first, &mut second] {
            match receiver.recv().await.unwrap() {
                Event::BoardChanged { cwd } => assert_eq!(cwd, "/work/app"),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn an_event_nobody_hears_says_so() {
        let host = Host::with_paths(Paths::rooted(&std::env::temp_dir()));
        assert_eq!(host.emit(Event::KitsChanged), Err(NoListener));
        let _listening = host.subscribe();
        assert_eq!(host.emit(Event::KitsChanged), Ok(()));
    }

    #[test]
    fn events_keep_the_names_the_window_listened_for() {
        // As src/lib/memory.ts, kits.ts and accounts.ts name them.
        assert_eq!(Event::MemoryChanged.name(), "memory:changed");
        assert_eq!(Event::KitsChanged.name(), "kits:changed");
        assert_eq!(Event::AccountsChanged.name(), "accounts:changed");
        assert_eq!(Event::RemoteChanged.name(), "remote:changed");
    }
}
