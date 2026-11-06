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
//! `Helper::route` gets the events of the windows that start with it, and with
//! `Helper::serve` answers their pages' requests (a kit's page is served by
//! `kit_scheme::serve`, not from the preview's folders).
//!
//! # A window follows what the person sees
//!
//! The panel is in the app's window and the page is in another, so the two have
//! to be kept together by hand (`Windows::set_seen`):
//!
//! - **It is on screen only while its entry is.** The entry has to be the tab
//!   in front, of a workspace that is awake and drawn, whose panel is shown.
//!   When it stops being so the window is hidden, not closed, so that a PDF
//!   keeps its page and a player its place; a player is paused first, since a
//!   hidden window plays on. When it is so again the window comes back.
//! - **It does not take the keyboard from whoever is typing.** A window
//!   opened for something an agent or a hook showed (`Entry::opened_by`) is
//!   opened and brought back without asking for the keyboard. Where the window
//!   system gives the keyboard to any window it maps whatever is asked
//!   (Wayland, see [`Arrival`]), the app does not map one by itself for an
//!   agent's file at all: the panel offers the window, and a click on it is
//!   the person's.
//! - **The helper goes when nothing is shown in it.** Once neither the
//!   preview nor a kit's tab holds a window in it ([`Helper::hold`]) for
//!   [`HELPER_IDLE`], it is told to quit: WebKit's processes hold some
//!   hundred megabytes for as long as they run. The next page starts it again.
//! - **A PDF waits to be known drawable.** The engine is asked whether it has
//!   a PDF viewer once, by a window that stays hidden until it answers, so a
//!   window that would be blank never flashes up.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
type Serve = Arc<dyn Fn(&SchemeRequest) -> SchemeResponse + Send + Sync>;

/// How long the helper runs on with no window in it before it is told to
/// quit: long enough that going back to a PDF a moment later does not start
/// it again.
pub const HELPER_IDLE: Duration = Duration::from_secs(60);

/// The helper process and the way to it, shared by everything that shows a
/// page: started on the first window, and again after it was lost or let go.
#[derive(Default)]
pub struct Helper {
    client: Mutex<Option<Arc<Client>>>,
    routes: Mutex<Vec<(String, Route)>>,
    serves: Mutex<Vec<(String, Serve)>>,
    /// How many windows each user of the helper (by the prefix of their
    /// windows' names) holds in it.
    held: Mutex<HashMap<String, usize>>,
    /// Counts the changes of `held`: a check for idleness made before the
    /// last change is stale.
    held_changes: AtomicU64,
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

    /// Says how many windows the user of `prefix` holds in the helper, from
    /// the window's thread, where every window is opened. Answers the change
    /// to check for idleness after [`HELPER_IDLE`] when none is held any more
    /// and the helper runs.
    pub fn hold(&self, prefix: &str, windows: usize) -> Option<u64> {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if held.get(prefix).copied().unwrap_or(0) == windows {
            return None;
        }
        held.insert(prefix.to_string(), windows);
        let change = self.held_changes.fetch_add(1, Ordering::SeqCst) + 1;
        let idle = held.values().all(|count| *count == 0);
        drop(held);
        (idle && self.client().is_some()).then_some(change)
    }

    /// Tells the helper to quit if no window was held since `change`. Called
    /// on the window's thread, as every window is opened there, so none can
    /// be on its way meanwhile. Says whether it was told.
    pub fn quit_if_idle(&self, change: u64) -> bool {
        let idle = self
            .held
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .values()
            .all(|count| *count == 0);
        if !idle || self.held_changes.load(Ordering::SeqCst) != change {
            return false;
        }
        // Let go here: the next window starts another. The thread reading
        // its events holds the last client and drops it once the helper's
        // output ends, which reaps it.
        let Some(client) = self.lock_client().take() else {
            return false;
        };
        let _ = client.quit();
        true
    }

    /// A helper that runs `client`, for the tests.
    #[cfg(test)]
    pub(crate) fn running(client: Arc<Client>) -> Arc<Helper> {
        let helper = Helper::new();
        *helper.lock_client() = Some(client);
        helper
    }

