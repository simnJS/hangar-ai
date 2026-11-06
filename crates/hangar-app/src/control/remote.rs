//! The control operations of a node's agents (docs/native/nodes.md,
//! "Orchestration on a node"): the node forwards core's control requests on
//! its events, the desktop runs them here through the same handlers as a
//! local one, and answers on the node (`POST /node/control/{id}/reply`).
//!
//! They run against [`RemotePort`]: the app's port, narrowed to the node's
//! own workspaces (an agent of a node never sees or drives this machine's),
//! with what touches the disk sent to the node — the panes' queues, the
//! folders, the worktrees, the deliveries. The panes themselves are drawn
//! here and their shells run on the node already (`nodes::remote`), and the
//! orchestration engine finds a workspace's node from the workspace.
//!
//! A delivery runs on the node from start to end: the merge, the check, the
//! review, the landing. The workspace's orchestration settings are the
//! desktop's (`Workspace.orchestration`); each delivery takes them with it
//! (`Delivery.check_command`, `reviewer_model`, `land`), as a local one
//! does, so the node never holds a copy that could fall behind. What only
//! means something on this machine is left out: its shells, its build
//! cache, its Claude accounts (the node's review spends the node's login).
//!
//! A worker a local workspace sent to a node (nodes/away.rs) is not the
//! node's: its requests come through the node all the same, and run here
//! against its own workspace only ([`AwayPort`]), as a local worker's do —
//! its board, its orchestrator, its delivery, landed on this machine.

use std::sync::Arc;

use serde_json::Value;

use hangar_core::accounts::Account;
use hangar_core::board::{Board, Task, TaskPatch};
use hangar_core::delivery::{Cleanup, Delivery, Outcome, Plan, Running};
use hangar_core::git::Worktree;
use hangar_core::kits::KitInfo;
use hangar_core::shells::ShellInfo;

use crate::nodes::NodeClient;
use crate::nodes::engine;
use crate::state::types::AppState;
use crate::state::{PaneId, Snapshot, WsId};

use super::types::Caller;

use super::delivery::Deliveries;
use super::port::{
    AppPort, BoxFuture, CmdOut, DeliveryDeps, FromGit, KitTabCatalog, MobileHooks, ModelCmd,
    ModelFileInfo, OrchestrationHooks, PaneTerminal, Port, PreviewHost,
};

/// The operations a node's agents are refused, and why: those that reach
/// past the node's own workspaces.
pub fn refused(op: &str) -> Option<&'static str> {
    match op {
        "workspace_create" => Some(
            "workspace_create makes a workspace on the person's computer, which an agent of a node does not do. \
Ask the person to create it in Hangar.AI.",
        ),
        op if op.starts_with("project_") => Some(
            "The project manager's operations read every project of the person's computer: \
they are not open to an agent of a node.",
        ),
        "dev_command" => Some("dev_command is not open to an agent of a node."),
        _ => None,
    }
}

/// Said when a node's agent shows a file in the preview: its files are on
/// the node, which the preview does not read yet.
const NO_PREVIEW: &str = "The preview does not show files of a node yet: the file is on the node, \
not on the person's computer.";

/// The app's port, for the requests of node `node`.
pub struct RemotePort {
    inner: Port,
    node: String,
}

impl RemotePort {
    pub fn new(inner: Port, node: String) -> RemotePort {
        RemotePort { inner, node }
    }

    fn client(&self) -> Result<NodeClient, String> {
        crate::nodes::remote::client_of(&self.node)
    }
}

/// `snapshot` with only the workspaces of node `node`.
pub fn narrowed(snapshot: &Snapshot, node: &str) -> Snapshot {
    let mut state = (*snapshot.state).clone();
    state
        .workspaces
        .retain(|ws| ws.node.as_deref() == Some(node));
    let mine = |id: &String| state.workspaces.iter().any(|ws| &ws.id == id);
    let opened = snapshot
        .opened
        .iter()
        .filter(|id| mine(id))
        .cloned()
        .collect();
    let asleep = snapshot
        .asleep
        .iter()
        .filter(|id| mine(id))
        .cloned()
        .collect();
    let delivering = snapshot
        .delivering
        .iter()
        .filter(|(id, _)| mine(id))
        .map(|(id, tasks)| (id.clone(), tasks.clone()))
        .collect();
    Snapshot {
        state: Arc::new(state),
        opened,
        asleep,
        activity: snapshot.activity.clone(),
        delivering,
        hydrated: snapshot.hydrated,
    }
}

