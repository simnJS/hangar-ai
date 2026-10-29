//! The apps the agents build, seen from the phone: an agent's dev server —
//! Vite, Next and the like — opened on the phone, hot reload included.
//!
//! A dev server expects the root of an origin of its own (`/@vite/client`,
//! `/_next/…`), which no path under the mobile server's can give it. So an
//! app shown on the phone gets a port of the PC's tailnet address: six
//! slots, each a listener on 127.0.0.1 (proxy.rs) that Tailscale Serve
//! publishes on an HTTPS port of its own, 8443 to 8448. The phone's cookie
//! is the host's, not the port's, and SameSite counts the ports as one site:
//! it reaches the slots too, and is what opens them.
//!
//! A slot shows one app, set only by the phone's `/m/api/app/open` for a dev
//! server the window lists (`remote_apps`) and that is listening: never an
//! open proxy. A slot nothing went through for 30 minutes is freed.

use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::watch;

use super::tailscale::{self, Mapping};
use super::{now_ms, proxy, store, Remote};
use crate::control::ControlError;

/// How many apps the phone can have open at once.
const SLOTS: u16 = 6;
/// The first slot's HTTPS port on the tailnet; the others follow it.
const FIRST_HTTPS: u16 = 8443;
/// A slot nothing went through for this long — no request, no byte of a
/// socket — is freed.
pub const IDLE_MS: u64 = 30 * 60 * 1000;
/// How often a slot's listener looks for slots gone idle.
pub const SWEEP_EVERY: Duration = Duration::from_secs(60);
/// A server on the loopback takes a connection at once. This only bounds a
/// port nobody listens on, which Windows takes two seconds to refuse.
const REACH_TIMEOUT: Duration = Duration::from_millis(500);
/// More ports than the window ever asks about at once.
const MAX_PORTS: usize = 256;

type Pending<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What the slots need of Tailscale. The tests play it: no test ever runs
/// `tailscale serve` on the machine running it.
pub trait Tailnet: Send + Sync {
    /// The PC's name on the tailnet and what Tailscale Serve publishes
    /// (`serve status --json`), or why Tailscale cannot say.
    fn config(&self) -> Pending<'_, Result<(String, Value), String>>;
    /// `tailscale serve --bg --https=<https> http://127.0.0.1:<local>`.
    fn publish(&self, https: u16, local: u16) -> Pending<'_, Result<(), String>>;
}

/// The Tailscale CLI.
pub struct Cli;

impl Tailnet for Cli {
    fn config(&self) -> Pending<'_, Result<(String, Value), String>> {
        Box::pin(tailscale::serve_config())
    }

    fn publish(&self, https: u16, local: u16) -> Pending<'_, Result<(), String>> {
        Box::pin(tailscale::publish(https, local))
    }
}

/// A slot's two ports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotPorts {
    /// On the tailnet: `https://<pc>.<tailnet>.ts.net:<https>/`.
    pub https: u16,
    /// Its listener on 127.0.0.1, which Tailscale Serve proxies to.
    pub local: u16,
}

/// 8443 to 8448, proxied to 47822 to 47827 beside the mobile server's 47821.
/// Fixed: Tailscale Serve is told them once and keeps them.
pub fn default_ports() -> Vec<SlotPorts> {
    (0..SLOTS)
        .map(|i| SlotPorts {
            https: FIRST_HTTPS + i,
            local: store::DEFAULT_PORT + 1 + i,
        })
        .collect()
}

/// An app the window allows, as `remote_apps` lists it — with its source,
/// pane and task, which are the phone's to show.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub workspace_id: String,
    #[serde(default)]
    pub workspace: String,
    pub port: u16,
}

/// What `/m/api/app/open` answers: where the phone opens the app.
#[derive(Debug, PartialEq, Serialize)]
pub struct Opened {
    pub url: String,
    pub slot: usize,
}

