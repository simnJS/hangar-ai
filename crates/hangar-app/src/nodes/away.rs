//! A local workspace's workers sent to a node (docs/native/nodes.md, "Later":
//! "workers sent from a local workspace to a node, on the same base"): the
//! worker runs on the node, in a worktree of a repository there, and its
//! branch comes back to this machine's repository for the usual delivery.
//!
//! How the code gets there and back, decided: over the node's own SSH, the
//! way the connection reaches it (`commands::git_ssh_command`), never a
//! clone of the repository's remote. A remote may need credentials the node
//! does not have, may not be reachable from it, and does not hold the
//! commits not pushed yet that the worker has to start from; this machine
//! has all of them, and the node is reached already. So:
//!
//! - the node keeps one bare repository per local repository,
//!   `~/hangar-repos/<name>-<hash>` (`POST /node/repo` makes it once):
//!   bare, so a push never meets a checked-out branch;
//! - a worker's start pushes the branch it starts from into it
//!   (`git push <target>:<repo> refs/heads/<b>:refs/heads/<b>`, forced: the
//!   node's copy only follows this one), and its worktree is made there,
//!   `<repo>-worktrees/<branch>`, by the node (`/node/worktree`);
//! - its delivery fetches its branch back
//!   (`git fetch <target>:<repo> +refs/heads/<w>:refs/heads/<w>`) and checks
//!   it out here, in the worktree a local worker would have had
//!   (`worktree_folder`): core's delivery then merges, checks, reviews and
//!   lands it from there as it does any worker's, unchanged. That checkout
//!   is Hangar's, nobody works in it: it is made again at each delivery, and
//!   it goes with the branch once the work lands.
//!
//! Which node a pane runs on is the pane's `node` (in its JSON, as the
//! workspace's is). Until the pane model holds it as a field, it rides in
//! the pane's extra fields under that same name, which the field will read.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, RwLock};

use hangar_core::delivery::Cleanup;
use serde_json::{Value, json};

use crate::state::types::{AppState, NodeConfig, Pane};

use super::commands::{self, Cmd};
use super::engine;
use super::model::NodeClient;
use super::runner::Runner;
use super::wire;

/// The pane's JSON field naming the node it runs on.
pub const PANE_NODE: &str = "node";

/// The node pane `pane` runs on, when it is one sent from a local workspace.
pub fn pane_node(pane: &Pane) -> Option<String> {
    pane.extra
        .get(PANE_NODE)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|node| !node.is_empty())
        .map(str::to_string)
}

/// Pane `pane` runs on node `node`.
pub fn set_pane_node(pane: &mut Pane, node: &str) {
    pane.extra.insert(PANE_NODE.into(), json!(node));
}

/// The label of a node: its name, else its SSH target.
pub fn label_of(node: &NodeConfig) -> String {
    if node.label.trim().is_empty() {
        node.ssh_target.trim().to_string()
    } else {
        node.label.trim().to_string()
    }
}

/// FNV-1a, 32 bits: a short name that stays the same from one run to the
/// next, which std's hasher does not promise.
fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The name of the node's repository for this machine's repository at
/// `main_root`: its folder's name, then a hash of its path, so two
/// checkouts named alike never share one.
pub fn repo_name(main_root: &str) -> String {
    let spelled = main_root.trim_end_matches(['/', '\\']);
    let folder = spelled.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name: String = folder
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || "._-".contains(ch) {
                ch
            } else {
                '-'
            }
        })
        .take(60)
        .collect();
    name = name.trim_start_matches(['.', '-']).to_string();
    if name.is_empty() {
        name = "repo".into();
    }
    let key = spelled.replace('\\', "/").to_lowercase();
    format!("{name}-{:08x}", fnv(&key))
}

/// The node's repository and the worktree root a worker's folder there is
/// in: `<repo>-worktrees/<folder>[/<subfolder>]` gives `(<repo>,
/// <repo>-worktrees/<folder>)`. `None` for a folder Hangar did not lay out.
pub fn repo_of(folder: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = folder.split('/').collect();
    let at = parts
        .iter()
        .enumerate()
        .rev()
        .find(|(index, part)| {
            *index > 0 && part.len() > "-worktrees".len() && part.ends_with("-worktrees")
        })
        .map(|(index, _)| index)?;
    if at + 1 >= parts.len() || parts[at + 1].is_empty() {
        return None;
    }
    let mut repo: Vec<&str> = parts[..at].to_vec();
    let name = parts[at].strip_suffix("-worktrees")?;
    repo.push(name);
    Some((repo.join("/"), parts[..=at + 1].join("/")))
}

