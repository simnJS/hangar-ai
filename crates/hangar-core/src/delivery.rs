//! Deliveries: a worker's branch landing on its workspace's branch.
//!
//! One at a time per repository. Each one is merged onto the branch as it is
//! now in a worktree of its own, checked with the workspace's command, read by
//! a reviewer agent with a fresh context, and only then landed, or handed back
//! to its worker with what failed. Nothing is ever pushed.
//!
//! The person's own checkout is never worked in. Every merge, check and review
//! happens in a temporary worktree that goes away whatever the outcome, and
//! landing is a fast-forward: git either moves the branch over the tested
//! merge or refuses, and a refusal is reported, never forced through — no
//! stash, no reset, no checkout over local changes.
//!
//! The window decides who may deliver and what follows an outcome — the board,
//! the worker's pane, the orchestrator (lib/deliveryControl.ts). This module
//! only does the git, the check and the review, and says what happened in
//! words the worker can act on.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shells::ShellInfo;

/// Hides the console window Windows would open for every child of a GUI app.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A check is a project's own test suite, and some take a while; one that is
/// still going after this is stuck rather than slow.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Reading a large diff with a few files opened around it.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// A checkout of a large repository, or a merge commit waiting on a signing
/// prompt nobody will answer.
const GIT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_millis(100);
/// How long a finished process's output is waited for. Past it, whatever it
/// left running still holds the pipe, and what was read so far is the answer.
const DRAIN: Duration = Duration::from_secs(2);

/// The reviewer's answer comes on stdout in one JSON document.
const OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// A check's output is only ever shown by its end.
const CHECK_BYTES: usize = 64 * 1024;
const TAIL_LINES: usize = 60;
const TAIL_BYTES: usize = 6 * 1024;
/// The diff handed to the reviewer in its prompt. A longer one is cut, and the
/// reviewer is told to read the rest of the files itself.
const DIFF_BYTES: usize = 200 * 1024;
/// Times the whole delivery is tried again because the branch it lands on
/// moved while it was being tested.
const LANDING_TRIES: usize = 3;

const SCRATCH_PREFIX: &str = "hangar-delivery-";
/// Where a delivery that passed but did not land keeps its tested merge: out
/// of the branch namespace, and safe from `git gc` until it lands.
const TESTED_REFS: &str = "refs/hangar/deliveries";

/// The tools the reviewer gets: reading only. Where a platform's Claude Code
/// has no Grep or Glob of its own, it simply has fewer.
const REVIEW_TOOLS: &str = "Read,Grep,Glob";

const VERDICT_SCHEMA: &str = r#"{"type":"object","properties":{"approve":{"type":"boolean"},"blocking":{"type":"array","items":{"type":"object","properties":{"file":{"type":"string"},"line":{"type":"integer"},"problem":{"type":"string"}},"required":["file","problem"]}},"notes":{"type":"string"}},"required":["approve","blocking","notes"]}"#;

/// What an agent session or a Hangar pane leaves in the environment of
/// anything started under it, which a check or a reviewer must not inherit:
/// a reviewer passing for the pane Hangar was started from, or for a nested
/// session of it. The user's own configuration — `CLAUDE_CONFIG_DIR`, a
/// provider's `CLAUDE_CODE_USE_*`, an API key — is left as it is.
const SCRUBBED: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_PID",
    "HANGAR_PANE_ID",
    "HANGAR_PANE_NAME",
    "HANGAR_BRIDGE_URL",
    "HANGAR_BRIDGE_TOKEN",
    "HANGAR_WORKSPACE",
    "HANGAR_AGENT",
    "HANGAR_PORT_BASE",
    "HANGAR_PORT_COUNT",
];

/* ── Processes ─────────────────────────────────────────────────── */

/// Set on the way out: every process a delivery runs is ended, and none
/// starts after.
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// A process id, with its start time where the platform tells it.
type Child = (u32, Option<u64>);

/// The processes deliveries are waiting on, so the app's exit can end them.
fn children() -> &'static Mutex<HashMap<u64, Child>> {
    static CHILDREN: OnceLock<Mutex<HashMap<u64, Child>>> = OnceLock::new();
    CHILDREN.get_or_init(Default::default)
}

/// Ends every check and review still running, and whatever they started.
/// Called when the main window goes: a test suite must not outlive the app.
pub fn cancel_all() {
    CANCELLED.store(true, Ordering::SeqCst);
    let running: Vec<Child> = children().lock().unwrap().values().copied().collect();
    for (pid, born) in running {
        kill_tree(pid, born, true);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Exited,
    TimedOut,
    Cancelled,
}

#[derive(Debug)]
struct Finished {
    ended: Ended,
    code: Option<i32>,
    /// Its stdout, or both streams when they were merged; the end of it past
    /// the capture limit.
    out: String,
    err: String,
    elapsed: Duration,
    #[cfg_attr(not(test), allow(dead_code))]
    pid: u32,
}

impl Finished {
    fn succeeded(&self) -> bool {
        self.ended == Ended::Exited && self.code == Some(0)
    }
}

struct Spawn {
    input: Option<Vec<u8>>,
    limit: Duration,
    /// Stdout and stderr through one pipe, in the order they were written:
    /// what a check printed reads the way it did in a terminal.
    merged: bool,
    cap: usize,
    /// Cleaning up still runs while the app is closing.
    cancellable: bool,
}

/// The last `cap` bytes a stream wrote.
struct Capture {
    bytes: Vec<u8>,
    cap: usize,
}

impl Capture {
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > self.cap * 2 {
            let excess = self.bytes.len() - self.cap;
            self.bytes.drain(..excess);
        }
    }

    fn text(&self) -> String {
        let start = self.bytes.len().saturating_sub(self.cap);
        String::from_utf8_lossy(&self.bytes[start..]).into_owned()
    }
}

/// Reads `pipe` to its end on a thread of its own, so a process filling one
/// pipe while nobody reads it can never block on it.
fn drain(
    mut pipe: impl Read + Send + 'static,
    cap: usize,
) -> (Arc<Mutex<Capture>>, mpsc::Receiver<()>) {
    let capture = Arc::new(Mutex::new(Capture {
        bytes: Vec::new(),
        cap,
    }));
    let (done, finished) = mpsc::channel();
    let sink = capture.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => sink.lock().unwrap().push(&buffer[..n]),
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = done.send(());
    });
    (capture, finished)
}

#[cfg(windows)]
fn started_at(pid: u32) -> Option<u64> {
    crate::procs::started(pid)
}

#[cfg(not(windows))]
fn started_at(_pid: u32) -> Option<u64> {
    None
}

/// Ends `pid` and everything below it. Each child runs in a process group of
/// its own on unix, which is exactly its tree. Windows keeps no such thing, so
/// the tree is read off the process table, as `pty_kill` does for a pane —
/// `born` tells the real children of a process that already ended from the
/// orphans of an older one that had the same id.
#[cfg(unix)]
fn kill_tree(pid: u32, _born: Option<u64>, _root_too: bool) {
    let Ok(group) = libc::pid_t::try_from(pid) else {
        return;
    };
    unsafe {
        if group > 1 && group != libc::getpgrp() {
            libc::killpg(group, libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
fn kill_tree(pid: u32, born: Option<u64>, root_too: bool) {
    let Some(table) = crate::procs::table() else {
        return;
    };
    let started = |of: u32| {
        if of == pid {
            born
        } else {
            crate::procs::started(of)
        }
    };
    let mut doomed = crate::procs::descendants(&table, pid, started);
    if root_too {
        doomed.insert(0, pid);
    }
    crate::procs::terminate(&doomed);
}

#[cfg(windows)]
fn prepare(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn prepare(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// Runs `command` to its end, or to its time limit, and keeps what it said.
/// Never more than `limit`: past it, the process and everything it started
/// are killed, and the answer says so.
fn run(mut command: Command, spawn: Spawn) -> std::io::Result<Finished> {
    if spawn.cancellable && CANCELLED.load(Ordering::SeqCst) {
        return Err(std::io::Error::other("Hangar is closing"));
    }
    command.stdin(if spawn.input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut shared = None;
    if spawn.merged {
        let (reader, writer) = std::io::pipe()?;
        command.stdout(writer.try_clone()?);
        command.stderr(writer);
        shared = Some(reader);
    } else {
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
    }
    prepare(&mut command);

    let started = Instant::now();
    let mut child = command.spawn()?;
    // The command holds our copies of the merged pipe's writing end, and its
    // reader sees no end while any copy is open.
    drop(command);
    let pid = child.id();
    let born = started_at(pid);
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    children().lock().unwrap().insert(id, (pid, born));

    if let (Some(input), Some(mut stdin)) = (spawn.input, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let out = match (shared, child.stdout.take()) {
        (Some(reader), _) => Some(drain(reader, spawn.cap)),
        (None, Some(stdout)) => Some(drain(stdout, spawn.cap)),
        (None, None) => None,
    };
    let err = child.stderr.take().map(|stderr| drain(stderr, spawn.cap));

    let deadline = started + spawn.limit;
    let (mut ended, code) = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break (Ended::Exited, status.code());
        }
        let cancelled = spawn.cancellable && CANCELLED.load(Ordering::SeqCst);
        if cancelled || Instant::now() >= deadline {
            kill_tree(pid, born, false);
            let _ = child.kill();
            let code = child.wait().ok().and_then(|status| status.code());
            break (
                if cancelled {
                    Ended::Cancelled
                } else {
                    Ended::TimedOut
                },
                code,
            );
        }
        std::thread::sleep(POLL);
    };
    // Whatever it started and left running — a server, a watcher — would
    // otherwise hold the worktree's files and outlive the delivery.
    kill_tree(pid, born, false);
    children().lock().unwrap().remove(&id);
    if ended == Ended::Exited && spawn.cancellable && CANCELLED.load(Ordering::SeqCst) {
        ended = Ended::Cancelled;
    }

    let read = |stream: Option<(Arc<Mutex<Capture>>, mpsc::Receiver<()>)>| match stream {
        Some((capture, done)) => {
            let _ = done.recv_timeout(DRAIN);
            let text = capture.lock().unwrap().text();
            text
        }
        None => String::new(),
    };
    Ok(Finished {
        ended,
        code,
        out: read(out),
        err: read(err),
        elapsed: started.elapsed(),
        pid,
    })
}

fn scrub(command: &mut Command) {
    for name in SCRUBBED {
        command.env_remove(name);
    }
}

/// The end of `text`, by lines and by bytes, for a message or a comment.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let kept = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    if kept.len() <= TAIL_BYTES {
        return kept;
    }
    let mut start = kept.len() - TAIL_BYTES;
    while !kept.is_char_boundary(start) {
        start += 1;
    }
    kept[start..].to_string()
}

/* ── Git ───────────────────────────────────────────────────────── */

fn git_command(dir: &Path, args: &[&str]) -> Command {
    // A bundled macOS app starts with launchd's bare PATH (path_env.rs).
    crate::path_env::ensure();
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        // Refusals are told apart by their wording below, which a translated
        // git would change (git.rs pins it for the same reason).
        .env("LC_ALL", "C")
        .env("LANGUAGE", "")
        // Nobody is there to type a password.
        .env("GIT_TERMINAL_PROMPT", "0");
    scrub(&mut command);
    command
}

fn git_run(dir: &Path, args: &[&str], cancellable: bool) -> Result<Finished, String> {
    let finished = run(
        git_command(dir, args),
        Spawn {
            input: None,
            limit: GIT_TIMEOUT,
            merged: false,
            cap: OUTPUT_BYTES,
            cancellable,
        },
    )
    .map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "git was not found on PATH".to_string(),
        _ => format!("could not run git: {err}"),
    })?;
    match finished.ended {
        Ended::Exited => Ok(finished),
        Ended::TimedOut => Err(format!(
            "`git {}` did not finish within {} minutes and was stopped",
            args.join(" "),
            GIT_TIMEOUT.as_secs() / 60
        )),
        Ended::Cancelled => Err("Hangar is closing".to_string()),
    }
}

/// `git -C dir args`'s stdout, or the line of stderr that says why not.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let finished = git_run(dir, args, true)?;
    if finished.succeeded() {
        Ok(finished.out)
    } else {
        Err(problem(&finished.err, &finished.out))
    }
}

/// The line of a refusal worth repeating: git's own `fatal:`/`error:` one,
/// failing that the last thing it said.
fn problem(stderr: &str, stdout: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .chain(stdout.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let line = lines
        .iter()
        .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
        .or_else(|| lines.last())
        .copied()
        .unwrap_or("git failed without saying why");
    line.trim_start_matches("fatal:")
        .trim_start_matches("error:")
        .trim()
        .to_string()
}

fn rev(dir: &Path, name: &str) -> Result<String, String> {
    git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .map(|out| out.trim().to_string())
    .map_err(|_| format!("{name} does not exist"))
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

fn branch_at(dir: &Path) -> Option<String> {
    git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|out| out.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Paths in the form they are compared in: Windows takes either separator and
/// ignores case, and git answers in forward slashes there (git.rs does the same).
fn key(path: &str) -> String {
    #[cfg(windows)]
    let key = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
    #[cfg(not(windows))]
    let key = path.trim_end_matches('/').to_string();
    key
}

/// The repository's common directory, which every worktree of it shares: the
/// key its deliveries queue on.
fn common_dir(dir: &Path) -> Result<PathBuf, String> {
    let out = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .map(|out| out.trim().to_string())
    // A git older than 2.31 echoes the option it does not know.
    .filter(|out| !out.is_empty() && !out.starts_with('-'));
    let path = match out {
        Some(path) => PathBuf::from(path),
        None => {
            let relative = git(dir, &["rev-parse", "--git-common-dir"])?;
            dir.join(relative.trim())
        }
    };
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

/// A path as git prints it, in the spelling the rest of the system uses: git
/// answers in forward slashes on Windows too.
fn native(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// Every worktree of the repository and the branch it has checked out.
fn worktrees(dir: &Path) -> Result<Vec<(String, Option<String>, bool)>, String> {
    let listing = git(dir, &["worktree", "list", "--porcelain"])?;
    let mut found = Vec::new();
    for block in listing.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        let mut bare = false;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(value.trim().to_string());
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch = Some(value.trim().trim_start_matches("refs/heads/").to_string());
            } else if line.trim() == "bare" {
                bare = true;
            }
        }
        if let Some(path) = path {
            found.push((path, branch, bare));
        }
    }
    Ok(found)
}

/// Uncommitted changes in a worktree, untracked files included: what a commit
/// would still have to take.
fn uncommitted(dir: &Path) -> Result<Vec<String>, String> {
    let status = git(
        dir,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    )?;
    let mut files = Vec::new();
    let mut fields = status.split('\0').filter(|field| !field.is_empty());
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        files.push(path.to_string());
        // A rename or a copy names where it came from next.
        if code.starts_with('R') || code.starts_with('C') {
            fields.next();
        }
    }
    Ok(files)
}

/// `files` for a sentence: the first few, and how many more.
fn listing(files: &[String]) -> String {
    const SHOWN: usize = 10;
    let mut text = files
        .iter()
        .take(SHOWN)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if files.len() > SHOWN {
        text.push_str(&format!(" (and {} more)", files.len() - SHOWN));
    }
    text
}

/// Where the delivery worktrees of the old versions went: one new folder per
/// delivery, in a folder of the system's own. Only the startup sweep reads it.
fn legacy_root() -> PathBuf {
    std::env::temp_dir().join("hangar-deliveries")
}

/// The folder every check of a workspace runs in: short and stable, because
/// Cargo keys the workspace's own crates by path and a new path recompiles
/// them all. `%LOCALAPPDATA%\hangar\c\<8 hex>` (elsewhere the user's local
/// data folder, then the system's temp one).
fn check_folder(request: &Delivery) -> PathBuf {
    let root = match request
        .check_root
        .as_deref()
        .filter(|root| !root.is_empty())
    {
        Some(root) => PathBuf::from(root),
        None => dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("hangar")
            .join("c"),
    };
    let _ = std::fs::create_dir_all(&root);
    let root = dunce::canonicalize(&root).unwrap_or(root);
    root.join(crate::build_cache::key_for(&request.workspace_id))
}

/// The workspace's worktree for checks: made once, put on each merge in turn.
/// Nothing is removed afterwards; the next check starts by resetting it.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// The worktree at `path`, on `base`, with none of the last check's
    /// files that git tracks or could add. Ignored files stay (dependencies,
    /// and what the check's own tools keep); the build output is outside it.
    /// Made again when missing or broken.
    fn acquire(repo: &Path, base: &str, path: PathBuf) -> Result<Scratch, String> {
        if Scratch::reset(repo, base, &path).is_err() {
            remove_scratch(repo, &path);
            let _ = git_run(repo, &["worktree", "prune"], false);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let spelled = path.to_string_lossy().into_owned();
            git(
                repo,
                &["worktree", "add", "--detach", "--force", &spelled, base],
            )
            .map_err(|err| format!("could not make a worktree to test the delivery in: {err}"))?;
        }
        Ok(Scratch { path })
    }

    fn reset(repo: &Path, base: &str, path: &Path) -> Result<(), String> {
        let listed = worktrees(repo)?;
        let wanted = key(&path.to_string_lossy());
        if !path.is_dir() || !listed.iter().any(|(listed, _, _)| key(listed) == wanted) {
            return Err("not a worktree of this repository".into());
        }
        // A merge a check left half done, or files it changed.
        let _ = git_run(path, &["merge", "--abort"], false);
        git(path, &["reset", "--hard", "-q"])?;
        git(path, &["checkout", "--detach", "--force", "-q", base])?;
        git(path, &["clean", "-ffd", "-q"])?;
        Ok(())
    }
}

impl Drop for Scratch {
    /// The folder stays; what a delivery left in it does not: a conflicted
    /// merge, changed files, new ones. Whatever path leaves, this runs.
    fn drop(&mut self) {
        let _ = git_run(&self.path, &["merge", "--abort"], false);
        let _ = git_run(&self.path, &["reset", "--hard", "-q"], false);
        let _ = git_run(&self.path, &["clean", "-ffd", "-q"], false);
    }
}

fn remove_scratch(repo: &Path, path: &Path) {
    let spelled = path.to_string_lossy().into_owned();
    let args = ["worktree", "remove", "--force", "--force", spelled.as_str()];
    let removed = git_run(repo, &args, false).is_ok_and(|finished| finished.succeeded());
    if !removed || path.exists() {
        // A file something still held, or a worktree git never finished
        // making: the folder goes by hand, then git forgets it.
        let _ = std::fs::remove_dir_all(path);
        let _ = git_run(repo, &args, false);
    }
}

/// Whether a delivery worktree of an old version is no one's any more: made
/// by a process that is gone, or by this one. Another Hangar on the same
/// machine may be testing in its own right now.
fn legacy_stale(name: &str, table: &Option<Vec<crate::procs::Proc>>) -> bool {
    let Some(rest) = name.strip_prefix(SCRATCH_PREFIX) else {
        return false;
    };
    let owner: Option<u32> = rest.split('-').next().and_then(|pid| pid.parse().ok());
    match (owner, table) {
        (Some(pid), _) if pid == std::process::id() => true,
        (Some(pid), Some(table)) => !table.iter().any(|proc| proc.pid == pid),
        _ => false,
    }
}

/// At the app's start: the worktrees old versions left in `root` (the
/// system's temp folder) go, then git forgets the ones whose folder is gone,
/// in each of `repos` (any folder of a repository).
fn sweep_legacy(root: &Path, repos: &[String]) {
    let table = crate::procs::table();
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let prefix = format!("{}/", key(&canonical.to_string_lossy()));
    let mut seen = std::collections::HashSet::new();
    for repo in repos {
        let repo = Path::new(repo);
        let Ok(common) = common_dir(repo) else {
            continue;
        };
        if !seen.insert(key(&common.to_string_lossy())) {
            continue;
        }
        for (path, _, _) in worktrees(repo).unwrap_or_default() {
            let name = Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            if key(&path).starts_with(&prefix) && legacy_stale(&name, &table) {
                remove_scratch(repo, Path::new(&path));
            }
        }
    }
    // Folders no repository lists any more.
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if legacy_stale(&name, &table) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let _ = std::fs::remove_dir(root);
    for repo in repos {
        let _ = git_run(Path::new(repo), &["worktree", "prune"], false);
    }
}

/// The app's start: what old versions left in the system's temp folder goes.
pub fn sweep_leftovers(repos: &[String]) {
    sweep_legacy(&legacy_root(), repos);
}

fn tested_ref(task: &str, part: &str) -> String {
    let name: String = task
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{TESTED_REFS}/{name}/{part}")
}

/// `tip` is the head of the worker's branch that was merged.
fn save_tested(repo: &Path, task: &str, base: &str, merge: &str, tip: &str) {
    let _ = git(repo, &["update-ref", &tested_ref(task, "base"), base]);
    let _ = git(repo, &["update-ref", &tested_ref(task, "merge"), merge]);
    let _ = git(repo, &["update-ref", &tested_ref(task, "tip"), tip]);
}

/// A delivery that passed its check and its review and waits for the person.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tested {
    /// The head of the branch it lands on, when it was tested.
    pub base: String,
    /// The merge it was checked and reviewed as.
    pub merge: String,
    /// The worker's branch is still where it was tested. When it is not, what
    /// waits is not the worker's latest work and never lands as it is.
    pub current: bool,
}

