//! The node API (docs/native/nodes.md, section 3): HTTP and WebSocket on
//! 127.0.0.1, reached by the desktop through its SSH tunnel, every route
//! behind the node's Bearer token (`node.json`).
//!
//! - `GET /node/info`: version, target, hostname, home, `claude` and `git` on
//!   PATH, how many desktops drive its orchestration;
//! - `GET /node/panes`, `POST /node/panes` (spawn), `POST
//!   /node/panes/{id}/input` `{data}`, `POST /node/panes/{id}/resize`
//!   `{cols, rows}`, `DELETE /node/panes/{id}`;
//! - `GET /node/panes/{id}/stream?from=<seq>` (WebSocket): `{"reset":
//!   {"start"}}` when `from` fell out of the history, the history after it as
//!   `{"seq", "data"}` frames, then the live ones and `{"exit": {"code"}}`;
//!   `{"input": "…"}` and `{"resize": {"cols", "rows"}}` frames are taken on
//!   the same socket;
//! - `GET /node/events` (WebSocket): core's events (events.rs);
//! - `GET /node/fs?path=`: a folder's entries (files.rs);
//! - `GET /node/chat`, `GET /node/context`, `GET /node/sessions`: the Claude
//!   Code transcripts of the node (transcripts.rs);
//! - `GET /node/memory`: the node's whole memory, contents included, for the
//!   desktop's memory view (core's `/api/memory` index leaves them out, and
//!   its writes are what the view sends);
//! - `GET /node/events?control=1`: the same, from a desktop that drives the
//!   node's orchestration: core's control requests are its to answer, with
//!   `POST /node/control/{id}/reply` (control.rs);
//! - `/node/worktree`, `/node/branch`, `/node/git`, `/node/board/*`,
//!   `/node/bridge/{pane}`, `/node/delivery/*`: what the desktop's
//!   orchestration engine does on the node's disk (engine.rs);
//! - `/api/*`: core's own API, relayed (proxy.rs).
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::ws::rejection::WebSocketUpgradeRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use hangar_core::pty::PtyManager;
use hangar_core::Host;
use serde::Deserialize;
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tokio::sync::broadcast::error::RecvError;

use crate::panes::{Frame, Pane, Panes, Spawn};
use crate::proxy;

pub struct Node {
    token: String,
    host: Host,
    pub panes: Panes,
    core: proxy::Core,
    home: PathBuf,
    /// The desktops answering core's control requests (control.rs).
    pub drivers: Arc<crate::control::Drivers>,
}

impl Node {
    pub fn new(token: String, host: Host, pty: Arc<PtyManager>, core: proxy::Core) -> Self {
        Self {
            token,
            host,
            panes: Panes::new(pty),
            core,
            home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
            drivers: Arc::default(),
        }
    }

    pub fn host(&self) -> &Host {
        &self.host
    }
}

pub type Shared = Arc<Node>;

pub fn router(node: Shared) -> Router {
    Router::new()
        .route("/node/info", get(info))
        .route("/node/panes", get(list_panes).post(spawn))
        .route("/node/panes/{id}", delete(kill))
        .route("/node/panes/{id}/input", post(input))
        .route("/node/panes/{id}/resize", post(resize))
        .route("/node/panes/{id}/stream", get(stream))
        .route("/node/events", get(events))
        .route("/node/fs", get(fs))
        .route("/node/chat", get(crate::transcripts::chat))
        .route("/node/context", get(crate::transcripts::context))
        .route("/node/sessions", get(crate::transcripts::sessions))
        .route("/node/memory", get(memory))
        .route("/node/control/{id}/reply", post(crate::control::reply))
        .route(
            "/node/worktree",
            post(crate::engine::worktree_add).delete(crate::engine::worktree_remove),
        )
        .route("/node/worktree/hook", post(crate::engine::worktree_hook))
        .route("/node/worktree/copy", post(crate::engine::worktree_copy))
        .route("/node/branch", delete(crate::engine::branch_delete))
        .route("/node/git", post(crate::engine::git))
        .route(
            "/node/board/claim-worker",
            post(crate::engine::claim_worker),
        )
        .route(
            "/node/board/release-worker",
            post(crate::engine::release_worker),
        )
        .route("/node/board/update", post(crate::engine::board_update))
        .route("/node/board/comment", post(crate::engine::board_comment))
        .route("/node/bridge/{pane}", post(crate::engine::enqueue))
        .route(
            "/node/delivery/preflight",
            post(crate::engine::delivery_preflight),
        )
        .route("/node/delivery/run", post(crate::engine::delivery_run))
        .route(
            "/node/delivery/cleanup",
            post(crate::engine::delivery_cleanup),
        )
        .route(
            "/node/delivery/running",
            get(crate::engine::delivery_running),
        )
        .route("/api/{*rest}", any(relay))
        .layer(middleware::from_fn_with_state(node.clone(), authorize))
        .with_state(node)
}

