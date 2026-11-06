//! The `worker_*` control operations (`lib/workerControl.ts`): an
//! orchestrator starting, listing and stopping the workers of its workspace.
//!
//! A worker is a Claude Code pane on one board task, in a git worktree of its
//! own, with ports of its own and a brief written from the task. Starting one
//! is all or nothing: the claim, the worktree, the pane and the brief either
//! all happen, or whatever was done is undone and the task is back in todo.
//! What would break the work — two tasks in doing that share a file, a
//! workspace with no room for another pane — is refused here and in
//! hangar-core's board.rs, never left to a prompt. How many workers run at
//! once and how many a task gets are not: Hangar sets no limit on either,
//! the orchestrator decides.
//!
//! A worker can be sent to a node (`node`, nodes/away.rs): the branch it
//! starts from is pushed to a repository there, its worktree is made there
//! and its pane runs there. The engine reaches the node's repository with
//! the same git calls as a local one, by its path, which the port routes;
//! the board, the claim and the delivery stay here.
//!
//! A workspace that is not a git repository has no worktree to give: its
//! workers start in its own folder, side by side (`Pane::shared_folder`), and
//! board.rs holds every task that runs beside another to the files it owns.
//!
//! Like the rest of the control ops, what is refused is read by a model:
//! English, and saying what to do next. The strings are the TS's.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

use hangar_core::board::Task;
use hangar_core::git::{
    Changes, CopyResult, DeleteBranchResult, HookReport, RemoveResult, RepoInfo, Worktree,
};

use super::events::{JournalEntry, OrchestratorEvent};
use super::port::{Announcement, Caller, NewPane, WorkerInit};
use crate::state::PaneActivity;
use crate::state::pane_names::{all_pane_names, pick_pane_name};
use crate::state::roles::{
    NEEDS_YOU_LABEL, READY_TO_LAND_LABEL, orchestration_of, orchestrator_of, workers_of,
};
use crate::state::types::{AgentId, AppState, MAX_PANES, Pane, PaneId, PaneRole, Workspace, WsId};

/// How many ports a worker gets: `HANGAR_PORT_COUNT` (pty.rs).
pub const PORT_COUNT: u32 = 20;
/// The pool every workspace's workers share: below the range each system
/// hands out for outgoing connections (Linux from 32768, Windows and macOS
/// from 49152), so a worker's server never meets a port the system lent
/// elsewhere, and short of 27017, which MongoDB listens on by default.
pub const PORT_POOL_START: u32 = 21_000;
const PORT_POOL_END: u32 = 26_999;

/// Branch names tried for one task before giving up: `x`, `x-2`, …
const BRANCH_TRIES: u32 = 9;
/// Folders tried for one branch: `x`, `x-1`, … — VS Code's 20 as well.
const FOLDER_TRIES: u32 = 20;
/// Longest piece of the task's title a branch name carries.
const SLUG_MAX: usize = 32;
/// How long a killed worker's processes get to let go of its worktree.
const SETTLE_MS: u64 = 400;
/// Windows refuses to delete a folder a dying process still stands in.
const REMOVE_TRIES: u32 = 6;
const REMOVE_RETRY_MS: u64 = 500;

/// The files a checkout leaves behind because git ignores them, which a
/// worker's worktree gets copied (`COPY_PATTERNS` of lib/worktrees.ts).
///
/// lib/worktrees.ts belongs to P9, which ports it into
/// `hangar-core/src/worktree_naming.rs`; until that lands, this module keeps
/// its own copy of the three pieces it uses (this list, `worktreeFolder` and
/// `sanitizeBranchName`).
pub const COPY_PATTERNS: [&str; 4] = [".env", ".env.*", "*.local", ".vscode/**"];

/* ── Port ──────────────────────────────────────────────────────── */

/// The starts still under way. They hold their place, their name and their
/// ports from the first check to the end, so two starts at once can neither
/// both take the last free pane of the grid, nor the same name or the same
/// ports.
#[derive(Debug, Default)]
pub struct Starting {
    names: HashSet<String>,
    ports: HashSet<u32>,
    by_workspace: HashMap<WsId, usize>,
}

impl Starting {
    fn in_workspace(&self, id: &str) -> usize {
        self.by_workspace.get(id).copied().unwrap_or(0)
    }
}

