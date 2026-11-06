//! What a workspace on a node asks of it (docs/native/nodes.md §5): its
//! panes started or found again, their output followed over the stream
//! WebSocket from the last byte seen, what is typed sent back on it, and the
//! node's events.
//!
//! The tunnel's local port changes when a node reconnects, so nothing here
//! keeps a [`NodeClient`]: each attempt asks the [`directory`] for the
//! node's current one, which the connection keeps (`nodes::apply_status`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::model::NodeClient;
use super::wire::{self, Incoming, Socket, WsError};

// The Claude Code transcripts of a remote workspace (nodes/chat.rs): the
// context gauge, the session hunt and picker, and the chat view's
// conversation ask the node, not this machine's disk.
#[path = "chat.rs"]
mod chat;
#[allow(
    unused_imports,
    reason = "for the chat view's conversation, which calls it next"
)]
pub use chat::chat;
pub use chat::{client_of, context, sessions};

/// The connected nodes' clients, by node id: what a pane's stream, started
/// once, reaches its node with however many times the tunnel came back.
/// The process's, as the tunnels are.
static DIRECTORY: LazyLock<RwLock<HashMap<String, NodeClient>>> = LazyLock::new(Default::default);

/// The client of node `node` while it is connected.
pub fn directory(node: &str) -> Option<NodeClient> {
    DIRECTORY
        .read()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(node)
        .cloned()
}

/// Node `node` is reached through `client` from now on, or not at all.
pub fn publish(node: &str, client: Option<NodeClient>) {
    let mut map = DIRECTORY
        .write()
        .unwrap_or_else(|poison| poison.into_inner());
    match client {
        Some(client) => map.insert(node.to_owned(), client),
        None => map.remove(node),
    };
}

/// How long to wait between two attempts at a node that does not answer.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    pub first: Duration,
    pub max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            first: Duration::from_millis(500),
            max: Duration::from_secs(10),
        }
    }
}

impl Backoff {
    fn next(&self, wait: Duration) -> Duration {
        (wait * 2).min(self.max)
    }
}

/// The MCP scope a Claude Code pane of a remote workspace gets: written by
/// the node, so that it names the node's own `hangar-node --mcp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMcp {
    pub workspace_id: String,
    pub workspace_cwd: String,
    pub enabled: Vec<String>,
    pub strict: bool,
}

/// What `POST /node/panes` is sent: the parameters a local spawn carries
/// that mean something on the node. The kits, the build cache and the
/// Claude accounts are this machine's, and stay here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSpawn {
    pub id: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub name: String,
    pub agent: String,
    pub bridge: bool,
    pub port_base: Option<u16>,
    pub mcp: Option<RemoteMcp>,
    /// Variables for a worker's shell (the node applies them to a pane with
    /// a `port_base`): a worker sent from a local workspace gets
    /// [`BOARD_RELAY`], so its board is this machine's.
    pub env: Vec<(String, String)>,
}

/// Set in the shell of a worker a local workspace sent to a node: its MCP
/// proxy sends its board's calls to this machine (hangar-core's mcp.rs,
/// `board_relay`), where its task is, rather than to the node's board.
pub const BOARD_RELAY: &str = "HANGAR_BOARD_RELAY";

impl RemoteSpawn {
    pub fn body(&self) -> Value {
        let mut body = json!({
            "id": self.id,
            "cwd": self.cwd,
            "cols": self.cols,
            "rows": self.rows,
            "name": self.name,
            "agent": self.agent,
            "bridge": self.bridge,
        });
        if let Some(port) = self.port_base {
            body["portBase"] = json!(port);
        }
        if !self.env.is_empty() {
            body["env"] = self
                .env
                .iter()
                .map(|(name, value)| (name.clone(), json!(value)))
                .collect::<serde_json::Map<_, _>>()
                .into();
        }
        if let Some(mcp) = &self.mcp {
            body["mcp"] = json!({
                "workspaceId": mcp.workspace_id,
                "workspaceCwd": mcp.workspace_cwd,
                "enabled": mcp.enabled,
                "strict": mcp.strict,
            });
        }
        body
    }
}