/// The delivery of `task` kept tested, measured against `tip`, the head of
/// the worker's branch now.
fn tested(repo: &Path, task: &str, tip: &str) -> Option<Tested> {
    let base = rev(repo, &tested_ref(task, "base")).ok()?;
    let merge = rev(repo, &tested_ref(task, "merge")).ok()?;
    let current = match rev(repo, &tested_ref(task, "tip")) {
        Ok(tested_tip) => tested_tip == tip,
        // Kept before the branch's head was: the merge holds the head it was
        // tested on, and none of what the worker committed after.
        Err(_) => git(repo, &["merge-base", "--is-ancestor", tip, &merge]).is_ok(),
    };
    Some(Tested {
        base,
        merge,
        current,
    })
}

fn forget_tested(repo: &Path, task: &str) {
    for part in ["base", "merge", "tip"] {
        let _ = git_run(repo, &["update-ref", "-d", &tested_ref(task, part)], false);
    }
}

/* ── Checks that cannot tell anything ──────────────────────────── */

/// Files that cannot break a build or a test, by the patterns the check
/// skipping reads: a name's ending, a name's beginning, a folder.
const DOC_SUFFIXES: &[&str] = &[".md", ".mdx", ".txt", ".rst"];
const DOC_NAME_PREFIXES: &[&str] = &["LICENSE", "CHANGELOG"];
const DOC_FOLDERS: &[&str] = &["docs/", ".github/ISSUE_TEMPLATE/"];
/// Text files that build tools read all the same: not documentation.
fn build_input(name: &str) -> bool {
    name == "CMakeLists.txt"
        || ((name.starts_with("requirements") || name.starts_with("constraints"))
            && name.ends_with(".txt"))
}

fn documentation(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let name = path.rsplit('/').next().unwrap_or(&path);
    if build_input(name) {
        return false;
    }
    DOC_FOLDERS.iter().any(|folder| path.starts_with(folder))
        || DOC_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
        || DOC_NAME_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// Every changed path is documentation: a check could not fail on it.
fn documentation_only(changed: &[String]) -> bool {
    !changed.is_empty() && changed.iter().all(|path| documentation(path))
}

const SKIPPED_DOCS: &str = "documentation only";
const SKIPPED_PASSED: &str = "already passed on this exact tree";
/// Trees remembered per repository.
const PASSED_TREES: usize = 200;

fn passed_file(request: &Delivery) -> Option<PathBuf> {
    let dir = match request.data_dir.as_deref().filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => crate::paths::Paths::from_env().app_data_dir().ok()?,
    };
    Some(dir.join("delivery-passed.json"))
}

fn passed_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}

/// What a passed check vouches for: the tree it ran on, with the line that
/// ran, since another check would say something else about the same files.
fn passed_entry(tree: &str, line: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(line.as_bytes());
    let line: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{tree}:{line}")
}

fn read_passed(file: &Path) -> HashMap<String, Vec<String>> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn passed_before(request: &Delivery, repo: &str, tree: &str, line: &str) -> bool {
    let Some(file) = passed_file(request) else {
        return false;
    };
    let _held = passed_lock().lock().unwrap_or_else(|p| p.into_inner());
    read_passed(&file)
        .get(repo)
        .is_some_and(|trees| trees.contains(&passed_entry(tree, line)))
}

fn remember_passed(request: &Delivery, repo: &str, tree: &str, line: &str) {
    let Some(file) = passed_file(request) else {
        return;
    };
    let _held = passed_lock().lock().unwrap_or_else(|p| p.into_inner());
    let mut all = read_passed(&file);
    let trees = all.entry(repo.to_string()).or_default();
    let entry = passed_entry(tree, line);
    trees.retain(|kept| *kept != entry);
    trees.push(entry);
    if trees.len() > PASSED_TREES {
        let extra = trees.len() - PASSED_TREES;
        trees.drain(..extra);
    }
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&all) {
        let _ = std::fs::write(&file, text);
    }
}

/* ── What a task owns ──────────────────────────────────────────── */

fn match_segment(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            match_segment(&pattern[1..], text)
                || (!text.is_empty() && match_segment(pattern, &text[1..]))
        }
        (Some('?'), Some(_)) => match_segment(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => match_segment(&pattern[1..], &text[1..]),
        _ => false,
    }
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.first() {
        None => path.is_empty(),
        Some(&"**") => (0..=path.len()).any(|skip| match_segments(&pattern[1..], &path[skip..])),
        Some(segment) => {
            !path.is_empty() && {
                let segment: Vec<char> = segment.chars().collect();
                let name: Vec<char> = path[0].chars().collect();
                match_segment(&segment, &name) && match_segments(&pattern[1..], &path[1..])
            }
        }
    }
}

