use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use subtle::ConstantTimeEq;

use crate::board::{self, BoardStore, NewTask, ProgressReport, TaskPatch};
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
    /// Hangar Mobile is the desktop's only (`desktop` feature): a headless
    /// node answers an agent's shares that there is no phone to share with.
    #[cfg(feature = "desktop")]
    pub remote: Option<Arc<crate::remote::Remote>>,
}

pub struct ApiState {
    /// Borrowed from the app's single store: the UI and the agents must
    /// contend for the same lock, otherwise claims are not actually atomic.
    pub store: &'static BoardStore,
    tokens: Arc<Tokens>,
    pub host: Host,
    #[cfg(feature = "desktop")]
    pub remote: Option<Arc<crate::remote::Remote>>,
}

#[derive(Deserialize)]
pub struct WorkspaceQuery {
    /// Absolute path of the workspace the caller is acting on.
    cwd: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);
type ApiResult = Result<Json<serde_json::Value>, ApiError>;

fn bad(status: StatusCode, message: impl Into<String>) -> ApiError {
    (status, Json(json!({ "error": message.into() })))
}

// ---------------------------------------------------------------------------
// Who is calling
//
// Every route is behind a bearer token generated at startup, so other local
// processes cannot read or edit the user's boards just by knowing the port.
// The token also says who calls: a body says what it likes, and every pane's
// environment would otherwise hold the one token that lets it say it. So a
// pane gets a token of its own when it is spawned (`PaneTokens::issue`), and
// whatever its requests say about who sent them — `caller.paneId`, a claim's
// `agent`, a comment's `author`, a block's `by`, a fact's `agent` — is
// replaced by the pane the token was issued to.
// ---------------------------------------------------------------------------

/// Who a request comes from, as its bearer token says, never as its body does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The token `start` was given: Hangar's own, kept in memory. Its body is
    /// taken at its word, as every caller's was before panes had tokens.
    App,
    /// The token for processes Hangar did not start (`outside_token`), such
    /// as a Claude Code in another terminal: it may name itself on the board,
    /// but it is no pane, so it can never say it is one.
    Outside,
    /// A pane's own token (`PaneTokens::issue`): it is that pane, and only it.
    Pane(PaneIdentity),
}

/// The pane a token was issued to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneIdentity {
    pub id: String,
    pub name: Option<String>,
}