/// The MCP file the node wrote for a spawn.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteScope {
    pub path: String,
    pub servers: Vec<String>,
    pub missing: Vec<String>,
}

/// How a pane of a remote workspace started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// A new shell, with the MCP file the node wrote for it, if asked.
    Spawned { mcp: Option<RemoteScope> },
    /// Its shell was still running on the node (the desktop restarted, or
    /// the workspace was opened again): the pane follows it, and its
    /// history up to `seq` is what it printed before.
    Attached { seq: u64 },
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The pane's shell on the node: the one still running under its id, or a
/// new one.
pub async fn start(client: &NodeClient, spawn: &RemoteSpawn) -> Result<Started, String> {
    let listed = wire::request(client, "GET", "/node/panes", None)
        .await?
        .json()?;
    let running = listed
        .get("panes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|pane| {
            pane.get("id").and_then(Value::as_str) == Some(spawn.id.as_str())
                && pane.get("alive").and_then(Value::as_bool) == Some(true)
        });
    if let Some(pane) = running {
        let seq = pane.get("seq").and_then(Value::as_u64).unwrap_or(0);
        return Ok(Started::Attached { seq });
    }
    let answer = wire::request(client, "POST", "/node/panes", Some(&spawn.body()))
        .await?
        .json()?;
    let mcp = answer
        .get("mcp")
        .filter(|mcp| !mcp.is_null())
        .map(|mcp| RemoteScope {
            path: mcp
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            servers: strings(mcp.get("servers")),
            missing: strings(mcp.get("missing")),
        });
    Ok(Started::Spawned { mcp })
}

/// The pane's shell killed on the node, its history with it.
pub async fn kill(client: &NodeClient, pane: &str) -> Result<(), String> {
    let path = format!("/node/panes/{}", wire::encode(pane));
    let answer = wire::request(client, "DELETE", &path, None).await?;
    // Already gone is what was asked.
    if answer.ok() || answer.status == 404 {
        Ok(())
    } else {
        Err(answer.error())
    }
}

/// What a pane's terminal is sent from its stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// Output, `seq` the bytes printed in all with it. `replay`: printed
    /// before this desktop followed the pane, to be drawn and not answered.
    Output {
        seq: u64,
        data: String,
        replay: bool,
    },
    /// The node no longer holds what the pane printed since the last byte
    /// seen: the terminal is cleared, and the history follows.
    Reset,
    /// The shell ended, or the node lost the pane.
    Exit,
}

/// The subprotocol a stream offers for its output in binary frames
/// (hangar-node's api.rs): an 8-byte big-endian `seq`, then the data. A node
/// that does not know it sends JSON frames, read all the same.
pub const BINARY_PROTOCOL: &str = "hangar-binary";

/// What the pane sends on its stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    Input(String),
    Resize(u16, u16),
}

impl Outgoing {
    fn frame(&self) -> String {
        match self {
            Outgoing::Input(data) => json!({ "input": data }).to_string(),
            Outgoing::Resize(cols, rows) => {
                json!({ "resize": { "cols": cols, "rows": rows } }).to_string()
            }
        }
    }
}

/// How one connection of a stream ended.
enum Ended {
    /// The pane is over: its shell ended, or the pane went from the desktop.
    Done,
    /// The connection dropped: the next one resumes from the last byte.
    Dropped,
}

