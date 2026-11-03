//! What the window writes to a board: the edits of a task, the new ones, the
//! comments and the deletions, as hangar-core takes them.
//!
//! Each runs off the UI thread (it takes the board's lock and writes
//! `board.json`) and answers with `Msg::Mutated`, after which the cache reads
//! the board again: the window shows what the file says, not what it hoped.

use hangar_core::board::{NewTask, TaskPatch};
use iced::Task;

use crate::message::Message;

use super::Msg;
use super::model::{Column, block_patch, unblock_patch};

/// One change to a task, as the dialog and the drag make it. Own type because
/// hangar-core's `TaskPatch` is not `Clone`, which a message must be.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Title(String),
    Description(String),
    Column(Column),
    Priority(u8),
    /// Frees the task for any agent.
    Release,
    Owns(Vec<String>),
    DependsOn(Vec<String>),
    Block {
        reason: String,
        recheck: String,
        human_only: bool,
    },
    Unblock {
        note: String,
    },
}

impl Edit {
    /// The patch that makes it (`board_update`'s `patch`).
    pub fn patch(&self) -> TaskPatch {
        match self {
            Edit::Title(title) => TaskPatch {
                title: Some(title.clone()),
                ..TaskPatch::default()
            },
            Edit::Description(description) => TaskPatch {
                description: Some(description.clone()),
                ..TaskPatch::default()
            },
            Edit::Column(column) => TaskPatch {
                column: Some(column.id().to_owned()),
                ..TaskPatch::default()
            },
            Edit::Priority(priority) => TaskPatch {
                priority: Some(*priority),
                ..TaskPatch::default()
            },
            Edit::Release => TaskPatch {
                release: Some(true),
                ..TaskPatch::default()
            },
            Edit::Owns(owns) => TaskPatch {
                owns: Some(owns.clone()),
                ..TaskPatch::default()
            },
            Edit::DependsOn(ids) => TaskPatch {
                depends_on: Some(ids.clone()),
                ..TaskPatch::default()
            },
            Edit::Block {
                reason,
                recheck,
                human_only,
            } => block_patch(reason, recheck, *human_only),
            Edit::Unblock { note } => unblock_patch(note),
        }
    }
}

/// A task to make (`NewTask`).
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub title: String,
    pub description: String,
    pub column: Column,
    pub priority: u8,
    pub owns: Vec<String>,
    pub depends_on: Vec<String>,
}

impl Draft {
    /// Just a title, in a column: what a column's add field makes.
    pub fn titled(title: &str, column: Column) -> Draft {
        Draft {
            title: title.trim().to_owned(),
            description: String::new(),
            column,
            priority: 1,
            owns: Vec::new(),
            depends_on: Vec::new(),
        }
    }

    pub fn new_task(&self) -> NewTask {
        NewTask {
            title: self.title.clone(),
            description: self.description.clone(),
            column: Some(self.column.id().to_owned()),
            priority: Some(self.priority),
            labels: Vec::new(),
            depends_on: self.depends_on.clone(),
            owns: self.owns.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Mutation {
    Create(Draft),
    Update { id: String, edit: Edit },
    Comment { id: String, text: String },
    Delete { id: String },
}

/// What follows a change that the dialog made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// A task written: the dialog is done once it is on the board.
    CloseOnOk,
    /// A task deleted: the dialog is closed whatever the board answered.
    CloseAlways,
}

/// Who is told when a change fails. The board swallows errors (the next read
/// shows what is true); only the dialog's block form says why a block was
/// refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errors {
    Swallow,
    ToDialog,
}

fn apply(cwd: String, mutation: Mutation) -> Result<(), String> {
    match mutation {
        Mutation::Create(draft) => hangar_core::board_create(cwd, draft.new_task()).map(drop),
        Mutation::Update { id, edit } => hangar_core::board_update(cwd, id, edit.patch()).map(drop),
        Mutation::Comment { id, text } => {
            hangar_core::board_comment(cwd, id, "user".into(), text).map(drop)
        }
        Mutation::Delete { id } => hangar_core::board_delete(cwd, id),
    }
}

/// Runs the change off the UI thread and answers `Msg::Mutated`.
pub fn run(cwd: &str, mutation: Mutation, errors: Errors, after: After) -> Task<Message> {
    let folder = cwd.to_owned();
    let work = cwd.to_owned();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || apply(work, mutation))
                .await
                .unwrap_or_else(|panic| Err(panic.to_string()))
        },
        move |result| {
            Message::Board(Msg::Mutated {
                cwd: folder.clone(),
                errors,
                after,
                result,
            })
        },
    )
}

/// A task, read off the UI thread, for `Msg::Loaded`.
pub fn read(cwd: &str) -> Task<Message> {
    let folder = cwd.to_owned();
    let work = cwd.to_owned();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || hangar_core::board_load(work).tasks)
                .await
                .ok()
        },
        move |tasks| Message::Board(Msg::Loaded(folder.clone(), tasks.map(std::sync::Arc::new))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_edit_is_one_field_of_the_patch() {
        let patch = Edit::Title("New".into()).patch();
        assert_eq!(patch.title.as_deref(), Some("New"));
        assert!(patch.description.is_none() && patch.column.is_none());

        assert_eq!(
            Edit::Column(Column::Review).patch().column.as_deref(),
            Some("review")
        );
        assert_eq!(Edit::Priority(3).patch().priority, Some(3));
        assert_eq!(Edit::Release.patch().release, Some(true));
        assert_eq!(
            Edit::Owns(vec!["src/**".into()]).patch().owns,
            Some(vec!["src/**".to_owned()])
        );
        assert_eq!(
            Edit::DependsOn(vec!["a".into()]).patch().depends_on,
            Some(vec!["a".to_owned()])
        );
    }

    #[test]
    fn blocking_and_unblocking_are_the_persons() {
        let block = Edit::Block {
            reason: " wait ".into(),
            recheck: String::new(),
            human_only: true,
        }
        .patch()
        .block
        .expect("a block");
        assert_eq!(block.reason, "wait");
        assert!(block.person && block.human_only);

        let unblock = Edit::Unblock {
            note: "done".into(),
        }
        .patch()
        .unblock
        .expect("an unblock");
        assert!(unblock.person);
        assert_eq!(unblock.note.as_deref(), Some("done"));
    }

    #[test]
    fn a_draft_makes_a_new_task_in_its_column() {
        let draft = Draft {
            description: "why".into(),
            priority: 2,
            owns: vec!["docs/".into()],
            depends_on: vec!["x".into()],
            ..Draft::titled("  Write it  ", Column::Doing)
        };
        let task = draft.new_task();
        assert_eq!(task.title, "Write it");
        assert_eq!(task.column.as_deref(), Some("doing"));
        assert_eq!(task.priority, Some(2));
        assert_eq!(task.owns, vec!["docs/".to_owned()]);
        assert_eq!(task.depends_on, vec!["x".to_owned()]);
        assert!(task.labels.is_empty());
    }
}
