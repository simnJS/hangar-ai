//! HTML tabs: a kit's page in a window of the webview helper, its files
//! served by hangar-core's `kit_scheme::serve`, its `window.hangar.call` and
//! `on` relayed over the helper's `ipc` to this package's `PageHost`, and so
//! to the same gate and API a native tab has (`KitFrame` of
//! KitTabView.tsx).
//!
//! The helper is the preview's (`preview::webview::Helper`), shared: windows
//! named `kit-…` are this package's, their events routed here and their
//! pages' requests answered here.
//!
//! What changes from a frame of the main window: a page is a window of its
//! own, opened the first time its tab's pane is on screen
//! (opening a window for a tab an agent added out of sight would take the
//! keyboard from whoever is typing, which a frame never did), and kept open
//! while the tab is, whichever is in front: on screen while its tab is in
//! front of a panel that is, hidden otherwise, with the same rules for the
//! keyboard as the preview's windows (`preview::webview`, "A window follows
//! what the person sees"). A reload is a new window and a new host; a page
//! that leaves for another document of its kit is closed, and the tab says so
//! (`kitsTab.left`).

use std::time::Instant;

use hangar_webview::{Client, Content, OpenWindow, SchemeRequest, SchemeResponse, kit_relay};
use serde_json::Value;

use super::api::{PageHost, Post};
use super::tabs::ThemeSender;
use crate::preview::store::WatchId;
use crate::preview::webview::{Arrival, Presence, WinState};

/// What this package's windows are called.
pub const PREFIX: &str = "kit-";

/// One open HTML tab.
pub struct PageTab {
    pub host: PageHost,
    pub state: WinState,
    /// The entry's revision the page was opened at.
    pub revision: u64,
    /// The page, as the window opens it.
    pub url: String,
    pub kit_id: String,
    /// Documents that finished loading: the first is the tab's page.
    loads: u32,
    /// It went to another document, and was closed.
    pub left: bool,
    pub theme: ThemeSender,
    /// The title the window was last given.
    title: String,
    pub watch: Option<WatchId>,
    /// Where the window is, and where it should be (`Presence`).
    presence: Presence,
    /// The window is not opened by itself: the window system would give the
    /// keyboard to any window it maps, and an agent opened the tab
    /// (`Arrival::Held`). The panel offers the window.
    pub held: bool,
}

impl std::fmt::Debug for PageTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageTab")
            .field("window", &self.host.window())
            .field("state", &self.state)
            .field("left", &self.left)
            .finish()
    }
}

/// What an event of the helper asks of the package.
pub enum Handled {
    Nothing,
    /// Messages for the page.
    Post(Vec<Value>),
    /// A call to run, whose answer goes back with `PageHost::settle`.
    Call(hangar_kit_ui::abi::CallId, hangar_kit_host::CallFuture),
}

impl PageTab {
    pub fn new(host: PageHost, kit_id: &str, url: String, revision: u64, title: &str) -> PageTab {
        PageTab {
            host,
            state: WinState::Waiting,
            revision,
            url,
            kit_id: kit_id.to_owned(),
            loads: 0,
            left: false,
            theme: ThemeSender::default(),
            title: title.to_owned(),
            watch: None,
            presence: Presence::new(true),
            held: false,
        }
    }

    /// How the window may come up by itself: `agent`, an agent or a hook
    /// opened the tab and the person has not asked for its window since;
    /// `unfocused_open`, a window can be opened without the keyboard here
    /// (`hangar_webview::opens_unfocused`). A held page is not opened; it
    /// reads as a closed window, and the panel offers it.
    pub fn arriving(mut self, agent: bool, unfocused_open: bool) -> PageTab {
        self.presence = Presence::new(!agent);
        if Arrival::of(agent, unfocused_open) == Arrival::Held {
            self.held = true;
            self.state = WinState::Closed;
        }
        self
    }

    pub fn window(&self) -> &str {
        self.host.window()
    }

