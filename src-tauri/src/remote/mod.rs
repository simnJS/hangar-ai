//! Hangar Mobile, the PC's side: the phone's web app and its API, served on
//! 127.0.0.1 for Tailscale Serve to publish on the person's tailnet.
//!
//! Off until the person turns it on. The phone reaches the PC at
//! `https://<pc>.<tailnet>.ts.net`, which Tailscale Serve proxies to this
//! listener: no port is opened to the network, and nothing here listens past
//! the loopback interface.
//!
//! It is a listener apart from the agents' API (server.rs at the crate root),
//! with its own auth: a phone's cookie opens none of the agents' routes, and
//! no agent's token opens these. What a phone asks of the panes is relayed to
//! the window as an internal request (control.rs), `remote_<op>`, which the
//! window answers.
//!
//! - store.rs: what is kept on disk, in remote.json.
//! - files.rs: the files that go both ways, shared by an agent or uploaded
//!   by the phone.
//! - pairing.rs: the six-digit code and the QR code a phone pairs with.
//! - push.rs: the notifications the PC pushes to the phones, app closed.
//! - server.rs: the listener, its routes and their doors.
//! - assets.rs: the web app, compiled in from dist-mobile/.
//! - tailscale.rs: the CLI, asked for the PC's name and told to serve.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::HeaderMap;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::control::{self, ControlError, ControlRequest};

mod assets;
mod files;
mod pairing;
mod push;
mod server;
mod store;
mod tailscale;

use files::Shares;
pub use files::{NewShare, Share};
use pairing::{PairError, Pairing};
use push::{Delivery, Message, PushKey, Subscription, Text};
use store::{Device, DeviceView, Store};
use tailscale::TailscaleStatus;

/// Beside state.json, in the app data dir.
const FILE: &str = "remote.json";

/// The web app's side of the contract: a window that has not answered a call
/// in 10 s is a PC too busy to, and the phone says so rather than waiting on.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// The main window, as the mobile server reaches it: a phone's calls go out
/// as internal requests (control.rs), and a phone paired or revoked is
/// announced as `remote:changed`, for the settings to redraw their list.
///
/// Two closures rather than the AppHandle itself, so the tests can stand in
/// for the window — and so a test binary never links the webview runtime,
/// which on Windows does not even load without the app's manifest.
#[derive(Clone)]
struct Window {
    send: Arc<dyn Fn(ControlRequest) -> Result<(), String> + Send + Sync>,
    changed: Arc<dyn Fn() + Send + Sync>,
    /// A file shared with the phones, announced as `remote:shared`, for the
    /// push to listen to.
    shared: Arc<dyn Fn(&Share) + Send + Sync>,
}

impl Window {
    fn of(app: &AppHandle) -> Self {
        let events = app.clone();
        let changes = app.clone();
        let shares = app.clone();
        Self {
            send: Arc::new(move |request| control::emit_internal(&events, request)),
            changed: Arc::new(move || {
                let _ = changes.emit("remote:changed", ());
            }),
            shared: Arc::new(move |share| {
                let _ = shares.emit("remote:shared", share);
            }),
        }
    }

    async fn ask(&self, op: &str, args: Value, limit: Duration) -> Result<Value, ControlError> {
        control::ask_internal_via(|request| (self.send)(request), op, args, limit).await
    }

    fn changed(&self) {
        (self.changed)()
    }

    fn shared(&self, share: &Share) {
        (self.shared)(share)
    }
}

pub struct Remote {
    store: Store,
    /// The code the settings are showing, while they are.
    pairing: Mutex<Option<Pairing>>,
    listening: tokio::sync::Mutex<Listening>,
    window: Window,
    /// What the phone calls this PC.
    pc: String,
    call_timeout: Duration,
    assets: assets::Lookup,
    shares: Arc<Shares>,
    /// The PC's VAPID key, read or made the first time it is needed.
    push_key: Mutex<Option<Arc<PushKey>>>,
    push_key_file: PathBuf,
    push_agent: ureq::Agent,
}

#[derive(Default)]
struct Listening {
    running: Option<server::Running>,
    /// Why the last start failed: the port taken, most likely.
    error: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairStart {
    pub code: String,
    /// `https://<dnsName>/?code=<code>`, what the QR code holds.
    pub url: String,
    pub qr_svg: String,
    /// Milliseconds since the epoch.
    pub expires_at: u64,
}

/// What became of a push: phones that took it, subscriptions forgotten
/// because their push service said they are gone, and failures, tried again
/// with the next one.
#[derive(Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushOutcome {
    pub sent: usize,
    pub dropped: usize,
    pub failed: usize,
}

