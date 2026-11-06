//! What the desktop's orchestration engine does on the node's disk, for a
//! workspace that lives there (docs/native/nodes.md, "Orchestration on a
//! node"): the workers' worktrees and branches, their claims on the board,
//! the messages queued for their sessions, and the deliveries.
//!
//! Each route is one of hangar-core's helpers, called as the desktop calls
//! it for a local workspace, its answer core's own JSON:
//!
//! - `POST /node/worktree` `{repo, path, branch, create?, base?}`:
//!   `git_worktree_add`; `DELETE /node/worktree` `{repo, path, force?}`:
//!   `git_worktree_remove` (no `preRemove` hook, as the engine removes);
//!   `POST /node/worktree/hook` `{repo, path, command}`: the workspace's
//!   `postCreate` in a new worktree; `POST /node/worktree/copy` `{from, to,
//!   patterns}`: the ignored files a worktree is given (`.env`, …);
//! - `DELETE /node/branch` `{repo, branch, force?}`: `git_branch_delete`;
//! - `POST /node/repo` `{name}`: the bare repository the workers a local
//!   workspace sends here work from, `~/hangar-repos/<name>`, made when
//!   missing; the desktop pushes the workers' base branch into it over SSH
//!   and fetches their branches back from it (see [`repo`]);
//! - `POST /node/git` `{op, path, …}`: the read-only questions the engine
//!   asks, and nothing else — no command line is ever taken (see [`GitOp`]);
//! - `POST /node/board/claim-worker`, `/release-worker`, `/update`,
//!   `/comment`: a worker's claim and release, a delivery's moves and
//!   comments, with `board:changed` told as the desktop tells it;
//! - `POST /node/bridge/{pane}` `{text, from?}`: a message queued for a
//!   pane's session (a worker's brief, an orchestrator's news);
//! - `POST /node/delivery/preflight`, `/run`, `/cleanup`, `GET
//!   /node/delivery/running`: core's delivery, run on the node with the
//!   workspace's settings the desktop sends with each one (`checkCommand`,
//!   `reviewerModel`, …).
#![cfg(target_os = "linux")]

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use hangar_core::board::TaskPatch;
use hangar_core::delivery::Delivery;
use hangar_core::Event;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::{error, Shared};

/// A helper's answer: its value as JSON, or its refusal as a 409 (what git
/// said is the desktop's to show, not a fault of the node's).
fn answered<T: Serialize>(outcome: Result<T, String>) -> Response {
    match outcome {
        Ok(value) => Json(json!({ "value": value })).into_response(),
        Err(err) => error(StatusCode::CONFLICT, &err),
    }
}

/// The first of `paths` that is not absolute, refused: the node's helpers
/// run from the daemon's own folder, where a relative path means nothing
/// the desktop meant.
fn not_absolute(paths: &[(&str, &str)]) -> Option<Response> {
    paths
        .iter()
        .find(|(_, path)| path.trim().is_empty() || !std::path::Path::new(path).is_absolute())
        .map(|(name, _)| {
            error(
                StatusCode::BAD_REQUEST,
                &format!("{name} must be an absolute path on the node"),
            )
        })
}

#[derive(Deserialize)]
pub struct AddBody {
    repo: String,
    path: String,
    branch: String,
    /// A new branch from `base` (the engine's case); `false`: check out an
    /// existing one.
    #[serde(default = "yes")]
    create: bool,
    #[serde(default)]
    base: Option<String>,
}

fn yes() -> bool {
    true
}

pub async fn worktree_add(Json(body): Json<AddBody>) -> Response {
    if let Some(refused) = not_absolute(&[("repo", &body.repo), ("path", &body.path)]) {
        return refused;
    }
    if body.branch.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "a worktree needs a branch");
    }
    answered(
        hangar_core::git::git_worktree_add(
            body.repo,
            body.path,
            body.branch,
            body.create,
            body.base,
        )
        .await,
    )
}

#[derive(Deserialize)]
pub struct RemoveBody {
    repo: String,
    path: String,
    #[serde(default)]
    force: bool,
}

