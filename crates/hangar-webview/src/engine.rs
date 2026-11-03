//! The windows: tao makes them, wry puts the platform's webview in each, and
//! this module turns what the parent asks into calls on both and what they
//! report into events.
//!
//! Everything here runs on the main thread, which is where GTK, WebView2 and
//! WKWebView want to be driven from. The one thread besides is the reader of
//! standard input, which hands each command over through the event loop's proxy.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use hangar_webview::{
    CloseReason, Command, Content, ErrorCode, Event, Header, KIT_BRIDGE, Navigation, OpenWindow,
    PROTOCOL_VERSION, PageLoadState, SchemeRequest, SchemeResponse, valid_scheme_name,
};
use tao::dpi::LogicalSize;
use tao::event::{Event as LoopEvent, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::http::{HeaderName, HeaderValue, Response, StatusCode};
use wry::{
    NewWindowResponse, PageLoadEvent, PermissionResponse, RequestAsyncResponder, WebView,
    WebViewBuilder,
};

use crate::output::Emitter;

/// What a window measures when the parent does not say: roomy enough for a page
/// meant for a tab of Hangar's.
const DEFAULT_SIZE: (u32, u32) = (1000, 720);

/// What the loop is woken for.
enum UserEvent {
    Command(Command),
    /// Standard input ended, or the parent said `quit`.
    Quit,
}

/// A window and its webview. The webview is the first field because fields drop
/// in order, and a webview must go before the window it is drawn in.
struct Open {
    webview: WebView,
    window: Window,
}

/// A scheme request the page is waiting on.
struct Waiting {
    window: String,
    responder: RequestAsyncResponder,
}

type Pending = Arc<Mutex<HashMap<u64, Waiting>>>;

struct Engine {
    emitter: Emitter,
    windows: HashMap<String, Open>,
    by_loop_id: HashMap<WindowId, String>,
    pending: Pending,
    requests: Arc<AtomicU64>,
}

/// Run until told to stop. Does not return: the event loop owns the process.
pub fn run(emitter: Emitter) -> ! {
    // A panic is the parent's business too: it hears of it as an error, in
    // place of a helper that stops without a word. Whatever it printed to
    // standard error stays, for a person to read.
    let previous = std::panic::take_hook();
    let panicking = emitter.clone();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        panicking.error(
            None,
            ErrorCode::Other,
            format!("the helper panicked: {info}"),
        );
        panicking.flush();
    }));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let reading = emitter.clone();
    thread::Builder::new()
        .name("input".into())
        .spawn(move || read_commands(&proxy, &reading))
        .expect("a thread starts");

    let mut engine = Engine {
        emitter,
        windows: HashMap::new(),
        by_loop_id: HashMap::new(),
        pending: Arc::default(),
        requests: Arc::new(AtomicU64::new(0)),
    };
    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            LoopEvent::NewEvents(StartCause::Init) => engine.ready(),
            // Both ways to ask for the end, standard input closing and `quit`,
            // leave through the loop, so that its last event flushes the output.
            LoopEvent::UserEvent(UserEvent::Quit | UserEvent::Command(Command::Quit)) => {
                engine.quit();
                *flow = ControlFlow::Exit;
            }
            LoopEvent::UserEvent(UserEvent::Command(command)) => engine.command(command, target),
            LoopEvent::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(id) = engine.by_loop_id.get(&window_id).cloned() {
                    engine.close(&id, CloseReason::User);
                }
            }
            LoopEvent::LoopDestroyed => engine.emitter.flush(),
            _ => {}
        }
    })
}

