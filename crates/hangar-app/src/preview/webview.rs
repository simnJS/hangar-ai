//! What only a browser engine can show (a PDF, an HTML page, a video, a
//! sound) goes to `hangar-webview`, the helper process, in a window of its
//! own: the main process links no browser engine (docs/native/PLAN.md, 4.5).
//!
//! The helper opens `hangar-model://localhost/<path>` and every request the
//! page makes to that scheme comes back here, where hangar-core's
//! `model::serve` answers it from the folders the preview may read: the same
//! rules, the same `Range` handling, the same CSP as the web view of the old
//! app had. The helper holds no rule of its own about what a page may load.
//!
//! One helper serves the whole app. The kit tabs (P16) share it: windows named
//! `preview-…` are this package's, and whoever registers a prefix with
//! `Helper::route` gets the events of the windows that start with it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hangar_webview::{
    Client, Closed, Content, Event, OpenWindow, SchemeRequest, SchemeResponse, SpawnOptions,
    scheme_base,
};
use serde_json::Value;

use super::model::{Kind, preview_url};
use super::store::Entry;
use crate::message::Message;
use crate::services::inbox::Inbox;

/// The scheme the helper's windows load files by (hangar-core's
/// `model::SCHEME`).
pub const SCHEME: &str = hangar_core::model::SCHEME;

/// What this package's windows are called, so that their events find their
/// way back here.
pub const PREFIX: &str = "preview-";

/// Where the helper is, or why it is not.
#[derive(Debug, Clone)]
pub enum StartError {
    /// Not next to the app, and not a development build that could run it
    /// through cargo.
    Missing,
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Missing => f.write_str("hangar-webview was not found next to the app"),
            StartError::Failed(why) => f.write_str(why),
        }
    }
}

type Route = Box<dyn Fn(&Event) + Send + Sync>;

/// The helper process and the way to it, shared by everything that shows a
/// page: started on the first window, and again after it was lost.
#[derive(Default)]
pub struct Helper {
    client: Mutex<Option<Arc<Client>>>,
    routes: Mutex<Vec<(String, Route)>>,
}

impl std::fmt::Debug for Helper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Helper")
            .field("running", &self.client().is_some())
            .finish()
    }
}

/// How to start the helper: next to the app in a package, `HANGAR_WEBVIEW`
/// when set, and in a development build through cargo
/// (`cargo run -p hangar-webview --features engine`), which builds it when it
/// is not built yet.
pub fn spawn_options() -> Result<SpawnOptions, StartError> {
    if let Some(program) = hangar_webview::find_helper() {
        return Ok(SpawnOptions::new(program));
    }
    if cfg!(debug_assertions) {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("Cargo.toml");
        if manifest.is_file() {
            return Ok(SpawnOptions::new("cargo")
                .arg("run")
                .arg("--quiet")
                .arg("--manifest-path")
                .arg(manifest)
                .arg("-p")
                .arg("hangar-webview")
                .arg("--features")
                .arg("engine")
                .arg("--"));
        }
    }
    Err(StartError::Missing)
}

impl Helper {
    pub fn new() -> Arc<Helper> {
        Arc::new(Helper::default())
    }

    fn lock_client(&self) -> std::sync::MutexGuard<'_, Option<Arc<Client>>> {
        self.client
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// The running helper, if it is.
    pub fn client(&self) -> Option<Arc<Client>> {
        self.lock_client()
            .as_ref()
            .filter(|client| !client.is_closed())
            .cloned()
    }

    /// Hears the events of the windows whose name starts with `prefix`
    /// (other than this package's own).
    pub fn route(&self, prefix: &str, handler: impl Fn(&Event) + Send + Sync + 'static) {
        self.routes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push((prefix.to_string(), Box::new(handler)));
    }

    /// Starts the helper if it is not running, and answers its client. It
    /// takes a moment: call it off the UI thread. `inbox` is where this
    /// package's events go.
    pub fn start(self: &Arc<Self>, inbox: Inbox) -> Result<Arc<Client>, StartError> {
        let mut held = self.lock_client();
        if let Some(client) = held.as_ref().filter(|client| !client.is_closed()) {
            return Ok(client.clone());
        }
        let (client, events) = Client::spawn_with(spawn_options()?)
            .map_err(|err| StartError::Failed(err.to_string()))?;
        let client = Arc::new(client);
        *held = Some(client.clone());
        drop(held);
        let helper = self.clone();
        let responder = client.clone();
        std::thread::Builder::new()
            .name("hangar-preview-webview".into())
            .spawn(move || helper.pump(events, responder, inbox))
            .map_err(|err| StartError::Failed(err.to_string()))?;
        Ok(client)
    }

