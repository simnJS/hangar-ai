//! The board's data and the rules that decide with it (`src/lib/board.ts`,
//! tested by `board.test.ts`).
//!
//! The task types are hangar-core's own (`hangar_core::board`), the very ones
//! `board_load` answers with and the agents' API writes, so there is nothing
//! to convert between the file, the core and the window. What this module adds
//! is what the TS file added on top of the wire types: the four columns as a
//! type, the blocked-state rules, the patches the window sends.

use std::collections::HashMap;

use hangar_core::board::{BlockRequest, TaskPatch, UnblockRequest};
use hangar_i18n::Key;

// What the dialog and the other packages name; some have no user yet.
#[allow(unused_imports)]
pub use hangar_core::board::{
    Blocked, Comment as TaskComment, MAX_BLOCK_REASON, NEEDS_YOU_LABEL, Task,
};

/// The four columns, in the board's order (`BOARD_COLUMNS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Column {
    Todo,
    Doing,
    Review,
    Done,
}

impl Column {
    /// `BOARD_COLUMNS`: the full board's order.
    pub const ALL: [Column; 4] = [Column::Todo, Column::Doing, Column::Review, Column::Done];

    /// `DOCK_ORDER`: beside the terminals the work under way matters most, so
    /// what is in progress and what waits for a review come first.
    pub const DOCK_ORDER: [Column; 4] = [Column::Doing, Column::Review, Column::Todo, Column::Done];

    /// The id the board file and the agents' API use.
    pub fn id(self) -> &'static str {
        match self {
            Column::Todo => "todo",
            Column::Doing => "doing",
            Column::Review => "review",
            Column::Done => "done",
        }
    }

    pub fn from_id(id: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|column| column.id() == id)
    }

    /// The column a task sits in. `None` for a name this version does not
    /// know (a board edited by hand), which no column shows, as before.
    pub fn of(task: &Task) -> Option<Column> {
        Column::from_id(&task.column)
    }

    /// `columnKey`: the column's name in the interface.
    pub fn label_key(self) -> Key {
        match self {
            Column::Todo => Key::BoardTodo,
            Column::Doing => Key::BoardDoing,
            Column::Review => Key::BoardReview,
            Column::Done => Key::BoardDone,
        }
    }
}

/// Whether the task waits on something outside the board.
#[cfg(test)]
pub fn is_blocked(task: &Task) -> bool {
    task.blocked.is_some()
}

/// `isoDay` for a calendar date: the day as `board.rs` writes it, padded.
/// Today's is `hangar_core::board::today()`.
#[cfg(test)]
pub fn iso_day(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

/// What is wrong with a recheck day typed for a new block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecheckProblem {
    /// Not a day written YYYY-MM-DD, or not a day of the calendar.
    Format,
    /// A day before today.
    Past,
}

impl RecheckProblem {
    /// The sentence that says it (`board.blockRecheckPast`, `…Format`).
    pub fn message_key(self) -> Key {
        match self {
            RecheckProblem::Past => Key::BoardBlockRecheckPast,
            RecheckProblem::Format => Key::BoardBlockRecheckFormat,
        }
    }
}