pub fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// Same length or not, the bytes compared in constant time: the token is
/// all that keeps another process of the machine out of the shells.
fn same_token(expected: &str, headers: &HeaderMap) -> bool {
    let provided = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    !expected.is_empty() && bool::from(expected.as_bytes().ct_eq(provided.as_bytes()))
}

async fn authorize(State(node): State<Shared>, request: Request, next: Next) -> Response {
    if same_token(&node.token, request.headers()) {
        next.run(request).await
    } else {
        error(StatusCode::UNAUTHORIZED, "invalid token")
    }
}

/// A blocking call of core's (a spawn, a kill, a write the shell is slow to
/// read), off the runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Failed> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| Failed(err.to_string()))
}

/// A blocking call that panicked or was cancelled.
struct Failed(String);

impl IntoResponse for Failed {
    fn into_response(self) -> Response {
        error(StatusCode::INTERNAL_SERVER_ERROR, &self.0)
    }
}

fn on_path(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(dir.join(binary))
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
    })
}

fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer's length is passed with it.
    let named = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    if !named {
        return String::new();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

async fn info(State(node): State<Shared>) -> Response {
    // The login shell's PATH, which core gives the process (path_env.rs)
    // before any pane starts, so a tool installed by the profile counts.
    let tools = blocking(|| {
        hangar_core::path_env::ensure();
        (on_path("claude"), on_path("git"))
    })
    .await;
    let (claude, git) = match tools {
        Ok(tools) => tools,
        Err(failed) => return failed.into_response(),
    };
    Json(json!({
        "version": crate::VERSION,
        "target": crate::TARGET,
        "hostname": hostname(),
        "home": node.home.to_string_lossy(),
        "claude": claude,
        "git": git,
        "pid": std::process::id(),
        // The desktops answering its control requests (control.rs).
        "drivers": node.drivers.connected(),
    }))
    .into_response()
}

async fn list_panes(State(node): State<Shared>) -> Response {
    Json(json!({ "panes": node.panes.list() })).into_response()
}

#[derive(Deserialize)]
struct ShellBody {
    program: String,
    #[serde(default)]
    args: Vec<String>,
}

/// The workspace's MCP servers for the pane's Claude Code, written by the
/// node (hangar-core's mcp_scope.rs) so the file names `hangar-node --mcp`
/// for Hangar's own server: the node's executable, not the desktop's.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpBody {
    workspace_id: String,
    workspace_cwd: String,
    /// The servers the workspace chose; empty for Hangar's own alone.
    #[serde(default)]
    enabled: Vec<String>,
    /// Loaded with `--strict-mcp-config`: the kits' servers go in the file.
    #[serde(default)]
    strict: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpawnBody {
    id: String,
    cwd: String,
    #[serde(default)]
    shell: Option<ShellBody>,
    cols: u16,
    rows: u16,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    /// Variables for a worker's pane (core's `build_env`): they apply to a
    /// pane with `portBase` only, as on the desktop.
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    port_base: Option<u16>,
    #[serde(default)]
    bridge: Option<bool>,
    #[serde(default)]
    kits: Option<Vec<String>>,
    #[serde(default)]
    claude_account: Option<String>,
    #[serde(default)]
    mcp: Option<McpBody>,
}

async fn spawn(State(node): State<Shared>, Json(body): Json<SpawnBody>) -> Response {
    if body.id.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "a pane needs an id");
    }
    if !std::path::Path::new(&body.cwd).is_dir() {
        return error(StatusCode::BAD_REQUEST, "the pane's folder does not exist");
    }
    let shell = body.shell.map(|shell| hangar_core::shells::ShellInfo {
        id: shell.program.clone(),
        label: shell.program.clone(),
        program: shell.program,
        args: shell.args,
    });
    let host = node.host.clone();
    let worker = node.clone();
    let outcome = blocking(move || {
        let mcp = match body.mcp {
            Some(mcp) => Some(hangar_core::mcp_scope::mcp_scope_write(
                &host,
                mcp.workspace_id,
                mcp.workspace_cwd,
                body.cwd.clone(),
                mcp.enabled,
                mcp.strict.then(|| body.kits.clone().unwrap_or_default()),
                body.claude_account.clone(),
            )?),
            None => None,
        };
        let spawned = worker.panes.spawn(Spawn {
            id: body.id,
            cwd: body.cwd,
            shell,
            cols: body.cols,
            rows: body.rows,
            name: body.name,
            agent: body.agent,
            bridge: body.bridge,
            port_base: body.port_base,
            build_env: body.env.into_iter().collect(),
            mcp_config: mcp.as_ref().map(|file| file.path.clone()),
            kits: body.kits,
            claude_account: body.claude_account,
        })?;
        Ok::<_, String>((spawned, mcp))
    })
    .await;
    match outcome {
        Ok(Ok((spawned, mcp))) => Json(json!({ "spawned": spawned, "mcp": mcp })).into_response(),
        Ok(Err(err)) => error(StatusCode::CONFLICT, &err),
        Err(failed) => failed.into_response(),
    }
}

