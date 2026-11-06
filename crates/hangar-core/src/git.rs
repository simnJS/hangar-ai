//! Git worktrees, the app's answer to "several agents, one repository".
//!
//! Two agents editing the same checkout step on each other's toes; a linked
//! worktree gives each of them its own directory and its own branch while the
//! history stays shared. Everything here shells out to the `git` binary rather
//! than linking a library: it is the git the user already has, with their
//! config, their credential helper and their hooks, and it is one dependency
//! the installer does not grow.
//!
//! Nothing in here is allowed to be fatal to the UI. A machine without git, or
//! a workspace that is not a repository, is a perfectly normal way to use the
//! app — `git_repo_info` reports it as data so the frontend can hide the
//! feature rather than show an error nobody asked for.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Hides the console window Windows would otherwise flash for every git call.
/// The app is a GUI process with no console of its own, so each child gets one
/// created for it unless asked otherwise.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const NOT_INSTALLED: &str = "git was not found on PATH";

/// Why a git call produced no answer.
///
/// The two cases have to be told apart rather than collapsed into a string:
/// "there is no git on this machine" and "this folder is not a repository"
/// look identical to a caller comparing messages, and `git_repo_info` reports
/// them as two different fields.
#[derive(Debug)]
enum GitError {
    Missing,
    /// git ran and refused, carrying its own diagnostic.
    Failed(String),
}

impl From<GitError> for String {
    fn from(error: GitError) -> Self {
        match error {
            GitError::Missing => NOT_INSTALLED.to_string(),
            GitError::Failed(message) => message,
        }
    }
}

/// Runs `git -C <dir> <args>` and hands back its stdout.
///
/// `-C` rather than `Command::current_dir`: git resolves it itself, which keeps
/// the Windows path in git's hands instead of the process launcher's, and it
/// makes every call here self-describing — the directory is part of the command
/// line we would type by hand to reproduce it.
fn run_git(dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let mut command = git_command(dir, args);
    command.stdin(Stdio::null());
    let limit = network_limit(args);
    let output = run_limited(command, limit).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => GitError::Missing,
        std::io::ErrorKind::TimedOut => GitError::Failed(format!(
            "`git {}` did not finish within {} seconds and was stopped — is a remote asking for \
             credentials?",
            args.first().copied().unwrap_or(""),
            limit.map_or(0, |limit| limit.as_secs())
        )),
        _ => GitError::Failed(format!("could not run git: {err}")),
    })?;

    if !output.status.success() {
        return Err(GitError::Failed(diagnostic(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// How long a command that talks to a remote may run. A fetch that waits for
/// a passphrase nobody can type would otherwise hold its thread for good.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);

fn network_limit(args: &[&str]) -> Option<Duration> {
    matches!(
        args.first().copied(),
        Some("fetch" | "pull" | "push" | "clone" | "ls-remote")
    )
    .then_some(NETWORK_TIMEOUT)
}

/// The command `run_git` runs: `git -C <dir> <args>`, set so that nothing
/// waits for a person and nothing of the pane leaks into git's hooks.
fn git_command(dir: &Path, args: &[&str]) -> Command {
    // Without this a bundled macOS app looks at the bare `launchd` PATH, where
    // a Homebrew git does not exist. See path_env.rs.
    crate::path_env::ensure();

    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        // git translates its diagnostics, and two of the decisions below are
        // taken by reading them — whether a removal only needs confirming, and
        // whether a `rev-parse` is worth retrying in another spelling. Pinning
        // the language is what keeps those checks working on a localised
        // install. `LANGUAGE` has to go too: gettext lets it override `LC_ALL`.
        .env("LC_ALL", "C")
        .env("LANGUAGE", "")
        // Nobody is there to type a password or a passphrase.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    crate::delivery::scrub(&mut command);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// `Command::output`, with a time limit: past it the process and what it
/// started are killed and the error is `TimedOut`. `None`: no limit.
fn run_limited(
    mut command: Command,
    limit: Option<Duration>,
) -> std::io::Result<std::process::Output> {
    let Some(limit) = limit else {
        return command.output();
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let read = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut stream) = stream {
                let _ = stream.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let out = read(child.stdout.take().map(|s| Box::new(s) as _));
    let err = read(child.stderr.take().map(|s| Box::new(s) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if started.elapsed() >= limit => {
                kill_tree(&mut child);
                let _ = child.wait();
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    Ok(std::process::Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// Runs a git-shaped task on the blocking pool and hands its answer back.
///
/// Every command below goes through this because a synchronous command runs
/// where it is called — the Tauri window called them on the main thread, the
/// one the window, and every pane's IPC, lived on. A `git fetch` against a
/// slow remote, or the checkout a `worktree add` performs, would freeze the
/// whole app for its duration; voice_start draws the same line for the same
/// reason.
pub(crate) async fn off_thread<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|e| e.to_string())?
}

/// The one line of a failed git call worth showing.
///
/// git mixes progress into stderr ahead of the failure — `worktree add` opens
/// with "Preparing worktree (…)" — so the first line is regularly not the
/// problem, and the usage dump that follows an unknown option is never it. The
/// line carrying git's own prefix is the diagnostic; failing that, the last
/// thing it said before giving up.
fn diagnostic(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let message = lines
        .iter()
        .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
        .or_else(|| lines.last())
        .copied()
        .unwrap_or_default();

    let message = message
        .strip_prefix("fatal:")
        .or_else(|| message.strip_prefix("error:"))
        .unwrap_or(message)
        .trim();

    if message.is_empty() {
        "git failed without saying why".to_string()
    } else {
        message.to_string()
    }
}

/// A path in the form the rest of the app compares them in.
///
/// Windows is case-insensitive and accepts either separator, and git answers in
/// forward slashes there while a path typed by the user arrives in backslashes.
/// Comparing the two literally would report that nobody is standing in any
/// worktree at all.
///
/// Both sides are first resolved to the spelling the file system itself gives
/// the directory (`/var` → `/private/var` on macOS, `RUNNER~1` → `runneradmin`
/// on Windows, a junction or symbolic link → its target). git and the panes do
/// not always spell a folder the same way, and a literal comparison would
/// refuse a sweep or miss a held worktree.
fn compare_key(path: &str) -> String {
    let resolved = resolve_path(path);
    #[cfg(windows)]
    let key = resolved
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    #[cfg(not(windows))]
    let key = resolved.trim_end_matches('/').to_string();
    key
}

/// `path` with its deepest existing ancestor canonicalised and the rest, which
/// may not exist (yet, or any more), appended unchanged. A path that cannot be
/// resolved at all comes back as it was.
fn resolve_path(path: &str) -> String {
    let given = Path::new(path);
    if !given.has_root() {
        return path.to_string();
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut base = given.to_path_buf();
    loop {
        if let Ok(real) = std::fs::canonicalize(&base) {
            let mut real = strip_verbatim(real);
            for part in tail.iter().rev() {
                real.push(part);
            }
            return display(&real);
        }
        match (base.file_name().map(|name| name.to_owned()), base.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                base = parent.to_path_buf();
            }
            _ => return path.to_string(),
        }
    }
}

/// Drops the `\\?\` Windows verbatim prefix `fs::canonicalize` adds.
///
/// Nothing else in the app — or on screen — spells paths that way, and git
/// itself never does, so a canonicalised path would compare unequal to every
/// other form of the same directory.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest.to_owned());
        }
    }
    path
}

/// Resolves what `rev-parse` printed against the directory it was run in.
///
/// Only the older spelling below needs it: without `--path-format=absolute`,
/// git prints the git directories relative to the working directory, which is
/// `dir`. An answer that is already absolute is left exactly as git wrote it.
fn absolutize(dir: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let joined = dir.join(path);
    std::fs::canonicalize(&joined)
        .map(strip_verbatim)
        .unwrap_or(joined)
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Where a repository keeps its three defining directories.
struct Layout {
    /// The working tree containing the queried path. None for a bare
    /// repository, which has none.
    root: Option<PathBuf>,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

/// `rev-parse` spellings, most informative first.
///
/// The first is the only one normally used. Two situations make it fail on a
/// directory that really is a repository, and each is covered by dropping the
/// part git objected to: a bare repository has no working tree to name, and a
/// git older than 2.31 does not know `--path-format` at all.
const LAYOUT_QUERIES: [&[&str]; 4] = [
    &[
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-dir",
        "--git-common-dir",
    ],
    &[
        "rev-parse",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
    ],
    &[
        "rev-parse",
        "--show-toplevel",
        "--git-dir",
        "--git-common-dir",
    ],
    &["rev-parse", "--git-dir", "--git-common-dir"],
];

/// Failures another spelling can still answer. Anything else — no repository
/// here, no such directory — is final, and retrying it three more times only
/// costs three more process spawns on the most common path of all: a workspace
/// that is not under git.
fn worth_retrying(message: &str) -> bool {
    message.contains("must be run in a work tree") || message.contains("path-format")
}

fn parse_layout(dir: &Path, output: &str) -> Option<Layout> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    // `rev-parse` is also git's option parser for scripts: a flag it does not
    // know is echoed to stdout with exit 0 instead of being refused. On a git
    // without `--path-format` the second spelling would then hand back three
    // lines whose first is the literal option — read as the worktree root.
    // No real answer starts with a dash: these lines are absolute paths, or
    // `.git`-style relatives on the oldest spelling.
    if lines.iter().any(|line| line.starts_with('-')) {
        return None;
    }

    match lines.as_slice() {
        [root, git_dir, common_dir] => Some(Layout {
            root: Some(absolutize(dir, root)),
            git_dir: absolutize(dir, git_dir),
            common_dir: absolutize(dir, common_dir),
        }),
        [git_dir, common_dir] => Some(Layout {
            root: None,
            git_dir: absolutize(dir, git_dir),
            common_dir: absolutize(dir, common_dir),
        }),
        _ => None,
    }
}

fn layout(dir: &Path) -> Result<Layout, GitError> {
    let mut last = GitError::Failed("git said nothing about this directory".to_string());

    for query in LAYOUT_QUERIES {
        match run_git(dir, query) {
            Ok(output) => match parse_layout(dir, &output) {
                Some(layout) => return Ok(layout),
                None => last = GitError::Failed("git described this directory oddly".to_string()),
            },
            Err(GitError::Missing) => return Err(GitError::Missing),
            Err(GitError::Failed(message)) => {
                let retry = worth_retrying(&message);
                last = GitError::Failed(message);
                if !retry {
                    break;
                }
            }
        }
    }
    Err(last)
}

/// The branch name at HEAD, or None when HEAD is detached or unreachable.
fn head_branch(dir: &Path) -> Option<String> {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|out| out.trim().to_string())
        // git spells a detached HEAD by naming it, which is not a branch.
        .filter(|name| !name.is_empty() && name != "HEAD")
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub installed: bool,
    pub is_repo: bool,
    pub root: Option<String>,
    pub main_root: Option<String>,
    pub is_linked_worktree: bool,
    pub branch: Option<String>,
    pub head: Option<String>,
}

/// A linked worktree keeps its own git directory under the main repository's,
/// and only shares the common one.
fn is_linked_worktree(layout: &Layout) -> bool {
    compare_key(&display(&layout.git_dir)) != compare_key(&display(&layout.common_dir))
}

/// The common dir is the main repository's `.git`, so the repository itself
/// is its parent. A bare repository has no such wrapper: it *is* the common
/// dir, and there is nothing above it to point at.
fn main_root(layout: &Layout) -> Option<String> {
    if layout.common_dir.file_name() == Some(OsStr::new(".git")) {
        layout.common_dir.parent().map(display)
    } else {
        Some(display(&layout.common_dir))
    }
}

fn repo_info(dir: &Path) -> RepoInfo {
    let layout = match layout(dir) {
        Ok(layout) => layout,
        // Both of these are ordinary states, not failures: the frontend hides
        // the worktree UI for the first and offers `git init` for the second.
        Err(GitError::Missing) => return RepoInfo::default(),
        Err(GitError::Failed(_)) => {
            return RepoInfo {
                installed: true,
                ..RepoInfo::default()
            }
        }
    };

    let is_linked_worktree = is_linked_worktree(&layout);
    let main_root = main_root(&layout);

    RepoInfo {
        installed: true,
        is_repo: true,
        root: layout.root.as_deref().map(display),
        main_root,
        is_linked_worktree,
        branch: head_branch(dir),
        // Fails on an unborn HEAD — a repository with no commit yet, which is
        // still a repository worth reporting.
        head: run_git(dir, &["rev-parse", "--short", "HEAD"])
            .ok()
            .map(|out| out.trim().to_string())
            .filter(|sha| !sha.is_empty()),
    }
}

/// Never fails: a missing git and a directory outside any repository are
/// answers, and the UI needs to be able to draw both.
pub async fn git_repo_info(path: String) -> RepoInfo {
    off_thread(move || Ok(repo_info(Path::new(&path))))
        .await
        .unwrap_or_default()
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_current: bool,
    pub bare: bool,
    pub detached: bool,
    /// Reason the worktree is locked. `Some("")` when it is locked without one,
    /// which is still a lock — the distinction the UI needs is None vs Some.
    pub locked: Option<String>,
    pub prunable: Option<String>,
}

/// Reads `git worktree list --porcelain`: blocks separated by a blank line,
/// each opening with `worktree <path>` and followed by the attributes that
/// apply — `HEAD <sha>`, `branch <ref>`, and the bare flags `bare`, `detached`,
/// `locked [reason]`, `prunable [reason]`.
///
/// `is_current` is not decided here; it depends on where the caller is
/// standing, which the porcelain output does not say.
fn parse_worktree_list(porcelain: &str) -> Vec<Worktree> {
    let mut worktrees: Vec<Worktree> = Vec::new();
    let mut current: Option<Worktree> = None;

    for line in porcelain.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if let Some(entry) = current.take() {
                worktrees.push(entry);
            }
            continue;
        }

        let (key, value) = match line.split_once(' ') {
            Some((key, value)) => (key, value.trim()),
            None => (line, ""),
        };

        if key == "worktree" {
            if let Some(entry) = current.take() {
                worktrees.push(entry);
            }
            current = Some(Worktree {
                path: value.to_string(),
                // git always lists the main worktree first.
                is_main: worktrees.is_empty(),
                ..Worktree::default()
            });
            continue;
        }

        // An attribute before any `worktree` line would belong to nothing.
        let Some(entry) = current.as_mut() else {
            continue;
        };
        match key {
            "HEAD" => entry.head = Some(value.to_string()),
            "branch" => entry.branch = (!value.is_empty()).then(|| short_branch(value).to_string()),
            "bare" => entry.bare = true,
            "detached" => entry.detached = true,
            "locked" => entry.locked = Some(value.to_string()),
            "prunable" => entry.prunable = Some(value.to_string()),
            _ => {}
        }
    }

    if let Some(entry) = current.take() {
        worktrees.push(entry);
    }
    worktrees
}

fn short_branch(refname: &str) -> &str {
    refname.strip_prefix("refs/heads/").unwrap_or(refname)
}

/// Every worktree of the repository holding `dir`, the one `dir` is inside
/// marked as current.
fn list_worktrees(dir: &Path) -> Result<Vec<Worktree>, GitError> {
    let mut worktrees = parse_worktree_list(&run_git(dir, &["worktree", "list", "--porcelain"])?);

    // Which entry the caller is inside. `--show-toplevel` is absolute on
    // every git that has it, and comes out in the same spelling as the
    // porcelain paths, so the two are comparable. A bare repository has no
    // toplevel and simply leaves every entry unmarked.
    if let Ok(toplevel) = run_git(dir, &["rev-parse", "--show-toplevel"]) {
        let here = compare_key(toplevel.trim());
        if let Some(entry) = worktrees
            .iter_mut()
            .find(|worktree| compare_key(&worktree.path) == here)
        {
            entry.is_current = true;
        }
    }
    Ok(worktrees)
}

pub async fn git_worktree_list(path: String) -> Result<Vec<Worktree>, String> {
    off_thread(move || Ok(list_worktrees(Path::new(&path))?)).await
}

/// A worktree whose branch is merged into the main checkout's HEAD — a
/// candidate for the end-of-cycle cleanup.
///
/// Ported from Git Worktree Manager (MIT): `git branch --merged`, keeping the
/// `+` lines (a branch checked out in another worktree), crossed with
/// `git worktree list --porcelain` for the paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedWorktree {
    pub path: String,
    pub branch: String,
    /// Lock reason, as in `Worktree`. A locked worktree is listed so the user
    /// sees why it stays, but the UI never offers to remove it.
    pub locked: Option<String>,
    /// Files with uncommitted changes, untracked ones included. None when git
    /// could not say — treated as unknown, never as clean.
    pub dirty: Option<u32>,
    /// Its head is the main checkout's: the branch has no commit of its own.
    /// git calls that merged, but it is as likely a worker that just started.
    pub no_own_commit: bool,
    /// A pane of Hangar is open in it.
    pub held: bool,
    /// What the UI ticks by default: merged with something to show for it,
    /// clean, unlocked, and nobody working in it. Everything else is listed,
    /// with the reason, and left to the person to tick.
    pub preselected: bool,
}

impl MergedWorktree {
    fn is_preselected(&self) -> bool {
        self.locked.is_none() && self.dirty == Some(0) && !self.no_own_commit && !self.held
    }
}

/// The branch names `git branch --merged` lists as checked out in another
/// worktree. Each line is a two-character marker then the name: `*` for the
/// branch of the directory git ran in, `+` for one held by another worktree,
/// blank for the rest. Only `+` lines can have a worktree to clean up; the
/// `*` one is where we stand, and a detached head is written in parentheses.
fn merged_in_other_worktrees(listing: &str) -> HashSet<String> {
    listing
        .lines()
        .filter_map(|line| line.strip_prefix('+'))
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with('('))
        .map(str::to_string)
        .collect()
}

/// Which worktrees the cleanup may even propose: a linked worktree on a branch
/// that is merged. Never the main checkout, the one the panel is opened from,
/// a bare or detached entry (no branch to judge), nor one whose folder is gone
/// (`prune` is the tool for those). Locks and dirtiness are carried, not
/// filtered: the person sees why a worktree is held back.
fn merged_candidates(
    worktrees: &[Worktree],
    merged: &HashSet<String>,
    held: &[String],
) -> Vec<MergedWorktree> {
    let main_head = worktrees
        .iter()
        .find(|worktree| worktree.is_main)
        .and_then(|worktree| worktree.head.as_deref());
    worktrees
        .iter()
        .filter(|worktree| {
            !worktree.is_main
                && !worktree.is_current
                && !worktree.bare
                && !worktree.detached
                && worktree.prunable.is_none()
        })
        .filter_map(|worktree| {
            let branch = worktree.branch.as_ref()?;
            merged.contains(branch).then(|| MergedWorktree {
                path: worktree.path.clone(),
                branch: branch.clone(),
                locked: worktree.locked.clone(),
                dirty: None,
                no_own_commit: main_head.is_some() && worktree.head.as_deref() == main_head,
                held: held
                    .iter()
                    .any(|folder| compare_key(folder) == compare_key(&worktree.path)),
                preselected: false,
            })
        })
        .collect()
}

fn dirty_files(worktree: &str) -> Option<u32> {
    run_git(
        Path::new(worktree),
        &["status", "--porcelain", "--untracked-files=all"],
    )
    .ok()
    .map(|out| out.lines().filter(|line| !line.trim().is_empty()).count() as u32)
}

fn merged_worktrees(dir: &Path, held: &[String]) -> Result<Vec<MergedWorktree>, GitError> {
    let worktrees = list_worktrees(dir)?;
    // Judged against the main checkout's HEAD, wherever the panel was opened
    // from: from a linked worktree, `--merged` would compare to its own branch.
    let main = worktrees
        .iter()
        .find(|worktree| worktree.is_main && !worktree.bare)
        .map(|worktree| worktree.path.clone());
    let base = main.as_deref().map(Path::new).unwrap_or(dir);
    let merged = merged_in_other_worktrees(&run_git(base, &["branch", "--merged"])?);

    let mut found = merged_candidates(&worktrees, &merged, held);
    for entry in &mut found {
        entry.dirty = dirty_files(&entry.path);
        entry.preselected = entry.is_preselected();
    }
    Ok(found)
}

pub async fn git_worktrees_merged(
    path: String,
    held_folders: Vec<String>,
) -> Result<Vec<MergedWorktree>, String> {
    off_thread(move || Ok(merged_worktrees(Path::new(&path), &held_folders)?)).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// `refname:short`: a remote keeps its remote in the name, as "origin/xyz".
    pub name: String,
    /// "local" or "remote".
    pub kind: String,
    pub is_current: bool,
    /// The worktree already holding this branch, if any. A branch can only be
    /// checked out once, so this is what the UI greys out.
    pub worktree_path: Option<String>,
    /// Short sha of the commit it points at.
    pub head: Option<String>,
    /// Unix seconds — the key the listing is already sorted on.
    pub committer_date: Option<i64>,
    pub author_name: Option<String>,
    pub subject: Option<String>,
    /// The branch this one tracks, as "origin/xyz".
    pub upstream: Option<String>,
    /// git's own summary of the gap to the upstream: "[ahead 1, behind 2]", or
    /// "[gone]" when the upstream has been deleted.
    pub track: Option<String>,
}

/// The atoms `parse_branches` reads back, in that order.
///
/// NUL between them rather than a tab: a commit subject can contain a tab, and
/// nothing at all can contain a NUL. Ported from microsoft/vscode
/// extensions/git (MIT), which separates its for-each-ref fields the same way.
const BRANCH_FORMAT: &str = "--format=%(refname)%00%(refname:short)%00%(HEAD)%00%(worktreepath)%00%(objectname:short)%00%(committerdate:unix)%00%(authorname)%00%(subject)%00%(upstream:short)%00%(upstream:track)";

/// One field of a branch line, or None when it has nothing to say.
///
/// Missing and empty have to read alike: git prints an empty string for every
/// atom a ref does not have, and a lone space for `%(HEAD)` on all but the
/// current branch.
fn branch_field<'a>(fields: &[&'a str], index: usize) -> Option<&'a str> {
    fields
        .get(index)
        .copied()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Reads the `for-each-ref` listing produced by `BRANCH_FORMAT`.