#[derive(Debug, PartialEq)]
pub enum OpenError {
    /// Not an app the window lists, or one of Hangar's own ports.
    NotAllowed,
    /// Allowed, but no server listens on it.
    NotListening,
    /// The window said no, and why.
    Refused(String),
    /// The window did not answer in time.
    Busy,
    /// Tailscale is missing or signed out, or would not serve the slot.
    Tailscale(String),
    /// Every slot's port is in use by something else than Hangar.
    NoSlot,
}

/// A slot as the settings list it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotView {
    pub slot: usize,
    pub https_port: u16,
    pub local_port: u16,
    /// What Tailscale Serve does with `httpsPort`: `mapped` to the slot,
    /// `free`, or `taken` by something of the person's. Null when Tailscale
    /// could not be asked.
    pub mapping: Option<Mapping>,
    /// The app it shows, while it does.
    pub app: Option<ShownApp>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShownApp {
    pub workspace_id: String,
    pub workspace: String,
    pub port: u16,
    /// Milliseconds since the epoch: the slot is freed 30 minutes after.
    pub last_used: u64,
}

/// What a slot's listener relays a request with.
pub struct Route {
    /// The dev server, on 127.0.0.1 or ::1.
    pub target: SocketAddr,
    /// Brought up to now by each request and each byte of a socket.
    pub last_used: Arc<AtomicU64>,
    /// Closed once the slot is freed or shows another app: the sockets it
    /// carries end with it.
    pub closing: watch::Receiver<()>,
}

struct Slot {
    ports: SlotPorts,
    /// Started the first time the slot is used, until Hangar Mobile stops.
    listener: Option<proxy::Running>,
    showing: Option<Showing>,
}

struct Showing {
    app: App,
    target: SocketAddr,
    last_used: Arc<AtomicU64>,
    /// Dropped with it, which closes its routes' `closing`.
    open: watch::Sender<()>,
}

impl Showing {
    fn idle(&self, now: u64) -> bool {
        now.saturating_sub(self.last_used.load(Ordering::Relaxed)) >= IDLE_MS
    }
}

pub struct Apps {
    tailnet: Box<dyn Tailnet>,
    slots: Mutex<Vec<Slot>>,
    /// One opening at a time: two taps never race for a slot, nor run
    /// `tailscale serve` twice.
    opening: tokio::sync::Mutex<()>,
}

impl Apps {
    pub fn new(tailnet: Box<dyn Tailnet>, ports: Vec<SlotPorts>) -> Self {
        let slots = ports
            .into_iter()
            .map(|ports| Slot {
                ports,
                listener: None,
                showing: None,
            })
            .collect();
        Self {
            tailnet,
            slots: Mutex::new(slots),
            opening: Default::default(),
        }
    }

    /// Where a slot's listener relays to, its last use brought up to `now`;
    /// none for a slot that shows nothing, or went 30 minutes unused.
    pub fn route(&self, slot: usize, now: u64) -> Option<Route> {
        let mut slots = self.slots.lock().unwrap();
        let slot = slots.get_mut(slot)?;
        if slot
            .showing
            .as_ref()
            .is_some_and(|showing| showing.idle(now))
        {
            slot.showing = None;
        }
        let showing = slot.showing.as_ref()?;
        showing.last_used.fetch_max(now, Ordering::Relaxed);
        Some(Route {
            target: showing.target,
            last_used: showing.last_used.clone(),
            closing: showing.open.subscribe(),
        })
    }

    /// Frees the slots that went 30 minutes unused.
    pub fn sweep(&self, now: u64) {
        for slot in self.slots.lock().unwrap().iter_mut() {
            if slot
                .showing
                .as_ref()
                .is_some_and(|showing| showing.idle(now))
            {
                slot.showing = None;
            }
        }
    }

    /// Hangar Mobile stopped: every slot freed, every listener closed.
    pub async fn close(&self) {
        let _one = self.opening.lock().await;
        for slot in self.slots.lock().unwrap().iter_mut() {
            slot.showing = None;
            if let Some(listener) = slot.listener.take() {
                listener.stop();
            }
        }
    }

