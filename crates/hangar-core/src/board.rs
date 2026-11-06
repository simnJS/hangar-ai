use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::delivery::owns::{expand_braces, glob_matches, owned_segments};
use crate::store::{write_atomic, Access};

/// The label of a task that waits for the person (src/lib/orchestration.ts).
pub const NEEDS_YOU_LABEL: &str = "needs-you";

pub const COLUMNS: [&str; 4] = ["todo", "doing", "review", "done"];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    /// Free-form author label, e.g. "claude:pane-2" or "user".
    pub author: String,
    pub text: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// One of COLUMNS.
    pub column: String,
    #[serde(default)]
    pub priority: u8,
    /// Agent currently holding the task; None means free to pick up.
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Ids this task waits on. Used by `next_task` to skip work that is not ready.
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub order: f64,
    /// When the task last entered `doing`; None anywhere else. Absent from
    /// boards written before it existed, which then show no elapsed time
    /// rather than a made-up one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doing_since: Option<u64>,
    /// Paths and globs this task's worker may change, relative to the
    /// workspace (orchestration). Two tasks in `doing` never share one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owns: Vec<String>,
    /// How many workers were started on this task (orchestration).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub attempts: u32,
    /// Waiting on something outside the board (see `Blocked`). A blocked task
    /// stays in `todo`, held by nobody. Absent from boards written before it
    /// existed, which read as not blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<Blocked>,
    /// How far along the task is, as the agent working on it estimates it
    /// (`report_progress`). Cleared when the task goes back to `todo`; set to
    /// 100 when it reaches `done`. Absent from boards written before it
    /// existed, which read as no estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
}

/// An agent's own estimate of how far along its task is: reported, never
/// computed, so the board shows what the agent believes, with its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// 0 to 100.
    pub percent: u8,
    /// What was reached, in a few words. May be empty.
    #[serde(default)]
    pub note: String,
    /// When it was reported, in ms since the epoch.
    #[serde(default)]
    pub at: u64,
    /// Who reported it: the pane's name for a pane, else what the caller gave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// The longest progress note kept, in characters; the rest is cut.
pub const MAX_PROGRESS_NOTE: usize = 200;

/// A progress report, as `report_progress` takes it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProgressReport {
    /// Clamped to 0..=100; a fraction is rounded.
    pub percent: f64,
    #[serde(default)]
    pub note: Option<String>,
    /// Who reports. The API replaces it with the pane a token was issued to.
    #[serde(default)]
    pub by: Option<String>,
    /// Hangar itself or a workspace's orchestrator: may report on a task it
    /// does not hold. Never read from a request body (the API sets it).
    #[serde(skip)]
    pub any_holder: bool,
}

/// A task that cannot go on because of something outside the board: a package
/// release, a person's decision, a date. A state, not a column: the board keeps
/// its four, and whoever reads `COLUMNS` sees nothing new.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    /// What it waits for. Never empty.
    pub reason: String,
    /// Who blocked it: an agent's name, or "user".
    #[serde(default)]
    pub by: String,
    /// When, in ms since the epoch.
    #[serde(default)]
    pub since: u64,
    /// The day (ISO, YYYY-MM-DD) to look again. Once it has passed (the day
    /// after) the task unblocks itself (`sweep_rechecks`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recheck: Option<String>,
    /// Only a person lifts it: no agent can, whatever it says.
    #[serde(default)]
    pub human_only: bool,
    /// When a `human_only` task's recheck day passed and the reminder went out;
    /// it stays blocked, and is reminded once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reminded: Option<u64>,
}

/// The longest reason kept, in characters.
pub const MAX_BLOCK_REASON: usize = 500;
/// The longest note kept with an unblock.
const MAX_UNBLOCK_NOTE: usize = 500;
/// Who signs the comments Hangar writes itself on a task.
pub const HANGAR_AUTHOR: &str = "hangar";

/// What blocks a task, as `block_task` takes it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockRequest {
    pub reason: String,
    /// ISO day, YYYY-MM-DD, today or later.
    #[serde(default)]
    pub recheck: Option<String>,
    #[serde(default)]
    pub human_only: bool,
    /// Who is blocking; "user" when empty.
    #[serde(default)]
    pub by: Option<String>,
    /// The request comes from the window's own UI, not from an agent over MCP.
    /// Only the window sets it (mcp.rs strips it from what an agent sends).
    #[serde(default)]
    pub person: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnblockRequest {
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub by: Option<String>,
    /// See `BlockRequest::person`.
    #[serde(default)]
    pub person: bool,
}

/// Today's date as the board reads it: the local day, ISO.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl Task {
    /// Moves the task, keeping `doing_since` true to the column it lands in.
    /// Staying in `doing` keeps the original stamp.
    fn enter_column(&mut self, column: String, stamp: u64) {
        if column != "doing" {
            self.doing_since = None;
        } else if self.column != "doing" {
            self.doing_since = Some(stamp);
        }
        if column == "todo" {
            // Back to the start: whatever was estimated no longer holds.
            self.progress = None;
        } else if column == "done" {
            if let Some(progress) = self.progress.as_mut() {
                progress.percent = 100;
                progress.at = stamp;
            }
        }
        self.column = column;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Board {
    #[serde(default)]
    pub tasks: Vec<Task>,
}

impl Board {
    fn find_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Largest order value in a column, so new cards land at the bottom.
    fn next_order(&self, column: &str) -> f64 {
        self.tasks
            .iter()
            .filter(|t| t.column == column)
            .map(|t| t.order)
            .fold(0.0, f64::max)
            + 1.0
    }
}

/// Boards live beside the code they describe, one per workspace directory.
fn board_path(workspace_cwd: &str) -> PathBuf {
    Path::new(workspace_cwd).join(".hangar").join("board.json")
}

/// Where boards lived before the directory was renamed.
///
/// Read-only, and never deleted: a board found here is read as-is until the
/// next write, which lands in `.hangar/` and adopts it. Same care as
/// `adopt_previous_state()` in store.rs — a rename that moves the file would
/// strand every board belonging to an older build still on the machine.
fn legacy_board_path(workspace_cwd: &str) -> PathBuf {
    Path::new(workspace_cwd).join(".iabench").join("board.json")
}

/// Every worktree of a repository works off the board of its main repository.
///
/// A linked worktree is its own directory, so the rule above — one board per
/// workspace directory — would give each of them a separate, empty board, and
/// agents spread across worktrees would be coordinating with nobody. Resolving
/// here, in the only two functions that touch the file, is what puts them all
/// on the one board; it covers the MCP path too, where `HANGAR_WORKSPACE` may
/// well point at a worktree.
///
/// Anything that is not a linked worktree keeps the path it was given.
fn shared_workspace(workspace_cwd: &str) -> String {
    crate::git::shared_root(workspace_cwd).unwrap_or_else(|| workspace_cwd.to_string())
}

/// Why the board on disk could not be used. Neither is ever an empty board:
/// one that is written back would wipe every task, comment and history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unreadable {
    /// The file is there but could not be read (permissions; on Windows, an
    /// antivirus or an indexer holding it). It may well be fine: it is left
    /// as it is and nothing is written over it.
    Io { path: PathBuf, reason: String },
    /// It was read, but it is not a board (a hand edit, a write cut short,
    /// a field an older build does not know the type of).
    Corrupt { path: PathBuf, reason: String },
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Io { path, reason } => write!(
                f,
                "the board {} cannot be read ({reason}); nothing was written over it",
                path.display()
            ),
            Unreadable::Corrupt { path, reason } => write!(
                f,
                "the board {} is not a valid board ({reason})",
                path.display()
            ),
        }
    }
}

/// How many times a read that failed is tried, and how long apart: a file
/// held for a moment (an antivirus, a backup tool) reads fine on the next go.
const READ_ATTEMPTS: u32 = 3;
const READ_RETRY: std::time::Duration = std::time::Duration::from_millis(50);

/// The file the board of `workspace_cwd` is read from, if there is one.
fn locate(workspace_cwd: &str) -> Option<PathBuf> {
    let shared = shared_workspace(workspace_cwd);
    let mut candidates = vec![board_path(&shared), legacy_board_path(&shared)];
    // A worktree opened as a workspace before boards were shared has a board
    // of its own. Reading it when the main repository has none is the same
    // adoption the `.iabench` rename got: the tasks stay visible, and the
    // first write moves them to the shared location for good.
    if shared != workspace_cwd {
        candidates.push(board_path(workspace_cwd));
        candidates.push(legacy_board_path(workspace_cwd));
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// The board on disk, parsed afresh: what the tests check a write against.
#[cfg(test)]
fn read_board(workspace_cwd: &str) -> Result<Board, Unreadable> {
    match locate(workspace_cwd) {
        Some(path) => read_file(path),
        None => Ok(Board::default()),
    }
}

fn read_file(path: PathBuf) -> Result<Board, Unreadable> {
    let mut attempt = 1;
    let bytes = loop {
        match fs::read(&path) {
            Ok(bytes) => break bytes,
            // Gone between the look and the read: there is no board.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Board::default()),
            Err(_) if attempt < READ_ATTEMPTS => {
                attempt += 1;
                std::thread::sleep(READ_RETRY);
            }
            Err(err) => {
                return Err(Unreadable::Io {
                    path,
                    reason: err.to_string(),
                })
            }
        }
    };
    serde_json::from_slice(&bytes).map_err(|err| Unreadable::Corrupt {
        path,
        reason: err.to_string(),
    })
}

/// The board to change, or why there is none to change.
///
/// A board that will not parse is moved aside (`store::quarantine`, as
/// `state.json` and the memory are) and the operation fails, saying where it
/// went: the next one starts on an empty board, and the old one is there to
/// be repaired by hand. One that cannot be moved, or cannot be read at all,
/// stays where it is and every change is refused.
fn set_aside(err: Unreadable) -> String {
    let Unreadable::Corrupt { path, .. } = &err else {
        return err.to_string();
    };
    match crate::store::quarantine(path) {
        Some(kept) => format!(
            "{err}; it was kept as {} and the board starts empty: try again",
            kept.display()
        ),
        None => format!("{err}; it could not be moved aside, so nothing is written over it"),
    }
}

fn write_board(workspace_cwd: &str, board: &Board) -> Result<(), String> {
    let shared = shared_workspace(workspace_cwd);
    let path = board_path(&shared);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(board).map_err(|e| e.to_string())?;
    // A unique temporary file, flushed before the rename: a power cut cannot
    // leave an empty board, nor can two Hangar processes share one temp.
    write_atomic(&path, body.as_bytes(), Access::AsBefore)?;
    keep_out_of_git(&shared);
    Ok(())
}

/// The board is the workspace's own working state, not the project's: once
/// per workspace and per run of the app, it is kept out of `git status`
/// (git::exclude_locally), off the writing thread, which holds the board.
fn keep_out_of_git(root: &str) {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let first = SEEN
        .get_or_init(Mutex::default)
        .lock()
        .map(|mut seen| seen.insert(root.to_string()))
        .unwrap_or(false);
    if first {
        let root = PathBuf::from(root);
        std::thread::spawn(move || {
            crate::git::exclude_locally(&root, ".hangar", ".hangar/board.json")
        });
    }
}

/// What a board file looked like when it was last parsed or written: its
/// size, its times and, on Unix, the inode an atomic replace gives it. Any
/// change to the file — a write by this process, by another one, a hand edit,
/// a `chmod` — changes one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    inode: (u64, u64),
    #[cfg(unix)]
    changed: (i64, i64),
}

impl Stamp {
    pub(crate) fn of(path: &Path) -> Option<Stamp> {
        let meta = fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Stamp {
            len: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            inode: (meta.dev(), meta.ino()),
            #[cfg(unix)]
            changed: (meta.ctime(), meta.ctime_nsec()),
        })
    }
}