pub async fn worktree_remove(Json(body): Json<RemoveBody>) -> Response {
    if let Some(refused) = not_absolute(&[("repo", &body.repo), ("path", &body.path)]) {
        return refused;
    }
    answered(hangar_core::git::git_worktree_remove(body.repo, body.path, body.force, None).await)
}

#[derive(Deserialize)]
pub struct HookBody {
    repo: String,
    path: String,
    command: String,
}

/// The workspace's `postCreate`, which the person wrote in its settings, run
/// in the worktree just made — as the desktop runs it for a local one.
pub async fn worktree_hook(Json(body): Json<HookBody>) -> Response {
    if let Some(refused) = not_absolute(&[("repo", &body.repo), ("path", &body.path)]) {
        return refused;
    }
    answered(hangar_core::git::git_worktree_hook(body.repo, body.path, Some(body.command)).await)
}

#[derive(Deserialize)]
pub struct CopyBody {
    from: String,
    to: String,
    patterns: Vec<String>,
}

pub async fn worktree_copy(Json(body): Json<CopyBody>) -> Response {
    if let Some(refused) = not_absolute(&[("from", &body.from), ("to", &body.to)]) {
        return refused;
    }
    answered(
        hangar_core::git::git_copy_untracked(body.from, body.to, body.patterns, Some(true)).await,
    )
}

#[derive(Deserialize)]
pub struct BranchBody {
    repo: String,
    branch: String,
    #[serde(default)]
    force: bool,
}

pub async fn branch_delete(Json(body): Json<BranchBody>) -> Response {
    if let Some(refused) = not_absolute(&[("repo", &body.repo)]) {
        return refused;
    }
    answered(hangar_core::git::git_branch_delete(body.repo, body.branch, body.force).await)
}

/// Where the repositories of the workers sent from local workspaces live,
/// under the node's home.
pub const REPOS_DIR: &str = "hangar-repos";

#[derive(Deserialize)]
pub struct RepoBody {
    name: String,
}

/// What `POST /node/repo` answers.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Repo {
    /// Absolute, on the node.
    pub path: String,
    /// Made by this call; `false`: it was there already.
    pub created: bool,
}

/// A repository's name, as the desktop gives it: one plain folder name, so
/// it stays inside [`REPOS_DIR`].
fn repo_name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() || name.len() > 100 {
        return Some("name must be 1 to 100 characters");
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Some("name must not start with a dot or a dash");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "._-".contains(ch))
    {
        return Some("name takes letters, digits, dots, dashes and underscores only");
    }
    None
}

/// `<home>/hangar-repos/<name>`, a bare repository, made when it is not
/// there. Bare: nothing is checked out in it, so the desktop's pushes never
/// meet a checked-out branch, and each worker gets a worktree of it.
pub fn ensure_repo(home: &std::path::Path, name: &str) -> Result<Repo, String> {
    if let Some(problem) = repo_name_problem(name) {
        return Err(problem.to_string());
    }
    let path = home.join(REPOS_DIR).join(name);
    let spelled = path.to_string_lossy().into_owned();
    if path.join("HEAD").is_file() && path.join("objects").is_dir() {
        return Ok(Repo {
            path: spelled,
            created: false,
        });
    }
    if path.exists()
        && std::fs::read_dir(&path)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(true)
    {
        return Err(format!(
            "{spelled} exists and is not a bare git repository: move it away first"
        ));
    }
    std::fs::create_dir_all(&path).map_err(|err| format!("could not make {spelled}: {err}"))?;
    let output = std::process::Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("git: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git init --bare {spelled}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Repo {
        path: spelled,
        created: true,
    })
}

pub async fn repo(State(node): State<Shared>, Json(body): Json<RepoBody>) -> Response {
    if let Some(problem) = repo_name_problem(&body.name) {
        return error(StatusCode::BAD_REQUEST, problem);
    }
    let home = node.home().to_path_buf();
    answered(off_runtime(move || ensure_repo(&home, &body.name)).await)
}