impl AppPort for RemotePort {
    fn snapshot(&self) -> Arc<Snapshot> {
        Arc::new(narrowed(&self.inner.snapshot(), &self.node))
    }

    fn apply(&self, cmd: ModelCmd) -> BoxFuture<Result<CmdOut, String>> {
        self.inner.apply(cmd)
    }

    fn terminal(&self, pane: &PaneId) -> Option<Arc<dyn PaneTerminal>> {
        self.inner.terminal(pane)
    }

    fn write(&self, pane: &PaneId, data: &str) -> BoxFuture<Result<(), String>> {
        self.inner.write(pane, data)
    }

    fn alive(&self, pane: &PaneId) -> BoxFuture<bool> {
        self.inner.alive(pane)
    }

    /// On the node, whether the pane is mounted here yet or not.
    fn kill(&self, pane: &PaneId) -> BoxFuture<Result<(), String>> {
        let client = self.client();
        let pane = pane.clone();
        Box::pin(async move { crate::nodes::remote::kill(&client?, &pane).await })
    }

    fn enqueue(
        &self,
        pane: &PaneId,
        text: &str,
        from: Option<&str>,
    ) -> BoxFuture<Result<(), String>> {
        let client = self.client();
        let (pane, text, from) = (pane.clone(), text.to_string(), from.map(str::to_string));
        Box::pin(async move { engine::enqueue(&client?, &pane, &text, from.as_deref()).await })
    }

    fn dir_exists(&self, path: &str) -> BoxFuture<bool> {
        let client = self.client();
        let path = path.to_string();
        Box::pin(async move {
            match client {
                Ok(client) => engine::dir_exists(&client, &path).await,
                Err(_) => false,
            }
        })
    }

    fn can_detach(&self) -> bool {
        self.inner.can_detach()
    }

    fn sleep(&self, ms: u64) -> BoxFuture<()> {
        self.inner.sleep(ms)
    }

    fn spawn(&self, task: BoxFuture<()>) {
        self.inner.spawn(task);
    }

    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    fn model_allow_roots(&self, _ws: &WsId, _roots: Vec<String>) -> BoxFuture<Result<(), String>> {
        Box::pin(std::future::ready(Err(NO_PREVIEW.to_string())))
    }

    fn model_stat(&self, _path: &str) -> BoxFuture<Result<ModelFileInfo, String>> {
        Box::pin(std::future::ready(Err(NO_PREVIEW.to_string())))
    }

    fn worktrees(&self, path: &str) -> BoxFuture<Result<Vec<Worktree>, String>> {
        let client = self.client();
        let path = path.to_string();
        Box::pin(async move { engine::worktree_list(&client?, &path).await })
    }

    fn preview(&self, ws: &WsId, detached: bool) -> BoxFuture<Option<Arc<dyn PreviewHost>>> {
        self.inner.preview(ws, detached)
    }

    fn preview_inspect(&self, _path: &str) -> BoxFuture<Result<Value, String>> {
        Box::pin(std::future::ready(Err(NO_PREVIEW.to_string())))
    }

    fn kits(&self) -> BoxFuture<Result<Vec<KitInfo>, String>> {
        self.inner.kits()
    }

    fn kit_tabs(&self) -> Option<Arc<dyn KitTabCatalog>> {
        self.inner.kit_tabs()
    }

    fn git_flow(&self) -> Option<Arc<dyn FromGit>> {
        None
    }

    fn delivery(&self) -> Option<Arc<dyn DeliveryDeps>> {
        Some(Arc::new(RemoteDelivery {
            node: self.node.clone(),
        }))
    }

    fn orchestration(&self) -> Option<Arc<dyn OrchestrationHooks>> {
        self.inner.orchestration()
    }

    fn mobile(&self) -> Option<Arc<dyn MobileHooks>> {
        None
    }

    fn deliveries(&self) -> &Deliveries {
        self.inner.deliveries()
    }
}

/// core's delivery, the board and the queue of node `node`.
pub struct RemoteDelivery {
    pub node: String,
}