/// A board as parsed from `path` when the file looked like `stamp`.
struct Parsed {
    path: PathBuf,
    stamp: Stamp,
    board: Arc<Board>,
}

pub(crate) fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Serializes every mutation per workspace.
///
/// This is the whole reason the app owns the board instead of letting each
/// agent edit the file: without a single writer, two agents doing
/// read-modify-write would both claim the same task and duplicate the work.
///
/// The writer is one per board file (a repository and its worktrees share
/// one), so the agents of one workspace never wait on another's `fsync`. A
/// read takes no writer: the file is only ever replaced whole
/// (`write_atomic`), so it sees the board before a change or after it, and
/// never waits behind one being made durable.
///
/// The board last parsed for each workspace is kept with the look of its file
/// (`Stamp`), and parsed again only when the file changed: the agents list
/// the board far more often than they change it, and the orchestrator reads
/// every board every few seconds.
#[derive(Default)]
pub struct BoardStore {
    writers: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    parsed: Mutex<HashMap<String, Parsed>>,
}

impl BoardStore {
    /// Reading is also when a recheck day that has passed is acted on
    /// (`sweep_rechecks`): the orchestrator's engine reads every board every
    /// few seconds, so no timer of its own is needed.
    ///
    /// A board that cannot be read shows as an empty one, and is left as it
    /// is on disk: only a change (`update`) sets it aside, and says so.
    pub fn read(&self, cwd: &str) -> Board {
        let mut board = match self.parsed(cwd) {
            Ok(board) => Arc::unwrap_or_clone(board),
            Err(err) => {
                eprintln!("[hangar] {err}");
                return Board::default();
            }
        };
        if sweep_rechecks(&mut board, &today()) {
            // Written under the writer, from the latest board: `update`
            // sweeps it the same way before anything else.
            if let Ok(swept) = self.update(cwd, |latest| Ok(latest.clone())) {
                return swept;
            }
        }
        board
    }

    /// Applies `mutate` under the workspace's lock and persists the result.
    pub fn update<T>(
        &self,
        cwd: &str,
        mutate: impl FnOnce(&mut Board) -> Result<T, String>,
    ) -> Result<T, String> {
        let shared = shared_workspace(cwd);
        let writer = locked(&self.writers).entry(shared).or_default().clone();
        let _writing = locked(&writer);
        let mut board = Arc::unwrap_or_clone(self.parsed(cwd).map_err(set_aside)?);
        sweep_rechecks(&mut board, &today());
        let outcome = mutate(&mut board)?;
        write_board(cwd, &board)?;
        self.remember(cwd, board);
        Ok(outcome)
    }

    /// The board of `cwd`, parsed again only when its file changed.
    fn parsed(&self, cwd: &str) -> Result<Arc<Board>, Unreadable> {
        let Some(path) = locate(cwd) else {
            locked(&self.parsed).remove(cwd);
            return Ok(Arc::default());
        };
        // Taken before the read: a file replaced in between is parsed new
        // under the old stamp, which the next look finds changed.
        let stamp = Stamp::of(&path);
        if let Some(stamp) = stamp {
            let known = locked(&self.parsed);
            if let Some(parsed) = known.get(cwd) {
                if parsed.path == path && parsed.stamp == stamp {
                    return Ok(parsed.board.clone());
                }
            }
        }
        let board = Arc::new(read_file(path.clone())?);
        if let Some(stamp) = stamp {
            locked(&self.parsed).insert(
                cwd.to_string(),
                Parsed {
                    path,
                    stamp,
                    board: board.clone(),
                },
            );
        }
        Ok(board)
    }