/// The read-only questions `POST /node/git` answers: what the engine reads
/// of a repository (git's status, log and diff, through core's helpers).
/// Anything else is refused by name; no git command line is ever taken.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GitOp {
    /// `git_repo_info`: whether it is a repository, its root, its branch.
    RepoInfo { path: String },
    /// `git worktree list --porcelain`.
    WorktreeList { path: String },
    /// `git status` and `git diff --numstat` of the working tree.
    Changes {
        path: String,
        #[serde(default)]
        base: Option<String>,
    },
    /// `git rev-list --count base..head`.
    CommitsAhead {
        path: String,
        base: String,
        head: String,
    },
    /// Whether a folder exists (the engine's `dir_exists`, before it picks
    /// a worktree's folder).
    DirExists { path: String },
}

/// The ops [`GitOp`] knows, for a refusal.
pub const GIT_OPS: &[&str] = &[
    "repo_info",
    "worktree_list",
    "changes",
    "commits_ahead",
    "dir_exists",
];

pub async fn git(body: Result<Json<GitOp>, axum::extract::rejection::JsonRejection>) -> Response {
    let Ok(Json(op)) = body else {
        return error(
            StatusCode::BAD_REQUEST,
            &format!("op must be one of: {}", GIT_OPS.join(", ")),
        );
    };
    match op {
        GitOp::RepoInfo { path } => answered(Ok(hangar_core::git::git_repo_info(path).await)),
        GitOp::WorktreeList { path } => answered(hangar_core::git::git_worktree_list(path).await),
        GitOp::Changes { path, base } => answered(hangar_core::git::git_changes(path, base).await),
        GitOp::CommitsAhead { path, base, head } => {
            answered(hangar_core::git::git_commits_ahead(path, base, head).await)
        }
        GitOp::DirExists { path } => {
            let exists = tokio::task::spawn_blocking(move || hangar_core::store::dir_exists(path))
                .await
                .unwrap_or(false);
            answered(Ok(exists))
        }
    }
}

/// The board views and the orchestrator's events reload on this, which the
/// writes through the agents' API send by themselves and these do not.
fn changed(node: &Shared, cwd: String) {
    let _ = node.host().emit(Event::BoardChanged { cwd });
}

async fn off_runtime<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimBody {
    cwd: String,
    id: String,
    worker: String,
    #[serde(default)]
    shared_folder: bool,
}

pub async fn claim_worker(State(node): State<Shared>, Json(body): Json<ClaimBody>) -> Response {
    let host = node.host().clone();
    answered(
        off_runtime(move || {
            hangar_core::board_claim_worker(
                &host,
                body.cwd,
                body.id,
                body.worker,
                Some(body.shared_folder),
            )
        })
        .await,
    )
}

#[derive(Deserialize)]
pub struct ReleaseBody {
    cwd: String,
    id: String,
    worker: String,
    #[serde(default)]
    undo: bool,
}

pub async fn release_worker(State(node): State<Shared>, Json(body): Json<ReleaseBody>) -> Response {
    let host = node.host().clone();
    answered(
        off_runtime(move || {
            hangar_core::board_release_worker(&host, body.cwd, body.id, body.worker, body.undo)
        })
        .await,
    )
}

#[derive(Deserialize)]
pub struct UpdateBody {
    cwd: String,
    id: String,
    patch: TaskPatch,
}

pub async fn board_update(State(node): State<Shared>, Json(body): Json<UpdateBody>) -> Response {
    let cwd = body.cwd.clone();
    let outcome =
        off_runtime(move || hangar_core::board_update(body.cwd, body.id, body.patch)).await;
    if outcome.is_ok() {
        changed(&node, cwd);
    }
    answered(outcome)
}

#[derive(Deserialize)]
pub struct CommentBody {
    cwd: String,
    id: String,
    author: String,
    text: String,
}