#[derive(Deserialize)]
struct InputBody {
    data: String,
}

fn done(outcome: Result<Result<(), String>, Failed>) -> Response {
    match outcome {
        Ok(Ok(())) => Json(json!({ "ok": true })).into_response(),
        Ok(Err(err)) => error(StatusCode::NOT_FOUND, &err),
        Err(failed) => failed.into_response(),
    }
}

async fn input(
    State(node): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<InputBody>,
) -> Response {
    done(blocking(move || node.panes.write(&id, body.data)).await)
}

#[derive(Deserialize)]
struct ResizeBody {
    cols: u16,
    rows: u16,
}

async fn resize(
    State(node): State<Shared>,
    Path(id): Path<String>,
    Json(body): Json<ResizeBody>,
) -> Response {
    done(blocking(move || node.panes.resize(&id, body.cols, body.rows)).await)
}

async fn kill(State(node): State<Shared>, Path(id): Path<String>) -> Response {
    match blocking(move || node.panes.kill(&id)).await {
        Ok(true) => Json(json!({ "ok": true })).into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "pty not found"),
        Err(failed) => failed.into_response(),
    }
}

#[derive(Deserialize)]
struct StreamQuery {
    #[serde(default)]
    from: u64,
}

async fn stream(
    State(node): State<Shared>,
    Path(id): Path<String>,
    Query(query): Query<StreamQuery>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    // An unknown pane first, whether or not the request upgrades.
    let Some(pane) = node.panes.get(&id) else {
        return error(StatusCode::NOT_FOUND, "pty not found");
    };
    let upgrade = match upgrade {
        Ok(upgrade) => upgrade,
        Err(rejection) => return rejection.into_response(),
    };
    upgrade.on_upgrade(move |socket| watch(node, id, pane, query.from, socket))
}

fn text(value: Value) -> Message {
    Message::Text(value.to_string().into())
}

