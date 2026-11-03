//! The port the control bus and the engines (P18–P20) reach the app through:
//! `ControlDeps` of control.ts, one to one, iced-free and `Send + Sync`
//! (inventory A6, WP0).
//!
//! An operation is written once against `Port` and run two ways: in the app
//! over the live `Services` (live.rs), and in the tests over a fake that logs
//! what was done the way the TS fakes did (`"open:w2"`, `"touch:w1"`), so the
//! vitest suites port one to one.
//!
//! The port is a trait object rather than a generic, against A6's sketch: the
//! packages hand each other handlers through it (`Hooks`), which needs
//! `dyn`, and boxing one future per call costs nothing beside the IPC an
//! operation answers.
//!
//! # For P19 and P20
//!
//! - Read the model with `port.snapshot()`; write it with
//!   `port.apply(ModelCmd::…).await`, which answers once the UI thread has
//!   applied it and republished the snapshot.
//! - Install your handlers once hydrated: send
//!   `Message::Control(control::Msg::Install(Hook::…))` through the inbox.
//!   Until then the bus answers your operations with the TS's own "not in
//!   this window" refusals.
//! - P20 runs the person's pane operations with `control::run_as_person`,
//!   lands with `control::delivery::land_delivery`, and reads a delivery's
//!   state with `control::delivery::delivery_state`.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};

use hangar_core::accounts::Account;
use hangar_core::board::{Board, Task, TaskPatch};
use hangar_core::delivery::{Cleanup, Delivery, Outcome, Plan, Running};
use hangar_core::git::Worktree;
use hangar_core::kits::KitInfo;
use hangar_core::shells::ShellInfo;

pub use crate::services::preview::{BoxFuture, SnapshotAngle};
use crate::message::GitRequest;
use crate::state::{
    AgentId, PaneActivity, PaneId, PanePatch, Settings, Snapshot, SplitDir, StoreOutcome,
    WorkspaceDraft, WorkspacePatch, Workspace, WsId,
};

use super::types::{Caller, ControlEvent, OrchestratorEvent};

/// The port, shared: what every operation takes.
pub type Port = Arc<dyn AppPort>;

/// What a model write answers: the ids it made (the new pane's, the new
/// workspace's). A command that is not a store action answers the default.
pub type CmdOut = StoreOutcome;

/// A write to the model, carried out on the UI thread
/// (`Message::Control(control::Msg::Model(cmd, answer))`, inventory A6).
/// Engines never touch `Model`: this is how they change it.
#[derive(Debug, Clone)]
pub enum ModelCmd {
    /// A pane, answered with its id (`None` at `MAX_PANES`). `init` is
    /// applied before anything spawns it, so a worker's shell starts with its
    /// role, task, model and port base (`HANGAR_PORT_BASE`).
    AddPane {
        ws: WsId,
        agent: Option<AgentId>,
        near: Option<PaneId>,
        dir: Option<SplitDir>,
        cwd: Option<String>,
        name: Option<String>,
        init: Option<PanePatch>,
    },
    ClosePane {
        ws: WsId,
        pane: PaneId,
    },
    /// Restarts the pane under a new id, which it answers; the focus follows
    /// it, and a pane restarted while its workspace slept keeps the session
    /// it was owed unless the patch clears `session_id`.
    RespawnPane {
        ws: WsId,
        pane: PaneId,
        patch: PanePatch,
        /// The patch sets `session_id` to `None`: a fresh start.
        fresh: bool,
    },
    /// A patch in place: done before its terminal mounts, the pane starts
    /// with it.
    UpdatePane {
        ws: WsId,
        pane: PaneId,
        patch: PanePatch,
    },
    UpdateWorkspace {
        ws: WsId,
        patch: WorkspacePatch,
    },
    /// Mounts a workspace's terminals without switching to it.
    OpenWorkspace(WsId),
    /// An agent acting on a workspace is using it: its sleep waits.
    Touch(WsId),
    /// Switches the window to the workspace and focuses the pane.
    Activate {
        ws: WsId,
        pane: PaneId,
    },
    /// Moves a workspace to a window of its own.
    Detach(WsId),
    /// Brings it back into the main window.
    Reattach(WsId),
    /// Says on screen what an agent did.
    Announce(ControlEvent),
    /// A workspace, answered with its id; `activate` false leaves the person
    /// where they were.
    CreateWorkspace {
        draft: WorkspaceDraft,
        activate: bool,
    },
    RemoveWorkspace(WsId),
    /// A delivery being tested or landed (`beginDelivery`): its workspace
    /// does not sleep, and « Fusionner » shows a spinner, until the matching
    /// `EndDelivery`. Counted, so two holders of one task end separately.
    BeginDelivery {
        ws: WsId,
        task: String,
    },
    EndDelivery {
        ws: WsId,
        task: String,
    },
}