    /// Hears the events of the windows whose name starts with `prefix`
    /// (other than this package's own).
    pub fn route(&self, prefix: &str, handler: impl Fn(&Event) + Send + Sync + 'static) {
        self.routes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push((prefix.to_string(), Box::new(handler)));
    }

    /// Answers the requests the pages of the windows whose name starts with
    /// `prefix` make to their schemes, in place of the preview's files.
    pub fn serve(
        &self,
        prefix: &str,
        answer: impl Fn(&SchemeRequest) -> SchemeResponse + Send + Sync + 'static,
    ) {
        self.serves
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push((prefix.to_string(), Arc::new(answer)));
    }

    /// Who answers a window's requests: whoever serves its prefix, or the
    /// preview's files.
    fn server_of(&self, window: &str) -> Option<Serve> {
        self.serves
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .find(|(prefix, _)| window.starts_with(prefix.as_str()))
            .map(|(_, serve)| serve.clone())
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
    /// answered by a small pool of threads ([`Pool`]), so that a big video
    /// does not hold up the next page and a page of 300 pictures does not
    /// start 300 threads; the rest goes to its owner.
    fn pump(&self, events: hangar_webview::Events, client: Arc<Client>, inbox: Inbox) {
        let pool = Pool::new("hangar-preview-serve", SERVE_THREADS);
        while let Some(event) = events.recv() {
            if let Event::SchemeRequest(request) = event {
                let client = client.clone();
                let server = self.server_of(&request.window);
                pool.run(move || {
                    let response = match server {
                        Some(serve) => serve(&request),
                        None => answer(&request),
                    };
                    // A helper that is gone has nobody to answer to.
                    let _: Result<(), Closed> = client.respond(response);
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
        | Event::Focused { window, .. }
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
    /// [`HELPER_IDLE`] passed since no window was held any more, at this
    /// change of what is held ([`Helper::hold`]).
    Idle(u64),
}

/// Holds `windows` for `prefix` in the helper, and when that leaves the
/// helper with nothing to show, the check that lets it go after
/// [`HELPER_IDLE`].
pub fn hold(helper: &Helper, prefix: &str, windows: usize) -> iced::Task<Message> {
    match helper.hold(prefix, windows) {
        Some(change) => iced::Task::perform(tokio::time::sleep(HELPER_IDLE), move |()| {
            Message::Preview(super::Msg::Web(WebMsg::Idle(change)))
        }),
        None => iced::Task::none(),
    }
}

/// A client, to ride in a message.
#[derive(Clone)]
pub struct HelperHandle(pub Arc<Client>);

impl std::fmt::Debug for HelperHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelperHandle(..)")
    }
}

impl WinState {
    /// The helper has, or is about to have, a window for it.
    pub fn holds(&self) -> bool {
        matches!(self, WinState::Waiting | WinState::Opening | WinState::Open)
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

/// How a window may come up by itself for an entry, which depends on who
/// showed the entry and on what the window system does with a window it maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// The person opened the entry: its window may take the keyboard.
    Focused,
    /// An agent or a hook showed it, and the window system opens a window
    /// without the keyboard when told to (X11, Windows, macOS): the window
    /// comes up beside whoever is typing.
    Unfocused,
    /// An agent or a hook showed it, and the window system gives the keyboard
    /// to any window it maps, whatever the window is told (Wayland: see
    /// `hangar_webview::opens_unfocused`). The window is not mapped by itself,
    /// not when it opens and not when it comes back: the panel offers it, and
    /// the person's click is what opens it.
    Held,
}

impl Arrival {
    /// `agent`: the entry was shown by an agent or a hook (`Entry::opened_by`).
    /// `unfocused_open`: a window can be opened without the keyboard here.
    pub fn of(agent: bool, unfocused_open: bool) -> Arrival {
        match (agent, unfocused_open) {
            (false, _) => Arrival::Focused,
            (true, true) => Arrival::Unfocused,
            (true, false) => Arrival::Held,
        }
    }
}

/// Where a helper window is, and where it should be: on screen while its
/// entry is seen, hidden while it is not, and never taking the keyboard from
/// whoever is typing unless the person asked for it. The preview's windows
/// (`Win`) and the kit tabs' pages (`kit_tabs::bridge::PageTab`) each hold one
/// and ask it what to send.
///
/// Whether a window is made at all for what an agent showed is decided before
/// there is one (`Arrival`). A window that exists is on screen or off it with
/// its entry, whoever opened the file.
#[derive(Debug, Clone)]
pub struct Presence {
    /// The helper has the window on screen, as far as the app knows.
    shown: bool,
    /// Its entry's pane is on screen.
    seen: bool,
    /// The window asks for the keyboard the first time it is on screen,
    /// because the person asked for it.
    keyboard: bool,
    /// What the window holds is known to be drawable. Only a PDF waits for
    /// it: its window stays hidden until the engine says it has a viewer.
    drawable: bool,
}

impl Presence {
    /// For the window of an entry whose pane is on screen.
    /// `keyboard`: the person asked for the window, which may take the
    /// keyboard when it first shows; otherwise it never does.
    pub fn new(keyboard: bool) -> Presence {
        Presence {
            shown: false,
            seen: true,
            keyboard,
            drawable: true,
        }
    }

    /// The window is not shown before the engine is known to draw its page.
    pub fn until_drawable(mut self, drawable: bool) -> Presence {
        self.drawable = drawable;
        self
    }

    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// Whether the window is to be on screen as it opens.
    fn visible_at_open(&self) -> bool {
        self.seen && self.drawable
    }

    /// The `open` command, hidden while the entry is not seen or the page not
    /// known drawable (it loads all the same), and asking for the keyboard only
    /// when it is shown and the person asked.
    pub fn opening(&self, command: OpenWindow) -> OpenWindow {
        let show = self.visible_at_open();
        let command = if show { command } else { command.hidden() };
        if show && self.keyboard {
            command
        } else {
            command.unfocused()
        }
    }

    /// The `open` command went out.
    pub fn opened(&mut self) {
        self.shown = self.visible_at_open();
        self.keyboard &= !self.shown;
    }

    /// The window is gone, or the helper is.
    pub fn gone(&mut self) {
        self.shown = false;
    }

    /// Whether the entry's pane is on screen.
    pub fn set_seen(&mut self, seen: bool) {
        self.seen = seen;
    }

    /// The engine is found to draw the page.
    pub fn make_drawable(&mut self) {
        self.drawable = true;
    }

    /// Puts an open window on the screen or off it, as the entry says: on
    /// screen while the entry is seen and the page is known drawable, taken off
    /// otherwise, with its media paused. It asks for the keyboard only the
    /// first time, and only if the person asked for the window.
    pub fn place(&mut self, client: &Client, window: &str) {
        let wanted = self.seen && self.drawable;
        if wanted == self.shown {
            return;
        }
        show(client, window, wanted, self.keyboard);
        self.shown = wanted;
        if wanted {
            self.keyboard = false;
        }
    }

    /// What the person asks for with the panel's button: the window raised
    /// with the keyboard, put on screen first if it is hidden. Nothing is done
    /// for a page not known drawable.
    pub fn bring_forward(&mut self, client: &Client, window: &str) {
        self.seen = true;
        if self.shown {
            let _ = client.focus(window);
        } else if self.drawable {
            show(client, window, true, true);
            self.shown = true;
            self.keyboard = false;
        }
    }
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
    /// What the player's element said when it could not play the file; empty
    /// when the engine gave no reason.
    pub media_error: Option<String>,
    /// Whether it is on screen (`Windows::set_seen`).
    presence: Presence,
}

impl Win {
    /// The helper has the window on screen.
    #[cfg(test)]
    pub fn is_shown(&self) -> bool {
        self.presence.is_shown()
    }
}

/// What an event of a window asks of the app.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Handled {
    /// A link the window was stopped from following, to open outside.
    pub link: Option<String>,
}

/// What a window that goes out of sight is told, whatever it holds: stop what
/// plays. A window that is hidden goes on, with its sound.
pub const PAUSE_MEDIA: &str =
    "document.querySelectorAll('video,audio').forEach(function(m){m.pause()})";

/// The address a player's page reports to, in place of an `ipc` it does not
/// have: `hangar-media:error?<reason>` or `hangar-media:ready?`.
const MEDIA_REPORT: &str = "hangar-media:";

/// Run in a player's page before any of its own, to tell the app when its
/// media cannot be played, from the element's own `error` event (and when it
/// plays after all, `loadeddata`). The events do not bubble, so they are caught
/// on the way down.
///
/// A player's page is the engine's own document for the file, and WebKit does
/// not deliver the messages of `window.ipc` from one (observed on WebKitGTK
/// 2.52.6: the call returns and nothing arrives), so the page reports in the
/// one way it has left: it goes to `MEDIA_REPORT`. The window allows no such
/// navigation (`allow_navigation`), so the page stays where it is and the
/// helper tells the app that a navigation was refused, with the address.
pub const MEDIA_WATCH: &str = "(function(){\
var names=['','MEDIA_ERR_ABORTED','MEDIA_ERR_NETWORK','MEDIA_ERR_DECODE','MEDIA_ERR_SRC_NOT_SUPPORTED'];\
function say(state,message){location.href='hangar-media:'+state+'?'+encodeURIComponent(message||'')}\
function media(e){return e.target instanceof HTMLMediaElement?e.target:null}\
window.addEventListener('error',function(e){var m=media(e);\
if(m)say('error',m.error?(m.error.message||names[m.error.code]||''):'')},true);\
window.addEventListener('loadeddata',function(e){if(media(e))say('ready')},true);\
})();";

/// The longest reason a player's page may give for not playing, in characters.
const MEDIA_REASON_MAX: usize = 200;

/// What a player's page reported by going to `MEDIA_REPORT`: the reason its
/// media failed (empty when the engine gave none), or `None` for it playing.
/// Any other address is not a report, and reads as nothing at all.
fn read_media_report(url: &str) -> Option<Option<String>> {
    // A report is a short address; a page sending megabytes is not making one.
    if url.len() > 2048 {
        return None;
    }
    let rest = url.strip_prefix(MEDIA_REPORT)?;
    let (state, reason) = rest.split_once('?').unwrap_or((rest, ""));
    match state {
        "error" => Some(Some(
            percent_decoded(reason)
                .chars()
                .take(MEDIA_REASON_MAX)
                .collect(),
        )),
        "ready" => Some(None),
        _ => None,
    }
}

/// `decodeURIComponent`, kindly: a `%` that starts no escape stays as it is,
/// and bytes that are no UTF-8 become the replacement character.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escape = (bytes[at] == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match escape {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Puts a window on screen or takes it off, as a window follows its entry.
/// Taken off, what plays stops first. Put back, it asks for the keyboard only
/// when `keyboard` says so.
pub fn show(client: &Client, window: &str, shown: bool, keyboard: bool) {
    if !shown {
        let _ = client.eval(window, PAUSE_MEDIA);
    }
    let _ = client.set_visible(window, shown, shown && keyboard);
}

/// What the bar's reload of a page does (`HtmlView.tsx`: it mounted the frame
/// again whatever had become of it, and read the source again when that was
/// what was shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reload {
    /// The window is open: its page loads again.
    pub navigate: bool,
    /// The window was closed, or failed: it is made again, for the person
    /// asked for it.
    pub reopen: bool,
    /// The source is shown: the file is read again.
    pub reread: bool,
}

impl Reload {
    /// `source`: the page is shown as its source. `window`: the state of the
    /// entry's window, if it has one.
    pub fn of(source: bool, window: Option<&WinState>) -> Reload {
        Reload {
            navigate: window == Some(&WinState::Open),
            reopen: matches!(window, Some(WinState::Closed | WinState::Failed(_))),
            reread: source,
        }
    }
}

/// The entries whose windows are to be on screen: those whose pane is on
/// screen, in a workspace that is awake. `front` lists the files of the
/// workspaces on screen: the workspace, the entry, and whether its pane is
/// on screen.
pub fn seen_entries<'a>(
    front: impl IntoIterator<Item = (&'a str, &'a Entry, bool)>,
    asleep: impl Fn(&str) -> bool,
) -> HashMap<&'a str, &'a Entry> {
    front
        .into_iter()
        .filter(|(workspace, _, shown)| *shown && !asleep(workspace))
        .map(|(_, entry, _)| (entry.id.as_str(), entry))
        .collect()
}

/// The windows of the entries, by entry id.
#[derive(Debug)]
pub struct Windows {
    wins: HashMap<String, Win>,
    /// Entries a window was opened for on their own: a closed one stays
    /// closed until the person asks for it again.
    tried: HashSet<String>,
    /// Whether the engine has a PDF viewer, once a PDF window has asked.
    pdf_viewer: Option<bool>,
    /// A window can be opened without the keyboard here.
    unfocused_open: bool,
}

impl Windows {
    /// How many windows the helper has, or is about to have, for these.
    pub fn held(&self) -> usize {
        self.wins.values().filter(|win| win.state.holds()).count()
    }
}

impl Default for Windows {
    fn default() -> Self {
        // The tests of the package drive it with files an agent showed and
        // expect their windows; whether a session lets a window open without
        // the keyboard is the machine's and not theirs, so they assume it does
        // and the policy is tested on its own (`with_unfocused_open`).
        Windows::with_unfocused_open(cfg!(test) || hangar_webview::opens_unfocused())
    }
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
    let open = OpenWindow::new(
        name,
        Content::url(preview_url(&entry.path, entry.revision, false)),
    )
    .title(entry.title.clone())
    .size(width, height)
    .scheme(SCHEME)
    .allow_navigation([scheme_base(SCHEME)])
    .background(red, green, blue);
    match kind {
        // The player's own page tells when its media cannot be played.
        Kind::Video | Kind::Audio => open.init_script(MEDIA_WATCH),
        _ => open,
    }
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
    /// No window yet; `unfocused_open` says whether the window system opens a
    /// window without the keyboard when told to (`Default` asks the session).
    pub fn with_unfocused_open(unfocused_open: bool) -> Windows {
        Windows {
            wins: HashMap::new(),
            tried: HashSet::new(),
            pdf_viewer: None,
            unfocused_open,
        }
    }

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
    #[cfg(test)]
    pub fn entry_of(&self, window: &str) -> Option<&str> {
        self.wins
            .iter()
            .find(|(_, win)| win.name == window)
            .map(|(entry, _)| entry.as_str())
    }

    /// How a window may come up by itself for an entry.
    pub fn arrival(&self, entry: &Entry) -> Arrival {
        Arrival::of(entry.opened_by.is_some(), self.unfocused_open)
    }

    /// The entries among `seen` that have no window and have not had one tried,
    /// each with whether its window may ask for the keyboard: a file whose
    /// pane is on screen opens its own window once. An entry an agent showed
    /// is left out where the window system would hand the keyboard to the
    /// window (`Arrival::Held`): its pane offers the window instead.
    pub fn to_open(&self, seen: &HashMap<&str, &Entry>) -> Vec<(Entry, bool)> {
        let mut due: Vec<(Entry, bool)> = seen
            .values()
            .filter(|entry| !self.was_tried(&entry.id) && self.get(&entry.id).is_none())
            .filter_map(|entry| match self.arrival(entry) {
                Arrival::Focused => Some(((*entry).clone(), true)),
                Arrival::Unfocused => Some(((*entry).clone(), false)),
                Arrival::Held => None,
            })
            .collect();
        due.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        due
    }

    /// Opens (or opens again) the window of an entry, whose pane is on
    /// screen: sent at once when the helper runs, kept waiting when it
    /// is starting. `keyboard`: the window asks for the keyboard when it first
    /// shows, which is for a window the person asked for.
    ///
    /// A PDF is not shown before the engine is known to draw it. When that is
    /// known not to be, no window is made at all.
    pub fn open(
        &mut self,
        client: Option<&Client>,
        entry: &Entry,
        kind: Kind,
        theme: &hangar_theme::Theme,
        keyboard: bool,
    ) {
        let name = format!("{PREFIX}{}", entry.id);
        let previous = self.wins.get(&entry.id).cloned();
        let pdf = matches!(kind, Kind::Pdf);
        let win = Win {
            name: name.clone(),
            state: if pdf && self.pdf_viewer == Some(false) {
                WinState::NoPdfViewer
            } else {
                WinState::Waiting
            },
            revision: entry.revision,
            kind,
            looping: previous.as_ref().is_some_and(|win| win.looping),
            speed: previous.as_ref().map_or(1.0, |win| win.speed),
            media_error: None,
            presence: Presence::new(keyboard).until_drawable(!pdf || self.pdf_viewer == Some(true)),
        };
        let drawable = win.state == WinState::Waiting;
        self.wins.insert(entry.id.clone(), win);
        if drawable && let Some(client) = client {
            self.send_open(client, entry, theme);
        }
    }

    fn send_open(&mut self, client: &Client, entry: &Entry, theme: &hangar_theme::Theme) {
        let Some(win) = self.wins.get_mut(&entry.id) else {
            return;
        };
        let command = win
            .presence
            .opening(open_command(&win.name, entry, win.kind, theme));
        win.state = match client.open(command) {
            Ok(()) => {
                win.presence.opened();
                WinState::Opening
            }
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
            win.presence.gone();
        }
    }

    /// Puts a window on the screen or off it as its entry says
    /// (`Presence::place`), once it is open: a window still on its way is
    /// placed when it says it is.
    fn place(win: &mut Win, client: &Client) {
        if win.state == WinState::Open {
            win.presence.place(client, &win.name);
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

    /// Ties the windows to what the person sees: `seen` are the entries whose
    /// pane is on screen (`seen_entries`). A window of one of them is
    /// put on screen, a window of any other is taken off it, with its media
    /// paused (`show`). A window comes back without the keyboard (`focus` is
    /// what asks for it).
    pub fn set_seen(&mut self, client: Option<&Client>, seen: &HashMap<&str, &Entry>) {
        for (id, win) in &mut self.wins {
            win.presence.set_seen(seen.contains_key(id.as_str()));
            if let Some(client) = client {
                Self::place(win, client);
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

    /// Brings a window forward, with the keyboard: a window that is on screen
    /// is raised, one that is hidden (because its tab was behind, or because
    /// it is an agent's and waits for the person) is put on screen. This is
    /// what the person asks for with the button of the entry's pane, so the
    /// pane is on screen.
    pub fn focus(&mut self, client: &Client, entry: &str) {
        let Some(win) = self
            .wins
            .get_mut(entry)
            .filter(|win| win.state == WinState::Open)
        else {
            return;
        };
        win.presence.bring_forward(client, &win.name);
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

    /// An event of the helper about one of the windows.
    pub fn on_event(&mut self, client: Option<&Client>, event: &Event) -> Handled {
        let mut handled = Handled::default();
        match event {
            Event::Opened { window } => {
                if let Some(win) = self.by_window(window) {
                    win.state = WinState::Open;
                    // What was asked of the window when it was opened may not
                    // hold any more: the tab may have been left meanwhile.
                    if let Some(client) = client {
                        Self::place(win, client);
                    }
                }
            }
            Event::PageLoad {
                window,
                state: hangar_webview::PageLoadState::Finished,
                ..
            } => {
                let known = self.pdf_viewer;
                let Some(win) = self.by_window(window) else {
                    return handled;
                };
                if let Some(client) = client {
                    match win.kind {
                        // Asked once: the engine is the same for every window.
                        Kind::Pdf if known.is_none() => {
                            let _ = client.eval_with_token(
                                &win.name,
                                PDF_CHECK,
                                &format!("pdf:{}", win.name),
                            );
                        }
                        Kind::Video | Kind::Audio => {
                            let _ = client.eval(&win.name, &player_script(win.looping, win.speed));
                            // A page can start playing as it loads (a media
                            // document does), also in a window nobody sees.
                            if !win.presence.is_shown() {
                                let _ = client.eval(&win.name, PAUSE_MEDIA);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::EvalResult { token, result, .. } if token.starts_with("pdf:") => {
                let viewer = super::model::pdf_inline(match result {
                    Value::Bool(enabled) => Some(*enabled),
                    _ => None,
                });
                self.pdf_viewer = Some(viewer);
                for win in self.wins.values_mut().filter(|win| win.kind == Kind::Pdf) {
                    if viewer {
                        win.presence.make_drawable();
                        if let Some(client) = client {
                            Self::place(win, client);
                        }
                    } else if matches!(win.state, WinState::Opening | WinState::Open) {
                        // A window the engine cannot fill stays blank or
                        // downloads the file: close it, the panel says so. It
                        // was hidden all along, so nothing flashed.
                        win.state = WinState::NoPdfViewer;
                        win.presence.gone();
                        if let Some(client) = client {
                            let _ = client.close(&win.name);
                        }
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
                    win.presence.gone();
                }
            }
            // A command that raced a window's closing, or a script the page
            // would not run (the pause of a hidden window is sent whatever
            // the page holds), is no failure of the window.
            Event::Error {
                code:
                    hangar_webview::ErrorCode::UnknownWindow | hangar_webview::ErrorCode::EvalFailed,
                ..
            } => {}
            Event::Error {
                window: Some(window),
                message,
                ..
            } => {
                if let Some(win) = self.by_window(window) {
                    win.state = WinState::Failed(message.clone());
                }
            }
            Event::NavigationBlocked {
                window,
                url,
                new_window,
            } => {
                // A player's page reports to an address (`MEDIA_WATCH`); any
                // other page may try to go where it likes, and what it is
                // refused is a link for the browser.
                let report = self
                    .by_window(window)
                    .filter(|win| !new_window && matches!(win.kind, Kind::Video | Kind::Audio))
                    .and_then(|win| read_media_report(url).map(|report| (win, report)));
                match report {
                    Some((win, report)) => win.media_error = report,
                    None => handled.link = Some(url.clone()),
                }
            }
            _ => {}
        }
        handled
    }

    fn by_window(&mut self, window: &str) -> Option<&mut Win> {
        self.wins.values_mut().find(|win| win.name == window)
    }
}

#[cfg(test)]
pub(crate) mod testing;
/// How many requests of the helper's pages are served at once.
const SERVE_THREADS: usize = 6;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A few threads that run what they are given in turn: at most `size` at
/// once, the rest waiting in line. A thread the system refuses is one fewer;
/// with none at all, a job runs on the caller's thread rather than never.
pub(crate) struct Pool {
    sender: Option<std::sync::mpsc::Sender<Job>>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Pool {
    pub(crate) fn new(name: &str, size: usize) -> Pool {
        let (sender, receiver) = std::sync::mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let threads: Vec<_> = (0..size.max(1))
            .filter_map(|n| {
                let receiver = receiver.clone();
                std::thread::Builder::new()
                    .name(format!("{name}-{n}"))
                    .spawn(move || {
                        loop {
                            let job = receiver
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .recv();
                            let Ok(job) = job else {
                                return;
                            };
                            // A job that panics loses its own answer, not the
                            // thread.
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                        }
                    })
                    .ok()
            })
            .collect();
        Pool {
            sender: (!threads.is_empty()).then_some(sender),
            threads,
        }
    }

    /// Runs `job` on a thread of the pool, once one is free.
    pub(crate) fn run(&self, job: impl FnOnce() + Send + 'static) {
        let job: Job = Box::new(job);
        match &self.sender {
            Some(sender) => {
                if let Err(refused) = sender.send(job) {
                    (refused.0)();
                }
            }
            None => job(),
        }
    }

    /// How many threads it has.
    #[cfg(test)]
    pub(crate) fn size(&self) -> usize {
        self.threads.len()
    }
}

/// The threads finish what is in line, then end.
impl Drop for Pool {
    fn drop(&mut self) {
        self.sender = None;
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests;