    /// For the settings: each slot, the app it shows, and what Tailscale
    /// Serve does with its port.
    pub async fn slots(&self) -> Vec<SlotView> {
        let config = self.tailnet.config().await.ok();
        self.sweep(now_ms());
        let slots = self.slots.lock().unwrap();
        slots
            .iter()
            .enumerate()
            .map(|(index, slot)| SlotView {
                slot: index,
                https_port: slot.ports.https,
                local_port: slot.ports.local,
                mapping: config.as_ref().map(|(dns_name, config)| {
                    tailscale::mapping(config, dns_name, slot.ports.https, slot.ports.local)
                }),
                app: slot.showing.as_ref().map(|showing| ShownApp {
                    workspace_id: showing.app.workspace_id.clone(),
                    workspace: showing.app.workspace.clone(),
                    port: showing.app.port,
                    last_used: showing.last_used.load(Ordering::Relaxed),
                }),
            })
            .collect()
    }

    fn is_slot_port(&self, port: u16) -> bool {
        self.slots
            .lock()
            .unwrap()
            .iter()
            .any(|slot| slot.ports.local == port)
    }

    /// The slots to try for an app on `port`, best first: the one showing it
    /// already, then the free ones, then the others, least recently used
    /// first. Those gone idle are freed first.
    fn candidates(&self, port: u16, now: u64) -> Vec<(usize, SlotPorts)> {
        self.sweep(now);
        let slots = self.slots.lock().unwrap();
        let mut order: Vec<usize> = (0..slots.len()).collect();
        // Stable: free slots keep their order.
        order.sort_by_key(|&index| match &slots[index].showing {
            Some(showing) if showing.app.port == port => (0, 0),
            None => (1, 0),
            Some(showing) => (2, showing.last_used.load(Ordering::Relaxed)),
        });
        order
            .into_iter()
            .map(|index| (index, slots[index].ports))
            .collect()
    }

    /// The slot's ports once its listener runs, started if it did not yet;
    /// none when its port is in use by another program.
    fn listen(&self, remote: &Arc<Remote>, index: usize) -> Option<SlotPorts> {
        let mut slots = self.slots.lock().unwrap();
        let slot = &mut slots[index];
        if slot.listener.is_none() {
            match proxy::listen(remote.clone(), index, slot.ports.local) {
                Ok(running) => {
                    // The same port, but where the tests ask for any (0).
                    slot.ports.local = running.port();
                    slot.listener = Some(running);
                }
                Err(err) => {
                    eprintln!(
                        "Hangar Mobile could not listen on 127.0.0.1:{} for an app: {err}",
                        slot.ports.local
                    );
                    return None;
                }
            }
        }
        Some(slot.ports)
    }

    /// The slot shows `app` from now on. The app it showed already stays as
    /// it was, its sockets open; any other's close.
    fn show(&self, index: usize, app: App, target: SocketAddr, now: u64) {
        let mut slots = self.slots.lock().unwrap();
        match &mut slots[index].showing {
            Some(showing) if showing.app.port == app.port && showing.target == target => {
                showing.app = app;
                showing.last_used.fetch_max(now, Ordering::Relaxed);
            }
            showing => {
                *showing = Some(Showing {
                    app,
                    target,
                    last_used: Arc::new(AtomicU64::new(now)),
                    open: watch::channel(()).0,
                })
            }
        }
    }
}

