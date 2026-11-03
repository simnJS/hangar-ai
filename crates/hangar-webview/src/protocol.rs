//! The messages between Hangar and its webview helper: one JSON object per
//! line, each tagged by `"type"`, in both directions.
//!
//! Lines rather than a length prefix or a binary format, since the helper is
//! debugged by hand more often than it is profiled (`echo '{"type":"quit"}' |
//! hangar-webview`), and a JSON string never holds a raw newline, so a line is
//! always one message. Bodies of requests and responses are the exception to
//! "readable": they are bytes, and travel as base64.
//!
//! A window is named by the id the parent gives it in `open`, and every other
//! message about it carries that id as `window`, so the parent never has to
//! learn an identifier of the engine's.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::base64;

/// What this build of the protocol is. It is announced by the helper in
/// `ready`, and the client refuses a helper that announces another.
///
/// It changes when a message is removed or means something else. A new
/// command, event, field or error code does not change it: unknown fields are
/// ignored on both sides, an event a client does not know reads as
/// [`Event::Unknown`], and a command a helper does not know is answered by an
/// [`ErrorCode::BadCommand`] error.
pub const PROTOCOL_VERSION: u32 = 1;

/// Pixels the way a window system counts them before the screen's scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// What a window shows when it opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    /// A page to load: `https://…`, `file://…`, or a page of one of the
    /// window's [`schemes`](OpenWindow::schemes), whose requests the parent
    /// answers. Use [`crate::scheme_url`] for the last, which is not spelt the
    /// same on every platform.
    Url { url: String },
    /// A document given whole. Its origin is opaque (`null`), so it cannot
    /// load files or call anything by relative URL: give it a scheme instead if
    /// it has to.
    Html { html: String },
}

impl Content {
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url { url: url.into() }
    }

    pub fn html(html: impl Into<String>) -> Self {
        Self::Html { html: html.into() }
    }
}

fn yes() -> bool {
    true
}

/// A window to open: `{"type":"open", "window":"w1", "content":{…}, …}`.
///
/// Only `window` and `content` are required; everything else has the default
/// that is safest for a page nobody has vetted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenWindow {
    /// The parent's name for the window, unique among the windows open now.
    pub window: String,
    #[serde(default)]
    pub title: String,
    /// The window's inner size in logical pixels; the helper picks one when it
    /// is left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    pub content: Content,
    /// Custom URL schemes the window serves through the parent, by name
    /// (`hangar-kit`). Each request to one becomes an [`Event::SchemeRequest`]
    /// that the parent answers with a [`SchemeResponse`], so what a page can
    /// load is decided in the parent and not in the helper.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemes: Vec<String>,
    /// Script run in the page's main frame before any of the page's own, each
    /// time a document loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_script: Option<String>,
    /// Install [`KIT_BRIDGE`](crate::KIT_BRIDGE) before `init_script`, so a
    /// kit's page finds `window.hangar` working as it did in a frame of
    /// Hangar's window.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kit_bridge: bool,
    /// Allow the engine's inspector (a right click opens it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub devtools: bool,
    /// Keep nothing of the page on disk when the window closes: no cookies, no
    /// storage, no cache. On by default, so a page does not leave anything
    /// for the next to find.
    #[serde(default = "yes")]
    pub ephemeral: bool,
    /// The URL prefixes a navigation of this window may go to, `None` for any.
    /// When set, a navigation outside it does not happen and is reported as an
    /// [`Event::NavigationBlocked`]. A kit's page is one page: it gets the
    /// prefix of its own files, and a link that leaves them goes nowhere.
    /// `about:blank` is always allowed. The prefixes of a scheme in `schemes`
    /// are spelt as [`crate::scheme_url`] does and also match the form Windows
    /// gives them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_navigation: Option<Vec<String>>,
    /// The colour shown before the page paints, as `[red, green, blue]`; white
    /// when left out. A dark window should not flash white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<[u8; 3]>,
}

impl OpenWindow {
    pub fn new(window: impl Into<String>, content: Content) -> Self {
        Self {
            window: window.into(),
            title: String::new(),
            size: None,
            content,
            schemes: Vec::new(),
            init_script: None,
            kit_bridge: false,
            devtools: false,
            ephemeral: true,
            allow_navigation: None,
            background: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.size = Some(Size { width, height });
        self
    }

    pub fn scheme(mut self, name: impl Into<String>) -> Self {
        self.schemes.push(name.into());
        self
    }

    pub fn init_script(mut self, script: impl Into<String>) -> Self {
        self.init_script = Some(script.into());
        self
    }

    pub fn kit_bridge(mut self) -> Self {
        self.kit_bridge = true;
        self
    }

    pub fn devtools(mut self) -> Self {
        self.devtools = true;
        self
    }

    pub fn allow_navigation(mut self, prefixes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.allow_navigation = Some(prefixes.into_iter().map(Into::into).collect());
        self
    }

    pub fn background(mut self, red: u8, green: u8, blue: u8) -> Self {
        self.background = Some([red, green, blue]);
        self
    }
}

/// One header of a request or a response. A list of them rather than a map,
/// since HTTP allows a name twice (`Set-Cookie`) and order can matter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self { name: name.into(), value: value.into() }
    }
}

