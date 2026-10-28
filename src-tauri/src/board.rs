use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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

fn read_board(workspace_cwd: &str) -> Board {
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
    let path = candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| board_path(&shared));
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_board(workspace_cwd: &str, board: &Board) -> Result<(), String> {
    let shared = shared_workspace(workspace_cwd);
    let path = board_path(&shared);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(board).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
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

/// Serializes every mutation per workspace.
///
/// This is the whole reason the app owns the board instead of letting each
/// agent edit the file: without a single writer, two agents doing
/// read-modify-write would both claim the same task and duplicate the work.
#[derive(Default)]
pub struct BoardStore {
    guard: Mutex<()>,
}

impl BoardStore {
    pub fn read(&self, cwd: &str) -> Board {
        let _guard = self.guard.lock().unwrap();
        read_board(cwd)
    }

    /// Applies `mutate` under the global lock and persists the result.
    pub fn update<T>(
        &self,
        cwd: &str,
        mutate: impl FnOnce(&mut Board) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self.guard.lock().unwrap();
        let mut board = read_board(cwd);
        let outcome = mutate(&mut board)?;
        write_board(cwd, &board)?;
        Ok(outcome)
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
    };
    board.tasks.push(task.clone());
    Ok(task)
}

pub fn patch_task(board: &mut Board, id: &str, patch: TaskPatch) -> Result<Task, String> {
    if let Some(column) = &patch.column {
        if !COLUMNS.contains(&column.as_str()) {
            return Err(format!("unknown column '{column}'"));
        }
    }
    let task = board.find_mut(id).ok_or("task not found")?;
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
/// what to do instead: an orchestrator reads it.
///
/// `shared_folder`: the workspace is not a git repository, so the worker
/// starts in its folder beside the others rather than in a worktree of its
/// own (`folder_clash`).
pub fn claim_for_worker(
    board: &mut Board,
    id: &str,
    worker: &str,
    max_attempts: u32,
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
            Some(t) => Some(format!("\"{}\" ({}, in {})", t.title, t.id, t.column)),
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
    if task.attempts >= max_attempts {
        return Err(format!(
            "\"{title}\" has had {} worker(s) already, the most one task gets (maxAttempts {max_attempts}). \
             Do not start another: add the label needs-you (board_update_task labels, keeping its other \
             labels) and comment on the task what went wrong and what you need from the person.",
            task.attempts
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

/// Past this many alternatives a brace pattern is read no further: a pattern
/// that large is a mistake, and the ones read already decide most overlaps.
const MAX_ALTERNATIVES: usize = 64;

/// Segments past this are not read: nobody owns a path that deep, and each
/// `**` makes the comparison more expensive.
const MAX_SEGMENTS: usize = 32;

/// `src/{lib,components}/x` as the two patterns it stands for.
fn expand_braces(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut depth = 0;
    let mut open = 0;
    for (at, &c) in chars.iter().enumerate() {
        match c {
            '{' => {
                if depth == 0 {
                    open = at;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth > 0 {
                    continue;
                }
                let alternatives = split_alternatives(&chars[open + 1..at]);
                // `{x}` alone is not a choice: its braces stay as they are.
                if alternatives.len() < 2 {
                    continue;
                }
                let head: String = chars[..open].iter().collect();
                let tail: String = chars[at + 1..].iter().collect();
                return alternatives
                    .iter()
                    .flat_map(|alternative| expand_braces(&format!("{head}{alternative}{tail}")))
                    .take(MAX_ALTERNATIVES)
                    .collect();
            }
            _ => {}
        }
    }
    vec![pattern.to_string()]
}

/// The commas of one brace group that are not inside a nested one.
fn split_alternatives(inner: &[char]) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut depth = 0;
    for &c in inner {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(String::new());
                continue;
            }
            _ => {}
        }
        parts.last_mut().unwrap().push(c);
    }
    parts
}

/// A pattern as the rule compares it: relative, in forward slashes, without
/// `./`, and in lower case — the filesystems that ignore case would put both
/// spellings on one file, and erring towards an overlap only costs a refusal
/// that says why. None for a pattern that names nothing.
///
/// A folder owns what is under it, so it ends in an implicit `**`. A name with
/// an extension reads as a file and does not: `src/app.ts` is not where
/// `**/*.test.ts` lives. A folder whose name has a dot in it is written with a
/// trailing slash, or as `name/**`.
fn owned_segments(pattern: &str) -> Option<Vec<String>> {
    let normalized = pattern.trim().replace('\\', "/").to_lowercase();
    let mut segments: Vec<String> = normalized
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .take(MAX_SEGMENTS)
        .map(str::to_string)
        .collect();
    let last = segments.last()?;
    let file = !normalized.ends_with('/') && last.char_indices().any(|(at, c)| c == '.' && at > 0);
    if !file && last != "**" {
        segments.push("**".to_string());
    }
    Some(segments)
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

/// One name against one segment pattern: `*`, `?` and `[…]` (`[!…]` or
/// `[^…]` negated, with ranges). None of them crosses a separator.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    glob_from(&pattern, &name)
}

fn glob_from(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => {
            let rest = &pattern[1..];
            (0..=name.len()).any(|skipped| glob_from(rest, &name[skipped..]))
        }
        Some('?') => !name.is_empty() && glob_from(&pattern[1..], &name[1..]),
        Some('[') => match (char_class(pattern), name.first()) {
            (Some((length, matches)), Some(&c)) => {
                matches(c) && glob_from(&pattern[length..], &name[1..])
            }
            (Some(_), None) => false,
            // No closing bracket: a plain `[`.
            (None, first) => first == Some(&'[') && glob_from(&pattern[1..], &name[1..]),
        },
        Some(c) => name.first() == Some(c) && glob_from(&pattern[1..], &name[1..]),
    }
}

/// The class `pattern` opens with, as its length and what it accepts.
#[allow(clippy::type_complexity)]
fn char_class(pattern: &[char]) -> Option<(usize, Box<dyn Fn(char) -> bool>)> {
    let mut at = 1;
    let negated = matches!(pattern.get(at), Some('!') | Some('^'));
    if negated {
        at += 1;
    }
    let start = at;
    // A `]` first in the class is one of its characters.
    let close = (start..pattern.len()).find(|&i| pattern[i] == ']' && i > start)?;
    let members: Vec<char> = pattern[start..close].to_vec();
    let accepts = move |c: char| {
        let mut i = 0;
        let mut found = false;
        while i < members.len() {
            if i + 2 < members.len() && members[i + 1] == '-' {
                found |= members[i] <= c && c <= members[i + 2];
                i += 3;
            } else {
                found |= members[i] == c;
                i += 1;
            }
        }
        found != negated
    };
    Some((close + 1, Box::new(accepts)))
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

/// The coordination primitive: highest-priority unassigned task in `todo`
/// whose dependencies are all done. Returns None when nothing is actionable.
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
mod tests {
    use super::*;

    const ONE_TASK: &str = r#"{"tasks":[{"id":"a","title":"tache existante","column":"todo","created_at":1,"updated_at":1}]}"#;

    fn temp_workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hangar-board-{tag}-{}", new_id()));
        fs::create_dir_all(&dir).expect("temp workspace");
        dir
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

        let board = read_board(&cwd);

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

        let board = read_board(&cwd);
        write_board(&cwd, &board).expect("write");

        let adopted = read_board(&cwd);
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

        let board = read_board(&cwd);

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

        let claimed = claim_for_worker(&mut board, &id, "Leo", 2, false).unwrap();
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
            |board: &mut Board, id: &str| claim_for_worker(board, id, "Leo", 2, false).unwrap_err();
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
    fn a_worker_claim_stops_at_the_attempt_limit() {
        let mut board = Board::default();
        let id = owned(&mut board, "Flaky", &["src/flaky.rs"]);
        claim_for_worker(&mut board, &id, "Leo", 2, false).unwrap();
        release_worker(&mut board, &id, "Leo", false).unwrap();
        claim_for_worker(&mut board, &id, "Mia", 2, false).unwrap();
        release_worker(&mut board, &id, "Mia", false).unwrap();

        let refused = claim_for_worker(&mut board, &id, "Sam", 2, false).unwrap_err();
        assert!(refused.contains("2 worker(s) already"), "{refused}");
        assert!(refused.contains("needs-you"), "{refused}");
        assert_eq!(board.tasks[0].column, "todo");
        // The limit is the setting's, not a constant.
        assert!(claim_for_worker(&mut board, &id, "Sam", 3, false).is_ok());
    }

    #[test]
    fn two_workers_on_tasks_that_own_nothing_never_run_at_once() {
        let mut board = Board::default();
        let first = todo_task(&mut board);
        let second = todo_task(&mut board);
        claim_for_worker(&mut board, &first, "Leo", 2, false).unwrap();

        let refused = claim_for_worker(&mut board, &second, "Mia", 2, false).unwrap_err();
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
        assert!(claim_for_worker(&mut board, &second, "Mia", 2, false).is_ok());
    }

    #[test]
    fn workers_sharing_a_folder_without_git_all_say_what_they_own() {
        let mut board = Board::default();
        let unscoped = todo_task(&mut board);
        let scoped = owned(&mut board, "Scoped", &["docs/**"]);
        let other = owned(&mut board, "Other", &["src/**"]);
        let overlapping = owned(&mut board, "Overlapping", &["docs/guide.md"]);

        // Alone, a task that owns nothing runs.
        claim_for_worker(&mut board, &unscoped, "Leo", 2, true).unwrap();
        // Beside it, nothing else does, however scoped.
        let refused = claim_for_worker(&mut board, &scoped, "Mia", 2, true).unwrap_err();
        assert!(
            refused.contains("does not say which files it owns"),
            "{refused}"
        );
        assert!(refused.contains("not a git repository"), "{refused}");
        // In a repository, worktrees keep them apart until they land.
        claim_for_worker(&mut board, &scoped, "Mia", 2, false).unwrap();
        release_worker(&mut board, &scoped, "Mia", true).unwrap();

        release_worker(&mut board, &unscoped, "Leo", true).unwrap();
        claim_for_worker(&mut board, &scoped, "Mia", 2, true).unwrap();
        let refused = claim_for_worker(&mut board, &unscoped, "Leo", 2, true).unwrap_err();
        assert!(refused.contains("Set owns on this task"), "{refused}");
        // Scoped and apart, they run side by side; sharing a file, they do not.
        claim_for_worker(&mut board, &other, "Sam", 2, true).unwrap();
        let refused = claim_for_worker(&mut board, &overlapping, "Ivy", 2, true).unwrap_err();
        assert!(refused.contains("owns too"), "{refused}");
    }

    #[test]
    fn releasing_a_worker_gives_back_only_what_is_still_its_own() {
        let mut board = Board::default();
        let id = owned(&mut board, "Login", &["src/login/**"]);

        // A start that never got going takes its attempt back.
        claim_for_worker(&mut board, &id, "Leo", 2, false).unwrap();
        let undone = release_worker(&mut board, &id, "Leo", true).unwrap();
        assert_eq!(
            (undone.column.as_str(), undone.assignee, undone.attempts),
            ("todo", None, 0)
        );
        assert_eq!(undone.doing_since, None);

        // A stopped worker's attempt still counts.
        claim_for_worker(&mut board, &id, "Leo", 2, false).unwrap();
        let stopped = release_worker(&mut board, &id, "Leo", false).unwrap();
        assert_eq!((stopped.column.as_str(), stopped.attempts), ("todo", 1));

        // Waiting for review: it stays where it is.
        claim_for_worker(&mut board, &id, "Mia", 2, false).unwrap();
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

    #[test]
    fn a_workspace_with_no_board_anywhere_starts_empty() {
        let dir = temp_workspace("empty");
        let cwd = dir.to_string_lossy().into_owned();

        assert!(read_board(&cwd).tasks.is_empty());
        fs::remove_dir_all(&dir).ok();
    }
}