/// What the worker ops reach (`ControlDeps` and `WorkerDeps` as
/// workerControl.ts used them, one to one).
pub trait WorkerPort: Sync {
    /// The persisted model as it stands now (`snapshot()`).
    fn state(&self) -> Arc<AppState>;
    fn is_open(&self, workspace_id: &str) -> bool;
    fn activity(&self, pane: &str) -> Option<PaneActivity>;
    /// The starts under way, shared by every op of the app.
    fn starting(&self) -> &Mutex<Starting>;
    /// Whether no process listens on `port` (a block with a busy port is
    /// not handed out).
    fn port_free(&self, port: u32) -> bool {
        port_is_free(port)
    }
    /// What the workspace's orchestrator heard lately (`orchestratorJournal`).
    fn journal(&self, workspace_id: &str) -> Vec<JournalEntry>;
    /// `postOrchestratorEvent`.
    fn post_event(&self, workspace_id: &str, event: OrchestratorEvent);
    fn announce(&self, announcement: Announcement);
    /// The new pane's id, or `None` when the workspace has no room.
    fn add_pane(
        &self,
        workspace_id: &str,
        pane: NewPane,
    ) -> impl Future<Output = Option<PaneId>> + Send;
    fn close_pane(&self, workspace_id: &str, pane: &str) -> impl Future<Output = ()> + Send;
    /// Keeps the workspace awake (hibernation's `touchWorkspace`).
    fn touch(&self, workspace_id: &str);
    fn open_workspace(&self, workspace_id: &str);
    /// Ends a pane's shell and everything it started.
    fn kill_pane(&self, pane: &str) -> impl Future<Output = Result<(), String>> + Send;
    fn alive(&self, pane: &str) -> impl Future<Output = bool> + Send;
    /// `bridge_enqueue`: submitted once the pane's session is ready.
    fn enqueue(
        &self,
        pane: &str,
        text: &str,
        from: &str,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn dir_exists(&self, path: &str) -> impl Future<Output = bool> + Send;
    fn sleep(&self, ms: u64) -> impl Future<Output = ()> + Send;
    fn board_load(&self, cwd: &str) -> impl Future<Output = Result<Vec<Task>, String>> + Send;
    /// board.rs `claim_for_worker`: refused, with the reason, for whatever
    /// stands in the task's way.
    fn claim(
        &self,
        cwd: &str,
        id: &str,
        worker: &str,
        shared_folder: bool,
    ) -> impl Future<Output = Result<Task, String>> + Send;
    /// board.rs `release_worker`.
    fn release(
        &self,
        cwd: &str,
        id: &str,
        worker: &str,
        undo: bool,
    ) -> impl Future<Output = Result<Task, String>> + Send;
    fn repo_info(&self, path: &str) -> impl Future<Output = RepoInfo> + Send;
    fn worktree_list(
        &self,
        path: &str,
    ) -> impl Future<Output = Result<Vec<Worktree>, String>> + Send;
    fn worktree_add(
        &self,
        path: &str,
        worktree_path: &str,
        branch: &str,
        create_branch: bool,
        base_ref: Option<&str>,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn worktree_remove(
        &self,
        path: &str,
        worktree_path: &str,
        force: bool,
    ) -> impl Future<Output = Result<RemoveResult, String>> + Send;
    /// The workspace's postCreate command, run in the new worktree.
    fn worktree_hook(
        &self,
        path: &str,
        worktree_path: &str,
        command: &str,
    ) -> impl Future<Output = Result<Option<HookReport>, String>> + Send;
    fn branch_delete(
        &self,
        path: &str,
        branch: &str,
        force: bool,
    ) -> impl Future<Output = Result<DeleteBranchResult, String>> + Send;
    /// The files git ignores that the patterns name: a worker's `.env`.
    fn copy_ignored(
        &self,
        from: &str,
        to: &str,
        patterns: &[&str],
    ) -> impl Future<Output = Result<CopyResult, String>> + Send;
    /// What is not committed in the working tree holding `path`.
    fn changes(&self, path: &str) -> impl Future<Output = Result<Changes, String>> + Send;
    fn commits_ahead(
        &self,
        path: &str,
        base: &str,
        head: &str,
    ) -> impl Future<Output = Result<u32, String>> + Send;
    /// The nodes connected now, a worker can be sent to: id and label.
    fn nodes(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    /// Node `node`'s repository for this machine's repository at
    /// `main_root`, with `branch` as it is here pushed into it
    /// (`nodes::away::send_branch`): its path on the node. The git calls
    /// above given that path, or a worktree of it, act on the node.
    fn send_branch(
        &self,
        node: &str,
        main_root: &str,
        branch: &str,
    ) -> impl Future<Output = Result<String, String>> + Send {
        let _ = (node, main_root, branch);
        std::future::ready(Err(
            "Workers cannot be sent to a node from this Hangar.".to_string()
        ))
    }
}

/* ── Names, paths and ports ────────────────────────────────────── */

/// The string a JSON argument holds, trimmed, when it holds one (`text`).
fn text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// Strips the `\\?\` a canonical Windows path starts with.
fn unprefixed(path: &str) -> &str {
    path.strip_prefix("\\\\?\\").unwrap_or(path)
}

/// Same folder, the way Windows compares them: either separator, any case.
fn comparable(path: &str) -> String {
    unprefixed(path)
        .trim_end_matches(['\\', '/'])
        .replace('\\', "/")
        .to_lowercase()
}

/// Where `folder` sits inside `root`, in forward slashes: "" for the root
/// itself, `None` outside it. A workspace on a subfolder of its repository
/// has its workers start in the same subfolder of their worktrees.
fn inside(root: &str, folder: &str) -> Option<String> {
    let base = comparable(root);
    let target = comparable(folder);
    if target == base {
        return Some(String::new());
    }
    if !target.starts_with(&format!("{base}/")) {
        return None;
    }
    // Cut from the folder as given, so the subfolder keeps its own case.
    let given: Vec<char> = unprefixed(folder)
        .trim_end_matches(['\\', '/'])
        .replace('\\', "/")
        .chars()
        .collect();
    let keep = target.chars().count() - base.chars().count() - 1;
    Some(given[given.len().saturating_sub(keep)..].iter().collect())
}

fn join_path(root: &str, relative: &str) -> String {
    if relative.is_empty() {
        return root.to_string();
    }
    let separator = if root.contains('\\') { "\\" } else { "/" };
    format!(
        "{}{separator}{}",
        root.trim_end_matches(['\\', '/']),
        relative.split('/').collect::<Vec<_>>().join(separator)
    )
}

/// The worktree a pane works in: the deepest one holding its folder.
fn worktree_of<'a>(trees: &'a [Worktree], folder: &str) -> Option<&'a Worktree> {
    let mut best: Option<&Worktree> = None;
    for tree in trees
        .iter()
        .filter(|tree| inside(&tree.path, folder).is_some())
    {
        if best.is_none_or(|known| tree.path.chars().count() > known.path.chars().count()) {
            best = Some(tree);
        }
    }
    best
}

/// A worktree's folder, spelled the way the pane has it when the pane is at
/// its root.
fn root_of(tree: &Worktree, folder: &str) -> String {
    if inside(&tree.path, folder).as_deref() == Some("") {
        folder.to_string()
    } else {
        tree.path.clone()
    }
}

/// A branch name is not a folder name: `feat/login` would nest a level
/// (`folderName` of lib/worktrees.ts).
fn folder_name(branch: &str) -> String {
    static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\\/:*?"<>|\s]+"#).unwrap());
    UNSAFE.replace_all(branch, "-").into_owned()
}

/// Everything git would refuse in a ref name — and everything a shell would
/// fight over — as a dash (`sanitizeBranchName` of lib/worktrees.ts, itself
/// ported from microsoft/vscode, extensions/git/src/commands.ts, MIT). The
/// pass can leave a leading dash (".env" → "-env"), which git refuses as an
/// option lookalike, so dashes are stripped again after it.
pub fn sanitize_branch_name(name: &str) -> String {
    static REFUSED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\.|/\.|\.\.|~|\^|:|/$|\.lock$|\.lock/|\\|\*|\s|^\s*$|\.$|\[|\]$").unwrap()
    });
    let name = name.trim().trim_start_matches('-');
    let name = REFUSED.replace_all(name, "-");
    name.trim_start_matches('-').to_string()
}

/// `<parent of the main repo>/<repo>-worktrees/<branch>`: siblings of the
/// checkout rather than children of it, so git never sees them as content
/// (`worktreeFolder` of lib/worktrees.ts).
pub fn worktree_folder(main_root: &str, branch: &str) -> String {
    let separator = if main_root.contains('\\') { "\\" } else { "/" };
    let mut segments: Vec<&str> = main_root
        .trim_end_matches(['\\', '/'])
        .split(['\\', '/'])
        .collect();
    let repo = segments.pop().unwrap_or("");
    let siblings = format!("{repo}-worktrees");
    let folder = folder_name(branch);
    segments.push(&siblings);
    segments.push(&folder);
    segments.join(separator)
}

/// Whether nothing listens on `port` on this machine: a bind on loopback and
/// on every interface (IPv4 and IPv6) works. A server of another Hangar
/// process, or one that outlived its worker, holds ports the model knows
/// nothing about. An address family the machine lacks does not count as busy.
pub fn port_is_free(port: u32) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
    let Ok(port) = u16::try_from(port) else {
        return false;
    };
    let addrs = [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
    ];
    addrs.iter().all(|addr| match TcpListener::bind(addr) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::AddrInUse,
    })
}

/// The first block of `PORT_COUNT` ports nobody has, from a pool shared by
/// every workspace of the app: 21000, 21020, 21040… Taken means a pane of any
/// workspace holds it, a start under way reserves it, or `is_free` finds one
/// of its ports listened on by another process. A worker keeps its block for
/// as long as its pane lives, restarts included. `None` when every block is
/// taken (`allocatePortBase`).
pub fn allocate_port_base(
    workspaces: &[Workspace],
    reserved: impl IntoIterator<Item = u32>,
    is_free: impl Fn(u32) -> bool,
) -> Option<u32> {
    let mut taken: Vec<u32> = reserved.into_iter().collect();
    taken.extend(
        workspaces
            .iter()
            .flat_map(|ws| ws.panes.iter())
            .filter_map(|pane| pane.port_base),
    );
    let mut base = PORT_POOL_START;
    while base + PORT_COUNT - 1 <= PORT_POOL_END {
        if taken
            .iter()
            .all(|other| other + PORT_COUNT <= base || base + PORT_COUNT <= *other)
            && (base..base + PORT_COUNT).all(&is_free)
        {
            return Some(base);
        }
        base += PORT_COUNT;
    }
    None
}

/// A task's title as a branch can carry it: ASCII, lower case, dashes, cut
/// at a word (`taskSlug`). Accents go ("écran" is "ecran"); a title with no
/// Latin letter at all leaves "task".
pub fn task_slug(title: &str) -> String {
    let folded: String = title
        .nfd()
        .filter(|ch| !('\u{0300}'..='\u{036f}').contains(ch))
        .collect::<String>()
        .to_lowercase();
    let mut words = String::new();
    let mut dash = false;
    for ch in folded.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if dash && !words.is_empty() {
                words.push('-');
            }
            dash = false;
            words.push(ch);
        } else {
            dash = true;
        }
    }
    if words.chars().count() <= SLUG_MAX {
        return if words.is_empty() {
            "task".into()
        } else {
            words
        };
    }
    let cut: String = words.chars().take(SLUG_MAX).collect();
    let cut = match cut.rfind('-') {
        Some(at) if at >= SLUG_MAX / 2 => cut[..at].to_string(),
        _ => cut,
    };
    cut.trim_end_matches('-').to_string()
}

