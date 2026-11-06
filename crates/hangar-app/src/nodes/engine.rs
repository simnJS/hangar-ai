//! What the orchestration engine and the deliveries do on a node, for a
//! workspace that lives there (docs/native/nodes.md, "Orchestration on a
//! node"): hangar-node's engine.rs routes, called as hangar-core's helpers
//! are called for a local workspace, their answers read back into core's
//! own types.
//!
//! Core's git and delivery types are written by the node, never read by
//! core (they only `Serialize`): the shapes below read them, field for
//! field, and become core's.

use std::time::Duration;

use hangar_core::accounts::Account;
use hangar_core::board::{Board, Task, TaskPatch};
use hangar_core::delivery::{Check, Cleanup, Delivery, Kind, Outcome, Plan, Running, Tested};
use hangar_core::git::{
    ChangedFile, Changes, CopyResult, DeleteBranchResult, HookReport, RemoveResult, RepoInfo,
    Worktree,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::model::NodeClient;
use super::wire;

/// A worktree's checkout, a branch's deletion, a postCreate hook: as long as
/// core gives a hook (`git::HOOK_TIMEOUT`, 5 minutes) and the checkout of a
/// large repository — what core's control waits for a `worker_*` answer.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(8 * 60);

/// A delivery: its check and its review are minutes, and the node gives up
/// on them by itself (core's limits); this only runs out when the node no
/// longer answers.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(90 * 60);

/// `value` of the node's `{"value": …}`, read as `T`; its `{"error"}`
/// otherwise.
async fn call<T: DeserializeOwned>(
    client: &NodeClient,
    method: &str,
    path: &str,
    body: Option<Value>,
    limit: Duration,
) -> Result<T, String> {
    let answer = wire::request_within(client, method, path, body.as_ref(), limit)
        .await?
        .json()?;
    serde_json::from_value(answer.get("value").cloned().unwrap_or(Value::Null))
        .map_err(|err| format!("{method} {path}: {err}"))
}

async fn quick<T: DeserializeOwned>(
    client: &NodeClient,
    method: &str,
    path: &str,
    body: Value,
) -> Result<T, String> {
    call(client, method, path, Some(body), wire::REQUEST_TIMEOUT).await
}

/* ── git ────────────────────────────────────────────────────────── */

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoInfoWire {
    #[serde(default)]
    installed: bool,
    #[serde(default)]
    is_repo: bool,
    root: Option<String>,
    main_root: Option<String>,
    #[serde(default)]
    is_linked_worktree: bool,
    branch: Option<String>,
    head: Option<String>,
}

/// `git_repo_info` on the node. A node that does not answer reads as git
/// missing, as a local call that fails does.
pub async fn repo_info(client: &NodeClient, path: &str) -> RepoInfo {
    let read: Result<RepoInfoWire, String> = quick(
        client,
        "POST",
        "/node/git",
        json!({ "op": "repo_info", "path": path }),
    )
    .await;
    match read {
        Ok(info) => RepoInfo {
            installed: info.installed,
            is_repo: info.is_repo,
            root: info.root,
            main_root: info.main_root,
            is_linked_worktree: info.is_linked_worktree,
            branch: info.branch,
            head: info.head,
        },
        Err(_) => RepoInfo::default(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorktreeWire {
    path: String,
    head: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    is_main: bool,
    #[serde(default)]
    is_current: bool,
    #[serde(default)]
    bare: bool,
    #[serde(default)]
    detached: bool,
    locked: Option<String>,
    prunable: Option<String>,
}

pub async fn worktree_list(client: &NodeClient, path: &str) -> Result<Vec<Worktree>, String> {
    let trees: Vec<WorktreeWire> = quick(
        client,
        "POST",
        "/node/git",
        json!({ "op": "worktree_list", "path": path }),
    )
    .await?;
    Ok(trees
        .into_iter()
        .map(|tree| Worktree {
            path: tree.path,
            head: tree.head,
            branch: tree.branch,
            is_main: tree.is_main,
            is_current: tree.is_current,
            bare: tree.bare,
            detached: tree.detached,
            locked: tree.locked,
            prunable: tree.prunable,
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangedFileWire {
    path: String,
    old_path: Option<String>,
    status: String,
    additions: Option<u32>,
    deletions: Option<u32>,
    #[serde(default)]
    binary: bool,
}

#[derive(Deserialize)]
struct ChangesWire {
    root: String,
    base: Option<String>,
    #[serde(default)]
    files: Vec<ChangedFileWire>,
    #[serde(default)]
    truncated: bool,
}

pub async fn changes(client: &NodeClient, path: &str) -> Result<Changes, String> {
    let read: ChangesWire = quick(
        client,
        "POST",
        "/node/git",
        json!({ "op": "changes", "path": path }),
    )
    .await?;
    Ok(Changes {
        root: read.root,
        base: read.base,
        files: read
            .files
            .into_iter()
            .map(|file| ChangedFile {
                path: file.path,
                old_path: file.old_path,
                status: file.status,
                additions: file.additions,
                deletions: file.deletions,
                binary: file.binary,
            })
            .collect(),
        truncated: read.truncated,
    })
}

pub async fn commits_ahead(
    client: &NodeClient,
    path: &str,
    base: &str,
    head: &str,
) -> Result<u32, String> {
    quick(
        client,
        "POST",
        "/node/git",
        json!({ "op": "commits_ahead", "path": path, "base": base, "head": head }),
    )
    .await
}

/// Whether folder `path` exists on the node; one it cannot tell of does not.
pub async fn dir_exists(client: &NodeClient, path: &str) -> bool {
    quick(
        client,
        "POST",
        "/node/git",
        json!({ "op": "dir_exists", "path": path }),
    )
    .await
    .unwrap_or(false)
}

pub async fn worktree_add(
    client: &NodeClient,
    repo: &str,
    path: &str,
    branch: &str,
    create: bool,
    base: Option<&str>,
) -> Result<(), String> {
    let _: Value = call(
        client,
        "POST",
        "/node/worktree",
        Some(json!({
            "repo": repo, "path": path, "branch": branch, "create": create, "base": base,
        })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HookWire {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    timed_out: bool,
    exit_code: Option<i32>,
    #[serde(default)]
    output: String,
    error: Option<String>,
}

impl From<HookWire> for HookReport {
    fn from(hook: HookWire) -> Self {
        HookReport {
            ok: hook.ok,
            timed_out: hook.timed_out,
            exit_code: hook.exit_code,
            output: hook.output,
            error: hook.error,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoveWire {
    #[serde(default)]
    removed: bool,
    #[serde(default)]
    needs_force: bool,
    message: Option<String>,
    #[serde(default)]
    hook_blocked: bool,
    hook: Option<HookWire>,
}

pub async fn worktree_remove(
    client: &NodeClient,
    repo: &str,
    path: &str,
    force: bool,
) -> Result<RemoveResult, String> {
    let removed: RemoveWire = call(
        client,
        "DELETE",
        "/node/worktree",
        Some(json!({ "repo": repo, "path": path, "force": force })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(RemoveResult {
        removed: removed.removed,
        needs_force: removed.needs_force,
        message: removed.message,
        hook_blocked: removed.hook_blocked,
        hook: removed.hook.map(HookReport::from),
    })
}

pub async fn worktree_hook(
    client: &NodeClient,
    repo: &str,
    path: &str,
    command: &str,
) -> Result<Option<HookReport>, String> {
    let hook: Option<HookWire> = call(
        client,
        "POST",
        "/node/worktree/hook",
        Some(json!({ "repo": repo, "path": path, "command": command })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(hook.map(HookReport::from))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteWire {
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    needs_force: bool,
    message: Option<String>,
}

pub async fn branch_delete(
    client: &NodeClient,
    repo: &str,
    branch: &str,
    force: bool,
) -> Result<DeleteBranchResult, String> {
    let deleted: DeleteWire = call(
        client,
        "DELETE",
        "/node/branch",
        Some(json!({ "repo": repo, "branch": branch, "force": force })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(DeleteBranchResult {
        deleted: deleted.deleted,
        needs_force: deleted.needs_force,
        message: deleted.message,
    })
}

#[derive(Deserialize)]
struct CopyWire {
    #[serde(default)]
    copied: u32,
    #[serde(default)]
    failed: u32,
}

pub async fn copy_ignored(
    client: &NodeClient,
    from: &str,
    to: &str,
    patterns: &[&str],
) -> Result<CopyResult, String> {
    let copied: CopyWire = call(
        client,
        "POST",
        "/node/worktree/copy",
        Some(json!({ "from": from, "to": to, "patterns": patterns })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(CopyResult {
        copied: copied.copied,
        failed: copied.failed,
    })
}

/* ── the board and the bridge ──────────────────────────────────── */

/// The node's board in `cwd`, whole (`/api/board`, core's own).
pub async fn board(client: &NodeClient, cwd: &str) -> Result<Board, String> {
    let path = format!("/api/board?cwd={}", wire::encode(cwd));
    let value = wire::request(client, "GET", &path, None).await?.json()?;
    serde_json::from_value(value).map_err(|err| err.to_string())
}

pub async fn claim_worker(
    client: &NodeClient,
    cwd: &str,
    id: &str,
    worker: &str,
    shared_folder: bool,
) -> Result<Task, String> {
    quick(
        client,
        "POST",
        "/node/board/claim-worker",
        json!({ "cwd": cwd, "id": id, "worker": worker, "sharedFolder": shared_folder }),
    )
    .await
}

pub async fn release_worker(
    client: &NodeClient,
    cwd: &str,
    id: &str,
    worker: &str,
    undo: bool,
) -> Result<Task, String> {
    quick(
        client,
        "POST",
        "/node/board/release-worker",
        json!({ "cwd": cwd, "id": id, "worker": worker, "undo": undo }),
    )
    .await
}

/// A `TaskPatch` as the node reads one back. Core's patch only reads: its
/// block and unblock, which no delivery sends, are refused rather than lost.
pub fn patch_json(patch: &TaskPatch) -> Result<Value, String> {
    if patch.block.is_some() || patch.unblock.is_some() {
        return Err("a block or an unblock is not sent to a node".into());
    }
    let mut body = serde_json::Map::new();
    let mut put = |key: &str, value: Value| {
        if !value.is_null() {
            body.insert(key.to_owned(), value);
        }
    };
    put("title", json!(patch.title));
    put("description", json!(patch.description));
    put("column", json!(patch.column));
    put("priority", json!(patch.priority));
    put("assignee", json!(patch.assignee));
    put("release", json!(patch.release));
    put("labels", json!(patch.labels));
    put("depends_on", json!(patch.depends_on));
    put("order", json!(patch.order));
    put("owns", json!(patch.owns));
    Ok(Value::Object(body))
}

pub async fn board_update(
    client: &NodeClient,
    cwd: &str,
    id: &str,
    patch: &TaskPatch,
) -> Result<Task, String> {
    quick(
        client,
        "POST",
        "/node/board/update",
        json!({ "cwd": cwd, "id": id, "patch": patch_json(patch)? }),
    )
    .await
}

pub async fn board_comment(
    client: &NodeClient,
    cwd: &str,
    id: &str,
    author: &str,
    text: &str,
) -> Result<Task, String> {
    quick(
        client,
        "POST",
        "/node/board/comment",
        json!({ "cwd": cwd, "id": id, "author": author, "text": text }),
    )
    .await
}

/// `text` queued for pane `pane`'s session on the node, from `from`
/// (`bridge_enqueue` there: the pane's mod reads the node's queue).
pub async fn enqueue(
    client: &NodeClient,
    pane: &str,
    text: &str,
    from: Option<&str>,
) -> Result<(), String> {
    let path = format!("/node/bridge/{}", wire::encode(pane));
    let _: Value = quick(client, "POST", &path, json!({ "text": text, "from": from })).await?;
    Ok(())
}

/* ── the deliveries ────────────────────────────────────────────── */

#[derive(Deserialize)]
struct TestedWire {
    base: String,
    merge: String,
    #[serde(default)]
    current: bool,
}

#[derive(Deserialize)]
struct PlanWire {
    repo: String,
    queue: String,
    branch: String,
    target: String,
    #[serde(default)]
    ahead: u32,
    tested: Option<TestedWire>,
}

pub async fn delivery_preflight(
    client: &NodeClient,
    workspace: &str,
    worktree: &str,
    target: Option<&str>,
    landing: bool,
    task: Option<&str>,
) -> Result<Plan, String> {
    let plan: PlanWire = call(
        client,
        "POST",
        "/node/delivery/preflight",
        Some(json!({
            "workspace": workspace, "worktree": worktree, "target": target,
            "landing": landing, "task": task,
        })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(Plan {
        repo: plan.repo,
        queue: plan.queue,
        branch: plan.branch,
        target: plan.target,
        ahead: plan.ahead,
        tested: plan.tested.map(|tested| Tested {
            base: tested.base,
            merge: tested.merge,
            current: tested.current,
        }),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CheckWire {
    command: String,
    #[serde(default)]
    passed: bool,
    exit_code: Option<i32>,
    #[serde(default)]
    timed_out: bool,
    #[serde(default)]
    seconds: u64,
    #[serde(default)]
    tail: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutcomeWire {
    outcome: String,
    detail: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    in_checkout: bool,
    #[serde(default)]
    changed: Vec<String>,
    #[serde(default)]
    outside_owns: Vec<String>,
    check: Option<CheckWire>,
    check_skipped: Option<String>,
    review: Option<hangar_core::delivery::Verdict>,
    merge: Option<String>,
    base: Option<String>,
    landed_in: Option<String>,
}

fn kind_of(name: &str) -> Kind {
    match name {
        "landed" => Kind::Landed,
        "ready" => Kind::Ready,
        "conflict" => Kind::Conflict,
        "checkFailed" => Kind::CheckFailed,
        "changesRequested" => Kind::ChangesRequested,
        "dirty" => Kind::Dirty,
        "stale" => Kind::Stale,
        _ => Kind::Error,
    }
}

/// What a delivery request is on the wire: core's `Delivery` reads it.
pub fn delivery_json(delivery: &Delivery) -> Value {
    json!({
        "workspace": delivery.workspace,
        "worktree": delivery.worktree,
        "branch": delivery.branch,
        "target": delivery.target,
        "task": {
            "id": delivery.task.id,
            "title": delivery.task.title,
            "description": delivery.task.description,
            "owns": delivery.task.owns,
        },
        "summary": delivery.summary,
        "checkCommand": delivery.check_command,
        "shell": delivery.shell,
        "reviewerModel": delivery.reviewer_model,
        "claudeAccount": delivery.claude_account,
        "land": delivery.land,
        "reuse": delivery.reuse,
        "portBase": delivery.port_base,
        "workspaceId": delivery.workspace_id,
        "buildEnv": delivery.build_env,
        "checkRoot": delivery.check_root,
        "dataDir": delivery.data_dir,
    })
}

/// A delivery run on the node, to its end: the merge, the workspace's check
/// command, the review and the landing all happen there.
pub async fn delivery_run(client: &NodeClient, delivery: &Delivery) -> Result<Outcome, String> {
    let read: OutcomeWire = call(
        client,
        "POST",
        "/node/delivery/run",
        Some(delivery_json(delivery)),
        DELIVERY_TIMEOUT,
    )
    .await?;
    Ok(Outcome {
        outcome: kind_of(&read.outcome),
        detail: read.detail,
        files: read.files,
        in_checkout: read.in_checkout,
        changed: read.changed,
        outside_owns: read.outside_owns,
        check: read.check.map(|check| Check {
            command: check.command,
            passed: check.passed,
            exit_code: check.exit_code,
            timed_out: check.timed_out,
            seconds: check.seconds,
            tail: check.tail,
        }),
        check_skipped: read.check_skipped,
        review: read.review,
        merge: read.merge,
        base: read.base,
        landed_in: read.landed_in,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CleanupWire {
    #[serde(default)]
    worktree_removed: bool,
    #[serde(default)]
    branch_deleted: bool,
    #[serde(default)]
    kept: Vec<String>,
}

pub async fn delivery_cleanup(
    client: &NodeClient,
    worktree: &str,
    branch: &str,
    target: &str,
    task_id: &str,
) -> Result<Cleanup, String> {
    let cleanup: CleanupWire = call(
        client,
        "POST",
        "/node/delivery/cleanup",
        Some(json!({
            "worktree": worktree, "branch": branch, "target": target, "taskId": task_id,
        })),
        GIT_TIMEOUT,
    )
    .await?;
    Ok(Cleanup {
        worktree_removed: cleanup.worktree_removed,
        branch_deleted: cleanup.branch_deleted,
        kept: cleanup.kept,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunningWire {
    task_id: String,
    queue: String,
    stage: String,
    #[serde(default)]
    since: u64,
}

pub async fn delivery_running(client: &NodeClient) -> Result<Vec<Running>, String> {
    let running: Vec<RunningWire> = call(
        client,
        "GET",
        "/node/delivery/running",
        None,
        wire::REQUEST_TIMEOUT,
    )
    .await?;
    Ok(running
        .into_iter()
        .map(|run| Running {
            task_id: run.task_id,
            queue: run.queue,
            stage: run.stage,
            since: run.since,
        })
        .collect())
}

/// The Claude accounts a delivery on the node may spend: the node's own
/// login only — this machine's accounts are not there.
pub fn accounts() -> Vec<Account> {
    Vec::new()
}

/* ── the control requests ──────────────────────────────────────── */

/// The answer to control request `id` of node `node`, handed to its core
/// as the window's would be (`POST /node/control/{id}/reply`).
pub async fn control_reply(
    node: &str,
    id: &str,
    answer: Result<Value, String>,
) -> Result<(), String> {
    let client = super::remote::client_of(node)?;
    let body = match answer {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    let path = format!("/node/control/{}/reply", wire::encode(id));
    let answer = wire::request(&client, "POST", &path, Some(&body)).await?;
    if answer.ok() {
        Ok(())
    } else {
        Err(answer.error())
    }
}

/* ── which node ────────────────────────────────────────────────── */

/// The node of the workspace whose folder is `cwd`, when one of the state's
/// workspaces on a node has it.
pub fn node_of_cwd(state: &crate::state::types::AppState, cwd: &str) -> Option<String> {
    state
        .workspaces
        .iter()
        .find(|ws| ws.node.is_some() && ws.cwd == cwd)
        .and_then(|ws| ws.node.clone())
}

/// The node pane `pane` runs on: the remote panes mounted, else the
/// workspace that holds it (a worker's pane just added, not started yet).
pub fn node_of_pane(state: &crate::state::types::AppState, pane: &str) -> Option<String> {
    super::remote::node_of(pane).or_else(|| {
        state
            .workspaces
            .iter()
            .find(|ws| ws.panes.iter().any(|entry| entry.id == pane))
            .and_then(|ws| ws.node.clone())
    })
}