/// The parent's answer to an [`Event::SchemeRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemeResponse {
    /// The `request` of the request this answers.
    pub request: u64,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<Header>,
    /// The body, base64 in JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "base64::serde_bytes")]
    pub body: Vec<u8>,
}

impl SchemeResponse {
    pub fn new(request: u64, status: u16) -> Self {
        Self { request, status, headers: Vec::new(), body: Vec::new() }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header::new(name, value));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// A `200` with `body` and a `Content-Type`.
    pub fn ok(request: u64, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self::new(request, 200).header("Content-Type", content_type).body(body)
    }
}

/// What the parent asks of the helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Open a window. Answered by [`Event::Opened`], or by an [`Event::Error`]
    /// of [`ErrorCode::OpenFailed`] or [`ErrorCode::DuplicateWindow`].
    Open(OpenWindow),
    /// Load a URL in the window. Subject to its `allow_navigation` like any
    /// other navigation.
    Navigate { window: String, url: String },
    /// Run a script in the page. With a `token`, what it evaluates to comes
    /// back as an [`Event::EvalResult`] carrying the same token; without, it
    /// is fire and forget. A script that throws gets no result on some
    /// engines, so a caller that waits for one waits with a time limit.
    Eval {
        window: String,
        script: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
    /// Deliver `message` to the page as a `message` event on its `window`, the
    /// way `postMessage` from a parent frame would. On a page with the kit
    /// bridge, the event's `source` is the bridge's stand-in for
    /// `window.parent`, which is what `kit.js` checks.
    PostMessage { window: String, message: Value },
    /// Set the window's title. The helper never does it by itself, so the title
    /// is whatever the parent last said, whatever the page's `<title>` does.
    SetTitle { window: String, title: String },
    /// Raise the window and give its page the keyboard.
    Focus { window: String },
    /// Close the window. Answered by [`Event::WindowClosed`].
    Close { window: String },
    /// Answer a scheme request.
    SchemeResponse(SchemeResponse),
    /// Close every window and exit. Closing the helper's standard input does
    /// the same, so a helper never outlives the parent that started it.
    Quit,
}

impl Command {
    /// The line to write, without its newline.
    pub fn to_line(&self) -> String {
        // A command is plain data: serializing it cannot fail.
        serde_json::to_string(self).expect("a command serializes")
    }

    pub fn from_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line)
    }
}

/// A request a page made to one of its window's custom schemes, which the
/// parent has to answer with a [`SchemeResponse`] carrying the same `request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemeRequest {
    /// Unique among the requests of one helper process. The helper holds the
    /// engine's responder until the answer comes, so a request nobody answers
    /// is a page that waits.
    pub request: u64,
    pub window: String,
    /// `GET`, `HEAD`, `POST`…
    pub method: String,
    /// The full URL as the engine has it: `hangar-kit://localhost/a/b.html` on
    /// Linux and macOS, `http://hangar-kit.localhost/a/b.html` on Windows.
    pub uri: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    /// The request's body, base64 in JSON, empty for a `GET`.
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "base64::serde_bytes")]
    pub body: Vec<u8>,
}

/// The two moments of a document loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageLoadState {
    Started,
    Finished,
}

/// Why a window is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// The person closed it.
    User,
    /// The parent asked for it with `close`.
    Command,
    /// The helper is exiting.
    Quit,
}

/// What kind of thing went wrong, for a parent to act on without reading the
/// message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// A line of the parent's that is not a command the helper knows.
    BadCommand,
    /// A command for a window that is not open.
    UnknownWindow,
    /// `open` with the id of a window that is open.
    DuplicateWindow,
    /// The engine could not make the window or its webview.
    OpenFailed,
    /// The engine would not run a script.
    EvalFailed,
    /// The engine would not load a URL.
    NavigateFailed,
    /// A scheme response the helper could not turn into one for the engine: a
    /// status that is no HTTP status, or a header with an invalid name.
    BadResponse,
    /// The helper speaks another version of the protocol. Raised by the client,
    /// not sent by the helper.
    ProtocolMismatch,
    /// A line from the helper that is not an event. Raised by the client.
    BadEvent,
    /// Anything else, and any code a newer helper has that this client does not.
    #[serde(other)]
    Other,
}

