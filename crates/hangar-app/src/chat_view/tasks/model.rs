//! What the tasks panel shows, worked out from the board, the panes and the
//! labels, without drawing anything: the global progress, what waits for the
//! person, and the tasks grouped by column.
//!
//! It is computed when what it reads changed (a new read of the board, a saved
//! change of the state, a pane that starts or stops waiting) and kept between
//! frames ([`Cache`]): the view only lays out what is here.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use hangar_core::board::Task;

use crate::board::Column;
use crate::needs_you::model::{
    BoardSource, ItemBase, Kind, NeedsYouItem, latest_note, needs_you_items,
};
use crate::panes_logic::state_glyph_state;
use crate::state::live::GlyphState;
use crate::state::{Model, PaneId, Workspace, WsId};

/// The tasks panel of one workspace, ready to draw.
#[derive(Debug, Clone)]
pub struct Panel {
    /// The board has been read: before that the panel says it is reading
    /// rather than showing an empty board.
    pub loaded: bool,
    pub overall: Overall,
    /// What waits for the person, in `needs_you::model::KINDS` order: agents
    /// in a dialog, questions, deliveries ready to land.
    pub waiting: Vec<Waiting>,
    /// The tasks, by column in the dock's order (what is under way first).
    /// Only columns that have a task.
    pub groups: Vec<Group>,
}

/// The board's progress as one number: the mean of the tasks' (done is 100,
/// anything else its reported percent, or 0 without a report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overall {
    pub percent: u8,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub column: Column,
    pub rows: Vec<Row>,
}

/// One task, as its row says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub title: String,
    /// Who holds it: the worker's name.
    pub assignee: Option<String>,
    pub column: Column,
    /// What its agent last reported, when it did.
    pub progress: Option<Reported>,
    /// Why it waits, when it is blocked.
    pub blocked: Option<String>,
    pub description: String,
    pub last_comment: Option<LastComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    pub percent: u8,
    pub note: String,
    /// When, in ms since the epoch; 0 when the board did not say.
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastComment {
    pub author: String,
    pub text: String,
}

/// One thing that waits for the person, with what the panel needs to act on
/// it.
#[derive(Debug, Clone)]
pub enum Waiting {
    /// An agent asking for a permission or an answer in its pane.
    Pane { pane: PaneId, name: String },
    /// A task handed back with a question (`needs-you`), answered where the
    /// orchestrator is.
    Question {
        title: String,
        note: Option<String>,
        orchestrator: Option<(PaneId, String)>,
    },
    /// A delivery that passed its check and its review (`ready-to-land`):
    /// landed through the needs-you package, which keeps its landing state.
    Land {
        base: ItemBase,
        task: String,
        title: String,
        assignee: Option<String>,
    },
}

/// The share a task counts for in the global progress.
fn share(task: &Task) -> u8 {
    if task.column == Column::Done.id() {
        return 100;
    }
    task.progress.as_ref().map_or(0, |p| p.percent.min(100))
}

/// The global progress of the tasks this version knows the column of.
pub fn overall(tasks: &[Task]) -> Overall {
    let known: Vec<&Task> = tasks
        .iter()
        .filter(|task| Column::of(task).is_some())
        .collect();
    if known.is_empty() {
        return Overall::default();
    }
    let sum: u32 = known.iter().map(|task| u32::from(share(task))).sum();
    let total = known.len();
    let mean = (f64::from(sum) / total as f64).round() as u8;
    Overall {
        percent: mean,
        done: known
            .iter()
            .filter(|task| task.column == Column::Done.id())
            .count(),
        total,
    }
}

fn row_of(task: &Task, column: Column) -> Row {
    Row {
        id: task.id.clone(),
        title: task.title.clone(),
        assignee: task.assignee.clone().filter(|who| !who.is_empty()),
        column,
        progress: task.progress.as_ref().map(|p| Reported {
            percent: p.percent.min(100),
            note: p.note.trim().to_string(),
            at: p.at,
        }),
        blocked: task.blocked.as_ref().map(|b| b.reason.clone()),
        description: task.description.trim().to_string(),
        last_comment: task.comments.last().map(|c| LastComment {
            author: c.author.clone(),
            text: c.text.trim().to_string(),
        }),
    }
}

/// The tasks by column, in the dock's order; in a column the most urgent
/// first, then in the order they were put there, as the board sorts them.
pub fn groups(tasks: &[Task]) -> Vec<Group> {
    Column::DOCK_ORDER
        .into_iter()
        .filter_map(|column| {
            let mut items: Vec<&Task> = tasks
                .iter()
                .filter(|task| task.column == column.id())
                .collect();
            if items.is_empty() {
                return None;
            }
            items.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then_with(|| a.order.total_cmp(&b.order))
            });
            Some(Group {
                column,
                rows: items.into_iter().map(|task| row_of(task, column)).collect(),
            })
        })
        .collect()
}