impl PaneIdentity {
    /// What the pane signs a claim, a comment, a block or a fact with: its
    /// name, which is what the MCP proxy signs with (`HANGAR_PANE_NAME`), and
    /// its id when it has none.
    pub fn signature(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

impl Caller {
    /// The name the body's own is replaced by: a pane's, and nobody else's.
    fn signature(&self) -> Option<&str> {
        match self {
            Caller::Pane(me) => Some(me.signature()),
            Caller::App | Caller::Outside => None,
        }
    }
}

/// The tokens of the panes Hangar spawned, one per pane, kept in memory only:
/// each goes into its own pane's environment and nowhere else.
#[derive(Default)]
pub struct PaneTokens {
    panes: Mutex<HashMap<String, Issued>>,
}

struct Issued {
    token: String,
    name: Option<String>,
}

impl PaneTokens {
    /// The token of pane `pane_id`, made on its first spawn. The same pane
    /// gets the same token until it is revoked, so the parts of one spawn that
    /// each put it in the environment (the bridge mod, a kit's hooks, the MCP
    /// proxy) agree; its name is the one given last.
    pub fn issue(&self, pane_id: &str, name: Option<&str>) -> String {
        let name = name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        let mut panes = self.lock();
        let issued = panes.entry(pane_id.to_string()).or_insert_with(|| Issued {
            token: crate::endpoint::generate_token(),
            name: None,
        });
        issued.name = name;
        issued.token.clone()
    }

    /// Forgets the token of a pane that was closed: it opens nothing after.
    pub fn revoke(&self, pane_id: &str) {
        self.lock().remove(pane_id);
    }

    /// The pane `provided` was issued to. Every token is compared, in constant
    /// time, whichever matches: how long this takes says nothing of which.
    fn identify(&self, provided: &str) -> Option<PaneIdentity> {
        let panes = self.lock();
        let mut found = None;
        for (id, issued) in panes.iter() {
            if same_token(&issued.token, provided) {
                found = Some(PaneIdentity {
                    id: id.clone(),
                    name: issued.name.clone(),
                });
            }
        }
        found
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Issued>> {
        self.panes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The panes' tokens of this run of Hangar: what the spawn of a pane issues
/// from and what the API checks against.
pub fn pane_tokens() -> &'static PaneTokens {
    static TOKENS: OnceLock<PaneTokens> = OnceLock::new();
    TOKENS.get_or_init(PaneTokens::default)
}

/// The token for processes Hangar did not start, made once per run: the one
/// to advertise in endpoint.json, which any process of the person can read.
pub fn outside_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(crate::endpoint::generate_token)
}

/// Constant time over the bytes, as the phone's tokens are compared
/// (remote/store.rs): only the length can tell, and every token has the same.
fn same_token(expected: &str, provided: &str) -> bool {
    !expected.is_empty() && bool::from(expected.as_bytes().ct_eq(provided.as_bytes()))
}

/// The three kinds of token the API knows.
struct Tokens {
    app: String,
    outside: &'static str,
    panes: &'static PaneTokens,
}

impl Tokens {
    fn caller(&self, headers: &HeaderMap) -> Result<Caller, ApiError> {
        let provided = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or_default();
        // Both compared whatever the first says, for the same reason as above.
        let app = same_token(&self.app, provided);
        let outside = same_token(self.outside, provided);
        if provided.is_empty() {
            Err(bad(StatusCode::UNAUTHORIZED, "invalid token"))
        } else if app {
            Ok(Caller::App)
        } else if outside {
            Ok(Caller::Outside)
        } else {
            self.panes
                .identify(provided)
                .map(Caller::Pane)
                .ok_or_else(|| bad(StatusCode::UNAUTHORIZED, "invalid token"))
        }
    }
}

fn authorize(state: &ApiState, headers: &HeaderMap) -> Result<Caller, ApiError> {
    state.tokens.caller(headers)
}

/// A route that acts as pane `pane` (the bridge mod's): Hangar's own token,
/// or that pane's.
fn as_pane(caller: &Caller, pane: &str) -> Result<(), ApiError> {
    match caller {
        Caller::App => Ok(()),
        Caller::Pane(me) if me.id == pane => Ok(()),
        Caller::Pane(_) => Err(bad(
            StatusCode::FORBIDDEN,
            "a pane's token speaks for that pane only",
        )),
        Caller::Outside => Err(bad(
            StatusCode::FORBIDDEN,
            "this route is for the panes Hangar runs, with the token in their environment",
        )),
    }
}

/// What a control request's `caller` says, made true: a pane's token puts its
/// pane there, whatever the body said, and the outside token takes any pane
/// out (without one, every role gate of the window refuses). `cwd` is where
/// the caller works, not who it is, and stays.
fn stamp_caller(args: &mut Value, caller: &Caller) {
    match caller {
        Caller::App => {}
        Caller::Pane(me) => {
            let cwd = args
                .get("caller")
                .and_then(|caller| caller.get("cwd"))
                .cloned()
                .unwrap_or(Value::Null);
            args["caller"] = json!({ "paneId": me.id, "paneName": me.name, "cwd": cwd });
        }
        Caller::Outside => {
            if let Some(stated) = args.get_mut("caller").and_then(Value::as_object_mut) {
                stated.remove("paneId");
                stated.remove("paneName");
            }
        }
    }
}

/// A block or an unblock over HTTP is never the person's: only the window's
/// own call, which does not come this way, may say it is (`BlockRequest::
/// person`). A pane signs it with its own name.
fn hold_patch(patch: &mut TaskPatch, caller: &Caller) {
    if let Some(block) = patch.block.as_mut() {
        block.person = false;
        if let Some(me) = caller.signature() {
            block.by = Some(me.to_string());
        }
    }
    if let Some(unblock) = patch.unblock.as_mut() {
        unblock.person = false;
        if let Some(me) = caller.signature() {
            unblock.by = Some(me.to_string());
        }
    }
}

/// Runs a store's work on the blocking pool: it takes a `std` lock that every
/// workspace's agents and the window contend for, then reads, parses and
/// renames files, and a cold `shared_root` starts git. On the runtime's own
/// threads it would hold up every other route, the control relay and the
/// bridge's among them, for as long as it waits.
async fn off_runtime<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(work).await.map_err(|err| {
        bad(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("the store's work failed: {err}"),
        )
    })
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
    let store = state.store;
    let board = off_runtime(move || store.read(&q.cwd)).await?;
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
    let store = state.store;
    let board = off_runtime(move || store.read(&q.cwd)).await?;
    Ok(Json(json!({ "task": board::next_task(&board) })))
}

/// Applies `mutate` to the board of `cwd` off the runtime, tells the UI when
/// it took, and turns its refusal into `refused`.
async fn update_board<T: Send + 'static>(
    state: &ApiState,
    cwd: String,
    refused: StatusCode,
    mutate: impl FnOnce(&mut board::Board) -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    let store = state.store;
    let at = cwd.clone();
    let outcome = off_runtime(move || store.update(&at, mutate))
        .await?
        .map_err(|e| bad(refused, e))?;
    notify(state, &cwd);
    Ok(outcome)
}

async fn create_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<WorkspaceQuery>,
    Json(input): Json<NewTask>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let task = update_board(&state, q.cwd, StatusCode::BAD_REQUEST, |board| {
        board::create_task(board, input)
    })
    .await?;
    Ok(Json(json!({ "task": task })))
}

