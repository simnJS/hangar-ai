//! The host API a native tab calls: the methods of the tab message protocol
//! of HTML tabs (docs/kits.md, "The tab message protocol"), with the same
//! names, params, results, errors and permissions, and the `ui.*` methods
//! that stand for what an HTML page had in `hangar.ui` (a toast, a
//! confirmation, the clipboard).
//!
//! The SDK sends these as [`Call`](crate::abi::Call)s; Hangar checks each one
//! against what the tab holds before anything else (hangar-kit-host). The
//! typed requests below are the same calls, for Rust: `R::METHOD` and `R`'s
//! JSON are what goes over, and `R::Output` is what comes back.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::style::Notice;

/// The names of the methods.
pub mod method {
    pub const WORKSPACE_INFO: &str = "workspace.info";
    pub const PANES_LIST: &str = "panes.list";
    pub const PANES_SEND: &str = "panes.send";
    pub const FILES_READ: &str = "files.read";
    pub const FILES_READ_BINARY: &str = "files.readBinary";
    pub const FILES_LIST: &str = "files.list";
    pub const FILES_WRITE: &str = "files.write";
    pub const FILES_WRITE_BINARY: &str = "files.writeBinary";
    pub const DATA_READ: &str = "data.read";
    pub const DATA_WRITE: &str = "data.write";
    pub const LOOPBACK_FETCH: &str = "loopback.fetch";
    pub const COMMAND_RUN: &str = "command.run";
    pub const PREVIEW_SHOW: &str = "preview.show";
    pub const NOTIFY: &str = "notify";
    pub const TAB_PARAMS: &str = "tab.params";
    pub const TAB_SET_TITLE: &str = "tab.setTitle";
    pub const TAB_SET_BADGE: &str = "tab.setBadge";
    pub const UI_TOAST: &str = "ui.toast";
    pub const UI_CONFIRM: &str = "ui.confirm";
    pub const UI_COPY: &str = "ui.copy";
    /// Native tabs only, as the three `ui.*` above: what a page has in its
    /// own document (a file input, a paste, a canvas's `toBlob`).
    pub const FILES_PICK: &str = "files.pick";
    pub const UI_PASTE_IMAGE: &str = "ui.pasteImage";
    pub const CANVAS_EXPORT: &str = "canvas.export";
    pub const IMAGE_SIZE: &str = "image.size";
}

/// The permissions a manifest may ask for (docs/kits.md, "Tab permissions").
pub mod permission {
    pub const WORKSPACE_READ: &str = "workspace.read";
    pub const PANES_READ: &str = "panes.read";
    pub const PANES_SEND: &str = "panes.send";
    pub const FILES_READ: &str = "files.read";
    pub const FILES_WRITE: &str = "files.write";
    pub const DATA_READ: &str = "data.read";
    pub const DATA_WRITE: &str = "data.write";
    pub const PREVIEW_SHOW: &str = "preview.show";
    pub const NOTIFY: &str = "notify";
    /// `loopback:<port>`, the port written plainly, 1024 to 65535.
    pub const LOOPBACK_PREFIX: &str = "loopback:";
    /// `command:<name>`, a command the kit declares.
    pub const COMMAND_PREFIX: &str = "command:";
}

/// What a method needs the tab to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// Nothing: it is about the tab itself (`tab.*`, `ui.*`).
    Nothing,
    /// This permission.
    Permission(&'static str),
    /// `loopback:<port>` for the `port` of its params.
    Loopback,
    /// `command:<name>` for the `name` of its params.
    Command,
}