/// The phone's tap on an app: the window's word that it is allowed, its
/// server listening, a slot for it — the one showing it already, if any —
/// and the slot's port published on the tailnet, once.
pub async fn open(
    remote: &Arc<Remote>,
    workspace_id: &str,
    port: u16,
) -> Result<Opened, OpenError> {
    let apps = &remote.apps;
    // Never Hangar's own listeners, whatever the window lists.
    if port == remote.store.read().port || apps.is_slot_port(port) {
        return Err(OpenError::NotAllowed);
    }
    let listed = remote
        .window
        .ask("remote_apps", json!({}), remote.call_timeout)
        .await
        .map_err(|err| match err {
            ControlError::Refused(message) => OpenError::Refused(message),
            ControlError::Unavailable(_) => OpenError::Busy,
        })?;
    let app = allowed(&listed, workspace_id, port).ok_or(OpenError::NotAllowed)?;
    let target = reach(port).await.ok_or(OpenError::NotListening)?;

    let _one = apps.opening.lock().await;
    let (dns_name, config) = apps.tailnet.config().await.map_err(OpenError::Tailscale)?;
    let mapping =
        |ports: SlotPorts| tailscale::mapping(&config, &dns_name, ports.https, ports.local);
    for (index, ports) in apps.candidates(port, now_ms()) {
        if mapping(ports) == Mapping::Taken {
            continue;
        }
        let Some(ports) = apps.listen(remote, index) else {
            continue;
        };
        match mapping(ports) {
            Mapping::Mapped => {}
            Mapping::Free => apps
                .tailnet
                .publish(ports.https, ports.local)
                .await
                .map_err(OpenError::Tailscale)?,
            Mapping::Taken => continue,
        }
        apps.show(index, app, target, now_ms());
        return Ok(Opened {
            url: format!("https://{dns_name}:{}/", ports.https),
            slot: index,
        });
    }
    Err(OpenError::NoSlot)
}

/// The app `remote_apps` lists for that workspace and port, if it does.
fn allowed(listed: &Value, workspace_id: &str, port: u16) -> Option<App> {
    listed["apps"]
        .as_array()?
        .iter()
        .filter_map(|app| App::deserialize(app).ok())
        .find(|app| app.workspace_id == workspace_id && app.port == port)
}

/// Where on the loopback a server takes connections on `port`: 127.0.0.1,
/// or else ::1 — where a dev server told `localhost` listens alone when the
/// system resolves that name to it first, as Node does on Windows.
pub async fn reach(port: u16) -> Option<SocketAddr> {
    let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
    match tokio::join!(accepts(v4), accepts(v6)) {
        (true, _) => Some(v4),
        (false, true) => Some(v6),
        (false, false) => None,
    }
}