fn is_leap(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The year, month and day of `YYYY-MM-DD`, when it is a day of the calendar.
fn parse_day(text: &str) -> Option<(u32, u32, u32)> {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = text.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    // The TS built a JavaScript `Date` to check the day, which reads the years
    // 0 to 99 as 1900 to 1999 and so refused them: the same here.
    if year < 100 || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some((year, month, day))
}

/// What is wrong with a recheck day typed for a new block; `None` when it is
/// fine. Nothing at all is fine too: the block has no recheck then.
pub fn recheck_problem(day: &str, today: &str) -> Option<RecheckProblem> {
    let text = day.trim();
    if text.is_empty() {
        return None;
    }
    if parse_day(text).is_none() {
        return Some(RecheckProblem::Format);
    }
    // ISO days sort as text, which is what `board.rs` compares them as.
    (text < today).then_some(RecheckProblem::Past)
}

/// Why a card cannot leave todo, when it is blocked: the reason to show. A
/// move out of todo would leave the state behind on a task that is not waiting
/// any more, or clear it silently; the task has to be unblocked first.
pub fn blocked_move_refusal(task: &Task, to: Column) -> Option<&str> {
    let blocked = task.blocked.as_ref()?;
    (to != Column::Todo).then_some(blocked.reason.as_str())
}

/// The patch that blocks a task from the window: the person is the caller.
pub fn block_patch(reason: &str, recheck: &str, human_only: bool) -> TaskPatch {
    let day = recheck.trim();
    TaskPatch {
        block: Some(BlockRequest {
            reason: reason.trim().to_owned(),
            recheck: (!day.is_empty()).then(|| day.to_owned()),
            human_only,
            by: Some("user".into()),
            person: true,
        }),
        ..TaskPatch::default()
    }
}

/// The patch that unblocks a task from the window.
pub fn unblock_patch(note: &str) -> TaskPatch {
    let text = note.trim();
    TaskPatch {
        unblock: Some(UnblockRequest {
            note: (!text.is_empty()).then(|| text.to_owned()),
            by: Some("user".into()),
            person: true,
        }),
        ..TaskPatch::default()
    }
}

/// A dependency still holding a task back.
#[derive(Debug, Clone, Copy)]
pub enum Blocker<'a> {
    /// A task that is not done yet.
    Task(&'a Task),
    /// An id no task has any more: a deleted dependency blocks forever, which
    /// is exactly what needs showing.
    #[allow(dead_code)] // The card says "missing" without the id; the tests read it.
    Missing(&'a str),
}

/// The dependencies still holding a task back, as `next_task` sees them: any
/// id that is not a done task, including one that no longer exists.
pub fn blockers_of<'a>(task: &'a Task, by_id: &HashMap<&str, &'a Task>) -> Vec<Blocker<'a>> {
    task.depends_on
        .iter()
        .filter_map(|id| match by_id.get(id.as_str()) {
            None => Some(Blocker::Missing(id)),
            Some(dep) if dep.column == "done" => None,
            Some(dep) => Some(Blocker::Task(dep)),
        })
        .collect()
}

/// The tasks by id, for `blockers_of` and the dependency list.
pub fn index_by_id(tasks: &[Task]) -> HashMap<&str, &Task> {
    tasks.iter().map(|task| (task.id.as_str(), task)).collect()
}

/// A task for the tests of this package: every field at its default.
#[cfg(test)]
pub(crate) fn test_task(id: &str) -> Task {
    Task {
        id: id.into(),
        title: id.into(),
        description: String::new(),
        column: "todo".into(),
        priority: 1,
        assignee: None,
        labels: Vec::new(),
        comments: Vec::new(),
        depends_on: Vec::new(),
        created_at: 1,
        updated_at: 1,
        order: 1.0,
        doing_since: None,
        owns: Vec::new(),
        attempts: 0,
        blocked: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked() -> Blocked {
        Blocked {
            reason: "npm release".into(),
            by: "Eli".into(),
            since: 1,
            recheck: None,
            human_only: false,
            reminded: None,
        }
    }

    fn blocked_task() -> Task {
        Task {
            blocked: Some(blocked()),
            ..test_task("a")
        }
    }

    #[test]
    fn reads_an_old_task_as_not_blocked() {
        assert!(!is_blocked(&test_task("a")));
        assert!(is_blocked(&blocked_task()));
    }

    #[test]
    fn iso_day_is_the_local_day_padded() {
        assert_eq!(iso_day(2026, 1, 5), "2026-01-05");
        assert_eq!(iso_day(2026, 11, 2), "2026-11-02");
    }

    #[test]
    fn recheck_accepts_nothing_today_and_later() {
        let today = "2026-11-02";
        assert_eq!(recheck_problem("", today), None);
        assert_eq!(recheck_problem("   ", today), None);
        assert_eq!(recheck_problem("2026-11-02", today), None);
        assert_eq!(recheck_problem("2027-01-01", today), None);
    }

    #[test]
    fn recheck_refuses_the_past_and_what_is_not_a_day() {
        let today = "2026-11-02";
        assert_eq!(
            recheck_problem("2026-11-01", today),
            Some(RecheckProblem::Past)
        );
        assert_eq!(
            recheck_problem("next week", today),
            Some(RecheckProblem::Format)
        );
        assert_eq!(
            recheck_problem("2026-02-30", today),
            Some(RecheckProblem::Format)
        );
    }

    #[test]
    fn recheck_knows_leap_years_and_the_shape_of_a_day() {
        let today = "2026-01-01";
        assert_eq!(recheck_problem("2028-02-29", today), None);
        assert_eq!(
            recheck_problem("2027-02-29", today),
            Some(RecheckProblem::Format)
        );
        assert_eq!(
            recheck_problem("2026-13-01", today),
            Some(RecheckProblem::Format)
        );
        assert_eq!(
            recheck_problem("2026-1-5", today),
            Some(RecheckProblem::Format)
        );
        // A day with a sign or a letter in it is no day.
        assert_eq!(
            recheck_problem("2026-+1-05", today),
            Some(RecheckProblem::Format)
        );
    }

    #[test]
    fn a_blocked_card_gives_its_reason_when_it_leaves_todo() {
        let task = blocked_task();
        assert_eq!(
            blocked_move_refusal(&task, Column::Doing),
            Some("npm release")
        );
        assert_eq!(
            blocked_move_refusal(&task, Column::Done),
            Some("npm release")
        );
    }

    #[test]
    fn a_blocked_card_may_stay_and_a_free_one_may_go() {
        assert_eq!(blocked_move_refusal(&blocked_task(), Column::Todo), None);
        assert_eq!(blocked_move_refusal(&test_task("a"), Column::Doing), None);
    }

    #[test]
    fn blocks_as_the_person_trimmed_with_the_recheck_only_when_given() {
        let patch = block_patch("  wait  ", " ", true);
        let block = patch.block.expect("a block");
        assert_eq!(block.reason, "wait");
        assert_eq!(block.recheck, None);
        assert!(block.human_only);
        assert_eq!(block.by.as_deref(), Some("user"));
        assert!(block.person);
        assert!(patch.unblock.is_none());

        let dated = block_patch("wait", "2026-12-01", false);
        assert_eq!(
            dated.block.and_then(|block| block.recheck).as_deref(),
            Some("2026-12-01")
        );
    }

    #[test]
    fn unblocks_as_the_person_with_the_note_only_when_given() {
        let plain = unblock_patch(" ").unblock.expect("an unblock");
        assert_eq!(plain.note, None);
        assert_eq!(plain.by.as_deref(), Some("user"));
        assert!(plain.person);
        assert_eq!(
            unblock_patch("out").unblock.and_then(|u| u.note).as_deref(),
            Some("out")
        );
    }

    #[test]
    fn columns_round_trip_through_their_ids() {
        for column in Column::ALL {
            assert_eq!(Column::from_id(column.id()), Some(column));
        }
        assert_eq!(Column::from_id("archive"), None);
        assert_eq!(Column::of(&test_task("a")), Some(Column::Todo));
    }

    #[test]
    fn blockers_are_the_dependencies_not_done_and_the_deleted_ones() {
        let done = Task {
            column: "done".into(),
            ..test_task("done")
        };
        let doing = Task {
            column: "doing".into(),
            ..test_task("doing")
        };
        let waiting = Task {
            depends_on: vec!["done".into(), "doing".into(), "gone".into()],
            ..test_task("waiting")
        };
        let tasks = vec![done, doing, waiting];
        let by_id = index_by_id(&tasks);
        let blockers = blockers_of(&tasks[2], &by_id);
        assert_eq!(blockers.len(), 2);
        assert!(matches!(blockers[0], Blocker::Task(task) if task.id == "doing"));
        assert!(matches!(blockers[1], Blocker::Missing("gone")));
        assert!(blockers_of(&tasks[0], &by_id).is_empty());
    }
}