/// Everything an operation reaches beyond its arguments (`ControlDeps`).
///
/// The methods with a default are the TS's optional deps: a window without
/// them refuses the operations that need them, in the TS's words.
pub trait AppPort: Send + Sync + 'static {
    // ── the model ──────────────────────────────────────────────────

    /// The model as it stood after the last update that changed it.
    fn snapshot(&self) -> Arc<Snapshot>;

    /// A model write, answered once applied.
    fn apply(&self, cmd: ModelCmd) -> BoxFuture<Result<CmdOut, String>>;

    // ── terminals and processes ────────────────────────────────────

    /// The pane's terminal, wherever it is drawn; `None` while it has none.
    fn terminal(&self, pane: &PaneId) -> Option<Arc<dyn PaneTerminal>>;

    /// Bytes to the pane's PTY, as typed.
    fn write(&self, pane: &PaneId, data: &str) -> BoxFuture<Result<(), String>>;

    /// Whether the pane's shell runs.
    fn alive(&self, pane: &PaneId) -> BoxFuture<bool>;

    /// Ends a pane's shell and everything it started (`pty_kill`, tree).
    fn kill(&self, pane: &PaneId) -> BoxFuture<Result<(), String>>;

    /// The hangar-bridge queue: submitted once the Claude Code session is
    /// idle. `from` names the agent's pane (or `hangar`) when the person did
    /// not write it; `None` reads as the person's own words.
    fn enqueue(&self, pane: &PaneId, text: &str, from: Option<&str>)
    -> BoxFuture<Result<(), String>>;

    fn dir_exists(&self, path: &str) -> BoxFuture<bool>;

    /// Whether workspaces can go to windows of their own at all.
    fn can_detach(&self) -> bool {
        true
    }

    /// `tokio::time::sleep` in the app; nothing in the tests.
    fn sleep(&self, ms: u64) -> BoxFuture<()>;

    /// Epoch milliseconds now; fixed in the tests.
    fn now_ms(&self) -> u64 {
        super::types::now_ms()
    }

    // ── the preview ────────────────────────────────────────────────

    /// Lets core serve models and files from these folders on behalf of this
    /// workspace, keeping what the other workspaces were granted.
    fn model_allow_roots(&self, ws: &WsId, roots: Vec<String>) -> BoxFuture<Result<(), String>>;

    /// Canonical path, size and mtime; refused outside the allowed folders.
    fn model_stat(&self, path: &str) -> BoxFuture<Result<ModelFileInfo, String>>;

    /// Every worktree of the repository holding `path`; refused outside one.
    fn worktrees(&self, path: &str) -> BoxFuture<Result<Vec<Worktree>, String>>;

    /// A workspace's preview, drawn in whichever window shows it; `None`
    /// when that window does not answer.
    fn preview(&self, ws: &WsId, detached: bool) -> BoxFuture<Option<Arc<dyn PreviewHost>>>;

    /// What core makes of a file an agent shows (`preview_inspect`), as it
    /// answered; `check_preview_file` reads it.
    fn preview_inspect(&self, path: &str) -> BoxFuture<Result<Value, String>>;

    /// The kits installed on this machine (`kits_list`).
    fn kits(&self) -> BoxFuture<Result<Vec<KitInfo>, String>> {
        Box::pin(std::future::ready(Ok(Vec::new())))
    }

    // ── the other packages ─────────────────────────────────────────

    /// What a kit's tabs are, for `kit_tab_*` (P16).
    fn kit_tabs(&self) -> Option<Arc<dyn KitTabCatalog>> {
        None
    }

    /// The flow behind `workspace_create` (P8's `git_flow`).
    fn from_git(&self) -> Option<Arc<dyn FromGit>> {
        None
    }

    /// Merges, checks, reviews and lands workers' deliveries (delivery.rs).
    fn delivery(&self) -> Option<Arc<dyn DeliveryDeps>> {
        None
    }

    /// The orchestration engine (P19): `worker_*`, the briefing, the events,
    /// the projects.
    fn orchestration(&self) -> Option<Arc<dyn OrchestrationHooks>> {
        None
    }

    /// Hangar Mobile's operations (P20): `remote_*`.
    fn mobile(&self) -> Option<Arc<dyn MobileHooks>> {
        None
    }

    /// The deliveries in flight and settled while the app runs
    /// (deliveryControl's module state).
    fn deliveries(&self) -> &super::delivery::Deliveries;

    /// Tells the orchestration engine (`postOrchestratorEvent`). A worker
    /// start or stop during the orchestrator's own turn is `quiet`.
    fn post_event(&self, ws: &WsId, event: OrchestratorEvent, quiet: bool) {
        if let Some(engine) = self.orchestration() {
            engine.post(ws, event, quiet);
        }
    }
}

