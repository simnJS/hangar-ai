//! « Besoin de toi »: everything that waits for the person, across the
//! workspaces, in one list (`lib/needsYou.ts` and its tests), so that a day
//! spent letting orchestrators work is not a day spent walking every workspace
//! to see who is stuck.
//!
//! Three things wait: an agent asking for a permission or an answer (its own
//! activity), a task the orchestrator or Hangar handed back with a question
//! (`needs-you`), and a delivery that passed its check and its review and
//! waits to land (`ready-to-land`, when automatic landing is off). The two
//! labels are put on the board by the orchestrator and by Hangar's deliveries;
//! this only reads them.
//!
//! Nothing of it exists until a workspace has an orchestrator or a worker:
//! someone who never pins one keeps the app exactly as it was.

use std::collections::{HashMap, HashSet};

use hangar_core::board::Task;

use crate::state::roles::{NEEDS_YOU_LABEL, READY_TO_LAND_LABEL, orchestrator_of};
use crate::state::types::{PaneRole, Workspace};

/// The tasks of each board folder that has been read. A folder absent from the
/// map is a board still being read, or one nobody asked for.
pub type Boards = HashMap<String, Vec<Task>>;

/// Where the boards come from: the shared cache of the board package in the
/// app (`board::Live`, `useBoardsOf`), a plain map in the tests. A folder
/// whose board has not been read yet has no tasks to list.
pub trait BoardSource {
    fn tasks_in(&self, cwd: &str) -> Option<&[Task]>;
}

impl BoardSource for Boards {
    fn tasks_in(&self, cwd: &str) -> Option<&[Task]> {
        self.get(cwd).map(Vec::as_slice)
    }
}

impl BoardSource for crate::board::Live {
    fn tasks_in(&self, cwd: &str) -> Option<&[Task]> {
        self.tasks(cwd)
    }
}

/// What every item says about itself.
#[derive(Debug, Clone)]
pub struct ItemBase {
    /// Stable while the item stands for the same thing, for lists and notices.
    pub key: String,
    pub workspace_id: String,
    pub workspace_name: String,
}

#[derive(Debug, Clone)]
pub enum NeedsYouItem {
    /// An agent in a dialog: a permission, a question.
    Waiting {
        base: ItemBase,
        pane_id: String,
        pane_name: String,
    },
    /// A task labelled `needs-you`.
    Question {
        base: ItemBase,
        cwd: String,
        task: Task,
        orchestrator_id: Option<String>,
    },
    /// A task labelled `ready-to-land`.
    Land {
        base: ItemBase,
        cwd: String,
        task: Task,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Waiting,
    Question,
    Land,
}

/// The order the list is read in: what blocks an agent right now comes first.
pub const KINDS: [Kind; 3] = [Kind::Waiting, Kind::Question, Kind::Land];

impl NeedsYouItem {
    pub fn base(&self) -> &ItemBase {
        match self {
            NeedsYouItem::Waiting { base, .. }
            | NeedsYouItem::Question { base, .. }
            | NeedsYouItem::Land { base, .. } => base,
        }
    }

    pub fn key(&self) -> &str {
        &self.base().key
    }

    pub fn kind(&self) -> Kind {
        match self {
            NeedsYouItem::Waiting { .. } => Kind::Waiting,
            NeedsYouItem::Question { .. } => Kind::Question,
            NeedsYouItem::Land { .. } => Kind::Land,
        }
    }

    /// The board folder of a task item.
    pub fn cwd(&self) -> Option<&str> {
        match self {
            NeedsYouItem::Waiting { .. } => None,
            NeedsYouItem::Question { cwd, .. } | NeedsYouItem::Land { cwd, .. } => Some(cwd),
        }
    }