/// Every method, and what it needs: `KIT_METHODS` of src/lib/kits.ts, and the
/// tab's own.
pub const METHODS: &[(&str, Needs)] = &[
    (
        method::WORKSPACE_INFO,
        Needs::Permission(permission::WORKSPACE_READ),
    ),
    (
        method::PANES_LIST,
        Needs::Permission(permission::PANES_READ),
    ),
    (
        method::PANES_SEND,
        Needs::Permission(permission::PANES_SEND),
    ),
    (
        method::FILES_READ,
        Needs::Permission(permission::FILES_READ),
    ),
    (
        method::FILES_READ_BINARY,
        Needs::Permission(permission::FILES_READ),
    ),
    (
        method::FILES_LIST,
        Needs::Permission(permission::FILES_READ),
    ),
    (
        method::FILES_WRITE,
        Needs::Permission(permission::FILES_WRITE),
    ),
    (
        method::FILES_WRITE_BINARY,
        Needs::Permission(permission::FILES_WRITE),
    ),
    (method::DATA_READ, Needs::Permission(permission::DATA_READ)),
    (
        method::DATA_WRITE,
        Needs::Permission(permission::DATA_WRITE),
    ),
    (method::LOOPBACK_FETCH, Needs::Loopback),
    (method::COMMAND_RUN, Needs::Command),
    (
        method::PREVIEW_SHOW,
        Needs::Permission(permission::PREVIEW_SHOW),
    ),
    (method::NOTIFY, Needs::Permission(permission::NOTIFY)),
    (method::TAB_PARAMS, Needs::Nothing),
    (method::TAB_SET_TITLE, Needs::Nothing),
    (method::TAB_SET_BADGE, Needs::Nothing),
    (method::UI_TOAST, Needs::Nothing),
    (method::UI_CONFIRM, Needs::Nothing),
    (method::UI_COPY, Needs::Nothing),
    (
        method::FILES_PICK,
        Needs::Permission(permission::FILES_READ),
    ),
    (method::UI_PASTE_IMAGE, Needs::Nothing),
    (
        method::CANVAS_EXPORT,
        Needs::Permission(permission::FILES_WRITE),
    ),
    // What it needs is its source's: held to it with its params.
    (method::IMAGE_SIZE, Needs::Nothing),
];

/// The methods a native tab has and an HTML tab does not: a page has these
/// in its own document, and is answered `unknown` for them.
pub const NATIVE_ONLY: &[&str] = &[
    method::UI_TOAST,
    method::UI_CONFIRM,
    method::UI_COPY,
    method::FILES_PICK,
    method::UI_PASTE_IMAGE,
    method::CANVAS_EXPORT,
    method::IMAGE_SIZE,
];

/// What `method` needs; none for a method there is not.
pub fn needs(method: &str) -> Option<Needs> {
    METHODS
        .iter()
        .find(|(name, _)| *name == method)
        .map(|(_, needs)| *needs)
}

/// A port a tab may name: 1024 to 65535.
pub fn is_loopback_port(port: u64) -> bool {
    (1024..=65535).contains(&port)
}

/// The port of a `loopback:<port>` permission written plainly (no sign, no
/// leading zero); none for anything else. `kitLoopbackPort` in kits.ts.
pub fn loopback_port(permission: &str) -> Option<u16> {
    let digits = permission.strip_prefix(permission::LOOPBACK_PREFIX)?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port: u64 = digits.parse().ok()?;
    is_loopback_port(port).then_some(port as u16)
}

/// How a declared command is named: `[a-z][a-z0-9-]*`.
pub fn is_command_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The name in a `command:<name>` permission; none for anything else.
pub fn command_name(permission: &str) -> Option<&str> {
    permission
        .strip_prefix(permission::COMMAND_PREFIX)
        .filter(|name| is_command_name(name))
}

/// Whether a tab holding `granted` may call `method` with `params`: what
/// `kitMayCall` in kits.ts decides, the tab's own methods aside.
pub fn may_call(method: &str, params: &Value, granted: &[String]) -> bool {
    match needs(method) {
        None => false,
        Some(Needs::Nothing) => true,
        Some(Needs::Permission(permission)) => granted.iter().any(|held| held == permission),
        Some(Needs::Loopback) => match params.get("port").and_then(Value::as_u64) {
            Some(port) if is_loopback_port(port) => granted
                .iter()
                .any(|held| loopback_port(held) == Some(port as u16)),
            _ => false,
        },
        Some(Needs::Command) => match params.get("name").and_then(Value::as_str) {
            Some(name) if is_command_name(name) => {
                granted.iter().any(|held| command_name(held) == Some(name))
            }
            _ => false,
        },
    }
}