/// Whether `owns` — paths and globs relative to the workspace folder, which
/// sits at `prefix` in the repository — covers `path`, relative to the
/// repository. A path owns everything under it, and so does a glob matching
/// a folder: `src/lib` and `src/*` both own `src/lib/a.ts`. `*` and `?` stay
/// within a name, `**` crosses folders.
fn owned(path: &str, owns: &[String], prefix: &str) -> bool {
    let fold = |text: &str| {
        let text = text.replace('\\', "/");
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    let path = fold(path);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    owns.iter().any(|pattern| {
        let pattern = fold(pattern.trim());
        let pattern = pattern.trim_start_matches("./").trim_matches('/');
        let full = match pattern {
            "" | "." => fold(prefix).trim_end_matches('/').to_string(),
            _ => format!("{}{pattern}", fold(prefix)),
        };
        let wanted: Vec<&str> = full.split('/').filter(|s| !s.is_empty()).collect();
        // The path itself, or a folder it is in.
        wanted.is_empty()
            || (1..=segments.len()).any(|len| match_segments(&wanted, &segments[..len]))
    })
}

/* ── The check ─────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq)]
enum Family {
    PowerShell,
    Cmd,
    /// sh, bash, zsh and kin. `msys`: a Windows bash (Git Bash, MSYS2,
    /// Cygwin), whose login profile moves to the home folder unless told not to.
    Posix {
        msys: bool,
    },
    Fish,
    Nu,
    Wsl,
    Other,
}

fn family(shell: &ShellInfo) -> Family {
    let stem = Path::new(&shell.program)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match (shell.id.as_str(), stem.as_str()) {
        ("pwsh" | "powershell", _) | (_, "pwsh" | "powershell") => Family::PowerShell,
        ("cmd", _) | (_, "cmd") => Family::Cmd,
        ("git-bash" | "msys2" | "cygwin", _) => Family::Posix { msys: true },
        ("wsl", _) | (_, "wsl") => Family::Wsl,
        ("fish", _) | (_, "fish") => Family::Fish,
        ("nu", _) | (_, "nu") => Family::Nu,
        (_, "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "ash") => Family::Posix {
            msys: cfg!(windows),
        },
        _ => Family::Other,
    }
}

/// How a shell runs one command line and exits with its status.
#[derive(Debug, PartialEq)]
struct Invocation {
    program: String,
    args: Vec<String>,
    /// The command line as the program must read it, past Rust's quoting:
    /// cmd.exe parses its own command line, by rules of its own.
    raw: Option<String>,
    env: Vec<(&'static str, &'static str)>,
}

fn invocation(shell: &ShellInfo, line: &str) -> Invocation {
    let posix = |flags: &[&str]| {
        let mut args: Vec<String> = flags.iter().map(|flag| flag.to_string()).collect();
        args.push(line.to_string());
        args
    };
    let plain = |args: Vec<String>| Invocation {
        program: shell.program.clone(),
        args,
        raw: None,
        env: vec![],
    };
    // cmd.exe strips the outer quotes of what follows /s /c and runs the rest
    // as typed; /d leaves out the AutoRun commands an interactive one gets.
    let cmd = |program: String| Invocation {
        program,
        args: vec![],
        raw: Some(format!("/d /s /c \"{line}\"")),
        env: vec![],
    };
    match family(shell) {
        Family::PowerShell => {
            // Encoded, so no quoting stands between the line and PowerShell;
            // with the profile, as in a pane. A native command's status is
            // passed on, which `-Command` would round to 0 or 1.
            let script = format!(
                "{line}\nif (-not $?) {{ if ($LASTEXITCODE) {{ exit $LASTEXITCODE }} else {{ exit 1 }} }}"
            );
            let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
            plain(vec![
                "-NoLogo".into(),
                "-NonInteractive".into(),
                "-EncodedCommand".into(),
                encoded,
            ])
        }
        Family::Cmd => cmd(shell.program.clone()),
        Family::Posix { msys } => Invocation {
            // A login shell: the PATH the person's terminal has.
            env: if msys {
                // Stay in the folder it was started in, and keep the Windows PATH.
                vec![("CHERE_INVOKING", "1"), ("MSYS2_PATH_TYPE", "inherit")]
            } else {
                vec![]
            },
            ..plain(posix(&["-l", "-c"]))
        },
        Family::Fish | Family::Nu => plain(posix(&["-l", "-c"])),
        Family::Wsl => plain(posix(&["-e", "sh", "-lc"])),
        // A shell whose syntax nothing here knows.
        Family::Other if cfg!(windows) => cmd(cmd_program()),
        Family::Other => plain(posix(&["-c"])),
    }
}

fn cmd_program() -> String {
    std::env::var("ComSpec")
        .ok()
        .filter(|path| Path::new(path).is_file())
        .unwrap_or_else(|| "cmd.exe".to_string())
}

/// The workspace's shell when it still exists; otherwise cmd on Windows, and
/// the login shell elsewhere.
fn check_shell(shell: Option<&ShellInfo>) -> ShellInfo {
    if let Some(shell) = shell.filter(|shell| crate::shells::program_exists(&shell.program)) {
        return shell.clone();
    }
    if cfg!(windows) {
        return ShellInfo {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            program: cmd_program(),
            args: vec![],
        };
    }
    crate::shells::default_shell().unwrap_or(ShellInfo {
        id: "sh".into(),
        label: "sh".into(),
        program: "/bin/sh".into(),
        args: vec![],
    })
}

fn check_command(
    invocation: &Invocation,
    dir: &Path,
    port_base: Option<u32>,
    build_env: &[(String, String)],
) -> Command {
    crate::path_env::ensure();
    let mut command = Command::new(&invocation.program);
    command.args(&invocation.args).current_dir(dir);
    #[cfg(windows)]
    if let Some(raw) = &invocation.raw {
        use std::os::windows::process::CommandExt;
        command.raw_arg(raw);
    }
    #[cfg(not(windows))]
    if let Some(raw) = &invocation.raw {
        command.arg(raw);
    }
    scrub(&mut command);
    // The shared build cache, set after the scrub so that it survives it.
    for (name, value) in build_env {
        command.env(name, value);
    }
    for (name, value) in &invocation.env {
        command.env(name, value);
    }
    // The worker's ports: a check that starts a server takes them, not
    // another worker's.
    if let Some(base) = port_base {
        command
            .env("HANGAR_PORT_BASE", base.to_string())
            .env("HANGAR_PORT_COUNT", "20");
    }
    command
}

/// Copies the files a check is likely to need and a fresh checkout lacks,
/// from the main checkout: the ones a worktree made in the worktree panel
/// gets (WorktreePanel.tsx) — `.env`, `.env.*` and `*.local` at the root, and
/// `.vscode`. Copies what is missing and replaces nothing: what the merge
/// itself holds stays as it was tested.
fn copy_local_files(from: &Path, to: &Path) {
    fn missing(from: &Path, to: &Path) {
        if to.exists() {
            return;
        }
        if from.is_dir() {
            let _ = std::fs::create_dir_all(to);
            for entry in std::fs::read_dir(from).into_iter().flatten().flatten() {
                missing(&entry.path(), &to.join(entry.file_name()));
            }
        } else if from.is_file() {
            let _ = std::fs::copy(from, to);
        }
    }
    for entry in std::fs::read_dir(from).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let local = kind.is_file()
            && (name == ".env" || name.starts_with(".env.") || name.ends_with(".local"));
        if local || (kind.is_dir() && name == ".vscode") {
            missing(&entry.path(), &to.join(&name));
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub seconds: u64,
    /// The end of what it printed.
    pub tail: String,
}

fn run_check(request: &Delivery, dir: &Path, line: &str, limit: Duration) -> Result<Check, String> {
    let shell = check_shell(request.shell.as_ref());
    let invocation = invocation(&shell, line);
    let finished = run(
        check_command(&invocation, dir, request.port_base, &request.build_env),
        Spawn {
            input: None,
            limit,
            merged: true,
            cap: CHECK_BYTES,
            cancellable: true,
        },
    )
    .map_err(|err| format!("could not start {} to run the check: {err}", shell.program))?;
    if finished.ended == Ended::Cancelled {
        return Err("Hangar closed while the check was running".to_string());
    }
    Ok(Check {
        command: line.to_string(),
        passed: finished.succeeded(),
        exit_code: finished.code,
        timed_out: finished.ended == Ended::TimedOut,
        seconds: finished.elapsed.as_secs(),
        tail: tail(&finished.out),
    })
}

/* ── The review ────────────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Blocking {
    pub file: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub problem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub approve: bool,
    #[serde(default)]
    pub blocking: Vec<Blocking>,
    #[serde(default)]
    pub notes: String,
}

impl Verdict {
    fn passes(&self) -> bool {
        self.approve && self.blocking.is_empty()
    }
}

/// Reads a delivery and decides. The real one is a Claude Code run; the tests
/// hand in their own.
///
/// `account` is the Claude account of the workspace the delivery is for
/// (`None`: the default one): the review spends that account, as its panes do.
pub trait Reviewer {
    fn review(
        &self,
        dir: &Path,
        model: &str,
        prompt: &str,
        account: Option<&str>,
    ) -> Result<Verdict, String>;
}

/// `claude -p` with a fresh context, in the temporary worktree, able to read
/// and nothing else: no shell, no edits, no MCP server, no question asked.
pub struct Claude {
    pub limit: Duration,
}

const NO_CLAUDE: &str =
    "Claude Code (`claude`) was not found on PATH, so nobody could review the delivery";

fn claude_program() -> Option<PathBuf> {
    crate::path_env::ensure();
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd", "claude.bat"]
    } else {
        &["claude"]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    // Where its own installer puts it, for a PATH that was never told.
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
    }
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

fn review_args(model: &str) -> Vec<String> {
    [
        "-p",
        "--model",
        model,
        "--output-format",
        "json",
        "--json-schema",
        VERDICT_SCHEMA,
        "--tools",
        REVIEW_TOOLS,
        "--allowedTools",
        REVIEW_TOOLS,
        // No MCP server, the user's or the project's.
        "--strict-mcp-config",
        // Whatever is not allowed above is refused without asking anyone.
        "--permission-mode",
        "dontAsk",
        // The user's settings — their login, their provider — but not the
        // ones the branch under review carries: its hooks are not to run.
        "--setting-sources",
        "user",
        // A review is not a conversation anyone will resume.
        "--no-session-persistence",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect()
}

/// The reviewer's command: `claude -p` in `dir`, with Hangar's environment
/// minus `SCRUBBED`, and `account_env` — what accounts.rs gives a pane on the
/// workspace's account — on top. Empty on the default account, which keeps
/// the environment as it is.
fn review_command(
    program: &Path,
    dir: &Path,
    model: &str,
    account_env: &[(&'static str, Option<PathBuf>)],
) -> Command {
    let mut command = Command::new(program);
    command.args(review_args(model)).current_dir(dir);
    scrub(&mut command);
    for (name, value) in account_env {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

/// A usage limit of the account, as Claude Code words it (« You've hit your
/// weekly limit · resets Nov 3, 1pm »).
fn usage_limit(said: &str) -> Option<String> {
    let lower = said.to_lowercase();
    if !(lower.contains("hit your") && lower.contains("limit"))
        && !lower.contains("usage limit")
        && !lower.contains("limit reached")
    {
        return None;
    }
    said.lines()
        .find(|line| line.to_lowercase().contains("limit"))
        .map(|line| {
            let line = line.trim().trim_matches(['"', ',']);
            line.rsplit("\"result\":\"")
                .next()
                .unwrap_or(line)
                .trim_end_matches(['"', '}', ','])
                .to_string()
        })
}

/// The name the person gave the account, for a message.
fn account_label(id: &str) -> String {
    crate::accounts::accounts_list()
        .into_iter()
        .find(|account| account.id == id)
        .map(|account| account.label)
        .unwrap_or_else(|| id.to_string())
}

/// What the person reads when the account behind the review is out of quota:
/// the work is fine, the review is waiting for the account.
fn limit_message(account: &str, said: &str) -> String {
    format!(
        "the Claude account \"{account}\" has reached its usage limit, so the review could not run \
(Claude Code said: {said}). Nothing is wrong with the delivery: submit it again once the limit resets"
    )
}

impl Reviewer for Claude {
    fn review(
        &self,
        dir: &Path,
        model: &str,
        prompt: &str,
        account: Option<&str>,
    ) -> Result<Verdict, String> {
        let program = claude_program().ok_or(NO_CLAUDE)?;
        let launch = crate::accounts::for_launch(Some("claude"), account);
        let env = launch
            .as_ref()
            .map(|launch| crate::accounts::env_for(Some("claude"), launch.dir.as_deref()))
            .unwrap_or_default();
        let account_name = launch
            .as_ref()
            .map(|launch| account_label(&launch.id))
            .unwrap_or_default();
        let command = review_command(&program, dir, model, &env);
        let finished = run(
            command,
            Spawn {
                input: Some(prompt.as_bytes().to_vec()),
                limit: self.limit,
                merged: false,
                cap: OUTPUT_BYTES,
                cancellable: true,
            },
        )
        .map_err(|err| format!("could not start {}: {err}", program.display()))?;
        match finished.ended {
            Ended::Cancelled => Err("Hangar closed while the review was running".to_string()),
            Ended::TimedOut => Err(format!(
                "the review did not finish within {} minutes and was stopped",
                self.limit.as_secs() / 60
            )),
            Ended::Exited => verdict_of(&finished.out).map_err(|err| {
                if let Some(limit) =
                    usage_limit(&finished.out).or_else(|| usage_limit(&finished.err))
                {
                    return limit_message(&account_name, &limit);
                }
                let said = tail(finished.err.trim());
                if said.is_empty() {
                    err
                } else {
                    format!("{err} (Claude Code said: {said})")
                }
            }),
        }
    }
}

/// The verdict in `claude -p --output-format json`'s answer: its structured
/// output, or, from a Claude Code that does not fill that in, its result read
/// as JSON.
fn verdict_of(output: &str) -> Result<Verdict, String> {
    let answer: Value = serde_json::from_str(output.trim())
        .ok()
        .or_else(|| {
            output
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        })
        .ok_or("the reviewer's answer could not be read")?;
    let said = answer
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if answer.get("is_error").and_then(Value::as_bool) == Some(true) {
        return Err(if said.is_empty() {
            "the review failed without saying why".to_string()
        } else {
            format!("the review failed: {said}")
        });
    }
    if let Some(structured) = answer.get("structured_output").filter(|v| v.is_object()) {
        return serde_json::from_value(structured.clone())
            .map_err(|err| format!("the reviewer's verdict is malformed: {err}"));
    }
    let fenced = said
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(fenced).map_err(|_| "the reviewer answered without a verdict".to_string())
}

fn review_prompt(
    request: &Delivery,
    base: &str,
    merge: &str,
    changed: &[String],
    outside: &[String],
    diff: &str,
) -> String {
    let task = &request.task;
    let mut prompt = format!(
        "You are reviewing a delivery before it lands on the branch `{target}`. A worker agent did the task \
below on its own branch, `{branch}`. Hangar merged that branch onto the current `{target}` in this folder, \
which holds the merged result. You did not write this code, and nothing else checks it before it lands.\n\n",
        target = request.target,
        branch = request.branch,
    );
    prompt.push_str("## The task\n\n");
    prompt.push_str(&format!("Title: {}\n\n", task.title.trim()));
    let description = task.description.trim();
    prompt.push_str(&format!(
        "Description, with its acceptance criteria:\n{}\n\n",
        if description.is_empty() {
            "(none given)"
        } else {
            description
        }
    ));
    if task.owns.is_empty() {
        prompt.push_str("Files the task owns: not stated.\n\n");
    } else {
        prompt.push_str(&format!(
            "Files the task owns (the worker was told to change only these): {}\n\n",
            task.owns.join(", ")
        ));
    }
    if let Some(summary) = request
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        prompt.push_str(&format!(
            "The worker's summary of what it did:\n{summary}\n\n"
        ));
    }
    if let Some(check) = request.check_command.as_deref() {
        prompt.push_str(&format!(
            "The check `{check}` already passed on this merged result.\n\n"
        ));
    }

    prompt.push_str("## What to decide\n\n");
    prompt.push_str(
        "Approve unless the change is wrong or misses a requirement the task states. Block only for:\n\
- correctness: a bug, broken behaviour, a crash, lost data, a security hole, code that cannot work;\n\
- a requirement stated in the task that the change does not meet.\n\
Do not block for style, naming, formatting, a structure you would have chosen differently, missing \
extras, or anything the task did not ask for: mention those briefly in notes, if at all. Each blocking \
item names the file, the line when you can, and what is wrong, concretely enough for the worker to fix it \
without guessing.\n\n",
    );
    if !outside.is_empty() {
        prompt.push_str(&format!(
            "Changed outside the files the task owns: {}. That is a signal to weigh, not a verdict: judge \
whether each change is needed for the task (a test, a shared type, a line registering the new code) or \
unrelated work that should not land with it, and block only in the second case.\n\n",
            outside.join(", ")
        ));
    }
    prompt.push_str(
        "You can read any file here with Read, and search with Grep and Glob when you have them, to check the \
change in its context. You cannot run anything or change anything, and you need not: judge from the code.\n\n\
Write the blocking items and the notes in English: the worker that fixes them reads them.\n\n",
    );

    prompt.push_str(&format!(
        "## The change: `git diff {}...{}`\n\nFiles changed ({}): {}\n\n",
        short(base),
        short(merge),
        changed.len(),
        changed.join(", ")
    ));
    if diff.len() > DIFF_BYTES {
        let mut end = DIFF_BYTES;
        while !diff.is_char_boundary(end) {
            end -= 1;
        }
        prompt.push_str(&format!(
            "```diff\n{}\n```\n\nThe diff is longer than {} KB and was cut there: read the changed files it does \
not reach yourself before deciding.\n",
            &diff[..end],
            DIFF_BYTES / 1024
        ));
    } else {
        prompt.push_str(&format!("```diff\n{}\n```\n", diff.trim_end()));
    }
    prompt
}

/* ── A delivery ────────────────────────────────────────────────── */

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskBrief {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub owns: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    /// The workspace's folder, which the task's `owns` is relative to.
    pub workspace: String,
    /// The worker's worktree, on `branch`.
    pub worktree: String,
    pub branch: String,
    /// The branch it lands on.
    pub target: String,
    pub task: TaskBrief,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub check_command: Option<String>,
    /// The shell the check runs through: the worker pane's.
    #[serde(default)]
    pub shell: Option<ShellInfo>,
    pub reviewer_model: String,
    /// The workspace's Claude account, which the review spends; none: the
    /// default one.
    #[serde(default)]
    pub claude_account: Option<String>,
    /// Lands once it passes; otherwise it waits, tested, for the person.
    pub land: bool,
    /// Lands the merge tested earlier as it is, when the branch it lands on
    /// has not moved since; otherwise tests it again first.
    #[serde(default)]
    pub reuse: bool,
    #[serde(default)]
    pub port_base: Option<u32>,
    /// The workspace's id, which names its check folder.
    #[serde(default)]
    pub workspace_id: String,
    /// What the check's build tools are told, after the environment is
    /// scrubbed: the shared build cache's variables (`build_cache::env_for`).
    #[serde(default)]
    pub build_env: Vec<(String, String)>,
    /// Where the check folders live; none: the user's local data folder.
    #[serde(default)]
    pub check_root: Option<String>,
    /// Where the trees whose check passed are kept; none: the app's data folder.
    #[serde(default)]
    pub data_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Landed,
    Ready,
    Conflict,
    CheckFailed,
    ChangesRequested,
    Dirty,
    /// Asked to land what waited for the person, after the worker's branch
    /// moved on: nothing landed, and what waited is dropped.
    Stale,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub outcome: Kind,
    /// What happened and what to do about it, for the model that reads it.
    pub detail: String,
    /// The files in conflict, or with uncommitted changes in the way.
    pub files: Vec<String>,
    /// For `dirty`: the person's checkout, rather than the worker's worktree.
    pub in_checkout: bool,
    pub changed: Vec<String>,
    pub outside_owns: Vec<String>,
    pub check: Option<Check>,
    /// Why the check did not run although the workspace has one.
    pub check_skipped: Option<String>,
    pub review: Option<Verdict>,
    /// The merge it was tested as, and the head of the branch it was merged onto.
    pub merge: Option<String>,
    pub base: Option<String>,
    /// The checkout the branch was fast-forwarded in; none when it is checked
    /// out nowhere and only its ref moved.
    pub landed_in: Option<String>,
}

impl Outcome {
    fn new(outcome: Kind, detail: String) -> Outcome {
        Outcome {
            outcome,
            detail,
            files: vec![],
            in_checkout: false,
            changed: vec![],
            outside_owns: vec![],
            check: None,
            check_skipped: None,
            review: None,
            merge: None,
            base: None,
            landed_in: None,
        }
    }

    fn error(detail: impl Into<String>) -> Outcome {
        let detail = detail.into();
        let mut detail = detail.trim_end_matches('.').to_string();
        detail.push_str(". Nothing was merged.");
        if let Some(first) = detail.get(..1) {
            detail = format!("{}{}", first.to_uppercase(), &detail[1..]);
        }
        Outcome::new(Kind::Error, detail)
    }
}

pub struct Limits {
    pub check: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            check: CHECK_TIMEOUT,
        }
    }
}