    #[cfg(test)]
    pub fn task(&self) -> Option<&Task> {
        match self {
            NeedsYouItem::Waiting { .. } => None,
            NeedsYouItem::Question { task, .. } | NeedsYouItem::Land { task, .. } => Some(task),
        }
    }
}

/// A workspace where orchestration is at work: an orchestrator or a worker in
/// it (`isOrchestrated`).
pub fn is_orchestrated(ws: &Workspace) -> bool {
    ws.panes
        .iter()
        .any(|pane| matches!(pane.role, Some(PaneRole::Orchestrator | PaneRole::Worker)))
}

/// The boards to read: those of the workspaces that orchestrate, once each,
/// since two workspaces on one folder share a board.
pub fn watched_boards(workspaces: &[&Workspace]) -> Vec<String> {
    let mut seen = HashSet::new();
    workspaces
        .iter()
        .filter(|ws| is_orchestrated(ws))
        .filter(|ws| seen.insert(ws.cwd.as_str()))
        .map(|ws| ws.cwd.clone())
        .collect()
}

/// The list, in `KINDS` order, then in the navigator's order of workspaces.
/// Questions go by priority, then freshest first; deliveries by age, the one
/// that has waited longest first. A task carrying both labels is a question:
/// whatever made it one is the thing to settle before it can land.
///
/// `workspaces` are in the order the person sees them (the navigator's), and
/// `waiting` holds the panes waiting for an answer, in any order.
pub fn needs_you_items(
    workspaces: &[&Workspace],
    waiting: &HashSet<String>,
    boards: &impl BoardSource,
) -> Vec<NeedsYouItem> {
    if !workspaces.iter().any(|ws| is_orchestrated(ws)) {
        return Vec::new();
    }

    let mut items: Vec<NeedsYouItem> = Vec::new();
    for ws in workspaces {
        for pane in ws.panes.iter().filter(|pane| waiting.contains(&pane.id)) {
            items.push(NeedsYouItem::Waiting {
                base: ItemBase {
                    key: format!("pane:{}", pane.id),
                    workspace_id: ws.id.clone(),
                    workspace_name: ws.name.clone(),
                },
                pane_id: pane.id.clone(),
                pane_name: pane.name.clone(),
            });
        }
    }

    let mut questions: Vec<NeedsYouItem> = Vec::new();
    let mut lands: Vec<NeedsYouItem> = Vec::new();
    let mut read: HashSet<&str> = HashSet::new();
    for ws in workspaces {
        // A shared board is listed under the first workspace that
        // orchestrates on it, once.
        if !is_orchestrated(ws) || !read.insert(ws.cwd.as_str()) {
            continue;
        }
        let Some(tasks) = boards.tasks_in(&ws.cwd) else {
            continue;
        };
        let base = |key: String| ItemBase {
            key,
            workspace_id: ws.id.clone(),
            workspace_name: ws.name.clone(),
        };
        let open: Vec<&Task> = tasks.iter().filter(|task| task.column != "done").collect();
        let orchestrator_id = orchestrator_of(ws).map(|pane| pane.id.clone());

        let mut asking = open.clone();
        asking.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(b.updated_at.cmp(&a.updated_at))
        });
        for task in asking {
            if task.labels.iter().any(|label| label == NEEDS_YOU_LABEL) {
                questions.push(NeedsYouItem::Question {
                    base: base(format!("task:{}:{}:{NEEDS_YOU_LABEL}", ws.cwd, task.id)),
                    cwd: ws.cwd.clone(),
                    task: task.clone(),
                    orchestrator_id: orchestrator_id.clone(),
                });
            }
        }

        let mut ready = open;
        ready.sort_by_key(|task| task.updated_at);
        for task in ready {
            let ready_to_land = task.labels.iter().any(|label| label == READY_TO_LAND_LABEL);
            let asking = task.labels.iter().any(|label| label == NEEDS_YOU_LABEL);
            if ready_to_land && !asking {
                lands.push(NeedsYouItem::Land {
                    base: base(format!("task:{}:{}:{READY_TO_LAND_LABEL}", ws.cwd, task.id)),
                    cwd: ws.cwd.clone(),
                    task: task.clone(),
                });
            }
        }
    }

    items.extend(questions);
    items.extend(lands);
    items
}

/// What a task item says about itself: its latest comment, on one line.
pub fn latest_note(task: &Task) -> Option<String> {
    let last = task.comments.last()?;
    last.text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// What the notices have already seen: the items of the last look, and the
/// boards that had been read by then.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoticeMemory {
    pub seen: HashSet<String>,
    pub primed: HashSet<String>,
}

/// The items worth a desktop notification: tasks that were not in the list the
/// last time it was read. A board read for the first time only sets the
/// baseline (what was already waiting when the app opened is no news) and an
/// item that leaves and comes back is news again. Agents in a dialog are left
/// out: their pane already notifies.
pub fn notice_delta(
    memory: &NoticeMemory,
    items: &[NeedsYouItem],
    loaded: &HashSet<String>,
) -> (NoticeMemory, Vec<NeedsYouItem>) {
    let fresh = items
        .iter()
        .filter(|item| {
            item.cwd()
                .is_some_and(|cwd| memory.primed.contains(cwd) && !memory.seen.contains(item.key()))
        })
        .cloned()
        .collect();
    let next = NoticeMemory {
        seen: items.iter().map(|item| item.key().to_string()).collect(),
        primed: loaded.clone(),
    };
    (next, fresh)
}

#[cfg(test)]
mod tests;