async fn patch_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
    Json(mut patch): Json<TaskPatch>,
) -> ApiResult {
    let caller = authorize(&state, &headers)?;
    hold_patch(&mut patch, &caller);
    let task = update_board(&state, q.cwd, StatusCode::BAD_REQUEST, move |board| {
        board::patch_task(board, &id, patch)
    })
    .await?;
    Ok(Json(json!({ "task": task })))
}

async fn delete_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    update_board(&state, q.cwd, StatusCode::BAD_REQUEST, move |board| {
        board::delete_task(board, &id)
    })
    .await?;
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
    let caller = authorize(&state, &headers)?;
    let agent = caller.signature().map_or(body.agent, str::to_string);
    // CONFLICT rather than BAD_REQUEST: the caller should retry with another task.
    let task = update_board(&state, q.cwd, StatusCode::CONFLICT, move |board| {
        board::claim_task(board, &id, &agent)
    })
    .await?;
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
    let caller = authorize(&state, &headers)?;
    let author = caller.signature().map_or(body.author, str::to_string);
    let task = update_board(&state, q.cwd, StatusCode::BAD_REQUEST, move |board| {
        board::add_comment(board, &id, &author, &body.text)
    })
    .await?;
    Ok(Json(json!({ "task": task })))
}

/// Records an agent's estimate of how far along a task is. A pane reports as
/// itself, whatever the body says, and only on the task it holds, unless it
/// is an orchestrator (the window says). Hangar's own token may report on any
/// task in doing; the outside token on the one held under the name it gives.
async fn progress_task(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<WorkspaceQuery>,
    Json(mut report): Json<ProgressReport>,
) -> ApiResult {
    let caller = authorize(&state, &headers)?;
    report.any_holder = false;
    match &caller {
        Caller::App => report.any_holder = true,
        Caller::Pane(me) => {
            let signature = me.signature().to_string();
            report.by = Some(signature.clone());
            let store = state.store;
            let (cwd, task_id) = (q.cwd.clone(), id.clone());
            // The window is asked only when the task is someone else's: a
            // worker reporting on its own never waits on it.
            let someone_else_s = off_runtime(move || {
                store.read(&cwd).tasks.iter().any(|task| {
                    task.id == task_id
                        && task.column == "doing"
                        && task.assignee.as_deref() != Some(signature.as_str())
                })
            })
            .await?;
            if someone_else_s {
                report.any_holder = is_orchestrator(&state.host, &me.id).await;
            }
        }
        Caller::Outside => {}
    }
    let task = update_board(&state, q.cwd, StatusCode::BAD_REQUEST, move |board| {
        board::report_progress(board, &id, report)
    })
    .await?;
    Ok(Json(json!({ "task": task })))
}