///
/// The order git printed is kept exactly: it sorted by `-committerdate`, which
/// is the order both VS Code's branch picker and Git Worktree Manager show, and
/// re-sorting here would only be able to get it wrong.
fn parse_branches(listing: &str) -> Vec<Branch> {
    listing
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\0').collect();
            let refname = branch_field(&fields, 0)?;

            let kind = if refname.starts_with("refs/heads/") {
                "local"
            } else if refname.starts_with("refs/remotes/") {
                // `origin/HEAD` is a symref at the remote's default branch, not
                // a branch of its own, and checking it out is not a thing to
                // offer. Ported from microsoft/vscode extensions/git (MIT),
                // which drops it from every listing it builds.
                if refname.ends_with("/HEAD") {
                    return None;
                }
                "remote"
            } else {
                // Nothing else was asked for, and a row whose `kind` the UI
                // cannot switch on has no place in the list.
                return None;
            };

            Some(Branch {
                name: branch_field(&fields, 1)?.to_string(),
                kind: kind.to_string(),
                // git marks the checked-out branch with "*" and every other one
                // with a space. Only a local branch is ever checked out.
                is_current: kind == "local" && branch_field(&fields, 2) == Some("*"),
                worktree_path: branch_field(&fields, 3).map(str::to_string),
                head: branch_field(&fields, 4).map(str::to_string),
                committer_date: branch_field(&fields, 5).and_then(|date| date.parse::<i64>().ok()),
                author_name: branch_field(&fields, 6).map(str::to_string),
                subject: branch_field(&fields, 7).map(str::to_string),
                upstream: branch_field(&fields, 8).map(str::to_string),
                track: branch_field(&fields, 9).map(str::to_string),
            })
        })
        .collect()
}

/// Every branch of the repository, most recently committed to first.
pub async fn git_branches(path: String) -> Result<Vec<Branch>, String> {
    off_thread(move || {
        let listing = run_git(
            Path::new(&path),
            &[
                "for-each-ref",
                "refs/heads",
                "refs/remotes",
                "--sort=-committerdate",
                BRANCH_FORMAT,
            ],
        )?;
        Ok(parse_branches(&listing))
    })
    .await
}

/// `--all` because the panel offers one button and a repository is allowed more
/// than one remote, and `--prune` so a branch someone deleted upstream stops
/// being offered here as something to check out.
pub async fn git_fetch(path: String) -> Result<(), String> {
    off_thread(move || {
        run_git(Path::new(&path), &["fetch", "--all", "--prune"])?;
        Ok(())
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteBranchResult {
    pub deleted: bool,
    /// The branch holds commits nothing else does. Same shape as
    /// `RemoveResult`: something to ask the user about, not to report as a
    /// failure.
    pub needs_force: bool,
    pub message: Option<String>,
}

/// git refusing to drop unmerged work — the one deletion failure a
/// confirmation answers.
///
/// Ported from microsoft/vscode extensions/git (MIT): `_deleteBranch` catches
/// this exact refusal, asks, and re-runs the deletion forced (commands.ts).
fn not_fully_merged(message: &str) -> bool {
    message.to_lowercase().contains("not fully merged")
}

pub async fn git_branch_delete(
    path: String,
    branch: String,
    force: bool,
) -> Result<DeleteBranchResult, String> {
    off_thread(move || {
        let flag = if force { "-D" } else { "-d" };

        match run_git(Path::new(&path), &["branch", flag, &branch]) {
            Ok(_) => Ok(DeleteBranchResult {
                deleted: true,
                needs_force: false,
                message: None,
            }),
            Err(GitError::Failed(message)) if !force && not_fully_merged(&message) => {
                Ok(DeleteBranchResult {
                    deleted: false,
                    needs_force: true,
                    message: Some(message),
                })
            }
            // Already gone: the branch is not there, which is what was asked.
            Err(GitError::Failed(message))
                if message.contains("not found") && message.contains("branch") =>
            {
                Ok(DeleteBranchResult {
                    deleted: true,
                    needs_force: false,
                    message: Some(message),
                })
            }
            Err(error) => Err(error.into()),
        }
    })
    .await
}

pub async fn git_worktree_add(
    path: String,
    worktree_path: String,
    branch: String,
    create_branch: bool,
    base_ref: Option<String>,
) -> Result<(), String> {
    off_thread(move || {
        let mut args: Vec<&str> = vec!["worktree", "add"];
        if create_branch {
            args.push("-b");
            args.push(&branch);
            args.push(&worktree_path);
            // Left to git when absent, which branches from the current HEAD.
            if let Some(base) = base_ref
                .as_deref()
                .map(str::trim)
                .filter(|base| !base.is_empty())
            {
                args.push(base);
            }
        } else {
            args.push(&worktree_path);
            args.push(&branch);
        }

        run_git(Path::new(&path), &args)?;
        forget_shared_roots();
        Ok(())
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    pub removed: bool,
    /// The removal was refused for something the user can wave through. The UI
    /// turns this into a confirmation, not an error.
    pub needs_force: bool,
    pub message: Option<String>,
    /// The `preRemove` hook stopped the removal: nothing was removed, and the
    /// person may remove anyway (without the hook) once they have read why.
    pub hook_blocked: bool,
    /// What the `preRemove` hook did, when one ran.
    pub hook: Option<HookReport>,
}

impl RemoveResult {
    fn plain(removed: bool, needs_force: bool, message: Option<String>) -> Self {
        RemoveResult {
            removed,
            needs_force,
            message,
            hook_blocked: false,
            hook: None,
        }
    }
}

/// Refusals that a confirmation answers, rather than something to go and fix.
///
/// The lock check matches git's full phrase, not the word: a path or branch
/// name with "blocked" in it lands in these messages verbatim, and offering
/// `--force --force` over a refusal force cannot fix only buries the reason.
fn recoverable_by_force(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("contains modified or untracked files")
        || message.contains("locked working tree")
}

// ---------------------------------------------------------------------------
// Leftovers
//
// On Windows `git worktree remove` can give up half way: a process of the
// worker that just closed still holds a file, a path is too long. Git has by
// then taken the folder's `.git` away, so the next try answers "is not a
// working tree" and what is left of the folder stays on the disk for good.
// This deletes what remains, and only what Hangar made.
// ---------------------------------------------------------------------------

/// Why a folder was not swept.
#[derive(Debug, PartialEq)]
pub(crate) enum Sweep {
    /// Not a folder Hangar made for a worker, or one git still owns: nothing
    /// was touched.
    Refused(String),
    /// It is Hangar's, but something holds it. One line naming what.
    Held(String),
}

const SWEEP_TRIES: u32 = 5;
const SWEEP_PAUSE: Duration = Duration::from_millis(400);

/// Whether the entry is a symbolic link or a junction: something to delete, never to enter.
fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn remove_link(path: &Path, meta: &std::fs::Metadata) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const DIRECTORY: u32 = 0x10;
        if meta.file_attributes() & DIRECTORY != 0 {
            return std::fs::remove_dir(path);
        }
    }
    let _ = meta;
    std::fs::remove_file(path)
}

/// `path` in the form Windows accepts past 260 characters (`\\?\`, which also
/// takes no `/` and no `..`). A `node_modules` tree goes deeper than the plain
/// form allows, and what cannot be named cannot be deleted. Anywhere else, and
/// for a relative path, it is `path` itself.
fn verbatim(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy().replace('/', "\\");
        if text.starts_with("\\\\?\\") {
            return PathBuf::from(text);
        }
        if let Some(unc) = text.strip_prefix("\\\\") {
            return PathBuf::from(format!("\\\\?\\UNC\\{unc}"));
        }
        let drive = text.as_bytes();
        if drive.len() > 2
            && drive[0].is_ascii_alphabetic()
            && drive[1] == b':'
            && drive[2] == b'\\'
        {
            return PathBuf::from(format!("\\\\?\\{text}"));
        }
        PathBuf::from(text)
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// Deletes what it can under `path`, `path` included, noting each entry it
/// could not. A link is removed itself and never entered, so a junction in
/// `node_modules` costs its own entry and not its target.
fn remove_tree(path: &Path, held: &mut Vec<PathBuf>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if is_link(&meta) {
        if remove_link(path, &meta).is_err() {
            held.push(path.to_path_buf());
        }
        return;
    }
    if meta.is_dir() {
        let before = held.len();
        // A folder without write permission cannot lose its entries.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = meta.permissions();
            if permissions.mode() & 0o700 != 0o700 {
                permissions.set_mode(permissions.mode() | 0o700);
                let _ = std::fs::set_permissions(path, permissions);
            }
        }
        match std::fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    remove_tree(&entry.path(), held);
                }
            }
            Err(_) => held.push(path.to_path_buf()),
        }
        if held.len() == before && std::fs::remove_dir(path).is_err() {
            held.push(path.to_path_buf());
        }
        return;
    }
    let mut permissions = meta.permissions();
    if permissions.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = std::fs::set_permissions(path, permissions);
    }
    if std::fs::remove_file(path).is_err() {
        held.push(path.to_path_buf());
    }
}

/// `<parent of the main checkout>/<name>-worktrees`, where Hangar makes them.
fn worktrees_folder(main: &str) -> Option<String> {
    let main = main.trim_end_matches(['/', '\\']);
    let (parent, name) = main.rsplit_once(['/', '\\'])?;
    (!name.is_empty()).then(|| format!("{parent}/{name}-worktrees"))
}

/// Removes the folder of a worktree git no longer holds (or has just let go
/// of), then forgets whatever registration is left. Waits and retries while
/// something holds it. Succeeds when the folder is gone.
pub(crate) fn sweep_leftovers(
    repo: &Path,
    folder: &Path,
    tries: u32,
    pause: Duration,
) -> Result<(), Sweep> {
    let name = folder.display().to_string();
    let refused = |why: &str| Err(Sweep::Refused(format!("{name} {why}: nothing was deleted")));
    let listed = list_worktrees(repo).map_err(|error| {
        Sweep::Refused(format!(
            "{name} could not be checked ({}): nothing was deleted",
            String::from(error)
        ))
    })?;
    let here = compare_key(&name);
    let Some(main) = listed.first() else {
        return refused("is not in a repository");
    };
    if compare_key(&main.path) == here {
        return refused("is the main checkout");
    }
    let inside = worktrees_folder(&main.path)
        .map(|root| format!("{}/", compare_key(&root)))
        .is_some_and(|root| here.starts_with(&root) && here.len() > root.len());
    if !inside {
        return refused("is not in the workspace's worktrees folder");
    }
    let registered = listed.iter().any(|entry| compare_key(&entry.path) == here);
    match std::fs::symlink_metadata(folder) {
        Ok(meta) if is_link(&meta) => return refused("is a link"),
        Ok(meta) if !meta.is_dir() => return refused("is not a folder"),
        _ => {}
    }
    // A live `.git` file is git's: the worktree is still whole. A `.git`
    // folder is a repository of its own.
    let dot_git = folder.join(".git");
    if dot_git.is_dir() {
        return refused("holds a repository of its own");
    }
    if registered && dot_git.exists() {
        return refused("is still a working tree");
    }

    let mut held = Vec::new();
    for attempt in 1..=tries.max(1) {
        held.clear();
        remove_tree(&verbatim(folder), &mut held);
        if !folder.exists() {
            break;
        }
        if attempt < tries.max(1) {
            std::thread::sleep(pause * attempt);
        }
    }
    if folder.exists() {
        held.sort();
        let shown: Vec<String> = held
            .iter()
            .take(3)
            .map(|path| {
                let text = path.display().to_string();
                text.strip_prefix("\\\\?\\").unwrap_or(&text).to_string()
            })
            .collect();
        let more = held.len().saturating_sub(shown.len());
        let files = if shown.is_empty() {
            "something in it".to_string()
        } else {
            format!(
                "{}{}",
                shown.join(", "),
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                }
            )
        };
        return Err(Sweep::Held(format!(
            "{name} could not be deleted: {files} cannot be removed, a program still uses it. Close it and remove the folder."
        )));
    }
    let _ = run_git(repo, &["worktree", "prune"]);
    forget_shared_roots();
    Ok(())
}

