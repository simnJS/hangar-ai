use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::board::{self, BoardStore, NewTask, TaskPatch};

pub struct ApiState {
    /// Borrowed from the app's single store: the UI and the agents must
    /// contend for the same lock, otherwise claims are not actually atomic.
    pub store: &'static BoardStore,
    pub token: String,
    pub app: AppHandle,
}

#[derive(Deserialize)]
pub struct WorkspaceQuery {
    /// Absolute path of the workspace the caller is acting on.
    cwd: String,
}

type ApiResult = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;

fn bad(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "error": message.into() })))
}

/// Every route is behind a bearer token generated at startup, so other local
/// processes cannot read or edit the user's boards just by knowing the port.
fn authorize(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();

    if provided == state.token {
        Ok(())
    } else {
        Err(bad(StatusCode::UNAUTHORIZED, "invalid token"))
    }
}

/// Tells the UI to reload the board it is showing.
fn notify(state: &ApiState, cwd: &str) {
    let _ = state.app.emit("board:changed", cwd);
}

async fn get_board(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let board = state.store.read(&q.cwd);
    Ok(Json(
        json!({ "tasks": board.tasks, "columns": board::COLUMNS }),
    ))
}

async fn next_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let board = state.store.read(&q.cwd);
    Ok(Json(json!({ "task": board::next_task(&board) })))
}

async fn create_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
    Json(input): Json<NewTask>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let task = state
        .store
        .update(&q.cwd, |board| board::create_task(board, input))
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e))?;
    notify(&state, &q.cwd);
    Ok(Json(json!({ "task": task })))
}

async fn patch_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
    Json(patch): Json<TaskPatch>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let task = state
        .store
        .update(&q.cwd, |board| board::patch_task(board, &id, patch))
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e))?;
    notify(&state, &q.cwd);
    Ok(Json(json!({ "task": task })))
}

async fn delete_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    state
        .store
        .update(&q.cwd, |board| board::delete_task(board, &id))
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e))?;
    notify(&state, &q.cwd);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ClaimBody {
    agent: String,
}

async fn claim_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
    Json(body): Json<ClaimBody>,
) -> ApiResult {
    authorize(&state, &headers)?;
    // CONFLICT rather than BAD_REQUEST: the caller should retry with another task.
    let task = state
        .store
        .update(&q.cwd, |board| board::claim_task(board, &id, &body.agent))
        .map_err(|e| bad(StatusCode::CONFLICT, e))?;
    notify(&state, &q.cwd);
    Ok(Json(json!({ "task": task })))
}

#[derive(Deserialize)]
struct CommentBody {
    author: String,
    text: String,
}

async fn comment_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
    Json(body): Json<CommentBody>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let task = state
        .store
        .update(&q.cwd, |board| {
            board::add_comment(board, &id, &body.author, &body.text)
        })
        .map_err(|e| bad(StatusCode::BAD_REQUEST, e))?;
    notify(&state, &q.cwd);
    Ok(Json(json!({ "task": task })))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

/// Binds on an ephemeral loopback port and returns it, so nothing has to
/// negotiate a fixed port that might already be taken.
pub async fn start(
    app: AppHandle,
    store: &'static BoardStore,
    token: String,
) -> Result<u16, String> {
    let state = Arc::new(ApiState { store, token, app });

    let router = Router::new()
        .route("/health", get(health))
        .route("/api/board", get(get_board))
        .route("/api/next", get(next_task))
        .route("/api/tasks", post(create_task))
        .route(
            "/api/tasks/{id}",
            axum::routing::patch(patch_task).delete(delete_task),
        )
        .route("/api/tasks/{id}/claim", post(claim_task))
        .route("/api/tasks/{id}/comment", post(comment_task))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok(port)
}
