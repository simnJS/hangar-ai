//! Discord Rich Presence.
//!
//! Every call on Discord's local IPC blocks: opening the pipe while Discord is
//! closed, writing to it, waiting for the reply to a command. None of that
//! belongs on a Tauri command thread, so the whole conversation lives in one
//! worker thread. The frontend never talks to Discord — it hands over the
//! presence it wants shown and forgets about it; the worker owns connecting,
//! reconnecting, rate limiting and giving up quietly when Discord is not there.

use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use serde::{Deserialize, Serialize};

/// Discord throttles a client that updates faster than roughly five times per
/// twenty seconds. Coalescing to one update every few seconds stays well under
/// that, and nothing here changes fast enough for the delay to show.
const MIN_INTERVAL: Duration = Duration::from_secs(4);

/// Discord being closed is the ordinary case, not a failure: keep retrying, but
/// slowly enough that a machine without Discord pays nothing for the feature.
const RETRY_DELAY: Duration = Duration::from_secs(15);

/// Nothing left to do. The worker still wakes up on its own, so a notify that
/// crossed paths with the lock cannot strand it.
const IDLE_WAIT: Duration = Duration::from_secs(60);

/// How long shutdown waits for the presence to be cleared before letting the
/// process go. Discord drops it anyway when the pipe closes; this only buys the
/// tidy path a moment, never enough to be felt as a slow quit.
const SHUTDOWN_GRACE: Duration = Duration::from_millis(400);

/// Discord rejects a field longer than this instead of trimming it.
const MAX_FIELD: usize = 128;

/// What the frontend wants Discord to show. Compared as a whole to decide
/// whether an update is worth sending, so every field takes part.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    /// The Discord application whose uploaded art the asset keys refer to.
    pub app_id: String,
    pub details: String,
    pub state: String,
    pub large_image: String,
    pub large_text: String,
    /// Where clicking the art takes you — the repository, in practice.
    pub large_url: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
    /// Unix milliseconds; Discord counts up from here as "elapsed".
    pub started_at: Option<i64>,
    pub button_label: Option<String>,
    pub button_url: Option<String>,
}

/// What the settings page shows about the link, so a presence that never
/// appears can be told apart from Discord simply not being open.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub connected: bool,
    /// Last failure, kept until the next successful connection.
    pub error: Option<String>,
}

#[derive(Default)]
struct Inner {
    /// `None` means the feature is off: the worker clears and disconnects.
    wanted: Option<Presence>,
    /// Bumped on every write, so the worker can tell a fresh request from a
    /// wake-up it caused itself.
    revision: u64,
    stopping: bool,
    stopped: bool,
    status: Status,
}

#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
    signal: Condvar,
}

impl Shared {
    fn snapshot(&self) -> (Option<Presence>, u64, bool) {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        (inner.wanted.clone(), inner.revision, inner.stopping)
    }

    /// Sleeps until `timeout`, or until someone changes what is wanted. `seen`
    /// is the revision the caller acted on: a write that landed while it was
    /// working means there is already more to do, so do not sleep at all.
    fn wait(&self, seen: u64, timeout: Duration) {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.revision != seen || inner.stopping {
            return;
        }
        let _ = self.signal.wait_timeout(inner, timeout);
    }

    fn report(&self, connected: bool, error: Option<String>) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.status.connected = connected;
        inner.status.enabled = inner.wanted.is_some();
        // A successful connection is the only thing that clears the last
        // failure: an error kept around after the link came back would read as
        // broken forever.
        inner.status.error = if connected { None } else { error };
    }

    fn finished(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.stopped = true;
        inner.status = Status::default();
        self.signal.notify_all();
    }
}

/// One live connection to Discord, and what it last managed to send.
struct Link {
    client: DiscordIpcClient,
    app_id: String,
    sent: Option<Presence>,
    /// Earliest moment the next update may go out.
    next_send: Instant,
}

impl Link {
    fn open(app_id: &str) -> Result<Self, String> {
        let mut client = DiscordIpcClient::new(app_id);
        client.connect().map_err(|err| err.to_string())?;
        Ok(Self {
            client,
            app_id: app_id.to_string(),
            sent: None,
            next_send: Instant::now(),
        })
    }

    fn push(&mut self, want: &Presence) -> Result<(), String> {
        self.client
            .set_activity(build(want))
            .map_err(|err| err.to_string())?;
        // Discord answers every command. Those frames are of no use here, but
        // left unread they fill the pipe buffer, and the write that finally
        // overflows it blocks this thread for good.
        self.client.recv().map_err(|err| err.to_string())?;
        self.sent = Some(want.clone());
        self.next_send = Instant::now() + MIN_INTERVAL;
        Ok(())
    }

    /// Takes the presence down on the way out. Both steps are best-effort: the
    /// connection is being dropped either way.
    fn close(mut self) {
        let _ = self.client.clear_activity();
        let _ = self.client.close();
    }
}

