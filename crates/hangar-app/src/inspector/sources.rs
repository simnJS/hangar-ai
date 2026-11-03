//! Where the inspector reads what other packages own.
//!
//! The inspector draws the focused pane from things it does not keep: what the
//! pane reported (P4's `PaneInsight`, its activity and the plan's limits), the
//! board task on it (P12's boards cache), the shortcut of a command (P7's
//! keymap) and the order the panes are read in. Each read is a function here,
//! so the sections never reach into another package's `Live` themselves.
//!
//! The words and numbers those packages' TS libraries produced for the
//! inspector (`formatTokens`, `elapsedLabel`, `shortTaskId`, `queuedBy`,
//! `paneEventText`, `resolveShell`) are called where they are drawn, from the
//! package that owns each: `panes_logic` for the pane's, `board::format` for
//! the task's.

use std::collections::HashSet;

use hangar_core::board::Task;
use iced::window;

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::shell::WindowKind;
use crate::state::live::{EpochMs, PaneActivity, PaneInsight, PlanLimits};
use crate::state::{Model, Pane, PaneId, Workspace};

// ---- The workspace and the pane -----------------------------------------------------------

/// The workspace a window shows: its own when it is a workspace window, the
/// active one in the main window.
pub fn window_workspace(m: &Model, window: window::Id) -> Option<&Workspace> {
    match m.live.windows.get(&window) {
        Some(WindowKind::Workspace(id)) => m.workspace(id),
        _ => m.active_workspace(),
    }
}

/// The panes of a workspace in the order they are read on screen (`readingOrder`
/// of `lib/layout.ts`): the tree's leaves that name a pane, once each, then
/// the panes the tree does not mention, in their own order.
///
/// TODO(P0b): `state::layout::normalize_tree` is not ported yet; this is the
/// part of it a reading order needs, and it gives the same order.
pub fn reading_order(ws: &Workspace) -> Vec<&Pane> {
    let mut seen = HashSet::new();
    let mut order: Vec<&Pane> = Vec::with_capacity(ws.panes.len());
    if let Some(tree) = &ws.tree {
        for id in crate::state::layout::leaf_ids(tree) {
            if let Some(pane) = ws.panes.iter().find(|pane| pane.id == id)
                && seen.insert(&pane.id)
            {
                order.push(pane);
            }
        }
    }
    order.extend(ws.panes.iter().filter(|pane| !seen.contains(&pane.id)));
    order
}

/// The pane the inspector speaks about and its 1-based place in reading
/// order. The pane in focus when the workspace has one; otherwise the first,
/// which is where App's effect puts the focus ("keep focus on a pane that
/// still exists") before the inspector could be asked.
pub fn focused_pane<'a>(m: &Model, ws: &'a Workspace) -> Option<(&'a Pane, usize)> {
    let order = reading_order(ws);
    let wanted = m.ui.focus_by_workspace.get(&ws.id);
    let found = wanted.and_then(|id| order.iter().position(|pane| &pane.id == id));
    let at = found.unwrap_or(0);
    order.get(at).map(|pane| (*pane, at + 1))
}

// ---- What the pane reported (P4) ----------------------------------------------------------

/// What the pane knows about its agent (`usePaneInsight`): model, version,
/// context, cost, queue, edited files, history. Empty for a pane that has
/// reported nothing yet.
pub fn insight<'a>(m: &'a Model, pane: &PaneId) -> &'a PaneInsight {
    m.live.panes_logic.insight(pane)
}

/// The pane's activity and whether it comes from the bridge (`usePaneActivity`).
pub fn activity<'a>(m: &'a Model, pane: &PaneId) -> Option<&'a PaneActivity> {
    m.live.panes_logic.pane_activity(pane)
}

/// The plan's rate limits, from whichever session reported last
/// (`usePlanLimits`).
pub fn plan_limits(m: &Model) -> Option<&PlanLimits> {
    m.live.panes_logic.plan_limits()
}

/// Epoch milliseconds now.
pub fn now_ms() -> i64 {
    at_ms(crate::board::format::now_ms())
}

/// A moment P4 keeps, as the signed milliseconds the inspector's formats take.
pub fn at_ms(at: EpochMs) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX)
}

// ---- The board task (P12) ----------------------------------------------------------------

/// What the task card shows of a board task.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCard {
    pub id: String,
    pub title: String,
    pub priority: u8,
    /// `doing` or `review`: the only columns a pane's task can be in.
    pub column: String,
    /// When it entered `doing`, epoch milliseconds.
    pub doing_since: Option<i64>,
    pub comments: usize,
}

impl TaskCard {
    pub fn of(task: &Task) -> TaskCard {
        TaskCard {
            id: task.id.clone(),
            title: task.title.clone(),
            priority: task.priority,
            column: task.column.clone(),
            doing_since: task.doing_since.map(at_ms),
            comments: task.comments.len(),
        }
    }

    pub fn in_review(&self) -> bool {
        self.column == "review"
    }
}

