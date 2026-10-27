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

/// Where temporary worktrees go: a folder of the system's own, canonical so
/// that it reads the way git will print it back.
fn scratch_root() -> PathBuf {
    let root = std::env::temp_dir().join("hangar-deliveries");
    let _ = std::fs::create_dir_all(&root);
    dunce::canonicalize(&root).unwrap_or(root)
}

/// A temporary worktree, removed when it goes out of scope — whatever path
/// leaves: an outcome, an error, a timeout, a panic.
struct Scratch {
    /// Any folder of the repository, to run git from.
    repo: PathBuf,
    path: PathBuf,
}

impl Scratch {
    fn add(repo: &Path, base: &str, task: &str) -> Result<Scratch, String> {
        let tag: String = task
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(8)
            .collect();
        let unique = uuid::Uuid::new_v4().simple().to_string();
        let name = format!(
            "{SCRATCH_PREFIX}{}-{tag}-{}",
            std::process::id(),
            &unique[..8]
        );
        // Guarded before it exists, so a worktree half made is removed too.
        let scratch = Scratch {
            repo: repo.to_path_buf(),
            path: scratch_root().join(name),
        };
        let path = scratch.path.to_string_lossy().into_owned();
        git(repo, &["worktree", "add", "--detach", &path, base])
            .map_err(|err| format!("could not make a worktree to test the delivery in: {err}"))?;
        Ok(scratch)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        remove_scratch(&self.repo, &self.path);
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

/// Temporary worktrees a delivery left behind: the app closed in the middle
/// of one, or crashed. Only this repository's, only ones this process made or
/// whose process is gone — another Hangar on the same repository may be
/// testing in its own right now. The caller holds the repository's turn.
fn sweep(repo: &Path) {
    let Ok(listed) = worktrees(repo) else {
        return;
    };
    let root = format!("{}/", key(&scratch_root().to_string_lossy()));
    let table = crate::procs::table();
    for (path, _, _) in listed {
        if !key(&path).starts_with(&root) {
            continue;
        }
        let Some(rest) = Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(SCRATCH_PREFIX))
        else {
            continue;
        };
        let owner: Option<u32> = rest.split('-').next().and_then(|pid| pid.parse().ok());
        let stale = match (owner, &table) {
            (Some(pid), _) if pid == std::process::id() => true,
            (Some(pid), Some(table)) => !table.iter().any(|proc| proc.pid == pid),
            _ => false,
        };
        if stale {
            remove_scratch(repo, Path::new(&path));
        }
    }
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

fn save_tested(repo: &Path, task: &str, base: &str, merge: &str) {
    let _ = git(repo, &["update-ref", &tested_ref(task, "base"), base]);
    let _ = git(repo, &["update-ref", &tested_ref(task, "merge"), merge]);
}

/// The merge a delivery was tested and reviewed as, with the head of its
/// branch it was tested on.
fn tested(repo: &Path, task: &str) -> Option<(String, String)> {
    let base = rev(repo, &tested_ref(task, "base")).ok()?;
    let merge = rev(repo, &tested_ref(task, "merge")).ok()?;
    Some((base, merge))
}

fn forget_tested(repo: &Path, task: &str) {
    for part in ["base", "merge"] {
        let _ = git_run(repo, &["update-ref", "-d", &tested_ref(task, part)], false);
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

fn check_command(invocation: &Invocation, dir: &Path, port_base: Option<u32>) -> Command {
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
        check_command(&invocation, dir, request.port_base),
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
pub trait Reviewer {
    fn review(&self, dir: &Path, model: &str, prompt: &str) -> Result<Verdict, String>;
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

impl Reviewer for Claude {
    fn review(&self, dir: &Path, model: &str, prompt: &str) -> Result<Verdict, String> {
        let program = claude_program().ok_or(NO_CLAUDE)?;
        let mut command = Command::new(&program);
        command.args(review_args(model)).current_dir(dir);
        scrub(&mut command);
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
    /// Lands once it passes; otherwise it waits, tested, for the person.
    pub land: bool,
    /// Lands the merge tested earlier as it is, when the branch it lands on
    /// has not moved since; otherwise tests it again first.
    #[serde(default)]
    pub reuse: bool,
    #[serde(default)]
    pub port_base: Option<u32>,
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
    sweep(&worktree);

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
    if reuse {
        if let Some((tested_base, merge)) = tested(worktree, task) {
            if tested_base == base {
                stage.set("landing");
                return land_step(
                    request,
                    worktree,
                    &base,
                    &merge,
                    Outcome::new(Kind::Landed, String::new()),
                );
            }
        }
    }
    let tip = match rev(worktree, &format!("refs/heads/{branch}")) {
        Ok(sha) => sha,
        Err(err) => return done(Outcome::error(err)),
    };

    // 2. Merged onto the branch as it is now, away from everyone's checkout.
    let scratch = match Scratch::add(worktree, &base, task) {
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

    // 3. The workspace's check, on the merged result.
    if let Some(line) = request
        .check_command
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
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
                "The check `{line}` {how} on {branch} merged onto {target}. It runs in a fresh checkout of that \
merge, without the files git ignores (dependencies, build output) apart from .env-style files, so it has to \
install what it needs itself. The end of its output:\n```\n{}\n```\nFix it on your branch, run the check yourself, \
commit, then call delivery_submit again.",
                check.tail
            );
            outcome.check = Some(check);
            return done(outcome);
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
    let verdict = match reviewer.review(&dir, &request.reviewer_model, &prompt) {
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
    // 6. The temporary worktree goes before anything lands.
    drop(scratch);

    // 5. Landed, or kept tested for the person.
    if !request.land {
        save_tested(worktree, task, &base, &merge);
        let mut outcome = passed;
        outcome.outcome = Kind::Ready;
        outcome.detail = format!(
            "{branch} passed {}its review. Automatic landing is off in this workspace, so it waits for the person to \
land it on {target}. There is nothing more for you to do: stop here.",
            if request.check_command.is_some() {
                "its check and "
            } else {
                ""
            }
        );
        return done(outcome);
    }
    stage.set("landing");
    land_step(request, worktree, &base, &merge, passed)
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

fn land_step(
    request: &Delivery,
    repo: &Path,
    base: &str,
    merge: &str,
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
            save_tested(repo, task, base, merge);
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
}

/// What a delivery from `worktree` would be, or why it cannot be one — in
/// words for the worker. `landing` asks about a delivery already tested,
/// which only needs the branches.
fn preflight(
    workspace: &str,
    worktree: &str,
    target: Option<&str>,
    landing: bool,
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
    Ok(Plan {
        repo: repo.to_string_lossy().into_owned(),
        queue,
        branch,
        target,
        ahead,
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
    let Ok(common) = common_dir(dir) else {
        report
            .kept
            .push(format!("{worktree} is not a worktree any more"));
        return report;
    };
    // Run from the main checkout, not from the folder being removed.
    let repo = match worktrees(dir).ok().and_then(|list| list.into_iter().next()) {
        Some((main, _, _)) => PathBuf::from(main),
        None => common,
    };
    forget_tested(&repo, task);
    let listed = worktrees(&repo).unwrap_or_default();
    let linked = listed
        .iter()
        .skip(1)
        .any(|(path, _, _)| key(path) == key(worktree));
    if linked {
        for attempt in 0..tries.max(1) {
            match git_run(&repo, &["worktree", "remove", worktree], false) {
                Ok(finished) if finished.succeeded() => {
                    report.worktree_removed = true;
                    break;
                }
                Ok(finished) => {
                    let said = problem(&finished.err, &finished.out);
                    if said.contains("modified or untracked files") || said.contains("locked") {
                        report.kept.push(format!(
                            "the worktree {worktree} has changes that were never delivered ({said}): kept"
                        ));
                        break;
                    }
                    if attempt + 1 == tries.max(1) {
                        report.kept.push(format!(
                            "the worktree {worktree} could not be removed: {said}"
                        ));
                    } else {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                Err(err) => {
                    report.kept.push(format!(
                        "the worktree {worktree} could not be removed: {err}"
                    ));
                    break;
                }
            }
        }
        crate::git::forget_shared_roots();
    } else if listed.iter().any(|(path, _, _)| key(path) == key(worktree)) {
        report
            .kept
            .push(format!("{worktree} is the main checkout: kept"));
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

#[tauri::command]
pub async fn delivery_preflight(
    workspace: String,
    worktree: String,
    target: Option<String>,
    landing: Option<bool>,
) -> Result<Plan, String> {
    crate::git::off_thread(move || {
        preflight(
            &workspace,
            &worktree,
            target.as_deref(),
            landing.unwrap_or(false),
        )
    })
    .await
}

/// Takes as long as the delivery does — the check and the review are minutes —
/// on a thread of its own, never the window's.
#[tauri::command]
pub async fn delivery_run(delivery: Delivery) -> Outcome {
    tauri::async_runtime::spawn_blocking(move || {
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

#[tauri::command]
pub async fn delivery_cleanup(
    worktree: String,
    branch: String,
    target: String,
    task_id: String,
) -> Cleanup {
    tauri::async_runtime::spawn_blocking(move || cleanup(&worktree, &branch, &target, &task_id, 10))
        .await
        .unwrap_or(Cleanup {
            worktree_removed: false,
            branch_deleted: false,
            kept: vec!["the cleanup stopped unexpectedly".to_string()],
        })
}

#[tauri::command]
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
                title: "Add b.txt".into(),
                description: "Add b.txt holding one line.".into(),
                owns: vec!["b.txt".into()],
            },
            summary: Some("Added b.txt.".into()),
            check_command: None,
            shell: None,
            reviewer_model: "sonnet".into(),
            land: true,
            reuse: false,
            port_base: None,
        }
    }

    fn head(dir: &Path, name: &str) -> String {
        sh(dir, &["rev-parse", name])
    }

    /// Nothing left of a temporary worktree: the repository lists its main
    /// checkout and the worker's, and nothing in the scratch folder.
    fn assert_no_scratch(repo: &Repo) {
        let listed = worktrees(&repo.main).unwrap();
        let root = key(&scratch_root().to_string_lossy());
        assert!(
            listed
                .iter()
                .all(|(path, _, _)| !key(path).starts_with(&root)),
            "a temporary worktree is left: {listed:?}"
        );
        assert_eq!(listed.len(), 2, "{listed:?}");
        let tag: String = repo
            .task
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(8)
            .collect();
        let mine = format!("{SCRATCH_PREFIX}{}-{tag}-", std::process::id());
        let leftovers: Vec<_> = fs::read_dir(scratch_root())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(&mine))
            .collect();
        assert!(
            leftovers.is_empty(),
            "left in the scratch folder: {leftovers:?}"
        );
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
        fn review(&self, dir: &Path, model: &str, prompt: &str) -> Result<Verdict, String> {
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
        fn review(&self, _: &Path, _: &str, _: &str) -> Result<Verdict, String> {
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

    #[test]
    fn a_branch_ahead_of_the_target_lands_as_a_fast_forward() {
        let repo = repo("ff");
        write(&repo.worker, "b.txt", "bee\n");
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
            read(&repo.main, "b.txt"),
            "bee\n",
            "the person's checkout moved with it"
        );
        assert_eq!(
            outcome.landed_in.as_deref().map(key),
            Some(key(&repo.main.to_string_lossy()))
        );
        let check = outcome.check.clone().expect("the check ran");
        assert!(check.passed && check.tail.contains("checked"), "{check:?}");
        assert_eq!(outcome.changed, vec!["b.txt".to_string()]);
        assert!(outcome.outside_owns.is_empty());
        let prompt = reviewer.prompts.borrow()[0].clone();
        assert!(prompt.contains("Add b.txt holding one line."));
        assert!(prompt.contains("+bee"), "the diff is in the prompt");
        assert!(prompt.contains("The check `echo checked` already passed"));
        assert!(tested(&repo.main, &repo.task).is_none());
        assert_no_scratch(&repo);
    }

    #[test]
    fn the_check_gets_the_local_files_the_main_checkout_has_and_nothing_replaced() {
        let repo = repo("local-files");
        write(&repo.main, ".gitignore", ".env\n");
        write(&repo.main, "app.local", "tracked\n");
        commit(&repo.main, "Ignore .env, track app.local");
        sh(&repo.worker, &["merge", "-q", "main"]);
        write(&repo.worker, "b.txt", "bee\n");
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
        write(&repo.main, "c.txt", "sea\n");
        commit(&repo.main, "Add c");
        write(&repo.worker, "b.txt", "bee\n");
        commit(&repo.worker, "Add b");

        let outcome = deliver_now(&request(&repo), &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        let parents = sh(&repo.main, &["rev-list", "--parents", "-n", "1", "main"]);
        assert_eq!(parents.split(' ').count(), 3, "a merge commit: {parents}");
        assert_eq!(read(&repo.main, "b.txt"), "bee\n");
        assert_eq!(read(&repo.main, "c.txt"), "sea\n");
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
        write(&repo.worker, "b.txt", "bee\n");
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
        write(&repo.worker, "b.txt", "bee\n");
        commit(&repo.worker, "Add b");
        let mut request = request(&repo);
        // A child of the shell, which the shell's own end would not reach.
        request.check_command = Some(if cfg!(windows) {
            format!("{} -n 60 127.0.0.1 >nul", ping())
        } else {
            "sleep 60; echo late".into()
        });
        let started = Instant::now();

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
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{:?}",
            started.elapsed()
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
        write(&repo.worker, "b.txt", "bee\n");
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
        let (base, merge) = tested(&repo.main, &repo.task).expect("kept tested");
        assert_eq!(base, before);
        assert_no_scratch(&repo);

        // The person sets their edit aside themselves; the tested merge lands
        // as it was reviewed, without a second review.
        write(&repo.main, "a.txt", "one\ntwo\nthree\n");
        request.reuse = true;
        let outcome = deliver_now(&request, &Never);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(head(&repo.main, "main"), merge);
        assert_eq!(read(&repo.main, "a.txt"), "one\ntwo (worker)\nthree\n");
        assert!(tested(&repo.main, &repo.task).is_none());
        assert_no_scratch(&repo);
    }

    #[test]
    fn local_changes_elsewhere_ride_along_a_landing() {
        let repo = repo("bystander");
        write(&repo.worker, "b.txt", "bee\n");
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
        write(&repo.worker, "b.txt", "bee\n");
        commit(&repo.worker, "Add b");
        let main = repo.main.clone();
        let reviewer = fake_with(vec![Ok(approve())], move |call| {
            if call == 0 {
                write(&main, "c.txt", "sea\n");
                commit(&main, "The person commits meanwhile");
            }
        });

        let outcome = deliver_now(&request(&repo), &reviewer);

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(reviewer.calls.get(), 2, "reviewed again on the new head");
        assert_eq!(read(&repo.main, "b.txt"), "bee\n");
        assert_eq!(read(&repo.main, "c.txt"), "sea\n");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_delivery_waiting_for_the_person_is_tested_again_if_the_target_moved() {
        let repo = repo("ready");
        write(&repo.worker, "b.txt", "bee\n");
        commit(&repo.worker, "Add b");
        let before = head(&repo.main, "main");
        let mut request = request(&repo);
        request.land = false;

        let outcome = deliver_now(&request, &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Ready, "{}", outcome.detail);
        assert_eq!(head(&repo.main, "main"), before, "nothing landed");
        assert!(tested(&repo.main, &repo.task).is_some());
        assert_no_scratch(&repo);

        write(&repo.main, "c.txt", "sea\n");
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
        assert_eq!(read(&repo.main, "b.txt"), "bee\n");
        assert_eq!(read(&repo.main, "c.txt"), "sea\n");
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_target_checked_out_nowhere_lands_by_moving_its_ref() {
        let repo = repo("ref");
        sh(&repo.main, &["switch", "-q", "-c", "elsewhere"]);
        write(&repo.worker, "b.txt", "bee\n");
        commit(&repo.worker, "Add b");

        let outcome = deliver_now(&request(&repo), &fake(Ok(approve())));

        assert_eq!(outcome.outcome, Kind::Landed, "{}", outcome.detail);
        assert_eq!(outcome.landed_in, None);
        assert_eq!(head(&repo.main, "main"), head(&repo.main, "worker"));
        assert_eq!(
            read(&repo.main, "b.txt"),
            "",
            "the checkout on another branch is left alone"
        );
        assert_no_scratch(&repo);
    }

    #[test]
    fn blocking_items_come_back_with_where_and_what() {
        let repo = repo("changes");
        write(&repo.worker, "b.txt", "bee\n");
        write(&repo.worker, "src/app.ts", "export const app = 3;\n");
        commit(&repo.worker, "Add b, and change app");
        let reviewer = fake(Ok(Verdict {
            approve: false,
            blocking: vec![Blocking {
                file: "src/app.ts".into(),
                line: Some(1),
                problem: "Changes app, which the task did not ask for.".into(),
            }],
            notes: "b.txt itself is fine.".into(),
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
        assert!(outcome.detail.contains("Notes: b.txt itself is fine."));
        assert_eq!(outcome.outside_owns, vec!["src/app.ts".to_string()]);
        let prompt = reviewer.prompts.borrow()[0].clone();
        assert!(
            prompt.contains("Changed outside the files the task owns: src/app.ts."),
            "{prompt}"
        );
        assert!(read(&repo.main, "b.txt").is_empty());
        assert_no_scratch(&repo);
    }

    #[test]
    fn a_review_that_cannot_run_merges_nothing() {
        let repo = repo("no-review");
        write(&repo.worker, "b.txt", "bee\n");
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

        let empty = preflight(&main, &worker, None, false).unwrap_err();
        assert!(
            empty.contains("worker has no commit that main does not already have"),
            "{empty}"
        );

        write(&repo.worker, "b.txt", "bee\n");
        commit(&repo.worker, "Add b");
        let plan = preflight(&main, &worker, None, false).unwrap();
        assert_eq!(
            (plan.branch.as_str(), plan.target.as_str(), plan.ahead),
            ("worker", "main", 1)
        );
        assert_eq!(key(&plan.repo), key(&main));

        let missing = preflight(&main, &worker, Some("release"), false).unwrap_err();
        assert!(missing.contains("release, does not exist"), "{missing}");
        let itself = preflight(&main, &worker, Some("worker"), false).unwrap_err();
        assert!(itself.contains("on worker itself"), "{itself}");

        sh(&repo.worker, &["switch", "-q", "--detach"]);
        let detached = preflight(&main, &worker, None, false).unwrap_err();
        assert!(detached.contains("detached HEAD"), "{detached}");
    }

    #[test]
    fn after_landing_the_worktree_and_branch_go_unless_they_hold_more() {
        let repo = repo("cleanup");
        write(&repo.worker, "b.txt", "bee\n");
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