/// `hangar/<title>-<start of the id>`: whose it is, what it is, and which
/// task (`workerBranch`).
pub fn worker_branch(id: &str, title: &str) -> String {
    let id: String = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .take(6)
        .collect();
    sanitize_branch_name(&format!("hangar/{}-{id}", task_slug(title)))
}

/// One line for a failed hook, with its output after it; `None` when it
/// passed (`hookProblem` of lib/git.ts).
pub fn hook_problem(report: &HookReport) -> Option<String> {
    if report.ok {
        return None;
    }
    let why = match (&report.error, report.timed_out, report.exit_code) {
        (Some(error), _, _) => format!("could not run: {error}"),
        (None, true, _) => "timed out and was stopped".into(),
        (None, false, None) => "was stopped by a signal".into(),
        (None, false, Some(code)) => format!("failed (exit {code})"),
    };
    Some(if report.output.is_empty() {
        why
    } else {
        format!("{why}: {}", report.output)
    })
}

/* ── The brief ─────────────────────────────────────────────────── */

/// The workspace's postCreate command failed in the worktree: what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookFailure {
    pub command: String,
    pub problem: String,
}

/// Everything the brief is written from (`BriefFacts`).
#[derive(Debug, Clone)]
pub struct BriefFacts<'a> {
    pub task_id: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub owns: &'a [String],
    pub worker: &'a str,
    /// `None` when the person started it rather than an orchestrator.
    pub orchestrator: Option<&'a str>,
    pub workspace: &'a str,
    /// Where the worker runs: its worktree, or the workspace's folder without git.
    pub folder: &'a str,
    /// All three `None` in a workspace that is not a git repository.
    pub branch: Option<&'a str>,
    pub target: Option<&'a str>,
    pub worktree: Option<&'a str>,
    pub port_base: u32,
    /// The variables of the workspace's shared build cache are set in the
    /// worker's shell (`build_cache::env_for` gave some).
    pub build_cache: Vec<String>,
    pub check_command: Option<&'a str>,
    pub attempt: u32,
    pub hook_failure: Option<&'a HookFailure>,
    /// A git repository, but the worker was started without a worktree: it
    /// works in the shared checkout, on its current branch.
    pub shared_checkout: bool,
    /// The label of the node it runs on, when it was sent to one.
    pub node: Option<&'a str>,
}

/// The worker's first message, and all it knows: it starts with a fresh
/// context (`workerBrief`). The task's description carries the goal and its
/// acceptance; this says where to work, what not to touch, how to report,
/// when to stop and how to hand the work in.
pub fn worker_brief(facts: &BriefFacts<'_>) -> String {
    let owns: Vec<&str> = facts
        .owns
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let git = match (facts.branch, facts.target, facts.worktree) {
        (Some(branch), Some(target), Some(worktree))
            if !branch.is_empty() && !target.is_empty() && !worktree.is_empty() =>
        {
            Some((branch, target, worktree))
        }
        _ => None,
    };
    let place = match git {
        Some((branch, target, worktree)) if facts.node.is_some() => format!(
            "- You run on the node \"{}\", a machine Hangar reaches over SSH, not on the person's computer. Your folder is a git worktree of its own there: {worktree}, on branch {branch}, made from {target} as it is on the person's computer. Work and commit there only. Never push, and never merge into {target} yourself.",
            facts.node.unwrap_or_default()
        ),
        Some((branch, target, worktree)) => format!(
            "- Your folder is a git worktree of its own: {worktree}, on branch {branch}, made from {target}. Work and commit there only. Never push, and never merge into {target} yourself."
        ),
        None if facts.shared_checkout => format!(
            "- You work in the shared checkout of the workspace: {}, on its current branch, beside other agents who may be changing other files in it right now. Change and commit only your task's files (`git add <those paths>`, never `git add -A`); never checkout, reset, stash or rebase, never revert, move, reformat or delete a file that is not yours, and run nothing that rewrites the whole folder. Never push.",
            facts.folder
        ),
        None => format!(
            "- You work in the workspace's own folder: {}. It is not a git repository: there is no branch and nothing to commit, and what you save is in place at once. Other workers may be changing other files in this folder right now: never revert, move, reformat or delete a file that is not yours, and run nothing that rewrites the whole folder.",
            facts.folder
        ),
    };
    let done = match git {
        Some((branch, _, _)) if facts.node.is_some() => format!(
            "Commit everything on {branch}, leave the comment above, then call delivery_submit {{summary}} with a one-line summary. Hangar fetches your branch back to the person's computer, checks it there, has it reviewed and lands it; if anything fails, a message tells you what to fix."
        ),
        Some((branch, _, _)) => format!(
            "Commit everything on {branch}, leave the comment above, then call delivery_submit {{summary}} with a one-line summary. Hangar checks your branch, has it reviewed and lands it; if anything fails, a message tells you what to fix."
        ),
        None if facts.shared_checkout => "Commit your task's files (`git add <those paths>`), leave the comment above, then call delivery_submit {summary} with a one-line summary. Nothing is merged or reviewed by Hangar: it marks your task done, tells the orchestrator, who integrates your commits, and closes your pane.".to_string(),
        None => "Leave the comment above, then call delivery_submit {summary} with a one-line summary. Nothing is merged or reviewed here: Hangar marks your task done, tells the orchestrator and closes your pane.".to_string(),
    };
    let check = match (facts.check_command.filter(|c| !c.is_empty()), git.is_some()) {
        (Some(command), true) if facts.node.is_some() => {
            format!("- `{command}` passes in your worktree. It runs again on the person's computer, with a review of your diff, when you deliver.")
        }
        (Some(command), true) => {
            format!("- `{command}` passes in your worktree. It runs again, with a review of your diff, when you deliver.")
        }
        (Some(command), false) => format!("- `{command}` passes in the folder."),
        (None, true) => "- The project's own checks pass for what you changed: its tests, its build. A reviewer reads your diff when you deliver.".to_string(),
        (None, false) => "- The project's own checks pass for what you changed, where it has any: its tests, its build.".to_string(),
    };
    let lead = match facts.orchestrator {
        Some(name) => format!("Its orchestrator, {name}, started you"),
        None => "The person started you".to_string(),
    };
    let description = facts.description.trim();
    let lines: Vec<Option<String>> = vec![
        Some(format!(
            "You are {}, a worker in the Hangar workspace \"{}\". {lead} on one board task, and that task is your whole job.",
            facts.worker, facts.workspace
        )),
        Some(String::new()),
        Some(format!("## Task {}: {}", facts.task_id, facts.title)),
        Some(if description.is_empty() { "(No description beyond the title.)".into() } else { description.to_string() }),
        (facts.attempt > 1).then(|| {
            format!(
                "\nThis is attempt {}: an earlier worker did not finish it. Read the task's comments (board_list_tasks) before you start.",
                facts.attempt
            )
        }),
        Some(String::new()),
        Some("## Where you work".into()),
        Some(place),
        Some(if owns.is_empty() {
            "- No files were assigned to this task. Change only what it needs, and name every file you changed in your report.".into()
        } else {
            let listed: Vec<String> = owns.iter().map(|p| format!("`{p}`")).collect();
            format!(
                "- Files you own: {}. Change nothing outside them. If the task cannot be done without another file, name it in a comment on the task and stop.",
                listed.join(", ")
            )
        }),
        facts.hook_failure.map(|failure| {
            format!(
                "- The workspace's postCreate command (`{}`) ran in your worktree and {}. Your worktree is there and your task goes on, but what that command sets up (dependencies, submodules…) may be missing: run what you need yourself, and mention it in your report if it kept you from checking something.",
                failure.command, failure.problem
            )
        }),
        Some(format!(
            "- Your ports: HANGAR_PORT_BASE to HANGAR_PORT_BASE+19 ({} to {}; HANGAR_PORT_BASE and HANGAR_PORT_COUNT are set in your shell). Bind every server, test server and debugger you start to these: other agents run beside you.",
            facts.port_base,
            facts.port_base + PORT_COUNT - 1
        )),
        (!facts.build_cache.is_empty()).then(|| {
            let names: Vec<String> = facts.build_cache.iter().map(|n| format!("`{n}`")).collect();
            format!(
                "- Your build cache: {} {} set in your shell, to a folder the workspace's workers and delivery checks share, so dependencies are built once. Do not override or unset {}, and do not build into a folder of your own.",
                names.join(", "),
                if names.len() == 1 { "is" } else { "are" },
                if names.len() == 1 { "it" } else { "them" }
            )
        }),
        Some("- The task is claimed for you and in doing. Do not claim, move or reassign it: Hangar does.".into()),
        Some(String::new()),
        Some("## Done means".into()),
        Some("- What the task above asks for, its acceptance criteria included.".into()),
        Some(check),
        Some(String::new()),
        Some("## Report".into()),
        Some(format!(
            "Before you stop, whatever the reason, leave one short comment on your task with board_comment_task {{id: \"{}\", text}}: what you did, and each command you ran to check it, with its result.",
            facts.task_id
        )),
        Some(String::new()),
        Some("## When blocked".into()),
        Some("If something is missing — a decision, an access, a file outside yours, a dependency that does not work — say what it is in a comment on the task and stop. Never mock, stub or guess your way past it. If it is something outside the board (a release, a person's decision, a date), board_block_task {id, reason, recheck?} puts the task on hold; then stop, without delivering.".into()),
        Some(String::new()),
        Some("## When done".into()),
        Some(done),
    ];
    lines.into_iter().flatten().collect::<Vec<_>>().join("\n")
}