/// A delivery in progress, for `delivery_status`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub task_id: String,
    /// The repository it waits its turn in, as `Plan::queue` names it.
    pub queue: String,
    /// queued, merging, checking, reviewing or landing.
    pub stage: String,
    pub since: u64,
}

fn stages() -> &'static Mutex<HashMap<String, Running>> {
    static STAGES: OnceLock<Mutex<HashMap<String, Running>>> = OnceLock::new();
    STAGES.get_or_init(Default::default)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A delivery's entry in `stages`, gone with it.
struct Stage(String);

impl Stage {
    fn enter(task: &str, queue: &str) -> Stage {
        stages().lock().unwrap().insert(
            task.to_string(),
            Running {
                task_id: task.to_string(),
                queue: queue.to_string(),
                stage: "queued".into(),
                since: now_ms(),
            },
        );
        Stage(task.to_string())
    }

    fn set(&self, stage: &str) {
        if let Some(entry) = stages().lock().unwrap().get_mut(&self.0) {
            entry.stage = stage.to_string();
            entry.since = now_ms();
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        stages().lock().unwrap().remove(&self.0);
    }
}

/// One turn at a time per repository: deliveries landing on the same history
/// test on each other's results, not side by side on the same old head.
fn turn_of(repo: &str) -> Arc<Mutex<()>> {
    static TURNS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    TURNS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(repo.to_string())
        .or_default()
        .clone()
}

enum Step {
    Done(Box<Outcome>),
    /// The branch moved between the test and the landing.
    Moved,
}

/// Merges, checks, reviews and lands one delivery, or says why not. Waits for
/// any other delivery of the same repository to finish first.
pub fn deliver(request: &Delivery, reviewer: &dyn Reviewer, limits: &Limits) -> Outcome {
    let worktree = PathBuf::from(&request.worktree);
    let repo = match common_dir(&worktree) {
        Ok(dir) => key(&dir.to_string_lossy()),
        Err(err) => {
            return Outcome::error(format!("{} is not a git worktree: {err}", request.worktree))
        }
    };
    let stage = Stage::enter(&request.task.id, &repo);
    let turn = turn_of(&repo);
    let _turn = turn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !request.reuse {
        // Submitted again: this one replaces what of the task waited for the
        // person, whatever it turns out to be.
        forget_tested(&worktree, &request.task.id);
    }

    let mut reuse = request.reuse;
    for _ in 0..LANDING_TRIES {
        match attempt(request, &worktree, reviewer, limits, reuse, &stage) {
            Step::Done(outcome) => return *outcome,
            Step::Moved => reuse = false,
        }
    }
    Outcome::error(format!(
        "{} kept moving while the delivery was tested, {LANDING_TRIES} times in a row. Submit it again once it settles",
        request.target
    ))
}

fn attempt(
    request: &Delivery,
    worktree: &Path,
    reviewer: &dyn Reviewer,
    limits: &Limits,
    reuse: bool,
    stage: &Stage,
) -> Step {
    let done = |outcome: Outcome| Step::Done(Box::new(outcome));
    let (branch, target, task) = (&request.branch, &request.target, &request.task.id);
    stage.set("merging");

    // 1. Only what is committed is delivered.
    if !reuse {
        match uncommitted(worktree) {
            Ok(files) if !files.is_empty() => {
                let mut outcome = Outcome::new(
                    Kind::Dirty,
                    format!(
                        "Your worktree has uncommitted changes: {}. Only commits are delivered: commit them on \
{branch} (or delete what is not part of the task), then call delivery_submit again.",
                        listing(&files)
                    ),
                );
                outcome.files = files;
                return done(outcome);
            }
            Ok(_) => {}
            Err(err) => {
                return done(Outcome::error(format!(
                    "could not read {}: {err}",
                    request.worktree
                )))
            }
        }
    }
    let base = match rev(worktree, &format!("refs/heads/{target}")) {
        Ok(sha) => sha,
        Err(err) => return done(Outcome::error(err)),
    };
    let tip = match rev(worktree, &format!("refs/heads/{branch}")) {
        Ok(sha) => sha,
        Err(err) => return done(Outcome::error(err)),
    };
    // Landing what waited: as it was tested when neither branch moved since,
    // tested again on the target's new head when only the target did.
    if request.reuse {
        if let Some(kept) = tested(worktree, task, &tip) {
            if !kept.current {
                // The worker committed after it was tested: landing it would
                // land its older work, and testing the branch again would land
                // work it never submitted. Its next delivery_submit does that.
                forget_tested(worktree, task);
                let mut outcome = Outcome::new(
                    Kind::Stale,
                    format!(
                        "{branch} has commits that were not part of the delivery waiting to land on {target}, \
which was checked and reviewed as {}: it was not landed, so that what lands is your latest work. Once that work \
is finished and committed, call delivery_submit again: the new delivery is merged, checked and reviewed before it \
can land.",
                        short(&kept.merge)
                    ),
                );
                outcome.merge = Some(kept.merge);
                outcome.base = Some(base);
                return done(outcome);
            }
            if reuse && kept.base == base {
                stage.set("landing");
                return land_step(
                    request,
                    worktree,
                    &base,
                    &kept.merge,
                    &tip,
                    Outcome::new(Kind::Landed, String::new()),
                );
            }
        }
    }

    // 2. Merged onto the branch as it is now, away from everyone's checkout.
    // One folder per workspace, one check at a time in it.
    let folder = check_folder(request);
    let folder_turn = turn_of(&format!("check:{}", key(&folder.to_string_lossy())));
    let _folder_turn = folder_turn
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let scratch = match Scratch::acquire(worktree, &base, folder) {
        Ok(scratch) => scratch,
        Err(err) => return done(Outcome::error(err)),
    };
    let dir = scratch.path.clone();
    let mut merge_args = vec!["-c", "merge.autoStash=false"];
    // A repository with no identity configured still gets a merge commit.
    let anonymous = git(&dir, &["var", "GIT_COMMITTER_IDENT"]).is_err();
    if anonymous {
        merge_args.extend([
            "-c",
            "user.name=Hangar.AI",
            "-c",
            "user.email=hangar@localhost",
        ]);
    }
    // Hooks are the person's checkout's business: this merge is a test bench,
    // and the check is what verifies it.
    let message = format!("Merge branch '{branch}'");
    let title = request.task.title.trim();
    merge_args.extend(["merge", "--no-edit", "--no-verify", "-m", message.as_str()]);
    if !title.is_empty() {
        merge_args.extend(["-m", title]);
    }
    merge_args.push(tip.as_str());
    let merged = match git_run(&dir, &merge_args, true) {
        Ok(finished) => finished,
        Err(err) => return done(Outcome::error(err)),
    };
    if !merged.succeeded() {
        let conflicted: Vec<String> = git(&dir, &["diff", "--name-only", "-z", "--diff-filter=U"])
            .unwrap_or_default()
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .collect();
        if conflicted.is_empty() {
            return done(Outcome::error(format!(
                "merging {branch} onto {target} failed: {}",
                problem(&merged.err, &merged.out)
            )));
        }
        let mut outcome = Outcome::new(
            Kind::Conflict,
            format!(
                "Merging {branch} onto the current {target} conflicts in {}. Merge {target} into your branch \
(`git merge {target}`) and resolve the conflicts keeping both sides: what landed on {target} is the person's or \
another worker's work, never yours to drop. Commit, then call delivery_submit again. If both cannot stand \
together, say so in a comment on your task and stop.",
                listing(&conflicted)
            ),
        );
        outcome.files = conflicted;
        outcome.base = Some(base);
        return done(outcome);
    }
    let merge = match rev(&dir, "HEAD") {
        Ok(sha) => sha,
        Err(err) => return done(Outcome::error(err)),
    };
    if merge == base {
        return done(Outcome::error(format!(
            "{branch} has nothing that is not on {target} already. Commit your work on {branch}, then call delivery_submit again"
        )));
    }
    let changed: Vec<String> = git(&dir, &["diff", "--name-only", "-z", &base, &merge])
        .unwrap_or_default()
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect();
    let prefix = git(
        Path::new(&request.workspace),
        &["rev-parse", "--show-prefix"],
    )
    .map(|out| out.trim().to_string())
    .unwrap_or_default();
    let outside: Vec<String> = if request.task.owns.is_empty() {
        vec![]
    } else {
        changed
            .iter()
            .filter(|path| !owned(path, &request.task.owns, &prefix))
            .cloned()
            .collect()
    };
    let mut passed = Outcome::new(Kind::Landed, String::new());
    passed.changed = changed.clone();
    passed.outside_owns = outside.clone();
    passed.merge = Some(merge.clone());
    passed.base = Some(base.clone());

    // 3. The workspace's check, on the merged result. Not when it cannot tell
    // anything: nothing but documentation changed, or these very files passed
    // it before.
    let line = request
        .check_command
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty());
    let tree = git(&dir, &["rev-parse", "HEAD^{tree}"])
        .ok()
        .map(|out| out.trim().to_string());
    let repo_key = common_dir(worktree)
        .map(|dir| key(&dir.to_string_lossy()))
        .unwrap_or_default();
    if let Some(line) = line {
        if documentation_only(&changed) {
            passed.check_skipped = Some(SKIPPED_DOCS.to_string());
        } else if tree
            .as_deref()
            .is_some_and(|tree| passed_before(request, &repo_key, tree, line))
        {
            passed.check_skipped = Some(SKIPPED_PASSED.to_string());
        }
    }
    if let Some(line) = line.filter(|_| passed.check_skipped.is_none()) {
        stage.set("checking");
        if let Some((main, _, _)) = worktrees(worktree)
            .ok()
            .and_then(|list| list.into_iter().find(|(_, _, bare)| !bare))
        {
            copy_local_files(Path::new(&main), &dir);
        }
        let check = match run_check(request, &dir, line, limits.check) {
            Ok(check) => check,
            Err(err) => return done(Outcome::error(err)),
        };
        if !check.passed {
            let how = if check.timed_out {
                format!(
                    "did not finish within {} minutes and was stopped",
                    limits.check.as_secs().div_ceil(60)
                )
            } else {
                match check.exit_code {
                    Some(code) => format!("failed (exit {code})"),
                    None => "failed".to_string(),
                }
            };
            let mut outcome = passed;
            outcome.outcome = Kind::CheckFailed;
            outcome.detail = format!(
                "The check `{line}` {how} on {branch} merged onto {target}. It runs in a checkout of that \
merge that Hangar keeps for this workspace and resets to each merge: untracked files are removed, files git \
ignores stay, and the build output is shared and outside it. It has to install what it needs itself. The end of its output:\n```\n{}\n```\nFix it on your branch, run the check yourself, \
commit, then call delivery_submit again.",
                check.tail
            );
            outcome.check = Some(check);
            return done(outcome);
        }
        if let Some(tree) = &tree {
            remember_passed(request, &repo_key, tree, line);
        }
        passed.check = Some(check);
    }

    // 4. A reviewer that did not write it.
    stage.set("reviewing");
    let diff = git(
        &dir,
        &["diff", "--no-color", "--no-ext-diff", "-M", &base, &merge],
    )
    .unwrap_or_default();
    let prompt = review_prompt(request, &base, &merge, &changed, &outside, &diff);
    let verdict = match reviewer.review(
        &dir,
        &request.reviewer_model,
        &prompt,
        request.claude_account.as_deref(),
    ) {
        Ok(verdict) => verdict,
        Err(err) => {
            let mut outcome = passed;
            outcome.outcome = Kind::Error;
            outcome.detail = Outcome::error(format!("the review could not run: {err}")).detail;
            return done(outcome);
        }
    };
    if !verdict.passes() {
        let mut items: Vec<String> = verdict
            .blocking
            .iter()
            .map(|item| match item.line {
                Some(line) => format!("- {}:{line}: {}", item.file, item.problem.trim()),
                None => format!("- {}: {}", item.file, item.problem.trim()),
            })
            .collect();
        if items.is_empty() {
            items.push("- (no item given)".to_string());
        }
        let notes = verdict.notes.trim();
        let mut outcome = passed;
        outcome.outcome = Kind::ChangesRequested;
        outcome.detail = format!(
            "The reviewer asked for changes before {branch} lands:\n{}\n{}Fix these on your branch, commit, then call \
delivery_submit again. If you believe an item is wrong, say why in a comment on the task and stop, rather than \
working around it.",
            items.join("\n"),
            if notes.is_empty() {
                String::new()
            } else {
                format!("Notes: {notes}\n")
            }
        );
        outcome.review = Some(verdict);
        return done(outcome);
    }
    passed.review = Some(verdict);
    // 5. Landed, or kept tested for the person.
    if !request.land {
        save_tested(worktree, task, &base, &merge, &tip);
        let mut outcome = passed;
        outcome.outcome = Kind::Ready;
        let skipped = outcome
            .check_skipped
            .as_ref()
            .map(|why| format!("Its check was skipped: {why}. "))
            .unwrap_or_default();
        outcome.detail = format!(
            "{skipped}{branch} passed {}its review. Automatic landing is off in this workspace, so it waits for the person to \
land it on {target}. There is nothing more for you to do: stop here.",
            if request.check_command.is_some() && outcome.check_skipped.is_none() {
                "its check and "
            } else {
                ""
            }
        );
        return done(outcome);
    }
    stage.set("landing");
    land_step(request, worktree, &base, &merge, &tip, passed)
}

