//! What the ported tests build their worlds from: the TS tests' `pane`,
//! `workspace` and `task` helpers, and a clock the tests move by hand.

use hangar_core::board::{Blocked, Comment, Task};

use crate::state::types::{AgentId, Pane, PaneRole, Workspace};

pub fn pane(id: &str, name: &str, role: Option<PaneRole>) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent: AgentId::Claude,
        role,
        ..Pane::default()
    }
}

pub fn shell(id: &str, name: &str) -> Pane {
    Pane {
        agent: AgentId::Shell,
        ..pane(id, name, None)
    }
}

pub fn worker(id: &str, name: &str, task: &str) -> Pane {
    Pane {
        task_id: Some(task.into()),
        ..pane(id, name, Some(PaneRole::Worker))
    }
}

/// A workspace in `C:\dev\<id>`, as the TS tests had them.
pub fn workspace(id: &str, name: &str, panes: Vec<Pane>) -> Workspace {
    Workspace {
        id: id.into(),
        name: name.into(),
        cwd: format!("C:\\dev\\{id}"),
        panes,
        ..Workspace::default()
    }
}

pub fn task(id: &str) -> Task {
    Task {
        id: id.into(),
        title: format!("Task {id}"),
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
        progress: None,
    }
}

pub fn comment(id: &str, author: &str, text: &str, created_at: u64) -> Comment {
    Comment {
        id: id.into(),
        author: author.into(),
        text: text.into(),
        created_at,
    }
}

pub fn blocked(reason: &str, by: &str, human_only: bool) -> Blocked {
    Blocked {
        reason: reason.into(),
        by: by.into(),
        since: 1,
        recheck: None,
        human_only,
        reminded: None,
    }
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// The fake clock of the TS tests: starts at 1,000,000 ms.
pub const START: u64 = 1_000_000;

/// Something with timers a test moves the clock over.
pub trait Timed {
    fn next_wake(&self) -> Option<u64>;
    /// Fires the earliest timer due by `now`.
    fn fire_due(&mut self, now: u64);
}

/// Moves `t` by `ms`, firing every timer that falls due on the way, one at a
/// time and in order, each at its own time (`fakeTime().advance`).
pub fn advance(t: &mut u64, ms: u64, timed: &mut impl Timed) {
    let end = *t + ms;
    let mut fired = 0;
    while let Some(at) = timed.next_wake() {
        if at > end {
            break;
        }
        // A timer that keeps re-arming itself at once is a hot loop in the app.
        fired += 1;
        assert!(fired <= 1_000, "timers re-arm themselves without end");
        *t = (*t).max(at);
        timed.fire_due(*t);
    }
    *t = end;
}