pub async fn board_comment(State(node): State<Shared>, Json(body): Json<CommentBody>) -> Response {
    let cwd = body.cwd.clone();
    let outcome =
        off_runtime(move || hangar_core::board_comment(body.cwd, body.id, body.author, body.text))
            .await;
    if outcome.is_ok() {
        changed(&node, cwd);
    }
    answered(outcome)
}

#[derive(Deserialize)]
pub struct EnqueueBody {
    text: String,
    #[serde(default)]
    from: Option<String>,
}

/// Queued for pane `pane`'s session, submitted once it is ready
/// (`bridge_enqueue`): from `from`, never as the person's.
pub async fn enqueue(
    State(node): State<Shared>,
    Path(pane): Path<String>,
    Json(body): Json<EnqueueBody>,
) -> Response {
    if pane.trim().is_empty() || body.text.is_empty() {
        return error(StatusCode::BAD_REQUEST, "a message needs a pane and a text");
    }
    let host = node.host().clone();
    let queued = off_runtime(move || {
        Ok(hangar_core::bridge::bridge_enqueue(
            &host, pane, body.text, body.from,
        ))
    })
    .await;
    answered(queued)
}

#[derive(Deserialize)]
pub struct PreflightBody {
    workspace: String,
    worktree: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    landing: bool,
    #[serde(default)]
    task: Option<String>,
}

pub async fn delivery_preflight(Json(body): Json<PreflightBody>) -> Response {
    answered(
        hangar_core::delivery::delivery_preflight(
            body.workspace,
            body.worktree,
            body.target,
            Some(body.landing),
            body.task,
        )
        .await,
    )
}

/// A delivery to its end: the merge in a check worktree of the node's, the
/// workspace's check command run there, the review, the landing. Minutes:
/// the desktop waits for the answer as long as core does.
pub async fn delivery_run(Json(delivery): Json<Delivery>) -> Response {
    answered(Ok(hangar_core::delivery::delivery_run(delivery).await))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupBody {
    worktree: String,
    branch: String,
    target: String,
    task_id: String,
}

pub async fn delivery_cleanup(Json(body): Json<CleanupBody>) -> Response {
    answered(Ok(hangar_core::delivery::delivery_cleanup(
        body.worktree,
        body.branch,
        body.target,
        body.task_id,
    )
    .await))
}

pub async fn delivery_running() -> Response {
    answered(off_runtime(|| Ok(hangar_core::delivery::delivery_running())).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repo_is_made_bare_once_and_its_name_stays_in_the_folder() {
        let home = tempfile::tempdir().unwrap();
        let made = ensure_repo(home.path(), "demo-1a2b3c4d").unwrap();
        assert!(made.created);
        let path = home.path().join(REPOS_DIR).join("demo-1a2b3c4d");
        assert_eq!(made.path, path.to_string_lossy());
        assert!(path.join("HEAD").is_file(), "bare: HEAD at its root");
        let again = ensure_repo(home.path(), "demo-1a2b3c4d").unwrap();
        assert!(!again.created);
        for refused in ["", "../x", "a/b", ".hidden", "-x", "a b"] {
            assert!(ensure_repo(home.path(), refused).is_err(), "{refused:?}");
        }
        let taken = home.path().join(REPOS_DIR).join("taken");
        std::fs::create_dir_all(&taken).unwrap();
        std::fs::write(taken.join("file"), "x").unwrap();
        assert!(ensure_repo(home.path(), "taken").is_err());
    }

    #[test]
    fn git_takes_its_known_questions_and_nothing_else() {
        let op: GitOp = serde_json::from_value(
            json!({ "op": "commits_ahead", "path": "/r", "base": "main", "head": "w" }),
        )
        .unwrap();
        assert!(matches!(op, GitOp::CommitsAhead { .. }));
        for refused in [
            json!({ "op": "exec", "path": "/r", "args": ["rm", "-rf", "/"] }),
            json!({ "op": "push", "path": "/r" }),
            json!({ "path": "/r" }),
        ] {
            assert!(serde_json::from_value::<GitOp>(refused).is_err());
        }
    }
}