async fn accepts(addr: SocketAddr) -> bool {
    matches!(
        tokio::time::timeout(REACH_TIMEOUT, TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

/// Those of `ports` a server listens on, on this PC's loopback, in the
/// order asked and each once.
pub async fn listening(ports: &[u16]) -> Vec<u16> {
    let mut asked: Vec<u16> = Vec::new();
    for &port in ports {
        if port != 0 && !asked.contains(&port) && asked.len() < MAX_PORTS {
            asked.push(port);
        }
    }
    let mut probes = tokio::task::JoinSet::new();
    for (order, port) in asked.into_iter().enumerate() {
        probes.spawn(async move { (order, port, reach(port).await.is_some()) });
    }
    let mut found = Vec::new();
    while let Some(probe) = probes.join_next().await {
        if let Ok((order, port, true)) = probe {
            found.push((order, port));
        }
    }
    found.sort_unstable();
    found.into_iter().map(|(_, port)| port).collect()
}

#[cfg(test)]
pub(super) mod tests {
    use std::convert::Infallible;
    use std::sync::PoisonError;
    use std::task::{Context, Poll};

    use axum::body::{to_bytes, Body, Bytes};
    use axum::extract::{Request, State};
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::response::Response;
    use hyper::body::Frame;
    use hyper_util::rt::TokioIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::mpsc;

    use super::super::assets::tests::no_app;
    use super::super::tests::{remote_on, test_port, Fake};
    use super::*;

    pub const PC: &str = "atelier.tail1234.ts.net";

    /// Tailscale as the tests play it: connected as `atelier` or not, and
    /// what `tailscale serve` was told, kept in a configuration that
    /// `serve status` reads back.
    #[derive(Clone)]
    pub struct FakeTailnet {
        connected: bool,
        refusal: Option<String>,
        pub config: Arc<Mutex<Value>>,
        published: Arc<Mutex<Vec<(u16, u16)>>>,
    }

    impl FakeTailnet {
        pub fn offline() -> Self {
            Self {
                connected: false,
                refusal: None,
                config: Arc::new(Mutex::new(json!({}))),
                published: Default::default(),
            }
        }

        pub fn connected() -> Self {
            Self {
                connected: true,
                ..Self::offline()
            }
        }

        /// Connected, on a tailnet that has not allowed Serve.
        pub fn refusing(why: &str) -> Self {
            Self {
                refusal: Some(why.to_string()),
                ..Self::connected()
            }
        }

        pub fn published(&self) -> Vec<(u16, u16)> {
            self.published.lock().unwrap().clone()
        }
    }

    impl Tailnet for FakeTailnet {
        fn config(&self) -> Pending<'_, Result<(String, Value), String>> {
            let answer = if self.connected {
                Ok((PC.to_string(), self.config.lock().unwrap().clone()))
            } else {
                Err(tailscale::NOT_CONNECTED.to_string())
            };
            Box::pin(async move { answer })
        }

        fn publish(&self, https: u16, local: u16) -> Pending<'_, Result<(), String>> {
            let answer = match &self.refusal {
                Some(why) => Err(why.clone()),
                None => {
                    self.published.lock().unwrap().push((https, local));
                    let mut config = self.config.lock().unwrap();
                    config["TCP"][https.to_string()] = json!({ "HTTPS": true });
                    config["Web"][format!("{PC}:{https}")] = json!({
                        "Handlers": { "/": { "Proxy": format!("http://127.0.0.1:{local}") } }
                    });
                    Ok(())
                }
            };
            Box::pin(async move { answer })
        }
    }

    /// The share's ports 9 and 19 — a dev server, a slot's listener — are
    /// these tests' alone: one test at a time, its runtime and every
    /// listener on it gone before the next one starts.
    pub fn on_ports<F: Future>(test: impl FnOnce() -> F) -> F::Output {
        static PORTS: Mutex<()> = Mutex::new(());
        let _one = PORTS.lock().unwrap_or_else(PoisonError::into_inner);
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(test())
    }

    /// The window as it answers `remote_apps`: these ports of the workspace
    /// `w1`, called Shop.
    pub fn window_listing(ports: &[u16]) -> Fake {
        let apps: Vec<Value> = ports
            .iter()
            .map(|port| {
                json!({
                    "workspaceId": "w1",
                    "workspace": "Shop",
                    "port": port,
                    "source": "worker",
                    "pane": "Eli",
                })
            })
            .collect();
        Fake::answering(move |request| {
            Some(if request.op == "remote_apps" {
                Ok(json!({ "apps": apps }))
            } else {
                Err(format!("unexpected {}", request.op))
            })
        })
    }

    /// What the stand-in dev server received.
    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: String,
        pub uri: String,
        pub headers: HeaderMap,
        pub body: Vec<u8>,
    }

    #[derive(Clone, Default)]
    pub struct DevServer {
        pub seen: Arc<Mutex<Vec<Seen>>>,
        /// What `/stream` is sending, a chunk at a time.
        pub trickle: Arc<Mutex<Option<mpsc::Sender<&'static str>>>>,
    }

    /// A body sent as the test hands it over.
    struct Trickle(mpsc::Receiver<&'static str>);

    impl hyper::body::Body for Trickle {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
            self.0.poll_recv(cx).map(|chunk| {
                chunk.map(|chunk| Ok(Frame::data(Bytes::from_static(chunk.as_bytes()))))
            })
        }
    }

    /// A dev server on 127.0.0.1:`port`. It answers a request with its own
    /// body and headers of its own — two cookies, one by the device's name
    /// — `/stream` a chunk at a time as the test hands them over, and
    /// `/hmr` as a WebSocket that echoes what it is sent.
    pub async fn dev_server(port: u16) -> (u16, DevServer) {
        let dev = DevServer::default();
        let app = axum::Router::new().fallback(answer).with_state(dev.clone());
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (port, dev)
    }

    async fn answer(State(dev): State<DevServer>, mut request: Request) -> Response {
        let upgrade = (request.uri().path() == "/hmr").then(|| hyper::upgrade::on(&mut request));
        let (parts, body) = request.into_parts();
        let body = to_bytes(body, usize::MAX)
            .await
            .unwrap_or_default()
            .to_vec();
        dev.seen.lock().unwrap().push(Seen {
            method: parts.method.to_string(),
            uri: parts.uri.to_string(),
            headers: parts.headers.clone(),
            body: body.clone(),
        });
        if let Some(upgrade) = upgrade {
            tokio::spawn(async move {
                let Ok(upgraded) = upgrade.await else {
                    return;
                };
                let mut socket = TokioIo::new(upgraded);
                let mut buffer = [0u8; 256];
                while let Ok(read) = socket.read(&mut buffer).await {
                    if read == 0 || socket.write_all(&buffer[..read]).await.is_err() {
                        break;
                    }
                }
            });
            return Response::builder()
                .status(StatusCode::SWITCHING_PROTOCOLS)
                .header(header::CONNECTION, "upgrade")
                .header(header::UPGRADE, "websocket")
                .header("sec-websocket-accept", "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
                .body(Body::empty())
                .unwrap();
        }
        if parts.uri.path() == "/stream" {
            let (send, receive) = mpsc::channel(4);
            *dev.trickle.lock().unwrap() = Some(send);
            return Response::new(Body::new(Trickle(receive)));
        }
        Response::builder()
            .header(header::CONTENT_TYPE, "text/plain")
            .header(header::SET_COOKIE, "theme=dark; Path=/")
            .header(
                header::SET_COOKIE,
                "hangar_device=planted; Path=/; HttpOnly",
            )
            .header(header::CONTENT_SECURITY_POLICY, "default-src 'self'")
            .header(header::X_FRAME_OPTIONS, "SAMEORIGIN")
            .body(Body::from(body))
            .unwrap()
    }

    /// A phone paired the way the app pairs one, and its cookie's token.
    pub fn pair(remote: &Remote) -> String {
        let code = remote.begin_pairing(PC).unwrap().code;
        remote.pair(&code, "Pixel").unwrap().1
    }

    /// Slot 0 shows the app of the workspace `w1` on `port`, as if opened.
    pub fn showing(apps: &Apps, port: u16) {
        apps.show(0, app(port), at(port), now_ms());
    }

    fn app(port: u16) -> App {
        App {
            workspace_id: "w1".into(),
            workspace: "Shop".into(),
            port,
        }
    }

    fn at(port: u16) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    }

    /// Three slots, none of them ever listening: what is shown where.
    fn three_slots() -> Apps {
        let ports = (0..3)
            .map(|i| SlotPorts {
                https: 8443 + i,
                local: 47822 + i,
            })
            .collect();
        Apps::new(Box::new(FakeTailnet::connected()), ports)
    }

    fn first(apps: &Apps, port: u16, now: u64) -> usize {
        apps.candidates(port, now)[0].0
    }

    #[test]
    fn an_app_keeps_its_slot_and_another_takes_a_free_one() {
        let apps = three_slots();
        assert_eq!(first(&apps, 5173, 0), 0);
        apps.show(0, app(5173), at(5173), 0);
        assert_eq!(first(&apps, 5173, 1), 0);
        assert_eq!(first(&apps, 3000, 1), 1);
        apps.show(1, app(3000), at(3000), 1);
        assert_eq!(first(&apps, 3000, 2), 1);
        assert_eq!(first(&apps, 5173, 2), 0);
        assert_eq!(first(&apps, 8080, 2), 2);
    }

    #[test]
    fn a_slot_unused_for_30_minutes_is_freed() {
        let apps = three_slots();
        let now = 1_000_000;
        apps.show(0, app(5173), at(5173), now);
        apps.sweep(now + IDLE_MS - 1);
        assert!(apps.route(0, now + IDLE_MS - 1).is_some());
        // That request was a use: 30 minutes from it, not from the opening.
        apps.sweep(now + IDLE_MS + 1);
        assert!(apps.route(0, now + IDLE_MS + 1).is_some());
        let used = now + IDLE_MS + 1;
        apps.sweep(used + IDLE_MS);
        assert!(apps.route(0, used + IDLE_MS).is_none());
        // A route asked of a slot gone idle frees it too, sweep or not.
        apps.show(1, app(3000), at(3000), now);
        assert!(apps.route(1, now + IDLE_MS).is_none());
        assert_eq!(first(&apps, 8080, now + IDLE_MS), 0);
    }

    #[test]
    fn with_every_slot_in_use_the_least_recently_used_goes() {
        let apps = three_slots();
        apps.show(0, app(5173), at(5173), 3_000);
        apps.show(1, app(3000), at(3000), 1_000);
        apps.show(2, app(8080), at(8080), 2_000);
        let order: Vec<usize> = apps
            .candidates(4200, 4_000)
            .into_iter()
            .map(|(index, _)| index)
            .collect();
        assert_eq!(order, [1, 2, 0]);
        // Used since, it is no longer the first to go.
        apps.route(1, 5_000).unwrap();
        assert_eq!(first(&apps, 4200, 5_000), 2);
    }

    #[test]
    fn a_slot_that_shows_another_app_closes_the_sockets_of_the_last() {
        let apps = three_slots();
        apps.show(0, app(5173), at(5173), 0);
        let kept = apps.route(0, 1).unwrap().closing;
        // The same app again: its sockets stay open.
        apps.show(0, app(5173), at(5173), 2);
        assert!(matches!(kept.has_changed(), Ok(false)));
        apps.show(0, app(3000), at(3000), 3);
        assert!(kept.has_changed().is_err());
        assert_eq!(apps.route(0, 4).unwrap().target, at(3000));

        let freed = apps.route(0, 5).unwrap().closing;
        apps.sweep(5 + IDLE_MS);
        assert!(freed.has_changed().is_err());
    }

    #[test]
    fn the_ports_listening_are_those_that_take_a_connection() {
        on_ports(|| async {
            let server = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, test_port(9)))
                .await
                .unwrap();
            let port = server.local_addr().unwrap().port();
            assert_eq!(listening(&[port, 0, port]).await, [port]);
            assert_eq!(reach(port).await, Some(at(port)));
            drop(server);
            assert!(listening(&[port]).await.is_empty());
            assert_eq!(reach(port).await, None);
        });
    }

    #[test]
    fn a_server_on_the_ipv6_loopback_alone_is_found_there() {
        on_ports(|| async {
            let Ok(server) =
                tokio::net::TcpListener::bind((Ipv6Addr::LOCALHOST, test_port(9))).await
            else {
                eprintln!("no IPv6 loopback here: nothing to check");
                return;
            };
            let port = server.local_addr().unwrap().port();
            assert_eq!(
                reach(port).await,
                Some(SocketAddr::from((Ipv6Addr::LOCALHOST, port)))
            );
            assert_eq!(listening(&[port]).await, [port]);
        });
    }

    #[test]
    fn an_app_opens_on_a_slot_published_once_and_kept_while_used() {
        on_ports(|| async {
            let (port, _dev) = dev_server(test_port(9)).await;
            let tailnet = FakeTailnet::connected();
            let window = window_listing(&[port]);
            let remote = remote_on(window.clone(), no_app, tailnet.clone());

            let opened = open(&remote, "w1", port).await.unwrap();
            assert_eq!(
                opened,
                Opened {
                    url: format!("https://{PC}:8443/"),
                    slot: 0
                }
            );
            let local = remote.apps.slots().await[0].local_port;
            assert_eq!(tailnet.published(), [(8443, local)]);
            assert_eq!(*window.asked.lock().unwrap(), ["remote_apps"]);

            // Asked again: the same slot, and Tailscale Serve left alone.
            assert_eq!(open(&remote, "w1", port).await.unwrap(), opened);
            assert_eq!(tailnet.published().len(), 1);
            let shown = &remote.apps.slots().await[0];
            assert_eq!(shown.mapping, Some(Mapping::Mapped));
            let app = shown.app.as_ref().unwrap();
            assert_eq!(
                (app.workspace_id.as_str(), app.workspace.as_str()),
                ("w1", "Shop")
            );
            assert_eq!(app.port, port);

            // Half an hour without a request: freed, mapping kept.
            remote.apps.sweep(app.last_used + IDLE_MS);
            let freed = &remote.apps.slots().await[0];
            assert_eq!(freed.app, None);
            assert_eq!(freed.mapping, Some(Mapping::Mapped));
            assert_eq!(open(&remote, "w1", port).await.unwrap(), opened);
            assert_eq!(tailnet.published().len(), 1);

            // Hangar Mobile turned off: the slot closes with it.
            remote.apps.close().await;
            assert_eq!(remote.apps.slots().await[0].app, None);
        });
    }

    #[test]
    fn tailscale_signed_out_or_refusing_serve_says_so() {
        on_ports(|| async {
            let (port, _dev) = dev_server(test_port(9)).await;
            let offline = remote_on(window_listing(&[port]), no_app, FakeTailnet::offline());
            assert_eq!(
                open(&offline, "w1", port).await,
                Err(OpenError::Tailscale(tailscale::NOT_CONNECTED.to_string()))
            );
            assert_eq!(offline.apps.slots().await[0].mapping, None);

            let why = "Tailscale Serve is not enabled on your tailnet yet.";
            let refusing = FakeTailnet::refusing(why);
            let remote = remote_on(window_listing(&[port]), no_app, refusing.clone());
            assert_eq!(
                open(&remote, "w1", port).await,
                Err(OpenError::Tailscale(why.to_string()))
            );
            assert!(refusing.published().is_empty());
            assert_eq!(remote.apps.slots().await[0].app, None);
        });
    }

    #[test]
    fn a_port_tailscale_serves_for_something_else_is_never_taken() {
        on_ports(|| async {
            let (port, _dev) = dev_server(test_port(9)).await;
            let tailnet = FakeTailnet::connected();
            *tailnet.config.lock().unwrap() = json!({
                "TCP": { "8443": { "HTTPS": true } },
                "Web": { format!("{PC}:8443"): {
                    "Handlers": { "/": { "Proxy": "http://127.0.0.1:3000" } } } }
            });
            let remote = remote_on(window_listing(&[port]), no_app, tailnet.clone());
            assert_eq!(open(&remote, "w1", port).await, Err(OpenError::NoSlot));
            assert!(tailnet.published().is_empty());
            assert_eq!(remote.apps.slots().await[0].mapping, Some(Mapping::Taken));
        });
    }

    #[tokio::test]
    async fn hangar_s_own_ports_are_never_an_app() {
        let window = window_listing(&[store::DEFAULT_PORT]);
        let remote = remote_on(window.clone(), no_app, FakeTailnet::connected());
        assert_eq!(
            open(&remote, "w1", store::DEFAULT_PORT).await,
            Err(OpenError::NotAllowed)
        );
        assert!(window.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn only_the_listed_workspace_and_port_are_allowed() {
        let listed = json!({ "apps": [
            { "workspaceId": "w1", "workspace": "Shop", "port": 5173, "source": "worker" },
            { "workspaceId": "w2", "port": 3000, "source": "custom", "task": "t1" },
            { "workspaceId": "w3", "port": "8080" },
            "nonsense",
        ] });
        assert_eq!(allowed(&listed, "w1", 5173), Some(app(5173)));
        assert_eq!(allowed(&listed, "w2", 3000).map(|app| app.port), Some(3000));
        assert_eq!(allowed(&listed, "w1", 3000), None);
        assert_eq!(allowed(&listed, "w2", 5173), None);
        assert_eq!(allowed(&listed, "w3", 8080), None);
        assert_eq!(allowed(&json!({}), "w1", 5173), None);
    }
}