impl RemoteDelivery {
    fn client(&self) -> Result<NodeClient, String> {
        crate::nodes::remote::client_of(&self.node)
    }
}

/// What of a delivery request only this machine means is taken out: the
/// check runs in the node's shell, with no build cache of this machine's,
/// and its folders are the node's own.
pub fn for_node(mut delivery: Delivery) -> Delivery {
    delivery.shell = None;
    delivery.build_env = Vec::new();
    delivery.check_root = None;
    delivery.data_dir = None;
    delivery
}

impl DeliveryDeps for RemoteDelivery {
    fn preflight(
        &self,
        workspace: String,
        worktree: String,
        target: Option<String>,
        landing: bool,
        task: Option<String>,
    ) -> BoxFuture<Result<Plan, String>> {
        let client = self.client();
        Box::pin(async move {
            engine::delivery_preflight(
                &client?,
                &workspace,
                &worktree,
                target.as_deref(),
                landing,
                task.as_deref(),
            )
            .await
        })
    }

    fn run(&self, delivery: Delivery) -> BoxFuture<Result<Outcome, String>> {
        let client = self.client();
        Box::pin(async move { engine::delivery_run(&client?, &for_node(delivery)).await })
    }

    fn cleanup(
        &self,
        worktree: String,
        branch: String,
        target: String,
        task_id: String,
    ) -> BoxFuture<Result<Cleanup, String>> {
        let client = self.client();
        Box::pin(async move {
            engine::delivery_cleanup(&client?, &worktree, &branch, &target, &task_id).await
        })
    }

    fn running(&self) -> BoxFuture<Result<Vec<Running>, String>> {
        let client = self.client();
        Box::pin(async move { engine::delivery_running(&client?).await })
    }

    fn accounts(&self) -> BoxFuture<Result<Vec<Account>, String>> {
        Box::pin(std::future::ready(Ok(engine::accounts())))
    }

    /// None of this machine's: the check runs in the node's own shell.
    fn shells(&self) -> Vec<ShellInfo> {
        Vec::new()
    }

    fn board_load(&self, cwd: &str) -> BoxFuture<Result<Board, String>> {
        let client = self.client();
        let cwd = cwd.to_string();
        Box::pin(async move { engine::board(&client?, &cwd).await })
    }

    fn board_update(
        &self,
        cwd: &str,
        id: &str,
        patch: TaskPatch,
    ) -> BoxFuture<Result<Task, String>> {
        let client = self.client();
        let (cwd, id) = (cwd.to_string(), id.to_string());
        Box::pin(async move { engine::board_update(&client?, &cwd, &id, &patch).await })
    }

    fn board_comment(
        &self,
        cwd: &str,
        id: &str,
        author: &str,
        text: &str,
    ) -> BoxFuture<Result<Task, String>> {
        let client = self.client();
        let (cwd, id, author, text) = (
            cwd.to_string(),
            id.to_string(),
            author.to_string(),
            text.to_string(),
        );
        Box::pin(async move { engine::board_comment(&client?, &cwd, &id, &author, &text).await })
    }

    fn tell(&self, pane: &PaneId, text: &str) -> BoxFuture<Result<(), String>> {
        let client = self.client();
        let (pane, text) = (pane.clone(), text.to_string());
        Box::pin(async move {
            engine::enqueue(&client?, &pane, &text, Some(super::delivery_text::HANGAR)).await
        })
    }
}

/* ── A local workspace's worker on a node ──────────────────────── */

/// The local workspace whose worker asks, when that worker is one sent to
/// node `node`: its requests are this machine's, for that workspace.
pub fn away_workspace(state: &AppState, node: &str, args: &Value) -> Option<WsId> {
    let pane = Caller::of(args).pane_id?;
    state
        .workspaces
        .iter()
        .find(|ws| {
            ws.node.is_none()
                && ws.panes.iter().any(|entry| {
                    entry.id == pane
                        && crate::nodes::away::pane_node(entry).as_deref() == Some(node)
                })
        })
        .map(|ws| ws.id.clone())
}

