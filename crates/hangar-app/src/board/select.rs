//! Which tasks the board shows, and in what order: the filters, the filters'
//! options and the sort of a column (the logic of `BoardView.tsx` that has no
//! drawing in it).
//!
//! A filter's value is a string, as the TS kept it, because a drop-down's
//! options are strings: [`ANY`] for no filter, and for the assignee [`NOBODY`]
//! for the tasks nobody holds. Neither can be a label or an agent's name.

use std::cmp::Ordering;

use super::model::{Column, Task};

/// No filter on this dimension.
pub const ANY: &str = "";
/// The assignee filter's "unassigned". A control character, so that no agent
/// can be called that.
pub const NOBODY: &str = "\u{0}";

/// The state filter's two values besides [`ANY`], by what the task's own
/// `blocked` says (not by its dependencies).
pub const STATE_BLOCKED: &str = "blocked";
pub const STATE_FREE: &str = "free";

/// The board's three filters, and-combined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    pub label: String,
    pub assignee: String,
    pub state: String,
}

impl Filters {
    /// Whether any filter hides cards.
    pub fn active(&self) -> bool {
        !self.label.is_empty() || !self.assignee.is_empty() || !self.state.is_empty()
    }

    pub fn matches(&self, task: &Task) -> bool {
        (self.label.is_empty() || task.labels.contains(&self.label))
            && (self.assignee.is_empty()
                || if self.assignee == NOBODY {
                    task.assignee.is_none()
                } else {
                    task.assignee.as_deref() == Some(self.assignee.as_str())
                })
            && (self.state.is_empty() || (self.state == STATE_BLOCKED) == task.blocked.is_some())
    }

    pub fn clear(&mut self) {
        *self = Filters::default();
    }
}

/// `localeCompare` without a collation table: case and accents do not decide
/// the order, ties fall back on the text itself.
fn compare_names(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn sorted_unique<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut all: Vec<String> = values.map(str::to_owned).collect();
    all.sort_by(|a, b| compare_names(a, b));
    all.dedup();
    all
}

/// The labels on the board, sorted. A filter whose value left the board stays
/// listed, so the filter never shows a value it has no option for.
pub fn labels(tasks: &[Task], filter: &str) -> Vec<String> {
    sorted_unique(
        tasks
            .iter()
            .flat_map(|task| task.labels.iter().map(String::as_str))
            .chain(std::iter::once(filter))
            .filter(|label| !label.is_empty()),
    )
}

/// The agents holding tasks, sorted; the current filter stays listed.
pub fn assignees(tasks: &[Task], filter: &str) -> Vec<String> {
    sorted_unique(
        tasks
            .iter()
            .filter_map(|task| task.assignee.as_deref())
            .chain(std::iter::once(filter))
            .filter(|name| !name.is_empty() && *name != NOBODY),
    )
}

/// The tasks that pass the filters, in the board's order.
pub fn shown<'a>(tasks: &'a [Task], filters: &Filters) -> Vec<&'a Task> {
    tasks.iter().filter(|task| filters.matches(task)).collect()
}

/// A column's cards: the most urgent first, then in the order they were put
/// there. There is no manual reordering, and a drag never changes `order`.
pub fn column_items<'a>(shown: &[&'a Task], column: Column) -> Vec<&'a Task> {
    let mut items: Vec<&Task> = shown
        .iter()
        .copied()
        .filter(|task| task.column == column.id())
        .collect();
    items.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.order.total_cmp(&b.order))
    });
    items
}