/// Follows pane `pane` of node `node` until its shell ends or the pane goes
/// (`outgoing` closed): its output to `emit`, from the start, then from the
/// last byte seen at each reconnection; what comes on `outgoing` to the
/// node, the last size again at each reconnection. The output up to
/// `replay_until` was printed before the pane was followed.
pub async fn follow(
    node: String,
    pane: String,
    replay_until: u64,
    mut outgoing: UnboundedReceiver<Outgoing>,
    mut emit: impl FnMut(StreamEvent),
    backoff: Backoff,
    resolve: impl Fn(&str) -> Option<NodeClient>,
) {
    let mut last: u64 = 0;
    let mut size: Option<Outgoing> = None;
    let mut wait = backoff.first;
    loop {
        let connected = match resolve(&node) {
            Some(client) => {
                let path = format!("/node/panes/{}/stream?from={last}", wire::encode(&pane));
                Some(Socket::connect_offering(&client, &path, Some(BINARY_PROTOCOL)).await)
            }
            None => None,
        };
        match connected {
            Some(Ok(mut socket)) => {
                wait = backoff.first;
                let sized = match &size {
                    Some(resize) => socket.send_text(&resize.frame()).await.is_ok(),
                    None => true,
                };
                if sized {
                    let ended = pump(
                        &mut socket,
                        &mut outgoing,
                        &mut last,
                        &mut size,
                        replay_until,
                        &mut emit,
                    )
                    .await;
                    if let Ended::Done = ended {
                        socket.close().await;
                        return;
                    }
                }
            }
            // The node has no such pane: it ended and was forgotten (a
            // node that restarted), and nothing will come of it.
            Some(Err(WsError::Refused(404, _))) => {
                emit(StreamEvent::Exit);
                return;
            }
            Some(Err(_)) | None => {}
        }
        // Waiting for the node, what is typed meanwhile goes nowhere, as it
        // would into a pane that has no shell; the last size is kept.
        let sleep = tokio::time::sleep(wait);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                () = &mut sleep => break,
                next = outgoing.recv() => match next {
                    None => return,
                    Some(resize @ Outgoing::Resize(..)) => size = Some(resize),
                    Some(Outgoing::Input(_)) => {}
                },
            }
        }
        wait = backoff.next(wait);
    }
}

async fn pump(
    socket: &mut Socket,
    outgoing: &mut UnboundedReceiver<Outgoing>,
    last: &mut u64,
    size: &mut Option<Outgoing>,
    replay_until: u64,
    emit: &mut impl FnMut(StreamEvent),
) -> Ended {
    loop {
        // What is read already, frame by frame.
        loop {
            match socket.next() {
                Ok(Some(Incoming::Text(text))) => {
                    if let Some(ended) = frame(&text, last, replay_until, emit) {
                        return ended;
                    }
                }
                Ok(Some(Incoming::Binary(bytes))) => {
                    if let Some((seq, data)) = binary_output(bytes) {
                        output(seq, data, last, replay_until, emit);
                    }
                }
                Ok(Some(Incoming::Close)) | Err(_) => return Ended::Dropped,
                Ok(None) => break,
            }
        }
        if socket.flush().await.is_err() {
            return Ended::Dropped;
        }
        tokio::select! {
            read = socket.fill() => match read {
                Ok(true) => {}
                Ok(false) | Err(_) => return Ended::Dropped,
            },
            next = outgoing.recv() => match next {
                None => return Ended::Done,
                Some(message) => {
                    if let Outgoing::Resize(..) = message {
                        *size = Some(message.clone());
                    }
                    if socket.send_text(&message.frame()).await.is_err() {
                        return Ended::Dropped;
                    }
                }
            },
        }
    }
}

/// One frame of the stream: `{seq, data}`, `{reset}`, `{exit}` or an
/// `{error}` the node answered a frame of ours with.
fn frame(
    text: &str,
    last: &mut u64,
    replay_until: u64,
    emit: &mut impl FnMut(StreamEvent),
) -> Option<Ended> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    if value.get("exit").is_some() {
        emit(StreamEvent::Exit);
        return Some(Ended::Done);
    }
    if value.get("reset").is_some() {
        emit(StreamEvent::Reset);
        *last = 0;
        return None;
    }
    if let (Some(seq), Some(data)) = (
        value.get("seq").and_then(Value::as_u64),
        value.get("data").and_then(Value::as_str),
    ) {
        output(seq, data.to_owned(), last, replay_until, emit);
        return None;
    }
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        eprintln!("node stream: {error}");
    }
    None
}

