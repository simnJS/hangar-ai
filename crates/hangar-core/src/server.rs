use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::board::{self, BoardStore, NewTask, TaskPatch};
use crate::control::ControlError;
use crate::host::{Event, Host};
use crate::memory::{self, MemoryPatch, NewMemory};

/// What the API reaches of the app, which it found through the `AppHandle`
/// before: the host, to tell the window and find the memory; the panes, for
/// the hooks' route to check a caller against; Hangar Mobile, for an agent's
/// shares, when it could be opened.
#[derive(Clone)]
pub struct Reach {
    pub host: Host,
    pub pty: Arc<crate::pty::PtyManager>,
    pub remote: Option<Arc<crate::remote::Remote>>,
}

pub struct ApiState {
    /// Borrowed from the app's single store: the UI and the agents must
    /// contend for the same lock, otherwise claims are not actually atomic.
    pub store: &'static BoardStore,
    pub token: String,
    pub host: Host,
    pub remote: Option<Arc<crate::remote::Remote>>,
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
    authorize_token(&state.token, headers)
}

fn authorize_token(
    token: &str,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();

    if provided == token {
        Ok(())
    } else {
        Err(bad(StatusCode::UNAUTHORIZED, "invalid token"))
    }
}

/// Tells the UI to reload the board it is showing.
fn notify(state: &ApiState, cwd: &str) {
    let _ = state.host.emit(Event::BoardChanged {
        cwd: cwd.to_string(),
    });
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
    memory::dir(&state.host).map_err(|e| bad(StatusCode::INTERNAL_SERVER_ERROR, e))
}

fn memory_changed(state: &ApiState) {
    memory::notify_changed(&state.host);
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

// ---------------------------------------------------------------------------
// Bridge
//
// The hangar-bridge mod inside each pane's Claude Code reports here, keyed by
// the pane id Hangar put in its environment (see bridge.rs).
// ---------------------------------------------------------------------------

/// One report from a pane's mod, relayed to the window as is. The window owns
/// the meaning of each `kind`; only an edit is also remembered here, because
/// the conflict question is asked of the API, not of the window.
async fn bridge_event(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(pane): Path<String>,
    Json(event): Json<serde_json::Value>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let kind = event
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if kind.is_empty() {
        return Err(bad(StatusCode::BAD_REQUEST, "an event needs a kind"));
    }
    if kind == "edit" {
        if let Some(file) = event.get("file").and_then(|v| v.as_str()) {
            let name = event.get("name").and_then(|v| v.as_str()).unwrap_or(&pane);
            crate::bridge::record_edit(&pane, name, file);
        }
    }
    let _ = state.host.emit(Event::BridgeEvent {
        pane_id: pane,
        event,
    });
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NextQuery {
    /// Only a message an agent or Hangar sent: the mod asks for these in the
    /// middle of a turn, where the person's prompts must wait.
    #[serde(default)]
    labelled: bool,
}

async fn bridge_next(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(pane): Path<String>,
    Query(q): Query<NextQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let message = crate::bridge::take_next(&state.host, &pane, q.labelled);
    Ok(Json(json!({ "message": message })))
}

/// What the pane's mod adds to its session: its part in an orchestration and
/// the state it is handed again (orchestrator.rs). `null` for any other pane.
/// A window that cannot tell is an error, never a `null`: a mod told "no
/// role" would start an orchestrator without its rules.
async fn bridge_briefing(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(pane): Path<String>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let briefing = crate::orchestrator::briefing(&state.host, &pane)
        .await
        .map_err(|err| match err {
            crate::control::ControlError::Refused(message) => {
                bad(StatusCode::INTERNAL_SERVER_ERROR, message)
            }
            crate::control::ControlError::Unavailable(message) => {
                bad(StatusCode::SERVICE_UNAVAILABLE, message)
            }
        })?;
    Ok(Json(json!({ "briefing": briefing })))
}

#[derive(Deserialize)]
struct ConflictQuery {
    file: String,
}

async fn bridge_conflict(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(pane): Path<String>,
    Query(q): Query<ConflictQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    Ok(Json(
        json!({ "owner": crate::bridge::conflict(&pane, &q.file) }),
    ))
}

// ---------------------------------------------------------------------------
// Pane control
//
// The panes belong to the window's store, so these routes only relay: see
// control.rs.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ControlQuery {
    /// The caller's workspace, as the MCP proxy appends it to every URL. Used
    /// when the request names no workspace of its own.
    #[serde(default)]
    cwd: String,
}

async fn control(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(op): Path<String>,
    Query(q): Query<ControlQuery>,
    Json(mut args): Json<serde_json::Value>,
) -> ApiResult {
    authorize(&state, &headers)?;
    if !args.is_object() {
        args = json!({});
    }
    if args.get("callerCwd").is_none() && !q.cwd.is_empty() {
        args["callerCwd"] = json!(q.cwd);
    }
    let answer = if op == KIT_TAB_OPEN {
        ask_kit_tab_open(&state.host, args).await
    } else {
        crate::control::ask(&state.host, &op, args).await
    };
    answer.map(Json).map_err(control_failed)
}

fn control_failed(err: ControlError) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        ControlError::Refused(message) => bad(StatusCode::BAD_REQUEST, message),
        ControlError::Unavailable(message) => bad(StatusCode::SERVICE_UNAVAILABLE, message),
    }
}