/// `pre_remove` is the workspace's command, run in the worktree first. A
/// failure or a timeout keeps the worktree. The caller leaves it out when the
/// person chose to remove anyway.
fn remove_worktree(
    path: &str,
    worktree_path: &str,
    force: bool,
    pre_remove: Option<&str>,
    timeout: Duration,
) -> Result<RemoveResult, String> {
    let mut hook = None;
    if Path::new(worktree_path).is_dir() {
        if let Some(report) = run_configured_hook(pre_remove, path, worktree_path, timeout) {
            if !report.ok {
                let message = format!("The preRemove hook {}", report.describe(timeout));
                return Ok(RemoveResult {
                    removed: false,
                    needs_force: false,
                    message: Some(message),
                    hook_blocked: true,
                    hook: Some(report),
                });
            }
            hook = Some(report);
        }
    }

    let mut args: Vec<&str> = vec!["worktree", "remove"];
    if force {
        // Twice on purpose. One `--force` covers a worktree with uncommitted
        // work in it; git documents a second one as what it takes to remove a
        // locked worktree, and the user has already been asked by then.
        args.push("--force");
        args.push("--force");
    }
    args.push(worktree_path);

    match run_git(Path::new(path), &args) {
        Ok(_) => {
            forget_shared_roots();
            Ok(RemoveResult {
                hook,
                ..RemoveResult::plain(true, false, None)
            })
        }
        Err(GitError::Failed(message)) if !force && recoverable_by_force(&message) => {
            Ok(RemoveResult {
                hook,
                ..RemoveResult::plain(false, true, Some(message))
            })
        }
        // Git gave up half way, or has nothing left to remove: what remains of
        // the folder is swept, and the answer is about that, never git's stale
        // "is not a working tree".
        Err(GitError::Failed(message)) => {
            match sweep_leftovers(
                Path::new(path),
                Path::new(worktree_path),
                SWEEP_TRIES,
                SWEEP_PAUSE,
            ) {
                Ok(()) => Ok(RemoveResult {
                    hook,
                    ..RemoveResult::plain(true, false, None)
                }),
                Err(Sweep::Held(held)) => Err(held),
                Err(Sweep::Refused(refusal)) => {
                    let lowered = message.to_lowercase();
                    if lowered.contains("is not a working tree")
                        || (lowered.contains("cannot remove working tree")
                            && lowered.contains(".git does not exist"))
                    {
                        Err(refusal)
                    } else {
                        Err(message)
                    }
                }
            }
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn git_worktree_remove(
    path: String,
    worktree_path: String,
    force: bool,
    pre_remove: Option<String>,
) -> Result<RemoveResult, String> {
    off_thread(move || {
        remove_worktree(
            &path,
            &worktree_path,
            force,
            pre_remove.as_deref(),
            HOOK_TIMEOUT,
        )
    })
    .await
}

/// What an orphan sweep did, one line each, for the log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrphanSweep {
    pub removed: Vec<String>,
    pub kept: Vec<String>,
}

/// Removes the worktrees of the repository holding `dir` that nobody uses any
/// more: under the `<checkout>-worktrees` folder (never one made by hand
/// elsewhere, never the main checkout), on a branch whose every commit is on
/// the main checkout's branch, with nothing uncommitted, not locked, and no
/// pane in them (`held`: the folders panes run in, a pane in a subfolder
/// counts). Their branch goes with them. Whatever stays is said, with why.
pub fn sweep_orphan_worktrees(dir: &Path, held: &[String]) -> Result<OrphanSweep, String> {
    let mut sweep = OrphanSweep::default();
    let listed = list_worktrees(dir).map_err(String::from)?;
    let Some(main) = listed.iter().find(|entry| entry.is_main && !entry.bare) else {
        return Ok(sweep);
    };
    let Some(home) = worktrees_folder(&main.path).map(|root| format!("{}/", compare_key(&root)))
    else {
        return Ok(sweep);
    };
    let main_path = main.path.clone();
    // Folders git no longer lists and that hold no `.git`: what a removal
    // left behind (a `node_modules` git did not delete).
    let folder = home.trim_end_matches('/').to_string();
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            let text = path.to_string_lossy().replace('\\', "/");
            let here = compare_key(&text);
            let listed_here = listed.iter().any(|item| compare_key(&item.path) == here);
            let used = held.iter().any(|held| {
                let held = compare_key(held);
                held == here || held.starts_with(&format!("{here}/"))
            });
            let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
            if listed_here || used || !is_dir || path.join(".git").exists() {
                continue;
            }
            match sweep_leftovers(Path::new(&main_path), &path, SWEEP_TRIES, SWEEP_PAUSE) {
                Ok(()) => sweep
                    .removed
                    .push(format!("{text}: leftover folder removed")),
                Err(Sweep::Held(why) | Sweep::Refused(why)) => {
                    sweep.kept.push(format!("{text}: kept, {why}"))
                }
            }
        }
    }
    for item in merged_worktrees(Path::new(&main_path), &[]).map_err(String::from)? {
        let here = compare_key(&item.path);
        if !(here.starts_with(&home) && here.len() > home.len()) {
            continue; // not Hangar's: not even worth a line
        }
        let used = held.iter().any(|folder| {
            let folder = compare_key(folder);
            folder == here || folder.starts_with(&format!("{here}/"))
        });
        let why = if used {
            Some("a pane uses it".to_string())
        } else if item.locked.is_some() {
            Some("it is locked".to_string())
        } else if item.dirty != Some(0) {
            Some("it has uncommitted changes, or git could not say".to_string())
        } else if item.no_own_commit {
            Some("its branch has no commit of its own (a worker may be starting)".to_string())
        } else {
            None
        };
        if let Some(why) = why {
            sweep.kept.push(format!("{}: kept, {why}", item.path));
            continue;
        }
        match remove_worktree(&main_path, &item.path, false, None, HOOK_TIMEOUT) {
            Ok(result) if result.removed => {
                let branch = match run_git(Path::new(&main_path), &["branch", "-D", &item.branch]) {
                    Ok(_) => format!("branch {} deleted", item.branch),
                    Err(error) => {
                        format!("branch {} kept ({})", item.branch, String::from(error))
                    }
                };
                sweep
                    .removed
                    .push(format!("{}: removed, {branch}", item.path));
            }
            Ok(result) => sweep.kept.push(format!(
                "{}: kept, {}",
                item.path,
                result.message.unwrap_or_else(|| "git refused".into())
            )),
            Err(error) => sweep.kept.push(format!("{}: kept, {error}", item.path)),
        }
    }
    Ok(sweep)
}

// ---------------------------------------------------------------------------
// Worktree hooks
//
// Two commands the person may set per workspace: `postCreate`, run in a new
// worktree, and `preRemove`, run in one about to go. They run on this machine,
// as the person, through the default shell — which is why nothing here runs
// unless the person configured it, and why no agent has a way to set one.
// ---------------------------------------------------------------------------

/// How long a hook may run before its whole process tree is killed.
pub(crate) const HOOK_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long the drains get to finish once the hook has exited: a background
/// process it started (`pnpm dev &`) holds the pipes open for as long as it
/// lives, and the report must not wait for that.
const HOOK_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// How much of a hook's output is kept, per stream: the end of it, where the
/// reason for a failure usually is.
const HOOK_OUTPUT_CAP: usize = 8 * 1024;

/// What a hook did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookReport {
    /// Exit status zero.
    pub ok: bool,
    pub timed_out: bool,
    pub exit_code: Option<i32>,
    /// stdout then stderr, capped.
    pub output: String,
    /// Why it did not even start, when it did not.
    pub error: Option<String>,
}

impl HookReport {
    /// "failed (exit 3): <output>", for a message that carries its reason.
    fn describe(&self, timeout: Duration) -> String {
        let why = if let Some(error) = &self.error {
            format!("could not run: {error}")
        } else if self.timed_out {
            format!("timed out after {}", format_timeout(timeout))
        } else {
            match self.exit_code {
                Some(code) => format!("failed (exit {code})"),
                None => "was stopped by a signal".to_string(),
            }
        };
        if self.output.is_empty() {
            why
        } else {
            format!("{why}: {}", self.output)
        }
    }
}

fn format_timeout(timeout: Duration) -> String {
    let seconds = timeout.as_secs();
    if seconds >= 60 && seconds.is_multiple_of(60) {
        format!("{} min", seconds / 60)
    } else {
        format!("{seconds} s")
    }
}

/// The shell a hook goes through: the default detected one, WSL aside (it does
/// not carry the environment and the folder into its own filesystem).
fn hook_shell() -> Option<crate::shells::ShellInfo> {
    crate::shells::detect_shells()
        .into_iter()
        .find(|shell| shell.id != "wsl")
}

fn hook_command(shell: &crate::shells::ShellInfo, command: &str) -> Command {
    let mut process = Command::new(&shell.program);
    match shell.id.as_str() {
        "pwsh" | "powershell" => {
            process.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                command,
            ]);
        }
        #[cfg(windows)]
        "cmd" => {
            use std::os::windows::process::CommandExt;
            // The command line is handed over verbatim: Rust's own quoting
            // is not the one cmd.exe reads.
            process.raw_arg(format!("/D /S /C \"{command}\""));
        }
        _ => {
            // The pane arguments minus the interactive flag: a login shell
            // keeps PATH complete, an interactive one would wait for input.
            process.args(shell.args.iter().filter(|arg| arg.as_str() != "-i"));
            process.arg("-c").arg(command);
        }
    }
    process
}

/// Ends the process and everything it started.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(unix)]
    {
        // The hook leads its own process group (see `run_hook`).
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
}

/// What a drain has kept so far (the end of the stream, and whether anything
/// was dropped), and a signal that it reached the stream's end.
type Drained = (
    std::sync::Arc<Mutex<(Vec<u8>, bool)>>,
    std::sync::mpsc::Receiver<()>,
);

/// Reads a stream to its end, keeping only the last `HOOK_OUTPUT_CAP` bytes
/// (and whether anything was dropped). It never stops reading: a full pipe
/// would stall the hook. A stream that never ends — a background process
/// holds it — is left to its thread; `collect_output` stops waiting for it.
fn drain<R: Read + Send + 'static>(mut stream: R) -> Drained {
    let kept = std::sync::Arc::new(Mutex::new((Vec::new(), false)));
    let (done, finished) = std::sync::mpsc::channel();
    let sink = kept.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let mut guard = sink.lock().unwrap_or_else(|e| e.into_inner());
                    let (kept, dropped) = &mut *guard;
                    kept.extend_from_slice(&buffer[..read]);
                    if kept.len() > HOOK_OUTPUT_CAP * 2 {
                        let cut = kept.len() - HOOK_OUTPUT_CAP;
                        kept.drain(..cut);
                        *dropped = true;
                    }
                }
            }
        }
        let _ = done.send(());
    });
    (kept, finished)
}

/// What the drains kept. Waits at most `grace` in all for them to reach their
/// end: past it, what they hold is what the hook said.
fn collect_output(handles: Vec<Drained>, grace: Duration) -> String {
    let deadline = Instant::now() + grace;
    let mut parts = Vec::new();
    for (kept, finished) in handles {
        let _ = finished.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        let (mut bytes, mut dropped) = kept.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if bytes.len() > HOOK_OUTPUT_CAP {
            let cut = bytes.len() - HOOK_OUTPUT_CAP;
            bytes.drain(..cut);
            dropped = true;
        }
        let text = String::from_utf8_lossy(&bytes).replace('\0', "");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        parts.push(if dropped {
            format!("…{text}")
        } else {
            text.to_string()
        });
    }
    parts.join("\n")
}

/// Runs `command` in `worktree` through the shell, with `WORKTREE_PATH` and
/// `BASE_PATH` in its environment. Blocks: callers are on the blocking pool.
fn run_hook(command: &str, base: &str, worktree: &str, timeout: Duration) -> HookReport {
    let failed = |error: String| HookReport {
        error: Some(error),
        ..HookReport::default()
    };
    let Some(shell) = hook_shell() else {
        return failed("no shell was found on this machine".to_string());
    };
    crate::path_env::ensure();

    let mut process = hook_command(&shell, command);
    process
        .current_dir(worktree)
        .env("WORKTREE_PATH", worktree)
        .env("BASE_PATH", base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }

    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(err) => return failed(err.to_string()),
    };
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(drain(stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(drain(stderr));
    }

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                kill_tree(&mut child);
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break None,
        }
    };

    let output = collect_output(readers, HOOK_DRAIN_GRACE);
    HookReport {
        ok: !timed_out && status.is_some_and(|status| status.success()),
        timed_out,
        exit_code: status.and_then(|status| status.code()),
        output,
        error: None,
    }
}

/// The base checkout a worktree hangs off: where `workspace` is, or the main
/// checkout when it is itself a linked worktree.
fn base_of(workspace: &str) -> String {
    shared_root(workspace).unwrap_or_else(|| workspace.to_string())
}

/// `None` — and nothing run — unless the person configured a command.
fn run_configured_hook(
    command: Option<&str>,
    workspace: &str,
    worktree: &str,
    timeout: Duration,
) -> Option<HookReport> {
    let command = command
        .map(str::trim)
        .filter(|command| !command.is_empty())?;
    Some(run_hook(command, &base_of(workspace), worktree, timeout))
}

/// The `postCreate` command, run in a worktree that exists by now. Resolves
/// with what it did; a failing hook is a report, never an error, and never
/// undoes the worktree. `None` when no command is set.
pub async fn git_worktree_hook(
    path: String,
    worktree_path: String,
    command: Option<String>,
) -> Result<Option<HookReport>, String> {
    off_thread(move || {
        Ok(run_configured_hook(
            command.as_deref(),
            &path,
            &worktree_path,
            HOOK_TIMEOUT,
        ))
    })
    .await
}

pub async fn git_worktree_prune(path: String) -> Result<(), String> {
    off_thread(move || {
        run_git(Path::new(&path), &["worktree", "prune"])?;
        forget_shared_roots();
        Ok(())
    })
    .await
}

/// Marks a worktree as one `prune` must not collect and `remove` must not take
/// without being told twice.
///
/// No `--reason`: this is a toggle in a panel with nowhere to type one, and
/// Git Worktree Manager (MIT) locks the same way.
pub async fn git_worktree_lock(path: String, worktree_path: String) -> Result<(), String> {
    off_thread(move || {
        run_git(Path::new(&path), &["worktree", "lock", &worktree_path])?;
        Ok(())
    })
    .await
}

pub async fn git_worktree_unlock(path: String, worktree_path: String) -> Result<(), String> {
    off_thread(move || {
        run_git(Path::new(&path), &["worktree", "unlock", &worktree_path])?;
        Ok(())
    })
    .await
}

/// How many conflicting paths a refusal names; the rest is a count.
const MIGRATE_LISTED: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateResult {
    /// `moved`, `nothing` (the source holds no uncommitted change), `conflict`
    /// (a path is modified on both sides: nothing was touched) or
    /// `applyFailed` (the stash could not be applied in the target and is
    /// still kept).
    pub outcome: &'static str,
    /// The first paths changed on both sides; `conflict` only.
    pub files: Vec<String>,
    /// How many more paths than `files` lists.
    pub more: u32,
    /// The kept stash commit, to get the work back with
    /// `git stash apply <stash>`; `applyFailed` (and `moved` when the drop of
    /// the applied stash itself failed).
    pub stash: Option<String>,
    /// git's own words for a failure.
    pub message: Option<String>,
}

impl MigrateResult {
    fn of(outcome: &'static str) -> Self {
        MigrateResult {
            outcome,
            files: Vec::new(),
            more: 0,
            stash: None,
            message: None,
        }
    }
}

/// The paths `git status -z` reports as changed: staged, unstaged and
/// untracked, both ends of a rename.
fn changed_paths(dir: &Path) -> Result<HashSet<String>, GitError> {
    let status = run_git(
        dir,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )?;
    let mut paths = HashSet::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let Some(path) = entry.get(3..) else { continue };
        paths.insert(path.to_string());
        // A rename or copy carries its origin as a second entry.
        if entry.starts_with(['R', 'C']) || entry[1..].starts_with(['R', 'C']) {
            if let Some(origin) = entries.next() {
                paths.insert(origin.to_string());
            }
        }
    }
    Ok(paths)
}

/// `main`, or the short sha of a detached HEAD.
fn ref_label(dir: &Path) -> Result<String, GitError> {
    match run_git(dir, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        Ok(name) if !name.trim().is_empty() => Ok(name.trim().to_string()),
        _ => Ok(run_git(dir, &["rev-parse", "--short", "HEAD"])?
            .trim()
            .to_string()),
    }
}

/// The stash commit named `name`, looked up by name and not as `stash@{0}`:
/// the stash list is shared with every other worktree and another agent may
/// push onto it between our push and our lookup.
fn find_stash(dir: &Path, name: &str) -> Result<Option<String>, GitError> {
    let list = run_git(dir, &["stash", "list", "--format=%H %gs"])?;
    Ok(list.lines().find_map(|line| {
        let (sha, subject) = line.split_once(' ')?;
        subject
            .ends_with(&format!(": {name}"))
            .then(|| sha.to_string())
    }))
}

/// Moves the uncommitted work of `source` into `target`, emptying `source`.
///
/// Ported from microsoft/vscode extensions/git (MIT), `migrateChanges`: stash
/// index, tracked and untracked files under `migration:<source>-<target>`,
/// apply that stash in the target.
///
/// Nothing is dropped before the apply succeeded: a refusal touches nothing,
/// and a failed apply leaves the stash in place with its sha in the result.
///
/// Staged state is not kept: the work arrives as working-tree changes (new
/// files git had staged stay staged, that being what `stash apply` does).
/// `--index` is not used because it fails as soon as the two HEADs differ.
fn migrate_changes(source: &Path, target: &Path) -> Result<MigrateResult, GitError> {
    let source_paths = changed_paths(source)?;
    if source_paths.is_empty() {
        return Ok(MigrateResult::of("nothing"));
    }

    let target_paths = changed_paths(target)?;
    let mut both: Vec<String> = source_paths.intersection(&target_paths).cloned().collect();
    if !both.is_empty() {
        both.sort();
        let more = both.len().saturating_sub(MIGRATE_LISTED) as u32;
        both.truncate(MIGRATE_LISTED);
        return Ok(MigrateResult {
            files: both,
            more,
            ..MigrateResult::of("conflict")
        });
    }

    let name = format!("migration:{}-{}", ref_label(source)?, ref_label(target)?);
    run_git(
        source,
        &["stash", "push", "--include-untracked", "-m", &name],
    )?;
    let Some(stash) = find_stash(source, &name)? else {
        return Err(GitError::Failed(format!(
            "the stash {name} was created but could not be found again; look in `git stash list`"
        )));
    };

    if let Err(error) = run_git(target, &["stash", "apply", &stash]) {
        let message: String = error.into();
        // git can fail halfway, with part of the work already written. A target
        // that held nothing of its own has nothing to lose by being put back as
        // it was (`clean` without `-x` leaves ignored files alone); one that
        // does is left as it is rather than guessed at. Either way the whole
        // work sits in the stash.
        if target_paths.is_empty() {
            let _ = run_git(target, &["reset", "--hard", "-q"]);
            let _ = run_git(target, &["clean", "-fdq"]);
        }
        return Ok(MigrateResult {
            stash: Some(stash),
            message: Some(message),
            ..MigrateResult::of("applyFailed")
        });
    }

    // Applied: the stash has done its job. Dropping by the index it has now,
    // since the sha is not an accepted argument of `stash drop`.
    let index = run_git(source, &["stash", "list", "--format=%H"])?
        .lines()
        .position(|line| line == stash);
    let dropped = match index {
        Some(n) => run_git(source, &["stash", "drop", &format!("stash@{{{n}}}")]).is_ok(),
        None => true,
    };
    Ok(MigrateResult {
        stash: (!dropped).then_some(stash),
        ..MigrateResult::of("moved")
    })
}