/// A binary output frame: its `seq` and its data, when it is one.
pub fn binary_output(mut bytes: Vec<u8>) -> Option<(u64, String)> {
    let head: [u8; 8] = bytes.get(..8)?.try_into().ok()?;
    bytes.drain(..8);
    let data = String::from_utf8(bytes).ok()?;
    Some((u64::from_be_bytes(head), data))
}

/// Output ending at byte `seq`, to `emit`: what was drawn already (sent
/// again across a reconnection) left out, and what the pane printed before
/// it was followed (`replay_until`) marked so, apart from the rest when a
/// frame holds both (a node sends the output waiting as one frame).
pub fn output(
    seq: u64,
    mut data: String,
    last: &mut u64,
    replay_until: u64,
    emit: &mut impl FnMut(StreamEvent),
) {
    if seq <= *last {
        return;
    }
    let begins = seq.saturating_sub(data.len() as u64);
    if begins < *last {
        data.drain(..char_cut(&data, *last - begins));
    }
    let begins = seq.saturating_sub(data.len() as u64);
    *last = seq;
    if begins < replay_until && replay_until < seq {
        let cut = char_cut(&data, replay_until - begins);
        let rest = data.split_off(cut);
        if !data.is_empty() {
            emit(StreamEvent::Output {
                seq: begins + cut as u64,
                data,
                replay: true,
            });
        }
        if !rest.is_empty() {
            emit(StreamEvent::Output {
                seq,
                data: rest,
                replay: false,
            });
        }
        return;
    }
    if !data.is_empty() {
        emit(StreamEvent::Output {
            seq,
            data,
            replay: seq <= replay_until,
        });
    }
}

/// Byte `at` of `data`, or the next character's start when it falls inside one.
fn char_cut(data: &str, at: u64) -> usize {
    let mut cut = usize::try_from(at).unwrap_or(usize::MAX).min(data.len());
    while !data.is_char_boundary(cut) {
        cut += 1;
    }
    cut
}

/// Follows node `node`'s events (`/node/events`) for as long as the task
/// runs, reconnecting as the stream does: each to `emit`, a `{"lagged"}`
/// included (some were lost: what they would have refreshed is read again).
pub async fn follow_events(
    node: String,
    emit: impl FnMut(Value),
    backoff: Backoff,
    resolve: impl Fn(&str) -> Option<NodeClient>,
) {
    follow_events_at(node, "/node/events", emit, backoff, resolve).await;
}

/// [`follow_events`] on `path`: `/node/events?control=1` for the desktop
/// that drives the node's orchestration, whose control requests it answers.
pub async fn follow_events_at(
    node: String,
    path: &str,
    mut emit: impl FnMut(Value),
    backoff: Backoff,
    resolve: impl Fn(&str) -> Option<NodeClient>,
) {
    let mut wait = backoff.first;
    loop {
        if let Some(client) = resolve(&node)
            && let Ok(mut socket) = Socket::connect(&client, path).await
        {
            wait = backoff.first;
            // What changed while it was not followed is unknown.
            emit(json!({ "lagged": 0 }));
            'socket: loop {
                loop {
                    match socket.next() {
                        Ok(Some(Incoming::Text(text))) => {
                            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                                emit(value);
                            }
                        }
                        Ok(Some(Incoming::Binary(_))) => {}
                        Ok(Some(Incoming::Close)) | Err(_) => break 'socket,
                        Ok(None) => break,
                    }
                }
                if socket.flush().await.is_err() {
                    break;
                }
                match socket.fill().await {
                    Ok(true) => {}
                    Ok(false) | Err(_) => break,
                }
            }
        }
        tokio::time::sleep(wait).await;
        wait = backoff.next(wait);
    }
}

/// `GET /node/fs?path=`: a folder of the node, for the workspace dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    /// The sub-folders, by name.
    pub folders: Vec<String>,
}