/// A pane's terminal (`PaneTerminal` of windows.ts): the window's own, or the
/// one a workspace window draws.
pub trait PaneTerminal: Send + Sync {
    /// Pastes as the person would, bracketed when the program asked for it,
    /// so several lines land as one message.
    fn paste(&self, text: &str) -> BoxFuture<Result<(), String>>;

    /// The last `n` lines, wrapped rows joined, the blank bottom dropped
    /// (`readLines`).
    fn read(&self, n: usize) -> Vec<String>;

    /// The program put the cursor keys in application mode (DECCKM).
    fn application_cursor(&self) -> bool;
}

/// Size and modification time of a model the preview may read (`ModelFileInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFileInfo {
    /// Canonical, as core resolved it.
    pub path: String,
    pub size: u64,
    pub modified_ms: u64,
}

/// A file the preview may show, as `preview_inspect` described it and
/// `checkPreviewFile` checked it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFile {
    pub path: String,
    /// One of the `PREVIEW_KINDS`.
    pub kind: String,
    pub mime: String,
    pub size: u64,
    pub name: String,
    pub modified_ms: u64,
}

/// The params a kit's tab was opened with, by name. Ordered, so two sets
/// compare and key the same whatever order they came in (`kitParamsKey`).
pub type KitParams = BTreeMap<String, String>;

/// Loading, loaded or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryStatus {
    Loading,
    Ready,
    Error,
}

/// What an open entry of a preview is (`OpenModel | OpenFile | OpenKitTab`).
#[derive(Debug, Clone, PartialEq)]
pub enum EntryKind {
    Model {
        /// `glb`, `gltf`…
        format: String,
        /// What the viewer found (triangles, meshes…), once loaded.
        info: Option<Value>,
    },
    File {
        /// `image`, `text`, `pdf`…
        kind: String,
        mime: String,
        size: u64,
    },
    Kit {
        kit_id: String,
        tab_id: String,
        params: KitParams,
    },
}

/// One open entry of a workspace's preview (`OpenPreview`).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenEntry {
    pub id: String,
    /// The canonical path; `kit:<kit>/<tab>…` for a kit's tab.
    pub path: String,
    pub title: String,
    pub opened_by: Option<String>,
    pub modified_ms: u64,
    pub status: EntryStatus,
    pub error: Option<String>,
    pub what: EntryKind,
}

impl OpenEntry {
    pub fn is_model(&self) -> bool {
        matches!(self.what, EntryKind::Model { .. })
    }

    pub fn is_kit_tab(&self) -> bool {
        matches!(self.what, EntryKind::Kit { .. })
    }

    /// The viewer's findings, for a loaded model.
    pub fn info(&self) -> Option<&Value> {
        match &self.what {
            EntryKind::Model { info, .. } => info.as_ref(),
            _ => None,
        }
    }
}

/// A workspace's preview as listed (`ModelHost.list`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PreviewListing {
    pub entries: Vec<OpenEntry>,
    pub active_id: Option<String>,
    /// Its workspace is the one its window shows.
    pub on_screen: bool,
    /// Its panel is drawn, there.
    pub shown: bool,
}

/// What to open in a preview (`PreviewToShow`).
#[derive(Debug, Clone, PartialEq)]
pub enum Showing {
    Model {
        path: String,
        title: String,
        format: String,
        opened_by: Option<String>,
        modified_ms: u64,
    },
    File {
        kind: String,
        path: String,
        title: String,
        mime: String,
        size: u64,
        opened_by: Option<String>,
        modified_ms: u64,
    },
    Kit {
        /// `kitTabPath`: one per tab, or per set of params.
        path: String,
        kit_id: String,
        tab_id: String,
        title: String,
        params: KitParams,
        /// One tab whatever its params: new ones replace the old.
        singleton: bool,
        opened_by: Option<String>,
        /// Opens the panel with it in front; false adds it quietly.
        reveal: bool,
    },
}

impl Showing {
    pub fn path(&self) -> &str {
        match self {
            Showing::Model { path, .. } | Showing::File { path, .. } | Showing::Kit { path, .. } => {
                path
            }
        }
    }
}

