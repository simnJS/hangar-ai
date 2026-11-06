//! The binary itself, run as the desktop runs it over SSH: `serve --detach`
//! in a home of the test's own, then its API over loopback.
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