    /// The window to open: the page, served by the kit scheme, with the
    /// bridge that gives it `window.hangar` as a frame did, and nowhere to go
    /// but the kit's own files. It does not ask for the keyboard unless the
    /// person asked for the tab, and stays hidden while the tab is not seen.
    pub fn open_command(&self, background: [u8; 3]) -> OpenWindow {
        let [red, green, blue] = background;
        let open = OpenWindow::new(self.window(), Content::url(self.url.clone()))
            .title(self.title.clone())
            .size(1000, 720)
            .scheme(kit_relay::SCHEME)
            .kit_bridge()
            .allow_navigation(kit_relay::kit_prefixes(&self.kit_id))
            .background(red, green, blue);
        self.presence.opening(open)
    }

    /// Opens the window, now that the helper runs.
    pub fn open(&mut self, client: &Client, background: [u8; 3]) {
        self.state = match client.open(self.open_command(background)) {
            Ok(()) => {
                self.presence.opened();
                WinState::Opening
            }
            Err(_) => WinState::Failed("the helper stopped".into()),
        };
    }

    /// Ties the window to the tab: on screen while the tab's pane is on
    /// screen, hidden otherwise (its page and what it was doing are
    /// kept; what plays is paused), and back without the keyboard.
    pub fn set_seen(&mut self, client: Option<&Client>, seen: bool) {
        self.presence.set_seen(seen);
        if let Some(client) = client
            && self.state == WinState::Open
        {
            self.presence.place(client, self.host.window());
        }
    }

    /// The panel's button: raises the window with the keyboard, putting it on
    /// screen first if it is hidden.
    pub fn bring_forward(&mut self, client: &Client) {
        if self.state == WinState::Open {
            self.presence.bring_forward(client, self.host.window());
        }
    }

    /// Names the window as the tab is named, when that changed.
    pub fn retitle(&mut self, client: &Client, title: &str) {
        if self.title != title {
            self.title = title.to_owned();
            if matches!(self.state, WinState::Open | WinState::Opening) {
                let _ = client.set_title(self.window(), title);
            }
        }
    }

    /// An event of the helper about this tab's window.
    pub fn on_event(&mut self, event: &hangar_webview::Event, now: Instant) -> Handled {
        use hangar_webview::{Event, PageLoadState};
        match event {
            Event::Opened { .. } => {
                self.state = WinState::Open;
                Handled::Nothing
            }
            Event::Ipc { window, uri, body } => match self.host.receive(window, uri, body, now) {
                Some(Post::Now(answer)) => Handled::Post(vec![answer]),
                Some(Post::Later(id, future)) => Handled::Call(id, future),
                None => Handled::Nothing,
            },
            Event::PageLoad {
                state: PageLoadState::Finished,
                ..
            } => {
                self.loads += 1;
                // The first document is the tab's page; any other is
                // somewhere it went, which speaks to no one.
                if self.loads > 1 {
                    self.host.close();
                    self.left = true;
                    return Handled::Nothing;
                }
                Handled::Post(self.theme.load().into_iter().collect())
            }
            Event::WindowClosed { .. } => {
                self.host.close();
                self.presence.gone();
                if !self.left {
                    self.state = WinState::Closed;
                }
                Handled::Nothing
            }
            Event::Error {
                window: Some(_),
                message,
                ..
            } => {
                self.state = WinState::Failed(message.clone());
                Handled::Nothing
            }
            _ => Handled::Nothing,
        }
    }
}

/// Answers a page's request for a file of its kit: hangar-core serves it, from
/// the installed kits that can be used, with the kit scheme's policy. The
/// helper's windows have no page of Hangar's own, so no origin is Hangar's.
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
    let response = hangar_core::kit_scheme::serve(&served, &[]);
    let mut out = SchemeResponse::new(request.request, response.status().as_u16());
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            out = out.header(name.as_str(), value);
        }
    }
    out.body(response.into_body())
}

/// The colour shown before a page paints: the theme's own, so that a dark
/// workspace's kit does not flash white.
pub fn background_of(theme: &hangar_theme::Theme) -> [u8; 3] {
    let [red, green, blue, _] = theme.colors.chrome.into_rgba8();
    [red, green, blue]
}