    /// Keeps the board just written, as the file now looks, so the next read
    /// does not parse back what this one serialized.
    fn remember(&self, cwd: &str, board: Board) {
        let path = board_path(&shared_workspace(cwd));
        let mut known = locked(&self.parsed);
        match Stamp::of(&path) {
            Some(stamp) => {
                known.insert(
                    cwd.to_string(),
                    Parsed {
                        path,
                        stamp,
                        board: Arc::new(board),
                    },
                );
            }
            None => {
                known.remove(cwd);
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct NewTask {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub column: Option<String>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub owns: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<String>,
    pub column: Option<String>,
    pub priority: Option<u8>,
    /// Assign to someone. `serde` cannot distinguish an absent field from an
    /// explicit null here, so releasing uses its own flag below.
    pub assignee: Option<String>,
    /// Clears the assignee. Takes precedence over `assignee`.
    pub release: Option<bool>,
    pub labels: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub order: Option<f64>,
    pub owns: Option<Vec<String>>,
    /// Blocks the task on something outside the board (`block_task`).
    #[serde(default)]
    pub block: Option<BlockRequest>,
    /// Lifts the block (`unblock_task`).
    #[serde(default)]
    pub unblock: Option<UnblockRequest>,
}

pub fn create_task(board: &mut Board, input: NewTask) -> Result<Task, String> {
    let column = input.column.unwrap_or_else(|| "todo".to_string());
    if !COLUMNS.contains(&column.as_str()) {
        return Err(format!("unknown column '{column}'"));
    }
    let stamp = now_ms();
    let task = Task {
        id: new_id(),
        title: input.title,
        description: input.description,
        order: board.next_order(&column),
        doing_since: (column == "doing").then_some(stamp),
        column,
        priority: input.priority.unwrap_or(1),
        assignee: None,
        labels: input.labels,
        comments: vec![],
        depends_on: input.depends_on,
        created_at: stamp,
        updated_at: stamp,
        owns: input.owns,
        attempts: 0,
        blocked: None,
        progress: None,
    };
    board.tasks.push(task.clone());
    Ok(task)
}

pub fn patch_task(board: &mut Board, id: &str, mut patch: TaskPatch) -> Result<Task, String> {
    if let Some(column) = &patch.column {
        if !COLUMNS.contains(&column.as_str()) {
            return Err(format!("unknown column '{column}'"));
        }
    }
    if let Some(request) = patch.unblock.take() {
        unblock_task(board, id, request)?;
    }
    let block = patch.block.take();
    let task = board.find_mut(id).ok_or("task not found")?;
    if let (Some(blocked), Some(column)) = (&task.blocked, &patch.column) {
        if column != "todo" {
            return Err(format!(
                "\"{}\" is blocked ({}) and stays in todo: it cannot go to {column}. Unblock it first \
                 (board_unblock_task) once what it waits for is there.",
                task.title, blocked.reason
            ));
        }
    }
    let stamp = now_ms();

    if let Some(v) = patch.title {
        task.title = v;
    }
    if let Some(v) = patch.description {
        task.description = v;
    }
    if let Some(v) = patch.column {
        task.enter_column(v, stamp);
    }
    if let Some(v) = patch.priority {
        task.priority = v;
    }
    if let Some(v) = patch.assignee {
        task.assignee = Some(v);
    }
    if patch.release == Some(true) {
        task.assignee = None;
    }
    if let Some(v) = patch.labels {
        task.labels = v;
    }
    if let Some(v) = patch.owns {
        task.owns = v;
    }
    if let Some(v) = patch.depends_on {
        task.depends_on = v;
    }
    if let Some(v) = patch.order {
        task.order = v;
    }
    task.updated_at = stamp;
    if let Some(request) = block {
        return block_task(board, id, request, &today());
    }
    Ok(task.clone())
}

pub fn delete_task(board: &mut Board, id: &str) -> Result<(), String> {
    let before = board.tasks.len();
    board.tasks.retain(|t| t.id != id);
    if board.tasks.len() == before {
        return Err("task not found".into());
    }
    Ok(())
}

/// Takes ownership of a task. Fails when someone else already holds it, which
/// is what stops two agents working the same item — or when it would enter
/// `doing` beside a task that owns some of the same files.
pub fn claim_task(board: &mut Board, id: &str, agent: &str) -> Result<Task, String> {
    let task = board
        .tasks
        .iter()
        .find(|t| t.id == id)
        .ok_or("task not found")?;
    if let Some(owner) = task.assignee.as_ref().filter(|owner| *owner != agent) {
        return Err(format!("already claimed by {owner}"));
    }
    if let Some(blocked) = &task.blocked {
        return Err(format!(
            "cannot claim \"{}\": it is blocked: {}. {}",
            task.title,
            blocked.reason,
            blocked_advice(blocked)
        ));
    }
    // Only a task entering `doing` can collide with the ones already there.
    if task.column == "todo" {
        if let Some(clash) = ownership_clash(board, task, false) {
            return Err(format!(
                "cannot claim \"{}\": {}",
                task.title,
                clash.message()
            ));
        }
    }

    let task = board.find_mut(id).ok_or("task not found")?;
    let stamp = now_ms();
    task.assignee = Some(agent.to_string());
    if task.column == "todo" {
        task.enter_column("doing".into(), stamp);
    }
    task.updated_at = stamp;
    Ok(task.clone())
}

/// Hands a todo task to the worker an orchestrator is starting on it
/// (lib/workerControl.ts), and counts the attempt. Everything that stands in
/// the way is refused here, under the board's lock, and each refusal says
/// what to do instead: an orchestrator reads it. How many attempts a task
/// gets is not among them: the count is the orchestrator's to read, and when
/// to stop is its call.
///
/// `shared_folder`: the workspace is not a git repository, so the worker
/// starts in its folder beside the others rather than in a worktree of its
/// own (`folder_clash`).
pub fn claim_for_worker(
    board: &mut Board,
    id: &str,
    worker: &str,
    shared_folder: bool,
) -> Result<Task, String> {
    let task = board.tasks.iter().find(|t| t.id == id).ok_or_else(|| {
        format!("No task {id} on this board. board_list_tasks lists them with their ids.")
    })?;
    let title = &task.title;
    match task.column.as_str() {
        "todo" => {}
        "doing" => {
            return Err(format!(
                "\"{title}\" is in doing already{}: someone is on it. Start a worker on a task in todo.",
                held_by(task)
            ))
        }
        "review" => {
            return Err(format!(
                "\"{title}\" is in review: its work waits to be checked, not to be done. Move it back to todo \
                 (board_update_task) first if it really needs more work."
            ))
        }
        _ => return Err(format!("\"{title}\" is done already.")),
    }
    if let Some(blocked) = &task.blocked {
        return Err(format!(
            "\"{title}\" is blocked: {}. Do not start a worker on it. {}",
            blocked.reason,
            blocked_advice(blocked)
        ));
    }
    if let Some(owner) = task.assignee.as_ref().filter(|owner| *owner != worker) {
        return Err(format!(
            "\"{title}\" is in todo but held by {owner}. If nobody is on it, release it \
             (board_update_task with release=true) and try again."
        ));
    }
    let waiting: Vec<String> = task
        .depends_on
        .iter()
        .filter_map(|dep| match board.tasks.iter().find(|t| &t.id == dep) {
            Some(t) if t.column == "done" => None,
            Some(t) => Some(format!(
                "\"{}\" ({}, in {}{})",
                t.title,
                t.id,
                t.column,
                t.blocked
                    .as_ref()
                    .map(|b| format!(", blocked: {}", b.reason))
                    .unwrap_or_default()
            )),
            None => Some(format!("{dep} (no longer on the board)")),
        })
        .collect();
    if !waiting.is_empty() {
        return Err(format!(
            "\"{title}\" depends on tasks that are not done: {}. Get those done first; drop one from \
             depends_on (board_update_task) only if it no longer matters.",
            waiting.join(", ")
        ));
    }
    let clash = if shared_folder {
        folder_clash(board, task).or_else(|| ownership_clash(board, task, true))
    } else {
        ownership_clash(board, task, true)
    };
    if let Some(clash) = clash {
        return Err(format!("Cannot start \"{title}\": {}", clash.message()));
    }

    let task = board.find_mut(id).ok_or("task not found")?;
    let stamp = now_ms();
    task.assignee = Some(worker.to_string());
    task.enter_column("doing".into(), stamp);
    task.attempts += 1;
    task.updated_at = stamp;
    Ok(task.clone())
}

/// Gives a worker's task back (lib/workerControl.ts): to todo, free, when it
/// was still being worked on; left where it is once it waits for review or is
/// done. `undo` takes the attempt back as well, for a start that never got
/// going. A task somebody else holds by now is not the worker's to give.
pub fn release_worker(
    board: &mut Board,
    id: &str,
    worker: &str,
    undo: bool,
) -> Result<Task, String> {
    let task = board.find_mut(id).ok_or("task not found")?;
    if task.assignee.as_deref() != Some(worker) {
        return Ok(task.clone());
    }
    let stamp = now_ms();
    if undo {
        task.attempts = task.attempts.saturating_sub(1);
    }
    if task.column == "todo" || task.column == "doing" {
        task.assignee = None;
        task.enter_column("todo".into(), stamp);
    }
    task.updated_at = stamp;
    Ok(task.clone())
}

fn held_by(task: &Task) -> String {
    task.assignee
        .as_ref()
        .map(|owner| format!(", held by {owner}"))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Owned files
//
// A task's `owns` lists the paths and globs its worker may change. Two tasks in
// `doing` never share one, which is what lets workers run side by side without
// their branches colliding when they land. The rule lives here once, for
// every claim: an agent's `board_claim_task` and a worker an orchestrator
// starts alike.
// ---------------------------------------------------------------------------

/// Why a task may not enter `doing` beside another one.
#[derive(Debug)]
pub enum Clash<'a> {
    /// Some path is owned by both: each pair of patterns that meet, the
    /// entering task's first.
    Shared {
        with: &'a Task,
        paths: Vec<(String, String)>,
    },
    /// Neither task says which files it owns.
    Unscoped { with: &'a Task },
    /// Workers share one folder, outside git, and one of the two tasks does
    /// not say which files it owns: the entering one when `mine`.
    Folder { with: &'a Task, mine: bool },
}

impl Clash<'_> {
    pub fn message(&self) -> String {
        match self {
            Clash::Shared { with, paths } => {
                let listed: Vec<String> = paths
                    .iter()
                    .map(|(mine, theirs)| {
                        if mine == theirs {
                            mine.clone()
                        } else {
                            format!("{mine} (this task) and {theirs} (that one)")
                        }
                    })
                    .collect();
                format!(
                    "it owns files that \"{}\" ({}, in doing{}) owns too: {}. Two tasks in doing never \
                     share a file. Wait until that one leaves doing, or narrow the owns of one of them \
                     (board_update_task) so they share no path.",
                    with.title,
                    with.id,
                    held_by(with),
                    listed.join(", ")
                )
            }
            Clash::Unscoped { with } => format!(
                "neither it nor \"{}\" ({}, in doing{}) says which files it owns, so both could change \
                 the same ones, and two such tasks never run at once in one repository. Set owns on this \
                 task (board_update_task: paths or globs relative to the repository root; a folder covers \
                 everything under it) and try again.",
                with.title,
                with.id,
                held_by(with)
            ),
            Clash::Folder { with, mine: true } => format!(
                "it does not say which files it owns, and \"{}\" ({}, in doing{}) is being worked on \
                 in the same folder: the workspace is not a git repository, so its workers share one folder \
                 and change it as they go. Set owns on this task (board_update_task: paths or globs \
                 relative to the workspace folder; a folder covers everything under it) and try again, or \
                 start it once that one leaves doing.",
                with.title,
                with.id,
                held_by(with)
            ),
            Clash::Folder { with, mine: false } => format!(
                "\"{}\" ({}, in doing{}) does not say which files it owns, and the workspace is not a \
                 git repository, so its workers share one folder and change it as they go. Start this one \
                 once that one leaves doing, or set owns on that one (board_update_task) and try again.",
                with.title,
                with.id,
                held_by(with)
            ),
        }
    }
}

/// The first task in `doing` that `task` may not run beside.
///
/// `strict` is the orchestrator's rule on top of the overlap: two tasks that
/// say nothing about their files never run at once either. A plain claim
/// leaves that to the agents, as it always has — boards that never use
/// `owns` see no change at all.
pub fn ownership_clash<'a>(board: &'a Board, task: &Task, strict: bool) -> Option<Clash<'a>> {
    let mine = scoped(&task.owns);
    board
        .tasks
        .iter()
        .filter(|other| other.id != task.id && other.column == "doing")
        .find_map(|other| match (mine, scoped(&other.owns)) {
            (true, true) => {
                let paths = owns_overlap(&task.owns, &other.owns);
                (!paths.is_empty()).then_some(Clash::Shared { with: other, paths })
            }
            (false, false) if strict => Some(Clash::Unscoped { with: other }),
            _ => None,
        })
}

/// Whether `owns` names any file at all.
fn scoped(owns: &[String]) -> bool {
    owns.iter().any(|pattern| owned_segments(pattern).is_some())
}

/// The first task in `doing` that `task` may not run beside when workers share
/// the workspace's folder: outside git, nothing keeps one worker's changes
/// apart from another's until a merge, so both tasks have to say which files
/// they own. The overlap itself is `ownership_clash`'s.
fn folder_clash<'a>(board: &'a Board, task: &Task) -> Option<Clash<'a>> {
    let mine = scoped(&task.owns);
    board
        .tasks
        .iter()
        .filter(|other| other.id != task.id && other.column == "doing")
        .find_map(|other| {
            (!mine || !scoped(&other.owns)).then_some(Clash::Folder {
                with: other,
                mine: !mine,
            })
        })
}

/// Each pair of patterns, one from each list, that some path matches both of.
pub fn owns_overlap(mine: &[String], theirs: &[String]) -> Vec<(String, String)> {
    let read = |pattern: &String| -> Vec<Vec<String>> {
        expand_braces(pattern)
            .iter()
            .filter_map(|each| owned_segments(each))
            .collect()
    };
    let theirs: Vec<(&String, Vec<Vec<String>>)> = theirs.iter().map(|p| (p, read(p))).collect();
    let mut pairs = Vec::new();
    for pattern in mine {
        let ours = read(pattern);
        for (other, their) in &theirs {
            let meet = ours
                .iter()
                .any(|a| their.iter().any(|b| sequences_meet(a, b)));
            if meet {
                pairs.push((pattern.trim().to_string(), other.trim().to_string()));
            }
        }
    }
    pairs
}

/// Whether some path is matched by both segment lists, `**` standing for any
/// number of segments, none included.
fn sequences_meet(a: &[String], b: &[String]) -> bool {
    match (a.split_first(), b.split_first()) {
        (None, None) => true,
        (Some((head, rest)), _) if head == "**" => {
            sequences_meet(rest, b) || (!b.is_empty() && sequences_meet(a, &b[1..]))
        }
        (_, Some((head, rest))) if head == "**" => {
            sequences_meet(a, rest) || (!a.is_empty() && sequences_meet(&a[1..], b))
        }
        (Some((x, xs)), Some((y, ys))) => segments_meet(x, y) && sequences_meet(xs, ys),
        _ => false,
    }
}

const WILDCARDS: [char; 3] = ['*', '?', '['];

/// Whether some name is matched by both segments.
fn segments_meet(a: &str, b: &str) -> bool {
    match (a.contains(WILDCARDS), b.contains(WILDCARDS)) {
        (false, false) => a == b,
        (true, false) => glob_matches(a, b),
        (false, true) => glob_matches(b, a),
        // Two wildcards meet unless their fixed ends disagree: `*.ts` and
        // `*.tsx` cannot name one file, `use*.ts` and `*Store.ts` can. A
        // generous reading, on the side of an overlap.
        (true, true) => {
            let (head_a, tail_a) = fixed_ends(a);
            let (head_b, tail_b) = fixed_ends(b);
            (head_a.starts_with(head_b) || head_b.starts_with(head_a))
                && (tail_a.ends_with(tail_b) || tail_b.ends_with(tail_a))
        }
    }
}

/// What a wildcard segment fixes before its first wildcard and after its last.
fn fixed_ends(segment: &str) -> (&str, &str) {
    let first = segment.find(WILDCARDS).unwrap_or(segment.len());
    let last = segment
        .rfind(['*', '?', ']'])
        .map_or(segment.len(), |at| at + 1)
        .max(first);
    (&segment[..first], &segment[last..])
}

fn blocked_advice(blocked: &Blocked) -> String {
    if blocked.human_only {
        "Only a person lifts this block; pick another task.".to_string()
    } else {
        "Pick another task; board_unblock_task lifts it once what it waits for is there."
            .to_string()
    }
}

/// Puts a task on hold for something outside the board.
///
/// It lands in `todo` with nobody on it, so it holds no worker and no file
/// (`ownership_clash` only looks at `doing`). A task that was being worked on
/// gives the attempt back: it was stopped by the wait, not by the worker's
/// failure, and the count an orchestrator reads should not say otherwise.
/// `attempts` counts workers started (`claim_for_worker`); an agent's plain
/// claim does not count, so a task an agent held and blocks has at worst one
/// attempt more than it used, never fewer.
pub fn block_task(
    board: &mut Board,
    id: &str,
    request: BlockRequest,
    today: &str,
) -> Result<Task, String> {
    let task = board.find_mut(id).ok_or("task not found")?;
    match task.column.as_str() {
        "done" => {
            return Err(format!(
                "\"{}\" is done: there is nothing left to wait for. Blocking is for work that cannot go on.",
                task.title
            ))
        }
        "review" => {
            return Err(format!(
                "\"{}\" is in review: its work is finished and waits to be checked, not blocked. \
                 Move it back to todo (board_update_task) first if it needs more work.",
                task.title
            ))
        }
        _ => {}
    }
    let reason = request.reason.trim().to_string();
    if reason.is_empty() {
        return Err("A block needs a reason: say what the task waits for.".into());
    }
    if reason.chars().count() > MAX_BLOCK_REASON {
        return Err(format!(
            "The reason is too long ({} characters, at most {MAX_BLOCK_REASON}): say it shorter and put the detail in a comment.",
            reason.chars().count()
        ));
    }
    let recheck = match request
        .recheck
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        None => None,
        Some(day) => {
            let parsed = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")
                .map_err(|_| format!("recheck '{day}' is not a day: write it YYYY-MM-DD."))?;
            let normal = parsed.format("%Y-%m-%d").to_string();
            if normal.as_str() < today {
                return Err(format!(
                    "recheck {normal} is in the past (today is {today}): pick today or a later day."
                ));
            }
            Some(normal)
        }
    };
    if let Some(held) = &task.blocked {
        if held.human_only && !request.person {
            return Err(format!(
                "\"{}\" is already blocked for a person ({}): an agent cannot change that block.",
                task.title, held.reason
            ));
        }
    }
    let stamp = now_ms();
    if task.column == "doing" && task.assignee.is_some() {
        task.attempts = task.attempts.saturating_sub(1);
    }
    task.assignee = None;
    task.enter_column("todo".into(), stamp);
    let by = request
        .by
        .map(|by| by.trim().to_string())
        .filter(|by| !by.is_empty())
        .unwrap_or_else(|| "user".into());
    task.blocked = Some(Blocked {
        reason,
        by,
        since: stamp,
        recheck,
        human_only: request.human_only,
        reminded: None,
    });
    task.updated_at = stamp;
    Ok(task.clone())
}