/// Whether a tab holding `granted` reads hidden files: both `files.read` and
/// `files.write` on the same tab (docs/kits.md, "Hidden files and secrets").
pub fn reads_hidden_files(granted: &[String]) -> bool {
    granted.iter().any(|held| held == permission::FILES_READ)
        && granted.iter().any(|held| held == permission::FILES_WRITE)
}

/// Hangar's events, what each carries, and what each needs.
pub mod event {
    /// The workspace's panes changed: one added, closed, renamed, or
    /// changing state. `data` is `{}`: call `panes.list`. Needs `panes.read`.
    pub const PANES_CHANGED: &str = "panes.changed";
    /// A singleton got new params, or an agent or a hook asked for this tab
    /// again. `data` is a [`TabParams`](super::TabParamsAnswer). Needs nothing.
    pub const TAB_PARAMS: &str = "tab.params";
    /// The workspace's theme or Hangar's language changed. `data` is a
    /// [`Theme`](super::Theme). Needs nothing.
    pub const THEME: &str = "theme";
    /// The tab came to the front, or went behind another. `data` is a
    /// [`Visibility`](super::Visibility). Needs nothing.
    pub const TAB_VISIBILITY: &str = "tab.visibility";
}

/// What hearing `event` needs; none for an event there is not.
pub fn event_needs(name: &str) -> Option<Needs> {
    match name {
        event::PANES_CHANGED => Some(Needs::Permission(permission::PANES_READ)),
        event::TAB_PARAMS | event::THEME | event::TAB_VISIBILITY => Some(Needs::Nothing),
        _ => None,
    }
}

/// Whether a tab holding `granted` hears `event`.
pub fn may_hear(name: &str, granted: &[String]) -> bool {
    match event_needs(name) {
        Some(Needs::Nothing) => true,
        Some(Needs::Permission(permission)) => granted.iter().any(|held| held == permission),
        _ => false,
    }
}

/// The limits of docs/kits.md and src/lib/kitApi.ts, which a native tab is
/// held to as an HTML one is.
pub mod limits {
    /// Calls of a tab waiting for their answer, at most.
    pub const MAX_PENDING: usize = 16;
    /// `panes.send`'s longest text, in characters.
    pub const SEND_MAX_CHARS: usize = 20_000;
    /// `panes.send`, `data.write`, and `files.write` with `files.writeBinary`,
    /// each at most this many a minute.
    pub const WRITES_PER_MINUTE: usize = 10;
    /// `notify`, at most this many a minute.
    pub const NOTIFY_PER_MINUTE: usize = 5;
    /// `tab.setTitle` and `tab.setBadge`, each at most this many a minute.
    pub const TAB_CHANGES_PER_MINUTE: usize = 120;
    /// `ui.toast`, at most this many a minute.
    pub const TOASTS_PER_MINUTE: usize = 30;
    pub const NOTIFY_TITLE_MAX_CHARS: usize = 120;
    pub const NOTIFY_BODY_MAX_CHARS: usize = 1000;
    pub const TAB_TITLE_MAX_CHARS: usize = 40;
    pub const TAB_BADGE_TEXT_MAX_CHARS: usize = 12;
    pub const PATH_MAX_CHARS: usize = 1024;
    pub const FILE_MAX_BYTES: usize = 1024 * 1024;
    pub const BINARY_MAX_BYTES: usize = 10 * 1024 * 1024;
    pub const DATA_MAX_BYTES: usize = 1024 * 1024;
    pub const LIST_PATTERN_MAX_CHARS: usize = 256;
    pub const LOOPBACK_PATH_MAX_CHARS: usize = 2048;
    pub const LOOPBACK_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
    pub const LOOPBACK_TIMEOUT_MS: u32 = 5_000;
    pub const LOOPBACK_MAX_TIMEOUT_MS: u32 = 15_000;
    /// `ui.toast`'s and `ui.confirm`'s longest message, in characters.
    pub const UI_MESSAGE_MAX_CHARS: usize = 500;
    /// `ui.copy`'s longest text, in bytes.
    pub const COPY_MAX_BYTES: usize = 1024 * 1024;
    /// The pictures a tab holds at once (`files.pick` outside the
    /// workspace, `ui.pasteImage`); the oldest goes for a new one.
    pub const HELD_IMAGES: usize = 8;
    /// `canvas.export`: the longest side of a canvas, in its units, and so
    /// of the picture written at its own size.
    pub const CANVAS_MAX_SIDE: f32 = 8192.0;
    /// `canvas.export`: the most marks, and points of all its paths.
    pub const CANVAS_MAX_MARKS: usize = 5_000;
    pub const CANVAS_MAX_POINTS: usize = 200_000;
    /// `canvas.export`: the longest text of a mark, in characters.
    pub const CANVAS_TEXT_MAX_CHARS: usize = 500;
    /// `files.pick`: the most extensions a filter names.
    pub const PICK_MAX_EXTENSIONS: usize = 20;
}