/// The task an agent in this pane is on, as far as the board can tell
/// (`taskForPane` over `useBoardTasks(workspace.cwd)`): an open task whose
/// assignee resolves to this pane.
pub fn task_for_pane(m: &Model, ws: &Workspace, pane: &PaneId) -> Option<TaskCard> {
    let state = m.state();
    m.live
        .board
        .task_for_pane(
            ws,
            pane,
            &state.workspaces,
            state.active_workspace_id.as_deref(),
        )
        .map(TaskCard::of)
}

// ---- Shortcuts (P7) ----------------------------------------------------------------------

/// The binding of a command as the settings store it (`"Ctrl+Alt+I"`), or
/// `None` when the keymap has nothing for it: the text `keycap::binding` and
/// `IconBtn::keys` take, and true after a rebind.
pub fn shortcut(ctx: Ctx<'_>, command: CommandId) -> Option<String> {
    ctx.keymap.first_binding(command).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::state::types::{Pane, SplitNode};

    fn pane(id: &str) -> Pane {
        Pane {
            id: id.into(),
            name: id.to_uppercase(),
            ..Pane::default()
        }
    }

    fn workspace(ids: &[&str], tree: Option<SplitNode>) -> Workspace {
        Workspace {
            id: "w".into(),
            panes: ids.iter().map(|id| pane(id)).collect(),
            tree,
            ..Workspace::default()
        }
    }

    fn leaf(id: &str) -> SplitNode {
        SplitNode::Pane { id: id.into() }
    }

    fn row(a: SplitNode, b: SplitNode) -> SplitNode {
        SplitNode::Split {
            dir: crate::state::SplitDir::Row,
            ratio: 0.5,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    fn names(ws: &Workspace) -> Vec<&str> {
        reading_order(ws)
            .into_iter()
            .map(|pane| pane.id.as_str())
            .collect()
    }

    #[test]
    fn the_reading_order_follows_the_tree_not_the_spawning() {
        let ws = workspace(
            &["a", "b", "c"],
            Some(row(row(leaf("c"), leaf("a")), leaf("b"))),
        );
        assert_eq!(names(&ws), ["c", "a", "b"]);
    }

    #[test]
    fn a_stale_tree_is_repaired_keeping_what_it_knew() {
        // "x" no longer exists, "d" is not in the tree yet, "a" shows twice.
        let ws = workspace(
            &["a", "b", "d"],
            Some(row(row(leaf("b"), leaf("x")), row(leaf("a"), leaf("a")))),
        );
        assert_eq!(names(&ws), ["b", "a", "d"]);
        assert_eq!(names(&workspace(&["a", "b"], None)), ["a", "b"]);
    }

    #[test]
    fn the_focused_pane_is_found_with_its_place_and_falls_back_to_the_first() {
        let ws = workspace(
            &["a", "b", "c"],
            Some(row(row(leaf("c"), leaf("a")), leaf("b"))),
        );
        let mut m = Model::default();
        m.ui.focus_by_workspace.insert("w".into(), "a".into());
        let (found, place) = focused_pane(&m, &ws).unwrap();
        assert_eq!((found.id.as_str(), place), ("a", 2));

        m.ui.focus_by_workspace.insert("w".into(), "gone".into());
        let (found, place) = focused_pane(&m, &ws).unwrap();
        assert_eq!((found.id.as_str(), place), ("c", 1));

        assert!(focused_pane(&m, &workspace(&[], None)).is_none());
    }

    #[test]
    fn a_pane_that_reported_nothing_has_an_empty_insight_and_no_activity() {
        let m = Model::default();
        let insight = insight(&m, &"p".to_string());
        assert!(insight.edited_files.is_empty() && insight.events.is_empty());
        assert!(insight.session_since.is_none() && insight.cost_usd.is_none());
        assert!(activity(&m, &"p".to_string()).is_none());
        assert!(plan_limits(&m).is_none());
    }

    #[test]
    fn a_board_task_becomes_a_card() {
        let task = Task {
            id: "3f2a9c1e".into(),
            title: "Fix the login".into(),
            description: String::new(),
            column: "doing".into(),
            priority: 1,
            assignee: Some("ava".into()),
            labels: Vec::new(),
            comments: Vec::new(),
            depends_on: Vec::new(),
            created_at: 1,
            updated_at: 2,
            order: 0.0,
            doing_since: Some(1_700_000_000_000),
            owns: Vec::new(),
            attempts: 0,
            blocked: None,
        };
        let card = TaskCard::of(&task);
        assert_eq!(card.doing_since, Some(1_700_000_000_000));
        assert!(!card.in_review());
        assert_eq!(card.comments, 0);
    }

    #[test]
    fn a_pane_with_no_task_on_the_board_has_no_card() {
        let m = Model::default();
        let ws = workspace(&["a"], None);
        assert_eq!(task_for_pane(&m, &ws, &"a".to_string()), None);
    }
}