/// A workspace's own preview (`ModelHost` of modelHost.ts): never the one on
/// screen, nor another workspace's. P15 provides it over its preview store.
pub trait PreviewHost: Send + Sync {
    fn list(&self) -> BoxFuture<Result<PreviewListing, String>>;
    /// Opens an entry, or brings back the one already open on that path;
    /// answers its id.
    fn show(&self, entry: Showing) -> BoxFuture<Result<String, String>>;
    fn reveal(&self, id: &str) -> BoxFuture<()>;
    /// Loads a rewritten file again.
    fn reload(&self, id: &str, modified_ms: u64) -> BoxFuture<()>;
    /// The entry once loaded or failed; `None` past `ms`.
    fn when_settled(&self, id: &str, ms: u64) -> BoxFuture<Option<OpenEntry>>;
    /// A model rendered off screen, as PNG bytes.
    fn snapshot(
        &self,
        id: &str,
        angle: SnapshotAngle,
        width: u32,
        height: u32,
    ) -> BoxFuture<Result<Vec<u8>, String>>;
    fn close(&self, ids: Vec<String>) -> BoxFuture<()>;
}

/// A kit's tab an agent may open, read from the kit and the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitTabToOpen {
    pub title: String,
    pub singleton: bool,
    /// The params given, held to what the tab declares.
    pub params: KitParams,
}

/// What kit_tab_* reads of the kits (`kitTabListing`, `kitTabToOpen`,
/// `readKitTabParams` of kitTabs.ts). P16 provides it; the refusals are its
/// words, read by the agent.
pub trait KitTabCatalog: Send + Sync {
    /// `kit_tab_list`'s rows for a workspace (its kits and the ones enabled
    /// everywhere), as the TS answered them.
    fn listing(&self, ws: &Workspace, settings: &Settings, installed: &[KitInfo]) -> Vec<Value>;

    /// The tab `kit`/`tab` and the params `raw` read against it, or the
    /// reason it may not be opened on the workspace.
    fn to_open(
        &self,
        ws: &Workspace,
        settings: &Settings,
        installed: &[KitInfo],
        kit: &str,
        tab: &str,
        raw: Option<&Value>,
    ) -> Result<KitTabToOpen, String>;
}

/// How `workspace_create` went (`FromGitOutcome` of workspaceFromGit.ts).
#[derive(Debug, Clone, PartialEq)]
pub enum FromGitOutcome {
    /// Why not, in words for the agent.
    Refused { reason: String },
    /// A workspace was already on the folder: shown, nothing cloned.
    Existing {
        workspace: MadeWorkspace,
        notes: Vec<String>,
    },
    Created {
        workspace: MadeWorkspace,
        /// The repository's address.
        repo_url: String,
        /// The person's own repository.
        mine: bool,
        /// The folder was there already, a clone of it.
        reused_folder: bool,
        /// Claude Code was told to trust the folder: no question will wait.
        trusted: bool,
        notes: Vec<String>,
    },
}

/// The workspace `workspace_create` made or found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeWorkspace {
    pub id: WsId,
    pub name: String,
    pub cwd: String,
    /// Its orchestrator pane: id and name.
    pub orchestrator: Option<(PaneId, String)>,
}

/// The flow behind `workspace_create` (P8's `git_flow::run(req, deps)`).
pub trait FromGit: Send + Sync {
    fn run(&self, request: GitRequest) -> BoxFuture<FromGitOutcome>;
}

/// What a delivery reaches (`DeliveryDeps` of deliveryControl.ts).
pub trait DeliveryDeps: Send + Sync {
    fn preflight(
        &self,
        workspace: String,
        worktree: String,
        target: Option<String>,
        landing: bool,
        task: Option<String>,
    ) -> BoxFuture<Result<Plan, String>>;
    /// Answers once the delivery is over: minutes, with a check and a review.
    fn run(&self, delivery: Delivery) -> BoxFuture<Result<Outcome, String>>;
    fn cleanup(
        &self,
        worktree: String,
        branch: String,
        target: String,
        task_id: String,
    ) -> BoxFuture<Result<Cleanup, String>>;
    fn running(&self) -> BoxFuture<Result<Vec<Running>, String>>;
    /// The Claude accounts that exist, to resolve the workspace's.
    fn accounts(&self) -> BoxFuture<Result<Vec<Account>, String>>;
    /// The shells detected on this machine, for the worker's own.
    fn shells(&self) -> Vec<ShellInfo>;
    fn board_load(&self, cwd: &str) -> BoxFuture<Result<Board, String>>;
    /// Tells the board's readers (`board:changed`), as the HTTP API does.
    fn board_update(&self, cwd: &str, id: &str, patch: TaskPatch)
    -> BoxFuture<Result<Task, String>>;
    fn board_comment(
        &self,
        cwd: &str,
        id: &str,
        author: &str,
        text: &str,
    ) -> BoxFuture<Result<Task, String>>;
    /// Queues `text` for a pane as a message from Hangar, never as the
    /// person's.
    fn tell(&self, pane: &PaneId, text: &str) -> BoxFuture<Result<(), String>>;
}