/* ── Checks ────────────────────────────────────────────────────── */

/// The workspace's orchestrator, when it is the one calling; refused
/// otherwise (`orchestratorCalling`).
fn orchestrator_calling<'a>(
    ws: &'a Workspace,
    caller: &Caller,
    op: &str,
) -> Result<&'a Pane, String> {
    let orchestrator = orchestrator_of(ws);
    let me = caller
        .pane_id
        .as_deref()
        .and_then(|id| ws.panes.iter().find(|pane| pane.id == id));
    if let (Some(orchestrator), Some(me)) = (orchestrator, me)
        && me.id == orchestrator.id
    {
        return Ok(orchestrator);
    }
    if me.is_some_and(|me| me.role == Some(PaneRole::Worker)) {
        return Err(format!(
            "{op} is for the orchestrator of \"{}\", and you are one of its workers. Do the task in your brief; if it needs more hands or other files, say so in a comment on your task (board_comment_task) and stop.",
            ws.name
        ));
    }
    match orchestrator {
        None => Err(format!(
            "{op} is for a workspace's orchestrator, and \"{}\" has none. Only the person can pin one, from the menu of a Claude Code pane; ask them if this workspace should have one.",
            ws.name
        )),
        Some(orchestrator) => Err(format!(
            "{op} is for the orchestrator of \"{}\", which is \"{}\", not you. Leave the work on the board for it, or ask it with pane_send (when_idle=true).",
            ws.name, orchestrator.name
        )),
    }
}

fn model_arg(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(wanted) = text(value) else {
        return Ok(None);
    };
    let allowed = |ch: char| ch.is_ascii_alphanumeric() || "_.:[]-".contains(ch);
    if wanted.chars().count() > 80 || !wanted.chars().all(allowed) {
        return Err(format!(
            "\"{wanted}\" is not a model name. Use an alias — sonnet, opus, haiku — or a full model id such as claude-sonnet-4-5."
        ));
    }
    Ok(Some(wanted))
}

/// The workers its orchestrator still hears about: a new worker given one of
/// their names would read, in the next events, as the one that left.
fn recent_workers(journal: &[JournalEntry]) -> Vec<String> {
    journal
        .iter()
        .filter_map(|entry| entry.event.worker().map(str::to_string))
        .collect()
}

fn name_arg(
    value: Option<&Value>,
    state: &AppState,
    starting: &Starting,
    recent: &[String],
) -> Result<String, String> {
    let wanted = text(value);
    let Some(wanted) = wanted else {
        let taken = all_pane_names(&state.workspaces)
            .into_iter()
            .chain(starting.names.iter().cloned())
            .chain(recent.iter().cloned());
        return Ok(pick_pane_name(taken));
    };
    if wanted.contains(['"', '`', '$', '\r', '\n']) || wanted.chars().count() > 40 {
        return Err(format!(
            "\"{wanted}\" cannot name a pane: keep it under 40 characters, without quotes, backticks or $. Leave name out to get the next free first name."
        ));
    }
    let lower = wanted.to_lowercase();
    let holder = state.workspaces.iter().find(|ws| {
        ws.panes
            .iter()
            .any(|pane| pane.name.trim().to_lowercase() == lower)
    });
    if holder.is_some()
        || starting
            .names
            .iter()
            .any(|name| name.to_lowercase() == lower)
    {
        return Err(format!(
            "A pane is already named \"{wanted}\"{}; pane names are unique across Hangar. Pick another name, or leave name out to get the next free one.",
            holder
                .map(|ws| format!(" (in \"{}\")", ws.name))
                .unwrap_or_default()
        ));
    }
    Ok(wanted)
}

/// The connected node `wanted` names (its id or label), as id and label;
/// refused, with the ones it could be, otherwise.
fn node_arg(
    ws: &Workspace,
    wanted: &Value,
    worktree: Option<bool>,
    port: &impl WorkerPort,
) -> Result<(String, String), String> {
    let connected = port.nodes();
    let listed = if connected.is_empty() {
        "none is connected now (Settings › Nodes)".to_string()
    } else {
        let names: Vec<String> = connected
            .iter()
            .map(|(id, label)| format!("\"{label}\" ({id})"))
            .collect();
        format!("connected now: {}", names.join(", "))
    };
    let Some(wanted) = text(Some(wanted)) else {
        return Err(format!(
            "node is the id or name of a connected node; {listed}. Leave it out to start the worker here."
        ));
    };
    if ws.node.is_some() {
        return Err(format!(
            "\"{}\" lives on a node already, and its workers run there: leave node out.",
            ws.name
        ));
    }
    if worktree == Some(false) {
        return Err(
            "A worker on a node works in a worktree there: leave worktree out, or node.".into(),
        );
    }
    let lower = wanted.to_lowercase();
    connected
        .iter()
        .find(|(id, _)| *id == wanted)
        .or_else(|| {
            connected
                .iter()
                .find(|(_, label)| label.to_lowercase() == lower)
        })
        .cloned()
        .ok_or_else(|| format!("No connected node \"{wanted}\"; {listed}."))
}

/* ── worker_spawn ──────────────────────────────────────────────── */

/// What `worker_spawn` is asked (`{task, name?, model?}`).
#[derive(Debug, Clone, Default)]
pub struct SpawnRequest {
    pub task: Option<Value>,
    pub name: Option<Value>,
    pub model: Option<Value>,
    /// `false`: no worktree, the worker starts in the workspace's own folder.
    pub worktree: Option<Value>,
    /// The id or label of a connected node to run it on.
    pub node: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnedTask {
    pub id: String,
    pub title: String,
    pub attempt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Ports {
    pub base: u32,
    pub count: u32,
}

/// What `worker_spawn` answers (`SpawnedWorker`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpawnedWorker {
    pub worker: String,
    pub task: SpawnedTask,
    pub model: String,
    /// The folder it works in; the three after it are `None` without git.
    pub folder: String,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub worktree: Option<String>,
    pub ports: Ports,
    /// The id of the node it runs on, when it was sent to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub brief: String,
    pub note: String,
}

/// What a failed start undoes, last first.
#[derive(Debug, Clone)]
enum Undo {
    Release,
    /// `repo`: where the worktree was added from — the workspace's folder,
    /// or the node's repository.
    RemoveWorktree {
        repo: String,
        path: String,
        branch: String,
    },
    ClosePane(PaneId),
}

impl Undo {
    fn what(&self) -> String {
        match self {
            Undo::Release => "give the task back".into(),
            Undo::RemoveWorktree { path, .. } => format!("remove the worktree {path}"),
            Undo::ClosePane(_) => "close its pane".into(),
        }
    }
}

/// The parts of a start that stay the same from the checks to the end.
struct Start<'a> {
    ws: &'a Workspace,
    task_id: String,
    name: String,
    model: String,
    port_base: u32,
    by: Option<&'a Pane>,
    /// `Some(false)`: start in the workspace's own folder, with no worktree.
    worktree: Option<bool>,
    /// The node it is sent to: its id and label.
    node: Option<(String, String)>,
}