/// What the dock's sub-line counts: work under way and work waiting for a
/// review.
pub fn active_count(shown: &[&Task]) -> usize {
    shown
        .iter()
        .filter(|task| task.column == "doing" || task.column == "review")
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::model::{Blocked, test_task};

    fn task(id: &str, edit: impl FnOnce(&mut Task)) -> Task {
        let mut task = test_task(id);
        edit(&mut task);
        task
    }

    fn blocked() -> Option<Blocked> {
        Some(Blocked {
            reason: "release".into(),
            by: "user".into(),
            since: 1,
            recheck: None,
            human_only: false,
            reminded: None,
        })
    }

    fn ids(tasks: &[&Task]) -> Vec<String> {
        tasks.iter().map(|task| task.id.clone()).collect()
    }

    #[test]
    fn no_filter_shows_everything() {
        let tasks = vec![test_task("a"), test_task("b")];
        assert!(!Filters::default().active());
        assert_eq!(shown(&tasks, &Filters::default()).len(), 2);
    }

    #[test]
    fn filters_combine_with_and() {
        let tasks = vec![
            task("both", |t| {
                t.labels = vec!["bug".into()];
                t.assignee = Some("Ava".into());
            }),
            task("label-only", |t| t.labels = vec!["bug".into()]),
            task("agent-only", |t| t.assignee = Some("Ava".into())),
        ];
        let filters = Filters {
            label: "bug".into(),
            assignee: "Ava".into(),
            ..Filters::default()
        };
        assert!(filters.active());
        assert_eq!(ids(&shown(&tasks, &filters)), vec!["both"]);
    }

    #[test]
    fn the_assignee_filter_can_ask_for_nobody() {
        let tasks = vec![
            task("held", |t| t.assignee = Some("Ava".into())),
            test_task("free"),
        ];
        let filters = Filters {
            assignee: NOBODY.into(),
            ..Filters::default()
        };
        assert_eq!(ids(&shown(&tasks, &filters)), vec!["free"]);
    }

    #[test]
    fn the_state_filter_follows_the_blocked_state_not_the_dependencies() {
        let tasks = vec![
            task("blocked", |t| t.blocked = blocked()),
            task("waiting-on-another", |t| {
                t.depends_on = vec!["blocked".into()]
            }),
        ];
        let only_blocked = Filters {
            state: STATE_BLOCKED.into(),
            ..Filters::default()
        };
        let only_free = Filters {
            state: STATE_FREE.into(),
            ..Filters::default()
        };
        assert_eq!(ids(&shown(&tasks, &only_blocked)), vec!["blocked"]);
        assert_eq!(ids(&shown(&tasks, &only_free)), vec!["waiting-on-another"]);
    }

    #[test]
    fn lists_the_labels_sorted_once_and_keeps_a_filter_that_left_the_board() {
        let tasks = vec![
            task("a", |t| t.labels = vec!["ui".into(), "Bug".into()]),
            task("b", |t| t.labels = vec!["bug".into(), "ui".into()]),
        ];
        assert_eq!(labels(&tasks, ""), vec!["Bug", "bug", "ui"]);
        assert_eq!(labels(&tasks, "gone"), vec!["Bug", "bug", "gone", "ui"]);
    }

    #[test]
    fn lists_the_agents_that_hold_tasks_and_never_the_sentinel() {
        let tasks = vec![
            task("a", |t| t.assignee = Some("Max".into())),
            task("b", |t| t.assignee = Some("ava".into())),
            test_task("c"),
        ];
        assert_eq!(assignees(&tasks, ""), vec!["ava", "Max"]);
        assert_eq!(assignees(&tasks, NOBODY), vec!["ava", "Max"]);
        assert_eq!(assignees(&tasks, "Zed"), vec!["ava", "Max", "Zed"]);
    }

    #[test]
    fn sorts_a_column_by_priority_then_order() {
        let tasks = vec![
            task("late-low", |t| {
                t.priority = 1;
                t.order = 5.0;
            }),
            task("early-low", |t| {
                t.priority = 1;
                t.order = 1.0;
            }),
            task("urgent", |t| {
                t.priority = 3;
                t.order = 9.0;
            }),
            task("elsewhere", |t| t.column = "done".into()),
        ];
        let all = shown(&tasks, &Filters::default());
        assert_eq!(
            ids(&column_items(&all, Column::Todo)),
            vec!["urgent", "early-low", "late-low"]
        );
        assert_eq!(ids(&column_items(&all, Column::Done)), vec!["elsewhere"]);
        assert!(column_items(&all, Column::Review).is_empty());
    }

    #[test]
    fn counts_the_work_under_way_and_in_review() {
        let tasks = vec![
            task("a", |t| t.column = "doing".into()),
            task("b", |t| t.column = "review".into()),
            task("c", |t| t.column = "done".into()),
            test_task("d"),
        ];
        assert_eq!(active_count(&shown(&tasks, &Filters::default())), 2);
    }
}