/// « Bring changes back »: `worktree_path`'s uncommitted work into the main
/// checkout of the repository `path` belongs to.
pub async fn git_worktree_migrate(
    path: String,
    worktree_path: String,
) -> Result<MigrateResult, String> {
    off_thread(move || {
        let worktrees = list_worktrees(Path::new(&path))?;
        let main = worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .filter(|worktree| !worktree.bare)
            .ok_or_else(|| {
                "this repository has no main checkout to bring changes to".to_string()
            })?;
        if compare_key(&main.path) == compare_key(&worktree_path) {
            return Err("the main checkout is the destination, not a source".to_string());
        }
        Ok(migrate_changes(
            Path::new(&worktree_path),
            Path::new(&main.path),
        )?)
    })
    .await
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    pub copied: u32,
    /// Files that matched and could not be written. A file already sitting at
    /// the destination is neither copied nor failed: it is left alone.
    pub failed: u32,
}

/// Splits a `ls-files -z` listing.
///
/// `-z` rather than the default output: without it git C-quotes any path
/// holding a non-ASCII or otherwise special character — `"caf\303\251/.env"` —
/// and every one of those would then have to be unquoted before it named a
/// file again.
fn parse_ls_files(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// Entries that are never acted on, whatever the patterns say.
///
/// git lists neither `.git` nor anything above the checkout, so this catches
/// nothing in practice — but these strings become filesystem paths under a
/// directory the user did not name, and the diff view takes them from the UI.
/// Backslash counts as a separator too: on Windows it is one, and `..\x` would
/// climb. `.git` is matched without case, as the filesystems that ignore case
/// would open it. On Windows a colon is refused outright: "C:x" is a path on
/// another root that `join` would switch to, and "file:x" an NTFS stream.
fn is_safe_entry(entry: &str) -> bool {
    let relative = entry.trim_end_matches('/');
    !relative.is_empty()
        && !Path::new(relative).is_absolute()
        && relative.split(['/', '\\']).all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.eq_ignore_ascii_case(".git")
                && !(cfg!(windows) && segment.contains(':'))
        })
}

/// `*` and `?` within one path segment. Both stop at a separator, which is why
/// this is applied segment by segment rather than to the whole path.
fn match_segment(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    // The last `*` seen, and how much of the text it is currently holding:
    // where to come back to when it turns out to have swallowed too little.
    let (mut star, mut swallowed) = (None, 0);

    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some(p);
                swallowed = t;
                p += 1;
            }
            Some('?') => {
                p += 1;
                t += 1;
            }
            Some(character) if *character == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some(index) => {
                    p = index + 1;
                    swallowed += 1;
                    t = swallowed;
                }
                None => return false,
            },
        }
    }
    // Trailing stars are allowed to match nothing at all.
    pattern[p..].iter().all(|character| *character == '*')
}

/// Segment lists, with `**` as the only thing that crosses a separator.
fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    let Some((head, rest)) = pattern.split_first() else {
        return path.is_empty();
    };
    if *head == "**" {
        // Zero segments included, which is what makes a trailing "/**" match
        // the directory itself as well as everything under it.
        return (0..=path.len()).any(|skipped| match_segments(rest, &path[skipped..]));
    }
    match path.split_first() {
        Some((segment, tail)) if match_segment(head, segment) => match_segments(rest, tail),
        _ => false,
    }
}

/// One relative path against one pattern.
///
/// Deliberately not gitignore: patterns are anchored at the root of the
/// checkout and there is no implicit leading `**`, so ".env" means the one at
/// the top and not `packages/api/.env`. That is the rule Git Worktree Manager
/// (MIT) inherits from fast-glob, and the one someone typing ".env" into a
/// settings box means.
fn matches_pattern(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim().trim_matches('/');
    if pattern.is_empty() {
        return false;
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.trim_matches('/').split('/').collect();
    match_segments(&pattern, &path)
}

/// Copies one file, under the two rules the whole feature rests on: never
/// overwrite, never abort.
fn copy_file(from: &Path, to: &Path, result: &mut CopyResult) {
    // Something is already there — a file the branch itself carries, or a
    // second pattern that matched the same path. Leaving it is the safe half of
    // "bring what is missing", and it is not a failure to show anyone.
    if to.exists() {
        return;
    }

    let parents = match to.parent() {
        Some(parent) => std::fs::create_dir_all(parent),
        None => Ok(()),
    };
    // `fs::copy` follows symlinks: a link is reproduced as the bytes it points
    // at rather than as a link. Accepted for the .env-shaped files this exists
    // for — and it is also why a link to a directory lands in `failed` here
    // instead of opening a way to walk in circles.
    if parents.is_ok() && std::fs::copy(from, to).is_ok() {
        result.copied += 1;
    } else {
        result.failed += 1;
    }
}

/// A folded directory, depth first.
///
/// Symlinked directories are not descended into: `file_type` reports a link as
/// a link without following it, so they go to `copy_file` and fail there.
fn copy_tree(from: &Path, to: &Path, result: &mut CopyResult) {
    let Ok(entries) = std::fs::read_dir(from) else {
        result.failed += 1;
        return;
    };

    for entry in entries.flatten() {
        let target = to.join(entry.file_name());
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => copy_tree(&entry.path(), &target, result),
            _ => copy_file(&entry.path(), &target, result),
        }
    }
}

/// Copies whichever of git's entries the patterns claim.
///
/// Split out of the command so the matching, the walking and the counting can
/// be tested against a real tree without a repository around it.
fn copy_entries(from: &Path, to: &Path, entries: &[String], patterns: &[String]) -> CopyResult {
    let mut result = CopyResult::default();

    for entry in entries {
        if !is_safe_entry(entry) {
            continue;
        }
        let relative = entry.trim_end_matches('/');
        if !patterns
            .iter()
            .any(|pattern| matches_pattern(pattern, relative))
        {
            continue;
        }

        // A trailing slash is git having folded a wholly untracked or ignored
        // directory into a single entry; the pattern matched the folder, so the
        // folder goes over whole.
        if entry.ends_with('/') {
            copy_tree(&from.join(relative), &to.join(relative), &mut result);
        } else {
            copy_file(&from.join(relative), &to.join(relative), &mut result);
        }
    }
    result
}

/// Carries into a new worktree the files a checkout does not: `.env`,
/// `*.local`, whatever else the user listed. A linked worktree is a clean
/// checkout of a branch, which is precisely what leaves it unable to run until
/// the untracked configuration is sitting next to it.
///
/// Modelled on `git.worktreeIncludeFiles` in microsoft/vscode extensions/git
/// (MIT) — same purpose, same "copy what is missing, replace nothing" rule.
///
/// `ignored_only` leaves out the files git does not ignore: a worker's
/// worktree (lib/workerControl.ts) gets its `.env` without an untracked file
/// that a `git add -A` would put on its branch.
pub async fn git_copy_untracked(
    from: String,
    to: String,
    patterns: Vec<String>,
    ignored_only: Option<bool>,
) -> Result<CopyResult, String> {
    off_thread(move || {
        let source = Path::new(&from);
        let entries = untracked_entries(source, ignored_only == Some(true))?;
        Ok(copy_entries(source, Path::new(&to), &entries, &patterns))
    })
    .await
}

/// What a checkout holds that git does not track, the files it ignores
/// included, as `ls-files` lists them.
fn untracked_entries(source: &Path, ignored_only: bool) -> Result<Vec<String>, GitError> {
    // git decides what the candidates are, not a directory walk of our own:
    // `--directory` folds a wholly untracked directory into one entry, and
    // that is what keeps `node_modules` from being enumerated on the way to
    // finding a `.env` beside it.
    let mut entries = if ignored_only {
        Vec::new()
    } else {
        parse_ls_files(&run_git(
            source,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--directory",
                "-z",
            ],
        )?)
    };
    // The ignored files are half the point — `.env` is in `.gitignore` on
    // every project that has one — and git only lists them when asked
    // separately.
    entries.extend(parse_ls_files(&run_git(
        source,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
    )?));

    let mut seen = HashSet::new();
    entries.retain(|entry| seen.insert(entry.clone()));
    Ok(entries)
}

/// How many commits `head` has that `base` does not: what removing `head`
/// would lose once it is no longer checked out anywhere.
fn commits_ahead(dir: &Path, base: &str, head: &str) -> Result<u32, GitError> {
    // Read as revisions, never as options.
    for name in [base, head] {
        if name.trim().is_empty() || name.starts_with('-') {
            return Err(GitError::Failed(format!("'{name}' is not a branch name")));
        }
    }
    let range = format!("{base}..{head}");
    let count = run_git(dir, &["rev-list", "--count", &range, "--"])?;
    count
        .trim()
        .parse()
        .map_err(|_| GitError::Failed(format!("git counted '{}'", count.trim())))
}

/// `commits_ahead` for the window (lib/workerControl.ts): whether a worker's
/// branch still holds work its target branch does not.
pub async fn git_commits_ahead(path: String, base: String, head: String) -> Result<u32, String> {
    off_thread(move || Ok(commits_ahead(Path::new(&path), &base, &head)?)).await
}

/// git's empty tree in a SHA-1 repository: the answer when git cannot be asked
/// for the one matching the repository's own object format.
const EMPTY_TREE_SHA1: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Past these, a listing or a diff stops being something to read on screen and
/// starts being something that freezes the window drawing it.
const MAX_CHANGED_FILES: usize = 2000;
const MAX_DIFF_BYTES: usize = 512 * 1024;

/// What `changes` may read in all to count the lines of untracked files. A
/// stray `target/` nobody ignored would otherwise be read in full before the
/// list could show. Past it, files are listed without a count.
const UNTRACKED_READ_BUDGET: u64 = 16 * 1024 * 1024;

/// Enough of a file to tell text from binary, the way git does.
const BINARY_SNIFF_BYTES: usize = 8000;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    /// Relative to the root of the working tree, in git's forward slashes.
    pub path: String,
    /// Where a renamed or copied file came from.
    pub old_path: Option<String>,
    /// git's letter — M, A, D, R, C, T, U — or "?" for an untracked file.
    pub status: String,
    /// None for a binary file, where a line count means nothing.
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub binary: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    /// The working tree the paths are relative to.
    pub root: String,
    /// Short sha of the commit compared against; None before the first commit.
    pub base: Option<String>,
    pub files: Vec<ChangedFile>,
    /// More files changed than `files` holds.
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    /// Unified diff, as `git diff` prints it.
    pub text: String,
    pub binary: bool,
    /// Cut at a line boundary past `MAX_DIFF_BYTES`.
    pub truncated: bool,
}

/// The next NUL-terminated token of `rest`, which moves past it.
fn next_token<'a>(rest: &mut &'a str) -> Option<&'a str> {
    let (token, after) = rest.split_once('\0')?;
    *rest = after;
    Some(token)
}

/// Reads `diff --raw --numstat -z`: one diff for both lists, where two calls
/// each walked the whole working tree again.
///
/// The raw records come first, each opening with ':' (the modes and shas,
/// then the status), followed by one path — two for a rename or a copy, the
/// source first. The score glued to R and C ("R087") is dropped; the letter is
/// what the list shows. What follows the last of them is `--numstat`'s.
fn parse_raw_numstat(output: &str) -> (Vec<ChangedFile>, NumStat) {
    let mut rest = output;
    let mut files = Vec::new();
    while rest.starts_with(':') {
        let Some(header) = next_token(&mut rest) else {
            break;
        };
        let letter: String = header
            .rsplit(' ')
            .next()
            .unwrap_or_default()
            .chars()
            .take(1)
            .collect();
        let Some(first) = next_token(&mut rest) else {
            break;
        };
        let (old_path, path) = if letter == "R" || letter == "C" {
            let Some(second) = next_token(&mut rest) else {
                break;
            };
            (Some(first.to_string()), second.to_string())
        } else {
            (None, first.to_string())
        };
        files.push(ChangedFile {
            path,
            old_path,
            status: letter,
            ..ChangedFile::default()
        });
    }
    (files, parse_numstat(rest))
}

/// Lines added and deleted per path; None for a binary file.
type NumStat = HashMap<String, (Option<u32>, Option<u32>)>;

/// Reads `diff --numstat -z`, keyed by the path a file has now.
///
/// A plain record is "added<TAB>deleted<TAB>path"; a rename leaves the path
/// empty and puts the source and the destination in the next two tokens. A
/// binary file counts "-" for both, which parses as None.
fn parse_numstat(output: &str) -> NumStat {
    let mut tokens = output.split('\0');
    let mut stats = HashMap::new();

    while let Some(record) = tokens.next() {
        if record.is_empty() {
            continue;
        }
        let mut fields = record.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            let _source = tokens.next();
            let Some(destination) = tokens.next() else {
                break;
            };
            destination.to_string()
        } else {
            path.to_string()
        };
        stats.insert(path, (added.parse().ok(), deleted.parse().ok()));
    }
    stats
}

/// What git calls a binary file: a NUL byte near the start.
fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|byte| *byte == 0)
}

/// Reads at most `limit` bytes, plus one to learn whether there was more.
fn read_head(path: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn count_lines(text: &[u8]) -> u32 {
    let breaks = text.iter().filter(|byte| **byte == b'\n').count();
    let unterminated = text.last().is_some_and(|byte| *byte != b'\n');
    (breaks + usize::from(unterminated)) as u32
}

/// Line counts of the untracked files that made it into the list — git has no
/// diff for them, so every line is an addition.
///
/// Run after the list is cut, and within `UNTRACKED_READ_BUDGET`: a file too
/// big to show whole, or one past the budget, is only sniffed for binary
/// content and keeps no count.
fn count_untracked(root: &Path, files: &mut [ChangedFile]) {
    let mut budget = UNTRACKED_READ_BUDGET;
    for file in files.iter_mut().filter(|file| file.status == "?") {
        let path = root.join(&file.path);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let countable = size <= MAX_DIFF_BYTES as u64 && size <= budget;
        let limit = if countable {
            size as usize
        } else {
            BINARY_SNIFF_BYTES
        };
        let Some(bytes) = read_head(&path, limit) else {
            continue;
        };
        budget = budget.saturating_sub(bytes.len() as u64);
        file.binary = looks_binary(&bytes);
        if countable && !file.binary {
            file.additions = Some(count_lines(&bytes));
            file.deletions = Some(0);
        }
    }
}

/// Keeps a diff readable: cut at the last line that fits.
fn cap_diff(text: String) -> FileDiff {
    let binary = text.lines().any(|line| line.starts_with("Binary files "));
    if text.len() <= MAX_DIFF_BYTES {
        return FileDiff {
            text,
            binary,
            truncated: false,
        };
    }
    // A newline is one byte in UTF-8, so cutting after one never splits a
    // character.
    let cut = text.as_bytes()[..MAX_DIFF_BYTES]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    FileDiff {
        text: text[..cut].to_string(),
        binary,
        truncated: true,
    }
}

/// The diff git would print for a new file, built here because git has none
/// for a file it does not track.
fn untracked_diff(root: &Path, path: &str) -> FileDiff {
    let Some(bytes) = read_head(&root.join(path), MAX_DIFF_BYTES) else {
        return FileDiff::default();
    };
    let header = format!("diff --git a/{path} b/{path}\nnew file\n--- /dev/null\n+++ b/{path}\n");
    if looks_binary(&bytes) {
        return FileDiff {
            text: format!("{header}Binary files /dev/null and b/{path} differ\n"),
            binary: true,
            truncated: false,
        };
    }

    let truncated = bytes.len() > MAX_DIFF_BYTES;
    let shown = &bytes[..bytes.len().min(MAX_DIFF_BYTES)];
    let body = String::from_utf8_lossy(shown);
    let lines: Vec<&str> = body.lines().collect();
    let mut text = format!("{header}@@ -0,0 +1,{} @@\n", lines.len());
    for line in lines {
        text.push('+');
        text.push_str(line);
        text.push('\n');
    }
    FileDiff {
        text,
        binary: false,
        truncated,
    }
}

/// The top of the working tree holding `dir`. Every listing runs from there:
/// `ls-files` only reports below the directory it runs in, and the diff paths
/// are root-relative anyway.
fn work_root(dir: &Path) -> Result<PathBuf, GitError> {
    let toplevel = run_git(dir, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(toplevel.trim()))
}

/// Where a diff starts.
struct DiffFrom {
    rev: String,
    /// The empty tree of a repository with no commit yet: there is no sha to
    /// show for it.
    unborn: bool,
}

/// The empty tree in the repository's own object format — a SHA-256
/// repository has a different one. stdin is closed for every git call, so
/// `--stdin` hashes nothing.
fn empty_tree(root: &Path) -> String {
    run_git(root, &["hash-object", "-t", "tree", "--stdin"])
        .ok()
        .map(|sha| sha.trim().to_string())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| EMPTY_TREE_SHA1.to_string())
}

/// The commit a diff starts from.
///
/// With `base`, where the branch left it — `merge-base base HEAD` — so the
/// diff holds everything done on this branch, committed or not. Without it,
/// HEAD: only what is not committed yet. An unborn HEAD falls back on the
/// empty tree.
fn diff_from(root: &Path, base: Option<&str>) -> Result<DiffFrom, GitError> {
    if let Some(base) = base.map(str::trim).filter(|base| !base.is_empty()) {
        // A leading dash would reach git as an option, not a ref.
        if base.starts_with('-') {
            return Err(GitError::Failed(format!("'{base}' is not a ref")));
        }
        let rev = run_git(root, &["merge-base", base, "HEAD"])?
            .trim()
            .to_string();
        return Ok(DiffFrom { rev, unborn: false });
    }
    match run_git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]) {
        Ok(sha) if !sha.trim().is_empty() => Ok(DiffFrom {
            rev: "HEAD".to_string(),
            unborn: false,
        }),
        Err(GitError::Missing) => Err(GitError::Missing),
        _ => Ok(DiffFrom {
            rev: empty_tree(root),
            unborn: true,
        }),
    }
}