/// Discord rejects the whole payload over a field it considers too long, so
/// clip on character boundaries rather than let one long workspace name silence
/// the presence entirely.
fn clip(text: &str) -> &str {
    match text.char_indices().nth(MAX_FIELD) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

fn build(want: &Presence) -> activity::Activity<'_> {
    let mut assets = activity::Assets::new()
        .large_image(want.large_image.as_str())
        .large_text(clip(&want.large_text));
    if let Some(url) = &want.large_url {
        assets = assets.large_url(url.as_str());
    }
    if let Some(image) = &want.small_image {
        assets = assets.small_image(image.as_str());
    }
    if let Some(text) = &want.small_text {
        assets = assets.small_text(clip(text));
    }

    let mut act = activity::Activity::new()
        .details(clip(&want.details))
        .state(clip(&want.state))
        .assets(assets);

    if let Some(start) = want.started_at {
        act = act.timestamps(activity::Timestamps::new().start(start));
    }

    // Discord refuses a button without both halves, and only accepts http(s).
    if let (Some(label), Some(url)) = (&want.button_label, &want.button_url) {
        if !label.is_empty() && url.starts_with("http") {
            act = act.buttons(vec![activity::Button::new(label.as_str(), url.as_str())]);
        }
    }

    act
}

fn worker(shared: Arc<Shared>) {
    let mut link: Option<Link> = None;
    // Set after a failed attempt; nothing is dialled again before it passes.
    let mut retry_at: Option<Instant> = None;

    loop {
        let (wanted, revision, stopping) = shared.snapshot();
        if stopping {
            if let Some(link) = link {
                link.close();
            }
            shared.finished();
            return;
        }

        let mut wake_in = IDLE_WAIT;

        match wanted {
            // Turned off: drop the presence, and stop dialling Discord.
            None => {
                if let Some(link) = link.take() {
                    link.close();
                }
                retry_at = None;
                shared.report(false, None);
            }

            Some(want) => {
                // A connection is bound to the application it handshaked with;
                // a new id needs a new one.
                if link.as_ref().is_some_and(|open| open.app_id != want.app_id) {
                    if let Some(stale) = link.take() {
                        stale.close();
                    }
                }

                if link.is_none() {
                    let now = Instant::now();
                    match retry_at {
                        Some(at) if now < at => wake_in = at - now,
                        _ => match Link::open(&want.app_id) {
                            Ok(opened) => {
                                link = Some(opened);
                                retry_at = None;
                                shared.report(true, None);
                            }
                            Err(err) => {
                                retry_at = Some(now + RETRY_DELAY);
                                wake_in = RETRY_DELAY;
                                shared.report(false, Some(err));
                            }
                        },
                    }
                }

                if let Some(open) = link.as_mut() {
                    if open.sent.as_ref() != Some(&want) {
                        let now = Instant::now();
                        if now < open.next_send {
                            // Rate limited: come back exactly when it lifts.
                            wake_in = wake_in.min(open.next_send - now);
                        } else if let Err(err) = open.push(&want) {
                            // Discord quitting mid-session lands here. Drop the
                            // link and let the retry path pick it up again.
                            link = None;
                            retry_at = Some(now + RETRY_DELAY);
                            wake_in = RETRY_DELAY;
                            shared.report(false, Some(err));
                        }
                    }
                }
            }
        }

        shared.wait(revision, wake_in);
    }
}

/// Handle held by Tauri, shared with the worker thread.
#[derive(Default)]
pub struct DiscordPresence {
    shared: Arc<Shared>,
    /// The worker is only spawned once something is actually published, so an
    /// install that never turns the feature on runs no extra thread.
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl DiscordPresence {
    fn set(&self, wanted: Option<Presence>) {
        {
            let mut inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.stopping || inner.wanted == wanted {
                return;
            }
            inner.wanted = wanted;
            inner.revision = inner.revision.wrapping_add(1);
            inner.status.enabled = inner.wanted.is_some();
            self.shared.signal.notify_all();
        }
        self.ensure_worker();
    }

    fn ensure_worker(&self) {
        let mut handle = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        if handle.is_some() {
            return;
        }
        let shared = Arc::clone(&self.shared);
        *handle = thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || worker(shared))
            .ok();
    }

    fn status(&self) -> Status {
        self.shared
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }

    /// Clears the presence on the way out, without holding the quit up for a
    /// Discord that has stopped answering.
    pub fn stop(&self) {
        // Nothing was ever published: no thread to wait for, and waiting for
        // one that does not exist would burn the whole grace period.
        if self.worker.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
            return;
        }
        let mut inner = self.shared.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.stopped {
            return;
        }
        inner.stopping = true;
        self.shared.signal.notify_all();
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while !inner.stopped {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return;
            };
            let (guard, timeout) = self
                .shared
                .signal
                .wait_timeout(inner, left)
                .unwrap_or_else(|e| e.into_inner());
            inner = guard;
            if timeout.timed_out() {
                return;
            }
        }
    }
}

#[tauri::command]
pub fn discord_presence_set(
    presence: tauri::State<'_, DiscordPresence>,
    wanted: Option<Presence>,
) {
    presence.set(wanted);
}

#[tauri::command]
pub fn discord_presence_status(presence: tauri::State<'_, DiscordPresence>) -> Status {
    presence.status()
}