enum Landing {
    /// Fast-forwarded in that checkout, or, checked out nowhere, its ref moved.
    Landed(Option<String>),
    /// The checkout holding the branch would lose local changes: git said no.
    Refused {
        place: String,
        files: Vec<String>,
        reason: String,
    },
    Moved,
    Failed(String),
}

/// The files git names under a "would be overwritten" refusal.
fn overwritten(said: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut listing = false;
    for line in said.lines() {
        if line.contains("would be overwritten") {
            listing = true;
            continue;
        }
        if listing {
            if line.starts_with('\t') || line.starts_with("    ") {
                files.push(line.trim().to_string());
            } else {
                listing = false;
            }
        }
    }
    files
}

/// Moves `target` from `base` to `merge`, without ever touching what the
/// person has not committed.
fn land(repo: &Path, target: &str, base: &str, merge: &str) -> Landing {
    let list = match worktrees(repo) {
        Ok(list) => list,
        Err(err) => return Landing::Failed(err),
    };
    let holder = list
        .into_iter()
        .find(|(_, branch, bare)| !bare && branch.as_deref() == Some(target));
    let Some(place) = holder.map(|(place, _, _)| native(&place)) else {
        let reference = format!("refs/heads/{target}");
        return match git(
            repo,
            &[
                "update-ref",
                "-m",
                "hangar: land a delivery",
                &reference,
                merge,
                base,
            ],
        ) {
            Ok(_) => Landing::Landed(None),
            Err(err) => match rev(repo, &reference) {
                Ok(now) if now != base => Landing::Moved,
                _ => Landing::Failed(err),
            },
        };
    };
    let dir = Path::new(&place);
    match rev(dir, "HEAD") {
        Ok(head) if head != base => return Landing::Moved,
        Ok(_) => {}
        Err(err) => return Landing::Failed(err),
    }
    // A fast-forward or nothing: git refuses rather than overwrite local
    // changes, and autostash is off so it never sets them aside either.
    let finished = match git_run(
        dir,
        &[
            "-c",
            "merge.autoStash=false",
            "merge",
            "--ff-only",
            "--no-edit",
            merge,
        ],
        true,
    ) {
        Ok(finished) => finished,
        Err(err) => return Landing::Failed(err),
    };
    if finished.succeeded() {
        return Landing::Landed(Some(place));
    }
    let said = format!("{}\n{}", finished.err, finished.out);
    let lower = said.to_lowercase();
    if lower.contains("not possible to fast-forward") {
        return Landing::Moved;
    }
    let files = overwritten(&said);
    if !files.is_empty()
        || lower.contains("would be overwritten")
        || lower.contains("merge_head exists")
        || lower.contains("not concluded your merge")
        || lower.contains("unmerged files")
        || lower.contains("you have unmerged paths")
    {
        return Landing::Refused {
            place,
            files,
            reason: problem(&finished.err, &finished.out),
        };
    }
    Landing::Failed(problem(&finished.err, &finished.out))
}

/// `tip` is the head of the worker's branch that `merge` holds.
fn land_step(
    request: &Delivery,
    repo: &Path,
    base: &str,
    merge: &str,
    tip: &str,
    mut outcome: Outcome,
) -> Step {
    let (branch, target, task) = (&request.branch, &request.target, &request.task.id);
    outcome.merge = Some(merge.to_string());
    outcome.base = Some(base.to_string());
    match land(repo, target, base, merge) {
        Landing::Landed(place) => {
            forget_tested(repo, task);
            outcome.outcome = Kind::Landed;
            outcome.detail = match &place {
                Some(place) => format!(
                    "{branch} landed on {target} ({}), fast-forwarded in {place}.",
                    short(merge)
                ),
                None => format!(
                    "{branch} landed on {target} ({}); {target} is checked out nowhere, so only its ref moved.",
                    short(merge)
                ),
            };
            outcome.landed_in = place;
            Step::Done(Box::new(outcome))
        }
        Landing::Refused {
            place,
            files,
            reason,
        } => {
            // Tested and reviewed: kept as it is, for the person to land once
            // their checkout lets it through.
            save_tested(repo, task, base, merge, tip);
            outcome.outcome = Kind::Dirty;
            outcome.in_checkout = true;
            outcome.detail = format!(
                "{branch} passed {}its review, but {target} is checked out in {place}, which has local changes \
landing would overwrite{}. Nothing there was touched (git said: {reason}). It waits for the person: once \
those changes are committed or set aside, they land it from Hangar. There is nothing more for you to do: stop here.",
                if request.check_command.is_some() {
                    "its check and "
                } else {
                    ""
                },
                if files.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", listing(&files))
                }
            );
            outcome.files = files;
            outcome.landed_in = None;
            Step::Done(Box::new(outcome))
        }
        Landing::Moved => Step::Moved,
        Landing::Failed(err) => {
            outcome.outcome = Kind::Error;
            outcome.detail =
                Outcome::error(format!("landing {branch} on {target} failed: {err}")).detail;
            Step::Done(Box::new(outcome))
        }
    }
}

/* ── Before and after ──────────────────────────────────────────── */

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The main checkout, or the repository itself when it is bare.
    pub repo: String,
    /// What its deliveries wait in line on: the repository's common directory.
    pub queue: String,
    pub branch: String,
    pub target: String,
    /// Commits on the branch that the target does not have.
    pub ahead: u32,
    /// The task's delivery that waits for the person, when there is one.
    pub tested: Option<Tested>,
}

/// What a delivery from `worktree` would be, or why it cannot be one — in
/// words for the worker. `landing` asks about a delivery already tested,
/// which only needs the branches; `task` asks about what of it waits.
fn preflight(
    workspace: &str,
    worktree: &str,
    target: Option<&str>,
    landing: bool,
    task: Option<&str>,
) -> Result<Plan, String> {
    let dir = Path::new(worktree);
    let common =
        common_dir(dir).map_err(|err| format!("{worktree} is not a git worktree: {err}."))?;
    let home = common_dir(Path::new(workspace)).map_err(|err| {
        format!("The workspace folder {workspace} is not a git repository ({err}), so there is nothing to land on.")
    })?;
    if key(&common.to_string_lossy()) != key(&home.to_string_lossy()) {
        return Err(format!(
            "{worktree} is not a worktree of the workspace's repository ({workspace}), so it cannot land there."
        ));
    }
    let branch = branch_at(dir).ok_or_else(|| {
        format!(
            "{worktree} is on a detached HEAD: check out your branch, commit on it, then call delivery_submit again."
        )
    })?;
    let target = match target.map(str::trim).filter(|t| !t.is_empty()) {
        Some(target) => target.to_string(),
        None => branch_at(Path::new(workspace)).ok_or_else(|| {
            format!(
                "The workspace folder {workspace} is on a detached HEAD, so there is no branch to land on. The person \
can name one in the workspace's orchestration settings."
            )
        })?,
    };
    rev(dir, &format!("refs/heads/{target}"))
        .map_err(|_| format!("The branch deliveries land on, {target}, does not exist."))?;
    if branch == target {
        return Err(format!(
            "Your worktree is on {target} itself, the branch deliveries land on. Work on a branch of your own."
        ));
    }
    let queue = key(&common.to_string_lossy());
    let repo = if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().map(Path::to_path_buf).unwrap_or(common)
    } else {
        common
    };
    let ahead: u32 = git(
        dir,
        &["rev-list", "--count", &format!("{target}..{branch}")],
    )
    .ok()
    .and_then(|out| out.trim().parse().ok())
    .unwrap_or(0);
    if !landing {
        let files = uncommitted(dir)?;
        if !files.is_empty() {
            return Err(format!(
                "Your worktree has uncommitted changes: {}. Only commits are delivered: commit them on {branch} (or \
delete what is not part of the task), then call delivery_submit again.",
                listing(&files)
            ));
        }
        if ahead == 0 {
            return Err(format!(
                "{branch} has no commit that {target} does not already have. Commit your work on {branch}, then call \
delivery_submit again."
            ));
        }
    }
    let tested = task.and_then(|task| {
        let tip = rev(dir, &format!("refs/heads/{branch}")).ok()?;
        tested(dir, task, &tip)
    });
    Ok(Plan {
        repo: repo.to_string_lossy().into_owned(),
        queue,
        branch,
        target,
        ahead,
        tested,
    })
}