/// Lifts a block. A `human_only` one is the person's: only the window's own
/// call (`person`) may. Over MCP nothing tells a person from an agent (the
/// proxy only knows its pane), so for any MCP caller it is refused.
pub fn unblock_task(board: &mut Board, id: &str, request: UnblockRequest) -> Result<Task, String> {
    let task = board.find_mut(id).ok_or("task not found")?;
    let Some(blocked) = task.blocked.clone() else {
        return Err(format!("\"{}\" is not blocked.", task.title));
    };
    if blocked.human_only && !request.person {
        return Err(format!(
            "\"{}\" is blocked for a person to lift ({}). A person must unblock it, from the board; \
             Hangar cannot tell a person from an agent over MCP, so no agent can. Tell the orchestrator, \
             or comment on the task, and carry on with another one.",
            task.title, blocked.reason
        ));
    }
    let by = request
        .by
        .map(|by| by.trim().to_string())
        .filter(|by| !by.is_empty())
        .unwrap_or_else(|| "user".into());
    let note = request
        .note
        .map(|note| {
            note.trim()
                .chars()
                .take(MAX_UNBLOCK_NOTE)
                .collect::<String>()
        })
        .filter(|note| !note.is_empty());
    let text = match note {
        Some(note) => format!(
            "Unblocked by {by}: {note} (was blocked: {})",
            blocked.reason
        ),
        None => format!("Unblocked by {by} (was blocked: {})", blocked.reason),
    };
    task.blocked = None;
    push_comment(task, HANGAR_AUTHOR, text);
    Ok(task.clone())
}

/// Acts on the recheck days that have passed (the day after the one named). A
/// task unblocks by itself, with `recheck passed` noted; a `human_only` one
/// stays blocked and is reminded, once. True when something changed.
pub fn sweep_rechecks(board: &mut Board, today: &str) -> bool {
    let stamp = now_ms();
    let mut changed = false;
    for task in &mut board.tasks {
        let Some(blocked) = &task.blocked else {
            continue;
        };
        let Some(day) = blocked.recheck.clone().filter(|day| day.as_str() < today) else {
            continue;
        };
        let reason = blocked.reason.clone();
        if blocked.human_only {
            if blocked.reminded.is_some() {
                continue;
            }
            if let Some(blocked) = task.blocked.as_mut() {
                blocked.reminded = Some(stamp);
            }
            push_comment(
                task,
                HANGAR_AUTHOR,
                format!("recheck passed ({day}): still blocked until a person lifts it ({reason})"),
            );
        } else {
            task.blocked = None;
            push_comment(
                task,
                HANGAR_AUTHOR,
                format!("recheck passed ({day}; was blocked: {reason})"),
            );
        }
        changed = true;
    }
    changed
}

fn push_comment(task: &mut Task, author: &str, text: String) {
    let stamp = now_ms();
    task.comments.push(Comment {
        id: new_id(),
        author: author.to_string(),
        text,
        created_at: stamp,
    });
    task.updated_at = stamp;
}

pub fn add_comment(board: &mut Board, id: &str, author: &str, text: &str) -> Result<Task, String> {
    let task = board.find_mut(id).ok_or("task not found")?;
    task.comments.push(Comment {
        id: new_id(),
        author: author.to_string(),
        text: text.to_string(),
        created_at: now_ms(),
    });
    task.updated_at = now_ms();
    Ok(task.clone())
}

/// Records `report`'s estimate on task `id`. Only a task in `doing` takes
/// one, and only from whoever holds it, unless `report.any_holder`: each
/// refusal says why, for the agent that reads it.
pub fn report_progress(
    board: &mut Board,
    id: &str,
    report: ProgressReport,
) -> Result<Task, String> {
    let task = board.find_mut(id).ok_or_else(|| {
        format!("No task {id} on this board. board_list_tasks lists them with their ids.")
    })?;
    if task.column != "doing" {
        return Err(format!(
            "\"{}\" is in {}, not doing: progress is reported on a task being worked on.",
            task.title, task.column
        ));
    }
    let by = report
        .by
        .as_deref()
        .map(str::trim)
        .filter(|by| !by.is_empty())
        .map(str::to_string);
    if !report.any_holder {
        let holder = task.assignee.as_deref();
        if holder.is_none() || holder != by.as_deref() {
            return Err(format!(
                "\"{}\" is {}, not by {}: only whoever works on a task reports its progress.",
                task.title,
                holder.map_or("held by nobody".to_string(), |h| format!("held by {h}")),
                by.as_deref().unwrap_or("an unnamed caller"),
            ));
        }
    }
    let stamp = now_ms();
    task.progress = Some(Progress {
        percent: clamp_percent(report.percent),
        note: bounded_note(report.note.as_deref().unwrap_or("")),
        at: stamp,
        by,
    });
    task.updated_at = stamp;
    Ok(task.clone())
}