/// Whether the window gives pane `pane_id` the orchestrator's role. A window
/// that cannot tell says no: the report is then refused as anyone's would be.
async fn is_orchestrator(host: &Host, pane_id: &str) -> bool {
    crate::orchestrator::briefing(host, pane_id)
        .await
        .ok()
        .flatten()
        .is_some_and(|briefing| briefing["role"] == "orchestrator")
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

fn memory_changed(state: &ApiState) {
    memory::notify_changed(&state.host);
}

/// A refused mutation is the caller's to fix (400); a memory file that could
/// not be read or written is not (500), and in that case nothing was stored.
/// Reporting the second as a bad request would send an agent off rewriting a
/// perfectly good call against a store that is momentarily unreachable.
fn memory_failed(err: memory::StoreError) -> ApiError {
    match err {
        memory::StoreError::Storage(message) => bad(StatusCode::INTERNAL_SERVER_ERROR, message),
        memory::StoreError::Refused(message) => bad(StatusCode::BAD_REQUEST, message),
    }
}

/// The memory as it is on disk, read off the runtime. The app data directory
/// is resolved per request rather than cached in `ApiState`: it costs a
/// `create_dir_all`, which is why it is resolved there too.
///
/// An unreadable store answers with an error, never with an empty list: a
/// caller told "you know nothing" writes the fact again over a file it was
/// simply not allowed to open.
async fn read_memory_off(state: &ApiState) -> Result<memory::Memory, ApiError> {
    let host = state.host.clone();
    off_runtime(move || {
        let dir = memory::dir(&host).map_err(|e| bad(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        memory::store().read(&dir).map_err(memory_failed)
    })
    .await?
}

/// Applies `mutate` to the memory off the runtime, and tells the UI when it
/// took.
async fn update_memory<T: Send + 'static>(
    state: &ApiState,
    mutate: impl FnOnce(&mut memory::Memory) -> Result<T, String> + Send + 'static,
) -> Result<T, ApiError> {
    let host = state.host.clone();
    let outcome = off_runtime(move || {
        let dir = memory::dir(&host).map_err(|e| bad(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        memory::store().update(&dir, mutate).map_err(memory_failed)
    })
    .await??;
    memory_changed(state);
    Ok(outcome)
}

async fn get_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Query(q): Query<MemoryQuery>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let memory = read_memory_off(&state).await?;
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
        &read_memory_off(&state).await?,
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
    let memory = read_memory_off(&state).await?;
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
    let caller = authorize(&state, &headers)?;
    if let Some(me) = caller.signature() {
        input.agent = me.to_string();
    }
    // The origin is taken from the URL, not from the body: the caller states
    // what it learned, the proxy states where it was working. A worktree
    // resolves to its main repository the way the board does — the
    // `workspace` search filter runs over this string, and a fact learned on
    // a branch belongs to the project, not to a checkout that may be gone
    // tomorrow. Off the runtime, and before the memory's lock is taken: a
    // cold `shared_root` starts git.
    let cwd = q.cwd;
    input.workspace = off_runtime(move || crate::git::shared_root(&cwd).unwrap_or(cwd)).await?;
    let (entry, created) =
        update_memory(&state, move |memory| memory::upsert(memory, input)).await?;
    Ok(Json(json!({ "entry": entry, "created": created })))
}

async fn patch_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(patch): Json<MemoryPatch>,
) -> ApiResult {
    authorize(&state, &headers)?;
    let entry = update_memory(&state, move |memory| memory::patch(memory, &id, patch)).await?;
    Ok(Json(json!({ "entry": entry })))
}

async fn delete_memory(
    State(state): State<Arc<ApiState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult {
    authorize(&state, &headers)?;
    update_memory(&state, move |memory| memory::delete(memory, &id)).await?;
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
    as_pane(&authorize(&state, &headers)?, &pane)?;
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
    as_pane(&authorize(&state, &headers)?, &pane)?;
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
    as_pane(&authorize(&state, &headers)?, &pane)?;
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
    as_pane(&authorize(&state, &headers)?, &pane)?;
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
    let caller = authorize(&state, &headers)?;
    if !args.is_object() {
        args = json!({});
    }
    stamp_caller(&mut args, &caller);
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

fn control_failed(err: ControlError) -> ApiError {
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
    tokens: Arc<Tokens>,
    pane_alive: Box<dyn Fn(&str) -> bool + Send + Sync>,
    open: Box<dyn Fn(Value) -> WindowAnswer + Send + Sync>,
}

#[derive(Deserialize)]
struct KitTabQuery {
    /// `$HANGAR_PANE_ID`, from the hook's environment. A pane's own token
    /// says it already: then this may be left out, and may not name another.
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
    let pane = match route.tokens.caller(&headers)? {
        Caller::App => q.pane,
        Caller::Pane(me) if q.pane.is_empty() || q.pane == me.id => me.id,
        caller => {
            as_pane(&caller, &q.pane)?;
            q.pane
        }
    };
    if !(route.pane_alive)(&pane) {
        return Err(bad(
            StatusCode::NOT_FOUND,
            format!(
                "no pane {pane:?} is running in Hangar: pass ?pane=$HANGAR_PANE_ID, from the pane the hook runs in",
            ),
        ));
    }
    (route.open)(kit_tab_args(&pane, body))
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
// Only the desktop build reads past the path: a headless one refuses the share.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
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
    Json(mut body): Json<PhoneShareBody>,
) -> ApiResult {
    let caller = authorize(&state, &headers)?;
    if let Some(me) = caller.signature() {
        body.pane_name = me.to_string();
    }
    if body.path.trim().is_empty() {
        return Err(bad(StatusCode::BAD_REQUEST, "missing 'path'"));
    }
    share_with_phones(&state, body, q.cwd).await
}

#[cfg(feature = "desktop")]
async fn share_with_phones(state: &ApiState, body: PhoneShareBody, workspace: String) -> ApiResult {
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
            workspace,
            pane: body.pane_name,
        })
        .await
        .map_err(|err| bad(StatusCode::BAD_REQUEST, err))?;
    Ok(Json(json!({ "shared": share })))
}

/// A headless build has no Hangar Mobile: the route stays, so an agent's MCP
/// tool gets an answer that says why rather than a 404.
#[cfg(not(feature = "desktop"))]
async fn share_with_phones(_: &ApiState, _: PhoneShareBody, _: String) -> ApiResult {
    Err(bad(
        StatusCode::SERVICE_UNAVAILABLE,
        "Hangar Mobile is not part of this build",
    ))
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

/// Binds on an ephemeral loopback port and returns it, so nothing has to
/// negotiate a fixed port that might already be taken. `token` is Hangar's own
/// (`Caller::App`); the panes' tokens come from `pane_tokens`, and the one for
/// other processes is `outside_token`.
pub async fn start(reach: Reach, store: &'static BoardStore, token: String) -> Result<u16, String> {
    #[cfg(feature = "desktop")]
    let Reach { host, pty, remote } = reach;
    #[cfg(not(feature = "desktop"))]
    let Reach { host, pty } = reach;
    let tokens = Arc::new(Tokens {
        app: token,
        outside: outside_token(),
        panes: pane_tokens(),
    });
    let kit_tabs = KitTabRoute {
        tokens: tokens.clone(),
        pane_alive: Box::new(move |pane| crate::pty::pane_alive(&pty, pane)),
        open: {
            let host = host.clone();
            Box::new(move |args| {
                let host = host.clone();
                Box::pin(async move { ask_kit_tab_open(&host, args).await })
            })
        },
    };
    let state = Arc::new(ApiState {
        store,
        tokens,
        host,
        #[cfg(feature = "desktop")]
        remote,
    });
    let router = router(state, kit_tabs);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    Ok(port)
}

fn router(state: Arc<ApiState>, kit_tabs: KitTabRoute) -> Router {
    Router::new()
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
        .route("/api/tasks/{id}/progress", post(progress_task))
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
        .merge(kit_tab_router(kit_tabs))
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
    /// Hangar's own token, as `start` is given it.
    const APP: &str = "secret";
    /// The token endpoint.json would advertise.
    const OUTSIDE: &str = "the-outside-token";

    fn tokens_with(panes: &'static PaneTokens) -> Arc<Tokens> {
        Arc::new(Tokens {
            app: APP.to_string(),
            outside: OUTSIDE,
            panes,
        })
    }

    fn tokens() -> Arc<Tokens> {
        tokens_with(Box::leak(Box::default()))
    }

    /// The route, a pane `LIVE` running and a window that answers: what it was
    /// asked goes to the list handed back.
    fn route(answer: Result<Value, ControlError>) -> (Router, Arc<Mutex<Vec<Value>>>) {
        route_with(tokens(), answer)
    }

    fn route_with(
        tokens: Arc<Tokens>,
        answer: Result<Value, ControlError>,
    ) -> (Router, Arc<Mutex<Vec<Value>>>) {
        let asked: Arc<Mutex<Vec<Value>>> = Arc::default();
        let kept = asked.clone();
        let answer = Mutex::new(Some(answer));
        let router = kit_tab_router(KitTabRoute {
            tokens,
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

    // -----------------------------------------------------------------------
    // Who is calling, as the token says
    // -----------------------------------------------------------------------

    /// The API over a board and a memory of its own, as `start` serves it,
    /// with the pane `p-gus` ("Gus") holding a token of its own.
    struct Api {
        router: Router,
        host: Host,
        store: &'static BoardStore,
        panes: &'static PaneTokens,
        cwd: String,
        gus: String,
    }

    fn api() -> Api {
        let root = crate::testdir::scratch("server");
        let cwd = root.join("ws");
        std::fs::create_dir_all(&cwd).unwrap();
        let host = Host::with_paths(crate::paths::Paths::rooted(&root.join("data")));
        let store: &'static BoardStore = Box::leak(Box::default());
        let panes: &'static PaneTokens = Box::leak(Box::default());
        let gus = panes.issue("p-gus", Some("Gus"));
        let tokens = tokens_with(panes);
        let state = Arc::new(ApiState {
            store,
            tokens: tokens.clone(),
            host: host.clone(),
            #[cfg(feature = "desktop")]
            remote: None,
        });
        let kit_tabs = KitTabRoute {
            tokens,
            pane_alive: Box::new(|_| true),
            open: Box::new(|args| Box::pin(async move { Ok(args) })),
        };
        Api {
            router: router(state, kit_tabs),
            host,
            store,
            panes,
            cwd: cwd.to_string_lossy().into_owned(),
            gus,
        }
    }

    impl Api {
        fn uri(&self, path: &str) -> String {
            let sep = if path.contains('?') { '&' } else { '?' };
            format!("{path}{sep}cwd={}", crate::mcp::urlencode(&self.cwd))
        }

        async fn call(
            &self,
            method: &str,
            path: &str,
            token: &str,
            body: Option<Value>,
        ) -> (StatusCode, Value) {
            let request = Request::builder()
                .method(method)
                .uri(self.uri(path))
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"));
            let request = request
                .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
                .unwrap();
            send(self.router.clone(), request).await
        }

        /// A new task, made with Hangar's own token.
        async fn task(&self) -> String {
            let (status, body) = self
                .call("POST", "/api/tasks", APP, Some(json!({ "title": "t" })))
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            body["task"]["id"].as_str().unwrap().to_string()
        }
    }

    #[tokio::test]
    async fn a_pane_token_claims_and_comments_as_its_pane_whatever_the_body_says() {
        let api = api();
        let id = api.task().await;
        let (status, body) = api
            .call(
                "POST",
                &format!("/api/tasks/{id}/claim"),
                &api.gus,
                Some(json!({ "agent": "Leo" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["task"]["assignee"], "Gus");

        let (status, body) = api
            .call(
                "POST",
                &format!("/api/tasks/{id}/comment"),
                &api.gus,
                Some(json!({ "author": "user", "text": "done" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["task"]["comments"][0]["author"], "Gus");
    }

    /// Reports `percent` on task `id` with `token` and a body signed `by`.
    async fn report(
        api: &Api,
        id: &str,
        token: &str,
        by: &str,
        percent: i64,
    ) -> (StatusCode, Value) {
        api.call(
            "POST",
            &format!("/api/tasks/{id}/progress"),
            token,
            Some(json!({ "percent": percent, "note": " main change done ", "by": by })),
        )
        .await
    }

    /// A task claimed by `agent` with Hangar's own token.
    async fn held_by(api: &Api, agent: &str) -> String {
        let id = api.task().await;
        let (status, body) = api
            .call(
                "POST",
                &format!("/api/tasks/{id}/claim"),
                APP,
                Some(json!({ "agent": agent })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        id
    }

    #[tokio::test]
    async fn a_pane_token_reports_progress_as_its_pane_on_its_own_task() {
        let api = api();
        let id = held_by(&api, "Gus").await;
        let (status, body) = report(&api, &id, &api.gus, "Leo", 150).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let progress = &body["task"]["progress"];
        assert_eq!(progress["by"], "Gus");
        assert_eq!(progress["percent"], 100);
        assert_eq!(progress["note"], "main change done");

        let (_, board) = api.call("GET", "/api/board", APP, None).await;
        assert_eq!(board["tasks"][0]["progress"]["by"], "Gus");
    }

    #[tokio::test]
    async fn progress_is_refused_on_a_task_the_caller_does_not_hold() {
        let api = api();
        let id = held_by(&api, "Leo").await;
        // No window answers: Gus cannot be the orchestrator.
        let (status, body) = report(&api, &id, &api.gus, "Leo", 10).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("held by Leo"),
            "{body}"
        );

        let (status, _) = report(&api, &id, OUTSIDE, "claude-1", 10).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = report(&api, &id, OUTSIDE, "Leo", 10).await;
        assert_eq!(status, StatusCode::OK);
        // Hangar's own token reports on any task in doing.
        let (status, body) = report(&api, &id, APP, "hangar", 20).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["task"]["progress"]["by"], "hangar");

        let todo = api.task().await;
        let (status, body) = report(&api, &todo, APP, "hangar", 20).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap().contains("not doing"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn the_orchestrator_s_pane_reports_on_a_task_it_does_not_hold() {
        let api = api();
        let id = held_by(&api, "Sam").await;
        let mut events = api.host.subscribe();
        let window = tokio::spawn(async move {
            loop {
                if let Ok(crate::host::Event::ControlInternal(request)) = events.recv().await {
                    assert_eq!(request.op, "orchestrator_briefing");
                    let role = if request.args["paneId"] == "p-gus" {
                        "orchestrator"
                    } else {
                        "worker"
                    };
                    let answer = json!({ "role": role, "system": "rules", "context": "" });
                    crate::control::control_reply(request.id, true, Some(answer), None);
                }
            }
        });
        let (status, body) = report(&api, &id, &api.gus, "Sam", 60).await;
        window.abort();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["task"]["progress"]["by"], "Gus");
        assert_eq!(body["task"]["progress"]["percent"], 60);
    }

    #[tokio::test]
    async fn the_outside_token_names_itself_but_hangar_s_own_is_taken_at_its_word() {
        let api = api();
        for (token, agent) in [(OUTSIDE, "claude-1"), (APP, "Leo")] {
            let id = api.task().await;
            let (status, body) = api
                .call(
                    "POST",
                    &format!("/api/tasks/{id}/claim"),
                    token,
                    Some(json!({ "agent": agent })),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body["task"]["assignee"], agent);
        }
    }

    #[tokio::test]
    async fn no_http_caller_lifts_a_block_for_the_person() {
        let api = api();
        let id = api.task().await;
        let path = format!("/api/tasks/{id}");
        let (status, body) = api
            .call(
                "PATCH",
                &path,
                &api.gus,
                Some(json!({ "block": { "reason": "their call", "human_only": true, "by": "user", "person": true } })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["task"]["blocked"]["by"], "Gus");
        assert_eq!(body["task"]["blocked"]["humanOnly"], true);

        for token in [api.gus.as_str(), OUTSIDE, APP] {
            let (status, body) = api
                .call(
                    "PATCH",
                    &path,
                    token,
                    Some(json!({ "unblock": { "person": true, "by": "user" } })),
                )
                .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{token}: {body}");
        }
        let board = api.store.read(&api.cwd);
        assert!(board.tasks[0].blocked.is_some());
    }

    #[tokio::test]
    async fn a_fact_is_signed_by_the_pane_that_wrote_it() {
        let api = api();
        let (status, body) = api
            .call(
                "POST",
                "/api/memory",
                &api.gus,
                Some(json!({ "title": "Port", "content": "21200", "agent": "Leo" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["entry"]["agent"], "Gus");
    }

    /// What the window is asked by `/api/control/pane_list` with `token` and a
    /// body that says it is the orchestrator.
    async fn control_caller(api: &Api, token: &str) -> Value {
        let mut events = api.host.subscribe();
        let window = tokio::spawn(async move {
            loop {
                if let Ok(crate::host::Event::ControlRequest(request)) = events.recv().await {
                    let args = request.args.clone();
                    crate::control::control_reply(request.id, true, Some(request.args), None);
                    return args;
                }
            }
        });
        let (status, body) = api
            .call(
                "POST",
                "/api/control/pane_list",
                token,
                Some(json!({ "caller": { "paneId": "p-leo", "paneName": "Leo", "cwd": "/w" } })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        window.await.unwrap()["caller"].clone()
    }

    #[tokio::test]
    async fn a_pane_token_makes_the_control_caller_its_pane() {
        let api = api();
        assert_eq!(
            control_caller(&api, &api.gus).await,
            json!({ "paneId": "p-gus", "paneName": "Gus", "cwd": "/w" })
        );
        // The outside token is no pane: no role gate of the window lets it by.
        assert_eq!(control_caller(&api, OUTSIDE).await, json!({ "cwd": "/w" }));
        assert_eq!(
            control_caller(&api, APP).await,
            json!({ "paneId": "p-leo", "paneName": "Leo", "cwd": "/w" })
        );
    }

    #[tokio::test]
    async fn a_pane_token_speaks_for_its_own_pane_on_the_bridge() {
        let api = api();
        let other = "/api/bridge/p-leo/conflict?file=a.rs";
        let own = "/api/bridge/p-gus/conflict?file=a.rs";
        assert_eq!(
            api.call("GET", other, &api.gus, None).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            api.call("GET", own, OUTSIDE, None).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(api.call("GET", own, &api.gus, None).await.0, StatusCode::OK);
        assert_eq!(api.call("GET", other, APP, None).await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_and_revoked_tokens_are_refused() {
        let api = api();
        for token in ["", "nope", &api.gus[..10]] {
            let (status, _) = api.call("GET", "/api/board", token, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
        }
        assert_eq!(api.panes.issue("p-gus", Some("Gus")), api.gus);
        api.panes.revoke("p-gus");
        let (status, _) = api.call("GET", "/api/board", &api.gus, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_pane_token_opens_tabs_for_its_own_pane_only() {
        let panes: &'static PaneTokens = Box::leak(Box::default());
        let token = panes.issue(LIVE, None);
        for (uri, expected) in [
            ("/api/kit/tab/open", StatusCode::OK),
            (OPEN, StatusCode::OK),
            ("/api/kit/tab/open?pane=p-other", StatusCode::FORBIDDEN),
        ] {
            let (router, asked) = route_with(tokens_with(panes), Ok(json!({})));
            let (status, _) = send(router, request(uri, Some(&token), run())).await;
            assert_eq!(status, expected, "{uri}");
            if expected == StatusCode::OK {
                assert_eq!(asked.lock().unwrap()[0]["caller"]["paneId"], LIVE);
            }
        }
        let (router, asked) = route_with(tokens_with(panes), Ok(json!({})));
        let (status, _) = send(router, request(OPEN, Some(OUTSIDE), run())).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(asked.lock().unwrap().is_empty());
    }

    #[test]
    fn a_pane_s_name_is_the_one_given_last_and_signs_for_it() {
        let panes = PaneTokens::default();
        let token = panes.issue("p-1", Some("  "));
        assert_eq!(panes.identify(&token).unwrap().signature(), "p-1");
        assert_eq!(panes.issue("p-1", Some("Ada")), token);
        assert_eq!(panes.identify(&token).unwrap().signature(), "Ada");
        assert_ne!(panes.issue("p-2", None), token);
    }

    // -----------------------------------------------------------------------
    // The stores' work is off the runtime
    // -----------------------------------------------------------------------

    /// How long the store's lock is held by someone else.
    const HELD: std::time::Duration = std::time::Duration::from_millis(1500);

    /// Sends `path` while `hold` keeps a store's lock (it calls the closure it
    /// is given while holding it), and checks that the runtime — one thread,
    /// as `#[tokio::test]` builds it — still answers `/health` meanwhile, then
    /// the request once the lock is free. A handler that waited on the lock on
    /// the runtime's thread would hold `/health` up for as long.
    async fn answers_while_held(
        api: &Api,
        path: &str,
        hold: impl FnOnce(Box<dyn FnOnce() + Send>) + Send + 'static,
    ) {
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            hold(Box::new(move || {
                held_tx.send(()).unwrap();
                std::thread::sleep(HELD);
            }))
        });
        held_rx.recv().unwrap();

        let waiting = tokio::spawn({
            let router = api.router.clone();
            let request = Request::get(api.uri(path))
                .header("authorization", format!("Bearer {APP}"))
                .body(Body::empty())
                .unwrap();
            async move { send(router, request).await }
        });
        tokio::task::yield_now().await;

        let asked = std::time::Instant::now();
        let (status, _) = send(
            api.router.clone(),
            Request::get("/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            asked.elapsed() < HELD / 3,
            "/health waited {:?} on {path}",
            asked.elapsed()
        );
        assert!(!waiting.is_finished(), "{path} did not wait on the lock");

        let (status, body) = waiting.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        holder.join().unwrap();
    }

    #[tokio::test]
    async fn a_board_route_waiting_on_the_lock_leaves_the_runtime_free() {
        let api = api();
        let (store, cwd) = (api.store, api.cwd.clone());
        answers_while_held(&api, "/api/board", move |held| {
            let _ = store.update(&cwd, |_| -> Result<(), String> {
                held();
                Err("held only".to_string())
            });
        })
        .await;
    }

    #[tokio::test]
    async fn a_memory_route_waiting_on_the_lock_leaves_the_runtime_free() {
        let api = api();
        let dir = memory::dir(&api.host).unwrap();
        answers_while_held(&api, "/api/memory", move |held| {
            let _ = memory::store().update(&dir, |_| -> Result<(), String> {
                held();
                Err("held only".to_string())
            });
        })
        .await;
    }
}