/// What the helper tells the parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The first line a helper writes, once its window system is up: the
    /// protocol it speaks, the version of the helper and the engine it drives.
    Ready { protocol: u32, version: String, engine: String },
    /// A window is open.
    Opened { window: String },
    /// The page called `window.ipc.postMessage(body)`. The body is the page's
    /// string and nothing more: the page wrote it, so it is untrusted input to
    /// check, as is `uri`, which says which document sent it as far as the
    /// engine knows (some engines say the main frame's current URL).
    Ipc { window: String, uri: String, body: String },
    /// A page asked for something at one of the window's schemes.
    SchemeRequest(SchemeRequest),
    /// A document started or finished loading, its URL as the engine has it.
    PageLoad { window: String, state: PageLoadState, url: String },
    /// The document's `<title>` changed. The window's title is not touched; see
    /// [`Command::SetTitle`].
    TitleChanged { window: String, title: String },
    /// What a script run with a `token` evaluated to, as JSON. A script with
    /// no value gives `null`.
    EvalResult { window: String, token: String, result: Value },
    /// A navigation that `allow_navigation` stopped, or a page asking for a
    /// window of its own (`window.open`, `target=_blank`), which the helper
    /// never grants: the parent decides what to do with the URL.
    NavigationBlocked { window: String, url: String, new_window: bool },
    /// The window is gone.
    WindowClosed { window: String, reason: CloseReason },
    /// Something failed. `window` says which window it concerns, if one.
    Error { window: Option<String>, code: ErrorCode, message: String },
    /// An event this client does not know: a newer helper's. Ignore it.
    #[serde(other)]
    Unknown,
}