/// 0 to 100, rounded; not a number reads as 0.
fn clamp_percent(percent: f64) -> u8 {
    if percent.is_nan() {
        return 0;
    }
    percent.round().clamp(0.0, 100.0) as u8
}

/// Trimmed, on one line, and at most `MAX_PROGRESS_NOTE` characters.
fn bounded_note(note: &str) -> String {
    let line = note.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(MAX_PROGRESS_NOTE) {
        Some((end, _)) => line[..end].trim_end().to_string(),
        None => line,
    }
}

/// The coordination primitive: highest-priority unassigned task in `todo`
/// whose dependencies are all done, that is neither blocked nor waiting for
/// the person (needs-you). Returns None when nothing is actionable.
pub fn next_task(board: &Board) -> Option<Task> {
    let done: Vec<&str> = board
        .tasks
        .iter()
        .filter(|t| t.column == "done")
        .map(|t| t.id.as_str())
        .collect();

    board
        .tasks
        .iter()
        .filter(|t| t.column == "todo" && t.assignee.is_none())
        // Waiting on something outside the board, or on the person: not for
        // another agent to pick up and repeat the same check.
        .filter(|t| t.blocked.is_none() && !t.labels.iter().any(|l| l == NEEDS_YOU_LABEL))
        .filter(|t| t.depends_on.iter().all(|dep| done.contains(&dep.as_str())))
        .max_by(|a, b| {
            a.priority.cmp(&b.priority).then(
                b.order
                    .partial_cmp(&a.order)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        })
        .cloned()
}

#[cfg(test)]
pub(crate) mod bench;

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_TASK: &str = r#"{"tasks":[{"id":"a","title":"tache existante","column":"todo","created_at":1,"updated_at":1}]}"#;

    fn temp_workspace(tag: &str) -> PathBuf {
        crate::testdir::scratch(&format!("board-{tag}"))
    }

    fn seed(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn a_board_left_in_the_previous_directory_is_still_read() {
        let dir = temp_workspace("legacy-read");
        let cwd = dir.to_string_lossy().into_owned();
        seed(&legacy_board_path(&cwd), ONE_TASK);

        let board = read_board(&cwd).unwrap();

        assert_eq!(board.tasks.len(), 1);
        assert_eq!(board.tasks[0].title, "tache existante");
        fs::remove_dir_all(&dir).ok();
    }

    /// The adoption: the first write lands in the new directory with the tasks
    /// that were read from the old one, which stays where it is.
    #[test]
    fn writing_adopts_the_previous_board_without_moving_it() {
        let dir = temp_workspace("legacy-adopt");
        let cwd = dir.to_string_lossy().into_owned();
        let legacy = legacy_board_path(&cwd);
        seed(&legacy, ONE_TASK);

        let board = read_board(&cwd).unwrap();
        write_board(&cwd, &board).expect("write");

        let adopted = read_board(&cwd).unwrap();
        assert!(board_path(&cwd).is_file());
        assert_eq!(adopted.tasks.len(), 1);
        assert_eq!(adopted.tasks[0].title, "tache existante");
        assert!(legacy.is_file(), "the old board must be left in place");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_current_directory_wins_when_both_exist() {
        let dir = temp_workspace("both");
        let cwd = dir.to_string_lossy().into_owned();
        seed(&legacy_board_path(&cwd), ONE_TASK);
        seed(
            &board_path(&cwd),
            r#"{"tasks":[{"id":"b","title":"tache courante","column":"todo","created_at":2,"updated_at":2}]}"#,
        );

        let board = read_board(&cwd).unwrap();

        assert_eq!(board.tasks.len(), 1);
        assert_eq!(board.tasks[0].title, "tache courante");
        fs::remove_dir_all(&dir).ok();
    }

    fn todo_task(board: &mut Board) -> String {
        create_task(
            board,
            NewTask {
                title: "t".into(),
                description: String::new(),
                column: None,
                priority: None,
                labels: vec![],
                depends_on: vec![],
                owns: vec![],
            },
        )
        .unwrap()
        .id
    }

    fn move_to(board: &mut Board, id: &str, column: &str) -> Task {
        patch_task(
            board,
            id,
            TaskPatch {
                column: Some(column.into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn claiming_starts_the_doing_clock_and_leaving_doing_stops_it() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        assert_eq!(board.tasks[0].doing_since, None);

        let claimed = claim_task(&mut board, &id, "ava").unwrap();
        assert!(claimed.doing_since.is_some());

        assert_eq!(move_to(&mut board, &id, "review").doing_since, None);
        assert!(move_to(&mut board, &id, "doing").doing_since.is_some());
    }

    #[test]
    fn staying_in_doing_keeps_the_original_stamp() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        let since = claim_task(&mut board, &id, "ava").unwrap().doing_since;

        assert_eq!(move_to(&mut board, &id, "doing").doing_since, since);
        assert_eq!(
            claim_task(&mut board, &id, "ava").unwrap().doing_since,
            since
        );
    }

    #[test]
    fn a_task_written_before_doing_since_existed_still_loads() {
        let board: Board = serde_json::from_str(
            r#"{"tasks":[{"id":"a","title":"x","column":"doing","created_at":1,"updated_at":1}]}"#,
        )
        .unwrap();

        assert_eq!(board.tasks[0].doing_since, None);
        let written = serde_json::to_string(&board).unwrap();
        assert!(!written.contains("doing_since"));
    }

    fn owned(board: &mut Board, title: &str, owns: &[&str]) -> String {
        create_task(
            board,
            NewTask {
                title: title.into(),
                description: String::new(),
                column: None,
                priority: None,
                labels: vec![],
                depends_on: vec![],
                owns: owns.iter().map(|p| p.to_string()).collect(),
            },
        )
        .unwrap()
        .id
    }

    fn list(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(|p| p.to_string()).collect()
    }

    fn overlap(mine: &[&str], theirs: &[&str]) -> bool {
        !owns_overlap(&list(mine), &list(theirs)).is_empty()
    }

    #[test]
    fn owned_paths_overlap_when_some_file_is_in_both() {
        // A folder covers what is under it, written either way.
        assert!(overlap(&["src/lib/**"], &["src/lib/agents.ts"]));
        assert!(overlap(&["src/lib"], &["src/lib/agents.ts"]));
        assert!(overlap(&["./docs/"], &["docs/guide.md"]));
        assert!(overlap(&[".github"], &[".github/workflows/ci.yml"]));
        assert!(overlap(&["v1.2/"], &["v1.2/notes.md"]));
        assert!(overlap(&["**"], &["anything/at/all.rs"]));
        // Globs against names and against each other.
        assert!(overlap(&["**/*.test.ts"], &["src/lib/agents.test.ts"]));
        assert!(overlap(
            &["src/components/Orchestration*.tsx"],
            &["src/components/OrchestrationPanel.tsx"]
        ));
        assert!(overlap(&["src/use*.ts"], &["src/*Store.ts"]));
        assert!(overlap(&["src/[ab].ts"], &["src/b.ts"]));
        assert!(overlap(
            &["src/{lib,components}/x.ts"],
            &["src/components/x.ts"]
        ));
        // Windows spellings and case name the same file.
        assert!(overlap(&["SRC\\Lib\\"], &["src/lib/x.ts"]));
    }

    #[test]
    fn owned_paths_that_name_different_files_do_not_overlap() {
        assert!(!overlap(&["src/lib/a.ts"], &["src/lib/b.ts"]));
        assert!(!overlap(&["src/lib/**"], &["src/components/**"]));
        // A file is not a folder: nothing lives under it.
        assert!(!overlap(&["**/*.test.ts"], &["src/lib/agents.ts"]));
        assert!(!overlap(&["src/*.ts"], &["src/*.tsx"]));
        assert!(!overlap(
            &["src/components/Orchestration*.tsx"],
            &["src/components/NeedsYou.tsx"]
        ));
        assert!(!overlap(&["src/[ab].ts"], &["src/c.ts"]));
        assert!(!overlap(&["src/[!ab].ts"], &["src/a.ts"]));
        assert!(!overlap(
            &["src/{lib,components}/x.ts"],
            &["src/hooks/x.ts"]
        ));
        // Blank patterns own nothing.
        assert!(!overlap(&["", "  "], &["src/x.ts"]));
    }

    #[test]
    fn an_overlap_lists_each_pair_of_patterns_that_meet() {
        assert_eq!(
            owns_overlap(
                &list(&["src/lib/", "README.md"]),
                &list(&["src/lib/board.ts", "docs/**", "README.md"])
            ),
            vec![
                ("src/lib/".to_string(), "src/lib/board.ts".to_string()),
                ("README.md".to_string(), "README.md".to_string()),
            ]
        );
    }

    #[test]
    fn a_claim_into_doing_is_refused_beside_a_task_owning_the_same_files() {
        let mut board = Board::default();
        let first = owned(&mut board, "Board API", &["src-tauri/src/board.rs"]);
        claim_task(&mut board, &first, "ava").unwrap();
        let second = owned(
            &mut board,
            "Board view",
            &["src-tauri/src/**", "src/BoardView.tsx"],
        );

        let refused = claim_task(&mut board, &second, "leo").unwrap_err();
        assert!(refused.contains("\"Board API\""), "{refused}");
        assert!(refused.contains(&first), "{refused}");
        assert!(refused.contains("held by ava"), "{refused}");
        assert!(
            refused.contains("src-tauri/src/** (this task) and src-tauri/src/board.rs (that one)"),
            "{refused}"
        );
        let second_task = board.tasks.iter().find(|t| t.id == second).unwrap();
        assert_eq!(second_task.column, "todo");
        assert_eq!(second_task.assignee, None);

        // Disjoint files: both run.
        let third = owned(&mut board, "Docs", &["docs/**"]);
        assert_eq!(
            claim_task(&mut board, &third, "leo").unwrap().column,
            "doing"
        );
        // Once the first leaves doing, the second may go.
        move_to(&mut board, &first, "review");
        assert_eq!(
            claim_task(&mut board, &second, "mia").unwrap().column,
            "doing"
        );
    }

    /// Boards that never use `owns` keep claiming as they always did.
    #[test]
    fn a_plain_claim_never_minds_tasks_that_own_nothing() {
        let mut board = Board::default();
        let first = todo_task(&mut board);
        let second = todo_task(&mut board);
        let scoped = owned(&mut board, "Scoped", &["src/**"]);
        claim_task(&mut board, &first, "ava").unwrap();
        assert_eq!(
            claim_task(&mut board, &second, "leo").unwrap().column,
            "doing"
        );
        assert_eq!(
            claim_task(&mut board, &scoped, "mia").unwrap().column,
            "doing"
        );
        // A task already in doing is only being re-taken: nothing enters.
        assert!(claim_task(&mut board, &scoped, "mia").is_ok());
    }

    #[test]
    fn a_worker_claim_moves_the_task_to_doing_and_counts_the_attempt() {
        let mut board = Board::default();
        let id = owned(&mut board, "Login form", &["src/login/**"]);

        let claimed = claim_for_worker(&mut board, &id, "Leo", false).unwrap();
        assert_eq!(claimed.column, "doing");
        assert_eq!(claimed.assignee.as_deref(), Some("Leo"));
        assert_eq!(claimed.attempts, 1);
        assert!(claimed.doing_since.is_some());
    }

    #[test]
    fn a_worker_claim_says_why_a_task_cannot_be_started() {
        let mut board = Board::default();
        let done = owned(&mut board, "Done one", &["a/**"]);
        move_to(&mut board, &done, "done");
        let review = owned(&mut board, "Waiting one", &["b/**"]);
        move_to(&mut board, &review, "review");
        let doing = owned(&mut board, "Running one", &["c/**"]);
        claim_task(&mut board, &doing, "ava").unwrap();

        let refusal =
            |board: &mut Board, id: &str| claim_for_worker(board, id, "Leo", false).unwrap_err();
        assert!(refusal(&mut board, "nope").contains("board_list_tasks"));
        assert!(refusal(&mut board, &done).contains("done already"));
        assert!(refusal(&mut board, &review).contains("in review"));
        assert!(refusal(&mut board, &doing).contains("held by ava"));

        let held = owned(&mut board, "Held one", &["d/**"]);
        patch_task(
            &mut board,
            &held,
            TaskPatch {
                assignee: Some("max".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(refusal(&mut board, &held).contains("release=true"));

        let waiting = owned(&mut board, "Blocked one", &["e/**"]);
        patch_task(
            &mut board,
            &waiting,
            TaskPatch {
                depends_on: Some(vec![review.clone(), done.clone(), "gone".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let blocked = refusal(&mut board, &waiting);
        assert!(blocked.contains("\"Waiting one\""), "{blocked}");
        assert!(
            blocked.contains("gone (no longer on the board)"),
            "{blocked}"
        );
        assert!(!blocked.contains("Done one"), "{blocked}");

        let overlapping = owned(&mut board, "Overlapping one", &["c/x.rs"]);
        let clash = refusal(&mut board, &overlapping);
        assert!(clash.contains("\"Running one\""), "{clash}");
        assert!(
            clash.contains("c/x.rs (this task) and c/** (that one)"),
            "{clash}"
        );

        // Nothing refused moved, and no attempt was counted.
        for task in &board.tasks {
            if task.id != doing {
                assert_ne!(task.column, "doing", "{}", task.title);
            }
            assert_eq!(task.attempts, 0, "{}", task.title);
        }
    }

    #[test]
    fn a_worker_claim_counts_every_attempt_and_refuses_none_for_their_number() {
        let mut board = Board::default();
        let id = owned(&mut board, "Flaky", &["src/flaky.rs"]);
        for worker in ["Leo", "Mia", "Sam", "Ivy", "Zoe"] {
            claim_for_worker(&mut board, &id, worker, false).unwrap();
            release_worker(&mut board, &id, worker, false).unwrap();
        }
        assert_eq!(board.tasks[0].attempts, 5);

        // A sixth is started like the first: when to stop is the orchestrator's call.
        let claimed = claim_for_worker(&mut board, &id, "Ava", false).unwrap();
        assert_eq!((claimed.column.as_str(), claimed.attempts), ("doing", 6));
    }

    #[test]
    fn two_workers_on_tasks_that_own_nothing_never_run_at_once() {
        let mut board = Board::default();
        let first = todo_task(&mut board);
        let second = todo_task(&mut board);
        claim_for_worker(&mut board, &first, "Leo", false).unwrap();

        let refused = claim_for_worker(&mut board, &second, "Mia", false).unwrap_err();
        assert!(refused.contains("says which files it owns"), "{refused}");
        assert!(refused.contains("Set owns"), "{refused}");
        // Once it says what it owns, it runs beside the other one.
        patch_task(
            &mut board,
            &second,
            TaskPatch {
                owns: Some(vec!["docs/**".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(claim_for_worker(&mut board, &second, "Mia", false).is_ok());
    }

    #[test]
    fn workers_sharing_a_folder_without_git_all_say_what_they_own() {
        let mut board = Board::default();
        let unscoped = todo_task(&mut board);
        let scoped = owned(&mut board, "Scoped", &["docs/**"]);
        let other = owned(&mut board, "Other", &["src/**"]);
        let overlapping = owned(&mut board, "Overlapping", &["docs/guide.md"]);

        // Alone, a task that owns nothing runs.
        claim_for_worker(&mut board, &unscoped, "Leo", true).unwrap();
        // Beside it, nothing else does, however scoped.
        let refused = claim_for_worker(&mut board, &scoped, "Mia", true).unwrap_err();
        assert!(
            refused.contains("does not say which files it owns"),
            "{refused}"
        );
        assert!(refused.contains("not a git repository"), "{refused}");
        // In a repository, worktrees keep them apart until they land.
        claim_for_worker(&mut board, &scoped, "Mia", false).unwrap();
        release_worker(&mut board, &scoped, "Mia", true).unwrap();

        release_worker(&mut board, &unscoped, "Leo", true).unwrap();
        claim_for_worker(&mut board, &scoped, "Mia", true).unwrap();
        let refused = claim_for_worker(&mut board, &unscoped, "Leo", true).unwrap_err();
        assert!(refused.contains("Set owns on this task"), "{refused}");
        // Scoped and apart, they run side by side; sharing a file, they do not.
        claim_for_worker(&mut board, &other, "Sam", true).unwrap();
        let refused = claim_for_worker(&mut board, &overlapping, "Ivy", true).unwrap_err();
        assert!(refused.contains("owns too"), "{refused}");
    }

    #[test]
    fn releasing_a_worker_gives_back_only_what_is_still_its_own() {
        let mut board = Board::default();
        let id = owned(&mut board, "Login", &["src/login/**"]);

        // A start that never got going takes its attempt back.
        claim_for_worker(&mut board, &id, "Leo", false).unwrap();
        let undone = release_worker(&mut board, &id, "Leo", true).unwrap();
        assert_eq!(
            (undone.column.as_str(), undone.assignee, undone.attempts),
            ("todo", None, 0)
        );
        assert_eq!(undone.doing_since, None);

        // A stopped worker's attempt still counts.
        claim_for_worker(&mut board, &id, "Leo", false).unwrap();
        let stopped = release_worker(&mut board, &id, "Leo", false).unwrap();
        assert_eq!((stopped.column.as_str(), stopped.attempts), ("todo", 1));

        // Waiting for review: it stays where it is.
        claim_for_worker(&mut board, &id, "Mia", false).unwrap();
        move_to(&mut board, &id, "review");
        let kept = release_worker(&mut board, &id, "Mia", false).unwrap();
        assert_eq!(
            (kept.column.as_str(), kept.assignee.as_deref()),
            ("review", Some("Mia"))
        );

        // Somebody else's by now: not the worker's to give.
        move_to(&mut board, &id, "doing");
        patch_task(
            &mut board,
            &id,
            TaskPatch {
                assignee: Some("ava".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let theirs = release_worker(&mut board, &id, "Mia", false).unwrap();
        assert_eq!(
            (theirs.column.as_str(), theirs.assignee.as_deref()),
            ("doing", Some("ava"))
        );
    }

    // -- blocked ------------------------------------------------------------

    fn block(reason: &str) -> BlockRequest {
        BlockRequest {
            reason: reason.into(),
            by: Some("Eli".into()),
            ..Default::default()
        }
    }

    const TODAY: &str = "2026-11-02";

    #[test]
    fn an_old_board_reads_as_not_blocked_and_writes_nothing_new() {
        let board: Board = serde_json::from_str(ONE_TASK).unwrap();
        assert!(board.tasks[0].blocked.is_none());
        let again = serde_json::to_string(&board).unwrap();
        assert!(!again.contains("blocked"), "{again}");
    }

    #[test]
    fn a_blocked_task_round_trips_with_the_documented_names() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        block_task(
            &mut board,
            &id,
            BlockRequest {
                recheck: Some("2026-11-10".into()),
                human_only: true,
                ..block("waiting for the release")
            },
            TODAY,
        )
        .unwrap();
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains("\"humanOnly\":true"), "{json}");
        let back: Board = serde_json::from_str(&json).unwrap();
        let blocked = back.tasks[0].blocked.as_ref().unwrap();
        assert_eq!(blocked.reason, "waiting for the release");
        assert_eq!(blocked.by, "Eli");
        assert_eq!(blocked.recheck.as_deref(), Some("2026-11-10"));
    }

    #[test]
    fn next_task_skips_blocked_and_needs_you_tasks() {
        let mut board = Board::default();
        let blocked = todo_task(&mut board);
        let needs_you = todo_task(&mut board);
        let free = todo_task(&mut board);
        board.find_mut(&blocked).unwrap().priority = 9;
        board.find_mut(&needs_you).unwrap().priority = 8;
        board.find_mut(&needs_you).unwrap().labels = vec![NEEDS_YOU_LABEL.into()];
        block_task(&mut board, &blocked, block("npm release"), TODAY).unwrap();

        assert_eq!(next_task(&board).unwrap().id, free);
        unblock_task(&mut board, &blocked, UnblockRequest::default()).unwrap();
        assert_eq!(next_task(&board).unwrap().id, blocked);
    }

    #[test]
    fn claims_refuse_a_blocked_task_and_say_why() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        block_task(&mut board, &id, block("npm release"), TODAY).unwrap();

        let plain = claim_task(&mut board, &id, "ava").unwrap_err();
        assert!(plain.contains("npm release"), "{plain}");
        let worker = claim_for_worker(&mut board, &id, "Leo", false).unwrap_err();
        assert!(
            worker.contains("blocked") && worker.contains("npm release"),
            "{worker}"
        );
        let task = &board.tasks[0];
        assert_eq!((task.column.as_str(), task.attempts), ("todo", 0));
        assert!(task.assignee.is_none());
    }

    #[test]
    fn a_blocked_task_is_not_done_for_the_tasks_waiting_on_it() {
        let mut board = Board::default();
        let first = todo_task(&mut board);
        let second = todo_task(&mut board);
        board.find_mut(&second).unwrap().depends_on = vec![first.clone()];
        block_task(&mut board, &first, block("a decision"), TODAY).unwrap();

        assert!(next_task(&board).is_none());
        let refused = claim_for_worker(&mut board, &second, "Leo", false).unwrap_err();
        assert!(refused.contains("blocked: a decision"), "{refused}");
    }

    #[test]
    fn a_block_needs_a_reason_of_reasonable_length() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        for empty in ["", "   \n"] {
            let refused = block_task(&mut board, &id, block(empty), TODAY).unwrap_err();
            assert!(refused.contains("reason"), "{refused}");
        }
        let long = "x".repeat(MAX_BLOCK_REASON + 1);
        assert!(block_task(&mut board, &id, block(&long), TODAY).is_err());
        assert!(board.tasks[0].blocked.is_none());

        let kept = block_task(&mut board, &id, block("  trimmed  "), TODAY).unwrap();
        assert_eq!(kept.blocked.unwrap().reason, "trimmed");
    }

    #[test]
    fn blocking_a_task_in_doing_frees_its_files_and_keeps_the_attempt() {
        let mut board = Board::default();
        let mine = owned(&mut board, "mine", &["src/a/"]);
        let other = owned(&mut board, "other", &["src/a/"]);
        claim_for_worker(&mut board, &mine, "Leo", false).unwrap();
        assert!(claim_for_worker(&mut board, &other, "Mia", false).is_err());

        let blocked = block_task(&mut board, &mine, block("waiting"), TODAY).unwrap();
        assert_eq!(
            (
                blocked.column.as_str(),
                blocked.assignee.as_deref(),
                blocked.attempts
            ),
            ("todo", None, 0)
        );
        assert!(blocked.doing_since.is_none());
        claim_for_worker(&mut board, &other, "Mia", false).unwrap();
    }

    #[test]
    fn done_and_review_tasks_cannot_be_blocked() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        move_to(&mut board, &id, "review");
        let review = block_task(&mut board, &id, block("x"), TODAY).unwrap_err();
        assert!(review.contains("review"), "{review}");
        move_to(&mut board, &id, "done");
        let done = block_task(&mut board, &id, block("x"), TODAY).unwrap_err();
        assert!(done.contains("done"), "{done}");
        assert!(board.tasks[0].blocked.is_none());
    }

    #[test]
    fn a_blocked_task_stays_in_todo() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        block_task(&mut board, &id, block("npm release"), TODAY).unwrap();
        let refused = patch_task(
            &mut board,
            &id,
            TaskPatch {
                column: Some("doing".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(refused.contains("npm release"), "{refused}");
        assert!(board.tasks[0].blocked.is_some());
        assert_eq!(board.tasks[0].column, "todo");
    }

    #[test]
    fn human_only_is_lifted_by_the_window_alone() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        block_task(
            &mut board,
            &id,
            BlockRequest {
                human_only: true,
                ..block("the person decides")
            },
            TODAY,
        )
        .unwrap();

        let agent = unblock_task(&mut board, &id, UnblockRequest::default()).unwrap_err();
        assert!(agent.contains("person must unblock"), "{agent}");
        // An agent cannot replace the block either.
        assert!(block_task(&mut board, &id, block("softer"), TODAY).is_err());
        assert!(board.tasks[0].blocked.is_some());

        let lifted = unblock_task(
            &mut board,
            &id,
            UnblockRequest {
                person: true,
                note: Some("decided".into()),
                by: Some("simn".into()),
            },
        )
        .unwrap();
        assert!(lifted.blocked.is_none());
        let last = lifted.comments.last().unwrap();
        assert!(
            last.text.contains("simn") && last.text.contains("decided"),
            "{last:?}"
        );
    }

    #[test]
    fn unblocking_a_task_that_is_not_blocked_says_so() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        let refused = unblock_task(&mut board, &id, UnblockRequest::default()).unwrap_err();
        assert!(refused.contains("not blocked"), "{refused}");
    }

    #[test]
    fn a_recheck_in_the_past_is_refused_and_a_bad_date_too() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        let with = |day: &str| BlockRequest {
            recheck: Some(day.into()),
            ..block("waiting")
        };
        let past = block_task(&mut board, &id, with("2026-11-01"), TODAY).unwrap_err();
        assert!(past.contains("past"), "{past}");
        assert!(block_task(&mut board, &id, with("next week"), TODAY).is_err());
        assert!(board.tasks[0].blocked.is_none());
        block_task(&mut board, &id, with(TODAY), TODAY).unwrap();
        assert_eq!(
            board.tasks[0].blocked.as_ref().unwrap().recheck.as_deref(),
            Some(TODAY)
        );
    }

    #[test]
    fn a_recheck_that_has_passed_unblocks_with_a_comment() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        block_task(
            &mut board,
            &id,
            BlockRequest {
                recheck: Some("2026-11-05".into()),
                ..block("waiting")
            },
            TODAY,
        )
        .unwrap();

        // The day itself is not "passed".
        assert!(!sweep_rechecks(&mut board, "2026-11-05"));
        assert!(board.tasks[0].blocked.is_some());

        assert!(sweep_rechecks(&mut board, "2026-11-06"));
        let task = &board.tasks[0];
        assert!(task.blocked.is_none());
        let note = task.comments.last().unwrap();
        assert_eq!(note.author, HANGAR_AUTHOR);
        assert!(note.text.starts_with("recheck passed"), "{}", note.text);
        assert!(!sweep_rechecks(&mut board, "2026-11-07"));
    }

    #[test]
    fn a_human_only_recheck_reminds_once_and_stays_blocked() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        block_task(
            &mut board,
            &id,
            BlockRequest {
                recheck: Some("2026-11-05".into()),
                human_only: true,
                ..block("waiting")
            },
            TODAY,
        )
        .unwrap();

        assert!(sweep_rechecks(&mut board, "2026-11-06"));
        assert!(board.tasks[0].blocked.as_ref().unwrap().reminded.is_some());
        assert_eq!(board.tasks[0].comments.len(), 1);
        assert!(!sweep_rechecks(&mut board, "2026-11-07"));
        assert_eq!(board.tasks[0].comments.len(), 1);
    }

    #[test]
    fn reading_the_board_acts_on_a_recheck_that_has_passed() {
        let dir = temp_workspace("recheck");
        let cwd = dir.to_string_lossy().into_owned();
        seed(
            &board_path(&cwd),
            r#"{"tasks":[{"id":"a","title":"t","column":"todo","created_at":1,"updated_at":1,
            "blocked":{"reason":"waiting","by":"Eli","since":1,"recheck":"2020-01-01"}}]}"#,
        );

        let store = BoardStore::default();
        let board = store.read(&cwd);

        assert!(board.tasks[0].blocked.is_none());
        assert!(
            read_board(&cwd).unwrap().tasks[0].blocked.is_none(),
            "kept on disk"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_patch_can_block_and_unblock() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        let blocked = patch_task(
            &mut board,
            &id,
            TaskPatch {
                block: Some(block("a package release")),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(blocked.blocked.unwrap().by, "Eli");
        let free = patch_task(
            &mut board,
            &id,
            TaskPatch {
                unblock: Some(UnblockRequest::default()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(free.blocked.is_none());
    }

    /// The board kept parsed is the file's: a change another process makes
    /// (another Hangar, a hand edit) shows on the next read.
    #[test]
    fn a_board_changed_on_disk_is_read_again() {
        let dir = temp_workspace("stamp");
        let cwd = dir.to_string_lossy().into_owned();
        let store = BoardStore::default();
        store.update(&cwd, |board| Ok(todo_task(board))).unwrap();
        assert_eq!(store.read(&cwd).tasks.len(), 1);

        seed(
            &board_path(&cwd),
            ONE_TASK.replace("tache", "tâche").as_str(),
        );
        let board = store.read(&cwd);
        assert_eq!(board.tasks.len(), 1);
        assert_eq!(board.tasks[0].title, "tâche existante");

        // And a change starts from that board, not from the one kept.
        store.update(&cwd, |board| Ok(todo_task(board))).unwrap();
        let on_disk = read_board(&cwd).unwrap();
        assert_eq!(on_disk.tasks.len(), 2);
        assert_eq!(on_disk.tasks[0].id, "a");

        fs::remove_file(board_path(&cwd)).unwrap();
        assert!(store.read(&cwd).tasks.is_empty(), "a board gone is gone");
    }

    /// A change that is refused leaves the board kept as it was: the next
    /// read does not show half of it.
    #[test]
    fn a_refused_change_leaves_the_kept_board_alone() {
        let dir = temp_workspace("refused");
        let cwd = dir.to_string_lossy().into_owned();
        let store = BoardStore::default();
        store.update(&cwd, |board| Ok(todo_task(board))).unwrap();

        let refused = store.update(&cwd, |board| -> Result<(), String> {
            todo_task(board);
            Err("no".into())
        });
        assert!(refused.is_err());
        assert_eq!(store.read(&cwd).tasks.len(), 1);
        assert_eq!(read_board(&cwd).unwrap().tasks.len(), 1);
    }

    /// A change being written (its `fsync` can take tens of milliseconds)
    /// holds no reader of the board, nor any writer of another workspace's.
    #[test]
    fn a_change_in_progress_holds_up_no_read_and_no_other_workspace() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (here, there) = (temp_workspace("held"), temp_workspace("other"));
        let (here, there) = (
            here.to_string_lossy().into_owned(),
            there.to_string_lossy().into_owned(),
        );
        let store: Arc<BoardStore> = Arc::default();
        store.update(&here, |board| Ok(todo_task(board))).unwrap();

        let (entered, wait_entered) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        let writer = {
            let (store, here) = (store.clone(), here.clone());
            std::thread::spawn(move || {
                store.update(&here, |board| {
                    entered.send(()).unwrap();
                    held.recv().unwrap();
                    Ok(todo_task(board))
                })
            })
        };
        wait_entered.recv().unwrap();

        let (done, finished) = mpsc::channel();
        {
            let (store, here, there) = (store.clone(), here.clone(), there.clone());
            std::thread::spawn(move || {
                let read = store.read(&here).tasks.len();
                store.update(&there, |board| Ok(todo_task(board))).unwrap();
                done.send(read).unwrap();
            });
        }
        let read = finished.recv_timeout(Duration::from_secs(10));
        release.send(()).unwrap();
        writer.join().unwrap().unwrap();
        assert_eq!(read, Ok(1), "the read and the other workspace waited");
        assert_eq!(store.read(&here).tasks.len(), 2);
    }

    /// The board on disk, byte for byte, and the rescues beside it.
    fn on_disk(cwd: &str) -> (Option<Vec<u8>>, Vec<Vec<u8>>) {
        let path = board_path(cwd);
        let rescues = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt."))
            .map(|e| fs::read(e.path()).unwrap())
            .collect();
        (fs::read(&path).ok(), rescues)
    }

    /// The data loss this guards against: a board that would not parse was
    /// read as an empty one, and the next change wrote that over every task.
    #[test]
    fn a_board_that_does_not_parse_is_kept_aside_never_written_over() {
        let dir = temp_workspace("corrupt");
        let cwd = dir.to_string_lossy().into_owned();
        // Not UTF-8 at all: a write cut short in the middle of a character.
        let bytes = b"{\"tasks\":[{\"id\":\"a\",\"title\":\"caf\xc3".to_vec();
        let path = board_path(&cwd);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let store = BoardStore::default();

        // Reading shows nothing, and changes nothing on disk.
        assert!(store.read(&cwd).tasks.is_empty());
        assert_eq!(on_disk(&cwd), (Some(bytes.clone()), vec![]));

        // A change fails, and the file is kept aside whole.
        let refused = store.update(&cwd, |board| {
            todo_task(board);
            Ok(())
        });
        let message = refused.expect_err("the change must fail");
        assert!(message.contains(".corrupt."), "{message}");
        assert_eq!(on_disk(&cwd), (None, vec![bytes.clone()]));

        // The next one starts on an empty board, the rescue still there.
        store
            .update(&cwd, |board| {
                todo_task(board);
                Ok(())
            })
            .unwrap();
        assert_eq!(read_board(&cwd).unwrap().tasks.len(), 1);
        assert_eq!(on_disk(&cwd).1, vec![bytes]);
        fs::remove_dir_all(&dir).ok();
    }

    /// A board that cannot be read may be fine: it is neither moved nor
    /// written over, and every change is refused.
    #[cfg(unix)]
    #[test]
    fn a_board_that_cannot_be_read_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        // Permissions do not stop root.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = temp_workspace("eacces");
        let cwd = dir.to_string_lossy().into_owned();
        let path = board_path(&cwd);
        seed(&path, ONE_TASK);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o200)).unwrap();
        let store = BoardStore::default();

        assert!(store.read(&cwd).tasks.is_empty());
        let refused = store.update(&cwd, |board| {
            todo_task(board);
            Ok(())
        });
        assert!(refused.is_err(), "{refused:?}");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), ONE_TASK);
        assert!(on_disk(&cwd).1.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_workspace_with_no_board_anywhere_starts_empty() {
        let dir = temp_workspace("empty");
        let cwd = dir.to_string_lossy().into_owned();

        assert!(read_board(&cwd).unwrap().tasks.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    fn report(percent: f64, note: &str, by: &str) -> ProgressReport {
        ProgressReport {
            percent,
            note: Some(note.into()),
            by: Some(by.into()),
            any_holder: false,
        }
    }

    #[test]
    fn a_board_without_progress_reads_and_writes_as_before() {
        let board: Board = serde_json::from_str(
            r#"{"tasks":[{"id":"a","title":"x","column":"doing","created_at":1,"updated_at":1}]}"#,
        )
        .unwrap();
        assert_eq!(board.tasks[0].progress, None);
        assert!(!serde_json::to_string(&board).unwrap().contains("progress"));
    }

    #[test]
    fn progress_goes_through_serde_unchanged() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        claim_task(&mut board, &id, "Sam").unwrap();
        report_progress(&mut board, &id, report(50.0, "main change done", "Sam")).unwrap();

        let written = serde_json::to_string(&board).unwrap();
        let back: Board = serde_json::from_str(&written).unwrap();
        let progress = back.tasks[0].progress.clone().unwrap();
        assert_eq!(progress.percent, 50);
        assert_eq!(progress.note, "main change done");
        assert_eq!(progress.by.as_deref(), Some("Sam"));
        assert_eq!(back.tasks[0].progress, board.tasks[0].progress);
    }

    #[test]
    fn progress_is_clamped_and_its_note_trimmed_and_bounded() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        claim_task(&mut board, &id, "Sam").unwrap();
        let percent = |board: &mut Board, value: f64| {
            report_progress(board, &id, report(value, "", "Sam"))
                .unwrap()
                .progress
                .unwrap()
                .percent
        };
        assert_eq!(percent(&mut board, 150.0), 100);
        assert_eq!(percent(&mut board, -3.0), 0);
        assert_eq!(percent(&mut board, 42.6), 43);
        assert_eq!(percent(&mut board, f64::NAN), 0);

        let note = |board: &mut Board, text: &str| {
            report_progress(board, &id, report(10.0, text, "Sam"))
                .unwrap()
                .progress
                .unwrap()
                .note
        };
        assert_eq!(note(&mut board, "  plan\n  read  "), "plan read");
        let long = "é".repeat(MAX_PROGRESS_NOTE + 50);
        assert_eq!(note(&mut board, &long).chars().count(), MAX_PROGRESS_NOTE);
    }

    #[test]
    fn progress_is_refused_off_doing_and_from_whoever_does_not_hold_the_task() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        let err = report_progress(&mut board, &id, report(10.0, "", "Sam")).unwrap_err();
        assert!(err.contains("not doing"), "{err}");

        claim_task(&mut board, &id, "Sam").unwrap();
        let err = report_progress(&mut board, &id, report(10.0, "", "Eli")).unwrap_err();
        assert!(err.contains("held by Sam"), "{err}");
        let anonymous = ProgressReport {
            percent: 10.0,
            ..Default::default()
        };
        assert!(report_progress(&mut board, &id, anonymous).is_err());
        assert!(report_progress(&mut board, "nope", report(10.0, "", "Sam")).is_err());

        // Hangar, or the orchestrator, may report on a task held by another.
        let orchestrator = ProgressReport {
            any_holder: true,
            ..report(30.0, "", "Leo")
        };
        let task = report_progress(&mut board, &id, orchestrator).unwrap();
        assert_eq!(task.progress.unwrap().by.as_deref(), Some("Leo"));

        // Even they need the task in doing.
        move_to(&mut board, &id, "review");
        let late = ProgressReport {
            any_holder: true,
            ..report(30.0, "", "Leo")
        };
        assert!(report_progress(&mut board, &id, late).is_err());
    }

    #[test]
    fn done_sets_the_progress_to_100_and_todo_clears_it() {
        let mut board = Board::default();
        let id = todo_task(&mut board);
        claim_task(&mut board, &id, "Sam").unwrap();
        report_progress(&mut board, &id, report(80.0, "tests passing", "Sam")).unwrap();
        move_to(&mut board, &id, "review");
        let done = move_to(&mut board, &id, "done").progress.unwrap();
        assert_eq!(done.percent, 100);
        assert_eq!(done.note, "tests passing");

        // A task done without any estimate stays without one.
        let other = todo_task(&mut board);
        assert_eq!(move_to(&mut board, &other, "done").progress, None);

        let again = todo_task(&mut board);
        claim_task(&mut board, &again, "Sam").unwrap();
        report_progress(&mut board, &again, report(40.0, "", "Sam")).unwrap();
        assert_eq!(move_to(&mut board, &again, "todo").progress, None);

        // Released by its worker, or blocked: back in todo, cleared too.
        claim_task(&mut board, &again, "Sam").unwrap();
        report_progress(&mut board, &again, report(40.0, "", "Sam")).unwrap();
        assert_eq!(
            release_worker(&mut board, &again, "Sam", false)
                .unwrap()
                .progress,
            None
        );
    }
}