fn changes(dir: &Path, base: Option<&str>) -> Result<Changes, GitError> {
    let root = work_root(dir)?;
    let DiffFrom { rev: from, unborn } = diff_from(&root, base)?;
    // The diff and the untracked files side by side: each walks the whole
    // working tree, which on a big repository is most of the time this takes.
    let (diff, untracked) = std::thread::scope(|scope| {
        let untracked =
            scope.spawn(|| run_git(&root, &["ls-files", "--others", "--exclude-standard", "-z"]));
        let diff = run_git(
            &root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-color",
                "-M",
                "--raw",
                "--numstat",
                "-z",
                &from,
                "--",
            ],
        );
        let untracked = untracked
            .join()
            .unwrap_or_else(|_| Err(GitError::Failed("git ls-files stopped".to_string())));
        (diff, untracked)
    });

    let (mut files, stats) = parse_raw_numstat(&diff?);
    for file in &mut files {
        if let Some((added, deleted)) = stats.get(&file.path) {
            file.additions = *added;
            file.deletions = *deleted;
            file.binary = added.is_none() && deleted.is_none();
        }
    }

    files.extend(
        parse_ls_files(&untracked?)
            .into_iter()
            .map(|path| ChangedFile {
                path,
                status: "?".to_string(),
                ..ChangedFile::default()
            }),
    );
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let truncated = files.len() > MAX_CHANGED_FILES;
    files.truncate(MAX_CHANGED_FILES);
    // Only now, on what is left: the files cut above are never read.
    count_untracked(&root, &mut files);

    let base = if unborn {
        None
    } else {
        run_git(&root, &["rev-parse", "--short", &from])
            .ok()
            .map(|sha| sha.trim().to_string())
    };

    Ok(Changes {
        root: display(&root),
        base,
        files,
        truncated,
    })
}

fn file_diff(
    dir: &Path,
    file: &str,
    old_path: Option<&str>,
    untracked: bool,
    base: Option<&str>,
) -> Result<FileDiff, GitError> {
    // Both come back from the UI, and the untracked one is read straight off
    // the disk: nothing outside the working tree, nothing inside `.git`.
    if !is_safe_entry(file) || old_path.is_some_and(|old| !is_safe_entry(old)) {
        return Err(GitError::Failed(format!(
            "'{file}' is not a path inside the working tree"
        )));
    }
    let root = work_root(dir)?;
    if untracked {
        return Ok(untracked_diff(&root, file));
    }

    let from = diff_from(&root, base)?.rev;
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-color",
        "-M",
        from.as_str(),
        "--",
        file,
    ];
    // Both ends of a rename, or git sees a deletion and an addition in two
    // separate calls instead of one move.
    if let Some(old) = old_path {
        args.push(old);
    }
    Ok(cap_diff(run_git(&root, &args)?))
}

/// What changed in the working tree holding `path`: tracked changes against
/// HEAD — or against where `base` forked, to take in the branch's commits too —
/// plus the untracked files.
pub async fn git_changes(path: String, base: Option<String>) -> Result<Changes, String> {
    off_thread(move || Ok(changes(Path::new(&path), base.as_deref())?)).await
}

/// The unified diff of one file of `git_changes`, read-only.
pub async fn git_file_diff(
    path: String,
    file: String,
    old_path: Option<String>,
    untracked: bool,
    base: Option<String>,
) -> Result<FileDiff, String> {
    off_thread(move || {
        Ok(file_diff(
            Path::new(&path),
            &file,
            old_path.as_deref(),
            untracked,
            base.as_deref(),
        )?)
    })
    .await
}

/// Cache behind `shared_root`, at module level so the commands that change
/// what a path means — creating, removing or pruning worktrees — can drop it.
static SHARED_ROOTS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

/// Forgets every cached resolution. Called after anything that changes the
/// worktree topology: a path that stopped being a worktree, or just became
/// one, would otherwise keep its old answer for the life of the process and
/// route boards to the wrong repository.
pub fn forget_shared_roots() {
    if let Some(cache) = SHARED_ROOTS.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.clear();
        }
    }
}

/// Root of the main repository, but only when `cwd` is a linked worktree.
///
/// None otherwise — no git, no repository, or already the main repository —
/// which lets the caller keep the path it had rather than special-case us.
///
/// Cached for the life of the process. Every board read and every board write
/// goes through this, and the UI polls the board; without the cache each poll
/// would spawn a git process. The worktree commands in this file clear the
/// cache when they change the topology; a worktree made or removed behind the
/// app's back (a terminal running `git worktree add` by hand) keeps its stale
/// answer until one of them runs, or the app restarts.
pub fn shared_root(cwd: &str) -> Option<String> {
    let cache = SHARED_ROOTS.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(cache) = cache.lock() {
        if let Some(known) = cache.get(cwd) {
            return known.clone();
        }
    }

    // `layout` and not `repo_info`: the branch and the head sha would cost two
    // more child processes each, and this — the cold path of every board read —
    // has no use for either.
    let resolved = layout(Path::new(cwd))
        .ok()
        .filter(is_linked_worktree)
        .as_ref()
        .and_then(main_root);

    if let Ok(mut cache) = cache.lock() {
        cache.insert(cwd.to_string(), resolved.clone());
    }
    resolved
}

/// Keeps the folder `dir` of `root` (`.hangar`) out of `git status` through
/// the repository's own `info/exclude`, which is never committed.
///
/// Nothing is written when git already tracks `probe`, a file in that folder
/// (the person commits it on purpose), or already ignores it (a `.gitignore`,
/// their global excludes, or this same rule from an earlier run). Best effort:
/// without git, outside a repository, or when the file cannot be written,
/// things stay as they were.
pub(crate) fn exclude_locally(root: &Path, dir: &str, probe: &str) {
    if run_git(root, &["ls-files", "--error-unmatch", "--", probe]).is_ok()
        || run_git(root, &["check-ignore", "-q", "--", probe]).is_ok()
    {
        return;
    }
    // One call, two answers in the order asked: where `root` sits in the
    // repository (empty at its top), and the exclude file — the main one, for
    // a linked worktree, which is where git reads it.
    let Ok(out) = run_git(
        root,
        &["rev-parse", "--show-prefix", "--git-path", "info/exclude"],
    ) else {
        return;
    };
    let mut lines = out.lines();
    let prefix = lines.next().unwrap_or_default().trim();
    let Some(exclude) = lines.next().map(str::trim).filter(|path| !path.is_empty()) else {
        return;
    };
    let exclude = root.join(exclude);
    let mut body = std::fs::read_to_string(&exclude).unwrap_or_default();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&format!(
        "# Hangar.AI: the workspace's local task board\n/{prefix}{dir}/\n"
    ));
    if let Some(parent) = exclude.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&exclude, body);
}

/// What the git view costs on a big repository: the looks the inspector,
/// the pane headers and the git view take, on a repository named by
/// `HANGAR_BENCH_REPO` (the audit used nodejs/node: 49k commits, 57k files,
/// 300 files edited, 50 untracked, 20 worktrees).
///
/// ```text
/// HANGAR_BENCH_REPO=~/dev/bench/big-node cargo test --release -p hangar-core \
///     --lib git::bench -- --ignored --nocapture
/// ```
#[cfg(test)]
mod bench {
    use super::*;
    use crate::board::bench::{timed, Latencies};

