//! The boards of the workspaces' folders, read once and shared
//! (`src/lib/paneTask.ts`: `useBoardTasks`, `useBoardsOf`).
//!
//! Everything that only reads a board (the toolbar's activity line, the
//! inspector, the orchestration badge, the needs-you list) reads it from here
//! instead of opening `board.json` on every change. The board view and dock
//! draw from the same cache, so a task edited in the dialog shows everywhere
//! at once.
//!
//! # For the packages that read
//!
//! `Model.live.board` is a [`Live`]. It holds the board of the folder of every
//! workspace in the state (two workspaces on one folder share one), starting
//! when the state is loaded and following the workspaces as they come and go.
//! Each board is `None` until its first read answers, which a view shows as
//! "not loaded" rather than as an empty board.
//!
//! ```ignore
//! let live = &ctx.model.live.board;
//! let tasks: Option<&[Task]> = live.tasks(&workspace.cwd);
//! let mine = board::pane_task::task_for_pane(tasks, &pane.id, workspaces, active);
//! let boards = live.boards_of(&needs_you_folders);   // `useBoardsOf`
//! ```
//!
//! The cache is part of the model, so only code that has the model reads it:
//! the engines (P18–P20) run on tokio without it and call
//! `hangar_core::board_load` themselves, as their TS did (`workerControl`,
//! `orchestratorBrief`, `projects`).
//!
//! # How it stays current
//!
//! A read is `hangar_core::board_load` off the UI thread. It is repeated on
//! every `CoreEvent::BoardChanged`, whatever folder the event names: worktrees
//! alias one board, so an agent writing from a linked worktree carries a path
//! the window has never heard of, and a reload is one small file read. A read
//! asked for while one is out is folded into a single repeat, so a burst of
//! changes costs two reads of a folder, not one per change.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::state::{AppState, Workspace};

use super::model::Task;
use super::pane_task;

/// The shared read-model of the board package (`Model.live.board`).
#[derive(Debug, Default)]
pub struct Live {
    boards: HashMap<String, Option<Arc<Vec<Task>>>>,
    /// Folders a read is out for.
    reading: HashSet<String>,
    /// Folders whose board changed while a read was out: read again after it.
    stale: HashSet<String>,
}

impl Live {
    /// The tasks of the board in `cwd`, or `None` until they are read (or when
    /// no workspace has that folder).
    pub fn tasks(&self, cwd: &str) -> Option<&[Task]> {
        self.boards.get(cwd)?.as_ref().map(|tasks| tasks.as_slice())
    }

    /// The same, shared, for a task that will use it later.
    pub fn shared(&self, cwd: &str) -> Option<Arc<Vec<Task>>> {
        self.boards.get(cwd)?.clone()
    }