/// A call refused or failed: the protocol's `{ code, message }`. A `denied`
/// or a `not_found` is an ordinary state of a tab, to draw, not to panic on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitError {
    pub code: String,
    pub message: String,
}

impl KitError {
    /// The tab may not call it: a permission, a port or a command it does
    /// not hold.
    pub const DENIED: &str = "denied";
    /// The params are not what the method takes.
    pub const INVALID: &str = "invalid";
    /// No such method.
    pub const UNKNOWN: &str = "unknown";
    pub const NOT_FOUND: &str = "not_found";
    pub const CONFLICT: &str = "conflict";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const RATE_LIMITED: &str = "rate_limited";
    pub const UNAVAILABLE: &str = "unavailable";
    pub const UNREACHABLE: &str = "unreachable";
    pub const BAD_ARGS: &str = "bad_args";
    pub const BUSY: &str = "busy";
    pub const FAILED: &str = "failed";

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        KitError {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn is(&self, code: &str) -> bool {
        self.code == code
    }
}

impl fmt::Display for KitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for KitError {}

/// A typed call: its method, its params as `Self`, and what it answers.
pub trait Request: Serialize {
    const METHOD: &'static str;
    type Output: DeserializeOwned;
}

macro_rules! request {
    ($ty:ident => $method:expr, $output:ty) => {
        impl Request for $ty {
            const METHOD: &'static str = $method;
            type Output = $output;
        }
    };
}

// ---------------------------------------------------------------- Requests.

/// `workspace.info`, needs `workspace.read`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInfo {}
request!(WorkspaceInfo => method::WORKSPACE_INFO, Workspace);

/// `panes.list`, needs `panes.read`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PanesList {}
request!(PanesList => method::PANES_LIST, Vec<Pane>);

/// `panes.send`, needs `panes.send`: `text` waits for that pane's agent to
/// be free, labelled as coming from the kit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanesSend {
    pub pane: String,
    pub text: String,
}
request!(PanesSend => method::PANES_SEND, Queued);

/// `files.read`, needs `files.read`: a UTF-8 text file of the workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesRead {
    pub path: String,
}
request!(FilesRead => method::FILES_READ, String);

/// `files.readBinary`, needs `files.read`: an image or a PDF, as base64.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesReadBinary {
    pub path: String,
}
request!(FilesReadBinary => method::FILES_READ_BINARY, BinaryFile);

/// `files.list`, needs `files.read`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FilesList {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}
request!(FilesList => method::FILES_LIST, FileListing);

/// `files.write`, needs `files.write`. With `expected`, refused as
/// `conflict` when the file holds anything else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesWrite {
    pub path: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
}
request!(FilesWrite => method::FILES_WRITE, Done);