/// What a stream is sent from `from` on: the history (a reset first when it
/// no longer reaches back that far), then whether the pane ended. `None`
/// once the socket is gone; the last `seq` sent, and whether it ended,
/// otherwise.
async fn catch_up(
    socket: &mut WebSocket,
    attached: &crate::panes::Attached,
    mut last: u64,
) -> Option<(u64, bool)> {
    if let Some(start) = attached.replay.reset {
        socket
            .send(text(json!({ "reset": { "start": start } })))
            .await
            .ok()?;
    }
    for (seq, data) in &attached.replay.chunks {
        socket
            .send(text(json!({ "seq": seq, "data": data })))
            .await
            .ok()?;
        last = *seq;
    }
    if let Some(code) = attached.exit {
        socket
            .send(text(json!({ "exit": { "code": code } })))
            .await
            .ok()?;
        return Some((last, true));
    }
    Some((last, false))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Incoming {
    Input(String),
    Resize { cols: u16, rows: u16 },
}

async fn watch(node: Shared, id: String, pane: Arc<Pane>, from: u64, mut socket: WebSocket) {
    let mut attached = pane.attach(from);
    let Some((mut last, ended)) = catch_up(&mut socket, &attached, from).await else {
        return;
    };
    if ended {
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    loop {
        tokio::select! {
            frame = attached.live.recv() => match frame {
                Ok(Frame::Output { seq, data }) => {
                    if seq <= last {
                        continue;
                    }
                    let frame = text(json!({ "seq": seq, "data": &*data }));
                    if socket.send(frame).await.is_err() {
                        return;
                    }
                    last = seq;
                }
                Ok(Frame::Exit { code }) => {
                    let _ = socket.send(text(json!({ "exit": { "code": code } }))).await;
                    break;
                }
                // Fell behind the live frames: what it missed is still in
                // the history, unless it fell behind that too.
                Err(RecvError::Lagged(_)) => {
                    attached = pane.attach(last);
                    match catch_up(&mut socket, &attached, last).await {
                        Some((seq, false)) => last = seq,
                        Some((_, true)) | None => break,
                    }
                }
                Err(RecvError::Closed) => break,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Text(raw))) => {
                    let Ok(incoming) = serde_json::from_str::<Incoming>(raw.as_str()) else {
                        let refusal = json!({ "error": "expected {\"input\"} or {\"resize\"}" });
                        if socket.send(text(refusal)).await.is_err() {
                            return;
                        }
                        continue;
                    };
                    let node = node.clone();
                    let id = id.clone();
                    let done = tokio::task::spawn_blocking(move || match incoming {
                        Incoming::Input(data) => node.panes.write(&id, data),
                        Incoming::Resize { cols, rows } => node.panes.resize(&id, cols, rows),
                    })
                    .await;
                    if let Ok(Err(err)) = done {
                        if socket.send(text(json!({ "error": err }))).await.is_err() {
                            return;
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

#[derive(Deserialize)]
struct EventsQuery {
    /// `1`: the desktop driving the node's orchestration follows.
    #[serde(default)]
    control: Option<String>,
}

async fn events(
    State(node): State<Shared>,
    Query(query): Query<EventsQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let receiver = node.host.subscribe();
    let driver = (query.control.as_deref() == Some("1")).then(|| node.drivers.attach());
    upgrade.on_upgrade(move |socket| forward_events(receiver, socket, driver))
}

/// `_driver` is held for as long as the socket is up: while it is, core's
/// control requests have a desktop to answer them.
async fn forward_events(
    mut receiver: tokio::sync::broadcast::Receiver<hangar_core::Event>,
    mut socket: WebSocket,
    _driver: Option<crate::control::Driver>,
) {
    loop {
        tokio::select! {
            event = receiver.recv() => {
                let frame = match event {
                    Ok(event) => crate::events::to_json(&event),
                    // A change notice lost is a reload for the desktop.
                    Err(RecvError::Lagged(missed)) => json!({ "lagged": missed }),
                    Err(RecvError::Closed) => break,
                };
                if socket.send(text(frame)).await.is_err() {
                    return;
                }
            }
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}

#[derive(Deserialize)]
struct FsQuery {
    path: Option<String>,
}

async fn fs(State(node): State<Shared>, Query(query): Query<FsQuery>) -> Response {
    let home = node.home.clone();
    match blocking(move || crate::files::list(&home, query.path.as_deref())).await {
        Ok(Ok(listing)) => Json(listing).into_response(),
        Ok(Err(crate::files::Refusal::OutsideHome)) => error(
            StatusCode::FORBIDDEN,
            "outside the home directory: ask for an absolute path",
        ),
        Ok(Err(crate::files::Refusal::Unreadable(err))) => error(StatusCode::NOT_FOUND, &err),
        Err(failed) => failed.into_response(),
    }
}

/// The memory as the desktop's memory view draws it: every entry with its
/// content, in one answer. An unreadable store is an error, never an empty
/// list: the view says so rather than show a memory that seems blank.
async fn memory(State(node): State<Shared>) -> Response {
    let host = node.host.clone();
    match blocking(move || hangar_core::memory::memory_load(&host)).await {
        Ok(Ok(memory)) => Json(json!({ "entries": memory.entries })).into_response(),
        Ok(Err(err)) => error(StatusCode::INTERNAL_SERVER_ERROR, &err),
        Err(failed) => failed.into_response(),
    }
}

async fn relay(State(node): State<Shared>, request: Request) -> Response {
    proxy::forward(&node.core, request).await
}