/// `snapshot` with only workspace `id`.
pub fn narrowed_to(snapshot: &Snapshot, id: &str) -> Snapshot {
    let mut state = (*snapshot.state).clone();
    state.workspaces.retain(|ws| ws.id == id);
    Snapshot {
        state: Arc::new(state),
        opened: snapshot
            .opened
            .iter()
            .filter(|opened| *opened == id)
            .cloned()
            .collect(),
        asleep: snapshot
            .asleep
            .iter()
            .filter(|asleep| *asleep == id)
            .cloned()
            .collect(),
        activity: snapshot.activity.clone(),
        delivering: snapshot
            .delivering
            .iter()
            .filter(|(ws, _)| *ws == id)
            .map(|(ws, tasks)| (ws.clone(), tasks.clone()))
            .collect(),
        hydrated: snapshot.hydrated,
    }
}

/// The app's port, for a worker of local workspace `workspace` running on a
/// node: this machine's in everything (its panes on a node are reached
/// there by the app's port itself), but it sees its own workspace only, and
/// shows no file of this machine's in a preview — its files are the node's.
pub struct AwayPort {
    inner: Port,
    workspace: WsId,
}

impl AwayPort {
    pub fn new(inner: Port, workspace: WsId) -> AwayPort {
        AwayPort { inner, workspace }
    }
}

impl AppPort for AwayPort {
    fn snapshot(&self) -> Arc<Snapshot> {
        Arc::new(narrowed_to(&self.inner.snapshot(), &self.workspace))
    }

    fn apply(&self, cmd: ModelCmd) -> BoxFuture<Result<CmdOut, String>> {
        self.inner.apply(cmd)
    }

    fn terminal(&self, pane: &PaneId) -> Option<Arc<dyn PaneTerminal>> {
        self.inner.terminal(pane)
    }

    fn write(&self, pane: &PaneId, data: &str) -> BoxFuture<Result<(), String>> {
        self.inner.write(pane, data)
    }

    fn alive(&self, pane: &PaneId) -> BoxFuture<bool> {
        self.inner.alive(pane)
    }

    fn kill(&self, pane: &PaneId) -> BoxFuture<Result<(), String>> {
        self.inner.kill(pane)
    }

    fn enqueue(
        &self,
        pane: &PaneId,
        text: &str,
        from: Option<&str>,
    ) -> BoxFuture<Result<(), String>> {
        self.inner.enqueue(pane, text, from)
    }

    fn dir_exists(&self, path: &str) -> BoxFuture<bool> {
        self.inner.dir_exists(path)
    }

    fn can_detach(&self) -> bool {
        self.inner.can_detach()
    }

    fn sleep(&self, ms: u64) -> BoxFuture<()> {
        self.inner.sleep(ms)
    }

    fn spawn(&self, task: BoxFuture<()>) {
        self.inner.spawn(task);
    }

    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }

    fn model_allow_roots(&self, _ws: &WsId, _roots: Vec<String>) -> BoxFuture<Result<(), String>> {
        Box::pin(std::future::ready(Err(NO_PREVIEW.to_string())))
    }

    fn model_stat(&self, _path: &str) -> BoxFuture<Result<ModelFileInfo, String>> {
        Box::pin(std::future::ready(Err(NO_PREVIEW.to_string())))
    }

    fn worktrees(&self, path: &str) -> BoxFuture<Result<Vec<Worktree>, String>> {
        self.inner.worktrees(path)
    }

    fn preview(&self, ws: &WsId, detached: bool) -> BoxFuture<Option<Arc<dyn PreviewHost>>> {
        self.inner.preview(ws, detached)
    }

    fn preview_inspect(&self, _path: &str) -> BoxFuture<Result<Value, String>> {
        Box::pin(std::future::ready(Err(NO_PREVIEW.to_string())))
    }

    fn kits(&self) -> BoxFuture<Result<Vec<KitInfo>, String>> {
        self.inner.kits()
    }

    fn kit_tabs(&self) -> Option<Arc<dyn KitTabCatalog>> {
        self.inner.kit_tabs()
    }

    fn git_flow(&self) -> Option<Arc<dyn FromGit>> {
        None
    }

    fn delivery(&self) -> Option<Arc<dyn DeliveryDeps>> {
        self.inner.delivery()
    }

    fn orchestration(&self) -> Option<Arc<dyn OrchestrationHooks>> {
        self.inner.orchestration()
    }

    fn mobile(&self) -> Option<Arc<dyn MobileHooks>> {
        None
    }

    fn deliveries(&self) -> &Deliveries {
        self.inner.deliveries()
    }
}