/// The node's repositories sent to this run, by path: what the engine's git
/// calls on a worker's repository and worktree find their node by, before
/// its pane exists. The process's, as the tunnels are.
static REPOS: LazyLock<RwLock<HashMap<String, String>>> = LazyLock::new(Default::default);

/// Repository `repo` is on node `node`.
pub fn remember(node: &str, repo: &str) {
    REPOS
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(repo.to_string(), node.to_string());
}

/// `path` is repository `repo` or in one of its workers' worktrees.
fn within(repo: &str, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    path == repo || path.starts_with(&format!("{repo}-worktrees/"))
}

/// The node `path` is on, when it is the repository of workers sent from a
/// local workspace, or in one of their worktrees: one sent this run, or one
/// a pane of `state` runs in (after a restart).
pub fn node_of_path(state: &AppState, path: &str) -> Option<String> {
    let known = REPOS
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .find(|(repo, _)| within(repo, path))
        .map(|(_, node)| node.clone());
    known.or_else(|| {
        state
            .workspaces
            .iter()
            .flat_map(|ws| ws.panes.iter())
            .find_map(|pane| {
                let node = pane_node(pane)?;
                let (repo, _) = repo_of(pane.cwd.as_deref()?)?;
                within(&repo, path).then_some(node)
            })
    })
}

/// The bare repository for `name` on the node, made when missing: its
/// absolute path there.
pub async fn ensure_repo(client: &NodeClient, name: &str) -> Result<String, String> {
    let answer = wire::request_within(
        client,
        "POST",
        "/node/repo",
        Some(&json!({ "name": name })),
        engine::GIT_TIMEOUT,
    )
    .await?
    .json()?;
    answer
        .pointer("/value/path")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "POST /node/repo: no path in the answer".into())
}

/// `cmd` run to its end, its error output as the failure.
async fn run(runner: &impl Runner, cmd: Cmd) -> Result<(), String> {
    let said = runner.run(cmd.clone()).await?;
    if said.success {
        return Ok(());
    }
    let said = said.stderr.trim();
    Err(format!(
        "{} {} failed{}",
        cmd.program,
        cmd.args
            .iter()
            .find(|arg| matches!(arg.as_str(), "push" | "fetch"))
            .map_or("", String::as_str),
        if said.is_empty() {
            String::new()
        } else {
            format!(": {said}")
        }
    ))
}

/// Before a worker starts on node `node`: the node's repository for this
/// machine's repository at `main_root`, with `branch` as it is here. Its
/// path on the node.
pub async fn send_branch(
    runner: &impl Runner,
    node: &NodeConfig,
    client: &NodeClient,
    main_root: &str,
    branch: &str,
) -> Result<String, String> {
    let repo = ensure_repo(client, &repo_name(main_root)).await?;
    run(runner, commands::push_branch(node, main_root, &repo, branch))
        .await
        .map_err(|error| {
            format!(
                "Could not send {branch} to node \"{}\" ({error}). Check that git is installed there and that `ssh {}` works without a prompt.",
                label_of(node),
                node.ssh_target.trim()
            )
        })?;
    Ok(repo)
}

/// What a delivery from a node brought back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroughtBack {
    /// The checkout here the delivery runs from.
    pub worktree: String,
    pub branch: String,
}