/// The one board the panel reads, lent to the needs-you rules.
struct OneBoard<'a> {
    cwd: &'a str,
    tasks: &'a [Task],
}

impl BoardSource for OneBoard<'_> {
    fn tasks_in(&self, cwd: &str) -> Option<&[Task]> {
        (cwd == self.cwd).then_some(self.tasks)
    }
}

/// What waits for the person in `ws`: its panes in a dialog (`waiting`, in
/// the workspace's order), then its board's questions and deliveries as the
/// needs-you list has them.
pub fn waiting_of(ws: &Workspace, waiting: &[PaneId], tasks: Option<&[Task]>) -> Vec<Waiting> {
    let mut out: Vec<Waiting> = ws
        .panes
        .iter()
        .filter(|pane| waiting.contains(&pane.id))
        .map(|pane| Waiting::Pane {
            pane: pane.id.clone(),
            name: pane.name.clone(),
        })
        .collect();
    let Some(tasks) = tasks else {
        return out;
    };
    let board = OneBoard {
        cwd: &ws.cwd,
        tasks,
    };
    for item in needs_you_items(&[ws], &HashSet::new(), &board) {
        match item {
            NeedsYouItem::Question {
                task,
                orchestrator_id,
                ..
            } => out.push(Waiting::Question {
                note: latest_note(&task),
                orchestrator: orchestrator_id.and_then(|id| {
                    let pane = ws.panes.iter().find(|pane| pane.id == id)?;
                    Some((id, pane.name.clone()))
                }),
                title: task.title,
            }),
            NeedsYouItem::Land { base, task, .. } => out.push(Waiting::Land {
                base,
                task: task.id,
                title: task.title,
                assignee: task.assignee,
            }),
            NeedsYouItem::Waiting { .. } => {}
        }
    }
    out
}

impl Waiting {
    pub fn kind(&self) -> Kind {
        match self {
            Waiting::Pane { .. } => Kind::Waiting,
            Waiting::Question { .. } => Kind::Question,
            Waiting::Land { .. } => Kind::Land,
        }
    }
}

/// The panel of `ws` from what it reads.
pub fn panel_of(ws: &Workspace, tasks: Option<&[Task]>, waiting: &[PaneId]) -> Panel {
    let list = tasks.unwrap_or_default();
    Panel {
        loaded: tasks.is_some(),
        overall: overall(list),
        waiting: waiting_of(ws, waiting, tasks),
        groups: groups(list),
    }
}

/// The panes of `ws` whose agent is in a dialog, in the workspace's order.
pub fn waiting_panes(m: &Model, ws: &Workspace) -> Vec<PaneId> {
    ws.panes
        .iter()
        .filter(|pane| state_glyph_state(m, &pane.id) == GlyphState::Waiting)
        .map(|pane| pane.id.clone())
        .collect()
}

/// What the panel was computed from: when none of it changed, neither did
/// the panel.
#[derive(Debug, Clone)]
struct Inputs {
    ws: WsId,
    /// The board's read, by identity: each read of it is a new `Arc`.
    board: Option<Arc<Vec<Task>>>,
    /// The state's revision: pane names, roles, workspaces.
    revision: u64,
    waiting: Vec<PaneId>,
}

impl Inputs {
    fn same(&self, other: &Inputs) -> bool {
        self.ws == other.ws
            && self.revision == other.revision
            && self.waiting == other.waiting
            && match (&self.board, &other.board) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

/// The last panel worked out, and what from.
#[derive(Debug, Default)]
pub struct Cache {
    last: RefCell<Option<(Inputs, Arc<Panel>)>>,
}

impl Cache {
    /// The panel of `ws` as things are now: the one kept when nothing it
    /// reads changed, else a fresh one, kept for the next frame.
    pub fn panel(&self, m: &Model, ws: &Workspace) -> Arc<Panel> {
        let inputs = Inputs {
            ws: ws.id.clone(),
            board: m.live.board.shared(&crate::board::cache::board_key(ws)),
            revision: m.revision(),
            waiting: waiting_panes(m, ws),
        };
        let mut last = self.last.borrow_mut();
        if let Some((seen, panel)) = last.as_ref()
            && seen.same(&inputs)
        {
            return panel.clone();
        }
        let panel = Arc::new(panel_of(
            ws,
            inputs.board.as_deref().map(Vec::as_slice),
            &inputs.waiting,
        ));
        *last = Some((inputs, panel.clone()));
        panel
    }
}