/// Reads commands from standard input until it ends, which is a parent that has
/// gone: the helper then quits, so it never outlives the app that started it.
fn read_commands(proxy: &EventLoopProxy<UserEvent>, emitter: &Emitter) {
    let mut input = BufReader::new(std::io::stdin().lock());
    let mut raw = Vec::new();
    loop {
        raw.clear();
        match input.read_until(b'\n', &mut raw) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(text) = std::str::from_utf8(&raw) else {
            emitter.error(None, ErrorCode::BadCommand, "a command must be UTF-8 text");
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match Command::from_line(text) {
            Ok(command) => {
                if proxy.send_event(UserEvent::Command(command)).is_err() {
                    return;
                }
            }
            Err(error) => emitter.error(None, ErrorCode::BadCommand, error.to_string()),
        }
    }
    let _ = proxy.send_event(UserEvent::Quit);
}

impl Engine {
    fn ready(&self) {
        let engine = wry::webview_version().unwrap_or_else(|error| format!("unknown ({error})"));
        self.emitter.emit(Event::Ready {
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            engine: format!("{engine} on {}", std::env::consts::OS),
        });
    }

    fn command(&mut self, command: Command, target: &EventLoopWindowTarget<UserEvent>) {
        match command {
            Command::Open(open) => self.open(open, target),
            Command::Navigate { window, url } => {
                if let Some(open) = self.window(&window)
                    && let Err(error) = open.webview.load_url(&url)
                {
                    self.emitter
                        .error(Some(&window), ErrorCode::NavigateFailed, error.to_string());
                }
            }
            Command::Eval {
                window,
                script,
                token,
            } => self.eval(&window, &script, token),
            Command::PostMessage { window, message } => {
                // The message is parsed again inside the page, from a string
                // literal, and not written there as an object literal: a key like
                // `__proto__` is a property when it is parsed and the prototype
                // when it is written.
                let json = serde_json::to_string(&message).expect("a JSON value serializes");
                let literal = serde_json::to_string(&json).expect("a string serializes");
                let script = format!(
                    "(function (m) {{ var h = window.__hangarHost; if (h) h.deliver(m); else window.postMessage(m, '*'); }})(JSON.parse({literal}))"
                );
                self.eval(&window, &script, None);
            }
            Command::SetTitle { window, title } => {
                if let Some(open) = self.window(&window) {
                    open.window.set_title(&title);
                }
            }
            Command::Focus { window } => {
                if let Some(open) = self.window(&window) {
                    open.window.set_focus();
                    let _ = open.webview.focus();
                }
            }
            Command::Close { window } => {
                if self.window(&window).is_some() {
                    self.close(&window, CloseReason::Command);
                }
            }
            Command::SchemeResponse(response) => self.respond(response),
            // Taken by the loop itself, which has to leave.
            Command::Quit => {}
        }
    }

    /// The window, or an error to the parent if there is none.
    fn window(&self, id: &str) -> Option<&Open> {
        let found = self.windows.get(id);
        if found.is_none() {
            self.emitter.error(
                Some(id),
                ErrorCode::UnknownWindow,
                format!("no window {id} is open"),
            );
        }
        found
    }

    fn eval(&self, window: &str, script: &str, token: Option<String>) {
        let Some(open) = self.window(window) else {
            return;
        };
        let result = match token {
            None => open.webview.evaluate_script(script),
            Some(token) => {
                let emitter = self.emitter.clone();
                let id = window.to_string();
                open.webview
                    .evaluate_script_with_callback(script, move |raw| {
                        // The engine hands back JSON text; an engine that hands back
                        // something else still gets its answer through, as a string.
                        let result = if raw.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::from_str(&raw).unwrap_or(serde_json::Value::String(raw))
                        };
                        emitter.emit(Event::EvalResult {
                            window: id.clone(),
                            token: token.clone(),
                            result,
                        });
                    })
            }
        };
        if let Err(error) = result {
            self.emitter
                .error(Some(window), ErrorCode::EvalFailed, error.to_string());
        }
    }

    fn open(&mut self, open: OpenWindow, target: &EventLoopWindowTarget<UserEvent>) {
        let id = open.window.clone();
        if self.windows.contains_key(&id) {
            self.emitter.error(
                Some(&id),
                ErrorCode::DuplicateWindow,
                format!("window {id} is already open"),
            );
            return;
        }
        if let Some(bad) = open.schemes.iter().find(|name| !valid_scheme_name(name)) {
            self.emitter.error(
                Some(&id),
                ErrorCode::OpenFailed,
                format!("{bad:?} cannot be a custom scheme"),
            );
            return;
        }
        match self.build(&open, target) {
            Ok(built) => {
                self.by_loop_id.insert(built.window.id(), id.clone());
                self.windows.insert(id.clone(), built);
                self.emitter.emit(Event::Opened { window: id });
            }
            Err(message) => self
                .emitter
                .error(Some(&id), ErrorCode::OpenFailed, message),
        }
    }

    /// The window and webview for `open`. Every handler is bound to the window's
    /// id, so what a page does is always reported as that window's.
    fn build(
        &self,
        open: &OpenWindow,
        target: &EventLoopWindowTarget<UserEvent>,
    ) -> Result<Open, String> {
        let (width, height) = open
            .size
            .map_or(DEFAULT_SIZE, |size| (size.width, size.height));
        let window = WindowBuilder::new()
            .with_title(open.title.as_str())
            .with_inner_size(LogicalSize::new(f64::from(width), f64::from(height)))
            .build(target)
            .map_err(|error| error.to_string())?;

        let id = open.window.as_str();
        let mut builder = WebViewBuilder::new()
            .with_id(id)
            .with_devtools(open.devtools)
            .with_incognito(open.ephemeral)
            // A page asks for none of these on its own account: a window of its
            // own, a file on disk, the camera. The parent is told of the first,
            // and the rest are refused.
            .with_download_started_handler(|_, _| false)
            .with_permission_handler(|_| PermissionResponse::Deny);
        if let Some([red, green, blue]) = open.background {
            builder = builder.with_background_color((red, green, blue, 255));
        }

        let emitter = self.emitter.clone();
        let window_id = id.to_string();
        builder = builder.with_new_window_req_handler(move |url, _| {
            emitter.emit(Event::NavigationBlocked {
                window: window_id.clone(),
                url,
                new_window: true,
            });
            NewWindowResponse::Deny
        });

        let emitter = self.emitter.clone();
        let window_id = id.to_string();
        builder = builder.with_ipc_handler(move |request| {
            emitter.emit(Event::Ipc {
                window: window_id.clone(),
                uri: request.uri().to_string(),
                body: request.into_body(),
            });
        });

        let emitter = self.emitter.clone();
        let window_id = id.to_string();
        builder = builder.with_on_page_load_handler(move |event, url| {
            let state = match event {
                PageLoadEvent::Started => PageLoadState::Started,
                PageLoadEvent::Finished => PageLoadState::Finished,
            };
            emitter.emit(Event::PageLoad {
                window: window_id.clone(),
                state,
                url,
            });
        });

        let emitter = self.emitter.clone();
        let window_id = id.to_string();
        builder = builder.with_document_title_changed_handler(move |title| {
            emitter.emit(Event::TitleChanged {
                window: window_id.clone(),
                title,
            });
        });

        if let Some(prefixes) = &open.allow_navigation {
            let allowed = Navigation::new(prefixes, &open.schemes);
            let emitter = self.emitter.clone();
            let window_id = id.to_string();
            builder = builder.with_navigation_handler(move |url| {
                let allowed = allowed.allows(&url);
                if !allowed {
                    emitter.emit(Event::NavigationBlocked {
                        window: window_id.clone(),
                        url,
                        new_window: false,
                    });
                }
                allowed
            });
        }

        for scheme in &open.schemes {
            let emitter = self.emitter.clone();
            let pending = self.pending.clone();
            let requests = self.requests.clone();
            builder = builder.with_asynchronous_custom_protocol(
                scheme.clone(),
                move |window, request, responder| {
                    // The request is parked here under a number of ours and told to
                    // the parent, so the page waits and the helper does not.
                    let number = requests.fetch_add(1, Ordering::Relaxed) + 1;
                    let (head, body) = request.into_parts();
                    let headers = head
                        .headers
                        .iter()
                        .map(|(name, value)| {
                            Header::new(name.as_str(), String::from_utf8_lossy(value.as_bytes()))
                        })
                        .collect();
                    pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(
                            number,
                            Waiting {
                                window: window.to_string(),
                                responder,
                            },
                        );
                    emitter.emit(Event::SchemeRequest(SchemeRequest {
                        request: number,
                        window: window.to_string(),
                        method: head.method.to_string(),
                        uri: head.uri.to_string(),
                        headers,
                        body,
                    }));
                },
            );
        }

        if open.kit_bridge {
            builder = builder.with_initialization_script(KIT_BRIDGE);
        }
        if let Some(script) = &open.init_script {
            builder = builder.with_initialization_script(script.as_str());
        }
        builder = match &open.content {
            Content::Url { url } => builder.with_url(url.as_str()),
            Content::Html { html } => builder.with_html(html.as_str()),
        };

        let webview = attach(builder, &window)?;
        Ok(Open { webview, window })
    }

    /// Close a window for `reason`, and tell the parent. Requests of its pages
    /// that nobody answered are dropped with it.
    fn close(&mut self, id: &str, reason: CloseReason) {
        if let Some(open) = self.windows.remove(id) {
            self.by_loop_id.remove(&open.window.id());
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|_, waiting| waiting.window != id);
            drop(open);
            self.emitter.emit(Event::WindowClosed {
                window: id.to_string(),
                reason,
            });
        }
    }

    fn quit(&mut self) {
        let ids: Vec<String> = self.windows.keys().cloned().collect();
        for id in ids {
            self.close(&id, CloseReason::Quit);
        }
        self.emitter.flush();
    }

    /// Hand the parent's answer to the page that is waiting for it. An answer
    /// for a request that is gone (its window closed, or the page went
    /// elsewhere) is dropped without a word: it is a race, not a mistake.
    fn respond(&self, response: SchemeResponse) {
        let waiting = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&response.request);
        let Some(waiting) = waiting else { return };
        let (built, problems) = http_response(response);
        if !problems.is_empty() {
            self.emitter.error(
                Some(&waiting.window),
                ErrorCode::BadResponse,
                problems.join("; "),
            );
        }
        waiting.responder.respond(built);
    }
}

