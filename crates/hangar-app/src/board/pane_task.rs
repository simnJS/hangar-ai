//! The task an agent in a pane is on (`taskForPane` of `src/lib/paneTask.ts`,
//! tested by `paneTask.test.ts`).

use crate::state::Workspace;

use super::model::Task;
use crate::panes_logic::with_finder;

/// Work in progress first, then work waiting for review; the freshest of each.
fn rank(task: &Task) -> u8 {
    match task.column.as_str() {
        "doing" => 0,
        "review" => 1,
        _ => 2,
    }
}

/// The task an agent in this pane is on, as far as the board can tell: an open
/// task whose assignee resolves to this pane (`panes_logic::find_agent_pane`, which only answers
/// when the name is unambiguous). Done and unassigned tasks never count.
pub fn task_for_pane<'a>(
    tasks: Option<&'a [Task]>,
    pane_id: &str,
    workspaces: &[Workspace],
    active_workspace_id: Option<&str>,
) -> Option<&'a Task> {
    let tasks = tasks?;
    // Asked from views, so after every message: the names are resolved from
    // the memo (`panes_logic::with_finder`), not split again for each task.
    with_finder(workspaces, |finder| {
        tasks
            .iter()
            .filter(|task| {
                let Some(assignee) = &task.assignee else {
                    return false;
                };
                if task.column != "doing" && task.column != "review" {
                    return false;
                }
                finder
                    .find(assignee, active_workspace_id)
                    .is_some_and(|target| target.pane_id == pane_id)
            })
            .min_by(|a, b| {
                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| b.updated_at.cmp(&a.updated_at))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::model::test_task;
    use crate::state::{AgentId, Pane};

    fn workspaces() -> Vec<Workspace> {
        let pane = |id: &str, name: &str| Pane {
            id: id.into(),
            name: name.into(),
            agent: AgentId::Claude,
            ..Pane::default()
        };
        vec![Workspace {
            id: "ws".into(),
            name: "Hangar".into(),
            panes: vec![pane("p1", "Ava"), pane("p2", "Ben")],
            ..Workspace::default()
        }]
    }

    fn task(id: &str, edit: impl FnOnce(&mut Task)) -> Task {
        let mut task = Task {
            column: "doing".into(),
            created_at: 0,
            updated_at: 0,
            order: 0.0,
            ..test_task(id)
        };
        edit(&mut task);
        task
    }

    fn found(tasks: &[Task], pane: &str) -> Option<String> {
        task_for_pane(Some(tasks), pane, &workspaces(), Some("ws")).map(|task| task.id.clone())
    }

    #[test]
    fn finds_the_task_an_agent_claimed_under_its_panes_name() {
        let tasks = [
            task("t1", |t| t.assignee = Some("Ava (Hangar)".into())),
            task("t2", |t| t.assignee = Some("Ben".into())),
        ];
        assert_eq!(found(&tasks, "p1").as_deref(), Some("t1"));
        assert_eq!(found(&tasks, "p2").as_deref(), Some("t2"));
    }

    #[test]
    fn prefers_work_in_progress_over_work_in_review_then_the_freshest() {
        let tasks = [
            task("review", |t| {
                t.assignee = Some("Ava".into());
                t.column = "review".into();
                t.updated_at = 9;
            }),
            task("old", |t| {
                t.assignee = Some("Ava".into());
                t.updated_at = 1;
            }),
            task("new", |t| {
                t.assignee = Some("Ava".into());
                t.updated_at = 5;
            }),
        ];
        assert_eq!(found(&tasks, "p1").as_deref(), Some("new"));
    }

    #[test]
    fn ignores_done_unassigned_and_unresolvable_tasks() {
        let tasks = [
            task("done", |t| {
                t.assignee = Some("Ava".into());
                t.column = "done".into();
            }),
            task("free", |_| {}),
            task("stranger", |t| t.assignee = Some("someone-else".into())),
        ];
        assert_eq!(found(&tasks, "p1"), None);
        assert!(task_for_pane(None, "p1", &workspaces(), Some("ws")).is_none());
    }
}