    /// Reads the helper's events until it ends: a request for a file is
    /// answered at once, on a thread of its own so that a big video does not
    /// hold up the next page; the rest goes to its owner.
    fn pump(&self, events: hangar_webview::Events, client: Arc<Client>, inbox: Inbox) {
        while let Some(event) = events.recv() {
            if let Event::SchemeRequest(request) = event {
                let client = client.clone();
                std::thread::spawn(move || {
                    // A helper that is gone has nobody to answer to.
                    let _: Result<(), Closed> = client.respond(answer(&request));
                });
                continue;
            }
            let ours = match event_window(&event) {
                // The helper's own (`Ready`, a failure of no window) and the
                // windows of this package.
                None => true,
                Some(window) => window.starts_with(PREFIX),
            };
            if ours {
                inbox.send(Message::Preview(super::Msg::Web(WebMsg::Event(event))));
                continue;
            }
            let routes = self
                .routes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let Some(window) = event_window(&event) {
                for (prefix, handler) in routes.iter() {
                    if window.starts_with(prefix.as_str()) {
                        handler(&event);
                    }
                }
            }
        }
        // The helper is gone: what it showed is not there any more.
        let mut held = self.lock_client();
        if held.as_ref().is_some_and(|held| Arc::ptr_eq(held, &client)) {
            *held = None;
        }
        drop(held);
        inbox.send(Message::Preview(super::Msg::Web(WebMsg::Gone)));
    }
}

/// The window an event is about, if it is about one.
pub fn event_window(event: &Event) -> Option<&str> {
    match event {
        Event::Opened { window }
        | Event::Ipc { window, .. }
        | Event::PageLoad { window, .. }
        | Event::TitleChanged { window, .. }
        | Event::EvalResult { window, .. }
        | Event::NavigationBlocked { window, .. }
        | Event::WindowClosed { window, .. } => Some(window),
        Event::SchemeRequest(request) => Some(&request.window),
        Event::Error { window, .. } => window.as_deref(),
        _ => None,
    }
}

/// Answers a page's request for a file: hangar-core serves it, from the
/// folders the preview may read. Pages of the helper have no Hangar origin,
/// so none may `fetch` a file (an `Origin` they carry is refused).
pub fn answer(request: &SchemeRequest) -> SchemeResponse {
    let mut builder = http::Request::builder()
        .method(request.method.as_str())
        .uri(request.uri.as_str());
    for header in &request.headers {
        builder = builder.header(header.name.as_str(), header.value.as_str());
    }
    let Ok(served) = builder.body(request.body.clone()) else {
        return SchemeResponse::new(request.request, 400)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(&b"the request is not one"[..]);
    };
    let response = hangar_core::model::serve(&served, &[]);
    let mut out = SchemeResponse::new(request.request, response.status().as_u16());
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            out = out.header(name.as_str(), value);
        }
    }
    out.body(response.into_body())
}

/* ── the windows ───────────────────────────────────────────────── */

/// What this package's part of the helper's events says to `update`.
#[derive(Debug, Clone)]
pub enum WebMsg {
    /// The helper started, or could not.
    Started(Result<HelperHandle, StartError>),
    Event(Event),
    /// The helper's output ended.
    Gone,
}

/// A client, to ride in a message.
#[derive(Clone)]
pub struct HelperHandle(pub Arc<Client>);

