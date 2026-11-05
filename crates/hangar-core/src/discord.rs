//! Discord Rich Presence.
//!
//! Every call on Discord's local IPC blocks: opening the pipe while Discord is
//! closed, writing to it, waiting for the reply to a command. None of that
//! belongs on the thread the window calls a command on, so the whole
//! conversation lives in one worker thread. The frontend never talks to
//! Discord — it hands over the presence it wants shown and forgets about it;
//! the worker owns connecting, reconnecting, rate limiting and giving up
//! quietly when Discord is not there.

use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use discord_rich_presence::error::Error as IpcError;
use discord_rich_presence::{activity, DiscordIpc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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

/// How long a reply from Discord may take. A Discord that hangs would
/// otherwise hold the worker on a read that never returns, and with it every
/// later update and the quit's tidy path.
#[cfg_attr(windows, allow(dead_code))]
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// The IPC frame opcodes this module reads (Discord's RPC protocol).
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;

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

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Stream = std::fs::File;

/// The pipe to Discord, in place of the library's `DiscordIpcClient`: that one
/// keeps its socket private, so it cannot be given a read timeout, and its
/// handshake reads Discord's answer only to throw it away — a rejected app id
/// read as a working link. Implementing the library's trait keeps its framing
/// and its activity payloads.
struct Pipe {
    client_id: String,
    socket: Option<Stream>,
}

impl Pipe {
    fn new(client_id: &str) -> Self {
        Self {
            client_id: client_id.to_string(),
            socket: None,
        }
    }

    /// Opens the pipe and handshakes, which only counts once Discord has
    /// answered `READY`.
    fn open(client_id: &str) -> Result<Self, String> {
        let mut pipe = Self::new(client_id);
        pipe.connect_ipc().map_err(|err| err.to_string())?;
        pipe.handshake()?;
        Ok(pipe)
    }

    fn handshake(&mut self) -> Result<(), String> {
        self.send(json!({ "v": 1, "client_id": self.client_id }), 0)
            .map_err(|err| err.to_string())?;
        let (op, reply) = self.recv().map_err(|err| err.to_string())?;
        answer(op, &reply)?;
        if reply.get("evt").and_then(Value::as_str) != Some("READY") {
            return Err(format!("Discord did not accept the handshake: {reply}"));
        }
        Ok(())
    }

    /// Sends the presence and reads Discord's answer to it. Reading it is not
    /// optional: left unread, those frames fill the pipe buffer, and the write
    /// that finally overflows it would block this thread for good.
    fn push(&mut self, want: &Presence) -> Result<(), String> {
        self.set_activity(build(want))
            .map_err(|err| err.to_string())?;
        let (op, reply) = self.recv().map_err(|err| err.to_string())?;
        answer(op, &reply)
    }

    #[cfg(unix)]
    fn find_pipe() -> Option<std::path::PathBuf> {
        // Where the Linux and macOS clients put it, Flatpak and Snap builds
        // included (the same places the library looks).
        const ENV_KEYS: [&str; 4] = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"];
        const SUBPATHS: [&str; 7] = [
            "",
            "app/com.discordapp.Discord/",
            "app/dev.vencord.Vesktop/",
            ".flatpak/com.discordapp.Discord/xdg-run/",
            ".flatpak/dev.vencord.Vesktop/xdg-run/",
            "snap.discord-canary/",
            "snap.discord/",
        ];
        let snap = std::env::var_os("SNAP").is_some();
        for key in ENV_KEYS {
            let Some(value) = std::env::var_os(key) else {
                continue;
            };
            let mut base = std::path::PathBuf::from(value);
            if snap && key == ENV_KEYS[0] {
                base.pop();
            }
            if !base.is_dir() {
                continue;
            }
            for index in 0..10 {
                for subpath in SUBPATHS {
                    let path = base.join(subpath).join(format!("discord-ipc-{index}"));
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }
}

/// What Discord's reply to a command means: a close frame (a bad app id,
/// mostly) or an `ERROR` event is a failure, whatever the write said.
fn answer(op: u32, reply: &Value) -> Result<(), String> {
    let message = || {
        reply
            .get("message")
            .or_else(|| reply.get("data").and_then(|data| data.get("message")))
            .and_then(Value::as_str)
            .unwrap_or("no reason given")
            .to_string()
    };
    if op == OP_CLOSE {
        return Err(format!("Discord closed the connection: {}", message()));
    }
    if op != OP_FRAME {
        return Err(format!("unexpected frame from Discord (op {op})"));
    }
    if reply.get("evt").and_then(Value::as_str) == Some("ERROR") {
        return Err(format!("Discord refused the presence: {}", message()));
    }
    Ok(())
}

impl DiscordIpc for Pipe {
    fn get_client_id(&self) -> &str {
        &self.client_id
    }

    #[cfg(unix)]
    fn connect_ipc(&mut self) -> Result<(), IpcError> {
        let path = Self::find_pipe().ok_or(IpcError::IPCNotFound)?;
        let socket = Stream::connect(path).map_err(|_| IpcError::IPCConnectionFailed)?;
        socket
            .set_read_timeout(Some(REPLY_TIMEOUT))
            .and_then(|()| socket.set_write_timeout(Some(REPLY_TIMEOUT)))
            .map_err(|_| IpcError::IPCConnectionFailed)?;
        self.socket = Some(socket);
        Ok(())
    }

    /// A named pipe opened as a file has no timeout to set: Windows keeps the
    /// library's blocking behaviour here.
    #[cfg(windows)]
    fn connect_ipc(&mut self) -> Result<(), IpcError> {
        use std::os::windows::fs::OpenOptionsExt;
        for index in 0..10 {
            let path = format!(r"\\?\pipe\discord-ipc-{index}");
            if let Ok(handle) = std::fs::OpenOptions::new().access_mode(0x3).open(path) {
                self.socket = Some(handle);
                return Ok(());
            }
        }
        Err(IpcError::IPCConnectionFailed)
    }

    fn write(&mut self, data: &[u8]) -> Result<(), IpcError> {
        use std::io::Write;
        let socket = self.socket.as_mut().ok_or(IpcError::NotConnected)?;
        socket.write_all(data).map_err(IpcError::WriteError)
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<(), IpcError> {
        use std::io::Read;
        let socket = self.socket.as_mut().ok_or(IpcError::NotConnected)?;
        socket.read_exact(buffer).map_err(IpcError::ReadError)
    }

    fn close(&mut self) -> Result<(), IpcError> {
        use std::io::Write;
        let _ = self.send(json!({}), OP_CLOSE as u8);
        let socket = self.socket.as_mut().ok_or(IpcError::NotConnected)?;
        socket.flush().map_err(IpcError::FlushError)?;
        #[cfg(unix)]
        let _ = socket.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
}

/// One live connection to Discord, and what it last managed to send.
struct Link {
    client: Pipe,
    app_id: String,
    sent: Option<Presence>,
    /// Earliest moment the next update may go out.
    next_send: Instant,
}

impl Link {
    fn open(app_id: &str) -> Result<Self, String> {
        Ok(Self {
            client: Pipe::open(app_id)?,
            app_id: app_id.to_string(),
            sent: None,
            next_send: Instant::now(),
        })
    }

    fn push(&mut self, want: &Presence) -> Result<(), String> {
        self.client.push(want)?;
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

/// Handle held by the `Core` (lib.rs), shared with the worker thread.
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
        if self
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
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

pub fn discord_presence_set(presence: &DiscordPresence, wanted: Option<Presence>) {
    presence.set(wanted);
}

pub fn discord_presence_status(presence: &DiscordPresence) -> Status {
    presence.status()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Discord's end of the pipe, played by the test.
    struct Discord(Stream);

    impl Discord {
        fn pair(client_id: &str) -> (Pipe, Discord) {
            let (ours, theirs) = Stream::pair().unwrap();
            ours.set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            let mut pipe = Pipe::new(client_id);
            pipe.socket = Some(ours);
            (pipe, Discord(theirs))
        }

        fn read_frame(&mut self) -> (u32, Value) {
            let mut header = [0u8; 8];
            self.0.read_exact(&mut header).unwrap();
            let op = u32::from_le_bytes(header[..4].try_into().unwrap());
            let len = u32::from_le_bytes(header[4..].try_into().unwrap());
            let mut body = vec![0u8; len as usize];
            self.0.read_exact(&mut body).unwrap();
            (op, serde_json::from_slice(&body).unwrap())
        }

        fn reply(&mut self, op: u32, body: Value) {
            let body = body.to_string();
            self.0.write_all(&op.to_le_bytes()).unwrap();
            self.0
                .write_all(&(body.len() as u32).to_le_bytes())
                .unwrap();
            self.0.write_all(body.as_bytes()).unwrap();
        }
    }

    fn presence() -> Presence {
        Presence {
            app_id: "123".into(),
            details: "Working".into(),
            state: "on hangar".into(),
            large_image: "logo".into(),
            large_text: "Hangar".into(),
            large_url: None,
            small_image: None,
            small_text: None,
            started_at: None,
            button_label: None,
            button_url: None,
        }
    }

    #[test]
    fn the_handshake_waits_for_ready() {
        let (mut pipe, mut discord) = Discord::pair("123");
        let server = std::thread::spawn(move || {
            let (op, hello) = discord.read_frame();
            assert_eq!(op, 0);
            assert_eq!(hello["client_id"], "123");
            discord.reply(OP_FRAME, json!({ "cmd": "DISPATCH", "evt": "READY" }));
            discord
        });
        pipe.handshake().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_rejected_app_id_is_not_a_connection() {
        let (mut pipe, mut discord) = Discord::pair("bad");
        let server = std::thread::spawn(move || {
            discord.read_frame();
            discord.reply(
                OP_CLOSE,
                json!({ "code": 4000, "message": "Invalid Client ID" }),
            );
            discord
        });
        let err = pipe.handshake().unwrap_err();
        assert!(err.contains("Invalid Client ID"), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn an_error_answer_to_set_activity_is_a_failure() {
        let (mut pipe, mut discord) = Discord::pair("123");
        let server = std::thread::spawn(move || {
            let (op, command) = discord.read_frame();
            assert_eq!(op, 1);
            assert_eq!(command["cmd"], "SET_ACTIVITY");
            discord.reply(
                OP_FRAME,
                json!({ "cmd": "SET_ACTIVITY", "evt": "ERROR", "data": { "code": 4000, "message": "child \"activity\" fails" } }),
            );
            discord
        });
        let err = pipe.push(&presence()).unwrap_err();
        assert!(err.contains("refused"), "{err}");
        server.join().unwrap();
    }

    #[test]
    fn an_accepted_activity_is_a_success() {
        let (mut pipe, mut discord) = Discord::pair("123");
        let server = std::thread::spawn(move || {
            discord.read_frame();
            discord.reply(
                OP_FRAME,
                json!({ "cmd": "SET_ACTIVITY", "evt": null, "data": {} }),
            );
            discord
        });
        pipe.push(&presence()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_discord_that_never_answers_does_not_hold_the_worker() {
        let (mut pipe, discord) = Discord::pair("123");
        let started = Instant::now();
        assert!(pipe.handshake().is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        drop(discord);
    }
}
