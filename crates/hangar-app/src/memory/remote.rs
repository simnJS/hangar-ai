//! The memory of a workspace on a node: the node's own store, not this
//! machine's. Read whole with `GET /node/memory` (core's `GET /api/memory`
//! is an index without the contents, made for a model), written through
//! the node's `/api/memory` routes, which tell its agents' side and its
//! `/node/events` (`memory:changed`) as a write of theirs would.

use hangar_core::memory::MemoryEntry;
use serde_json::{Value, json};

use crate::nodes::NodeClient;
use crate::nodes::wire;

use super::{NewEntry, Wrote};

/// Every entry of the node's memory, contents included.
pub async fn load(client: &NodeClient) -> Result<Vec<MemoryEntry>, String> {
    let answer = wire::request(client, "GET", "/node/memory", None)
        .await?
        .json()?;
    serde_json::from_value(answer.get("entries").cloned().unwrap_or(json!([])))
        .map_err(|err| err.to_string())
}

/// What a write asks of the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// An upsert by title, from workspace `cwd` (the node resolves a
    /// worktree to its repository, as for a local write).
    Create {
        cwd: String,
        entry: NewEntry,
    },
    Update {
        id: String,
        entry: NewEntry,
    },
    Delete {
        id: String,
    },
}

impl Write {
    /// The method, the path and the body of the API call.
    fn call(&self) -> (&'static str, String, Option<Value>) {
        match self {
            Write::Create { cwd, entry } => (
                "POST",
                format!("/api/memory?cwd={}", wire::encode(cwd)),
                // The agent is the API's to set from the caller; the person
                // writing from the window signs "user", as locally.
                Some(json!({
                    "title": entry.title,
                    "content": entry.content,
                    "tags": entry.tags,
                    "agent": "user",
                })),
            ),
            Write::Update { id, entry } => (
                "PATCH",
                format!("/api/memory/{}", wire::encode(id)),
                Some(json!({
                    "title": entry.title,
                    "content": entry.content,
                    "tags": entry.tags,
                })),
            ),
            Write::Delete { id } => ("DELETE", format!("/api/memory/{}", wire::encode(id)), None),
        }
    }

    fn wrote(&self) -> Wrote {
        match self {
            Write::Create { .. } => Wrote::Created,
            Write::Update { .. } => Wrote::Updated,
            Write::Delete { .. } => Wrote::Deleted,
        }
    }
}

/// The write, on the node: a refusal (a limit, a missing entry) comes back
/// as the API words it.
pub async fn write(client: &NodeClient, write: Write) -> Result<Wrote, String> {
    let (method, path, body) = write.call();
    let answer = wire::request(client, method, &path, body.as_ref()).await?;
    if answer.ok() {
        Ok(write.wrote())
    } else {
        Err(answer.error())
    }
}

/// Whether an event of the node's `/node/events` says its memory may have
/// changed: `memory:changed`, or events lost (`lagged`, also sent at each
/// connection, for what changed while it was not followed).
pub fn touches_memory(event: &Value) -> bool {
    event.get("event").and_then(Value::as_str) == Some("memory:changed")
        || event.get("lagged").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::testing::{answer, listen, read_request};

    fn entry() -> NewEntry {
        NewEntry {
            title: "Build".into(),
            content: "cargo build".into(),
            tags: vec!["ci".into()],
        }
    }

    #[tokio::test]
    async fn the_memory_is_read_whole_and_written_through_the_api() {
        let (listener, client) = listen().await;
        let node = tokio::spawn(async move {
            let mut asked = Vec::new();
            let answers = [
                (
                    200,
                    r#"{"entries":[{"id":"m1","title":"Build","content":"cargo build","tags":["ci"],"workspace":"/w","agent":"Ada","created_at":1,"updated_at":2}]}"#,
                ),
                (200, r#"{"entry":{},"created":true}"#),
                (200, r#"{"entry":{}}"#),
                (404, r#"{"error":"memory entry not found"}"#),
            ];
            for (status, body) in answers {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_request(&mut stream).await;
                assert!(request.head.contains("Authorization: Bearer test-token"));
                asked.push((request.line().to_owned(), request.body));
                answer(&mut stream, status, body).await;
            }
            asked
        });
        let entries = load(&client).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "cargo build");
        assert_eq!(entries[0].agent, "Ada");
        let created = Write::Create {
            cwd: "/home/hangar/demo".into(),
            entry: entry(),
        };
        assert_eq!(write(&client, created).await, Ok(Wrote::Created));
        let updated = Write::Update {
            id: "m1".into(),
            entry: entry(),
        };
        assert_eq!(write(&client, updated).await, Ok(Wrote::Updated));
        let deleted = Write::Delete { id: "m 2".into() };
        assert_eq!(
            write(&client, deleted).await,
            Err("memory entry not found".into())
        );
        let asked = node.await.unwrap();
        assert_eq!(asked[0].0, "GET /node/memory HTTP/1.1");
        assert_eq!(
            asked[1].0,
            "POST /api/memory?cwd=%2Fhome%2Fhangar%2Fdemo HTTP/1.1"
        );
        let body: Value = serde_json::from_str(&asked[1].1).unwrap();
        assert_eq!(
            body,
            json!({ "title": "Build", "content": "cargo build", "tags": ["ci"], "agent": "user" })
        );
        assert_eq!(asked[2].0, "PATCH /api/memory/m1 HTTP/1.1");
        let patch: Value = serde_json::from_str(&asked[2].1).unwrap();
        // What core's `MemoryPatch` takes.
        let parsed: hangar_core::memory::MemoryPatch = serde_json::from_value(patch).unwrap();
        assert_eq!(parsed.title.as_deref(), Some("Build"));
        assert_eq!(parsed.tags, Some(vec!["ci".to_owned()]));
        assert_eq!(asked[3].0, "DELETE /api/memory/m%202 HTTP/1.1");
    }

    #[test]
    fn a_memory_change_or_lost_events_reload_it() {
        assert!(touches_memory(&json!({ "event": "memory:changed" })));
        assert!(touches_memory(&json!({ "lagged": 0 })));
        assert!(!touches_memory(
            &json!({ "event": "board:changed", "cwd": "/w" })
        ));
    }
}