/// `files.writeBinary`, needs `files.write`: a new raster image, never over
/// a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesWriteBinary {
    pub path: String,
    pub base64: String,
}
request!(FilesWriteBinary => method::FILES_WRITE_BINARY, Done);

/// `data.read`, needs `data.read`: a text of the kit's data folder;
/// `not_found` when absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataRead {
    pub path: String,
}
request!(DataRead => method::DATA_READ, DataText);

/// `data.write`, needs `data.write`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataWrite {
    pub path: String,
    pub text: String,
}
request!(DataWrite => method::DATA_WRITE, Done);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
}

/// `loopback.fetch`, needs `loopback:<port>` for that port: made by Hangar,
/// to `127.0.0.1` only. `unreachable` when nothing answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopbackFetch {
    pub port: u16,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<HttpMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
}
request!(LoopbackFetch => method::LOOPBACK_FETCH, LoopbackAnswer);

/// `command.run`, needs `command:<name>` for that name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandRun {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, String>,
}
request!(CommandRun => method::COMMAND_RUN, CommandAnswer);

/// `preview.show`, needs `preview.show`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreviewShow {
    pub path: String,
}
request!(PreviewShow => method::PREVIEW_SHOW, Shown);

/// `notify`, needs `notify`: titled `<kit name> · <title>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notify {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}
request!(Notify => method::NOTIFY, ());

/// `tab.params`: what this tab holds, and who asked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabParams {}
request!(TabParams => method::TAB_PARAMS, TabParamsAnswer);

/// `tab.setTitle`: its name in the tab strip, plain text on one line, 40
/// characters at most.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSetTitle {
    pub title: String,
}
request!(TabSetTitle => method::TAB_SET_TITLE, Done);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BadgeState {
    Running,
    Success,
    Failure,
    Neutral,
}

/// `tab.setBadge`: its state in the tab strip; `state: None` takes it away.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSetBadge {
    pub state: Option<BadgeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}
request!(TabSetBadge => method::TAB_SET_BADGE, Done);

/// `ui.toast`: a line at the foot of the tab for what just happened
/// (`hangar.ui.toast`). It goes after 4 seconds, an error stays until
/// closed. Answered when it goes: whether its action was pressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Toast {
    pub message: String,
    #[serde(default)]
    pub kind: Notice,
    /// The label of its one action, `Undo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}
request!(Toast => method::UI_TOAST, ToastClosed);

/// `ui.confirm`: a question in place of the tab's page, Cancel first and
/// holding the focus (`hangar.ui.confirm`). Answers whether it was
/// confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confirm {
    pub message: String,
    /// The confirming button's label; `Confirm` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<String>,
    /// The answer drawn as a danger button; `true` by default.
    #[serde(default = "danger_by_default")]
    pub danger: bool,
}
request!(Confirm => method::UI_CONFIRM, bool);

fn danger_by_default() -> bool {
    true
}

/// `ui.copy`: puts a text on the clipboard. Only from an update answering a
/// person's click or keystroke: a tab may not fill the clipboard by itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CopyText {
    pub text: String,
}
request!(CopyText => method::UI_COPY, Done);

/// `files.pick`, needs `files.read`, from a click or a keystroke only: the
/// system's file chooser, opened in the workspace folder. Answers `null`
/// when the person cancels, else a [`Picked`]: a file of the workspace by its
/// `path`, which `files.read` and `{ "file": … }` read; one outside it, held
/// by the tab as a picture (`held`), which `{ "held": … }` shows and
/// `canvas.export` draws on, up to 10 MB.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesPick {
    /// The chooser's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The extensions offered, without their dot: `["png", "jpg"]`. None,
    /// every file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
}
request!(FilesPick => method::FILES_PICK, Option<Picked>);

/// `ui.pasteImage`, from a click or a keystroke only: the picture on the
/// clipboard, held by the tab (`{ "held": … }`) as a PNG. `not_found` when
/// the clipboard holds none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PasteImage {}
request!(PasteImage => method::UI_PASTE_IMAGE, HeldImage);