/// Starts a worker on a task: claim, worktree, pane, brief — all of them, or
/// none (`spawnWorker`). `by` is the orchestrator asking; `None` when the
/// person does it from the window, and the brief then comes from Hangar.
pub async fn spawn_worker(
    ws: &Workspace,
    request: &SpawnRequest,
    port: &impl WorkerPort,
    by: Option<&Pane>,
) -> Result<SpawnedWorker, String> {
    let settings = orchestration_of(ws);
    let state = port.state();
    let worktree = match request.worktree.as_ref() {
        None | Some(Value::Null) => None,
        Some(Value::Bool(wanted)) => Some(*wanted),
        Some(_) => {
            return Err(
                "worktree is true or false: false starts the worker in the workspace's own folder, with no worktree and no branch.".into(),
            );
        }
    };
    let Some(task_id) = text(request.task.as_ref()) else {
        return Err(
            "Give the id of the task to hand over (task), as board_list_tasks shows it.".into(),
        );
    };
    let node = match request.node.as_ref() {
        None | Some(Value::Null) => None,
        Some(wanted) => Some(node_arg(ws, wanted, worktree, port)?),
    };
    if !state.settings.claude_bridge {
        return Err(
            "Hangar's Claude Code bridge is turned off (Settings › Sessions), so a worker would never receive its brief. Ask the person to turn it on."
                .into(),
        );
    }
    // Read before the reservation is held: the journal is the engine's.
    let recent = recent_workers(&port.journal(&ws.id));
    let (name, model, port_base) = {
        // Held from the first check to the reservation, so two starts at once
        // see each other, as the TS's single thread made them.
        let mut starting = port
            .starting()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let pending = starting.in_workspace(&ws.id);
        if ws.pane_count() + pending >= MAX_PANES {
            return Err(full(ws));
        }
        let model =
            model_arg(request.model.as_ref())?.unwrap_or_else(|| settings.worker_model.clone());
        let name = name_arg(request.name.as_ref(), &state, &starting, &recent)?;
        let Some(port_base) = allocate_port_base(
            &state.workspaces,
            starting.ports.iter().copied(),
            |candidate| port.port_free(candidate),
        ) else {
            return Err(
                "Every block of worker ports is taken or has a port in use by another process. Stop a worker you no longer need first."
                    .into(),
            );
        };
        // Held from here to the end, before anything is awaited.
        starting.names.insert(name.clone());
        starting.ports.insert(port_base);
        starting.by_workspace.insert(ws.id.clone(), pending + 1);
        (name, model, port_base)
    };
    let start = Start {
        ws,
        task_id,
        name,
        model,
        port_base,
        by,
        worktree,
        node,
    };
    let mut undo: Vec<Undo> = Vec::new();
    let outcome = start_steps(&start, port, &mut undo).await;
    let outcome = match outcome {
        Ok(spawned) => Ok(spawned),
        Err(message) => {
            let mut failed: Vec<String> = Vec::new();
            for step in undo.iter().rev() {
                if let Err(reason) = run_undo(&start, step, port).await {
                    failed.push(format!("could not {}: {reason}", step.what()));
                }
            }
            Err(if !failed.is_empty() {
                format!(
                    "{message} The start was undone, except that Hangar {}.",
                    failed.join("; ")
                )
            } else if !undo.is_empty() {
                format!("{message} Nothing was left behind: the task is back in todo.")
            } else {
                message
            })
        }
    };
    let mut starting = port
        .starting()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    starting.names.remove(&start.name);
    starting.ports.remove(&start.port_base);
    let left = starting.in_workspace(&ws.id).saturating_sub(1);
    if left > 0 {
        starting.by_workspace.insert(ws.id.clone(), left);
    } else {
        starting.by_workspace.remove(&ws.id);
    }
    outcome
}

fn full(ws: &Workspace) -> String {
    format!(
        "\"{}\" is full: a workspace holds at most {MAX_PANES} panes. Stop a worker or close a pane first.",
        ws.name
    )
}

async fn run_undo(start: &Start<'_>, step: &Undo, port: &impl WorkerPort) -> Result<(), String> {
    let ws = start.ws;
    match step {
        Undo::Release => port
            .release(&ws.cwd, &start.task_id, &start.name, true)
            .await
            .map(drop),
        Undo::RemoveWorktree { repo, path, branch } => {
            remove_worktree(repo, path, port).await?;
            port.branch_delete(repo, branch, true).await.map(drop)
        }
        Undo::ClosePane(id) => {
            let _ = port.kill_pane(id).await;
            port.close_pane(&ws.id, id).await;
            port.sleep(SETTLE_MS).await;
            Ok(())
        }
    }
}

