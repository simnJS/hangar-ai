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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

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
        .env("LANGUAGE", "");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => GitError::Missing,
        _ => GitError::Failed(format!("could not run git: {err}")),
    })?;

    if !output.status.success() {
        return Err(GitError::Failed(diagnostic(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs a git-shaped task on the blocking pool and hands its answer back.
///
/// Every command below goes through this because a synchronous
/// `#[tauri::command]` runs on the main thread — the one the window, and every
/// pane's IPC, lives on. A `git fetch` against a slow remote, or the checkout
/// a `worktree add` performs, would freeze the whole app for its duration;
/// voice_start draws the same line for the same reason.
async fn off_thread<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
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
fn compare_key(path: &str) -> String {
    #[cfg(windows)]
    let key = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
    #[cfg(not(windows))]
    let key = path.trim_end_matches('/').to_string();
    key
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
#[tauri::command]
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

#[tauri::command]
pub async fn git_worktree_list(path: String) -> Result<Vec<Worktree>, String> {
    off_thread(move || {
        let dir = Path::new(&path);
        let mut worktrees =
            parse_worktree_list(&run_git(dir, &["worktree", "list", "--porcelain"])?);

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
    })
    .await
}

/// A ref as the branch list shows it — local and remote alike, carrying
/// everything the panel labels a row with so it does not have to ask again per
/// branch.
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
#[tauri::command]
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
#[tauri::command]
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

#[tauri::command]
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
            Err(error) => Err(error.into()),
        }
    })
    .await
}

#[tauri::command]
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

#[tauri::command]
pub async fn git_worktree_remove(
    path: String,
    worktree_path: String,
    force: bool,
) -> Result<RemoveResult, String> {
    off_thread(move || {
        let mut args: Vec<&str> = vec!["worktree", "remove"];
        if force {
            // Twice on purpose. One `--force` covers a worktree with uncommitted
            // work in it; git documents a second one as what it takes to remove a
            // locked worktree, and the user has already been asked by then.
            args.push("--force");
            args.push("--force");
        }
        args.push(&worktree_path);

        match run_git(Path::new(&path), &args) {
            Ok(_) => {
                forget_shared_roots();
                Ok(RemoveResult {
                    removed: true,
                    needs_force: false,
                    message: None,
                })
            }
            Err(GitError::Failed(message)) if !force && recoverable_by_force(&message) => {
                Ok(RemoveResult {
                    removed: false,
                    needs_force: true,
                    message: Some(message),
                })
            }
            Err(error) => Err(error.into()),
        }
    })
    .await
}

#[tauri::command]
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
#[tauri::command]
pub async fn git_worktree_lock(path: String, worktree_path: String) -> Result<(), String> {
    off_thread(move || {
        run_git(Path::new(&path), &["worktree", "lock", &worktree_path])?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn git_worktree_unlock(path: String, worktree_path: String) -> Result<(), String> {
    off_thread(move || {
        run_git(Path::new(&path), &["worktree", "unlock", &worktree_path])?;
        Ok(())
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
/// directory the user did not name, and that is worth four lines. Backslash
/// counts as a separator too: on Windows it is one, and `..\x` would climb.
fn is_safe_entry(entry: &str) -> bool {
    let relative = entry.trim_end_matches('/');
    !relative.is_empty()
        && !Path::new(relative).is_absolute()
        && relative.split(['/', '\\']).all(|segment| {
            !segment.is_empty() && segment != "." && segment != ".." && segment != ".git"
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
#[tauri::command]
pub async fn git_copy_untracked(
    from: String,
    to: String,
    patterns: Vec<String>,
) -> Result<CopyResult, String> {
    off_thread(move || {
        let source = Path::new(&from);

        // git decides what the candidates are, not a directory walk of our own:
        // `--directory` folds a wholly untracked directory into one entry, and
        // that is what keeps `node_modules` from being enumerated on the way to
        // finding a `.env` beside it.
        let mut entries = parse_ls_files(&run_git(
            source,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--directory",
                "-z",
            ],
        )?);
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

        Ok(copy_entries(source, Path::new(&to), &entries, &patterns))
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
        let dir = std::env::temp_dir().join(format!(
            "hangar-git-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
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
}