/// `canvas.export`, needs `files.write`: a canvas drawn as a PNG, written as
/// a new file of the workspace (never over one: `conflict`), counted as a
/// `files.write`. Drawn at the canvas's own size; one whose PNG would be
/// over 10 MB is drawn smaller, at three quarters, a half, 35 %, a quarter,
/// and `scale` says which. The background is read under its own permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasExport {
    /// Ends in `.png`.
    pub path: String,
    pub width: f32,
    pub height: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<crate::ImageSource>,
    #[serde(default)]
    pub marks: Vec<crate::Mark>,
    /// The width of a mark without its own, as on the canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<f32>,
}
request!(CanvasExport => method::CANVAS_EXPORT, Exported);

impl CanvasExport {
    /// The export of what `canvas` shows, to `path`.
    pub fn of<H>(canvas: &crate::Canvas<H>, path: impl Into<String>) -> Self {
        CanvasExport {
            path: path.into(),
            width: canvas.width,
            height: canvas.height,
            background: canvas.background.clone(),
            marks: canvas.marks.clone(),
            stroke: canvas.stroke,
        }
    }
}

/// `image.size`: the size in pixels of a picture a view may show, read from
/// its first bytes, under its source's own permission (`data.read` for the
/// data folder, `files.read` for the workspace): what a canvas over a
/// capture is as large as, without reading the capture into the module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageSize {
    pub source: crate::ImageSource,
}
request!(ImageSize => method::IMAGE_SIZE, ImageInfo);

// ---------------------------------------------------------------- Results.

/// A picture's size, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
}

/// A file the person chose: inside the workspace, its `path`; outside it,
/// the picture the tab now holds (`held`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picked {
    /// Its name, without its folder.
    pub name: String,
    /// In bytes.
    pub size: u64,
    /// Relative to the workspace folder, with `/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
}

/// A picture the tab holds, and its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldImage {
    pub held: String,
    pub width: u32,
    pub height: u32,
}

/// What `canvas.export` wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: String,
    /// In pixels.
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// 1 at the canvas's own size.
    pub scale: f32,
}

/// What a call that answers nothing answers: `{}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Done {}

/// The workspace the tab is shown in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// Its absolute path.
    pub folder: String,
}

/// A pane, as `panes.list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    /// What `panes.send` takes.
    pub name: String,
    /// `claude` (Claude Code) or `shell`.
    pub agent: String,
    /// `orchestrator`, `worker` or `manager`.
    #[serde(default)]
    pub role: Option<String>,
    /// `working`, `waiting`, `yours` or `idle`; none when Hangar cannot tell.
    #[serde(default)]
    pub state: Option<String>,
    /// Whether `panes.send` reaches it now.
    pub can_send: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    pub queued: bool,
    pub pane: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryFile {
    pub base64: String,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileListing {
    pub files: Vec<FileEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative to the workspace folder, with `/`.
    pub path: String,
    /// In bytes.
    pub size: u64,
    /// In milliseconds since the Unix epoch.
    pub modified: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataText {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopbackAnswer {
    pub status: u16,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandAnswer {
    /// None when it did not end by itself.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    pub opened: bool,
    pub path: String,
}

/// Who asked for a tab: the pane of the agent or the hook, from Hangar's own
/// record of the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Caller {
    pub pane: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TabParamsAnswer {
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub caller: Option<Caller>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToastClosed {
    /// Its action was pressed.
    pub action: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scheme {
    #[default]
    Dark,
    Light,
}

/// The theme a tab is drawn in, as far as a kit needs it: Hangar draws the
/// widgets, so there are no tokens to apply, only a scheme for a kit's own
/// images and a language for its strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub scheme: Scheme,
    /// `en` or `fr`.
    pub locale: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            scheme: Scheme::Dark,
            locale: "en".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Visibility {
    pub visible: bool,
}
