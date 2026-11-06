//! What the window writes to a board: the edits of a task, the new ones, the
//! comments and the deletions, as hangar-core takes them.
//!
//! Each runs off the UI thread (it takes the board's lock and writes
//! `board.json`) and answers with `Msg::Mutated`, after which the cache reads
//! the board again: the window shows what the file says, not what it hoped.
//!
//! The board of a workspace on a node (its key is `cache::board_key`'s) is
//! the node's: it is read and written through the node's `/api/board` and
//! `/api/tasks…`, the routes its agents use too.

use hangar_core::board::{NewTask, TaskPatch};
use iced::Task;
use serde_json::{Value, json};

use crate::message::Message;
use crate::nodes::NodeClient;
use crate::nodes::wire;

use super::Msg;
use super::cache::remote_board;
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
    /// The patch as the API's `PATCH /api/tasks/{id}` takes it. A block or
    /// an unblock sent this way is signed "user" but not marked as the
    /// person's: the API keeps that mark for the window's own calls.
    pub fn json(&self) -> Value {
        match self {
            Edit::Title(title) => json!({ "title": title }),
            Edit::Description(description) => json!({ "description": description }),
            Edit::Column(column) => json!({ "column": column.id() }),
            Edit::Priority(priority) => json!({ "priority": priority }),
            Edit::Release => json!({ "release": true }),
            Edit::Owns(owns) => json!({ "owns": owns }),
            Edit::DependsOn(ids) => json!({ "depends_on": ids }),
            Edit::Block {
                reason,
                recheck,
                human_only,
            } => {
                let day = recheck.trim();
                json!({ "block": {
                    "reason": reason.trim(),
                    "recheck": (!day.is_empty()).then_some(day),
                    "human_only": human_only,
                    "by": "user",
                } })
            }
            Edit::Unblock { note } => {
                let text = note.trim();
                json!({ "unblock": {
                    "note": (!text.is_empty()).then_some(text),
                    "by": "user",
                } })
            }
        }
    }

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

    /// The task as the API's `POST /api/tasks` takes it.
    pub fn json(&self) -> Value {
        json!({
            "title": self.title,
            "description": self.description,
            "column": self.column.id(),
            "priority": self.priority,
            "depends_on": self.depends_on,
            "owns": self.owns,
        })
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

#[derive(Debug, Clone, PartialEq)]
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

/// The node's client for a remote board, or why there is none.
fn client_for(node: &str) -> Result<NodeClient, String> {
    crate::nodes::remote::directory(node).ok_or_else(|| format!("node {node} is not connected"))
}

/// `?cwd=` of the API's board routes.
fn on_board(path: &str, cwd: &str) -> String {
    format!("{path}?cwd={}", wire::encode(cwd))
}

/// The change, through the API of the node at `client`, to its board in
/// `cwd`.
pub async fn apply_remote(
    client: &NodeClient,
    cwd: &str,
    mutation: Mutation,
) -> Result<(), String> {
    let (method, path, body) = match mutation {
        Mutation::Create(draft) => ("POST", "/api/tasks".to_owned(), Some(draft.json())),
        Mutation::Update { id, edit } => (
            "PATCH",
            format!("/api/tasks/{}", wire::encode(&id)),
            Some(edit.json()),
        ),
        Mutation::Comment { id, text } => (
            "POST",
            format!("/api/tasks/{}/comment", wire::encode(&id)),
            Some(json!({ "author": "user", "text": text })),
        ),
        Mutation::Delete { id } => ("DELETE", format!("/api/tasks/{}", wire::encode(&id)), None),
    };
    let answer = wire::request(client, method, &on_board(&path, cwd), body.as_ref()).await?;
    if answer.ok() {
        Ok(())
    } else {
        Err(answer.error())
    }
}

/// The tasks of the node's board in `cwd`.
pub async fn read_remote(
    client: &NodeClient,
    cwd: &str,
) -> Result<Vec<hangar_core::board::Task>, String> {
    let board = wire::request(client, "GET", &on_board("/api/board", cwd), None)
        .await?
        .json()?;
    serde_json::from_value(
        board
            .get("tasks")
            .cloned()
            .unwrap_or(Value::Array(Vec::new())),
    )
    .map_err(|err| err.to_string())
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

#[cfg(test)]
thread_local! {
    /// The changes asked for on this thread, in order: a test checks which
    /// board a message wrote to without running the task.
    static ASKED: std::cell::RefCell<Vec<(String, Mutation)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The changes `run` was asked for since the last call, oldest first.
#[cfg(test)]
pub(super) fn take_asked() -> Vec<(String, Mutation)> {
    ASKED.with(|asked| std::mem::take(&mut *asked.borrow_mut()))
}

/// Runs the change off the UI thread and answers `Msg::Mutated`.
pub fn run(cwd: &str, mutation: Mutation, errors: Errors, after: After) -> Task<Message> {
    #[cfg(test)]
    ASKED.with(|asked| asked.borrow_mut().push((cwd.to_owned(), mutation.clone())));
    let folder = cwd.to_owned();
    let work = cwd.to_owned();
    Task::perform(
        async move {
            if let Some((node, cwd)) = remote_board(&work) {
                let client = client_for(node)?;
                return apply_remote(&client, cwd, mutation).await;
            }
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
            // A remote board not read (its node away) leaves the board as it
            // was; the node's events read it again once it is back.
            if let Some((node, cwd)) = remote_board(&work) {
                let client = client_for(node).ok()?;
                return read_remote(&client, cwd).await.ok();
            }
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

    /// A folder to keep a board in, removed when the test is over.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let dir = std::env::temp_dir()
                .join(format!("hangar-board-test-{}-{unique}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("a scratch folder");
            Scratch(dir)
        }

        fn cwd(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn find(cwd: &str, title: &str) -> hangar_core::board::Task {
        hangar_core::board_load(cwd.to_owned())
            .tasks
            .into_iter()
            .find(|task| task.title == title)
            .unwrap_or_else(|| panic!("no task called {title}"))
    }

    /// What the window sends, against the real board of hangar-core: the
    /// patches mean what the dialog and the drag think they mean.
    #[test]
    fn the_window_writes_a_board_the_way_it_means_to() {
        let scratch = Scratch::new();
        let cwd = scratch.cwd();

        apply(
            cwd.clone(),
            Mutation::Create(Draft {
                priority: 2,
                owns: vec!["src/**".into()],
                ..Draft::titled("Write it", Column::Doing)
            }),
        )
        .expect("a task is made");
        apply(
            cwd.clone(),
            Mutation::Create(Draft::titled("Then ship it", Column::Todo)),
        )
        .expect("another");
        let first = find(&cwd, "Write it");
        assert_eq!(first.column, "doing");
        assert_eq!(first.priority, 2);
        assert_eq!(first.owns, vec!["src/**".to_owned()]);
        assert!(first.doing_since.is_some());

        let update = |edit: Edit| {
            apply(
                cwd.clone(),
                Mutation::Update {
                    id: first.id.clone(),
                    edit,
                },
            )
        };

        // A block as the person: back to todo, held by nobody.
        update(Edit::Block {
            reason: " waiting for the npm release ".into(),
            recheck: String::new(),
            human_only: true,
        })
        .expect("a block");
        let blocked = find(&cwd, "Write it");
        assert_eq!(blocked.column, "todo");
        let hold = blocked.blocked.expect("blocked");
        assert_eq!(hold.reason, "waiting for the npm release");
        assert_eq!(hold.by, "user");
        assert!(hold.human_only);

        // It stays in todo until it is unblocked: the board says so too.
        let refused = update(Edit::Column(Column::Doing)).expect_err("refused");
        assert!(refused.contains("blocked"), "{refused}");

        // The person lifts a block only a person may.
        update(Edit::Unblock {
            note: "it is out".into(),
        })
        .expect("an unblock");
        let freed = find(&cwd, "Write it");
        assert!(freed.blocked.is_none());
        assert!(
            freed
                .comments
                .last()
                .is_some_and(|c| c.text.contains("it is out"))
        );

        update(Edit::Priority(0)).expect("a priority");
        update(Edit::Release).expect("a release");
        update(Edit::Owns(vec!["docs/".into(), "README.md".into()])).expect("owns");
        update(Edit::DependsOn(vec![find(&cwd, "Then ship it").id])).expect("a dependency");
        let edited = find(&cwd, "Write it");
        assert_eq!(edited.priority, 0);
        assert_eq!(
            edited.owns,
            vec!["docs/".to_owned(), "README.md".to_owned()]
        );
        assert_eq!(edited.depends_on.len(), 1);

        apply(
            cwd.clone(),
            Mutation::Comment {
                id: first.id.clone(),
                text: "A word".into(),
            },
        )
        .expect("a comment");
        let talked = find(&cwd, "Write it");
        let last = talked.comments.last().expect("a comment");
        assert_eq!(
            (last.author.as_str(), last.text.as_str()),
            ("user", "A word")
        );

        apply(
            cwd.clone(),
            Mutation::Delete {
                id: first.id.clone(),
            },
        )
        .expect("a delete");
        assert_eq!(hangar_core::board_load(cwd.clone()).tasks.len(), 1);
        // Deleting what is gone is an error the dialog does not show.
        assert!(apply(cwd, Mutation::Delete { id: first.id }).is_err());
    }

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

    /// A remote workspace's board, read and written through a fake node's
    /// API: the folder on the node in the query, the token sent, the JSON
    /// the API takes.
    #[tokio::test]
    async fn a_remote_board_goes_through_the_nodes_api() {
        use crate::nodes::testing::{TOKEN, answer, listen, read_request};

        let (listener, client) = listen().await;
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for reply in [
                serde_json::json!({ "tasks": [
                    { "id": "t1", "title": "Ship it", "column": "doing", "priority": 1,
                      "created_at": 1, "updated_at": 2 }
                ], "columns": [] }),
                serde_json::json!({ "task": {} }),
                serde_json::json!({ "task": {} }),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                assert!(
                    request
                        .head
                        .contains(&format!("Authorization: Bearer {TOKEN}"))
                );
                seen.push((request.line().to_owned(), request.body));
                answer(&mut stream, 200, &reply.to_string()).await;
            }
            seen
        });
        let tasks = read_remote(&client, "/home/hangar/demo").await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Ship it");
        let edit = Mutation::Update {
            id: "t1".into(),
            edit: Edit::Column(Column::Done),
        };
        apply_remote(&client, "/home/hangar/demo", edit)
            .await
            .unwrap();
        let create = Mutation::Create(Draft::titled("New", Column::Todo));
        apply_remote(&client, "/home/hangar/demo", create)
            .await
            .unwrap();
        let seen = server.await.unwrap();
        assert!(
            seen[0]
                .0
                .starts_with("GET /api/board?cwd=%2Fhome%2Fhangar%2Fdemo ")
        );
        assert!(
            seen[1]
                .0
                .starts_with("PATCH /api/tasks/t1?cwd=%2Fhome%2Fhangar%2Fdemo ")
        );
        let patch: serde_json::Value = serde_json::from_str(&seen[1].1).unwrap();
        assert_eq!(patch, serde_json::json!({ "column": "done" }));
        assert!(seen[2].0.starts_with("POST /api/tasks?cwd="));
        let made: serde_json::Value = serde_json::from_str(&seen[2].1).unwrap();
        assert_eq!(made["title"], "New");
        assert_eq!(made["column"], "todo");
    }

    /// The JSON patches are what core's `TaskPatch` reads.
    #[test]
    fn a_json_patch_reads_as_cores_patch() {
        let edits = [
            Edit::Title("t".into()),
            Edit::Priority(3),
            Edit::Release,
            Edit::DependsOn(vec!["a".into()]),
            Edit::Block {
                reason: " waits ".into(),
                recheck: "".into(),
                human_only: true,
            },
            Edit::Unblock { note: "ok".into() },
        ];
        for edit in edits {
            let patch: TaskPatch = serde_json::from_value(edit.json()).expect("a patch");
            let local = edit.patch();
            assert_eq!(patch.title, local.title);
            assert_eq!(patch.priority, local.priority);
            assert_eq!(patch.release, local.release);
            assert_eq!(patch.depends_on, local.depends_on);
            assert_eq!(
                patch
                    .block
                    .as_ref()
                    .map(|b| (&b.reason, &b.recheck, b.human_only)),
                local
                    .block
                    .as_ref()
                    .map(|b| (&b.reason, &b.recheck, b.human_only))
            );
            assert_eq!(
                patch.unblock.as_ref().map(|u| &u.note),
                local.unblock.as_ref().map(|u| &u.note)
            );
        }
        let task: NewTask =
            serde_json::from_value(Draft::titled("x", Column::Doing).json()).expect("a task");
        assert_eq!(task.column.as_deref(), Some("doing"));
    }
}