impl std::fmt::Debug for HelperHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelperHandle(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinState {
    /// Waiting for the helper to start.
    Waiting,
    /// Asked to open.
    Opening,
    Open,
    /// The person closed it.
    Closed,
    /// The helper is not installed.
    Missing,
    Failed(String),
    /// The engine has no PDF viewer.
    NoPdfViewer,
}

/// One entry's window.
#[derive(Debug, Clone)]
pub struct Win {
    pub name: String,
    pub state: WinState,
    /// The revision the window shows.
    pub revision: u64,
    pub kind: Kind,
    /// The player's loop and speed, applied again after each load.
    pub looping: bool,
    pub speed: f32,
}

/// The windows of the entries, by entry id.
#[derive(Debug, Default)]
pub struct Windows {
    wins: HashMap<String, Win>,
    /// Entries a window was opened for on their own: a closed one stays
    /// closed until the person asks for it again.
    tried: HashSet<String>,
}

/// The size a kind of page is first given, in logical pixels.
fn size_of(kind: Kind) -> (u32, u32) {
    match kind {
        Kind::Audio => (480, 220),
        Kind::Video => (960, 600),
        Kind::Pdf => (900, 1000),
        _ => (1000, 720),
    }
}

/// The colour shown before the page paints: white for a page, which expects
/// it as a browser gives it; black for a player; the theme's for the rest.
fn background_of(kind: Kind, theme: &hangar_theme::Theme) -> [u8; 3] {
    match kind {
        Kind::Html => [255, 255, 255],
        Kind::Video | Kind::Audio => [0, 0, 0],
        _ => {
            let c = theme.colors.bg;
            [
                (c.r * 255.0).round() as u8,
                (c.g * 255.0).round() as u8,
                (c.b * 255.0).round() as u8,
            ]
        }
    }
}

/// The window to open for an entry.
pub fn open_command(
    name: &str,
    entry: &Entry,
    kind: Kind,
    theme: &hangar_theme::Theme,
) -> OpenWindow {
    let (width, height) = size_of(kind);
    let [red, green, blue] = background_of(kind, theme);
    OpenWindow::new(
        name,
        Content::url(preview_url(&entry.path, entry.revision, false)),
    )
    .title(entry.title.clone())
    .size(width, height)
    .scheme(SCHEME)
    .allow_navigation([scheme_base(SCHEME)])
    .background(red, green, blue)
}

/// What a player is told to do: its loop and its speed. Applied to the first
/// media element of the page.
pub fn player_script(looping: bool, speed: f32) -> String {
    format!(
        "(()=>{{const m=document.querySelector('video,audio');if(m){{m.loop={looping};\
         m.defaultPlaybackRate={speed};m.playbackRate={speed};}}}})()"
    )
}

const PDF_CHECK: &str = "navigator.pdfViewerEnabled";

impl Windows {
    pub fn get(&self, entry: &str) -> Option<&Win> {
        self.wins.get(entry)
    }

    pub fn state(&self, entry: &str) -> Option<&WinState> {
        self.wins.get(entry).map(|win| &win.state)
    }

    /// Whether this entry has had a window opened for it by itself.
    pub fn was_tried(&self, entry: &str) -> bool {
        self.tried.contains(entry)
    }

    pub fn mark_tried(&mut self, entry: &str) {
        self.tried.insert(entry.to_string());
    }

    /// The entry a window belongs to.
    pub fn entry_of(&self, window: &str) -> Option<&str> {
        self.wins
            .iter()
            .find(|(_, win)| win.name == window)
            .map(|(entry, _)| entry.as_str())
    }

    /// Opens (or opens again) the window of an entry: sent at once when the
    /// helper runs, kept waiting when it is starting.
    pub fn open(
        &mut self,
        client: Option<&Client>,
        entry: &Entry,
        kind: Kind,
        theme: &hangar_theme::Theme,
    ) {
        let name = format!("{PREFIX}{}", entry.id);
        let previous = self.wins.get(&entry.id).cloned();
        let win = Win {
            name: name.clone(),
            state: WinState::Waiting,
            revision: entry.revision,
            kind,
            looping: previous.as_ref().is_some_and(|win| win.looping),
            speed: previous.as_ref().map_or(1.0, |win| win.speed),
        };
        self.wins.insert(entry.id.clone(), win);
        if let Some(client) = client {
            self.send_open(client, entry, theme);
        }
    }

    fn send_open(&mut self, client: &Client, entry: &Entry, theme: &hangar_theme::Theme) {
        let Some(win) = self.wins.get_mut(&entry.id) else {
            return;
        };
        let command = open_command(&win.name, entry, win.kind, theme);
        win.state = match client.open(command) {
            Ok(()) => WinState::Opening,
            Err(Closed) => {
                WinState::Failed(StartError::Failed("the helper stopped".into()).to_string())
            }
        };
    }

    /// The helper started: the windows that waited for it open now.
    pub fn flush(
        &mut self,
        client: &Client,
        entries: &HashMap<&str, &Entry>,
        theme: &hangar_theme::Theme,
    ) {
        let waiting: Vec<String> = self
            .wins
            .iter()
            .filter(|(_, win)| win.state == WinState::Waiting)
            .map(|(id, _)| id.clone())
            .collect();
        for id in waiting {
            if let Some(entry) = entries.get(id.as_str()) {
                self.send_open(client, entry, theme);
            }
        }
    }

    /// The helper could not start: the windows that waited say why.
    pub fn fail_waiting(&mut self, error: &StartError) {
        for win in self.wins.values_mut() {
            if win.state == WinState::Waiting {
                win.state = match error {
                    StartError::Missing => WinState::Missing,
                    StartError::Failed(why) => WinState::Failed(why.clone()),
                };
            }
        }
    }

    /// The helper ended: nothing it showed is there.
    pub fn lost(&mut self) {
        for win in self.wins.values_mut() {
            if matches!(win.state, WinState::Opening | WinState::Open) {
                win.state = WinState::Closed;
            }
        }
    }

    /// Brings the windows in line with the entries that are open: a closed
    /// entry's window closes, and a window whose file was rewritten loads the
    /// new revision.
    pub fn reconcile(&mut self, client: Option<&Client>, entries: &HashMap<&str, &Entry>) {
        let gone: Vec<String> = self
            .wins
            .keys()
            .filter(|id| !entries.contains_key(id.as_str()))
            .cloned()
            .collect();
        for id in gone {
            if let Some(win) = self.wins.remove(&id)
                && let Some(client) = client
            {
                let _ = client.close(&win.name);
            }
            self.tried.remove(&id);
        }
        for (id, win) in &mut self.wins {
            let Some(entry) = entries.get(id.as_str()) else {
                continue;
            };
            if win.revision == entry.revision {
                continue;
            }
            win.revision = entry.revision;
            if win.state == WinState::Open
                && let Some(client) = client
            {
                let url = preview_url(&entry.path, entry.revision, false);
                let _ = client.navigate(&win.name, &url);
            }
        }
    }

    /// Reloads a window's page: the same address again.
    pub fn reload(&mut self, client: &Client, entry: &Entry) {
        if let Some(win) = self
            .wins
            .get(&entry.id)
            .filter(|win| win.state == WinState::Open)
        {
            let _ = client.navigate(&win.name, &preview_url(&entry.path, entry.revision, false));
        }
    }

    pub fn focus(&self, client: &Client, entry: &str) {
        if let Some(win) = self
            .wins
            .get(entry)
            .filter(|win| win.state == WinState::Open)
        {
            let _ = client.focus(&win.name);
        }
    }

    /// The player's loop and speed, now and after each load.
    pub fn set_player(
        &mut self,
        client: Option<&Client>,
        entry: &str,
        looping: Option<bool>,
        speed: Option<f32>,
    ) {
        let Some(win) = self.wins.get_mut(entry) else {
            return;
        };
        win.looping = looping.unwrap_or(win.looping);
        win.speed = speed.unwrap_or(win.speed);
        if let Some(client) = client.filter(|_| win.state == WinState::Open) {
            let _ = client.eval(&win.name, &player_script(win.looping, win.speed));
        }
    }

    /// An event of the helper about one of the windows. Answers a link the
    /// window was stopped from following, for the caller to open outside.
    pub fn on_event(&mut self, client: Option<&Client>, event: &Event) -> Option<String> {
        match event {
            Event::Opened { window } => {
                if let Some(win) = self.by_window(window) {
                    win.state = WinState::Open;
                }
            }
            Event::PageLoad {
                window,
                state: hangar_webview::PageLoadState::Finished,
                ..
            } => {
                let win = self.by_window(window)?;
                let (name, kind, looping, speed) =
                    (win.name.clone(), win.kind, win.looping, win.speed);
                if let Some(client) = client {
                    match kind {
                        Kind::Pdf => {
                            let _ =
                                client.eval_with_token(&name, PDF_CHECK, &format!("pdf:{name}"));
                        }
                        Kind::Video | Kind::Audio => {
                            let _ = client.eval(&name, &player_script(looping, speed));
                        }
                        _ => {}
                    }
                }
            }
            Event::EvalResult {
                window,
                token,
                result,
            } if token.starts_with("pdf:") => {
                let viewer = super::model::pdf_inline(match result {
                    Value::Bool(enabled) => Some(*enabled),
                    _ => None,
                });
                if !viewer && let Some(win) = self.by_window(window) {
                    // A window the engine cannot fill stays blank or
                    // downloads the file: close it, the panel says so.
                    win.state = WinState::NoPdfViewer;
                    if let Some(client) = client {
                        let _ = client.close(window);
                    }
                }
            }
            Event::WindowClosed { window, .. } => {
                if let Some(win) = self.by_window(window) {
                    // A window closed here for want of a PDF viewer says
                    // that, not that it was closed.
                    if win.state != WinState::NoPdfViewer {
                        win.state = WinState::Closed;
                    }
                }
            }
            Event::Error {
                window: Some(window),
                message,
                ..
            } => {
                if let Some(win) = self.by_window(window) {
                    win.state = WinState::Failed(message.clone());
                }
            }
            Event::NavigationBlocked { url, .. } => {
                return Some(url.clone());
            }
            _ => {}
        }
        None
    }

    fn by_window(&mut self, window: &str) -> Option<&mut Win> {
        self.wins.values_mut().find(|win| win.name == window)
    }
}

#[cfg(test)]
mod tests;