/// The checkout whose `<name>-worktrees` folder holds `dir`, when that is a
/// repository. Only the sweep, which checks the rest, acts on it.
fn checkout_of(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().skip(1).find_map(|folder| {
        let name = folder.file_name()?.to_str()?;
        let main = folder.parent()?.join(name.strip_suffix("-worktrees")?);
        common_dir(&main).ok().map(|_| main)
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cleanup {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    /// What was kept, and why.
    pub kept: Vec<String>,
}

/// After a landing: the worker's worktree and branch go, unless they hold
/// something that did not land. Its pane was just closed, and on Windows the
/// processes it ran can hold the folder for a moment, hence the tries.
fn cleanup(worktree: &str, branch: &str, target: &str, task: &str, tries: u32) -> Cleanup {
    let mut report = Cleanup {
        worktree_removed: false,
        branch_deleted: false,
        kept: vec![],
    };
    let dir = Path::new(worktree);
    // Run from the main checkout, not from the folder being removed.
    let repo = match common_dir(dir) {
        Ok(common) => match worktrees(dir).ok().and_then(|list| list.into_iter().next()) {
            Some((main, _, _)) => PathBuf::from(main),
            None => common,
        },
        // Git has already let go of the folder (its `.git` is gone): the
        // checkout it came from is found by where Hangar puts worktrees.
        Err(_) => match checkout_of(dir) {
            Some(main) => main,
            None => {
                report
                    .kept
                    .push(format!("{worktree} is not a worktree any more"));
                return report;
            }
        },
    };
    forget_tested(&repo, task);
    let listed = worktrees(&repo).unwrap_or_default();
    let linked = listed
        .iter()
        .skip(1)
        .any(|(path, _, _)| key(path) == key(worktree));
    // What is left of the folder once git has done what it can.
    let sweep = |report: &mut Cleanup, said: Option<&str>| match crate::git::sweep_leftovers(
        &repo,
        dir,
        tries.clamp(1, 5),
        Duration::from_millis(400),
    ) {
        Ok(()) => report.worktree_removed = true,
        Err(crate::git::Sweep::Held(held)) => report.kept.push(held),
        Err(crate::git::Sweep::Refused(refusal)) => report.kept.push(match said {
            Some(said) if !said.contains("is not a working tree") => {
                format!("the worktree {worktree} could not be removed: {said}")
            }
            _ => refusal,
        }),
    };
    if linked {
        let mut failure: Option<String> = None;
        for attempt in 0..tries.max(1) {
            match git_run(&repo, &["worktree", "remove", worktree], false) {
                Ok(finished) if finished.succeeded() => {
                    report.worktree_removed = true;
                    failure = None;
                    break;
                }
                Ok(finished) => {
                    let said = problem(&finished.err, &finished.out);
                    if said.contains("modified or untracked files") || said.contains("locked") {
                        report.kept.push(format!(
                            "the worktree {worktree} has changes that were never delivered ({said}): kept"
                        ));
                        failure = None;
                        break;
                    }
                    let gone = said.contains("is not a working tree");
                    failure = Some(said);
                    // Git has already let go of it: only the folder is left.
                    if gone || attempt + 1 == tries.max(1) {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                Err(err) => {
                    report.kept.push(format!(
                        "the worktree {worktree} could not be removed: {err}"
                    ));
                    break;
                }
            }
        }
        if let Some(said) = failure {
            sweep(&mut report, Some(&said));
        }
        crate::git::forget_shared_roots();
    } else if listed.iter().any(|(path, _, _)| key(path) == key(worktree)) {
        report
            .kept
            .push(format!("{worktree} is the main checkout: kept"));
    } else if dir.exists() {
        // Git forgot it, the folder stayed: that is what piles up on disk.
        sweep(&mut report, None);
    } else {
        report.worktree_removed = true;
    }
    if report.worktree_removed {
        let reference = format!("refs/heads/{branch}");
        let landed = git(
            &repo,
            &[
                "merge-base",
                "--is-ancestor",
                &reference,
                &format!("refs/heads/{target}"),
            ],
        )
        .is_ok();
        if rev(&repo, &reference).is_err() {
            report.branch_deleted = true;
        } else if landed {
            // All of it is on the target, so nothing is lost; -D because -d
            // compares with whatever HEAD happens to be checked out.
            match git(&repo, &["branch", "-D", branch]) {
                Ok(_) => report.branch_deleted = true,
                Err(err) => report
                    .kept
                    .push(format!("the branch {branch} could not be deleted: {err}")),
            }
        } else {
            report.kept.push(format!(
                "the branch {branch} has commits that are not on {target}: kept"
            ));
        }
    }
    report
}

pub async fn delivery_preflight(
    workspace: String,
    worktree: String,
    target: Option<String>,
    landing: Option<bool>,
    task: Option<String>,
) -> Result<Plan, String> {
    crate::git::off_thread(move || {
        preflight(
            &workspace,
            &worktree,
            target.as_deref(),
            landing.unwrap_or(false),
            task.as_deref(),
        )
    })
    .await
}

/// Takes as long as the delivery does — the check and the review are minutes —
/// on a thread of its own, never the window's.
pub async fn delivery_run(delivery: Delivery) -> Outcome {
    tokio::task::spawn_blocking(move || {
        deliver(
            &delivery,
            &Claude {
                limit: REVIEW_TIMEOUT,
            },
            &Limits::default(),
        )
    })
    .await
    .unwrap_or_else(|err| Outcome::error(format!("the delivery stopped unexpectedly: {err}")))
}

pub async fn delivery_cleanup(
    worktree: String,
    branch: String,
    target: String,
    task_id: String,
) -> Cleanup {
    tokio::task::spawn_blocking(move || cleanup(&worktree, &branch, &target, &task_id, 10))
        .await
        .unwrap_or(Cleanup {
            worktree_removed: false,
            branch_deleted: false,
            kept: vec!["the cleanup stopped unexpectedly".to_string()],
        })
}

pub fn delivery_running() -> Vec<Running> {
    stages().lock().unwrap().values().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::fs;

    /// A repository on `main` with one commit, its main checkout, and a
    /// worker's worktree on `worker` branched from it. Removed with the test.
    struct Repo {
        root: PathBuf,
        main: PathBuf,
        worker: PathBuf,
        task: String,
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = git_command(dir, args).output().expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(dir: &Path, file: &str, text: &str) {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(dir: &Path, file: &str) -> String {
        fs::read_to_string(dir.join(file)).unwrap_or_default()
    }

    fn commit(dir: &Path, message: &str) {
        sh(dir, &["add", "-A"]);
        sh(dir, &["commit", "-q", "-m", message]);
    }

    fn repo(tag: &str) -> Repo {
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let root =
            std::env::temp_dir().join(format!("hangar-delivery-test-{tag}-{}", &unique[..8]));
        fs::create_dir_all(&root).unwrap();
        let root = dunce::canonicalize(&root).unwrap();
        let main = root.join("repo");
        fs::create_dir_all(&main).unwrap();
        sh(&main, &["init", "-q", "-b", "main"]);
        for (name, value) in [
            ("user.name", "Test"),
            ("user.email", "test@example.com"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            sh(&main, &["config", name, value]);
        }
        write(&main, "a.txt", "one\ntwo\nthree\n");
        write(&main, "src/app.ts", "export const app = 1;\n");
        commit(&main, "Start");
        let worker = root.join("worker");
        sh(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "worker",
                &worker.to_string_lossy(),
                "main",
            ],
        );
        Repo {
            root,
            main,
            worker,
            // Unique from its first characters, which name its scratch folder.
            task: format!("{}-{tag}", &unique[..8]),
        }
    }

    fn request(repo: &Repo) -> Delivery {
        Delivery {
            workspace: repo.main.to_string_lossy().into_owned(),
            worktree: repo.worker.to_string_lossy().into_owned(),
            branch: "worker".into(),
            target: "main".into(),
            task: TaskBrief {
                id: repo.task.clone(),
                title: "Add b.rs".into(),
                description: "Add b.rs holding one line.".into(),
                owns: vec!["b.rs".into()],
            },
            summary: Some("Added b.rs.".into()),
            check_command: None,
            shell: None,
            reviewer_model: "sonnet".into(),
            claude_account: None,
            land: true,
            reuse: false,
            port_base: None,
            workspace_id: format!("ws-{}", repo.task),
            build_env: vec![],
            check_root: Some(repo.root.join("checks").to_string_lossy().into_owned()),
            data_dir: Some(repo.root.join("data").to_string_lossy().into_owned()),
        }
    }

    fn head(dir: &Path, name: &str) -> String {
        sh(dir, &["rev-parse", name])
    }

    /// Nothing left of a delivery but the workspace's check folder: the
    /// repository lists its main checkout, the worker's and at most that one,
    /// and the folder holds no merge half done.
    fn assert_no_scratch(repo: &Repo) {
        let listed = worktrees(&repo.main).unwrap();
        let checks = format!("{}/", key(&repo.root.join("checks").to_string_lossy()));
        let others: Vec<_> = listed
            .iter()
            .filter(|(path, _, _)| key(path).starts_with(&checks))
            .collect();
        assert!(others.len() <= 1, "more than one check folder: {listed:?}");
        assert_eq!(listed.len(), 2 + others.len(), "{listed:?}");
        for (path, _, _) in others {
            let status = sh(Path::new(path), &["status", "--porcelain"]);
            assert!(status.is_empty(), "the check folder is not clean: {status}");
        }
    }

    fn approve() -> Verdict {
        Verdict {
            approve: true,
            blocking: vec![],
            notes: "Looks right.".into(),
        }
    }

    /// Answers with its verdicts in turn, the last one again and again,
    /// keeping the prompts it was given; `during` runs while it reviews, the
    /// way the person could commit meanwhile.
    struct Fake<F: Fn(usize)> {
        verdicts: RefCell<Vec<Result<Verdict, String>>>,
        prompts: RefCell<Vec<String>>,
        calls: Cell<usize>,
        during: F,
    }

    fn fake(verdict: Result<Verdict, String>) -> Fake<impl Fn(usize)> {
        fake_with(vec![verdict], |_| {})
    }

    fn fake_with<F: Fn(usize)>(verdicts: Vec<Result<Verdict, String>>, during: F) -> Fake<F> {
        Fake {
            verdicts: RefCell::new(verdicts),
            prompts: RefCell::new(vec![]),
            calls: Cell::new(0),
            during,
        }
    }

    impl<F: Fn(usize)> Reviewer for Fake<F> {
        fn review(
            &self,
            dir: &Path,
            model: &str,
            prompt: &str,
            _: Option<&str>,
        ) -> Result<Verdict, String> {
            assert_eq!(model, "sonnet");
            assert!(dir.is_dir(), "the reviewer reads the merged worktree");
            let call = self.calls.get();
            self.calls.set(call + 1);
            self.prompts.borrow_mut().push(prompt.to_string());
            (self.during)(call);
            let mut verdicts = self.verdicts.borrow_mut();
            if verdicts.len() > 1 {
                verdicts.remove(0)
            } else {
                verdicts[0].clone()
            }
        }
    }

    /// For a delivery that must not reach the review.
    struct Never;

    impl Reviewer for Never {
        fn review(&self, _: &Path, _: &str, _: &str, _: Option<&str>) -> Result<Verdict, String> {
            panic!("reviewed");
        }
    }

    /// By its full path: cargo hands tests a PATH longer than cmd.exe reads.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn ping() -> String {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\System32\PING.EXE")
    }

    fn deliver_now(request: &Delivery, reviewer: &dyn Reviewer) -> Outcome {
        deliver(request, reviewer, &Limits::default())
    }

    /// The task's delivery kept for the person, against the worker's branch now.
    fn kept(repo: &Repo) -> Option<Tested> {
        tested(&repo.main, &repo.task, &head(&repo.main, "worker"))
    }

    #[test]
    fn a_branch_ahead_of_the_target_lands_as_a_fast_forward() {
        let repo = repo("ff");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        request.check_command = Some("echo checked".into());
        let reviewer = fake(Ok(approve()));

        let outcome = deliver_now(&request, &reviewer);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(
            head(&repo.main, "main"),
            head(&repo.main, "worker"),
            "no merge commit"
        );
        assert_eq!(
            read(&repo.main, "b.rs"),
            "bee\n",
            "the person's checkout moved with it"
        );
        assert_eq!(
            outcome.landed_in.as_deref().map(key),
            Some(key(&repo.main.to_string_lossy()))
        );
        let check = outcome.check.clone().expect("the check ran");
        assert!(check.passed && check.tail.contains("checked"), "{check:?}");
        assert_eq!(outcome.changed, vec!["b.rs".to_string()]);
        assert!(outcome.outside_owns.is_empty());
        let prompt = reviewer.prompts.borrow()[0].clone();
        assert!(prompt.contains("Add b.rs holding one line."));
        assert!(prompt.contains("+bee"), "the diff is in the prompt");
        assert!(prompt.contains("The check `echo checked` already passed"));
        assert!(kept(&repo).is_none());
        assert_no_scratch(&repo);
    }

    #[test]
    fn the_check_gets_the_local_files_the_main_checkout_has_and_nothing_replaced() {
        let repo = repo("local-files");
        write(&repo.main, ".gitignore", ".env\n");
        write(&repo.main, "app.local", "tracked\n");
        commit(&repo.main, "Ignore .env, track app.local");
        sh(&repo.worker, &["merge", "-q", "main"]);
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        // Ignored, so only the main checkout has it; and a tracked file the
        // person is editing, which the check must see as committed.
        write(&repo.main, ".env", "SECRET=42\n");
        write(&repo.main, "app.local", "mine\n");
        let mut request = request(&repo);
        request.check_command = Some(if cfg!(windows) {
            "type .env && type app.local".into()
        } else {
            "cat .env app.local".into()
        });

        let outcome = deliver_now(&request, &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        let tail = outcome.check.unwrap().tail;
        assert!(
            tail.contains("SECRET=42") && tail.contains("tracked"),
            "{tail}"
        );
        assert!(!tail.contains("mine"), "{tail}");
        assert_eq!(read(&repo.main, "app.local"), "mine\n");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_target_that_moved_on_gets_a_merge_with_both_sides() {
        let repo = repo("merge");
        write(&repo.main, "c.rs", "sea\n");
        commit(&repo.main, "Add c");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");

        let outcome = deliver_now(&request(&repo), &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        let parents = sh(&repo.main, &["rev-list", "--parents", "-n", "1", "main"]);
        assert_eq!(parents.split(' ').count(), 3, "a merge commit: {parents}");
        assert_eq!(read(&repo.main, "b.rs"), "bee\n");
        assert_eq!(read(&repo.main, "c.rs"), "sea\n");
        assert_eq!(sh(&repo.main, &["status", "--porcelain"]), "");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_conflict_names_its_files_and_lands_nothing() {
        let repo = repo("conflict");
        write(&repo.main, "a.txt", "one\nTWO (main)\nthree\n");
        commit(&repo.main, "Change two on main");
        write(&repo.worker, "a.txt", "one\nTWO (worker)\nthree\n");
        commit(&repo.worker, "Change two on the branch");
        let before = head(&repo.main, "main");

        let outcome = deliver_now(&request(&repo), &Never);

        assert_eq!(outcome.outcome, Kind::Conflict, "{}", outcome.detail);
        assert_eq!(outcome.files, vec!["a.txt".to_string()]);
        assert!(
            outcome.detail.contains("conflicts in a.txt"),
            "{}",
            outcome.detail
        );
        assert!(
            outcome.detail.contains("git merge main"),
            "{}",
            outcome.detail
        );
        assert!(outcome.detail.contains("delivery_submit again"));
        assert_eq!(head(&repo.main, "main"), before);
        assert_eq!(read(&repo.main, "a.txt"), "one\nTWO (main)\nthree\n");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_failing_check_says_how_and_shows_the_end_of_its_output() {
        let repo = repo("check");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let before = head(&repo.main, "main");
        let mut request = request(&repo);
        request.check_command = Some("echo boom && exit 3".into());

        let outcome = deliver_now(&request, &Never);

        assert_eq!(outcome.outcome, Kind::CheckFailed, "{}", outcome.detail);
        let check = outcome.check.clone().expect("the check ran");
        assert_eq!(check.exit_code, Some(3));
        assert!(!check.timed_out);
        assert!(check.tail.contains("boom"), "{check:?}");
        assert!(
            outcome.detail.contains("failed (exit 3)"),
            "{}",
            outcome.detail
        );
        assert!(outcome.detail.contains("boom"));
        assert_eq!(head(&repo.main, "main"), before);
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_check_that_runs_too_long_is_stopped_with_what_it_started() {
        let repo = repo("timeout");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        // A child of the shell, which the shell's own end would not reach.
        request.check_command = Some(if cfg!(windows) {
            format!("{} -n 60 127.0.0.1 >nul", ping())
        } else {
            "sleep 60; echo late".into()
        });

        let outcome = deliver(
            &request,
            &Never,
            &Limits {
                check: Duration::from_secs(2),
            },
        );

        assert_eq!(outcome.outcome, Kind::CheckFailed, "{}", outcome.detail);
        assert!(
            outcome.check.as_ref().unwrap().timed_out,
            "{:?}",
            outcome.check
        );
        assert!(
            outcome.detail.contains("did not finish within 1 minutes"),
            "{}",
            outcome.detail
        );
        // The check was stopped at its limit, not left to run its 60 s. This is the
        // check's own duration: the git work around it, slow on a loaded machine,
        // does not count.
        let seconds = outcome.check.as_ref().unwrap().seconds;
        assert!(
            seconds < 30,
            "the check ran {seconds} s: {:?}",
            outcome.check
        );
        // On Windows, a process still running in the worktree would hold it.
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_timed_out_process_takes_its_children_with_it() {
        let command = if cfg!(windows) {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/d", "/c", &format!("{} -n 60 127.0.0.1 >nul", ping())]);
            cmd
        } else {
            let mut sh = Command::new("sh");
            sh.args(["-c", "sleep 60; echo late"]);
            sh
        };
        let finished = run(
            command,
            Spawn {
                input: None,
                limit: Duration::from_millis(1500),
                merged: true,
                cap: CHECK_BYTES,
                cancellable: true,
            },
        )
        .unwrap();

        assert_eq!(finished.ended, Ended::TimedOut, "{finished:?}");
        assert!(finished.elapsed < Duration::from_secs(20));
        // Killing returns before the process is gone from the table, and the
        // shell's id may have gone to a newer process since, whose children
        // are its own: only children older than that one were the shell's.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let table = crate::procs::table().expect("process table");
            let reused = crate::procs::started(finished.pid);
            let left: Vec<u32> = table
                .iter()
                .filter(|proc| proc.ppid == finished.pid && proc.pid != proc.ppid)
                .map(|proc| proc.pid)
                .filter(|&child| match (reused, crate::procs::started(child)) {
                    (Some(newer), Some(born)) => born < newer,
                    _ => true,
                })
                .collect();
            if left.is_empty() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "children of {} still running: {left:?}",
                finished.pid
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn uncommitted_work_in_the_worker_s_worktree_is_refused() {
        let repo = repo("dirty-worker");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        write(&repo.worker, "a.txt", "changed\n");
        write(&repo.worker, "notes.md", "untracked\n");
        let before = head(&repo.main, "main");

        let outcome = deliver_now(&request(&repo), &Never);

        assert_eq!(outcome.outcome, Kind::Dirty, "{}", outcome.detail);
        assert!(!outcome.in_checkout);
        assert_eq!(
            outcome.files,
            vec!["a.txt".to_string(), "notes.md".to_string()]
        );
        assert!(
            outcome.detail.contains("commit them on worker"),
            "{}",
            outcome.detail
        );
        assert_eq!(head(&repo.main, "main"), before);
        assert_no_scratch(&repo);

        let refused = preflight(
            &repo.main.to_string_lossy(),
            &repo.worker.to_string_lossy(),
            None,
            false,
            None,
        )
        .unwrap_err();
        assert!(
            refused.contains("uncommitted changes: a.txt, notes.md"),
            "{refused}"
        );
    }

    #[test]
    fn local_changes_in_the_way_stop_the_landing_and_lose_nothing() {
        let repo = repo("dirty-main");
        write(&repo.worker, "a.txt", "one\ntwo (worker)\nthree\n");
        commit(&repo.worker, "Change two");
        // The person's own edit, never committed, to the file the delivery changes.
        write(&repo.main, "a.txt", "one\ntwo (mine)\nthree\n");
        let before = head(&repo.main, "main");
        let mut request = request(&repo);
        request.task.owns = vec!["a.txt".into()];

        let outcome = deliver_now(&request, &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Dirty, "{}", outcome.detail);
        assert!(outcome.in_checkout);
        assert_eq!(outcome.files, vec!["a.txt".to_string()]);
        assert!(
            outcome.detail.contains("Nothing there was touched"),
            "{}",
            outcome.detail
        );
        assert_eq!(read(&repo.main, "a.txt"), "one\ntwo (mine)\nthree\n");
        assert_eq!(head(&repo.main, "main"), before);
        assert_eq!(
            sh(&repo.main, &["stash", "list"]),
            "",
            "nothing was stashed"
        );
        let Tested {
            base,
            merge,
            current,
        } = kept(&repo).expect("kept tested");
        assert_eq!(base, before);
        assert!(current);
        assert_no_scratch(&repo);

        // The person sets their edit aside themselves; the tested merge lands
        // as it was reviewed, without a second review.
        write(&repo.main, "a.txt", "one\ntwo\nthree\n");
        request.reuse = true;
        let outcome = deliver_now(&request, &Never);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(head(&repo.main, "main"), merge);
        assert_eq!(read(&repo.main, "a.txt"), "one\ntwo (worker)\nthree\n");
        assert!(kept(&repo).is_none());
        assert_no_scratch(&repo);
    }

    #[test]
    fn local_changes_elsewhere_ride_along_a_landing() {
        let repo = repo("bystander");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        write(&repo.main, "src/app.ts", "export const app = 2; // mine\n");

        let outcome = deliver_now(&request(&repo), &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(
            read(&repo.main, "src/app.ts"),
            "export const app = 2; // mine\n"
        );
        assert_eq!(sh(&repo.main, &["status", "--porcelain"]), "M src/app.ts");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_target_that_moves_during_the_review_is_tested_again_on_its_new_head() {
        let repo = repo("moving");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let main = repo.main.clone();
        let reviewer = fake_with(vec![Ok(approve())], move |call| {
            if call == 0 {
                write(&main, "c.rs", "sea\n");
                commit(&main, "The person commits meanwhile");
            }
        });

        let outcome = deliver_now(&request(&repo), &reviewer);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(reviewer.calls.get(), 2, "reviewed again on the new head");
        assert_eq!(read(&repo.main, "b.rs"), "bee\n");
        assert_eq!(read(&repo.main, "c.rs"), "sea\n");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_delivery_waiting_for_the_person_is_tested_again_if_the_target_moved() {
        let repo = repo("ready");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let before = head(&repo.main, "main");
        let mut request = request(&repo);
        request.land = false;

        let outcome = deliver_now(&request, &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Ready, "{}", outcome.detail);
        assert_eq!(head(&repo.main, "main"), before, "nothing landed");
        assert!(kept(&repo).is_some());
        assert_no_scratch(&repo);

        write(&repo.main, "c.rs", "sea\n");
        commit(&repo.main, "The person commits");
        request.land = true;
        request.reuse = true;
        let reviewer = fake(Ok(approve()));
        let outcome = deliver_now(&request, &reviewer);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(
            reviewer.calls.get(),
            1,
            "the merge it was tested as is stale"
        );
        assert_eq!(read(&repo.main, "b.rs"), "bee\n");
        assert_eq!(read(&repo.main, "c.rs"), "sea\n");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_fix_submitted_after_passing_goes_through_it_all_again_and_only_it_lands() {
        let repo = repo("resubmit");
        let (main, worker) = (
            repo.main.to_string_lossy().into_owned(),
            repo.worker.to_string_lossy().into_owned(),
        );
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let before = head(&repo.main, "main");
        let mut request = request(&repo);
        request.land = false;
        assert_eq!(
            deliver_now(&request, &fake(Ok(approve()))).outcome,
            Kind::Ready
        );
        let first = kept(&repo).expect("kept tested").merge;

        // Unchanged, what waits is still the worker's latest.
        let plan = preflight(&main, &worker, None, false, Some(&repo.task)).unwrap();
        let waiting = plan.tested.expect("it says what waits");
        assert_eq!(
            (waiting.merge.as_str(), waiting.current),
            (first.as_str(), true)
        );

        // The orchestrator finds a bug; the worker commits the fix.
        write(&repo.worker, "b.rs", "bee!\n");
        commit(&repo.worker, "Fix b");
        let plan = preflight(&main, &worker, None, false, Some(&repo.task)).unwrap();
        assert!(
            !plan.tested.expect("still kept").current,
            "the branch moved"
        );

        // Submitted again: merged, checked and reviewed again, and kept as the
        // new one that waits.
        request.check_command = Some("echo checked".into());
        let reviewer = fake(Ok(approve()));
        let outcome = deliver_now(&request, &reviewer);

        assert_eq!(outcome.outcome, Kind::Ready, "{}", outcome.detail);
        assert_eq!(reviewer.calls.get(), 1, "reviewed again");
        assert!(outcome.check.is_some(), "checked again");
        assert!(reviewer.prompts.borrow()[0].contains("+bee!"));
        let second = kept(&repo).expect("kept tested");
        assert!(second.current);
        assert_ne!(second.merge, first);
        assert_eq!(head(&repo.main, "main"), before, "nothing landed");

        // The person lands it: the fix, as it was tested.
        request.land = true;
        request.reuse = true;
        let outcome = deliver_now(&request, &Never);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(head(&repo.main, "main"), second.merge);
        assert_eq!(read(&repo.main, "b.rs"), "bee!\n");
        assert!(kept(&repo).is_none());
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_new_delivery_that_fails_leaves_nothing_waiting_to_land() {
        let repo = repo("resubmit-fails");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        request.land = false;
        assert_eq!(
            deliver_now(&request, &fake(Ok(approve()))).outcome,
            Kind::Ready
        );

        write(&repo.worker, "b.rs", "bee, broken\n");
        commit(&repo.worker, "Break b");
        let outcome = deliver_now(
            &request,
            &fake(Ok(Verdict {
                approve: false,
                blocking: vec![Blocking {
                    file: "b.rs".into(),
                    line: Some(1),
                    problem: "Broken.".into(),
                }],
                notes: String::new(),
            })),
        );

        assert_eq!(
            outcome.outcome,
            Kind::ChangesRequested,
            "{}",
            outcome.detail
        );
        assert!(
            kept(&repo).is_none(),
            "the first one does not wait any more"
        );
        assert_no_scratch(&repo);
    }

    #[test]
    fn landing_what_waited_after_the_worker_committed_again_lands_nothing() {
        let repo = repo("stale");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let before = head(&repo.main, "main");
        let mut request = request(&repo);
        request.land = false;
        assert_eq!(
            deliver_now(&request, &fake(Ok(approve()))).outcome,
            Kind::Ready
        );
        let first = kept(&repo).expect("kept tested").merge;

        // The worker commits more, and has not submitted it yet; the target
        // moved too, which changes nothing.
        write(&repo.worker, "b.rs", "bee, half done\n");
        commit(&repo.worker, "Start a fix");
        write(&repo.main, "c.rs", "sea\n");
        commit(&repo.main, "The person commits");
        let target = head(&repo.main, "main");
        request.land = true;
        request.reuse = true;
        let outcome = deliver_now(&request, &Never);

        assert_eq!(outcome.outcome, Kind::Stale, "{}", outcome.detail);
        assert!(
            outcome.detail.contains("call delivery_submit again"),
            "{}",
            outcome.detail
        );
        assert_eq!(outcome.merge.as_deref(), Some(first.as_str()));
        assert_eq!(head(&repo.main, "main"), target, "nothing landed");
        assert_ne!(target, before);
        assert_eq!(read(&repo.main, "b.rs"), "");
        assert!(kept(&repo).is_none(), "what waited is dropped");
        assert_no_scratch(&repo);
    }

    #[test]
    fn what_an_older_version_kept_still_tells_a_branch_that_moved() {
        let repo = repo("legacy");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        request.land = false;
        assert_eq!(
            deliver_now(&request, &fake(Ok(approve()))).outcome,
            Kind::Ready
        );
        // Kept with the target's head and the merge only, as 1.0.0 did.
        sh(
            &repo.main,
            &["update-ref", "-d", &tested_ref(&repo.task, "tip")],
        );
        assert!(kept(&repo).expect("kept tested").current);

        write(&repo.worker, "b.rs", "bee!\n");
        commit(&repo.worker, "Fix b");
        assert!(!kept(&repo).expect("kept tested").current);
    }

    /// A line that prints where it runs and what is there, and leaves a file.
    fn look() -> String {
        if cfg!(windows) {
            "cd && dir /b && echo x> junk.tmp".into()
        } else {
            "pwd && ls && echo x > junk.tmp".into()
        }
    }

    #[test]
    fn two_checks_of_a_workspace_run_in_the_same_folder_and_the_second_sees_only_its_own_merge() {
        let repo = repo("same-folder");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        request.land = false;
        request.check_command = Some(look());
        let first = deliver_now(&request, &fake(Ok(approve())));
        assert_eq!(first.outcome, Kind::Ready, "{}", first.detail);
        let first = first.check.unwrap().tail;
        assert!(first.contains("b.rs"), "{first}");

        sh(&repo.worker, &["rm", "-q", "b.rs"]);
        write(&repo.worker, "c.rs", "sea\n");
        commit(&repo.worker, "Swap b for c");
        let second = deliver_now(&request, &fake(Ok(approve())));
        assert_eq!(second.outcome, Kind::Ready, "{}", second.detail);
        let second = second.check.unwrap().tail;

        let place = |tail: &str| tail.lines().next().unwrap().trim().to_string();
        assert_eq!(place(&first), place(&second), "{first}\n{second}");
        assert!(second.contains("c.rs"), "{second}");
        assert!(!second.contains("b.rs"), "{second}");
        assert!(!second.contains("junk.tmp"), "{second}");
        assert_no_scratch(&repo);
    }

    #[test]
    fn the_check_runs_with_the_build_environment_after_the_scrub() {
        let shell = check_shell(None);
        let invocation = invocation(&shell, "true");
        let env = vec![("CARGO_TARGET_DIR".to_string(), "C:/cache/t".to_string())];
        let command = check_command(&invocation, Path::new("."), None, &env);
        assert_eq!(
            env_of(&command, "CARGO_TARGET_DIR"),
            Some(Some("C:/cache/t".into()))
        );
        let none = check_command(&invocation, Path::new("."), None, &[]);
        assert_eq!(env_of(&none, "CARGO_TARGET_DIR"), None);
    }

    #[test]
    fn only_documentation_is_documentation() {
        let docs = |paths: &[&str]| {
            documentation_only(&paths.iter().map(|p| p.to_string()).collect::<Vec<_>>())
        };
        assert!(docs(&[
            "README.md",
            "site/page.mdx",
            "notes.txt",
            "guide.rst",
            "docs/img/a.png",
            "LICENSE",
            "LICENSE-MIT",
            "CHANGELOG.md",
            ".github/ISSUE_TEMPLATE/bug.yml",
        ]));
        assert!(!docs(&[]));
        assert!(!docs(&["README.md", "src/main.rs"]));
        assert!(!docs(&[".github/workflows/ci.yml"]));
        assert!(!docs(&["CMakeLists.txt"]));
        assert!(!docs(&["requirements.txt"]));
        assert!(!docs(&["package.json"]));
    }

    #[test]
    fn a_documentation_only_delivery_skips_the_check_and_a_code_one_does_not() {
        let repo = repo("docs-only");
        write(&repo.worker, "README.md", "hello\n");
        commit(&repo.worker, "Docs");
        let mut request = request(&repo);
        request.land = false;
        request.check_command = Some("exit 1".into());
        let docs = deliver_now(&request, &fake(Ok(approve())));
        assert_eq!(docs.outcome, Kind::Ready, "{}", docs.detail);
        assert_eq!(docs.check_skipped.as_deref(), Some(SKIPPED_DOCS));
        assert!(docs.check.is_none());
        assert!(docs.review.is_some());

        write(&repo.worker, "src/lib.rs", "pub fn a() {}\n");
        commit(&repo.worker, "Code");
        let code = deliver_now(&request, &fake(Ok(approve())));
        assert_eq!(code.outcome, Kind::CheckFailed, "{}", code.detail);
        assert!(code.check_skipped.is_none());
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_tree_that_passed_skips_the_check_and_a_new_one_runs_it() {
        let repo = repo("same-tree");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        request.land = false;
        request.check_command = Some("echo ok".into());
        let first = deliver_now(&request, &fake(Ok(approve())));
        assert_eq!(first.outcome, Kind::Ready, "{}", first.detail);
        assert!(first.check.is_some() && first.check_skipped.is_none());

        // Submitted again: a new merge commit, the same files.
        let again = deliver_now(&request, &fake(Ok(approve())));
        assert_eq!(again.outcome, Kind::Ready, "{}", again.detail);
        assert!(again.check.is_none());
        assert_eq!(again.check_skipped.as_deref(), Some(SKIPPED_PASSED));

        // Other files: it runs. So does another check line on the same ones.
        write(&repo.worker, "c.rs", "sea\n");
        commit(&repo.worker, "Add c");
        let changed = deliver_now(&request, &fake(Ok(approve())));
        assert!(changed.check.is_some(), "{}", changed.detail);
        request.check_command = Some("echo fine".into());
        let other = deliver_now(&request, &fake(Ok(approve())));
        assert!(other.check.is_some(), "{}", other.detail);
        assert_no_scratch(&repo);
    }

    #[test]
    fn only_the_last_trees_that_passed_are_kept() {
        let dir = std::env::temp_dir().join(format!("hangar-passed-{}", uuid::Uuid::new_v4()));
        let mut request = request(&repo("passed-cap"));
        request.data_dir = Some(dir.to_string_lossy().into_owned());
        for n in 0..PASSED_TREES + 5 {
            remember_passed(&request, "repo", &format!("tree{n}"), "check");
        }
        assert!(!passed_before(&request, "repo", "tree0", "check"));
        assert!(passed_before(&request, "repo", "tree204", "check"));
        assert!(!passed_before(&request, "other", "tree204", "check"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn worktrees_old_versions_left_are_swept_with_their_folders() {
        let repo = repo("sweep");
        let old = repo.root.join("old");
        fs::create_dir_all(&old).unwrap();
        let name = format!("{SCRATCH_PREFIX}{}-abc-1234", std::process::id());
        let listed = old.join(&name);
        sh(
            &repo.main,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &listed.to_string_lossy(),
                "main",
            ],
        );
        let orphan = old.join(format!("{SCRATCH_PREFIX}{}-def-5678", std::process::id()));
        fs::create_dir_all(&orphan).unwrap();
        let other = old.join("someone-else");
        fs::create_dir_all(&other).unwrap();

        sweep_legacy(&old, &[repo.main.to_string_lossy().into_owned()]);

        assert!(!listed.exists() && !orphan.exists());
        assert!(other.exists(), "what is not a delivery's stays");
        assert_eq!(worktrees(&repo.main).unwrap().len(), 2);
    }

    #[test]
    fn a_target_checked_out_nowhere_lands_by_moving_its_ref() {
        let repo = repo("ref");
        sh(&repo.main, &["switch", "-q", "-c", "elsewhere"]);
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");

        let outcome = deliver_now(&request(&repo), &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(outcome.landed_in, None);
        assert_eq!(head(&repo.main, "main"), head(&repo.main, "worker"));
        assert_eq!(
            read(&repo.main, "b.rs"),
            "",
            "the checkout on another branch is left alone"
        );
        assert_no_scratch(&repo);
    }

    #[test]
    fn blocking_items_come_back_with_where_and_what() {
        let repo = repo("changes");
        write(&repo.worker, "b.rs", "bee\n");
        write(&repo.worker, "src/app.ts", "export const app = 3;\n");
        commit(&repo.worker, "Add b, and change app");
        let reviewer = fake(Ok(Verdict {
            approve: false,
            blocking: vec![Blocking {
                file: "src/app.ts".into(),
                line: Some(1),
                problem: "Changes app, which the task did not ask for.".into(),
            }],
            notes: "b.rs itself is fine.".into(),
        }));

        let outcome = deliver_now(&request(&repo), &reviewer);

        assert_eq!(
            outcome.outcome,
            Kind::ChangesRequested,
            "{}",
            outcome.detail
        );
        assert!(
            outcome
                .detail
                .contains("- src/app.ts:1: Changes app, which the task did not ask for."),
            "{}",
            outcome.detail
        );
        assert!(outcome.detail.contains("Notes: b.rs itself is fine."));
        assert_eq!(outcome.outside_owns, vec!["src/app.ts".to_string()]);
        let prompt = reviewer.prompts.borrow()[0].clone();
        assert!(
            prompt.contains("Changed outside the files the task owns: src/app.ts."),
            "{prompt}"
        );
        assert!(read(&repo.main, "b.rs").is_empty());
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_review_that_cannot_run_merges_nothing() {
        let repo = repo("no-review");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let before = head(&repo.main, "main");

        let outcome = deliver_now(&request(&repo), &fake(Err(NO_CLAUDE.to_string())));

        assert_eq!(outcome.outcome, Kind::Error);
        assert!(
            outcome.detail.contains("was not found on PATH"),
            "{}",
            outcome.detail
        );
        assert!(
            outcome.detail.ends_with("Nothing was merged."),
            "{}",
            outcome.detail
        );
        assert_eq!(head(&repo.main, "main"), before);
        assert_no_scratch(&repo);
    }

    #[test]
    fn preflight_names_the_branches_and_refuses_what_cannot_be_delivered() {
        let repo = repo("preflight");
        let main = repo.main.to_string_lossy().into_owned();
        let worker = repo.worker.to_string_lossy().into_owned();

        let empty = preflight(&main, &worker, None, false, None).unwrap_err();
        assert!(
            empty.contains("worker has no commit that main does not already have"),
            "{empty}"
        );

        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        let plan = preflight(&main, &worker, None, false, None).unwrap();
        assert_eq!(
            (plan.branch.as_str(), plan.target.as_str(), plan.ahead),
            ("worker", "main", 1)
        );
        assert_eq!(key(&plan.repo), key(&main));

        let missing = preflight(&main, &worker, Some("release"), false, None).unwrap_err();
        assert!(missing.contains("release, does not exist"), "{missing}");
        let itself = preflight(&main, &worker, Some("worker"), false, None).unwrap_err();
        assert!(itself.contains("on worker itself"), "{itself}");

        sh(&repo.worker, &["switch", "-q", "--detach"]);
        let detached = preflight(&main, &worker, None, false, None).unwrap_err();
        assert!(detached.contains("detached HEAD"), "{detached}");
    }

    #[test]
    fn after_landing_the_worktree_and_branch_go_unless_they_hold_more() {
        let repo = repo("cleanup");
        write(&repo.worker, "b.rs", "bee\n");
        commit(&repo.worker, "Add b");
        assert_eq!(
            deliver_now(&request(&repo), &fake(Ok(approve()))).outcome,
            Kind::Landed
        );
        let worker = repo.worker.to_string_lossy().into_owned();

        // Work that never landed keeps the worktree.
        write(&repo.worker, "late.txt", "late\n");
        let kept = cleanup(&worker, "worker", "main", &repo.task, 1);
        assert!(!kept.worktree_removed && !kept.branch_deleted, "{kept:?}");
        assert!(kept.kept[0].contains("never delivered"), "{kept:?}");

        // A commit that never landed keeps the branch.
        commit(&repo.worker, "Late");
        let kept = cleanup(&worker, "worker", "main", &repo.task, 1);
        assert!(kept.worktree_removed && !kept.branch_deleted, "{kept:?}");
        assert!(kept.kept[0].contains("not on main"), "{kept:?}");
        assert!(!repo.worker.exists());

        // All of it landed: both go.
        let other = repo.root.join("other");
        sh(
            &repo.main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "other",
                &other.to_string_lossy(),
                "main",
            ],
        );
        let gone = cleanup(&other.to_string_lossy(), "other", "main", &repo.task, 1);
        assert!(
            gone.worktree_removed && gone.branch_deleted && gone.kept.is_empty(),
            "{gone:?}"
        );
        assert!(sh(&repo.main, &["branch", "--list", "other"]).is_empty());

        // The main checkout is never removed.
        let main = cleanup(&repo.main.to_string_lossy(), "main", "main", &repo.task, 1);
        assert!(
            !main.worktree_removed && main.kept[0].contains("main checkout"),
            "{main:?}"
        );
        assert!(repo.main.join("a.txt").exists());
    }

    #[test]
    fn a_folder_git_let_go_of_is_swept_after_landing_and_a_dirty_one_stays() {
        let repo = repo("leftovers");
        let folder = repo.root.join("repo-worktrees").join("gone");
        fs::create_dir_all(folder.parent().unwrap()).unwrap();
        sh(
            &repo.main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "gone",
                &folder.to_string_lossy(),
                "main",
            ],
        );
        // Git gave up half way: no `.git`, files left, and an untouched
        // neighbour outside the folder.
        write(&folder, "node_modules/p/index.js", "x");
        write(&repo.root, "outside/keep.txt", "mine");
        fs::remove_file(folder.join(".git")).unwrap();

        let done = cleanup(&folder.to_string_lossy(), "gone", "main", &repo.task, 1);
        assert!(
            done.worktree_removed && done.branch_deleted && done.kept.is_empty(),
            "{done:?}"
        );
        assert!(!folder.exists());
        assert!(sh(&repo.main, &["worktree", "list"]).lines().count() == 2);
        assert_eq!(read(&repo.root.join("outside"), "keep.txt"), "mine");

        // Unmerged work keeps its folder and says so.
        let dirty = repo.root.join("repo-worktrees").join("dirty");
        sh(
            &repo.main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "dirty",
                &dirty.to_string_lossy(),
                "main",
            ],
        );
        write(&dirty, "wip.txt", "unsaved");
        let kept = cleanup(&dirty.to_string_lossy(), "dirty", "main", &repo.task, 1);
        assert!(!kept.worktree_removed && !kept.branch_deleted, "{kept:?}");
        assert!(kept.kept[0].contains("never delivered"), "{kept:?}");
        assert_eq!(read(&dirty, "wip.txt"), "unsaved");
    }

    #[test]
    fn what_a_task_owns_covers_its_paths_folders_and_globs() {
        let owns = |patterns: &[&str]| patterns.iter().map(|p| p.to_string()).collect::<Vec<_>>();
        assert!(owned("src/lib/a.ts", &owns(&["src/lib/a.ts"]), ""));
        assert!(owned("src/lib/a.ts", &owns(&["src/lib"]), ""));
        assert!(owned("src/lib/a.ts", &owns(&["./src/lib/"]), ""));
        assert!(owned("src/lib/a.ts", &owns(&["src/*"]), ""));
        assert!(owned("src/lib/deep/a.ts", &owns(&["src/**/*.ts"]), ""));
        assert!(owned("src/a.ts", &owns(&["src/**/*.ts"]), ""));
        assert!(owned(
            "src/lib/a.test.ts",
            &owns(&["src/lib/*.test.ts"]),
            ""
        ));
        assert!(owned("src/lib/a.ts", &owns(&["src\\lib"]), ""));
        assert!(!owned("src/lib/a.ts", &owns(&["src/lib/b.ts", "docs"]), ""));
        assert!(!owned("src/library/a.ts", &owns(&["src/lib"]), ""));
        assert!(!owned("src/lib/a.tsx", &owns(&["src/lib/*.ts"]), ""));
        // Relative to the workspace, which may sit below the repository root.
        assert!(owned("app/src/a.ts", &owns(&["src"]), "app/"));
        assert!(!owned("src/a.ts", &owns(&["src"]), "app/"));
        assert!(owned("app/anything", &owns(&["."]), "app/"));
    }

    #[test]
    fn each_shell_is_handed_the_line_its_own_way() {
        let shell = |id: &str, program: &str| ShellInfo {
            id: id.into(),
            label: id.into(),
            program: program.into(),
            args: vec![],
        };

        let cmd = invocation(
            &shell("cmd", r"C:\Windows\System32\cmd.exe"),
            "pnpm test && echo ok",
        );
        assert!(cmd.args.is_empty());
        assert_eq!(
            cmd.raw.as_deref(),
            Some("/d /s /c \"pnpm test && echo ok\"")
        );

        let pwsh = invocation(
            &shell("pwsh", r"C:\Program Files\PowerShell\7\pwsh.exe"),
            "pnpm test",
        );
        assert_eq!(
            &pwsh.args[..3],
            ["-NoLogo", "-NonInteractive", "-EncodedCommand"]
        );
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&pwsh.args[3])
            .unwrap();
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let script = String::from_utf16(&units).unwrap();
        assert!(script.starts_with("pnpm test\n"), "{script}");
        assert!(script.contains("exit $LASTEXITCODE"));

        let bash = invocation(
            &shell("git-bash", r"C:\Program Files\Git\bin\bash.exe"),
            "make check",
        );
        assert_eq!(bash.args, ["-l", "-c", "make check"]);
        assert!(bash.env.contains(&("CHERE_INVOKING", "1")));

        let zsh = invocation(&shell("login", "/bin/zsh"), "make check");
        assert_eq!(zsh.args, ["-l", "-c", "make check"]);
        assert_eq!(
            invocation(&shell("fish", "/usr/bin/fish"), "x").args,
            ["-l", "-c", "x"]
        );
        assert_eq!(
            invocation(&shell("nu", "/usr/bin/nu"), "x").args,
            ["-l", "-c", "x"]
        );
        assert_eq!(
            invocation(&shell("wsl", r"C:\Windows\System32\wsl.exe"), "x").args,
            ["-e", "sh", "-lc", "x"]
        );

        let unknown = invocation(&shell("elvish", "/usr/bin/elvish"), "x");
        if cfg!(windows) {
            assert!(
                unknown.raw.is_some(),
                "an unknown shell on Windows falls back to cmd"
            );
        } else {
            assert_eq!(unknown.args, ["-c", "x"]);
        }
    }

    #[test]
    fn the_verdict_is_read_from_claude_s_answer() {
        let structured = r#"{"type":"result","is_error":false,"result":"done","structured_output":{"approve":false,"blocking":[{"file":"a.ts","line":3,"problem":"off by one"}],"notes":""}}"#;
        let verdict = verdict_of(structured).unwrap();
        assert!(!verdict.approve);
        assert_eq!(verdict.blocking[0].line, Some(3));

        let in_result = r#"{"type":"result","is_error":false,"result":"```json\n{\"approve\":true,\"blocking\":[],\"notes\":\"ok\"}\n```"}"#;
        assert!(verdict_of(in_result).unwrap().passes());

        let failed = r#"{"type":"result","is_error":true,"result":"Credit balance is too low"}"#;
        assert!(verdict_of(failed)
            .unwrap_err()
            .contains("Credit balance is too low"));
        assert!(
            verdict_of(r#"{"type":"result","is_error":false,"result":"I approve."}"#)
                .unwrap_err()
                .contains("without a verdict")
        );
        assert!(verdict_of("not json").is_err());

        // Approving with a blocking item still blocks.
        let mixed: Verdict = serde_json::from_str(
            r#"{"approve":true,"blocking":[{"file":"a.ts","problem":"crashes"}],"notes":""}"#,
        )
        .unwrap();
        assert!(!mixed.passes());
    }

    #[test]
    fn the_reviewer_reads_and_does_nothing_else() {
        let args = review_args("sonnet");
        let after = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .map(|at| args[at + 1].as_str())
        };
        assert_eq!(args[0], "-p");
        assert_eq!(after("--model"), Some("sonnet"));
        assert_eq!(after("--tools"), Some("Read,Grep,Glob"));
        assert_eq!(after("--permission-mode"), Some("dontAsk"));
        assert_eq!(after("--setting-sources"), Some("user"));
        assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
        assert!(!args.iter().any(|arg| arg == "--mcp-config"));
        let schema: Value = serde_json::from_str(after("--json-schema").unwrap()).unwrap();
        assert_eq!(
            schema["required"],
            serde_json::json!(["approve", "blocking", "notes"])
        );
    }

    fn env_of(command: &Command, name: &str) -> Option<Option<std::ffi::OsString>> {
        command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new(name))
            .map(|(_, value)| value.map(|v| v.to_os_string()))
    }

    #[test]
    fn the_reviewer_runs_on_the_config_dir_of_the_workspace_account() {
        let profile = PathBuf::from("/home/me/.hangar/accounts/work");
        let launch = crate::accounts::Launch {
            id: "work".into(),
            dir: Some(profile.clone()),
            issues: Vec::new(),
        };
        let env = crate::accounts::env_for(Some("claude"), launch.dir.as_deref());
        let command = review_command(Path::new("claude"), Path::new("."), "sonnet", &env);
        assert_eq!(
            env_of(&command, "CLAUDE_CONFIG_DIR"),
            Some(Some(profile.into_os_string()))
        );
        // The Keychain entry is the profile's own, not main's.
        assert_eq!(
            env_of(&command, "CLAUDE_SECURESTORAGE_CONFIG_DIR"),
            Some(None)
        );
    }

    #[test]
    fn the_default_account_leaves_the_reviewer_environment_as_it_was() {
        let launch = crate::accounts::for_launch(Some("claude"), None).unwrap();
        assert_eq!(launch.dir, None);
        let env = crate::accounts::env_for(Some("claude"), launch.dir.as_deref());
        assert!(env.is_empty());
        let command = review_command(Path::new("claude"), Path::new("."), "sonnet", &env);
        assert_eq!(env_of(&command, "CLAUDE_CONFIG_DIR"), None);
        // Only the scrubbed variables are touched, all removed.
        assert!(command
            .get_envs()
            .all(|(name, value)| value.is_none() && SCRUBBED.iter().any(|s| *s == name)));
    }

    #[test]
    fn a_usage_limit_is_named_with_its_account() {
        let out = r#"{"type":"result","is_error":true,"result":"You've hit your weekly limit · resets Nov 3, 1pm"}"#;
        let said = usage_limit(out).expect("a limit");
        assert!(said.contains("weekly limit · resets Nov 3, 1pm"), "{said}");
        let message = limit_message("Work", &said);
        assert!(message.contains("\"Work\""));
        assert!(message.contains("usage limit"));
        assert!(message.contains("Nothing is wrong with the delivery"));
        assert_eq!(usage_limit("the review crashed"), None);
    }

    #[test]
    fn a_refusal_is_read_for_its_files_and_its_reason() {
        let said = "Updating 1111111..2222222\nerror: Your local changes to the following files would be overwritten by merge:\n\ta.txt\n\tsrc/b.ts\nPlease commit your changes or stash them before you merge.\nAborting\n";
        assert_eq!(
            overwritten(said),
            vec!["a.txt".to_string(), "src/b.ts".to_string()]
        );
        assert_eq!(
            problem(said, ""),
            "Your local changes to the following files would be overwritten by merge:"
        );
        assert_eq!(
            problem("", "fatal: not a git repository"),
            "not a git repository"
        );
    }

    #[test]
    fn a_long_output_is_kept_by_its_end() {
        let output: String = (1..=500).map(|n| format!("line {n}\n")).collect();
        let kept = tail(&output);
        assert!(kept.ends_with("line 500"));
        assert!(!kept.contains("line 440\n"));
        assert_eq!(kept.lines().count(), TAIL_LINES);
    }
}