/// The orchestration engine's part of the bus (P19).
pub trait OrchestrationHooks: Send + Sync {
    /// `worker_*`, for the workspace the request resolved to; the caller
    /// checked by the engine (`workerControl(op, ws, args, caller, deps)`).
    fn worker_control(
        &self,
        port: Port,
        op: String,
        ws: Workspace,
        args: Value,
        caller: Caller,
    ) -> BoxFuture<Result<Value, String>>;

    /// `orchestrator_briefing {paneId}` on the internal channel: the
    /// `Briefing`, or `null` for a pane with no role.
    fn briefing(&self, port: Port, args: Value) -> BoxFuture<Result<Value, String>>;

    /// `postOrchestratorEvent`.
    fn post(&self, ws: &WsId, event: OrchestratorEvent, quiet: bool);

    /// Every project's summary, for `project_list`: the summaries and
    /// `renderOverview` of them.
    fn projects(&self, port: Port) -> BoxFuture<Result<Projects, String>>;
}

/// What `loadProjects` read, with the texts `project_*` answers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Projects {
    /// `ProjectSummary[]`, serialized as the TS did.
    pub projects: Vec<ProjectView>,
    /// `renderOverview(projects, now)`.
    pub overview: String,
}

/// One project: its summary, and `renderProject` of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectView {
    pub id: WsId,
    pub summary: Value,
    pub text: String,
}

/// Hangar Mobile's part of the bus (P20): every `remote_*` operation of the
/// internal channel, answered as the person.
pub trait MobileHooks: Send + Sync {
    fn remote(&self, port: Port, op: String, args: Value) -> BoxFuture<Result<Value, String>>;
}

/// A handler another package installs on the bus once hydrated
/// (`control::Msg::Install`).
#[derive(Clone)]
pub enum Hook {
    Orchestration(Arc<dyn OrchestrationHooks>),
    Mobile(Arc<dyn MobileHooks>),
    FromGit(Arc<dyn FromGit>),
    KitTabs(Arc<dyn KitTabCatalog>),
    Preview(Arc<dyn PreviewHosts>),
}

impl fmt::Debug for Hook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Hook::Orchestration(_) => "Hook::Orchestration",
            Hook::Mobile(_) => "Hook::Mobile",
            Hook::FromGit(_) => "Hook::FromGit",
            Hook::KitTabs(_) => "Hook::KitTabs",
            Hook::Preview(_) => "Hook::Preview",
        })
    }
}

/// Every workspace's preview (P15): the `PreviewHost` of one, in whichever
/// window draws it. Until P15 installs it, the live port adapts the shell's
/// `PreviewAccess`.
pub trait PreviewHosts: Send + Sync {
    fn host(&self, ws: &WsId, detached: bool) -> Option<Arc<dyn PreviewHost>>;
}

/// A pane's activity in the snapshot, for the operations that report it.
pub fn activity_of<'a>(snapshot: &'a Snapshot, pane: &PaneId) -> Option<&'a PaneActivity> {
    snapshot.activity.get(pane)
}

/// Whether a workspace's terminals are mounted: opened in this run, or shown
/// in a window of its own (`isOpen` of App.tsx).
pub fn is_open(snapshot: &Snapshot, ws: &Workspace) -> bool {
    snapshot.opened.contains(&ws.id) || ws.detached == Some(true)
}

/// Put to sleep for being left alone; opening it wakes it.
pub fn is_asleep(snapshot: &Snapshot, ws: &WsId) -> bool {
    snapshot.asleep.contains(ws)
}

/// The workspace `id` as the snapshot has it.
pub fn workspace_in<'a>(snapshot: &'a Snapshot, id: &str) -> Option<&'a Workspace> {
    snapshot.state.workspaces.iter().find(|ws| ws.id == id)
}

/// A JSON object from `(key, value)` pairs, in order, for the answers.
pub fn object<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

/// `value` as JSON; what cannot be written reads as `null`.
pub fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