async fn start_steps(
    start: &Start<'_>,
    port: &impl WorkerPort,
    undo: &mut Vec<Undo>,
) -> Result<SpawnedWorker, String> {
    let ws = start.ws;
    let settings = orchestration_of(ws);
    let repo = port.repo_info(&ws.cwd).await;
    // Without git, there is no worktree to give: the worker starts in the
    // workspace's own folder, beside the others, and nothing is merged.
    let git = match (&repo.root, &repo.main_root) {
        (Some(root), Some(main_root)) if repo.installed && repo.is_repo => {
            Some((root.clone(), main_root.clone()))
        }
        _ => None,
    };
    // The orchestrator may start the worker in the workspace's own folder even
    // in a repository (`worktree: false`): no worktree, no branch, no delivery.
    let wants_tree = start.worktree.unwrap_or(true);
    let shared_checkout = git.is_some() && !wants_tree;
    let git = git.filter(|_| wants_tree);
    let target = git
        .as_ref()
        .and_then(|_| settings.branch.clone().or_else(|| repo.branch.clone()));
    if start.node.is_some() && git.is_none() {
        return Err(format!(
            "\"{}\" is not a git repository, so its worker cannot be sent to a node: what it does there comes back through git. Leave node out to start it here.",
            ws.name
        ));
    }
    if git.is_some() && target.is_none() {
        return Err(format!(
            "{} is on a detached HEAD, so there is no branch to start workers from. Ask the person to check one out, or to set the branch in the workspace's orchestration settings.",
            ws.cwd
        ));
    }

    let task = port
        .claim(&ws.cwd, &start.task_id, &start.name, git.is_none())
        .await?;
    undo.push(Undo::Release);

    let mut tree: Option<(String, String)> = None;
    let mut hook_failure: Option<HookFailure> = None;
    let mut folder = ws.cwd.clone();
    if let (Some((root, main_root)), Some(target)) = (&git, &target) {
        // On a node, the worktree is one of the node's repository, which
        // gets the branch it starts from first.
        let (repo, base_root) = match &start.node {
            Some((node, _)) => {
                let repo = port.send_branch(node, main_root, target).await?;
                (repo.clone(), repo)
            }
            None => (ws.cwd.clone(), main_root.clone()),
        };
        let (branch, path) = add_worktree(&repo, &base_root, &task, target, port).await?;
        // A workspace on a subfolder of its repository: the same subfolder there.
        folder = join_path(&path, &inside(root, &ws.cwd).unwrap_or_default());
        undo.push(Undo::RemoveWorktree {
            repo: repo.clone(),
            path: path.clone(),
            branch: branch.clone(),
        });
        // A convenience, as in the worktree panel: a failed copy never undoes
        // the start. The files are this machine's: a worker on a node goes
        // without.
        if start.node.is_none() {
            let _ = port.copy_ignored(root, &path, &COPY_PATTERNS).await;
        }
        // The person's own command, if they set one. Its failure never undoes
        // the start: the worker is told in its brief, and so is the
        // orchestrator.
        let command = port
            .state()
            .workspaces
            .iter()
            .find(|entry| entry.id == ws.id)
            .and_then(|entry| entry.worktree_hooks.as_ref())
            .and_then(|hooks| hooks.post_create.as_deref())
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(str::to_string);
        if let Some(command) = command {
            let report = match port.worktree_hook(&repo, &path, &command).await {
                Ok(report) => report,
                Err(error) => Some(HookReport {
                    ok: false,
                    timed_out: false,
                    exit_code: None,
                    output: String::new(),
                    error: Some(error),
                }),
            };
            if let Some(problem) = report.as_ref().and_then(hook_problem) {
                hook_failure = Some(HookFailure { command, problem });
            }
        }
        tree = Some((branch, path));
    }

    // Read again: the workspace may have changed during the awaits above.
    let state = port.state();
    let Some(current) = state.workspaces.iter().find(|entry| entry.id == ws.id) else {
        return Err(format!("\"{}\" was closed in the meantime.", ws.name));
    };
    let others = workers_of(current);
    let near = others
        .last()
        .copied()
        .or_else(|| orchestrator_of(current))
        .map(|pane| pane.id.clone());
    let pane = NewPane {
        agent: AgentId::Claude,
        cwd: folder.clone(),
        name: start.name.clone(),
        near,
        // In the same write as the pane itself: its terminal mounts with all of it.
        init: Some(WorkerInit {
            role: PaneRole::Worker,
            task_id: task.id.clone(),
            model: start.model.clone(),
            port_base: start.port_base,
            shared_folder: (git.is_none() || shared_checkout).then_some(true),
            node: start.node.as_ref().map(|(node, _)| node.clone()),
        }),
    };
    let Some(id) = port.add_pane(&ws.id, pane).await else {
        return Err(full(ws));
    };
    undo.push(Undo::ClosePane(id.clone()));
    let state = port.state();
    let created = state
        .workspaces
        .iter()
        .find(|entry| entry.id == ws.id)
        .and_then(|entry| entry.panes.iter().find(|pane| pane.id == id));
    if created.is_some_and(|pane| pane.name != start.name) {
        return Err(format!(
            "A pane named \"{}\" appeared in the meantime. Try again.",
            start.name
        ));
    }

    port.touch(&ws.id);
    port.open_workspace(&ws.id);
    let attempt = if task.attempts == 0 { 1 } else { task.attempts };
    let by = start.by.map(|pane| pane.name.as_str());
    let brief = worker_brief(&BriefFacts {
        task_id: &task.id,
        title: &task.title,
        description: &task.description,
        owns: &task.owns,
        worker: &start.name,
        orchestrator: by,
        workspace: &ws.name,
        folder: &folder,
        branch: tree.as_ref().map(|(branch, _)| branch.as_str()),
        target: target.as_deref(),
        worktree: tree.as_ref().map(|(_, path)| path.as_str()),
        port_base: start.port_base,
        // The shared build cache is this machine's: a worker on a node has
        // none (its folder is not this machine's to look into).
        build_cache: hangar_core::build_cache::env_for(
            std::path::Path::new(&ws.cwd),
            &ws.id,
            settings.shared_build_cache && ws.node.is_none() && start.node.is_none(),
        )
        .into_iter()
        .map(|(name, _)| name)
        .collect(),
        check_command: settings.check_command.as_deref(),
        attempt,
        hook_failure: hook_failure.as_ref(),
        shared_checkout,
        node: start.node.as_ref().map(|(_, label)| label.as_str()),
    });
    port.enqueue(&id, &brief, by.unwrap_or("hangar")).await?;

    port.announce(Announcement::Created {
        by: by.map(str::to_string),
        pane: start.name.clone(),
        workspace: ws.name.clone(),
    });
    port.post_event(
        &ws.id,
        OrchestratorEvent::WorkerStarted {
            worker: start.name.clone(),
            task_id: Some(task.id.clone()),
        },
    );
    let mut note = String::new();
    if shared_checkout {
        note.push_str(&format!(
            "The worker was started without a worktree, as asked: it works in \"{}\"'s own folder on the branch checked out there, beside you and any other worker. No branch, nothing merged or reviewed by Hangar: it commits its task's files, its task is done when it delivers, and you integrate. ",
            ws.name
        ));
    } else if git.is_none() {
        note.push_str(&format!(
            "\"{}\" is not a git repository, so the worker works in its folder, beside any other worker: no worktree, no branch, nothing merged or reviewed. Its changes are in place as it saves them, and its task is done when it delivers. ",
            ws.name
        ));
    }
    if let Some((_, label)) = &start.node {
        note.push_str(&format!(
            "It runs on the node \"{label}\", in a worktree of a copy of the repository there made from {}. When it delivers, Hangar fetches its branch back here, where it is checked, reviewed and landed as usual. ",
            target.as_deref().unwrap_or_default()
        ));
    }
    if let Some(failure) = &hook_failure {
        note.push_str(&format!(
            "The workspace's postCreate command ({}) {}; the worker was told in its brief. ",
            failure.command, failure.problem
        ));
    }
    note.push_str(
        "Its comments, its delivery, a wait for the person or an exit reach you as events: do not poll it. worker_list shows where every worker stands.",
    );
    Ok(SpawnedWorker {
        worker: start.name.clone(),
        task: SpawnedTask {
            id: task.id.clone(),
            title: task.title.clone(),
            attempt,
        },
        model: start.model.clone(),
        folder,
        branch: tree.as_ref().map(|(branch, _)| branch.clone()),
        base: target,
        worktree: tree.map(|(_, path)| path),
        ports: Ports {
            base: start.port_base,
            count: PORT_COUNT,
        },
        node: start.node.as_ref().map(|(node, _)| node.clone()),
        brief: "queued: submitted as soon as its Claude Code session is ready".into(),
        note,
    })
}

/// The first of `<path>`, `<path>-1`, `<path>-2`… that does not exist.
async fn free_folder(port: &impl WorkerPort, base: &str) -> Option<String> {
    if !port.dir_exists(base).await {
        return Some(base.to_string());
    }
    for index in 1..=FOLDER_TRIES {
        let candidate = format!("{base}-{index}");
        if !port.dir_exists(&candidate).await {
            return Some(candidate);
        }
    }
    None
}

/// A new branch from `target` and its worktree, beside the main checkout
/// `main_root`, added from `repo` (the workspace's folder, or the node's
/// repository).
async fn add_worktree(
    repo: &str,
    main_root: &str,
    task: &Task,
    target: &str,
    port: &impl WorkerPort,
) -> Result<(String, String), String> {
    let stem = worker_branch(&task.id, &task.title);
    let mut last = String::new();
    for round in 1..=BRANCH_TRIES {
        let branch = if round == 1 {
            stem.clone()
        } else {
            format!("{stem}-{round}")
        };
        let Some(path) = free_folder(port, &worktree_folder(main_root, &branch)).await else {
            continue;
        };
        match port
            .worktree_add(repo, &path, &branch, true, Some(target))
            .await
        {
            Ok(()) => return Ok((branch, path)),
            Err(error) => {
                last = error;
                // A branch left from an earlier attempt, kept for its commits:
                // the next name.
                if last.to_lowercase().contains("already exists") {
                    continue;
                }
                let lower = last.to_lowercase();
                let hint = if lower.contains("invalid reference") || lower.contains("not a valid") {
                    format!(
                        " Check that {target} exists; the person sets the branch workers start from in the workspace's orchestration settings."
                    )
                } else {
                    String::new()
                };
                return Err(format!(
                    "Could not create the worker's worktree at {path} (branch {branch} from {target}): {last}{hint}"
                ));
            }
        }
    }
    Err(format!(
        "Could not find a free branch name for the worker: {stem} and its numbered variants are all taken{}. Delete the old ones from the worktree panel, or ask the person to.",
        if last.is_empty() {
            String::new()
        } else {
            format!(" ({last})")
        }
    ))
}