impl Remote {
    pub fn open(app: &AppHandle) -> Result<Arc<Self>, String> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|err| format!("no app data dir: {err}"))?;
        Ok(Arc::new(Self::new(
            dir.join(FILE),
            Window::of(app),
            CALL_TIMEOUT,
            assets::embedded,
        )))
    }

    fn new(file: PathBuf, window: Window, call_timeout: Duration, assets: assets::Lookup) -> Self {
        // <app data>/remote/shares/, beside remote.json.
        let shares = Shares::open(
            file.parent()
                .unwrap_or_else(|| std::path::Path::new(""))
                .join("remote")
                .join("shares"),
        );
        Self {
            push_key_file: push::key_file(&file),
            store: Store::open(file),
            pairing: Mutex::new(None),
            listening: Default::default(),
            window,
            pc: pc_name(),
            call_timeout,
            assets,
            shares: Arc::new(shares),
            push_key: Mutex::new(None),
            push_agent: push::agent(),
        }
    }

    /// At launch: listening again if the person left it on. A start that
    /// fails leaves it on, so the next launch tries again, and the settings
    /// show why it is not running.
    pub async fn resume(self: &Arc<Self>) {
        if self.store.read().enabled {
            if let Err(err) = self.start().await {
                eprintln!("Hangar Mobile could not start: {err}");
            }
        }
    }

    async fn start(self: &Arc<Self>) -> Result<(), String> {
        let mut listening = self.listening.lock().await;
        if listening.running.is_some() {
            return Ok(());
        }
        match server::listen(self.clone(), self.store.read().port).await {
            Ok(running) => {
                listening.running = Some(running);
                listening.error = None;
                Ok(())
            }
            Err(err) => {
                listening.error = Some(err.clone());
                Err(err)
            }
        }
    }

    async fn stop(&self) {
        let mut listening = self.listening.lock().await;
        if let Some(running) = listening.running.take() {
            running.stop();
        }
        listening.error = None;
        *self.pairing.lock().unwrap() = None;
    }

    /// Turning it on that fails — the port taken — leaves it off, and says
    /// why: the person is looking at the switch.
    pub async fn set_enabled(self: &Arc<Self>, enabled: bool) -> Result<RemoteStatus, String> {
        if enabled {
            self.start().await?;
            if let Err(err) = self.store.update(|saved| saved.enabled = true) {
                self.stop().await;
                return Err(err);
            }
        } else {
            self.stop().await;
            self.store.update(|saved| saved.enabled = false)?;
        }
        Ok(self.status().await)
    }

    pub async fn status(&self) -> RemoteStatus {
        let saved = self.store.read();
        let listening = self.listening.lock().await;
        RemoteStatus {
            enabled: saved.enabled,
            running: listening.running.is_some(),
            port: listening
                .running
                .as_ref()
                .map_or(saved.port, server::Running::port),
            error: listening.error.clone(),
        }
    }

    /// A new code for a phone to pair with, replacing any before it. It needs
    /// the server running and the PC's name on the tailnet, which is where
    /// the QR code sends the phone.
    pub async fn pair_start(&self) -> Result<PairStart, String> {
        if self.listening.lock().await.running.is_none() {
            return Err("Hangar Mobile is off: turn it on first.".to_string());
        }
        let tailscale = tailscale::status(self.store.read().port).await;
        match tailscale.dns_name {
            Some(dns_name) => self.begin_pairing(&dns_name),
            None if tailscale.installed => {
                Err("Tailscale is not connected on this PC: open it and sign in.".to_string())
            }
            None => Err("Tailscale is not installed on this PC.".to_string()),
        }
    }

    fn begin_pairing(&self, dns_name: &str) -> Result<PairStart, String> {
        let pairing = Pairing::new(now_ms());
        let url = format!("https://{dns_name}/?code={}", pairing.code);
        let start = PairStart {
            code: pairing.code.clone(),
            qr_svg: pairing::qr_svg(&url)?,
            url,
            expires_at: pairing.expires_at,
        };
        *self.pairing.lock().unwrap() = Some(pairing);
        Ok(start)
    }

    /// Pairs the phone that sent the current code: a device with a token of
    /// its own, saved hashed. The token goes back once, to become its cookie.
    fn pair(&self, code: &str, name: &str) -> Result<(DeviceView, String), PairError> {
        let now = now_ms();
        pairing::redeem(&mut self.pairing.lock().unwrap(), code, now)?;
        let token = crate::endpoint::generate_token();
        let device = Device {
            id: uuid::Uuid::new_v4().to_string(),
            name: pairing::device_name(name),
            token_hash: store::token_hash(&token),
            created: now,
            last_seen: now,
            push: None,
        };
        let view = device.view();
        self.store
            .update(|saved| saved.devices.push(device))
            .map_err(PairError::Storage)?;
        self.window.changed();
        Ok((view, token))
    }

    /// The phone a request's cookie belongs to, if any.
    fn device(&self, headers: &HeaderMap) -> Option<DeviceView> {
        self.store.find(server::cookie_token(headers)?, now_ms())
    }

    pub fn devices(&self) -> Vec<DeviceView> {
        self.store.read().devices.iter().map(Device::view).collect()
    }

    /// Forgets a phone: its very next request is turned away, and nothing is
    /// pushed to it again — its subscription was kept with it. Revoking one
    /// already gone is no error.
    pub fn revoke(&self, id: &str) -> Result<(), String> {
        self.store
            .update(|saved| saved.devices.retain(|device| device.id != id))?;
        self.window.changed();
        Ok(())
    }

    /// An agent's `phone_share`: the file copied in and announced. Refused,
    /// with words the agent can pass on, while Hangar Mobile is off or no
    /// phone is paired to see it.
    pub async fn share(&self, new: NewShare) -> Result<Share, String> {
        if self.listening.lock().await.running.is_none() {
            return Err(
                "Hangar Mobile is off: the person can turn it on in Settings › Mobile".to_string(),
            );
        }
        if self.store.read().devices.is_empty() {
            return Err(
                "No phone is paired with Hangar Mobile: the person can pair one in Settings › Mobile"
                    .to_string(),
            );
        }
        let shares = self.shares.clone();
        let share = tokio::task::spawn_blocking(move || shares.add(new, now_ms()))
            .await
            .map_err(|err| format!("the share failed: {err}"))??;
        self.window.shared(&share);
        Ok(share)
    }

    /// The PC's VAPID key, made the first time it is needed and kept.
    fn push_key(&self) -> Result<Arc<PushKey>, String> {
        let mut kept = self.push_key.lock().unwrap();
        if let Some(key) = kept.as_ref() {
            return Ok(key.clone());
        }
        let key = Arc::new(PushKey::open(&self.push_key_file)?);
        *kept = Some(key.clone());
        Ok(key)
    }

    /// Where a phone's notifications go from now on: the one subscription a
    /// device has, replaced, and taken from any other device that had the
    /// same address — a phone paired again keeps its browser's subscription,
    /// and would otherwise get everything twice. False when the device is
    /// no longer paired.
    fn subscribe(&self, device: &str, subscription: Subscription) -> Result<bool, String> {
        let same = |kept: &Subscription| {
            kept.endpoint == subscription.endpoint
                && kept.p256dh == subscription.p256dh
                && kept.auth == subscription.auth
                && kept.lang == subscription.lang
        };
        let saved = self.store.read();
        let Some(phone) = saved.devices.iter().find(|phone| phone.id == device) else {
            return Ok(false);
        };
        let shared = saved.devices.iter().any(|other| {
            other.id != device
                && other
                    .push
                    .as_ref()
                    .is_some_and(|kept| kept.endpoint == subscription.endpoint)
        });
        // The phone says it again each time the app opens: nothing to write.
        if phone.push.as_ref().is_some_and(same) && !shared {
            return Ok(true);
        }
        let found = self.store.update(|saved| {
            let mut found = false;
            for phone in &mut saved.devices {
                if phone.id == device {
                    phone.push = Some(subscription.clone());
                    found = true;
                } else if phone
                    .push
                    .as_ref()
                    .is_some_and(|kept| kept.endpoint == subscription.endpoint)
                {
                    phone.push = None;
                }
            }
            found
        })?;
        if found {
            self.window.changed();
        }
        Ok(found)
    }

    /// The phone turned its notifications off.
    fn unsubscribe(&self, device: &str) -> Result<(), String> {
        let subscribed = self
            .store
            .read()
            .devices
            .iter()
            .any(|phone| phone.id == device && phone.push.is_some());
        if !subscribed {
            return Ok(());
        }
        self.store.update(|saved| {
            for phone in &mut saved.devices {
                if phone.id == device {
                    phone.push = None;
                }
            }
        })?;
        self.window.changed();
        Ok(())
    }

    /// How many phones take notifications: none while Hangar Mobile is off,
    /// which is when the window has no reason to watch for them.
    pub async fn push_subscribers(&self) -> usize {
        if self.listening.lock().await.running.is_none() {
            return 0;
        }
        self.store
            .read()
            .devices
            .iter()
            .filter(|phone| phone.push.is_some())
            .count()
    }

    /// The window's `remote_push`: one notification to every subscribed
    /// phone, each in its own language. A subscription its push service
    /// says is gone (404, 410) is forgotten; nothing goes out while Hangar
    /// Mobile is off.
    pub async fn push(&self, message: Message) -> Result<PushOutcome, String> {
        if self.listening.lock().await.running.is_none() {
            return Ok(PushOutcome::default());
        }
        let targets: Vec<Subscription> = self
            .store
            .read()
            .devices
            .into_iter()
            .filter_map(|phone| phone.push)
            .collect();
        if targets.is_empty() {
            return Ok(PushOutcome::default());
        }
        let key = self.push_key()?;
        let agent = self.push_agent.clone();
        let delivered = tokio::task::spawn_blocking(move || {
            targets
                .into_iter()
                .map(|target| {
                    let payload = push::payload(&message, target.lang);
                    let delivery = push::deliver(&agent, &key, &target, payload);
                    (target.endpoint, delivery)
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|err| format!("the push failed: {err}"))?;

        let mut outcome = PushOutcome::default();
        let mut gone = Vec::new();
        for (endpoint, delivery) in delivered {
            match delivery {
                Delivery::Delivered => outcome.sent += 1,
                Delivery::Gone => gone.push(endpoint),
                Delivery::Failed(err) => {
                    outcome.failed += 1;
                    eprintln!("Hangar Mobile could not push to a phone: {err}");
                }
            }
        }
        if !gone.is_empty() {
            self.store.update(|saved| {
                for phone in &mut saved.devices {
                    if phone
                        .push
                        .as_ref()
                        .is_some_and(|kept| gone.contains(&kept.endpoint))
                    {
                        phone.push = None;
                    }
                }
            })?;
            outcome.dropped = gone.len();
            self.window.changed();
        }
        Ok(outcome)
    }

    pub async fn tailscale(&self) -> TailscaleStatus {
        tailscale::status(self.store.read().port).await
    }

    pub async fn tailscale_serve(&self) -> Result<TailscaleStatus, String> {
        tailscale::serve(self.store.read().port).await
    }
}

/// The computer's own name, as its OS knows it.
fn pc_name() -> String {
    let name = os_name().unwrap_or_default();
    let name = name.trim().trim_end_matches(".local");
    if name.is_empty() {
        "PC".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(windows)]
fn os_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

#[cfg(unix)]
fn os_name() -> Option<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: gethostname writes at most `len` bytes into the buffer it is
    // handed, which outlives the call.
    let failed = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0;
    if failed {
        return None;
    }
    let end = buffer
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

#[cfg(not(any(windows, unix)))]
fn os_name() -> Option<String> {
    None
}

// ---------------------------------------------------------------------------
// The settings' commands (Settings → Hangar Mobile)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn remote_status(remote: State<'_, Arc<Remote>>) -> Result<RemoteStatus, String> {
    Ok(remote.status().await)
}

#[tauri::command]
pub async fn remote_set_enabled(
    remote: State<'_, Arc<Remote>>,
    enabled: bool,
) -> Result<RemoteStatus, String> {
    remote.inner().set_enabled(enabled).await
}

#[tauri::command]
pub async fn remote_pair_start(remote: State<'_, Arc<Remote>>) -> Result<PairStart, String> {
    remote.pair_start().await
}

#[tauri::command]
pub fn remote_devices(remote: State<'_, Arc<Remote>>) -> Vec<DeviceView> {
    remote.devices()
}

#[tauri::command]
pub fn remote_revoke(remote: State<'_, Arc<Remote>>, id: String) -> Result<(), String> {
    remote.revoke(&id)
}

#[tauri::command]
pub async fn remote_tailscale(remote: State<'_, Arc<Remote>>) -> Result<TailscaleStatus, String> {
    Ok(remote.tailscale().await)
}

/// Runs `tailscale serve`: only ever from the person's click in the settings.
#[tauri::command]
pub async fn remote_tailscale_serve(
    remote: State<'_, Arc<Remote>>,
) -> Result<TailscaleStatus, String> {
    remote.tailscale_serve().await
}

// ---------------------------------------------------------------------------
// The window's push (lib/remotePush)
// ---------------------------------------------------------------------------

/// A notification to every subscribed phone. `title` and `body` are one text
/// or `{ en, fr }`, each phone getting its language; `url` is the screen it
/// opens, `./#/pane/<id>`, `./#/shares`, `./#/`.
#[tauri::command]
pub async fn remote_push(
    remote: State<'_, Arc<Remote>>,
    title: Text,
    body: Text,
    url: String,
) -> Result<PushOutcome, String> {
    remote.push(Message { title, body, url }).await
}

/// How many phones take notifications, for the window to know whether to
/// watch for what it would push.
#[tauri::command]
pub async fn remote_push_subscribers(remote: State<'_, Arc<Remote>>) -> Result<usize, String> {
    Ok(remote.push_subscribers().await)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::json;

    type Answer = Option<Result<Value, String>>;

    /// The window, played by the tests: it notes each op it is asked and
    /// answers as told — or not at all.
    #[derive(Clone)]
    pub struct Fake {
        pub asked: Arc<Mutex<Vec<String>>>,
        /// What was announced as `remote:shared`.
        pub shared: Arc<Mutex<Vec<Share>>>,
        /// How many times `remote:changed` was.
        pub changes: Arc<Mutex<usize>>,
        answer: Arc<dyn Fn(&ControlRequest) -> Answer + Send + Sync>,
    }

    impl Fake {
        pub fn answering(
            answer: impl Fn(&ControlRequest) -> Answer + Send + Sync + 'static,
        ) -> Self {
            Self {
                asked: Default::default(),
                shared: Default::default(),
                changes: Default::default(),
                answer: Arc::new(answer),
            }
        }

        /// Answers with what it was asked.
        pub fn echo() -> Self {
            Self::answering(|request| Some(Ok(json!({ "op": request.op, "args": request.args }))))
        }

        pub fn silent() -> Self {
            Self::answering(|_| None)
        }

        pub fn refusing(reason: &'static str) -> Self {
            Self::answering(move |_| Some(Err(reason.to_string())))
        }

        fn window(&self) -> Window {
            let fake = self.clone();
            Window {
                send: Arc::new(move |request: ControlRequest| {
                    fake.asked.lock().unwrap().push(request.op.clone());
                    match (fake.answer)(&request) {
                        Some(Ok(value)) => {
                            control::control_reply(request.id, true, Some(value), None)
                        }
                        Some(Err(reason)) => {
                            control::control_reply(request.id, false, None, Some(reason))
                        }
                        None => {}
                    }
                    Ok(())
                }),
                changed: {
                    let fake = self.clone();
                    Arc::new(move || *fake.changes.lock().unwrap() += 1)
                },
                shared: {
                    let fake = self.clone();
                    Arc::new(move |share: &Share| fake.shared.lock().unwrap().push(share.clone()))
                },
            }
        }
    }

    /// A Remote of its own, saving to a folder no other test uses, with a
    /// window that runs out of patience in 200 ms rather than 10 s.
    pub fn remote_with(fake: Fake, assets: assets::Lookup) -> Arc<Remote> {
        Arc::new(Remote::new(
            store::tests::scratch_file("remote"),
            fake.window(),
            Duration::from_millis(200),
            assets,
        ))
    }

    /// A port from the share Hangar hands a worker running beside others
    /// (HANGAR_PORT_BASE), or any free one where there is no such share.
    pub fn test_port(offset: u16) -> u16 {
        std::env::var("HANGAR_PORT_BASE")
            .ok()
            .and_then(|base| base.parse::<u16>().ok())
            .map_or(0, |base| base + offset)
    }

    #[tokio::test]
    async fn it_is_off_until_turned_on_and_remembers_the_switch() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        remote
            .store
            .update(|saved| saved.port = test_port(1))
            .unwrap();
        let status = remote.status().await;
        assert!(!status.enabled && !status.running);

        let status = remote.set_enabled(true).await.unwrap();
        assert!(status.enabled && status.running, "{status:?}");
        assert_ne!(status.port, 0);
        let file = remote.store.file().to_path_buf();
        assert!(Store::open(file.clone()).read().enabled);

        let status = remote.set_enabled(false).await.unwrap();
        assert!(!status.enabled && !status.running, "{status:?}");
        assert!(!Store::open(file).read().enabled);
    }

    #[tokio::test]
    async fn off_and_on_again_listens_again() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        remote
            .store
            .update(|saved| saved.port = test_port(2))
            .unwrap();
        for _ in 0..3 {
            assert!(remote.set_enabled(true).await.unwrap().running);
            assert!(!remote.set_enabled(false).await.unwrap().running);
        }
    }

    #[tokio::test]
    async fn a_port_already_taken_leaves_it_off_and_says_why() {
        let taken = std::net::TcpListener::bind(("127.0.0.1", test_port(3))).unwrap();
        let port = taken.local_addr().unwrap().port();
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        remote.store.update(|saved| saved.port = port).unwrap();

        let refused = remote.set_enabled(true).await.unwrap_err();
        assert!(refused.contains(&port.to_string()), "{refused}");
        let status = remote.status().await;
        assert!(!status.enabled && !status.running);
        assert_eq!(status.error.as_deref(), Some(refused.as_str()));
    }

    #[tokio::test]
    async fn a_launch_with_it_left_on_listens_again() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        remote
            .store
            .update(|saved| {
                saved.enabled = true;
                saved.port = test_port(4);
            })
            .unwrap();
        remote.resume().await;
        assert!(remote.status().await.running);
        remote.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn pairing_needs_the_server_running() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        let refused = remote.pair_start().await.unwrap_err();
        assert!(refused.contains("off"), "{refused}");
    }

    #[test]
    fn a_pairing_shows_the_code_and_a_qr_code_of_the_address() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        let start = remote.begin_pairing("atelier.tail1234.ts.net").unwrap();
        assert_eq!(start.code.len(), 6);
        assert_eq!(
            start.url,
            format!("https://atelier.tail1234.ts.net/?code={}", start.code)
        );
        assert!(start.qr_svg.contains("<svg"));
        let now = now_ms();
        assert!(start.expires_at > now && start.expires_at <= now + pairing::LIFETIME_MS);

        let shown = serde_json::to_value(&start).unwrap();
        for key in ["code", "url", "qrSvg", "expiresAt"] {
            assert!(shown.get(key).is_some(), "{key} missing from {shown}");
        }
        // A new pairing replaces the last: only the newest code pairs.
        let newer = remote.begin_pairing("atelier.tail1234.ts.net").unwrap();
        if newer.code != start.code {
            assert_eq!(
                remote.pair(&start.code, "Old"),
                Err(PairError::Wrong { left: 4 })
            );
        }
        assert!(remote.pair(&newer.code, "New").is_ok());
    }

    #[test]
    fn the_settings_never_see_a_token_hash() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        remote.pair(&code, "Pixel").unwrap();
        let listed = serde_json::to_value(remote.devices()).unwrap();
        let device = listed[0].as_object().unwrap();
        let mut keys: Vec<&str> = device.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["created", "id", "lastSeen", "name"]);
    }

    fn new_share(source: PathBuf) -> NewShare {
        NewShare {
            source,
            caption: Some("the render".into()),
            workspace: "C:/work".into(),
            pane: "Ava".into(),
        }
    }

    fn a_file(tag: &str) -> PathBuf {
        let dir = store::tests::scratch_file(tag)
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("shot.png");
        std::fs::write(&file, b"png").unwrap();
        file
    }

    fn pair_a_phone(remote: &Remote) {
        let code = remote
            .begin_pairing("atelier.tail1234.ts.net")
            .unwrap()
            .code;
        remote.pair(&code, "Pixel").unwrap();
    }

    #[tokio::test]
    async fn a_share_needs_hangar_mobile_on() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        pair_a_phone(&remote);

        let refused = remote.share(new_share(a_file("off"))).await.unwrap_err();
        assert_eq!(
            refused,
            "Hangar Mobile is off: the person can turn it on in Settings › Mobile"
        );
        assert!(remote.shares.list(0).is_empty());
    }

    #[tokio::test]
    async fn a_share_needs_a_paired_phone() {
        let remote = remote_with(Fake::echo(), assets::tests::no_app);
        remote
            .store
            .update(|saved| saved.port = test_port(5))
            .unwrap();
        remote.set_enabled(true).await.unwrap();

        let refused = remote
            .share(new_share(a_file("unpaired")))
            .await
            .unwrap_err();
        assert!(refused.contains("No phone is paired"), "{refused}");
        assert!(remote.shares.list(0).is_empty());
        remote.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn a_share_is_copied_recorded_and_announced() {
        let window = Fake::echo();
        let remote = remote_with(window.clone(), assets::tests::no_app);
        remote
            .store
            .update(|saved| saved.port = test_port(6))
            .unwrap();
        remote.set_enabled(true).await.unwrap();
        pair_a_phone(&remote);

        let source = a_file("shared");
        let share = remote.share(new_share(source.clone())).await.unwrap();
        std::fs::remove_file(source).unwrap();
        assert_eq!(share.name, "shot.png");
        assert_eq!(share.pane, "Ava");
        assert_eq!(remote.shares.list(0), vec![share.clone()]);
        let (_, copy) = remote.shares.find(&share.id).unwrap();
        assert_eq!(std::fs::read(copy).unwrap(), b"png");
        assert_eq!(*window.shared.lock().unwrap(), vec![share.clone()]);

        // What the push listens to carries the whole record.
        let announced = serde_json::to_value(&share).unwrap();
        for key in [
            "id",
            "at",
            "workspace",
            "pane",
            "name",
            "mime",
            "size",
            "caption",
        ] {
            assert!(announced.get(key).is_some(), "{key} missing: {announced}");
        }
        remote.set_enabled(false).await.unwrap();
    }

    #[test]
    fn the_pc_has_a_name() {
        assert!(!pc_name().is_empty());
    }

    // -- Push ---------------------------------------------------------------

    /// A request a push service took: its path, headers and body.
    type Received = (String, axum::http::HeaderMap, Vec<u8>);

    /// A push service on the loopback: it answers 410 to an address ending
    /// in `gone`, 404 to one ending in `missing`, 500 to `flaky`, and 201
    /// (Created, as the real ones do) to anything else.
    async fn push_service(port: u16) -> (String, Arc<Mutex<Vec<Received>>>) {
        use axum::extract::State as Shared;
        use axum::http::{StatusCode, Uri};
        let received: Arc<Mutex<Vec<Received>>> = Default::default();
        let app = axum::Router::new()
            .fallback(
                |Shared(received): Shared<Arc<Mutex<Vec<Received>>>>,
                 uri: Uri,
                 headers: axum::http::HeaderMap,
                 body: axum::body::Bytes| async move {
                    let path = uri.path().to_string();
                    let status = if path.ends_with("gone") {
                        StatusCode::GONE
                    } else if path.ends_with("missing") {
                        StatusCode::NOT_FOUND
                    } else if path.ends_with("flaky") {
                        StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        StatusCode::CREATED
                    };
                    received
                        .lock()
                        .unwrap()
                        .push((path, headers, body.to_vec()));
                    status
                },
            )
            .with_state(received.clone());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), received)
    }

    /// Hangar Mobile on, on a port of its own, with two phones paired.
    async fn two_phones(window: Fake, port: u16) -> (Arc<Remote>, String, String) {
        let remote = remote_with(window, assets::tests::no_app);
        remote.store.update(|saved| saved.port = port).unwrap();
        remote.set_enabled(true).await.unwrap();
        pair_a_phone(&remote);
        pair_a_phone(&remote);
        let ids: Vec<String> = remote.devices().into_iter().map(|d| d.id).collect();
        (remote, ids[0].clone(), ids[1].clone())
    }

    fn waits(lang_en: &str, lang_fr: &str) -> Message {
        Message {
            title: Text::ByLang {
                en: lang_en.into(),
                fr: Some(lang_fr.into()),
            },
            body: Text::Same("Shop".into()),
            url: "./#/pane/p1".into(),
        }
    }

    #[tokio::test]
    async fn a_push_reaches_each_phone_encrypted_signed_and_in_its_language() {
        let (service, received) = push_service(test_port(11)).await;
        let (remote, en, fr) = two_phones(Fake::echo(), test_port(10)).await;
        let phone = |lang| push::tests::rfc_subscription(&format!("{service}/push/{lang:?}"), lang);
        assert!(remote.subscribe(&en, phone(push::Lang::En)).unwrap());
        assert!(remote.subscribe(&fr, phone(push::Lang::Fr)).unwrap());
        assert_eq!(remote.push_subscribers().await, 2);

        let outcome = remote
            .push(waits("Ava waits for you", "Ava vous attend"))
            .await
            .unwrap();
        assert_eq!(
            outcome,
            PushOutcome {
                sent: 2,
                dropped: 0,
                failed: 0
            }
        );
        let received = received.lock().unwrap().clone();
        assert_eq!(received.len(), 2);
        let key = remote.push_key().unwrap();
        for (path, headers, body) in received {
            assert_eq!(headers["content-encoding"], "aes128gcm");
            assert_eq!(headers["ttl"], (12 * 60 * 60).to_string());
            assert_eq!(headers["urgency"], "high");
            let authorization = headers["authorization"].to_str().unwrap();
            assert!(authorization.ends_with(&format!(", k={}", key.public())));
            let claims = push::tests::check_vapid(authorization);
            assert_eq!(claims["aud"], service);
            let read = push::tests::open_as_the_rfc_browser(body);
            let title = if path.ends_with("Fr") {
                "Ava vous attend"
            } else {
                "Ava waits for you"
            };
            assert_eq!(
                read,
                serde_json::json!({ "title": title, "body": "Shop", "url": "./#/pane/p1" })
            );
        }
        remote.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn a_subscription_its_service_calls_gone_is_dropped() {
        let (service, received) = push_service(test_port(13)).await;
        let (remote, first, second) = two_phones(Fake::echo(), test_port(12)).await;
        let third = {
            pair_a_phone(&remote);
            remote.devices()[2].id.clone()
        };
        for (device, path) in [(&first, "gone"), (&second, "missing"), (&third, "flaky")] {
            let subscription =
                push::tests::rfc_subscription(&format!("{service}/push/{path}"), push::Lang::En);
            remote.subscribe(device, subscription).unwrap();
        }

        let outcome = remote.push(waits("a", "b")).await.unwrap();
        assert_eq!(
            outcome,
            PushOutcome {
                sent: 0,
                dropped: 2,
                failed: 1
            }
        );
        assert_eq!(received.lock().unwrap().len(), 3);
        let kept: Vec<Option<String>> = remote
            .store
            .read()
            .devices
            .into_iter()
            .map(|phone| phone.push.map(|push| push.endpoint))
            .collect();
        // 410 and 404 are forgotten, on disk too; a 500 is tried next time.
        assert_eq!(kept, [None, None, Some(format!("{service}/push/flaky"))]);
        let reopened = Store::open(remote.store.file().to_path_buf()).read();
        assert!(reopened.devices[0].push.is_none() && reopened.devices[2].push.is_some());
        assert_eq!(remote.push_subscribers().await, 1);

        // The next push goes to the one left only.
        remote.push(waits("a", "b")).await.unwrap();
        assert_eq!(received.lock().unwrap().len(), 4);
        remote.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn a_revoked_phone_takes_its_subscription_along() {
        let (service, received) = push_service(test_port(15)).await;
        let (remote, revoked, kept) = two_phones(Fake::echo(), test_port(14)).await;
        let at = |path: &str| {
            push::tests::rfc_subscription(&format!("{service}/{path}"), push::Lang::En)
        };
        remote.subscribe(&revoked, at("revoked")).unwrap();
        remote.subscribe(&kept, at("kept")).unwrap();
        assert_eq!(remote.push_subscribers().await, 2);

        remote.revoke(&revoked).unwrap();
        assert_eq!(remote.push_subscribers().await, 1);
        let outcome = remote.push(waits("a", "b")).await.unwrap();
        assert_eq!(outcome.sent, 1);
        let paths: Vec<String> = received
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.0.clone())
            .collect();
        assert_eq!(paths, ["/kept"]);
        // A device no longer paired cannot subscribe again.
        assert!(!remote.subscribe(&revoked, at("revoked")).unwrap());
        assert_eq!(remote.push_subscribers().await, 1);
        remote.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn one_address_belongs_to_one_phone_and_saying_it_again_writes_nothing() {
        let window = Fake::echo();
        let (remote, old, new) = two_phones(window.clone(), test_port(16)).await;
        let address = push::tests::rfc_subscription("https://push.example/same", push::Lang::En);
        remote.subscribe(&old, address.clone()).unwrap();
        let changes = *window.changes.lock().unwrap();

        // The app opened again: the same subscription, nothing to save.
        remote.subscribe(&old, address.clone()).unwrap();
        assert_eq!(*window.changes.lock().unwrap(), changes);

        // The browser paired again under a new device keeps its address.
        remote.subscribe(&new, address.clone()).unwrap();
        let pushes: Vec<bool> = remote
            .store
            .read()
            .devices
            .iter()
            .map(|phone| phone.push.is_some())
            .collect();
        assert_eq!(pushes, [false, true]);
        assert_eq!(remote.push_subscribers().await, 1);

        // A language changed is saved.
        let french = Subscription {
            lang: push::Lang::Fr,
            ..address
        };
        remote.subscribe(&new, french).unwrap();
        assert_eq!(
            remote.store.read().devices[1].push.as_ref().unwrap().lang,
            push::Lang::Fr
        );
        remote.unsubscribe(&new).unwrap();
        assert_eq!(remote.push_subscribers().await, 0);
        remote.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn nothing_is_pushed_while_hangar_mobile_is_off() {
        let (service, received) = push_service(test_port(18)).await;
        let (remote, phone, _) = two_phones(Fake::echo(), test_port(17)).await;
        let subscription =
            push::tests::rfc_subscription(&format!("{service}/push/a"), push::Lang::En);
        remote.subscribe(&phone, subscription).unwrap();
        remote.set_enabled(false).await.unwrap();

        assert_eq!(remote.push_subscribers().await, 0);
        let outcome = remote.push(waits("a", "b")).await.unwrap();
        assert_eq!(outcome, PushOutcome::default());
        assert!(received.lock().unwrap().is_empty());
        // Kept for when it is on again.
        assert!(remote.store.read().devices[0].push.is_some());
    }
}