    /// Several boards at once, by folder: `useBoardsOf`. A folder nobody asked
    /// the cache to keep is `None`, like one still being read.
    pub fn boards_of<'a>(&'a self, cwds: &[String]) -> HashMap<String, Option<&'a [Task]>> {
        cwds.iter()
            .map(|cwd| (cwd.clone(), self.tasks(cwd)))
            .collect()
    }

    /// Whether the first read of this folder's board has answered.
    pub fn is_loaded(&self, cwd: &str) -> bool {
        self.tasks(cwd).is_some()
    }

    /// The task an agent in this pane is on (`taskForPane`), from the board of
    /// the pane's workspace.
    pub fn task_for_pane(
        &self,
        workspace: &Workspace,
        pane_id: &str,
        workspaces: &[Workspace],
        active_workspace_id: Option<&str>,
    ) -> Option<&Task> {
        pane_task::task_for_pane(
            self.tasks(&workspace.cwd),
            pane_id,
            workspaces,
            active_workspace_id,
        )
    }

    /// Every folder the cache holds a board for, sorted.
    pub fn folders(&self) -> Vec<&str> {
        let mut folders: Vec<&str> = self.boards.keys().map(String::as_str).collect();
        folders.sort_unstable();
        folders
    }

    // ---- The package's own side: what its `update` does ------------------

    /// Follows the folders the workspaces have: boards of folders that came
    /// are asked for, boards of folders that went are dropped. Answers the
    /// folders to read now.
    pub(super) fn follow(&mut self, wanted: &[String]) -> Vec<String> {
        let keep: HashSet<&str> = wanted.iter().map(String::as_str).collect();
        self.boards.retain(|cwd, _| keep.contains(cwd.as_str()));
        self.reading.retain(|cwd| keep.contains(cwd.as_str()));
        self.stale.retain(|cwd| keep.contains(cwd.as_str()));
        let mut fresh = Vec::new();
        for cwd in wanted {
            if !self.boards.contains_key(cwd) {
                self.boards.insert(cwd.clone(), None);
                fresh.push(cwd.clone());
            }
        }
        fresh
    }

    /// A board needs reading. Answers whether to start the read now; a read
    /// already out is asked to repeat instead.
    pub(super) fn request(&mut self, cwd: &str) -> bool {
        if !self.boards.contains_key(cwd) {
            return false;
        }
        if self.reading.contains(cwd) {
            self.stale.insert(cwd.to_owned());
            return false;
        }
        self.reading.insert(cwd.to_owned());
        true
    }

    /// Every folder held, to read again: the answer to a change event, whose
    /// folder is not compared (worktrees alias one board).
    pub(super) fn request_all(&mut self) -> Vec<String> {
        let folders: Vec<String> = self.boards.keys().cloned().collect();
        folders
            .into_iter()
            .filter(|cwd| self.request(cwd))
            .collect()
    }

    /// A read answered. A failed read leaves the board as it was, but a first
    /// read that fails is an empty board, as the view showed it. Answers
    /// whether the board changed meanwhile, so it must be read again at once.
    pub(super) fn answer(&mut self, cwd: &str, tasks: Option<Vec<Task>>) -> bool {
        self.reading.remove(cwd);
        if let Some(slot) = self.boards.get_mut(cwd) {
            match tasks {
                Some(tasks) => *slot = Some(Arc::new(tasks)),
                None if slot.is_none() => *slot = Some(Arc::new(Vec::new())),
                None => {}
            }
        }
        let again = self.stale.remove(cwd);
        if again {
            self.reading.insert(cwd.to_owned());
        }
        again
    }

    /// Whether any held board has a task with a recheck day: the reason to
    /// read again now and then, since a day that passes unblocks the task when
    /// the board is next read (`board.rs`).
    pub(super) fn has_rechecks(&self) -> bool {
        self.boards
            .values()
            .flatten()
            .any(|tasks| tasks.iter().any(|task| recheck_day(task).is_some()))
    }
}

fn recheck_day(task: &Task) -> Option<&str> {
    task.blocked.as_ref()?.recheck.as_deref()
}

