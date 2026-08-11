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
use crate::memory::{self, MemoryPatch, NewMemory};

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

// ---------------------------------------------------------------------------
// Memory
//
// The board is one file per workspace; the memory is one file for the whole
// install (see memory.rs). These routes therefore ignore `cwd` on every read —
// it is only kept as the origin stamped on a write.
// ---------------------------------------------------------------------------

/// Query parameters of the memory routes, in one struct because they all
/// arrive on URLs the MCP proxy builds, and it appends `cwd` to every one of
/// them. Nothing here is required: a memory read is answerable without any of
/// it, and refusing the call over a missing parameter we do not use would only
/// make the proxy harder to change.
#[derive(Deserialize)]
struct MemoryQuery {
    /// Workspace the writer is acting from. Recorded as the origin of a fact.
    #[serde(default)]
    cwd: String,
    /// Free-text search terms.
    #[serde(default)]
    q: String,
    tag: Option<String>,
    workspace: Option<String>,
    limit: Option<usize>,
}

/// The app data directory is resolved per request rather than cached in
/// `ApiState`: it costs a `create_dir_all` and keeps the memory routes from
/// changing the shape of the state the board routes share.
fn memory_dir(
    state: &ApiState,
) -> Result<std::path::PathBuf, (StatusCode, Json<serde_json::Value>)> {
    memory::dir(&state.app).map_err(|e| bad(StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn memory_changed(state: &ApiState) {
    memory::notify_changed(&state.app);
}

/// A refused mutation is the caller's to fix (400); a memory file that could
/// not be read or written is not (500), and in that case nothing was stored.
/// Reporting the second as a bad request would send an agent off rewriting a
/// perfectly good call against a store that is momentarily unreachable.
fn memory_failed(err: memory::StoreError) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        memory::StoreError::Storage(message) => bad(StatusCode::INTERNAL_SERVER_ERROR, message),
        memory::StoreError::Refused(message) => bad(StatusCode::BAD_REQUEST, message),
    }
}

async fn get_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<MemoryQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    // An unreadable store answers with an error, never with an empty list: a
    // caller told "you know nothing" writes the fact again over a file it was
    // simply not allowed to open.
    let memory = memory::store()
        .read(&memory_dir(&state)?)
        .map_err(memory_failed)?;
    // The index carries no contents — its one caller is the MCP proxy, whose
    // reader is a model paying for every character; the full text of an entry
    // is one `memory_read` away. Filtering lives here, next to the search
    // route's, so the two can never disagree about what a tag matches.
    let entries = memory::list(&memory, q.tag.as_deref(), q.workspace.as_deref());
    Ok(Json(json!({ "entries": entries })))
}

async fn search_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<MemoryQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let found = memory::search(
        &memory::store()
            .read(&memory_dir(&state)?)
            .map_err(memory_failed)?,
        &memory::SearchQuery {
            q: q.q,
            tag: q.tag,
            workspace: q.workspace,
            limit: q.limit,
        },
    );
    Ok(Json(
        serde_json::to_value(found).unwrap_or_else(|_| json!({ "results": [], "total": 0 })),
    ))
}

async fn read_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let memory = memory::store()
        .read(&memory_dir(&state)?)
        .map_err(memory_failed)?;
    let entry = memory::find(&memory, &id)
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "memory entry not found"))?;
    Ok(Json(json!({ "entry": entry })))
}

async fn create_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<MemoryQuery>,
    Json(mut input): Json<NewMemory>,
) -> ApiResult {
    authorize(&state, &headers)?;
    // The origin is taken from the URL, not from the body: the caller states
    // what it learned, the proxy states where it was working. A worktree
    // resolves to its main repository the way the board does — the `workspace`
    // search filter runs over this string, and a fact learned on a branch
    // belongs to the project, not to a checkout that may be gone tomorrow.
    input.workspace = crate::git::shared_root(&q.cwd).unwrap_or(q.cwd);
    let (entry, created) = memory::store()
        .update(&memory_dir(&state)?, |memory| memory::upsert(memory, input))
        .map_err(memory_failed)?;
    memory_changed(&state);
    Ok(Json(json!({ "entry": entry, "created": created })))
}

async fn patch_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(patch): Json<MemoryPatch>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let entry = memory::store()
        .update(&memory_dir(&state)?, |memory| {
            memory::patch(memory, &id, patch)
        })
        .map_err(memory_failed)?;
    memory_changed(&state);
    Ok(Json(json!({ "entry": entry })))
}

async fn delete_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult {
    authorize(&state, &headers)?;
    memory::store()
        .update(&memory_dir(&state)?, |memory| memory::delete(memory, &id))
        .map_err(memory_failed)?;
    memory_changed(&state);
    Ok(Json(json!({ "ok": true })))
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
        .route("/api/memory", get(get_memory).post(create_memory))
        // Declared before the parameterised sibling for readability only:
        // matchit 0.8, which axum routes with, prefers a static segment over a
        // `{param}` at the same depth whichever order they were inserted in, so
        // "search" can never be swallowed as an id.
        .route("/api/memory/search", get(search_memory))
        .route(
            "/api/memory/{id}",
            get(read_memory).patch(patch_memory).delete(delete_memory),
        )
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