pub async fn list_folder(client: &NodeClient, path: Option<&str>) -> Result<Listing, String> {
    let query = match path {
        Some(path) => format!("/node/fs?path={}", wire::encode(path)),
        None => "/node/fs".to_owned(),
    };
    let value = wire::request(client, "GET", &query, None).await?.json()?;
    Ok(Listing {
        path: value
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        parent: value
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_owned),
        folders: value
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|entry| entry.get("dir").and_then(Value::as_bool) == Some(true))
            .filter_map(|entry| entry.get("name")?.as_str().map(str::to_owned))
            .collect(),
    })
}

/// A pane of a remote workspace, as the code that reaches panes by id (the
/// orchestration, the control API, the saved commands, the kits' setup)
/// finds it: its node, what it types into it, whether its shell runs.
struct Registered {
    node: String,
    outgoing: UnboundedSender<Outgoing>,
    alive: Arc<AtomicBool>,
}

/// The remote panes mounted, by pane id: a pane id names one pane whatever
/// its workspace, and these are not in core's PTY manager.
static PANES: LazyLock<RwLock<HashMap<String, Registered>>> = LazyLock::new(Default::default);

fn panes() -> std::sync::RwLockReadGuard<'static, HashMap<String, Registered>> {
    PANES.read().unwrap_or_else(|poison| poison.into_inner())
}

/// Pane `pane` lives on node `node`, typed into through `outgoing`, its
/// shell running while `alive` holds.
pub fn register(
    pane: &str,
    node: &str,
    outgoing: UnboundedSender<Outgoing>,
    alive: Arc<AtomicBool>,
) {
    PANES
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(
            pane.to_owned(),
            Registered {
                node: node.to_owned(),
                outgoing,
                alive,
            },
        );
}

pub fn unregister(pane: &str) {
    PANES
        .write()
        .unwrap_or_else(|poison| poison.into_inner())
        .remove(pane);
}

/// The node of pane `pane`, when it is a remote one.
pub fn node_of(pane: &str) -> Option<String> {
    panes().get(pane).map(|pane| pane.node.clone())
}

/// `data` typed into pane `pane`, if it is a remote one (`None` otherwise:
/// the caller writes to the local PTY). Gone nowhere, as into a local pane
/// with no shell, while its node is away.
pub fn write(pane: &str, data: String) -> Option<Result<(), String>> {
    let panes = panes();
    let registered = panes.get(pane)?;
    if !registered.alive.load(Ordering::Acquire) {
        return Some(Err("pty not found".into()));
    }
    Some(
        registered
            .outgoing
            .send(Outgoing::Input(data))
            .map_err(|_| "pty not found".to_string()),
    )
}

/// Whether the shell of pane `pane` runs, if it is a remote one.
pub fn alive(pane: &str) -> Option<bool> {
    panes()
        .get(pane)
        .map(|pane| pane.alive.load(Ordering::Acquire))
}

/// Pane `pane`'s shell killed on its node, if it is a remote one.
pub async fn kill_pane(pane: &str) -> Option<Result<(), String>> {
    let node = node_of(pane)?;
    let Some(client) = directory(&node) else {
        return Some(Err(format!("node {node} is not connected")));
    };
    Some(kill(&client, pane).await)
}

/// `pty_has_children` for panes of either kind: the local ones asked of
/// core, the remote ones taken as running something, since the node is not
/// asked (a sleep must not take an agent at work for an idle shell).
pub async fn has_children(
    manager: Arc<hangar_core::pty::PtyManager>,
    ids: Vec<String>,
) -> Result<Vec<String>, String> {
    let (remote, local): (Vec<String>, Vec<String>) =
        ids.into_iter().partition(|id| node_of(id).is_some());
    let mut running = if local.is_empty() {
        Vec::new()
    } else {
        hangar_core::pty::pty_has_children(manager, local).await?
    };
    running.extend(remote.into_iter().filter(|id| alive(id) == Some(true)));
    Ok(running)
}
