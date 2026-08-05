use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    Path::new(workspace_cwd).join(".iabench").join("board.json")
}

fn read_board(workspace_cwd: &str) -> Board {
    let path = board_path(workspace_cwd);
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_board(workspace_cwd: &str, board: &Board) -> Result<(), String> {
    let path = board_path(workspace_cwd);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(board).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
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
        column,
        priority: input.priority.unwrap_or(1),
        assignee: None,
        labels: input.labels,
        comments: vec![],
        depends_on: input.depends_on,
        created_at: stamp,
        updated_at: stamp,
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

    if let Some(v) = patch.title {
        task.title = v;
    }
    if let Some(v) = patch.description {
        task.description = v;
    }
    if let Some(v) = patch.column {
        task.column = v;
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
    if let Some(v) = patch.depends_on {
        task.depends_on = v;
    }
    if let Some(v) = patch.order {
        task.order = v;
    }
    task.updated_at = now_ms();
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
/// is what stops two agents working the same item.
pub fn claim_task(board: &mut Board, id: &str, agent: &str) -> Result<Task, String> {
    let task = board.find_mut(id).ok_or("task not found")?;
    match &task.assignee {
        Some(owner) if owner != agent => Err(format!("already claimed by {owner}")),
        _ => {
            task.assignee = Some(agent.to_string());
            if task.column == "todo" {
                task.column = "doing".into();
            }
            task.updated_at = now_ms();
            Ok(task.clone())
        }
    }
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