// ---------------------------------------------------------------------------
// A kit's tabs
//
// Agents open one through the control route, like everything they ask the
// window (lib/control.ts). A hook of a kit runs in a pane and has no MCP, so it
// has a route of its own, whose whole contract is docs/kits.md: the same body,
// the bridge routes' token, and the pane it says it is in, which has to be one
// Hangar has running. The window finds the workspace from that pane and nothing
// else: whatever else the body holds does not get past `KitTabBody`.
// ---------------------------------------------------------------------------

const KIT_TAB_OPEN: &str = "kit_tab_open";

/// Holds a `kit_tab_open`'s values to the rules of the kit as it is on disk, and
/// asks the window. A kit Rust cannot read, or a tab it does not have, is the
/// window's to refuse, which it does by name.
async fn ask_kit_tab_open(host: &Host, args: Value) -> Result<Value, ControlError> {
    if let Some(refusal) = kit_param_refusal(&args).await {
        return Err(ControlError::Refused(refusal));
    }
    crate::control::ask(host, KIT_TAB_OPEN, args).await
}

async fn kit_param_refusal(args: &Value) -> Option<String> {
    let kit = args.get("kit")?.as_str()?;
    let tab = args.get("tab")?.as_str()?;
    let params = args.get("params")?;
    let manifest = crate::kits::kits_list()
        .await
        .ok()?
        .into_iter()
        .find(|entry| entry.id == kit)?
        .manifest?;
    manifest
        .tabs
        .iter()
        .find(|entry| entry.id == tab)?
        .param_refusal(params)
}

type WindowAnswer = Pin<Box<dyn Future<Output = Result<Value, ControlError>> + Send>>;

/// What the hooks' route reaches of the app, which a test stands in for.
struct KitTabRoute {
    token: String,
    pane_alive: Box<dyn Fn(&str) -> bool + Send + Sync>,
    open: Box<dyn Fn(Value) -> WindowAnswer + Send + Sync>,
}

#[derive(Deserialize)]
struct KitTabQuery {
    /// `$HANGAR_PANE_ID`, from the hook's environment.
    #[serde(default)]
    pane: String,
}

/// The body of `kit_tab_open`, and all that is read of it.
#[derive(Deserialize)]
struct KitTabBody {
    kit: String,
    tab: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    reveal: Option<bool>,
}