impl Event {
    /// The line to write, without its newline.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("an event serializes")
    }

    pub fn from_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line)
    }

    /// An [`Event::Error`] with no window.
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Error { window: None, code, message: message.into() }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn round_trip_command(command: Command) -> String {
        let line = command.to_line();
        assert!(!line.contains('\n'), "a line holds one message: {line}");
        assert_eq!(Command::from_line(&line).unwrap(), command, "{line}");
        line
    }

    fn round_trip_event(event: Event) -> String {
        let line = event.to_line();
        assert!(!line.contains('\n'), "a line holds one message: {line}");
        assert_eq!(Event::from_line(&line).unwrap(), event, "{line}");
        line
    }

    #[test]
    fn every_command_survives_a_round_trip() {
        let open = OpenWindow::new("w1", Content::url("hangar-kit://localhost/k/index.html"))
            .title("Kit")
            .size(900, 600)
            .scheme("hangar-kit")
            .init_script("console.log(1)")
            .kit_bridge()
            .devtools()
            .allow_navigation(["hangar-kit://localhost/k/"])
            .background(18, 18, 18);
        round_trip_command(Command::Open(open));
        round_trip_command(Command::Open(OpenWindow::new("w2", Content::html("<p>a\nb</p>"))));
        round_trip_command(Command::Navigate { window: "w1".into(), url: "https://example.com".into() });
        round_trip_command(Command::Eval { window: "w1".into(), script: "1 + 1".into(), token: None });
        round_trip_command(Command::Eval { window: "w1".into(), script: "1 + 1".into(), token: Some("t".into()) });
        round_trip_command(Command::PostMessage { window: "w1".into(), message: json!({ "hangar": 1, "event": "theme", "data": {} }) });
        round_trip_command(Command::SetTitle { window: "w1".into(), title: "Run #4".into() });
        round_trip_command(Command::Focus { window: "w1".into() });
        round_trip_command(Command::Close { window: "w1".into() });
        round_trip_command(Command::SchemeResponse(
            SchemeResponse::ok(7, "text/html", b"<p>hi</p>".to_vec()).header("X-A", "1").header("X-A", "2"),
        ));
        round_trip_command(Command::Quit);
    }

    #[test]
    fn every_event_survives_a_round_trip() {
        round_trip_event(Event::Ready { protocol: PROTOCOL_VERSION, version: "1.12.1".into(), engine: "wry".into() });
        round_trip_event(Event::Opened { window: "w1".into() });
        round_trip_event(Event::Ipc { window: "w1".into(), uri: "hangar-kit://localhost/k/a.html".into(), body: "{\"a\":1}\n".into() });
        round_trip_event(Event::SchemeRequest(SchemeRequest {
            request: 3,
            window: "w1".into(),
            method: "POST".into(),
            uri: "hangar-kit://localhost/k/a".into(),
            headers: vec![Header::new("accept", "*/*"), Header::new("accept", "text/html")],
            body: vec![0, 1, 2, 254, 255],
        }));
        round_trip_event(Event::PageLoad { window: "w1".into(), state: PageLoadState::Started, url: "about:blank".into() });
        round_trip_event(Event::PageLoad { window: "w1".into(), state: PageLoadState::Finished, url: "about:blank".into() });
        round_trip_event(Event::TitleChanged { window: "w1".into(), title: "é \u{2028} 😀".into() });
        round_trip_event(Event::EvalResult { window: "w1".into(), token: "t".into(), result: json!([1, "a", null]) });
        round_trip_event(Event::NavigationBlocked { window: "w1".into(), url: "https://x.test".into(), new_window: true });
        for reason in [CloseReason::User, CloseReason::Command, CloseReason::Quit] {
            round_trip_event(Event::WindowClosed { window: "w1".into(), reason });
        }
        round_trip_event(Event::Error { window: Some("w1".into()), code: ErrorCode::OpenFailed, message: "no display".into() });
        round_trip_event(Event::error(ErrorCode::BadCommand, "nope"));
    }

    #[test]
    fn the_wire_format_is_the_documented_one() {
        let open = Command::Open(OpenWindow::new("w1", Content::url("https://example.com")).title("Docs"));
        assert_eq!(
            serde_json::from_str::<Value>(&open.to_line()).unwrap(),
            json!({
                "type": "open",
                "window": "w1",
                "title": "Docs",
                "content": { "kind": "url", "url": "https://example.com" },
                "ephemeral": true,
            })
        );
        let response = Command::SchemeResponse(SchemeResponse::ok(9, "text/plain", b"hi".to_vec()));
        assert_eq!(
            serde_json::from_str::<Value>(&response.to_line()).unwrap(),
            json!({
                "type": "scheme_response",
                "request": 9,
                "status": 200,
                "headers": [{ "name": "Content-Type", "value": "text/plain" }],
                "body": "aGk=",
            })
        );
        let request = Event::SchemeRequest(SchemeRequest {
            request: 1,
            window: "w".into(),
            method: "GET".into(),
            uri: "s://localhost/".into(),
            headers: vec![],
            body: vec![],
        });
        assert_eq!(
            serde_json::from_str::<Value>(&request.to_line()).unwrap(),
            json!({ "type": "scheme_request", "request": 1, "window": "w", "method": "GET", "uri": "s://localhost/", "headers": [] })
        );
    }

    #[test]
    fn bodies_are_base64_and_keep_every_byte() {
        let bytes: Vec<u8> = (0..=255).chain(0..=255).collect();
        let line = Event::SchemeRequest(SchemeRequest {
            request: 1,
            window: "w".into(),
            method: "POST".into(),
            uri: "s://localhost/upload".into(),
            headers: vec![],
            body: bytes.clone(),
        })
        .to_line();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["body"].as_str().unwrap(), base64::encode(&bytes));
        let Event::SchemeRequest(back) = Event::from_line(&line).unwrap() else { panic!("not a request") };
        assert_eq!(back.body, bytes);

        let bad = r#"{"type":"scheme_response","request":1,"status":200,"body":"not base64!"}"#;
        let error = Command::from_line(bad).unwrap_err().to_string();
        assert!(error.contains("base64"), "{error}");
    }

    #[test]
    fn minimal_commands_get_safe_defaults() {
        let Command::Open(open) = Command::from_line(
            r#"{"type":"open","window":"w","content":{"kind":"html","html":"<p>"}}"#,
        )
        .unwrap() else {
            panic!("not an open")
        };
        assert!(open.ephemeral, "a page stores nothing unless asked");
        assert!(!open.devtools && !open.kit_bridge);
        assert_eq!(open.allow_navigation, None);
        assert!(open.schemes.is_empty() && open.title.is_empty() && open.size.is_none());
        assert_eq!(open, OpenWindow::new("w", Content::html("<p>")));
    }

    #[test]
    fn versions_unknown_fields_and_unknown_types_are_handled() {
        // A newer helper's `ready` reads, and says it is not ours.
        let Event::Ready { protocol, .. } =
            Event::from_line(r#"{"type":"ready","protocol":2,"version":"9","engine":"x","extra":true}"#).unwrap()
        else {
            panic!("not a ready")
        };
        assert_ne!(protocol, PROTOCOL_VERSION);

        // An event it has and we do not is skipped, not a failure.
        assert_eq!(Event::from_line(r#"{"type":"zoom_changed","window":"w","level":2}"#).unwrap(), Event::Unknown);
        // An error code it has and we do not folds into `other`.
        assert_eq!(
            Event::from_line(r#"{"type":"error","window":null,"code":"disk_full","message":"m"}"#).unwrap(),
            Event::Error { window: None, code: ErrorCode::Other, message: "m".into() }
        );

        // A command the helper does not know, or half of one, is an error for it to report.
        assert!(Command::from_line(r#"{"type":"teleport","window":"w"}"#).is_err());
        assert!(Command::from_line(r#"{"type":"navigate","window":"w"}"#).is_err());
        assert!(Command::from_line("not json").is_err());
    }
}
