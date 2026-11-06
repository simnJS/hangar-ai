//! The Claude Code transcripts of the node's panes, read where they are
//! written: the desktop's context gauge, session picker, session hunt and
//! chat view of a remote workspace ask these instead of its own disk.
//!
//! - `GET /node/chat?session=&cwd=&before=&limit=`: one page of a session's
//!   conversation (hangar-core's `chat_page`), `{"page": <ChatPage> | null}`,
//!   null while the session has no transcript yet;
//! - `GET /node/context?session=&cwd=`: where the session stands in its
//!   context window (`context_usage`), `{"usage": {"usedTokens", "model"} |
//!   null}`;
//! - `GET /node/sessions?cwd=[&labels=1]`: the conversations kept for `cwd`,
//!   newest first, `{"sessions": [{"id", "label", "modifiedMs"}], "now"}`,
//!   `now` the node's clock as it listed (epoch ms), for the desktop to read
//!   the times on its own clock: the session hunt only adopts a transcript
//!   written in the last seconds, and two machines' clocks differ. Without
//!   `labels` the listing is the directory's alone (`list_session_ids`: no
//!   transcript opened, the labels empty), what the session hunt asks every
//!   few seconds; with it, each label is read (`list_sessions`), for the
//!   session picker.
#![cfg(target_os = "linux")]

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::api::error;

#[derive(Deserialize)]
pub struct ChatQuery {
    #[serde(default)]
    session: String,
    #[serde(default)]
    cwd: String,
    before: Option<String>,
    limit: Option<usize>,
}

pub async fn chat(Query(query): Query<ChatQuery>) -> Response {
    // The session id is checked by core, the cursor too: a bad one is the
    // caller's to fix.
    match hangar_core::chat::chat_page(query.session, query.cwd, query.before, query.limit).await {
        Ok(page) => Json(json!({ "page": page })).into_response(),
        Err(err) => error(StatusCode::BAD_REQUEST, &err),
    }
}

#[derive(Deserialize)]
pub struct ContextQuery {
    #[serde(default)]
    session: String,
    #[serde(default)]
    cwd: String,
}

pub async fn context(Query(query): Query<ContextQuery>) -> Response {
    let usage = hangar_core::context::context_usage(query.session, query.cwd).await;
    Json(json!({ "usage": usage })).into_response()
}

#[derive(Deserialize)]
pub struct SessionsQuery {
    #[serde(default)]
    cwd: String,
    /// `1` or `true`: each session's label read from its transcript.
    #[serde(default)]
    labels: Option<String>,
}

fn wants_labels(asked: Option<&str>) -> bool {
    matches!(asked, Some("1" | "true"))
}

pub async fn sessions(Query(query): Query<SessionsQuery>) -> Response {
    if query.cwd.is_empty() {
        return error(StatusCode::BAD_REQUEST, "sessions are listed for a cwd");
    }
    let labels = wants_labels(query.labels.as_deref());
    let cwd = query.cwd;
    let listed = tokio::task::spawn_blocking(move || {
        if labels {
            hangar_core::sessions::list_sessions(cwd)
                .into_iter()
                .map(|session| {
                    json!({
                        "id": session.id,
                        "label": session.label,
                        "modifiedMs": session.modified_ms,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            hangar_core::sessions::list_session_ids(cwd)
                .into_iter()
                .map(
                    |(id, modified_ms)| json!({ "id": id, "label": "", "modifiedMs": modified_ms }),
                )
                .collect()
        }
    })
    .await;
    match listed {
        Ok(sessions) => Json(json!({ "sessions": sessions, "now": now_ms() })).into_response(),
        Err(err) => error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_asked_for_in_so_many_words() {
        assert!(wants_labels(Some("1")));
        assert!(wants_labels(Some("true")));
        assert!(!wants_labels(Some("0")));
        assert!(!wants_labels(None));
    }
}