/// What the window is asked: who is calling is the pane, and only the pane.
fn kit_tab_args(pane: &str, body: KitTabBody) -> Value {
    let mut args = json!({
        "kit": body.kit,
        "tab": body.tab,
        "caller": { "paneId": pane },
    });
    if let Some(params) = body.params {
        args["params"] = params;
    }
    if let Some(reveal) = body.reveal {
        args["reveal"] = json!(reveal);
    }
    args
}

async fn kit_tab_open(
    State(route): State<Arc<KitTabRoute>>,
    headers: HeaderMap,
    Query(q): Query<KitTabQuery>,
    Json(body): Json<KitTabBody>,
) -> ApiResult {
    authorize_token(&route.token, &headers)?;
    if !(route.pane_alive)(&q.pane) {
        return Err(bad(
            StatusCode::NOT_FOUND,
            format!(
                "no pane {:?} is running in Hangar: pass ?pane=$HANGAR_PANE_ID, from the pane the hook runs in",
                q.pane
            ),
        ));
    }
    (route.open)(kit_tab_args(&q.pane, body))
        .await
        .map(Json)
        .map_err(control_failed)
}

fn kit_tab_router(route: KitTabRoute) -> Router {
    Router::new()
        .route("/api/kit/tab/open", post(kit_tab_open))
        .with_state(Arc::new(route))
}

// ---------------------------------------------------------------------------
// Hangar Mobile
//
// What an agent shows the person on their phone: the file is copied and
// recorded by the remote module (remote/files.rs).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PhoneShareBody {
    path: String,
    caption: Option<String>,
    /// The caller's pane, from its environment (the MCP proxy), never from
    /// the model.
    #[serde(default, rename = "paneName")]
    pane_name: String,
}