    #[test]
    #[ignore = "a benchmark: run it in release with --ignored --nocapture"]
    fn git_view_on_a_big_repository() {
        let Some(repo) = std::env::var_os("HANGAR_BENCH_REPO") else {
            println!("HANGAR_BENCH_REPO is not set: nothing to measure");
            return;
        };
        let repo = PathBuf::from(repo);
        let (mut info, mut looks, mut big, mut listed) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let biggest = std::env::var("HANGAR_BENCH_FILE").unwrap_or_default();
        for _ in 0..7 {
            timed(&mut info, || repo_info(&repo));
            let found = timed(&mut looks, || changes(&repo, None).unwrap());
            assert!(!found.files.is_empty());
            if !biggest.is_empty() {
                timed(&mut big, || {
                    file_diff(&repo, &biggest, None, false, None).unwrap()
                });
            }
            timed(&mut listed, || list_worktrees(&repo).unwrap());
        }
        println!("{}", Latencies(info).line("git_repo_info"));
        println!("{}", Latencies(looks).line("git_changes"));
        if !big.is_empty() {
            println!("{}", Latencies(big).line("git_file_diff (big file)"));
        }
        println!("{}", Latencies(listed).line("git_worktree_list"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = "worktree /repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /repo/../feature\nHEAD def456\nbranch refs/heads/feature\nlocked busy testing\n\nworktree /repo/../detached\nHEAD 0f0f0f0\ndetached\nprunable gitdir file points to non-existent location\n";

    #[test]
    fn reads_every_block_of_the_porcelain_listing() {
        let worktrees = parse_worktree_list(PORCELAIN);

        assert_eq!(worktrees.len(), 3);
        assert_eq!(worktrees[0].path, "/repo");
        assert_eq!(worktrees[0].head.as_deref(), Some("abc123"));
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert!(worktrees[0].is_main);
        assert!(!worktrees[1].is_main);
        assert_eq!(worktrees[1].locked.as_deref(), Some("busy testing"));
        assert!(worktrees[2].detached);
        assert_eq!(worktrees[2].branch, None);
        assert!(worktrees[2].prunable.is_some());
        // Nothing in the porcelain says where the caller is standing.
        assert!(worktrees.iter().all(|worktree| !worktree.is_current));
    }

    /// A lock with no reason is still a lock, and the UI decides on Some vs
    /// None rather than on the text.
    #[test]
    fn a_lock_without_a_reason_is_still_reported() {
        let worktrees =
            parse_worktree_list("worktree /repo\nHEAD abc\nbranch refs/heads/main\nlocked\n");

        assert_eq!(worktrees[0].locked.as_deref(), Some(""));
    }

    #[test]
    fn a_bare_main_worktree_carries_no_branch() {
        let worktrees = parse_worktree_list(
            "worktree /repo.git\nbare\n\nworktree /wt\nHEAD abc\nbranch refs/heads/main\n",
        );

        assert!(worktrees[0].bare);
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[0].head, None);
        assert_eq!(worktrees[1].branch.as_deref(), Some("main"));
    }

    /// The last block has no blank line after it on some git versions.
    #[test]
    fn the_last_block_survives_a_missing_trailing_blank_line() {
        let worktrees = parse_worktree_list("worktree /repo\nHEAD abc\n\nworktree /wt\nHEAD def");

        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[1].path, "/wt");
    }

    #[test]
    fn the_diagnostic_skips_progress_and_usage_noise() {
        let stderr = b"Preparing worktree (checking out 'feature')\nfatal: 'feature' is already used by worktree at '/wt'\n";
        assert_eq!(
            diagnostic(stderr),
            "'feature' is already used by worktree at '/wt'"
        );

        let usage =
            b"error: unknown option `path-format=absolute'\nusage: git rev-parse --parseopt\n";
        assert_eq!(diagnostic(usage), "unknown option `path-format=absolute'");
        assert!(worth_retrying(&diagnostic(usage)));
    }

    #[test]
    fn only_the_refusals_a_confirmation_answers_ask_for_force() {
        assert!(recoverable_by_force(
            "'/wt' contains modified or untracked files, use --force to delete it"
        ));
        assert!(recoverable_by_force(
            "cannot remove a locked working tree, lock reason: busy"
        ));
        assert!(!recoverable_by_force("'/wt' is not a working tree"));
        // "locked" the word is not "locked" the state: a path is free to
        // contain it, and force fixes nothing about this refusal.
        assert!(!recoverable_by_force(
            "'C:/wt/fix-unblocked-login' is not a working tree"
        ));
    }

    /// Old gits echo an option they do not know to stdout and exit 0, so a
    /// `--path-format` line has to read as "try the next spelling", never as a
    /// worktree root.
    #[test]
    fn an_echoed_unknown_option_is_not_a_layout() {
        let dir = Path::new("/repo");
        assert!(parse_layout(dir, "--path-format=absolute\n/repo/.git\n/repo/.git\n").is_none());

        let layout = parse_layout(dir, "/repo\n/repo/.git\n/repo/.git\n").expect("real layout");
        assert_eq!(layout.root.as_deref(), Some(Path::new("/repo")));
    }

    #[test]
    fn branch_refs_are_shortened() {
        assert_eq!(short_branch("refs/heads/feature/x"), "feature/x");
        assert_eq!(short_branch("refs/tags/v1"), "refs/tags/v1");
    }

    #[test]
    fn paths_compare_the_way_the_platform_does() {
        assert_eq!(compare_key("/repo/wt/"), compare_key("/repo/wt"));
        #[cfg(windows)]
        {
            assert_eq!(compare_key(r"C:\Repo\Wt"), compare_key("c:/repo/wt"));
        }
    }

    /// One `for-each-ref` line in the order `BRANCH_FORMAT` asks for.
    fn branch_line(fields: &[&str]) -> String {
        fields.join("\0")
    }

    #[test]
    fn every_field_of_a_branch_listing_is_read() {
        let listing = [
            branch_line(&[
                "refs/heads/main",
                "main",
                "*",
                "C:/repo",
                "a0db3e1",
                "1786373144",
                "simnJS",
                "Send the site's responses with security headers",
                "origin/main",
                "[ahead 1, behind 2]",
            ]),
            branch_line(&[
                "refs/remotes/origin/HEAD",
                "origin",
                " ",
                "",
                "a0db3e1",
                "1786373144",
                "simnJS",
                "Send the site's responses with security headers",
                "",
                "",
            ]),
            branch_line(&[
                "refs/remotes/origin/imgbot",
                "origin/imgbot",
                " ",
                "",
                "e113bb2",
                "1786107810",
                "ImgBotApp",
                "[ImgBot] Optimize images",
                "",
                "",
            ]),
            branch_line(&[
                "refs/heads/lonely",
                "lonely",
                " ",
                "",
                "861265c",
                "",
                "",
                "",
                "",
                "[gone]",
            ]),
        ]
        .join("\n");

        let branches = parse_branches(&listing);

        // The `origin/HEAD` symref is not a branch anyone checks out.
        assert_eq!(branches.len(), 3);

        let main = &branches[0];
        assert_eq!(main.name, "main");
        assert_eq!(main.kind, "local");
        assert!(main.is_current);
        assert_eq!(main.worktree_path.as_deref(), Some("C:/repo"));
        assert_eq!(main.head.as_deref(), Some("a0db3e1"));
        assert_eq!(main.committer_date, Some(1_786_373_144));
        assert_eq!(main.author_name.as_deref(), Some("simnJS"));
        assert_eq!(main.upstream.as_deref(), Some("origin/main"));
        assert_eq!(main.track.as_deref(), Some("[ahead 1, behind 2]"));

        // A remote keeps the remote in its name, and is never the current one.
        assert_eq!(branches[1].name, "origin/imgbot");
        assert_eq!(branches[1].kind, "remote");
        assert!(!branches[1].is_current);
        assert_eq!(branches[1].worktree_path, None);

        // Everything git had nothing to say about is absent, not empty.
        assert_eq!(branches[2].name, "lonely");
        assert_eq!(branches[2].committer_date, None);
        assert_eq!(branches[2].author_name, None);
        assert_eq!(branches[2].subject, None);
        assert_eq!(branches[2].upstream, None);
        assert_eq!(branches[2].track.as_deref(), Some("[gone]"));
    }

    /// The reason the fields are NUL-separated rather than tab-separated: a
    /// commit subject is free text and regularly holds both.
    #[test]
    fn a_subject_keeps_the_tabs_and_quotes_in_it() {
        let listing = branch_line(&[
            "refs/heads/wip",
            "wip",
            " ",
            "",
            "abc1234",
            "1786000000",
            "dev",
            "fix:\tstop quoting \"paths\" twice",
            "",
            "",
        ]);

        let branches = parse_branches(&listing);

        assert_eq!(branches.len(), 1);
        assert_eq!(
            branches[0].subject.as_deref(),
            Some("fix:\tstop quoting \"paths\" twice")
        );
    }

    /// git sorted the listing; a branch whose date is missing sorts wherever
    /// git put it, and re-sorting here could only move it somewhere else.
    #[test]
    fn the_order_git_printed_is_the_order_that_comes_out() {
        let listing = [
            branch_line(&[
                "refs/heads/newest",
                "newest",
                "*",
                "",
                "aaa",
                "300",
                "",
                "",
                "",
                "",
            ]),
            branch_line(&[
                "refs/heads/dateless",
                "dateless",
                " ",
                "",
                "bbb",
                "",
                "",
                "",
                "",
                "",
            ]),
            branch_line(&[
                "refs/heads/oldest",
                "oldest",
                " ",
                "",
                "ccc",
                "100",
                "",
                "",
                "",
                "",
            ]),
        ]
        .join("\n");

        let names: Vec<String> = parse_branches(&listing)
            .into_iter()
            .map(|branch| branch.name)
            .collect();

        assert_eq!(names, ["newest", "dateless", "oldest"]);
    }

    #[test]
    fn only_unmerged_work_turns_a_failed_delete_into_a_confirmation() {
        assert!(not_fully_merged(
            "the branch 'feature' is not fully merged."
        ));
        assert!(!not_fully_merged("branch 'feature' not found."));
        assert!(!not_fully_merged(
            "cannot delete branch 'main' used by worktree at '/wt'"
        ));
    }

    #[test]
    fn a_nul_terminated_listing_splits_where_a_path_cannot() {
        let entries = parse_ls_files(".env\0node_modules/\0a b\tc/d.txt\0");

        assert_eq!(entries, [".env", "node_modules/", "a b\tc/d.txt"]);
    }

    #[test]
    fn nothing_climbs_out_of_the_source_checkout() {
        assert!(is_safe_entry(".env"));
        assert!(is_safe_entry("packages/api/.env"));
        assert!(is_safe_entry("node_modules/"));
        assert!(!is_safe_entry("../.env"));
        assert!(!is_safe_entry("packages/../../.env"));
        assert!(!is_safe_entry(r"..\.env"));
        assert!(!is_safe_entry(".git/config"));
        assert!(!is_safe_entry("/etc/passwd"));
        assert!(!is_safe_entry(""));
    }

    #[test]
    fn a_wildcard_stops_at_a_separator_and_a_globstar_does_not() {
        assert!(matches_pattern(".env", ".env"));
        // No implicit globstar: ".env" is the one at the root, nothing else.
        assert!(!matches_pattern(".env", "packages/api/.env"));
        assert!(matches_pattern("**/.env", "packages/api/.env"));
        assert!(matches_pattern(".env.*", ".env.production"));
        assert!(!matches_pattern(".env.*", ".env"));
        assert!(matches_pattern("*.local", "settings.local"));
        assert!(!matches_pattern("*.local", "config/settings.local"));
        assert!(matches_pattern("config/?.json", "config/a.json"));
        assert!(!matches_pattern("config/?.json", "config/ab.json"));
        assert!(!matches_pattern("", ".env"));
    }

    /// What a folded directory entry has to match for the folder to be taken.
    #[test]
    fn a_folded_directory_matches_its_own_name_and_its_globstar() {
        assert!(matches_pattern(".vscode", ".vscode"));
        assert!(matches_pattern(".vscode/**", ".vscode"));
        assert!(matches_pattern(".vscode/**", ".vscode/settings.json"));
        assert!(!matches_pattern(".vscode", ".vscode/settings.json"));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        crate::testdir::scratch(&format!("git-{tag}"))
    }

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parents");
        }
        std::fs::write(path, body).expect("write");
    }

    fn read(root: &Path, relative: &str) -> String {
        std::fs::read_to_string(root.join(relative)).expect("read")
    }

    fn git(root: &Path, args: &[&str]) {
        run_git(root, args).unwrap_or_else(|_| panic!("git {args:?}"));
    }

    fn repo(tag: &str) -> PathBuf {
        let root = temp_dir(tag);
        git(&root, &["init", "-q"]);
        root
    }

    fn untracked(root: &Path) -> String {
        run_git(root, &["status", "--porcelain", "--untracked-files=all"]).expect("status")
    }

    #[test]
    fn the_board_folder_is_left_out_of_git_status_without_a_committed_file() {
        let root = repo("exclude");
        write(&root, ".hangar/board.json", "{}");
        assert!(untracked(&root).contains(".hangar/board.json"));

        exclude_locally(&root, ".hangar", ".hangar/board.json");
        assert_eq!(untracked(&root), "");
        assert!(!root.join(".gitignore").exists());
        // Already ignored by its own rule: not written twice.
        exclude_locally(&root, ".hangar", ".hangar/board.json");
        assert_eq!(
            read(&root, ".git/info/exclude")
                .matches("/.hangar/")
                .count(),
            1
        );
    }

    #[test]
    fn a_workspace_in_a_subfolder_gets_a_rule_anchored_where_it_is() {
        let root = repo("exclude-sub");
        write(&root, "apps/web/.hangar/board.json", "{}");
        write(&root, ".hangar/board.json", "{}");

        exclude_locally(&root.join("apps/web"), ".hangar", ".hangar/board.json");
        let status = untracked(&root);
        assert!(!status.contains("apps/web/.hangar"));
        // Anchored: the board at the top is the top's business.
        assert!(status.contains(".hangar/board.json"));
    }

    #[test]
    fn a_board_the_person_tracks_or_ignores_is_left_to_them() {
        let tracked = repo("exclude-tracked");
        write(&tracked, ".hangar/board.json", "{}");
        git(&tracked, &["add", ".hangar/board.json"]);
        exclude_locally(&tracked, ".hangar", ".hangar/board.json");
        assert!(!read(&tracked, ".git/info/exclude").contains("/.hangar/"));

        let ignored = repo("exclude-ignored");
        write(&ignored, ".gitignore", ".hangar/\n");
        exclude_locally(&ignored, ".hangar", ".hangar/board.json");
        assert!(!read(&ignored, ".git/info/exclude").contains("/.hangar/"));
    }

    /// A main checkout with one commit, and a linked worktree made from it.
    fn main_and_worktree(tag: &str) -> (PathBuf, PathBuf) {
        let main = repo(tag);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "T"]);
        git(&main, &["config", "commit.gpgsign", "false"]);
        // The tests compare contents byte for byte: a machine's own autocrlf
        // must not rewrite the line endings on the way through stash.
        git(&main, &["config", "core.autocrlf", "false"]);
        write(&main, "a.txt", "one\n");
        write(&main, "b.txt", "bee\n");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let linked = temp_dir(&format!("{tag}-wt")).join("wt");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                linked.to_str().unwrap(),
            ],
        );
        (main, linked)
    }

    fn stash_count(dir: &Path) -> usize {
        run_git(dir, &["stash", "list"])
            .expect("stash list")
            .lines()
            .count()
    }

    #[test]
    fn tracked_and_untracked_changes_come_across_and_the_source_is_emptied() {
        let (main, linked) = main_and_worktree("migrate-ok");
        write(&linked, "a.txt", "one\ntwo\n");
        write(&linked, "new/deep.txt", "fresh\n");
        write(&linked, "staged.txt", "staged\n");
        git(&linked, &["add", "staged.txt"]);

        let result = migrate_changes(&linked, &main).expect("migrate");

        assert_eq!(result.outcome, "moved");
        assert_eq!(result.stash, None);
        assert_eq!(read(&main, "a.txt"), "one\ntwo\n");
        assert_eq!(read(&main, "new/deep.txt"), "fresh\n");
        assert_eq!(read(&main, "staged.txt"), "staged\n");
        assert_eq!(untracked(&linked), "");
        // The stash was dropped once applied.
        assert_eq!(stash_count(&main), 0);
    }

    #[test]
    fn a_path_changed_on_both_sides_is_refused_and_nothing_moves() {
        let (main, linked) = main_and_worktree("migrate-both");
        write(&linked, "a.txt", "from worktree\n");
        write(&linked, "only-here.txt", "x\n");
        write(&main, "a.txt", "from main\n");

        let result = migrate_changes(&linked, &main).expect("migrate");

        assert_eq!(result.outcome, "conflict");
        assert_eq!(result.files, vec!["a.txt".to_string()]);
        assert_eq!(result.more, 0);
        assert_eq!(read(&linked, "a.txt"), "from worktree\n");
        assert_eq!(read(&linked, "only-here.txt"), "x\n");
        assert_eq!(read(&main, "a.txt"), "from main\n");
        assert!(!main.join("only-here.txt").exists());
        assert_eq!(stash_count(&main), 0);
    }

    #[test]
    fn a_refusal_lists_five_files_and_counts_the_rest() {
        let (main, linked) = main_and_worktree("migrate-many");
        for n in 0..8 {
            write(&linked, &format!("f{n}.txt"), "w\n");
            write(&main, &format!("f{n}.txt"), "m\n");
        }

        let result = migrate_changes(&linked, &main).expect("migrate");

        assert_eq!(result.outcome, "conflict");
        assert_eq!(result.files.len(), 5);
        assert_eq!(result.files[0], "f0.txt");
        assert_eq!(result.more, 3);
    }

    #[test]
    fn a_clean_source_has_nothing_to_move() {
        let (main, linked) = main_and_worktree("migrate-none");
        let result = migrate_changes(&linked, &main).expect("migrate");
        assert_eq!(result.outcome, "nothing");
        assert_eq!(stash_count(&main), 0);
    }

    /// The apply fails (the file exists in the target's newer HEAD): the work
    /// has left the source, so it must still be in the stash, and the target
    /// must be as it was.
    #[test]
    fn a_failed_apply_keeps_the_stash_and_loses_nothing() {
        let (main, linked) = main_and_worktree("migrate-fail");
        write(&main, "clash.txt", "committed in main\n");
        git(&main, &["add", "clash.txt"]);
        git(&main, &["commit", "-q", "-m", "add clash"]);
        write(&linked, "clash.txt", "untracked in worktree\n");
        write(&linked, "a.txt", "one\nedited\n");

        let result = migrate_changes(&linked, &main).expect("migrate");

        assert_eq!(result.outcome, "applyFailed");
        assert!(result.message.is_some());
        let stash = result.stash.expect("stash sha");
        assert_eq!(stash_count(&main), 1);
        assert_eq!(read(&main, "clash.txt"), "committed in main\n");
        assert_eq!(untracked(&main), "");
        // The work is recoverable from the stash by its sha.
        assert_eq!(
            run_git(&main, &["show", &format!("{stash}:a.txt")]).expect("show"),
            "one\nedited\n"
        );
        assert_eq!(
            run_git(&main, &["show", &format!("{stash}^3:clash.txt")]).expect("show"),
            "untracked in worktree\n"
        );
    }

    #[test]
    fn the_stash_is_named_after_both_refs() {
        let (main, linked) = main_and_worktree("migrate-name");
        write(&main, "clash.txt", "m\n");
        git(&main, &["add", "clash.txt"]);
        git(&main, &["commit", "-q", "-m", "c"]);
        write(&linked, "clash.txt", "w\n");

        migrate_changes(&linked, &main).expect("migrate");

        let list = run_git(&main, &["stash", "list"]).expect("list");
        assert!(list.contains("migration:feature-"), "{list}");
    }

    #[test]
    fn outside_a_repository_nothing_is_written() {
        let root = temp_dir("exclude-none");
        exclude_locally(&root, ".hangar", ".hangar/board.json");
        assert!(!root.join(".git").exists());
    }

    #[test]
    fn copying_takes_the_matches_and_leaves_everything_else_alone() {
        let from = temp_dir("copy-from");
        let to = temp_dir("copy-to");

        write(&from, ".env", "TOKEN=1");
        write(&from, ".env.local", "TOKEN=2");
        write(&from, "notes.local", "keep me");
        write(&from, "README.md", "tracked-looking, not asked for");
        // Anchored patterns: this one is not the ".env" the user meant.
        write(&from, "packages/api/.env", "nested");
        write(&from, ".vscode/settings.json", "{}");
        write(&from, ".vscode/nested/tasks.json", "[]");
        // Already in the new worktree: never replaced, never counted.
        write(&to, ".env.local", "the one already there");

        let entries: Vec<String> = [
            ".env",
            ".env.local",
            "notes.local",
            "README.md",
            "packages/api/.env",
            ".vscode/",
            "../escaped.local",
        ]
        .iter()
        .map(|entry| entry.to_string())
        .collect();
        let patterns: Vec<String> = [".env", ".env.*", "*.local", ".vscode/**"]
            .iter()
            .map(|pattern| pattern.to_string())
            .collect();

        let result = copy_entries(&from, &to, &entries, &patterns);

        assert_eq!(read(&to, ".env"), "TOKEN=1");
        assert_eq!(read(&to, "notes.local"), "keep me");
        // The folded directory went over whole, subdirectories included.
        assert_eq!(read(&to, ".vscode/settings.json"), "{}");
        assert_eq!(read(&to, ".vscode/nested/tasks.json"), "[]");
        assert!(!to.join("README.md").exists());
        assert!(!to.join("packages").exists());
        assert_eq!(read(&to, ".env.local"), "the one already there");
        assert!(!to
            .parent()
            .expect("temp root")
            .join("escaped.local")
            .exists());
        assert_eq!(result.copied, 4);
        assert_eq!(result.failed, 0);

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn raw_records_read_renames_as_two_paths_and_drop_the_score() {
        // As git 2.x prints `diff -M --raw --numstat -z`, a path that starts
        // with ':' included: it is a path, not the next record.
        let (files, stats) = parse_raw_numstat(concat!(
            ":100644 100644 bf30bca 0000000 M\0src/a b.rs\0",
            ":100644 100644 9405325 0fdf397 R087\0old/x.rs\0new/x.rs\0",
            ":100644 000000 b68fde2 0000000 D\0:gone.txt\0",
            "3\t1\tsrc/a b.rs\0",
            "0\t2\t\0old/x.rs\0new/x.rs\0",
            "0\t1\t:gone.txt\0",
        ));

        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, "M");
        assert_eq!(files[0].path, "src/a b.rs");
        assert_eq!(files[0].old_path, None);
        assert_eq!(files[1].status, "R");
        assert_eq!(files[1].old_path.as_deref(), Some("old/x.rs"));
        assert_eq!(files[1].path, "new/x.rs");
        assert_eq!(files[2].status, "D");
        assert_eq!(files[2].path, ":gone.txt");
        assert_eq!(stats.get("src/a b.rs"), Some(&(Some(3), Some(1))));
        assert_eq!(stats.get("new/x.rs"), Some(&(Some(0), Some(2))));
        assert_eq!(stats.get(":gone.txt"), Some(&(Some(0), Some(1))));
        assert_eq!(parse_raw_numstat("").0.len(), 0);
    }

    #[test]
    fn numstat_keys_renames_by_their_new_path_and_binaries_by_none() {
        let stats = parse_numstat(concat!(
            "3\t1\tsrc/a.rs\0",
            "0\t2\t\0old/x.rs\0new/x.rs\0",
            "-\t-\tlogo.png\0"
        ));

        assert_eq!(stats.get("src/a.rs"), Some(&(Some(3), Some(1))));
        assert_eq!(stats.get("new/x.rs"), Some(&(Some(0), Some(2))));
        assert!(!stats.contains_key("old/x.rs"));
        assert_eq!(stats.get("logo.png"), Some(&(None, None)));
    }

    #[test]
    fn a_long_diff_is_cut_on_a_line_boundary() {
        let line = "+é".repeat(40) + "\n";
        let text = line.repeat(MAX_DIFF_BYTES / line.len() + 10);

        let capped = cap_diff(text);

        assert!(capped.truncated);
        assert!(capped.text.len() <= MAX_DIFF_BYTES);
        assert!(capped.text.ends_with('\n'));
        assert!(!cap_diff("Binary files a/x and b/x differ\n".into()).truncated);
        assert!(cap_diff("diff --git a/x b/x\nBinary files a/x and b/x differ\n".into()).binary);
    }

    #[test]
    fn an_untracked_file_diffs_as_all_additions() {
        let root = temp_dir("untracked");
        write(&root, "notes.md", "one\ntwo\nthree");
        std::fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).expect("write");

        let text = untracked_diff(&root, "notes.md");
        assert!(text.text.contains("@@ -0,0 +1,3 @@\n+one\n+two\n+three\n"));
        assert!(!text.binary);

        let untracked = |path: &str| ChangedFile {
            path: path.to_string(),
            status: "?".to_string(),
            ..ChangedFile::default()
        };
        let mut files = vec![untracked("notes.md"), untracked("blob.bin")];
        count_untracked(&root, &mut files);
        assert_eq!((files[0].additions, files[0].deletions), (Some(3), Some(0)));
        assert!(untracked_diff(&root, "blob.bin").binary);
        assert!(files[1].binary);
        assert_eq!(files[1].additions, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Counting reads what it lists and no more: a file too big to show whole
    /// is only sniffed, and tracked entries are not opened at all.
    #[test]
    fn counting_untracked_files_stays_within_its_reads() {
        let root = temp_dir("count-budget");
        write(&root, "big.txt", &"line\n".repeat(MAX_DIFF_BYTES / 5 + 10));
        write(&root, "small.txt", "a\nb\n");

        let mut files = vec![
            ChangedFile {
                path: "big.txt".into(),
                status: "?".into(),
                ..ChangedFile::default()
            },
            ChangedFile {
                path: "small.txt".into(),
                status: "M".into(),
                ..ChangedFile::default()
            },
        ];
        count_untracked(&root, &mut files);

        assert_eq!(files[0].additions, None);
        assert!(!files[0].binary);
        // Tracked: its counts come from git, never from here.
        assert_eq!(files[1].additions, None);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dot_git_is_refused_whatever_its_case_and_drives_on_windows() {
        assert!(!is_safe_entry(".GIT/config"));
        assert!(!is_safe_entry("sub/.Git/HEAD"));
        assert!(is_safe_entry(".github/workflows/ci.yml"));
        #[cfg(windows)]
        {
            assert!(!is_safe_entry(r"C:foo\bar"));
            assert!(!is_safe_entry("notes.txt:hidden"));
        }
    }

    #[test]
    fn a_diff_request_cannot_leave_the_working_tree() {
        let root = temp_dir("escape");
        for path in ["../secret", ".git/config", "/etc/passwd"] {
            assert!(file_diff(&root, path, None, true, None).is_err());
        }
        assert!(file_diff(&root, "ok.txt", Some("../x"), false, None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// git, isolated from the machine's config so a signing or hook setting
    /// cannot change what the fixture repository looks like.
    fn fixture_git(dir: &Path, args: &[&str]) -> bool {
        let empty = dir.join(".fixture-gitconfig");
        let _ = std::fs::write(&empty, "");
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    /// End to end against a real repository: what an agent leaves behind —
    /// an edit, a rename, a deletion, a new file — and the commits of a branch
    /// when asked to compare with where it forked.
    #[test]
    fn changes_list_everything_an_agent_left_behind() {
        let dir = temp_dir("changes");
        if !fixture_git(&dir, &["init", "-q"]) {
            // No git on this machine: nothing to test against.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        std::fs::write(dir.join(".git/info/exclude"), ".fixture-gitconfig\n").expect("exclude");
        for (key, value) in [("user.name", "t"), ("user.email", "t@example.com")] {
            assert!(fixture_git(&dir, &["config", key, value]));
        }
        assert!(fixture_git(
            &dir,
            &["symbolic-ref", "HEAD", "refs/heads/main"]
        ));

        // Before the first commit, the empty tree is the base.
        write(&dir, "keep.txt", "a\nb\nc\n");
        let unborn = changes(&dir, None).expect("unborn changes");
        assert_eq!(unborn.base, None);
        assert_eq!(unborn.files.len(), 1);
        assert_eq!(unborn.files[0].status, "?");

        write(&dir, "move.txt", "same content\nover a few\nlines\n");
        write(&dir, "drop.txt", "bye\n");
        assert!(fixture_git(&dir, &["add", "."]));
        assert!(fixture_git(&dir, &["commit", "-q", "-m", "init"]));

        write(&dir, "keep.txt", "a\nB\nc\nd\n");
        assert!(fixture_git(&dir, &["mv", "move.txt", "moved.txt"]));
        std::fs::remove_file(dir.join("drop.txt")).expect("delete");
        write(&dir, "new/fresh.rs", "fn main() {}\n");

        let report = changes(&dir, None).expect("changes");
        assert!(report.base.is_some());
        let by_path: HashMap<&str, &ChangedFile> =
            report.files.iter().map(|f| (f.path.as_str(), f)).collect();

        let keep = by_path["keep.txt"];
        assert_eq!(keep.status, "M");
        assert_eq!((keep.additions, keep.deletions), (Some(2), Some(1)));
        let moved = by_path["moved.txt"];
        assert_eq!(moved.status, "R");
        assert_eq!(moved.old_path.as_deref(), Some("move.txt"));
        assert_eq!(by_path["drop.txt"].status, "D");
        assert_eq!(by_path["new/fresh.rs"].status, "?");
        assert_eq!(by_path["new/fresh.rs"].additions, Some(1));

        let diff = file_diff(&dir, "keep.txt", None, false, None).expect("diff");
        assert!(diff.text.contains("-b\n+B\n"));
        let rename = file_diff(&dir, "moved.txt", Some("move.txt"), false, None).expect("rename");
        assert!(rename.text.contains("rename from move.txt"));

        // Asked from a subdirectory, the listing still covers the whole tree.
        let nested = changes(&dir.join("new"), None).expect("nested");
        assert_eq!(nested.files.len(), report.files.len());

        // A branch with a commit of its own: HEAD hides it, the fork point
        // does not.
        assert!(fixture_git(&dir, &["checkout", "-q", "-b", "feature"]));
        assert!(fixture_git(&dir, &["add", "-A"]));
        assert!(fixture_git(&dir, &["commit", "-q", "-m", "work"]));
        assert!(changes(&dir, None).expect("clean").files.is_empty());
        let branch = changes(&dir, Some("main")).expect("since main");
        assert!(branch.files.iter().any(|f| f.path == "keep.txt"));
        assert!(changes(&dir, Some("--output=x")).is_err());
        assert_eq!(empty_tree(&dir), EMPTY_TREE_SHA1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A SHA-256 repository has its own empty tree: the SHA-1 one would be an
    /// object git cannot find there.
    #[test]
    fn an_unborn_sha256_repository_still_lists_its_files() {
        let dir = temp_dir("sha256");
        if !fixture_git(&dir, &["init", "-q", "--object-format=sha256"]) {
            // No git, or one too old for SHA-256 repositories.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        std::fs::write(dir.join(".git/info/exclude"), ".fixture-gitconfig\n").expect("exclude");
        write(&dir, "first.txt", "hello\n");
        assert!(fixture_git(&dir, &["add", "first.txt"]));

        assert_ne!(empty_tree(&dir), EMPTY_TREE_SHA1);
        let report = changes(&dir, None).expect("unborn sha256 changes");
        assert_eq!(report.base, None);
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].status, "A");
        let diff = file_diff(&dir, "first.txt", None, false, None).expect("diff");
        assert!(diff.text.contains("+hello"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A pattern nobody's files match is not an error, and neither is a
    /// candidate that has since been deleted.
    #[test]
    fn a_missing_source_file_is_counted_and_not_fatal() {
        let from = temp_dir("copy-gone-from");
        let to = temp_dir("copy-gone-to");
        write(&from, ".env", "TOKEN=1");

        let entries: Vec<String> = [".env", "vanished.local"]
            .iter()
            .map(|entry| entry.to_string())
            .collect();
        let patterns: Vec<String> = [".env", "*.local"]
            .iter()
            .map(|pattern| pattern.to_string())
            .collect();

        let result = copy_entries(&from, &to, &entries, &patterns);

        assert_eq!(result.copied, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(read(&to, ".env"), "TOKEN=1");

        let _ = std::fs::remove_dir_all(&from);
        let _ = std::fs::remove_dir_all(&to);
    }

    /// A repository with one commit on main, configured the way the fixture
    /// above is. None when this machine has no git.
    fn fixture_repo(tag: &str) -> Option<PathBuf> {
        let dir = temp_dir(tag);
        if !fixture_git(&dir, &["init", "-q"]) {
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        std::fs::write(dir.join(".git/info/exclude"), ".fixture-gitconfig\n").expect("exclude");
        for (key, value) in [("user.name", "t"), ("user.email", "t@example.com")] {
            assert!(fixture_git(&dir, &["config", key, value]));
        }
        assert!(fixture_git(
            &dir,
            &["symbolic-ref", "HEAD", "refs/heads/main"]
        ));
        write(&dir, "README.md", "hello\n");
        assert!(fixture_git(&dir, &["add", "."]));
        assert!(fixture_git(&dir, &["commit", "-q", "-m", "init"]));
        Some(dir)
    }

    #[test]
    fn a_branch_is_ahead_by_the_commits_its_base_does_not_have() {
        let Some(dir) = fixture_repo("ahead") else {
            return;
        };
        assert!(fixture_git(&dir, &["branch", "worker"]));
        assert_eq!(commits_ahead(&dir, "main", "worker").expect("count"), 0);

        assert!(fixture_git(&dir, &["checkout", "-q", "worker"]));
        write(&dir, "work.txt", "done\n");
        assert!(fixture_git(&dir, &["add", "."]));
        assert!(fixture_git(&dir, &["commit", "-q", "-m", "work"]));
        assert_eq!(commits_ahead(&dir, "main", "worker").expect("count"), 1);
        // Merged into its base, it holds nothing of its own any more.
        assert!(fixture_git(&dir, &["checkout", "-q", "main"]));
        assert!(fixture_git(&dir, &["merge", "-q", "--ff-only", "worker"]));
        assert_eq!(commits_ahead(&dir, "main", "worker").expect("count"), 0);

        assert!(commits_ahead(&dir, "main", "--all").is_err());
        assert!(commits_ahead(&dir, "main", "missing").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignored_only_leaves_out_what_git_would_add() {
        let Some(dir) = fixture_repo("ignored-only") else {
            return;
        };
        write(&dir, ".gitignore", ".env\n");
        write(&dir, ".env", "TOKEN=1");
        write(&dir, "notes.local", "mine");

        let all = untracked_entries(&dir, false).expect("all");
        assert!(all.contains(&".env".to_string()), "{all:?}");
        assert!(all.contains(&"notes.local".to_string()), "{all:?}");
        let ignored = untracked_entries(&dir, true).expect("ignored");
        assert!(ignored.contains(&".env".to_string()), "{ignored:?}");
        assert!(!ignored.contains(&"notes.local".to_string()), "{ignored:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn entry(path: &str, branch: Option<&str>) -> Worktree {
        Worktree {
            path: path.to_string(),
            branch: branch.map(str::to_string),
            ..Worktree::default()
        }
    }

    #[test]
    fn only_plus_lines_of_a_merged_listing_name_other_worktrees() {
        let listing = "* main
+ done
  stale
+ other/feat
* (HEAD detached at abc1234)
+ (no branch)
";
        let merged = merged_in_other_worktrees(listing);
        let mut names: Vec<_> = merged.iter().map(String::as_str).collect();
        names.sort();
        assert_eq!(names, ["done", "other/feat"]);
    }

    #[test]
    fn the_selection_leaves_out_main_current_bare_detached_and_orphaned_entries() {
        let merged: HashSet<String> = ["main", "a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        let mut main = entry("/r", Some("main"));
        main.is_main = true;
        let mut current = entry("/r-b", Some("b"));
        current.is_current = true;
        let mut bare = entry("/r-c", Some("c"));
        bare.bare = true;
        let mut detached = entry("/r-d", None);
        detached.detached = true;
        let mut gone = entry("/r-e", Some("e"));
        gone.prunable = Some("gitdir file points to non-existent location".into());
        let mut locked = entry("/r-f", Some("f"));
        locked.locked = Some(String::new());
        let unmerged = entry("/r-g", Some("g"));

        let picked = merged_candidates(
            &[
                main,
                entry("/r-a", Some("a")),
                current,
                bare,
                detached,
                gone,
                locked,
                unmerged,
            ],
            &merged,
            &[],
        );
        let paths: Vec<_> = picked.iter().map(|w| w.path.as_str()).collect();
        assert_eq!(paths, ["/r-a", "/r-f"]);
        // A lock is carried, with its empty reason, not filtered out.
        assert_eq!(picked[1].locked.as_deref(), Some(""));
    }

    /// `repo` plus one linked worktree per `(branch, extra commit)`.
    fn repo_with_worktrees(
        tag: &str,
        branches: &[(&str, bool)],
    ) -> Option<(PathBuf, Vec<PathBuf>)> {
        let dir = fixture_repo(tag)?;
        let mut paths = Vec::new();
        for (branch, commit) in branches {
            let target = temp_dir(&format!("{tag}-{branch}")).join("wt");
            let arg = target.to_string_lossy().into_owned();
            assert!(fixture_git(
                &dir,
                &["worktree", "add", "-q", "-b", branch, &arg]
            ));
            if *commit {
                write(
                    &target,
                    &format!("{branch}.txt"),
                    "done
",
                );
                assert!(fixture_git(&target, &["add", "."]));
                assert!(fixture_git(&target, &["commit", "-q", "-m", "work"]));
            }
            paths.push(target);
        }
        Some((dir, paths))
    }

    fn cleanup(dir: &Path, paths: &[PathBuf]) {
        for path in paths {
            let _ = std::fs::remove_dir_all(path.parent().unwrap_or(path));
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn found(list: &[MergedWorktree]) -> Vec<&str> {
        let mut names: Vec<_> = list.iter().map(|w| w.branch.as_str()).collect();
        names.sort();
        names
    }

    #[test]
    fn merged_worktrees_are_told_apart_from_the_ones_with_work_of_their_own() {
        let Some((dir, paths)) = repo_with_worktrees(
            "merged",
            &[("fresh", false), ("landed", true), ("ongoing", true)],
        ) else {
            return;
        };
        // `landed` is merged into main; `ongoing` is not; `fresh` has no commit
        // beyond main, which git counts as merged too.
        assert!(fixture_git(&dir, &["merge", "-q", "--ff-only", "landed"]));
        assert!(fixture_git(&dir, &["branch", "plain"]));

        let list = merged_worktrees(&dir, &[]).expect("merged");
        // `plain` is merged but has no worktree; `main` is the main checkout.
        assert_eq!(found(&list), ["fresh", "landed"]);
        assert!(list
            .iter()
            .all(|w| w.locked.is_none() && w.dirty == Some(0)));
        cleanup(&dir, &paths);
    }

    #[test]
    fn the_worktree_the_panel_is_opened_from_is_never_offered() {
        let Some((dir, paths)) = repo_with_worktrees("current", &[("one", false), ("two", false)])
        else {
            return;
        };
        // Judged against main either way, but the current one drops out.
        let list = merged_worktrees(&paths[0], &[]).expect("merged");
        assert_eq!(found(&list), ["two"]);
        cleanup(&dir, &paths);
    }

    #[test]
    fn a_locked_worktree_is_listed_with_its_lock() {
        let Some((dir, paths)) = repo_with_worktrees("locked", &[("held", false), ("free", false)])
        else {
            return;
        };
        let held = paths[0].to_string_lossy().into_owned();
        assert!(fixture_git(
            &dir,
            &["worktree", "lock", "--reason", "mine", &held]
        ));

        let list = merged_worktrees(&dir, &[]).expect("merged");
        let held = list.iter().find(|w| w.branch == "held").expect("held");
        assert_eq!(held.locked.as_deref(), Some("mine"));
        let free = list.iter().find(|w| w.branch == "free").expect("free");
        assert_eq!(free.locked, None);
        cleanup(&dir, &paths);
    }

    #[test]
    fn uncommitted_work_is_counted_and_a_plain_removal_leaves_it_alone() {
        let Some((dir, paths)) = repo_with_worktrees("dirty", &[("messy", false), ("tidy", false)])
        else {
            return;
        };
        write(&paths[0], "scratch.txt", "mine");
        write(&paths[0], "notes/a.txt", "mine");

        let list = merged_worktrees(&dir, &[]).expect("merged");
        let messy = list.iter().find(|w| w.branch == "messy").expect("messy");
        assert_eq!(messy.dirty, Some(2));
        let tidy = list.iter().find(|w| w.branch == "tidy").expect("tidy");
        assert_eq!(tidy.dirty, Some(0));

        // The removal the batch performs without force is refused on the dirty
        // one, and git's refusal is the kind a confirmation answers.
        let target = paths[0].to_string_lossy().into_owned();
        let refusal = run_git(&dir, &["worktree", "remove", &target]);
        match refusal {
            Err(GitError::Failed(message)) => assert!(recoverable_by_force(&message), "{message}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(paths[0].join("scratch.txt").exists());
        cleanup(&dir, &paths);
    }

    #[test]
    fn a_fresh_worktree_at_the_head_of_main_is_not_preselected() {
        let Some((dir, paths)) = repo_with_worktrees("fresh-only", &[("started", false)]) else {
            return;
        };
        let list = merged_worktrees(&dir, &[]).expect("merged");
        assert_eq!(found(&list), ["started"]);
        assert!(list[0].no_own_commit);
        assert!(!list[0].preselected);
        cleanup(&dir, &paths);
    }

    #[test]
    fn only_a_clean_free_worktree_with_commits_of_its_own_is_preselected() {
        let Some((dir, paths)) = repo_with_worktrees(
            "preselect",
            &[
                ("good", true),
                ("open", true),
                ("messy", true),
                ("held", true),
            ],
        ) else {
            return;
        };
        for branch in ["good", "open", "messy", "held"] {
            assert!(fixture_git(&dir, &["merge", "-q", "--no-edit", branch]));
        }
        // Main moves on, so no head is main's any more.
        write(
            &dir,
            "later.txt",
            "x
",
        );
        assert!(fixture_git(&dir, &["add", "."]));
        assert!(fixture_git(&dir, &["commit", "-q", "-m", "later"]));
        write(&paths[2], "scratch.txt", "mine");
        let held = paths[3].to_string_lossy().into_owned();
        assert!(fixture_git(&dir, &["worktree", "lock", &held]));
        // A pane is open in `open`; the spelling differs from git's on purpose.
        let pane = paths[1].to_string_lossy().replace('\\', "/");

        let list = merged_worktrees(&dir, &[pane]).expect("merged");
        let by = |branch: &str| list.iter().find(|w| w.branch == branch).expect(branch);
        assert!(list.iter().all(|w| !w.no_own_commit));
        assert!(by("good").preselected);
        assert!(by("open").held && !by("open").preselected);
        assert_eq!(by("messy").dirty, Some(1));
        assert!(!by("messy").preselected);
        assert!(by("held").locked.is_some() && !by("held").preselected);
        cleanup(&dir, &paths);
    }

    // --- worktree hooks -----------------------------------------------------

    /// The shell kind a hook goes through here, so a test writes its command
    /// in that shell's own words. None when the machine has no shell at all.
    fn hook_dialect() -> Option<&'static str> {
        let shell = hook_shell()?;
        Some(match shell.id.as_str() {
            "pwsh" | "powershell" => "powershell",
            "cmd" => "cmd",
            _ => "posix",
        })
    }

    /// Writes `WORKTREE_PATH|BASE_PATH` to `marker.txt` in the hook's folder.
    fn marker_command(dialect: &str) -> &'static str {
        match dialect {
            "powershell" => {
                "\"$env:WORKTREE_PATH|$env:BASE_PATH\" | Out-File -Encoding ascii marker.txt"
            }
            "cmd" => "echo %WORKTREE_PATH%^|%BASE_PATH%> marker.txt",
            _ => "printf '%s|%s' \"$WORKTREE_PATH\" \"$BASE_PATH\" > marker.txt",
        }
    }

    fn sleep_command(dialect: &str) -> &'static str {
        match dialect {
            "powershell" => "Start-Sleep -Seconds 60",
            "cmd" => "ping -n 60 127.0.0.1 > nul",
            _ => "sleep 60",
        }
    }

    fn wt_str(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    const HOOK_WAIT: Duration = Duration::from_secs(60);

    #[test]
    fn a_post_create_hook_runs_in_the_worktree_and_sees_its_variables() {
        let Some(dialect) = hook_dialect() else {
            return;
        };
        let (main, linked) = main_and_worktree("hook-post");
        let report = run_configured_hook(
            Some(marker_command(dialect)),
            &wt_str(&main),
            &wt_str(&linked),
            HOOK_WAIT,
        )
        .expect("a configured hook runs");
        assert!(report.ok, "{report:?}");

        // The file landed in the worktree, so that is where it ran.
        let marker = std::fs::read_to_string(linked.join("marker.txt")).expect("marker");
        let marker = marker.trim().to_string();
        let (worktree_seen, base_seen) = marker.split_once('|').expect("two variables");
        let tail = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(worktree_seen.contains(&tail(&linked)), "{marker}");
        assert!(base_seen.contains(&tail(&main)), "{marker}");
        assert!(!main.join("marker.txt").exists());
    }

    #[test]
    fn a_failing_post_create_hook_keeps_the_worktree_and_reports() {
        let Some(_) = hook_dialect() else {
            return;
        };
        let (main, linked) = main_and_worktree("hook-post-fail");
        let report =
            run_configured_hook(Some("exit 3"), &wt_str(&main), &wt_str(&linked), HOOK_WAIT)
                .expect("ran");
        assert!(!report.ok && !report.timed_out, "{report:?}");
        assert_eq!(report.exit_code, Some(3));
        assert!(linked.is_dir());
        assert!(list_worktrees(&main)
            .unwrap()
            .iter()
            .any(|entry| compare_key(&entry.path) == compare_key(&wt_str(&linked))));
    }

    #[test]
    fn nothing_runs_when_no_hook_is_set() {
        let (main, linked) = main_and_worktree("hook-unset");
        for command in [None, Some(""), Some("   ")] {
            assert!(
                run_configured_hook(command, &wt_str(&main), &wt_str(&linked), HOOK_WAIT).is_none()
            );
        }
        // Removal without a hook is plain git.
        let result = remove_worktree(&wt_str(&main), &wt_str(&linked), false, None, HOOK_WAIT)
            .expect("removed");
        assert!(result.removed && result.hook.is_none() && !result.hook_blocked);
        assert!(!linked.exists());
    }

    #[test]
    fn a_failing_pre_remove_hook_keeps_the_worktree_and_says_why() {
        let Some(dialect) = hook_dialect() else {
            return;
        };
        let (main, linked) = main_and_worktree("hook-pre-fail");
        let command = match dialect {
            "powershell" => "Write-Output 'teardown refused'; exit 4",
            "cmd" => "echo teardown refused& exit 4",
            _ => "echo 'teardown refused'; exit 4",
        };
        let result = remove_worktree(
            &wt_str(&main),
            &wt_str(&linked),
            false,
            Some(command),
            HOOK_WAIT,
        )
        .expect("answers");
        assert!(!result.removed && result.hook_blocked && !result.needs_force);
        let message = result.message.expect("message");
        assert!(message.contains("exit 4"), "{message}");
        assert!(message.contains("teardown refused"), "{message}");
        assert!(linked.is_dir());
    }

    #[test]
    fn a_passing_pre_remove_hook_lets_the_removal_go_through() {
        let Some(dialect) = hook_dialect() else {
            return;
        };
        let (main, linked) = main_and_worktree("hook-pre-ok");
        // Writes outside the worktree, which is about to disappear.
        let result = remove_worktree(
            &wt_str(&main),
            &wt_str(&linked),
            false,
            Some(match dialect {
                "powershell" => "Write-Output ok",
                "cmd" => "echo ok",
                _ => "echo ok",
            }),
            HOOK_WAIT,
        )
        .expect("removed");
        assert!(result.removed, "{result:?}");
        assert!(result.hook.is_some_and(|hook| hook.ok));
        assert!(!linked.exists());
    }

    #[test]
    fn a_hook_that_outlives_the_timeout_is_stopped() {
        let Some(dialect) = hook_dialect() else {
            return;
        };
        let (main, linked) = main_and_worktree("hook-timeout");
        let started = Instant::now();
        let report = run_configured_hook(
            Some(sleep_command(dialect)),
            &wt_str(&main),
            &wt_str(&linked),
            Duration::from_millis(1500),
        )
        .expect("ran");
        assert!(report.timed_out && !report.ok, "{report:?}");
        assert!(started.elapsed() < Duration::from_secs(30));

        // And as a pre-remove hook, a timeout aborts the removal.
        let result = remove_worktree(
            &wt_str(&main),
            &wt_str(&linked),
            false,
            Some(sleep_command(dialect)),
            Duration::from_millis(1500),
        )
        .expect("answers");
        assert!(result.hook_blocked && !result.removed);
        assert!(result.message.unwrap().contains("timed out"));
        assert!(linked.is_dir());
    }

    // --- leftovers ------------------------------------------------------------

    /// `<root>/repo` with a commit, and `<root>/repo-worktrees/w` on `feature`.
    fn laid_out(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = temp_dir(tag);
        let main = root.join("repo");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["config", "user.email", "t@example.com"]);
        git(&main, &["config", "user.name", "T"]);
        git(&main, &["config", "commit.gpgsign", "false"]);
        write(&main, "a.txt", "one\n");
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "init"]);
        let linked = root.join("repo-worktrees").join("w");
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                linked.to_str().unwrap(),
            ],
        );
        (root, main, linked)
    }

    fn listed(main: &Path, folder: &Path) -> bool {
        list_worktrees(main)
            .unwrap()
            .iter()
            .any(|entry| compare_key(&entry.path) == compare_key(&wt_str(folder)))
    }

    /// A link from `at` to `target` that is a junction on Windows.
    fn link_dir(target: &Path, at: &Path) {
        #[cfg(windows)]
        {
            let status = Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                // Backslashes: `/n` in a path reads as one of its switches.
                .arg(at.to_string_lossy().replace('/', "\\"))
                .arg(target.to_string_lossy().replace('/', "\\"))
                .stdout(Stdio::null())
                .status()
                .expect("mklink");
            assert!(status.success());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, at).expect("symlink");
    }

    #[test]
    fn what_is_left_of_a_half_removed_worktree_is_swept_and_links_are_not_followed() {
        let (root, main, linked) = laid_out("sweep");
        let outside = root.join("precious");
        write(&outside, "keep.txt", "mine");
        write(&linked, "node_modules/pkg/index.js", "x");
        link_dir(&outside, &linked.join("node_modules/linked"));
        // What git leaves after giving up: its `.git` is gone, the rest stays.
        std::fs::remove_file(linked.join(".git")).unwrap();

        let result = remove_worktree(&wt_str(&main), &wt_str(&linked), true, None, HOOK_WAIT)
            .expect("swept");
        assert!(result.removed && result.message.is_none(), "{result:?}");
        assert!(!linked.exists());
        assert!(!listed(&main, &linked));
        assert_eq!(read(&outside, "keep.txt"), "mine");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_git_no_longer_lists_is_swept() {
        let (root, main, linked) = laid_out("unlisted");
        git(
            &main,
            &["worktree", "remove", "--force", linked.to_str().unwrap()],
        );
        write(&linked, "README.md", "left");
        write(&linked, "src/deep/file.ts", "left");
        assert!(!listed(&main, &linked));

        sweep_leftovers(&main, &linked, 1, Duration::ZERO).expect("swept");
        assert!(!linked.exists());
        // And once more is still fine: nothing there, nothing to say.
        sweep_leftovers(&main, &linked, 1, Duration::ZERO).expect("idempotent");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_is_swept_that_hangar_did_not_make_or_git_still_holds() {
        let (root, main, linked) = laid_out("refuse");
        let go = |folder: &Path| sweep_leftovers(&main, folder, 1, Duration::ZERO);

        // A live worktree.
        assert!(matches!(go(&linked), Err(Sweep::Refused(_))));
        assert!(linked.join("a.txt").is_file());
        // The main checkout.
        assert!(matches!(go(&main), Err(Sweep::Refused(_))));
        assert!(main.join("a.txt").is_file());
        // A folder elsewhere, however unlisted.
        let elsewhere = root.join("elsewhere");
        write(&elsewhere, "x.txt", "x");
        assert!(matches!(go(&elsewhere), Err(Sweep::Refused(_))));
        // The worktrees folder itself, and a repository of its own inside it.
        assert!(matches!(
            go(&root.join("repo-worktrees")),
            Err(Sweep::Refused(_))
        ));
        let other = root.join("repo-worktrees").join("other");
        std::fs::create_dir_all(&other).unwrap();
        git(&other, &["init", "-q"]);
        assert!(matches!(go(&other), Err(Sweep::Refused(_))));
        assert!(other.join(".git").is_dir());
        assert!(elsewhere.join("x.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_link_to_the_worktrees_folder_is_followed_and_a_link_out_of_it_is_not() {
        let (root, main, linked) = laid_out("via-link");
        git(
            &main,
            &["worktree", "remove", "--force", linked.to_str().unwrap()],
        );
        write(&linked, "left.txt", "left");
        // The same folder reached through a link spells differently but is inside.
        let alias = root.join("alias");
        link_dir(&root.join("repo-worktrees"), &alias);
        sweep_leftovers(&main, &alias.join("w"), 1, Duration::ZERO).expect("swept via link");
        assert!(!linked.exists());

        // A folder outside the worktrees folder stays refused, link or not.
        let elsewhere = root.join("elsewhere");
        write(&elsewhere, "x.txt", "x");
        let sneaky = root.join("repo-worktrees").join("sneaky");
        link_dir(&elsewhere, &sneaky);
        assert!(matches!(
            sweep_leftovers(&main, &sneaky, 1, Duration::ZERO),
            Err(Sweep::Refused(_))
        ));
        assert!(elsewhere.join("x.txt").is_file());
        let _ = std::fs::remove_dir_all(&alias);
        let _ = std::fs::remove_dir_all(&sneaky);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uncommitted_work_keeps_the_worktree_and_stale_git_words_never_reach_the_person() {
        let (root, main, linked) = laid_out("dirty");
        write(&linked, "wip.txt", "unsaved");
        let result = remove_worktree(&wt_str(&main), &wt_str(&linked), false, None, HOOK_WAIT)
            .expect("asks");
        assert!(!result.removed && result.needs_force, "{result:?}");
        assert_eq!(read(&linked, "wip.txt"), "unsaved");

        // A folder git cannot call a worktree, outside the worktrees folder.
        let stray = root.join("stray");
        write(&stray, "x.txt", "x");
        let error = remove_worktree(&wt_str(&main), &wt_str(&stray), true, None, HOOK_WAIT)
            .expect_err("refused");
        assert!(!error.contains("is not a working tree"), "{error}");
        assert!(error.contains("nothing was deleted"), "{error}");
        assert!(stray.join("x.txt").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn a_held_file_is_named_in_one_line() {
        use std::os::windows::fs::OpenOptionsExt;
        let (root, main, linked) = laid_out("held");
        git(
            &main,
            &["worktree", "remove", "--force", linked.to_str().unwrap()],
        );
        write(&linked, "free.txt", "x");
        write(&linked, "held.txt", "x");
        // Opened without sharing delete, as a program standing in it would.
        let _guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(linked.join("held.txt"))
            .unwrap();
        let Err(Sweep::Held(message)) =
            sweep_leftovers(&main, &linked, 2, Duration::from_millis(20))
        else {
            panic!("expected the folder to be held");
        };
        assert!(
            message.contains("held.txt") && !message.contains('\n'),
            "{message}"
        );
        assert!(!linked.join("free.txt").exists());
        drop(_guard);
        sweep_leftovers(&main, &linked, 1, Duration::ZERO).expect("free now");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_branch_already_gone_counts_as_deleted() {
        let (root, main, linked) = laid_out("branch-gone");
        git(
            &main,
            &["worktree", "remove", "--force", linked.to_str().unwrap()],
        );
        git(&main, &["branch", "-D", "feature"]);
        let again = run_git(&main, &["branch", "-D", "feature"]);
        let Err(GitError::Failed(message)) = again else {
            panic!("git should refuse");
        };
        assert!(
            message.contains("not found") && message.contains("branch"),
            "{message}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_hook_s_output_is_capped_to_its_end() {
        let Some(dialect) = hook_dialect() else {
            return;
        };
        let (main, linked) = main_and_worktree("hook-cap");
        let command = match dialect {
            "powershell" => "1..4000 | ForEach-Object { 'line number ' + $_ }",
            "cmd" => "for /L %i in (1,1,4000) do @echo line number %i",
            _ => "i=1; while [ $i -le 4000 ]; do echo \"line number $i\"; i=$((i+1)); done",
        };
        let report =
            run_configured_hook(Some(command), &wt_str(&main), &wt_str(&linked), HOOK_WAIT)
                .expect("ran");
        assert!(report.ok, "{report:?}");
        assert!(
            report.output.len() <= HOOK_OUTPUT_CAP + 8,
            "{}",
            report.output.len()
        );
        assert!(report.output.starts_with('…'));
        assert!(report.output.contains("line number 4000"));
    }

    // --- long paths, read-only files, orphans ---------------------------------

    #[test]
    fn a_deep_tree_with_a_read_only_file_and_a_link_out_is_removed_and_the_target_is_not() {
        let (root, main, linked) = laid_out("deep");
        let outside = root.join("precious");
        write(&outside, "keep.txt", "mine");
        write(&outside, "sub/more.txt", "mine too");
        // Past 260 characters, which only the `\\?\` form can name on Windows.
        let mut deep = linked.join("node_modules");
        for _ in 0..12 {
            deep = deep.join("a-folder-with-a-rather-long-name");
        }
        assert!(deep.to_string_lossy().len() > 300);
        std::fs::create_dir_all(verbatim(&deep)).unwrap();
        let file = deep.join("locked.txt");
        std::fs::write(verbatim(&file), "ro").unwrap();
        let mut permissions = std::fs::metadata(verbatim(&file)).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(verbatim(&file), permissions).unwrap();
        link_dir(&outside, &linked.join("node_modules/linked"));
        std::fs::remove_file(linked.join(".git")).unwrap();

        sweep_leftovers(&main, &linked, 1, Duration::ZERO).expect("swept");
        assert!(!linked.exists());
        assert_eq!(read(&outside, "keep.txt"), "mine");
        assert_eq!(read(&outside, "sub/more.txt"), "mine too");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A worktree under `repo-worktrees` on its own branch with one commit.
    fn worker_branch(main: &Path, root: &Path, name: &str) -> PathBuf {
        let folder = root.join("repo-worktrees").join(name);
        git(
            main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                name,
                folder.to_str().unwrap(),
            ],
        );
        git(&folder, &["config", "user.email", "t@example.com"]);
        git(&folder, &["config", "user.name", "T"]);
        git(&folder, &["config", "commit.gpgsign", "false"]);
        write(&folder, &format!("{name}.txt"), name);
        git(&folder, &["add", "."]);
        git(&folder, &["commit", "-q", "-m", name]);
        folder
    }

    #[test]
    fn the_orphan_sweep_removes_only_what_landed_is_clean_and_has_no_pane() {
        let (root, main, linked) = laid_out("orphans");
        // `laid_out` made `w` on `feature`: it has no commit of its own.
        let gone = worker_branch(&main, &root, "gone");
        let used = worker_branch(&main, &root, "used");
        let dirty = worker_branch(&main, &root, "dirty");
        git(
            &main,
            &[
                "merge", "-q", "--no-ff", "-m", "land", "gone", "used", "dirty",
            ],
        );
        let unlanded = worker_branch(&main, &root, "unlanded");
        write(&dirty, "wip.txt", "unsaved");
        // Ignored, as it is in a real project: it must not make it dirty.
        write(&main, ".git/info/exclude", "node_modules/\n");
        write(&gone, "node_modules/pkg/index.js", "x");
        // One the person made by hand, merged and clean, outside the folder.
        let by_hand = root.join("by-hand");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "hand",
                by_hand.to_str().unwrap(),
            ],
        );
        // What a removal left behind: no `.git`, git does not list it.
        let leftover = root.join("repo-worktrees").join("leftover");
        write(&leftover, "node_modules/pkg/index.js", "x");
        let held = vec![wt_str(&used.join("src")), wt_str(&linked)];

        let sweep = sweep_orphan_worktrees(&main, &held).expect("swept");

        assert_eq!(sweep.removed.len(), 2, "{sweep:?}");
        assert!(sweep.removed.iter().any(|line| line.contains("gone")));
        assert!(!leftover.exists());
        assert!(!gone.exists());
        assert!(run_git(&main, &["rev-parse", "--verify", "refs/heads/gone"]).is_err());
        for kept in [&used, &dirty, &unlanded, &linked, &by_hand, &main] {
            assert!(kept.exists(), "{kept:?}");
        }
        assert!(sweep.kept.iter().any(|line| line.contains("pane")));
        assert!(sweep.kept.iter().any(|line| line.contains("uncommitted")));
        assert!(
            !sweep.kept.iter().any(|line| line.contains("by-hand")),
            "{sweep:?}"
        );
        // The unlanded commit is still there to land.
        assert!(run_git(&main, &["rev-parse", "--verify", "refs/heads/unlanded"]).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_hook_leaving_a_background_process_holding_its_pipes_still_returns() {
        let dir = crate::testdir::scratch("hook");
        let started = Instant::now();
        let report = run_hook(
            "echo installed; (sleep 30 &) ; exit 0",
            dir.to_str().unwrap(),
            dir.to_str().unwrap(),
            Duration::from_secs(60),
        );
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            started.elapsed() < HOOK_DRAIN_GRACE + Duration::from_secs(10),
            "took {:?}",
            started.elapsed()
        );
        assert!(report.ok, "{report:?}");
        assert!(!report.timed_out);
        assert_eq!(report.output, "installed");
    }

    #[test]
    fn git_never_waits_for_a_password_or_a_passphrase() {
        let command = git_command(Path::new("."), &["fetch", "--all"]);
        let env = |name: &str| {
            command
                .get_envs()
                .find(|(key, _)| *key == OsStr::new(name))
                .map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
        };
        assert_eq!(env("GIT_TERMINAL_PROMPT"), Some(Some("0".to_string())));
        assert_eq!(env("GCM_INTERACTIVE"), Some(Some("never".to_string())));
        if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
            assert_eq!(
                env("GIT_SSH_COMMAND"),
                Some(Some("ssh -o BatchMode=yes".to_string()))
            );
        }
        // What a pane leaves in the environment is removed, not passed on.
        assert_eq!(env("HANGAR_BRIDGE_TOKEN"), Some(None));
    }

    #[test]
    fn only_the_commands_that_reach_a_remote_get_a_time_limit() {
        assert_eq!(network_limit(&["fetch", "--all"]), Some(NETWORK_TIMEOUT));
        assert_eq!(network_limit(&["push"]), Some(NETWORK_TIMEOUT));
        assert_eq!(network_limit(&["status"]), None);
        assert_eq!(network_limit(&["worktree", "add"]), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_command_past_its_limit_is_stopped() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let started = Instant::now();
        let err = run_limited(command, Some(Duration::from_millis(200))).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
