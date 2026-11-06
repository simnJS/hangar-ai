//! The binary itself, run as the desktop runs it over SSH: `serve --detach`
//! in a home of the test's own, then its API over loopback.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

const BIN: &str = env!("CARGO_BIN_EXE_hangar-node");
const WAIT: Duration = Duration::from_secs(20);

/// The binary with `home` as the account's home, and nothing of the
/// environment's Hangar (the pane these tests may run in, a development
/// build's data) leaking into it.
fn command(home: &Path) -> Command {
    let mut command = Command::new(BIN);
    command.env("HOME", home);
    for var in [
        "HANGAR_DATA_ROOT",
        "HANGAR_ENDPOINT_FILE",
        "HANGAR_PANE_ID",
        "HANGAR_PANE_NAME",
        "HANGAR_BRIDGE_URL",
        "HANGAR_BRIDGE_TOKEN",
        "HANGAR_PORT_BASE",
        "HANGAR_PORT_COUNT",
        "HANGAR_WORKSPACE",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        // The transcripts are the test home's own (`~/.claude/projects`).
        "CLAUDE_CONFIG_DIR",
    ] {
        command.env_remove(var);
    }
    command
}

fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/hangar-node")
}

fn read_node(home: &Path) -> Value {
    let raw = std::fs::read_to_string(state_dir(home).join("node.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

struct Node {
    home: tempfile::TempDir,
    port: u16,
    token: String,
    pid: i32,
}

impl Node {
    fn start() -> Node {
        let home = tempfile::tempdir().unwrap();
        let status = command(home.path())
            .args(["serve", "--detach"])
            .status()
            .unwrap();
        assert!(status.success(), "serve --detach: {status}");
        Node::found(home)
    }

    fn found(home: tempfile::TempDir) -> Node {
        let node = read_node(home.path());
        Node {
            port: node["port"].as_u64().unwrap() as u16,
            token: node["token"].as_str().unwrap().to_string(),
            pid: node["pid"].as_i64().unwrap() as i32,
            home,
        }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    /// One request over a plain socket (an HTTP client could go through a
    /// proxy the environment names): the status and the JSON answered.
    fn call(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value) {
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        socket.set_read_timeout(Some(WAIT)).unwrap();
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        write!(
            socket,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut raw = Vec::new();
        socket.read_to_end(&mut raw).unwrap();
        let raw = String::from_utf8(raw).unwrap();
        let (head, rest) = raw.split_once("\r\n\r\n").unwrap();
        let status = head[9..12].parse().unwrap();
        let body = if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            dechunk(rest)
        } else {
            rest.to_string()
        };
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    fn get(&self, path: &str) -> (u16, Value) {
        self.call("GET", path, Some(&self.token.clone()), None)
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.call("POST", path, Some(&self.token.clone()), Some(body))
    }

    fn delete(&self, path: &str) -> (u16, Value) {
        self.call("DELETE", path, Some(&self.token.clone()), None)
    }

    fn spawn(&self, id: &str, script: &str) {
        let (status, body) = self.post(
            "/node/panes",
            json!({
                "id": id,
                "cwd": self.home(),
                "shell": { "program": "sh", "args": ["-c", script] },
                "cols": 80,
                "rows": 24,
            }),
        );
        assert_eq!(status, 200, "{body}");
    }

    fn pane(&self, id: &str) -> Option<Value> {
        let (_, body) = self.get("/node/panes");
        body["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pane| pane["id"] == id)
            .cloned()
    }

    async fn socket(&self, path: &str) -> Socket {
        let mut request = format!("ws://127.0.0.1:{}{path}", self.port)
            .into_client_request()
            .unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", self.token).parse().unwrap(),
        );
        let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        Socket {
            socket,
            frames: Vec::new(),
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        unsafe { libc::kill(self.pid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(10);
        while alive(self.pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    loop {
        let (size, after) = rest.split_once("\r\n").unwrap();
        let size = usize::from_str_radix(size.trim(), 16).unwrap();
        if size == 0 {
            return out;
        }
        out.push_str(&after[..size]);
        rest = &after[size + 2..];
    }
}

struct Socket {
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    frames: Vec<Value>,
}

impl Socket {
    async fn send(&mut self, frame: Value) {
        self.socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .unwrap();
    }

    /// The next JSON frame; `None` once the socket closes.
    async fn next(&mut self) -> Option<Value> {
        loop {
            let message = tokio::time::timeout(WAIT, self.socket.next())
                .await
                .expect("no frame in time")?;
            match message.ok()? {
                Message::Text(text) => {
                    let frame: Value = serde_json::from_str(text.as_str()).unwrap();
                    self.frames.push(frame.clone());
                    return Some(frame);
                }
                Message::Close(_) => return None,
                _ => {}
            }
        }
    }

    /// The output received so far.
    fn text(&self) -> String {
        self.frames
            .iter()
            .filter_map(|frame| frame["data"].as_str())
            .collect()
    }

    fn last_seq(&self) -> u64 {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame["seq"].as_u64())
            .unwrap_or(0)
    }

    /// Frames until the output holds `needle` `times` times.
    async fn until(&mut self, needle: &str, times: usize) {
        while self.text().matches(needle).count() < times {
            let frame = self.next().await;
            assert!(
                frame.is_some(),
                "closed before {needle:?}: {:?}",
                self.text()
            );
        }
    }

    async fn until_exit(&mut self) -> Value {
        loop {
            match self.next().await {
                Some(frame) if frame.get("exit").is_some() => return frame,
                Some(_) => {}
                None => panic!("closed without an exit: {:?}", self.frames),
            }
        }
    }
}

#[test]
fn version_prints_the_version_and_the_target() {
    let out = Command::new(BIN).arg("version").output().unwrap();
    assert!(out.status.success());
    let out = String::from_utf8(out.stdout).unwrap();
    let parts: Vec<&str> = out.split_whitespace().collect();
    assert_eq!(parts.len(), 2, "{out}");
    assert_eq!(parts[0], env!("CARGO_PKG_VERSION"));
    assert!(parts[1].contains("-linux-"), "{out}");
}

#[test]
fn the_api_wants_the_token() {
    let node = Node::start();
    for path in ["/node/info", "/node/panes", "/api/board?cwd=/"] {
        assert_eq!(node.call("GET", path, None, None).0, 401, "{path}");
        assert_eq!(node.call("GET", path, Some("nope"), None).0, 401, "{path}");
        let almost = format!("{}x", &node.token[..node.token.len() - 1]);
        assert_eq!(node.call("GET", path, Some(&almost), None).0, 401, "{path}");
    }
    let (status, info) = node.get("/node/info");
    assert_eq!(status, 200);
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    assert!(info["target"].as_str().unwrap().contains("linux"));
    assert_eq!(info["home"], node.home().to_str().unwrap());
    assert!(info["claude"].is_boolean() && info["git"].is_boolean());
    assert!(info["hostname"].is_string());
}

#[test]
fn node_json_is_private_and_names_the_daemon() {
    let node = Node::start();
    let dir = state_dir(node.home());
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&dir.join("node.json")), 0o600);
    let written = read_node(node.home());
    assert_eq!(written["version"], env!("CARGO_PKG_VERSION"));
    assert!(alive(node.pid));
    assert_eq!(node.get("/node/info").1["pid"], written["pid"]);
}

#[test]
fn two_daemons_make_one() {
    let home = tempfile::tempdir().unwrap();
    // Two at once, as two desktops connecting together would.
    let mut first = command(home.path())
        .args(["serve", "--detach"])
        .spawn()
        .unwrap();
    let mut second = command(home.path())
        .args(["serve", "--detach"])
        .spawn()
        .unwrap();
    assert!(first.wait().unwrap().success());
    assert!(second.wait().unwrap().success());
    let node = Node::found(home);
    // Again, detached and in the foreground: both see it and leave it be.
    assert!(command(node.home())
        .args(["serve", "--detach"])
        .status()
        .unwrap()
        .success());
    let foreground = command(node.home())
        .arg("serve")
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(foreground.status.success());
    assert!(String::from_utf8_lossy(&foreground.stderr).contains("already running"));
    assert_eq!(read_node(node.home())["pid"], node.pid);
    let log = std::fs::read_to_string(state_dir(node.home()).join("node.log")).unwrap();
    assert_eq!(log.matches("listening on").count(), 1, "{log}");
}

#[test]
fn a_stopped_daemon_takes_its_node_json_with_it() {
    let node = Node::start();
    let file = state_dir(node.home()).join("node.json");
    unsafe { libc::kill(node.pid, libc::SIGTERM) };
    let deadline = Instant::now() + WAIT;
    while file.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!file.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pane_s_output_reaches_its_stream() {
    let node = Node::start();
    node.spawn("p-echo", "echo hi");
    // Whether it ended before the stream opened or after, it is all there.
    let mut stream = node.socket("/node/panes/p-echo/stream?from=0").await;
    let exit = stream.until_exit().await;
    assert!(stream.text().contains("hi"), "{:?}", stream.frames);
    assert_eq!(exit, json!({ "exit": { "code": null } }));
    assert!(stream.frames[0].get("reset").is_none());
    let pane = node.pane("p-echo").unwrap();
    assert_eq!(pane["alive"], false);
    assert_eq!(pane["seq"].as_u64().unwrap(), stream.last_seq());
}

#[tokio::test(flavor = "multi_thread")]
async fn input_comes_back_and_history_replays_from_a_seq() {
    let node = Node::start();
    node.spawn("p-cat", "exec cat");
    let mut stream = node.socket("/node/panes/p-cat/stream?from=0").await;
    // Through the socket: the terminal's echo, then cat's own.
    stream.send(json!({ "input": "alpha\n" })).await;
    stream.until("alpha", 2).await;
    let seen = stream.last_seq();
    // Through the route.
    assert_eq!(
        node.post("/node/panes/p-cat/input", json!({ "data": "beta\n" }))
            .0,
        200
    );
    stream.until("beta", 2).await;
    drop(stream);

    let mut again = node
        .socket(&format!("/node/panes/p-cat/stream?from={seen}"))
        .await;
    again.until("beta", 2).await;
    assert!(!again.text().contains("alpha"), "{:?}", again.text());
    assert!(again.frames[0].get("reset").is_none());

    let mut whole = node.socket("/node/panes/p-cat/stream?from=0").await;
    whole.until("beta", 2).await;
    assert!(whole.text().starts_with("alpha"), "{:?}", whole.text());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_seq_too_old_is_a_reset() {
    let node = Node::start();
    // More than the 4 MiB a pane keeps.
    node.spawn(
        "p-flood",
        "head -c 5000000 /dev/zero | tr '\\0' a; echo; echo flooded; exec sleep 60",
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let end = loop {
        let seq = node.pane("p-flood").unwrap()["seq"].as_u64().unwrap();
        if seq > 5_000_000 {
            // The last line too, which follows at once.
            std::thread::sleep(Duration::from_millis(200));
            break node.pane("p-flood").unwrap()["seq"].as_u64().unwrap();
        }
        assert!(Instant::now() < deadline, "only {seq} bytes");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stream = node.socket("/node/panes/p-flood/stream?from=0").await;
    let reset = stream.next().await.unwrap();
    let start = reset["reset"]["start"].as_u64().expect("a reset first");
    assert!(start > 0);
    while stream.last_seq() < end {
        stream.next().await.unwrap();
    }
    let replayed = stream.text();
    assert_eq!(replayed.len() as u64, end - start);
    assert!(replayed.len() <= 4 * 1024 * 1024 + 8192);
    assert!(replayed.contains("flooded"));
    assert_eq!(node.delete("/node/panes/p-flood").0, 200);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pane_resizes() {
    let node = Node::start();
    node.spawn(
        "p-size",
        "read x; stty size; read y; stty size; exec sleep 60",
    );
    let mut stream = node.socket("/node/panes/p-size/stream?from=0").await;
    assert_eq!(
        node.post(
            "/node/panes/p-size/resize",
            json!({ "cols": 100, "rows": 40 })
        )
        .0,
        200
    );
    assert_eq!(node.pane("p-size").unwrap()["cols"], 100);
    stream.send(json!({ "input": "\n" })).await;
    stream.until("40 100", 1).await;
    // And through the socket.
    stream
        .send(json!({ "resize": { "cols": 120, "rows": 50 } }))
        .await;
    stream.send(json!({ "input": "\n" })).await;
    stream.until("50 120", 1).await;
    assert_eq!(node.pane("p-size").unwrap()["rows"], 50);
    assert_eq!(
        node.post("/node/panes/p-gone/resize", json!({ "cols": 1, "rows": 1 }))
            .0,
        404
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_killed_pane_ends_its_streams_and_is_gone() {
    let node = Node::start();
    node.spawn("p-kill", "exec sleep 60");
    let mut stream = node.socket("/node/panes/p-kill/stream?from=0").await;
    assert!(node.pane("p-kill").unwrap()["alive"] == true);
    assert_eq!(node.delete("/node/panes/p-kill").0, 200);
    assert_eq!(
        stream.until_exit().await,
        json!({ "exit": { "code": null } })
    );
    assert!(node.pane("p-kill").is_none());
    assert_eq!(node.delete("/node/panes/p-kill").0, 404);
    assert_eq!(
        node.post("/node/panes/p-kill/input", json!({ "data": "x" }))
            .0,
        404
    );
    let (status, _) = node.get("/node/panes/p-kill/stream?from=0");
    assert_eq!(status, 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_api_proxy_reaches_the_board_and_events_follow() {
    let node = Node::start();
    let workspace = node.home().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let cwd = workspace.to_str().unwrap();
    let mut events = node.socket("/node/events").await;

    let (status, created) = node.post(
        &format!("/api/tasks?cwd={cwd}"),
        json!({ "title": "from the desktop" }),
    );
    assert_eq!(status, 200, "{created}");
    let (status, board) = node.get(&format!("/api/board?cwd={cwd}"));
    assert_eq!(status, 200, "{board}");
    let titles: Vec<&str> = board["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["title"].as_str())
        .collect();
    assert_eq!(titles, ["from the desktop"]);

    loop {
        let event = events.next().await.expect("an event");
        if event["event"] == "board:changed" {
            assert_eq!(event["cwd"], cwd);
            break;
        }
    }
}

#[test]
fn fs_lists_the_home_and_keeps_relative_paths_in_it() {
    let node = Node::start();
    std::fs::create_dir_all(node.home().join("project")).unwrap();
    let (status, listing) = node.get("/node/fs");
    assert_eq!(status, 200, "{listing}");
    let home = node.home().canonicalize().unwrap();
    assert_eq!(listing["path"], home.to_str().unwrap());
    assert!(listing["entries"]
        .as_array()
        .unwrap()
        .contains(&json!({ "name": "project", "dir": true })));
    assert_eq!(node.get("/node/fs?path=..").0, 403);
    assert_eq!(node.get("/node/fs?path=/").0, 200);
}

/// `hangar-node --mcp`, as an agent in one of the node's panes starts it:
/// it finds the node's own endpoint and answers from its board.
#[test]
fn the_mcp_proxy_reaches_the_node() {
    let node = Node::start();
    let mut proxy = command(node.home())
        .arg("--mcp")
        .env("HANGAR_WORKSPACE", node.home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = proxy.stdin.take().unwrap();
    let mut stdout = BufReader::new(proxy.stdout.take().unwrap());
    let ask = |stdin: &mut std::process::ChildStdin, id: u32, method: &str, params: Value| {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(stdin, "{line}").unwrap();
    };
    ask(
        &mut stdin,
        1,
        "initialize",
        json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
    );
    ask(
        &mut stdin,
        2,
        "tools/call",
        json!({ "name": "board_list_tasks", "arguments": {} }),
    );
    let mut answers = Vec::new();
    for _ in 0..2 {
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        answers.push(serde_json::from_str::<Value>(&line).unwrap());
    }
    drop(stdin);
    let _ = proxy.wait();
    assert!(answers[0]["result"].is_object(), "{answers:?}");
    let text = answers[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"columns\""), "{text}");
}

/// A Claude Code transcript as the node's would be, under the test home:
/// `~/.claude/projects/<cwd with every other character a dash>/<id>.jsonl`.
fn write_transcript(home: &Path, cwd: &str, session: &str, lines: &[Value]) {
    let folder: String = cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = home.join(".claude/projects").join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(dir.join(format!("{session}.jsonl")), text).unwrap();
}

fn transcript_line(kind: &str, uuid: &str, cwd: &str, extra: Value) -> Value {
    let mut line = json!({
        "parentUuid": null,
        "isSidechain": false,
        "type": kind,
        "uuid": uuid,
        "timestamp": "2026-11-06T05:00:00.000Z",
        "cwd": cwd,
        "sessionId": "sess-1",
        "version": "2.9.0",
    });
    for (key, value) in extra.as_object().unwrap() {
        line[key] = value.clone();
    }
    line
}

fn a_conversation(cwd: &str) -> Vec<Value> {
    vec![
        transcript_line(
            "user",
            "u1",
            cwd,
            json!({
                "message": { "role": "user", "content": "fix the build" },
                "origin": { "kind": "human" },
                "promptSource": "typed",
            }),
        ),
        transcript_line(
            "assistant",
            "a1",
            cwd,
            json!({
                "message": {
                    "id": "msg_a1",
                    "model": "claude-opus-5-5",
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "done" }],
                    "usage": {
                        "input_tokens": 2,
                        "cache_creation_input_tokens": 100,
                        "cache_read_input_tokens": 1000,
                        "output_tokens": 8,
                    },
                },
                "requestId": "req_1",
            }),
        ),
    ]
}

#[test]
fn the_transcripts_are_read_on_the_node() {
    let node = Node::start();
    let cwd = node.home().join("demo");
    std::fs::create_dir_all(&cwd).unwrap();
    let cwd = cwd.to_str().unwrap().to_owned();
    let session = "4f1c2d3e-0000-4000-8000-000000000001";

    // Nothing written yet: no page, no gauge, no session.
    let (status, empty) = node.get(&format!("/node/chat?session={session}&cwd={cwd}"));
    assert_eq!(status, 200, "{empty}");
    assert_eq!(empty["page"], Value::Null);
    let (status, none) = node.get(&format!("/node/context?session={session}&cwd={cwd}"));
    assert_eq!((status, &none["usage"]), (200, &Value::Null), "{none}");
    let (status, listed) = node.get(&format!("/node/sessions?cwd={cwd}"));
    assert_eq!((status, listed["sessions"].clone()), (200, json!([])));

    write_transcript(node.home(), &cwd, session, &a_conversation(&cwd));

    let (status, page) = node.get(&format!("/node/chat?session={session}&cwd={cwd}&limit=10"));
    assert_eq!(status, 200, "{page}");
    let items = page["page"]["items"].as_array().expect("items");
    let texts: Vec<&str> = items
        .iter()
        .filter_map(|item| item["text"].as_str())
        .collect();
    assert_eq!(texts, ["fix the build", "done"], "{page}");

    let (status, usage) = node.get(&format!("/node/context?session={session}&cwd={cwd}"));
    assert_eq!(status, 200, "{usage}");
    assert_eq!(
        usage["usage"],
        json!({ "usedTokens": 2 + 100 + 1000 + 8, "model": "claude-opus-5-5" })
    );

    let (status, ids) = node.get(&format!("/node/sessions?cwd={cwd}"));
    assert_eq!(status, 200, "{ids}");
    assert_eq!(ids["sessions"][0]["id"], session);
    assert_eq!(
        ids["sessions"][0]["label"], "",
        "the hunt's listing opens nothing"
    );
    let modified = ids["sessions"][0]["modifiedMs"].as_u64().unwrap();
    assert!(modified > 0);
    assert!(ids["now"].as_u64().unwrap() >= modified, "the node's clock");
    let (status, labelled) = node.get(&format!("/node/sessions?cwd={cwd}&labels=1"));
    assert_eq!(status, 200, "{labelled}");
    assert_eq!(labelled["sessions"][0]["id"], session);
    assert_eq!(labelled["sessions"][0]["label"], "fix the build");

    // What is not a session id, a cursor or a cwd is refused.
    assert_eq!(
        node.get(&format!("/node/chat?session=../x&cwd={cwd}")).0,
        400
    );
    assert_eq!(
        node.get(&format!(
            "/node/chat?session={session}&cwd={cwd}&before=nope"
        ))
        .0,
        400
    );
    assert_eq!(node.get("/node/sessions").0, 400);
    // Behind the token like every route.
    let path = format!("/node/sessions?cwd={cwd}");
    assert_eq!(node.call("GET", &path, None, None).0, 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_memory_is_the_node_s_and_its_changes_follow() {
    let node = Node::start();
    let mut events = node.socket("/node/events").await;
    let (status, created) = node.post(
        "/api/memory?cwd=/w",
        json!({ "title": "Build", "content": "cargo build", "tags": ["ci"] }),
    );
    assert_eq!(status, 200, "{created}");
    let (status, listed) = node.get("/api/memory");
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed["entries"][0]["title"], "Build");
    assert!(
        listed["entries"][0].get("content").is_none(),
        "core's index"
    );
    // The whole store, contents included, for the desktop's view.
    let (status, whole) = node.get("/node/memory");
    assert_eq!(status, 200, "{whole}");
    assert_eq!(whole["entries"][0]["content"], "cargo build");
    assert_eq!(whole["entries"][0]["tags"], json!(["ci"]));
    loop {
        let event = events.next().await.expect("an event");
        if event["event"] == "memory:changed" {
            break;
        }
    }
}

/// A control request of the node's agents, as the desktop that drives the
/// node sees it on `/node/events?control=1`.
async fn next_control(events: &mut Socket) -> Value {
    loop {
        let event = events.next().await.expect("an event");
        if event["event"] == "control:request" {
            return event;
        }
    }
}

/// `hangar-node --mcp` as an agent of the node runs it, its calls answered
/// one line each.
struct Mcp {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Mcp {
    fn start(node: &Node, cwd: &Path) -> Mcp {
        let mut child = command(node.home())
            .arg("--mcp")
            .env("HANGAR_WORKSPACE", cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Mcp {
            child,
            stdin,
            stdout,
        };
        let init = mcp.ask(
            1,
            "initialize",
            json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
        );
        assert!(init["result"].is_object(), "{init}");
        mcp
    }

    fn ask(&mut self, id: u32, method: &str, params: Value) -> Value {
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{line}").unwrap();
        let mut answer = String::new();
        self.stdout.read_line(&mut answer).unwrap();
        serde_json::from_str(&answer).unwrap()
    }

    /// A tool's text, and whether it is an error.
    fn call(&mut self, id: u32, tool: &str, arguments: Value) -> (String, bool) {
        let answer = self.ask(
            id,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        let result = &answer["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        (text, result["isError"] == true)
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An orchestrator of the node calls `worker_spawn` through its MCP proxy:
/// the request reaches the desktop following the node's control requests,
/// with its id, and the desktop's reply is the agent's answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_control_request_goes_to_the_driving_desktop_and_its_reply_comes_back() {
    let node = Arc::new(Node::start());
    let mut events = node.socket("/node/events?control=1").await;
    assert_eq!(node.get("/node/info").1["drivers"], 1);

    let asking = node.clone();
    let agent = tokio::task::spawn_blocking(move || {
        let mut mcp = Mcp::start(&asking, asking.home());
        mcp.call(2, "worker_spawn", json!({ "task": "t1" }))
    });
    let request = next_control(&mut events).await;
    assert_eq!(request["payload"]["op"], "worker_spawn");
    assert_eq!(request["payload"]["args"]["task"], "t1");
    let id = request["id"].as_str().unwrap().to_string();
    assert_eq!(request["payload"]["id"], id.as_str());

    let replying = node.clone();
    let (status, body) = tokio::task::spawn_blocking(move || {
        replying.post(
            &format!("/node/control/{id}/reply"),
            json!({ "ok": true, "value": { "worker": "Nora", "branch": "hangar/t1" } }),
        )
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["waited"], true);

    let (text, failed) = agent.await.unwrap();
    assert!(!failed, "{text}");
    assert!(text.contains("hangar/t1"), "{text}");
}

/// Nobody drives the node: its agents hear so at once, rather than after
/// core's time runs out. A socket that follows the events without
/// `control=1` (the desktop's memory view) answers nothing and does not count.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_driving_desktop_a_control_request_is_unavailable() {
    let node = Arc::new(Node::start());
    let _watching = node.socket("/node/events").await;
    assert_eq!(node.get("/node/info").1["drivers"], 0);
    let started = Instant::now();
    let asking = node.clone();
    let (status, body) =
        tokio::task::spawn_blocking(move || asking.post("/api/control/pane_list", json!({})))
            .await
            .unwrap();
    assert_eq!(status, 400, "{body}");
    let said = body["error"].as_str().unwrap();
    assert!(
        said.starts_with("Unavailable: the desktop driving this node is not connected"),
        "{said}"
    );
    assert!(started.elapsed() < Duration::from_secs(10), "at once");
}

/// The desktop goes away while a request it was sent is under way: that
/// request is refused, rather than left to core's time limit.
#[tokio::test(flavor = "multi_thread")]
async fn a_desktop_gone_mid_request_leaves_it_unavailable() {
    let node = Arc::new(Node::start());
    let mut events = node.socket("/node/events?control=1").await;
    let asking = node.clone();
    let answer =
        tokio::task::spawn_blocking(move || asking.post("/api/control/pane_list", json!({})));
    next_control(&mut events).await;
    events.socket.close(None).await.unwrap();
    drop(events);
    let (status, body) = answer.await.unwrap();
    assert_eq!(status, 400, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Unavailable: the desktop driving this node is not connected"),
        "{body}"
    );
}

/// `git` in `dir`, as the test's own author.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// What the desktop's engine does on the node's disk for a worker: its
/// worktree and branch made and removed, the repository read through the
/// questions `/node/git` knows, and no other.
#[test]
fn a_worker_s_worktree_is_made_read_and_removed_on_the_node() {
    let node = Node::start();
    let repo = node.home().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("README"), "hi\n").unwrap();
    std::fs::write(repo.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(repo.join(".gitignore"), ".env\n").unwrap();
    git(&repo, &["add", "README", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let repo_s = repo.to_str().unwrap().to_string();
    let tree = node.home().join("repo-worktrees").join("w1");
    let tree_s = tree.to_str().unwrap().to_string();

    let (status, info) = node.post("/node/git", json!({ "op": "repo_info", "path": repo_s }));
    assert_eq!(status, 200, "{info}");
    assert_eq!(info["value"]["isRepo"], true);
    assert_eq!(info["value"]["branch"], "main");

    let (status, body) = node.post("/node/git", json!({ "op": "dir_exists", "path": tree_s }));
    assert_eq!((status, &body["value"]), (200, &json!(false)), "{body}");

    let (status, body) = node.post(
        "/node/worktree",
        json!({ "repo": repo_s, "path": tree_s, "branch": "hangar/w1", "base": "main" }),
    );
    assert_eq!(status, 200, "{body}");
    assert!(tree.join("README").is_file(), "checked out on the node");

    let (status, copied) = node.post(
        "/node/worktree/copy",
        json!({ "from": repo_s, "to": tree_s, "patterns": [".env"] }),
    );
    assert_eq!(status, 200, "{copied}");
    assert_eq!(copied["value"]["copied"], 1);
    assert!(tree.join(".env").is_file());

    let (status, hook) = node.post(
        "/node/worktree/hook",
        json!({ "repo": repo_s, "path": tree_s, "command": "echo made > hooked" }),
    );
    assert_eq!(status, 200, "{hook}");
    assert_eq!(hook["value"]["ok"], true, "{hook}");
    assert!(tree.join("hooked").is_file());

    let (_, listed) = node.post(
        "/node/git",
        json!({ "op": "worktree_list", "path": repo_s }),
    );
    let branches: Vec<&str> = listed["value"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tree| tree["branch"].as_str())
        .collect();
    assert!(branches.contains(&"hangar/w1"), "{listed}");

    std::fs::write(tree.join("new.txt"), "x\n").unwrap();
    let (_, changes) = node.post("/node/git", json!({ "op": "changes", "path": tree_s }));
    let files: Vec<&str> = changes["value"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| file["path"].as_str())
        .collect();
    assert!(files.contains(&"new.txt"), "{changes}");
    git(&tree, &["add", "new.txt"]);
    git(&tree, &["commit", "-q", "-m", "work"]);
    let (_, ahead) = node.post(
        "/node/git",
        json!({ "op": "commits_ahead", "path": repo_s, "base": "main", "head": "hangar/w1" }),
    );
    assert_eq!(ahead["value"], 1, "{ahead}");

    for refused in [
        json!({ "op": "exec", "path": repo_s, "args": ["status"] }),
        json!({ "op": "push", "path": repo_s }),
    ] {
        let (status, body) = node.post("/node/git", refused);
        assert_eq!(status, 400, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("repo_info"),
            "{body}"
        );
    }
    let (status, _) = node.post(
        "/node/worktree",
        json!({ "repo": "repo", "path": "w", "branch": "b" }),
    );
    assert_eq!(status, 400, "a relative path means nothing on the node");

    let (status, removed) = node.call(
        "DELETE",
        "/node/worktree",
        Some(&node.token),
        Some(json!({ "repo": repo_s, "path": tree_s, "force": true })),
    );
    assert_eq!(status, 200, "{removed}");
    assert_eq!(removed["value"]["removed"], true, "{removed}");
    assert!(!tree.exists());
    let (status, deleted) = node.call(
        "DELETE",
        "/node/branch",
        Some(&node.token),
        Some(json!({ "repo": repo_s, "branch": "hangar/w1", "force": true })),
    );
    assert_eq!(status, 200, "{deleted}");
    assert_eq!(deleted["value"]["deleted"], true, "{deleted}");
}

/// A worker's claim and release on the node's board, and a brief queued for
/// its session there.
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_claims_its_task_and_is_briefed_on_the_node() {
    let node = Arc::new(Node::start());
    let workspace = node.home().join("ws");
    std::fs::create_dir_all(&workspace).unwrap();
    let cwd = workspace.to_str().unwrap().to_string();
    let mut events = node.socket("/node/events").await;
    let (status, created) = node.post(
        &format!("/api/tasks?cwd={cwd}"),
        json!({ "title": "the task" }),
    );
    assert_eq!(status, 200, "{created}");
    let id = created["task"]["id"].as_str().unwrap().to_string();

    let (status, claimed) = node.post(
        "/node/board/claim-worker",
        json!({ "cwd": cwd, "id": id, "worker": "Nora", "sharedFolder": false }),
    );
    assert_eq!(status, 200, "{claimed}");
    assert_eq!(claimed["value"]["column"], "doing");
    assert_eq!(claimed["value"]["assignee"], "Nora");
    loop {
        let event = events.next().await.expect("an event");
        if event["event"] == "board:changed" && event["cwd"] == cwd.as_str() {
            break;
        }
    }
    let (status, commented) = node.post(
        "/node/board/comment",
        json!({ "cwd": cwd, "id": id, "author": "hangar", "text": "checked" }),
    );
    assert_eq!(status, 200, "{commented}");
    let (status, released) = node.post(
        "/node/board/release-worker",
        json!({ "cwd": cwd, "id": id, "worker": "Nora", "undo": true }),
    );
    assert_eq!(status, 200, "{released}");
    assert_eq!(released["value"]["column"], "todo");

    let (status, queued) = node.post(
        "/node/bridge/pane-1",
        json!({ "text": "Your brief", "from": "hangar" }),
    );
    assert_eq!(status, 200, "{queued}");
    assert_eq!(queued["value"]["text"], "Your brief");
}