async fn phone_share(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
    Json(body): Json<PhoneShareBody>,
) -> ApiResult {
    authorize(&state, &headers)?;
    if body.path.trim().is_empty() {
        return Err(bad(StatusCode::BAD_REQUEST, "missing 'path'"));
    }
    let remote = state.remote.as_ref().ok_or_else(|| {
        bad(
            StatusCode::SERVICE_UNAVAILABLE,
            "Hangar Mobile is unavailable",
        )
    })?;
    let share = remote
        .share(crate::remote::NewShare {
            source: std::path::PathBuf::from(body.path),
            caption: body.caption,
            workspace: q.cwd,
            pane: body.pane_name,
        })
        .await
        .map_err(|err| bad(StatusCode::BAD_REQUEST, err))?;
    Ok(Json(json!({ "shared": share })))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

/// Binds on an ephemeral loopback port and returns it, so nothing has to
/// negotiate a fixed port that might already be taken.
pub async fn start(reach: Reach, store: &'static BoardStore, token: String) -> Result<u16, String> {
    let Reach { host, pty, remote } = reach;
    let kit_tabs = kit_tab_router(KitTabRoute {
        token: token.clone(),
        pane_alive: Box::new(move |pane| crate::pty::pane_alive(&pty, pane)),
        open: {
            let host = host.clone();
            Box::new(move |args| {
                let host = host.clone();
                Box::pin(async move { ask_kit_tab_open(&host, args).await })
            })
        },
    });
    let state = Arc::new(ApiState {
        store,
        token,
        host,
        remote,
    });

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
        .route("/api/bridge/{pane}/event", post(bridge_event))
        .route("/api/bridge/{pane}/next", get(bridge_next))
        .route("/api/bridge/{pane}/conflict", get(bridge_conflict))
        .route("/api/bridge/{pane}/briefing", get(bridge_briefing))
        .route("/api/control/{op}", post(control))
        .route("/api/phone/share", post(phone_share))
        .with_state(state)
        .merge(kit_tabs);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok(port)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    const LIVE: &str = "p-live";
    const OPEN: &str = "/api/kit/tab/open?pane=p-live";

    /// The route, a pane `LIVE` running and a window that answers: what it was
    /// asked goes to the list handed back.
    fn route(answer: Result<Value, ControlError>) -> (Router, Arc<Mutex<Vec<Value>>>) {
        let asked: Arc<Mutex<Vec<Value>>> = Arc::default();
        let kept = asked.clone();
        let answer = Mutex::new(Some(answer));
        let router = kit_tab_router(KitTabRoute {
            token: "secret".to_string(),
            pane_alive: Box::new(|pane| pane == LIVE),
            open: Box::new(move |args| {
                kept.lock().unwrap().push(args);
                let answer = answer.lock().unwrap().take().expect("asked twice");
                Box::pin(async move { answer })
            }),
        });
        (router, asked)
    }

    fn request(uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
        let mut request = Request::post(uri).header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        request.body(Body::from(body.to_string())).unwrap()
    }

    async fn send(router: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn run() -> Value {
        json!({ "kit": "ci-kit", "tab": "run", "params": { "run": "4567" } })
    }

    #[tokio::test]
    async fn a_hook_opens_a_tab_as_its_own_pane() {
        let (router, asked) = route(Ok(json!({ "outcome": "opened" })));
        let (status, body) = send(router, request(OPEN, Some("secret"), run())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "opened");
        assert_eq!(
            asked.lock().unwrap().as_slice(),
            [json!({
                "kit": "ci-kit",
                "tab": "run",
                "params": { "run": "4567" },
                "caller": { "paneId": LIVE },
            })]
        );
    }

    #[tokio::test]
    async fn the_route_is_refused_without_the_token() {
        for token in [None, Some("wrong"), Some("")] {
            let (router, asked) = route(Ok(json!({})));
            let (status, body) = send(router, request(OPEN, token, run())).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
            assert_eq!(body["error"], "invalid token");
            assert!(asked.lock().unwrap().is_empty(), "{token:?}");
        }
    }

    #[tokio::test]
    async fn an_unknown_pane_is_refused_before_the_window_hears_of_it() {
        for uri in ["/api/kit/tab/open?pane=p-gone", "/api/kit/tab/open"] {
            let (router, asked) = route(Ok(json!({})));
            let (status, body) = send(router, request(uri, Some("secret"), run())).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
            assert!(
                body["error"].as_str().unwrap().contains("HANGAR_PANE_ID"),
                "{body}"
            );
            assert!(asked.lock().unwrap().is_empty(), "{uri}");
        }
    }

    /// The workspace comes from the pane: the body has no say in it.
    #[tokio::test]
    async fn a_body_cannot_say_who_is_calling_or_where() {
        let (router, asked) = route(Ok(json!({})));
        let body = json!({
            "kit": "ci-kit",
            "tab": "run",
            "reveal": false,
            "workspace": "another",
            "pane": "p-other",
            "callerCwd": "D:/elsewhere",
            "caller": { "paneId": "p-other", "cwd": "D:/elsewhere" },
        });
        let (status, _) = send(router, request(OPEN, Some("secret"), body)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            asked.lock().unwrap().as_slice(),
            [json!({
                "kit": "ci-kit",
                "tab": "run",
                "reveal": false,
                "caller": { "paneId": LIVE },
            })]
        );
    }

    #[tokio::test]
    async fn the_window_s_refusal_reaches_the_hook_in_its_words() {
        let (router, _) = route(Err(ControlError::Refused(
            "Kit \"ci-kit\" is not enabled".to_string(),
        )));
        let (status, body) = send(router, request(OPEN, Some("secret"), run())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "Kit \"ci-kit\" is not enabled");

        let (router, _) = route(Err(ControlError::Unavailable("no window".to_string())));
        let (status, _) = send(router, request(OPEN, Some("secret"), run())).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn a_body_without_a_kit_or_a_tab_never_reaches_the_window() {
        for body in [json!({ "tab": "run" }), json!({ "kit": "ci-kit" })] {
            let (router, asked) = route(Ok(json!({})));
            let response = router
                .oneshot(request(OPEN, Some("secret"), body))
                .await
                .unwrap();
            assert!(response.status().is_client_error());
            assert!(asked.lock().unwrap().is_empty());
        }
    }
}