/// A delivery of a worker on node `node`, whose worktree there is
/// `worktree`: its branch fetched into this machine's repository (the one
/// of `workspace`, its folder here) and checked out where a local worker's
/// would be. `landing` (the person landing what waited) skips the check for
/// uncommitted changes, as core's preflight does.
pub async fn bring_back(
    runner: &impl Runner,
    node: &NodeConfig,
    client: &NodeClient,
    workspace: &str,
    worktree: &str,
    landing: bool,
) -> Result<BroughtBack, String> {
    let Some((repo, root)) = repo_of(worktree) else {
        return Err(format!(
            "{worktree} on node \"{}\" is not a worktree Hangar made, so its branch cannot be brought back.",
            label_of(node)
        ));
    };
    let info = engine::repo_info(client, &root).await;
    if !info.is_repo {
        return Err(format!(
            "Node \"{}\" did not answer about {root}: it may be unreachable, or the worktree gone. Try again once it is connected.",
            label_of(node)
        ));
    }
    let Some(branch) = info.branch.filter(|branch| !branch.is_empty()) else {
        return Err(format!(
            "{root} is on a detached HEAD: check out your branch, commit on it, then call delivery_submit again."
        ));
    };
    if !landing {
        let changes = engine::changes(client, &root).await?;
        if !changes.files.is_empty() {
            let named: Vec<&str> = changes
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            return Err(format!(
                "Your worktree has uncommitted changes: {}. Only commits are delivered: commit them on {branch} (or delete what is not part of the task), then call delivery_submit again.",
                named.join(", ")
            ));
        }
    }
    let here = hangar_core::git::git_repo_info(workspace.to_string()).await;
    let Some(main_root) = here.main_root.filter(|_| here.is_repo) else {
        return Err(format!(
            "The workspace folder {workspace} is not a git repository, so there is nothing to land on."
        ));
    };
    let checkout = crate::orchestration::worker::worktree_folder(&main_root, &branch);
    // The checkout of the last delivery goes first: git does not fetch into
    // a branch checked out somewhere.
    let listed = hangar_core::git::git_worktree_list(main_root.clone()).await?;
    let same = |path: &str| {
        path.replace('\\', "/").trim_end_matches('/')
            == checkout.replace('\\', "/").trim_end_matches('/')
    };
    if let Some(held) = listed
        .iter()
        .find(|tree| tree.branch.as_deref() == Some(branch.as_str()))
    {
        if !same(&held.path) {
            return Err(format!(
                "{branch} is checked out at {} on this computer, so it cannot be brought back from the node. Ask the person to free it.",
                held.path
            ));
        }
        let removed =
            hangar_core::git::git_worktree_remove(main_root.clone(), held.path.clone(), true, None)
                .await?;
        if !removed.removed {
            return Err(format!(
                "Could not make room for {branch} at {}: {}",
                held.path,
                removed.message.unwrap_or_default()
            ));
        }
    } else if Path::new(&checkout).exists() {
        return Err(format!(
            "{checkout} exists on this computer and is not {branch}'s worktree: ask the person to move it away."
        ));
    }
    run(
        runner,
        commands::fetch_branch(node, &main_root, &repo, &branch),
    )
    .await
    .map_err(|error| {
        format!(
            "Could not fetch {branch} from node \"{}\": {error}",
            label_of(node)
        )
    })?;
    hangar_core::git::git_worktree_add(
        main_root.clone(),
        checkout.clone(),
        branch.clone(),
        false,
        None,
    )
    .await
    .map_err(|error| format!("Could not check out {branch} at {checkout}: {error}"))?;
    Ok(BroughtBack {
        worktree: checkout,
        branch,
    })
}

/// After a landing: the worker's worktree and branch on the node go. Its
/// work is in this machine's repository and landed, so both are forced.
pub async fn clean_node(client: &NodeClient, worktree: &str, branch: &str) -> Cleanup {
    let mut report = Cleanup {
        worktree_removed: false,
        branch_deleted: false,
        kept: Vec::new(),
    };
    let Some((repo, root)) = repo_of(worktree) else {
        report.kept.push(format!(
            "{worktree} on the node is not a worktree Hangar made: kept"
        ));
        return report;
    };
    match engine::worktree_remove(client, &repo, &root, true).await {
        Ok(removed) if removed.removed => report.worktree_removed = true,
        Ok(removed) => report.kept.push(format!(
            "the worktree {root} on the node could not be removed: {}",
            removed.message.unwrap_or_default()
        )),
        Err(error) => report.kept.push(format!(
            "the worktree {root} on the node could not be removed: {error}"
        )),
    }
    match engine::branch_delete(client, &repo, branch, true).await {
        Ok(deleted) if deleted.deleted => report.branch_deleted = true,
        Ok(deleted) => report.kept.push(format!(
            "the branch {branch} on the node could not be deleted: {}",
            deleted.message.unwrap_or_default()
        )),
        Err(error) => report.kept.push(format!(
            "the branch {branch} on the node could not be deleted: {error}"
        )),
    }
    report
}

#[cfg(test)]
#[path = "away_tests.rs"]
mod tests;