/// The engine's response for the parent's. What cannot be carried over is
/// reported and not fatal: a header that is not valid is left out, and a status
/// that is not HTTP's becomes a 502 that says so, since the page is waiting
/// for something either way.
fn http_response(response: SchemeResponse) -> (Response<Vec<u8>>, Vec<String>) {
    let mut problems = Vec::new();
    let status = match StatusCode::from_u16(response.status) {
        Ok(status) => status,
        Err(_) => {
            problems.push(format!("{} is not an HTTP status", response.status));
            let body = format!("the parent answered with status {}", response.status);
            let bad = Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(body.into_bytes())
                .expect("a plain response builds");
            return (bad, problems);
        }
    };
    let mut built = Response::new(response.body);
    *built.status_mut() = status;
    for header in response.headers {
        match (
            HeaderName::from_bytes(header.name.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) {
            (Ok(name), Ok(value)) => {
                built.headers_mut().append(name, value);
            }
            _ => problems.push(format!("the header {:?} is not valid", header.name)),
        }
    }
    (built, problems)
}

/// Build the webview in `window`. GTK has no handle to build from on Wayland,
/// so on Linux it goes into the window's own box; the other platforms take the
/// window.
#[cfg(not(any(windows, target_os = "macos")))]
fn attach(builder: WebViewBuilder, window: &Window) -> Result<WebView, String> {
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;

    let vbox = window
        .default_vbox()
        .ok_or("the window has no box to draw the page in")?;
    builder.build_gtk(vbox).map_err(|error| error.to_string())
}

#[cfg(any(windows, target_os = "macos"))]
fn attach(builder: WebViewBuilder, window: &Window) -> Result<WebView, String> {
    builder.build(window).map_err(|error| error.to_string())
}