/// Forced: only called once nothing in it is worth keeping. `repo`: the
/// workspace's folder, or the node's repository.
async fn remove_worktree(repo: &str, path: &str, port: &impl WorkerPort) -> Result<(), String> {
    let mut last: Option<String> = None;
    for attempt in 1..=REMOVE_TRIES {
        match port.worktree_remove(repo, path, true).await {
            Ok(result) if result.removed => return Ok(()),
            Ok(result) => last = result.message,
            Err(error) => last = Some(error),
        }
        if attempt < REMOVE_TRIES {
            port.sleep(REMOVE_RETRY_MS).await;
        }
    }
    Err(last.unwrap_or_else(|| "git did not remove it".into()))
}

/* ── worker_list ───────────────────────────────────────────────── */

/// Where a delivery stands, as far as the board says.
fn delivery_of(task: Option<&Task>, shared_folder: bool) -> Option<&'static str> {
    let task = task?;
    if task.column == "done" {
        return Some(if shared_folder { "done" } else { "landed" });
    }
    if task.labels.iter().any(|label| label == READY_TO_LAND_LABEL) {
        return Some("ready");
    }
    if task.labels.iter().any(|label| label == NEEDS_YOU_LABEL) {
        return Some("needs-you");
    }
    (task.column == "review").then_some("review")
}

/// `worker_list`: each worker with its task, branch, ports and delivery.
pub async fn list_workers(ws: &Workspace, port: &impl WorkerPort) -> Value {
    let panes = workers_of(ws);
    let (tasks, trees) = if panes.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let (tasks, trees) =
            iced::futures::join!(port.board_load(&ws.cwd), port.worktree_list(&ws.cwd));
        (tasks.unwrap_or_default(), trees.unwrap_or_default())
    };
    // The workers on a node: the worktrees of the node's repository.
    let mut away: HashMap<String, Vec<Worktree>> = HashMap::new();
    for pane in &panes {
        let Some((repo, _)) = crate::nodes::away::pane_node(pane)
            .and(pane.cwd.as_deref())
            .and_then(crate::nodes::away::repo_of)
        else {
            continue;
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = away.entry(repo) {
            let listed = port.worktree_list(slot.key()).await.unwrap_or_default();
            slot.insert(listed);
        }
    }
    let nodes = port.nodes();
    let open = port.is_open(&ws.id);
    let mut workers = Vec::new();
    for pane in panes {
        let task = pane
            .task_id
            .as_deref()
            .and_then(|id| tasks.iter().find(|task| task.id == id));
        let shared = pane.shared_folder == Some(true);
        let node = crate::nodes::away::pane_node(pane);
        let listed = node
            .as_ref()
            .and(pane.cwd.as_deref())
            .and_then(crate::nodes::away::repo_of)
            .and_then(|(repo, _)| away.get(&repo))
            .unwrap_or(&trees);
        let tree = match (&pane.cwd, shared) {
            (Some(cwd), false) => worktree_of(listed, cwd),
            _ => None,
        };
        let task_value = match (task, &pane.task_id) {
            (Some(task), _) => {
                json!({"id": task.id, "title": task.title, "column": task.column, "attempts": task.attempts})
            }
            (None, Some(id)) if !id.is_empty() => json!({"id": id, "missing": true}),
            _ => Value::Null,
        };
        let worktree = if shared {
            Value::Null
        } else {
            match (tree, &pane.cwd) {
                (Some(tree), Some(cwd)) => Value::String(root_of(tree, cwd)),
                _ => json!(pane.cwd),
            }
        };
        let running = open && port.alive(&pane.id).await;
        let mut entry = json!({
            "name": pane.name,
            "task": task_value,
            "activity": port.activity(&pane.id).map(|a| a.activity),
            "running": running,
            "branch": tree.and_then(|tree| tree.branch.clone()),
            "worktree": worktree,
            "model": pane.model,
            "ports": pane.port_base.map(|base| json!({"base": base, "count": PORT_COUNT})),
            "delivery": delivery_of(task, shared),
        });
        // No git: it works in the workspace's folder, beside the others.
        if shared {
            entry["folder"] = json!(pane.cwd.clone().unwrap_or_else(|| ws.cwd.clone()));
        }
        if let Some(node) = node {
            let label = nodes
                .iter()
                .find(|(id, _)| *id == node)
                .map(|(_, label)| label.clone());
            entry["node"] = json!({ "id": node, "label": label, "connected": label.is_some() });
        }
        workers.push(entry);
    }
    let mut listing = json!({
        "workspace": ws.name,
        "workers": workers,
    });
    // Where a worker can be sent (worker_spawn node), when anywhere.
    if !nodes.is_empty() {
        listing["nodes"] = json!(
            nodes
                .iter()
                .map(|(id, label)| json!({ "id": id, "label": label }))
                .collect::<Vec<_>>()
        );
    }
    listing
}

