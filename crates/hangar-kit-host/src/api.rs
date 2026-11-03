//! What the app gives a tab: the answers to its calls. This crate checks a
//! call — permission, params, limits — and hands the app a [`HostCall`]
//! that only needs doing; the app reaches hangar-core, the panes and the
//! window, and this crate never does.

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use hangar_kit_ui::ImageSource;
use hangar_kit_ui::api::{self, BadgeState, KitError};

/// A call's answer, as the app makes it: the JSON result of the method
/// (docs/kits.md, "The tab message protocol"), or its error.
pub type CallFuture = Pin<Box<dyn Future<Output = Result<Value, KitError>> + Send + 'static>>;

/// The app's side of one tab. One per open tab: it knows which workspace,
/// which kit and which tab it answers for, from its own record, never from
/// what the kit says.
pub trait KitApi: Send + Sync + 'static {
    /// What this tab may call now: the permissions its manifest declares
    /// that the person granted the kit on this workspace. Read again at
    /// every call, so taking a grant back denies the next one.
    fn granted(&self) -> Vec<String>;

    /// What only the app can refuse of a call whose params are right, asked
    /// before the call counts against its rate: the workspace is still there,
    /// the pane `panes.send` names is one of its Claude Code agents with the
    /// bridge. `src/lib/kitApi.ts` checked those before it took from the
    /// limit, so a page that keeps naming the wrong pane never spends the
    /// sends of the right one. Nothing by default.
    fn check(&self, call: &HostCall) -> Result<(), KitError> {
        let _ = call;
        Ok(())
    }

    /// Does a call already let through. Its params are checked as
    /// `src/lib/kitApi.ts` checks them; what only the app can check (the
    /// pane exists, the path stays inside on disk, the command's rules)
    /// is its to do, answering the protocol's error codes.
    fn call(&self, call: HostCall) -> CallFuture;

    /// The bytes of an image a view shows: from the kit's own folder, or
    /// from its data folder, which this crate lets through only for a tab
    /// holding `data.read`. Read once per view that brings a new one, from
    /// the window's thread: keep it to a file read. Refused by default.
    fn image(&self, source: &ImageSource) -> Result<Vec<u8>, KitError> {
        let _ = source;
        Err(KitError::new(
            KitError::UNSUPPORTED,
            "this Hangar shows no image of a kit",
        ))
    }
}

/// A `loopback.fetch`, checked: nothing else of the kit's reaches the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackCall {
    pub port: u16,
    /// Starts with one `/`, URL-encoded already, query included.
    pub path: String,
    pub method: api::HttpMethod,
    /// On a `POST` only.
    pub body: Option<String>,
    pub timeout_ms: u32,
}

/// A call let through: its permission held, its params of the right shape
/// and within their limits, under its rate. Paths are relative, with `/`,
/// already held to the rules of `relativePath` in src/lib/kitApi.ts.
#[derive(Debug, Clone, PartialEq)]
pub enum HostCall {
    /// `{ id, name, folder }`.
    WorkspaceInfo,
    /// `[{ name, agent, role, state, canSend }]`.
    PanesList,
    /// `{ queued: true, pane }`; `not_found`, `denied` (not a Claude Code
    /// agent) or `unavailable` (no bridge) as the app finds the pane.
    PanesSend { pane: String, text: String },
    /// The file's text. `hidden`: the tab holds `files.write` too, so a
    /// hidden file is read as well (`kit_files_read`, not `kit_file_read`).
    FilesRead { path: String, hidden: bool },
    /// `{ base64, mime }`.
    FilesReadBinary { path: String, hidden: bool },
    /// `{ files, truncated }`; no `path` is the workspace folder.
    FilesList {
        path: Option<String>,
        pattern: Option<String>,
    },
    /// `{}`; `conflict` when `expected` is not what the file holds.
    FilesWrite {
        path: String,
        text: String,
        expected: Option<String>,
    },
    /// `{}`; `conflict` when anything is at the path.
    FilesWriteBinary { path: String, base64: String },
    /// `{ text }`, or `not_found`.
    DataRead { path: String },
    /// `{}`.
    DataWrite { path: String, text: String },
    /// `{ status, text }`, or `unreachable`.
    LoopbackFetch(LoopbackCall),
    /// `{ exitCode, stdout, stderr, timedOut, truncated }`. The kit, the
    /// workspace folder and the commands' acceptance are the app's record.
    CommandRun {
        name: String,
        args: std::collections::BTreeMap<String, String>,
    },
    /// `{ opened: true, path }`.
    PreviewShow { path: String },
    /// `null`. The app titles it `<kit name> · <title>`.
    Notify { title: String, body: String },
    /// `{ params, caller }`.
    TabParams,
    /// `{}`: trimmed, one line, 40 characters at most.
    TabSetTitle { title: String },
    /// `{}`; none takes the badge away.
    TabSetBadge {
        badge: Option<(BadgeState, Option<String>)>,
    },
    /// `{ action }` once it goes: whether its action was pressed.
    Toast(api::Toast),
    /// A boolean: whether the person confirmed. One at a time.
    Confirm(api::Confirm),
    /// `{}`. Only from an update that answers a person's click or keystroke.
    Copy { text: String },
}

impl HostCall {
    /// The method it answers, as the protocol names it.
    pub fn method(&self) -> &'static str {
        use api::method::*;
        match self {
            HostCall::WorkspaceInfo => WORKSPACE_INFO,
            HostCall::PanesList => PANES_LIST,
            HostCall::PanesSend { .. } => PANES_SEND,
            HostCall::FilesRead { .. } => FILES_READ,
            HostCall::FilesReadBinary { .. } => FILES_READ_BINARY,
            HostCall::FilesList { .. } => FILES_LIST,
            HostCall::FilesWrite { .. } => FILES_WRITE,
            HostCall::FilesWriteBinary { .. } => FILES_WRITE_BINARY,
            HostCall::DataRead { .. } => DATA_READ,
            HostCall::DataWrite { .. } => DATA_WRITE,
            HostCall::LoopbackFetch(_) => LOOPBACK_FETCH,
            HostCall::CommandRun { .. } => COMMAND_RUN,
            HostCall::PreviewShow { .. } => PREVIEW_SHOW,
            HostCall::Notify { .. } => NOTIFY,
            HostCall::TabParams => TAB_PARAMS,
            HostCall::TabSetTitle { .. } => TAB_SET_TITLE,
            HostCall::TabSetBadge { .. } => TAB_SET_BADGE,
            HostCall::Toast(_) => UI_TOAST,
            HostCall::Confirm(_) => UI_CONFIRM,
            HostCall::Copy { .. } => UI_COPY,
        }
    }
}

/// A future already answered: a refusal, or a fake's answer in a test.
pub fn ready(answer: Result<Value, KitError>) -> CallFuture {
    Box::pin(std::future::ready(answer))
}