/// The folders of the workspaces, each once, sorted: the boards to hold.
pub fn folders_of(state: &AppState) -> Vec<String> {
    let mut folders: Vec<String> = state.workspaces.iter().map(|ws| ws.cwd.clone()).collect();
    folders.retain(|cwd| !cwd.is_empty());
    folders.sort_unstable();
    folders.dedup();
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::model::{Blocked, test_task};

    fn folders(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_board_is_not_loaded_until_its_first_read_answers() {
        let mut live = Live::default();
        assert_eq!(live.follow(&folders(&["/a"])), folders(&["/a"]));
        assert!(live.tasks("/a").is_none());
        assert!(!live.is_loaded("/a"));
        assert!(live.request("/a"));
        live.answer("/a", Some(vec![test_task("t")]));
        assert_eq!(live.tasks("/a").map(<[Task]>::len), Some(1));
        assert!(live.is_loaded("/a"));
    }

    #[test]
    fn follows_the_folders_as_workspaces_come_and_go() {
        let mut live = Live::default();
        live.follow(&folders(&["/a", "/b"]));
        // Only the new folder is read.
        assert_eq!(live.follow(&folders(&["/b", "/c"])), folders(&["/c"]));
        assert_eq!(live.folders(), vec!["/b", "/c"]);
        assert!(live.tasks("/a").is_none());
    }

    #[test]
    fn a_read_asked_for_while_one_is_out_repeats_once_after_it() {
        let mut live = Live::default();
        live.follow(&folders(&["/a"]));
        assert!(live.request("/a"));
        // A burst of changes: none starts a read of its own.
        assert!(!live.request("/a"));
        assert!(!live.request("/a"));
        // The answer says to read again, once; that read is out now, and its
        // own answer asks for nothing more.
        assert!(live.answer("/a", Some(vec![])));
        assert!(!live.answer("/a", Some(vec![])));
        // Nothing out any more: the next request starts a read.
        assert!(live.request("/a"));
    }

    #[test]
    fn a_change_event_reads_every_board_that_is_not_being_read() {
        let mut live = Live::default();
        live.follow(&folders(&["/a", "/b"]));
        assert!(live.request("/a"));
        let mut started = live.request_all();
        started.sort();
        assert_eq!(started, folders(&["/b"]));
    }

    #[test]
    fn a_failed_first_read_is_an_empty_board_and_a_later_one_keeps_the_last() {
        let mut live = Live::default();
        live.follow(&folders(&["/a"]));
        live.request("/a");
        live.answer("/a", None);
        assert_eq!(live.tasks("/a").map(<[Task]>::len), Some(0));

        live.request("/a");
        live.answer("/a", Some(vec![test_task("t")]));
        live.request("/a");
        live.answer("/a", None);
        assert_eq!(live.tasks("/a").map(<[Task]>::len), Some(1));
    }

    #[test]
    fn an_answer_for_a_folder_that_went_is_dropped() {
        let mut live = Live::default();
        live.follow(&folders(&["/a"]));
        live.request("/a");
        live.follow(&folders(&[]));
        assert!(!live.answer("/a", Some(vec![test_task("t")])));
        assert!(live.tasks("/a").is_none());
        assert!(live.folders().is_empty());
    }

    #[test]
    fn reads_several_boards_at_once() {
        let mut live = Live::default();
        live.follow(&folders(&["/a", "/b"]));
        live.request("/a");
        live.answer("/a", Some(vec![test_task("t")]));
        let boards = live.boards_of(&folders(&["/a", "/b", "/nowhere"]));
        assert_eq!(boards["/a"].map(<[Task]>::len), Some(1));
        assert!(boards["/b"].is_none());
        assert!(boards["/nowhere"].is_none());
    }

    #[test]
    fn knows_whether_a_recheck_day_is_waiting() {
        let mut live = Live::default();
        live.follow(&folders(&["/a"]));
        live.request("/a");
        live.answer("/a", Some(vec![test_task("plain")]));
        assert!(!live.has_rechecks());

        let waiting = Task {
            blocked: Some(Blocked {
                reason: "release".into(),
                by: "user".into(),
                since: 1,
                recheck: Some("2026-12-01".into()),
                human_only: false,
                reminded: None,
            }),
            ..test_task("waiting")
        };
        live.request("/a");
        live.answer("/a", Some(vec![waiting]));
        assert!(live.has_rechecks());
    }

    #[test]
    fn the_folders_of_the_workspaces_are_each_held_once() {
        let ws = |cwd: &str| Workspace {
            cwd: cwd.into(),
            ..Workspace::default()
        };
        let state = AppState {
            workspaces: vec![ws("/b"), ws("/a"), ws("/b"), ws("")],
            ..AppState::default()
        };
        assert_eq!(folders_of(&state), folders(&["/a", "/b"]));
    }
}