/* ── worker_stop ───────────────────────────────────────────────── */

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedTask {
    pub id: String,
    pub title: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedWorktree {
    pub path: Option<String>,
    pub branch: Option<String>,
    pub removed: bool,
    pub branch_deleted: bool,
    /// Why it was kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept: Option<String>,
    /// The node it is on, for a worker sent to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

/// What `worker_stop` answers (`StoppedWorker`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedWorker {
    pub stopped: String,
    pub task: Option<StoppedTask>,
    pub worktree: StoppedWorktree,
}

/// Why a worker's worktree has to stay, or `None` when nothing in it is
/// worth keeping.
async fn work_kept(
    ws: &Workspace,
    repo: &str,
    path: &str,
    tree: &Worktree,
    port: &impl WorkerPort,
) -> Option<String> {
    let Ok(changes) = port.changes(path).await else {
        return Some("its changes could not be read, so nothing was deleted".into());
    };
    if !changes.files.is_empty() {
        let named: Vec<&str> = changes
            .files
            .iter()
            .take(5)
            .map(|file| file.path.as_str())
            .collect();
        let more = changes.files.len() - named.len();
        return Some(format!(
            "it has {} uncommitted change(s): {}{}",
            changes.files.len(),
            named.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ));
    }
    let head = tree.branch.clone().or_else(|| tree.head.clone());
    let target = match orchestration_of(ws).branch {
        Some(branch) => Some(branch),
        None => port.repo_info(&ws.cwd).await.branch,
    };
    let (Some(head), Some(target)) = (head, target) else {
        return Some("there is no branch to compare it with, so nothing was deleted".into());
    };
    let Ok(ahead) = port.commits_ahead(repo, &target, &head).await else {
        return Some(format!(
            "it could not be compared with {target}, so nothing was deleted"
        ));
    };
    (ahead > 0).then(|| {
        format!(
            "{} has {ahead} commit(s) that {target} does not",
            tree.branch.as_deref().unwrap_or(&head)
        )
    })
}

/// Stops a worker: its processes, its pane, its claim on the task, and its
/// worktree and branch when they hold nothing worth keeping (`stopWorker`).
/// `by` is who asked, for the line on screen: the orchestrator, or `None`
/// for the person.
pub async fn stop_worker(
    ws: &Workspace,
    pane: &Pane,
    keep_worktree: bool,
    port: &impl WorkerPort,
    by: Option<&str>,
) -> StoppedWorker {
    let shared = pane.shared_folder == Some(true);
    // A worker on a node: its worktree is one of the node's repository.
    let node = crate::nodes::away::pane_node(pane);
    let repo = node
        .as_ref()
        .and(pane.cwd.as_deref())
        .and_then(crate::nodes::away::repo_of)
        .map_or_else(|| ws.cwd.clone(), |(repo, _)| repo);
    // Read while the worktree still exists: it names the branch.
    let trees = match (&pane.cwd, shared) {
        (Some(_), false) => port.worktree_list(&repo).await.unwrap_or_default(),
        _ => Vec::new(),
    };
    let tree = match (&pane.cwd, shared) {
        (Some(cwd), false) => worktree_of(&trees, cwd),
        _ => None,
    };
    let path = match (tree, &pane.cwd) {
        (Some(tree), Some(cwd)) => Some(root_of(tree, cwd)),
        _ => pane.cwd.clone(),
    };
    // Another pane, of any workspace, works in it: never removed under it.
    let other = tree.and_then(|tree| pane_in(&port.state(), &pane.id, &tree.path));

    // Its processes first: nothing writes in the worktree once it is looked
    // at, and a process still standing in it would keep it from being removed.
    let _ = port.kill_pane(&pane.id).await;
    port.close_pane(&ws.id, &pane.id).await;
    port.touch(&ws.id);

    let task = match pane.task_id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => port.release(&ws.cwd, id, &pane.name, false).await.ok(),
        None => None,
    };

    let mut worktree = StoppedWorktree {
        path: path.clone(),
        branch: tree.and_then(|tree| tree.branch.clone()),
        removed: false,
        branch_deleted: false,
        kept: None,
        node: node.clone(),
    };
    match (&path, tree) {
        _ if shared => {
            worktree.kept = Some(
                "it worked in the workspace's own folder (not a git repository, or started without a worktree): there is no worktree to remove, and what it changed stays in the folder as it is"
                    .into(),
            );
        }
        (Some(_), None) => {
            worktree.kept = Some("git does not list it as a worktree any more".into())
        }
        (None, _) => {}
        (Some(_), Some(tree)) if tree.is_main || inside(&tree.path, &ws.cwd).is_some() => {
            worktree.kept = Some(
                "it is the workspace's own checkout, not one Hangar made for the worker".into(),
            );
        }
        (Some(_), Some(_)) if keep_worktree => {
            worktree.kept = Some("keepWorktree was asked for".into())
        }
        (Some(_), Some(_)) if other.is_some() => {
            worktree.kept = Some(format!(
                "the pane \"{}\" works in it",
                other.unwrap_or_default()
            ));
        }
        (Some(path), Some(tree)) => {
            port.sleep(SETTLE_MS).await;
            match work_kept(ws, &repo, path, tree, port).await {
                Some(reason) => {
                    worktree.kept = Some(format!(
                        "{reason}. Merge or salvage it from there; the person can remove it from the worktree panel."
                    ));
                }
                None => {
                    match remove_worktree(&repo, path, port).await {
                        Ok(()) => worktree.removed = true,
                        // Already one plain line naming what holds it: say it as it is.
                        Err(error) => {
                            let line = error.split_whitespace().collect::<Vec<_>>().join(" ");
                            worktree.kept = Some(format!(
                                "{line} The person can finish from the worktree panel."
                            ));
                        }
                    }
                    if worktree.removed
                        && let Some(branch) = &tree.branch
                    {
                        worktree.branch_deleted = port
                            .branch_delete(&repo, branch, true)
                            .await
                            .map(|result| result.deleted)
                            .unwrap_or(false);
                        if !worktree.branch_deleted {
                            worktree.kept = Some(format!(
                                "the worktree is gone, but the branch {branch} could not be deleted"
                            ));
                        }
                    }
                }
            }
        }
    }

    port.announce(Announcement::Closed {
        by: by.map(str::to_string),
        pane: pane.name.clone(),
        workspace: ws.name.clone(),
    });
    port.post_event(
        &ws.id,
        OrchestratorEvent::WorkerStopped {
            worker: pane.name.clone(),
            task_id: pane.task_id.clone(),
        },
    );
    StoppedWorker {
        stopped: pane.name.clone(),
        task: task.map(|task| StoppedTask {
            id: task.id,
            title: task.title,
            column: task.column,
        }),
        worktree,
    }
}

/// The name of a pane other than `except`, in any workspace, whose folder is
/// `root` or inside it.
fn pane_in(state: &AppState, except: &str, root: &str) -> Option<String> {
    state.workspaces.iter().find_map(|ws| {
        ws.panes
            .iter()
            .filter(|pane| pane.id != except && ws.node.is_none())
            .find(|pane| {
                let folder = pane.cwd.as_deref().unwrap_or(&ws.cwd);
                !folder.is_empty() && inside(root, folder).is_some()
            })
            .map(|pane| pane.name.clone())
    })
}

/// A worker of `ws` by pane name or id.
fn worker_in<'a>(ws: &'a Workspace, reference: Option<&Value>) -> Result<&'a Pane, String> {
    let workers = workers_of(ws);
    let names = if workers.is_empty() {
        "none".to_string()
    } else {
        workers
            .iter()
            .map(|pane| format!("\"{}\"", pane.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let Some(wanted) = text(reference) else {
        return Err(format!(
            "Name the worker to stop. Workers of \"{}\": {names}.",
            ws.name
        ));
    };
    let lower = wanted.to_lowercase();
    let matches = |pane: &&Pane| pane.id == wanted || pane.name.trim().to_lowercase() == lower;
    if let Some(found) = workers.iter().copied().find(matches) {
        return Ok(found);
    }
    if ws.panes.iter().any(|pane| matches(&pane)) {
        return Err(format!(
            "\"{wanted}\" is not a worker: worker_stop only stops workers. Workers of \"{}\": {names}.",
            ws.name
        ));
    }
    Err(format!(
        "No worker \"{wanted}\" in \"{}\". Workers: {names}.",
        ws.name
    ))
}

/// The `worker_*` ops, for a workspace the control bus already resolved
/// (`workerControl`).
pub async fn worker_control(
    op: &str,
    ws: &Workspace,
    args: &Value,
    caller: &Caller,
    port: &impl WorkerPort,
) -> Result<Value, String> {
    match op {
        "worker_spawn" => {
            let me = orchestrator_calling(ws, caller, op)?;
            let request = SpawnRequest {
                task: args.get("task").cloned(),
                name: args.get("name").cloned(),
                model: args.get("model").cloned(),
                worktree: args.get("worktree").cloned(),
                node: args.get("node").cloned(),
            };
            let spawned = spawn_worker(ws, &request, port, Some(me)).await?;
            serde_json::to_value(spawned).map_err(|err| err.to_string())
        }
        "worker_list" => {
            orchestrator_calling(ws, caller, op)?;
            Ok(list_workers(ws, port).await)
        }
        "worker_stop" => {
            let me = orchestrator_calling(ws, caller, op)?;
            let pane = worker_in(ws, args.get("worker"))?;
            let keep = args.get("keepWorktree") == Some(&Value::Bool(true));
            let stopped = stop_worker(ws, pane, keep, port, Some(&me.name)).await;
            serde_json::to_value(stopped).map_err(|err| err.to_string())
        }
        _ => Err(format!("Unknown operation \"{op}\".")),
    }
}

/// Stops every worker of a workspace: the person's « Arrêter les workers »
/// (`stopAllWorkers`). Answers the number stopped; one that fails does not
/// keep the others going.
pub async fn stop_all_workers(workspace_id: &str, port: &impl WorkerPort) -> usize {
    let state = port.state();
    let Some(current) = state
        .workspaces
        .iter()
        .find(|ws| ws.id == workspace_id)
        .cloned()
    else {
        return 0;
    };
    let mut stopped = 0;
    for pane in workers_of(&current) {
        let state = port.state();
        let fresh = state
            .workspaces
            .iter()
            .find(|ws| ws.id == workspace_id)
            .unwrap_or(&current);
        stop_worker(fresh, pane, false, port, None).await;
        stopped += 1;
    }
    stopped
}

#[cfg(test)]
mod tests;
