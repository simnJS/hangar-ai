//! The remote workspaces' side of the nodes: the wire, the panes' streams
//! against a fake node, the directory.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::model::{NodeClient, NodeInfo, NodeStatus};
use super::remote::{self, Backoff, Outgoing, RemoteSpawn, Started, StreamEvent};
use super::testing::{accept_ws, answer, listen, read_request, read_text, send_text};
use super::wire::{self, Incoming, client_frame, parse_answer, parse_frame};
use super::{Conn, Live, State, apply_status};

const FAST: Backoff = Backoff {
    first: Duration::from_millis(10),
    max: Duration::from_millis(50),
};

#[test]
fn a_client_frame_is_masked_and_reads_back() {
    let payload = vec![b'x'; 300];
    let mut frame = client_frame(0x1, &payload, [1, 2, 3, 4]);
    assert_eq!(frame[0], 0x81);
    assert_eq!(frame[1], 0x80 | 126, "a length past 125 takes two bytes");
    assert_ne!(&frame[8..], &payload[..], "the payload goes masked");
    // Half a frame is not one yet.
    let mut half = frame[..10].to_vec();
    assert_eq!(parse_frame(&mut half).unwrap(), None);
    assert_eq!(
        parse_frame(&mut frame).unwrap(),
        Some((true, 0x1, payload)),
        "unmasked as it is read"
    );
    assert!(frame.is_empty(), "the frame is taken off the buffer");
}

#[test]
fn an_answer_says_its_status_and_the_nodes_error() {
    let raw = b"HTTP/1.1 409 Conflict\r\nContent-Length: 21\r\n\r\n{\"error\":\"it runs\"}  ";
    let answer = parse_answer(raw).unwrap();
    assert_eq!(answer.status, 409);
    assert!(!answer.ok());
    assert_eq!(answer.error(), "it runs");
    assert_eq!(answer.json(), Err("it runs".into()));
    assert_eq!(wire::encode("a b/é"), "a%20b%2F%C3%A9");
}

/// A worker sent from a local workspace asks the node for the variable that
/// sends its board to this machine.
#[test]
fn a_spawn_carries_its_worker_s_variables() {
    let spawn = RemoteSpawn {
        id: "p2".into(),
        cwd: "/home/h/hangar-repos/site-1a-worktrees/w".into(),
        cols: 80,
        rows: 24,
        name: "Nia".into(),
        agent: "claude".into(),
        bridge: true,
        port_base: Some(21020),
        mcp: None,
        env: vec![(remote::BOARD_RELAY.into(), "1".into())],
    };
    assert_eq!(spawn.body()["env"], json!({ "HANGAR_BOARD_RELAY": "1" }));
}

#[test]
fn a_spawn_carries_what_the_node_takes() {
    let spawn = RemoteSpawn {
        id: "p1".into(),
        cwd: "/home/hangar/demo".into(),
        cols: 120,
        rows: 40,
        name: "Ava".into(),
        agent: "claude".into(),
        bridge: true,
        port_base: Some(21000),
        mcp: Some(remote::RemoteMcp {
            workspace_id: "w1".into(),
            workspace_cwd: "/home/hangar/demo".into(),
            enabled: vec!["github".into()],
            strict: true,
        }),
        env: Vec::new(),
    };
    assert_eq!(
        spawn.body(),
        json!({
            "id": "p1", "cwd": "/home/hangar/demo", "cols": 120, "rows": 40,
            "name": "Ava", "agent": "claude", "bridge": true, "portBase": 21000,
            "mcp": {
                "workspaceId": "w1", "workspaceCwd": "/home/hangar/demo",
                "enabled": ["github"], "strict": true,
            },
        })
    );
}

/// What a stream sent its pane, gathered.
fn gathered() -> (
    Arc<Mutex<Vec<StreamEvent>>>,
    impl FnMut(StreamEvent) + Send + 'static,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    (events, move |event| sink.lock().unwrap().push(event))
}

fn output(seq: u64, data: &str, replay: bool) -> StreamEvent {
    StreamEvent::Output {
        seq,
        data: data.into(),
        replay,
    }
}

/// The stream drops; the next connection asks from the last byte seen, gets
/// the size again, and a chunk sent twice across the drop is drawn once.
#[tokio::test]
async fn a_dropped_stream_resumes_from_the_last_byte_seen() {
    let (listener, client) = listen().await;
    let server = tokio::spawn(async move {
        let (mut first, request) = accept_ws(&listener).await;
        assert!(
            request
                .line()
                .starts_with("GET /node/panes/p%201/stream?from=0 "),
            "{}",
            request.line()
        );
        send_text(
            &mut first,
            &json!({ "seq": 6, "data": "first" }).to_string(),
        )
        .await;
        let mut buf = Vec::new();
        let typed = read_text(&mut first, &mut buf).await;
        let sized = read_text(&mut first, &mut buf).await;
        drop(first);

        let (mut second, request) = accept_ws(&listener).await;
        assert!(
            request.line().contains("/stream?from=6 "),
            "{}",
            request.line()
        );
        let mut buf = Vec::new();
        let sized_again = read_text(&mut second, &mut buf).await;
        send_text(
            &mut second,
            &json!({ "seq": 6, "data": "first" }).to_string(),
        )
        .await;
        send_text(
            &mut second,
            &json!({ "seq": 12, "data": "second" }).to_string(),
        )
        .await;
        send_text(&mut second, &json!({ "exit": { "code": 0 } }).to_string()).await;
        (typed, sized, sized_again)
    });

    let (outgoing, inbox) = tokio::sync::mpsc::unbounded_channel();
    let (events, emit) = gathered();
    let target = client.clone();
    let follow = tokio::spawn(remote::follow(
        "n".into(),
        "p 1".into(),
        0,
        inbox,
        emit,
        FAST,
        move |_| Some(target.clone()),
    ));
    outgoing.send(Outgoing::Input("ls\r".into())).unwrap();
    outgoing.send(Outgoing::Resize(100, 30)).unwrap();

    let (typed, sized, sized_again) = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("the server's script ran")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), follow)
        .await
        .expect("the stream ended with the shell")
        .unwrap();
    assert_eq!(typed, json!({ "input": "ls\r" }).to_string());
    let size = json!({ "resize": { "cols": 100, "rows": 30 } }).to_string();
    assert_eq!(sized, size);
    assert_eq!(
        sized_again, size,
        "the size is said again on the new socket"
    );
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            output(6, "first", false),
            output(12, "second", false),
            StreamEvent::Exit
        ]
    );
}

/// What a shell found running printed before is replayed; a reset clears
/// the terminal for the history that follows.
#[tokio::test]
async fn a_found_shells_history_is_replayed_and_a_reset_says_so() {
    let (listener, client) = listen().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = accept_ws(&listener).await;
        for frame in [
            json!({ "reset": { "start": 4 } }),
            json!({ "seq": 8, "data": "old" }),
            json!({ "seq": 12, "data": "new" }),
            json!({ "exit": { "code": null } }),
        ] {
            send_text(&mut socket, &frame.to_string()).await;
        }
    });
    let (_outgoing, inbox) = tokio::sync::mpsc::unbounded_channel();
    let (events, emit) = gathered();
    tokio::time::timeout(
        Duration::from_secs(10),
        remote::follow("n".into(), "p".into(), 10, inbox, emit, FAST, move |_| {
            Some(client.clone())
        }),
    )
    .await
    .expect("the stream ended");
    server.await.unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            StreamEvent::Reset,
            output(8, "old", true),
            output(12, "new", false),
            StreamEvent::Exit
        ]
    );
}

/// A pane the node does not know (it restarted) ends.
#[tokio::test]
async fn a_pane_the_node_lost_ends() {
    let (listener, client) = listen().await;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_request(&mut stream).await;
        answer(&mut stream, 404, r#"{"error":"pty not found"}"#).await;
    });
    let (_outgoing, inbox) = tokio::sync::mpsc::unbounded_channel();
    let (events, emit) = gathered();
    tokio::time::timeout(
        Duration::from_secs(10),
        remote::follow("n".into(), "gone".into(), 0, inbox, emit, FAST, move |_| {
            Some(client.clone())
        }),
    )
    .await
    .expect("the stream gave up");
    server.await.unwrap();
    assert_eq!(*events.lock().unwrap(), vec![StreamEvent::Exit]);
}

/// While its node is away the stream waits, and ends when its pane goes.
#[tokio::test]
async fn a_stream_waits_for_its_node_and_ends_with_its_pane() {
    let (outgoing, inbox) = tokio::sync::mpsc::unbounded_channel();
    let (events, emit) = gathered();
    let follow = tokio::spawn(remote::follow(
        "away".into(),
        "p".into(),
        0,
        inbox,
        emit,
        FAST,
        |_| None,
    ));
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(!follow.is_finished(), "it waits for the node");
    drop(outgoing);
    tokio::time::timeout(Duration::from_secs(5), follow)
        .await
        .expect("the pane went, the stream with it")
        .unwrap();
    assert!(events.lock().unwrap().is_empty());
}

/// A shell still running under the pane's id is followed, not spawned
/// again; one that is not is spawned, with the MCP file the node wrote.
#[tokio::test]
async fn a_pane_finds_its_shell_on_the_node_or_starts_one() {
    let spawn = RemoteSpawn {
        id: "p1".into(),
        cwd: "/home/hangar/demo".into(),
        cols: 80,
        rows: 24,
        name: "Shell".into(),
        agent: "shell".into(),
        bridge: false,
        port_base: None,
        mcp: None,
        env: Vec::new(),
    };
    let (listener, client) = listen().await;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        assert!(request.line().starts_with("GET /node/panes "));
        let listed = json!({ "panes": [{ "id": "p1", "alive": true, "seq": 42 }] });
        answer(&mut stream, 200, &listed.to_string()).await;

        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_request(&mut stream).await;
        let listed = json!({ "panes": [{ "id": "p1", "alive": false, "seq": 42 }] });
        answer(&mut stream, 200, &listed.to_string()).await;
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        assert!(request.line().starts_with("POST /node/panes "));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["cwd"], "/home/hangar/demo");
        let spawned = json!({
            "spawned": { "kitIssues": [] },
            "mcp": { "path": "/n/mcp.json", "servers": ["hangar"], "missing": ["gone"] },
        });
        answer(&mut stream, 200, &spawned.to_string()).await;
    });
    assert_eq!(
        remote::start(&client, &spawn).await,
        Ok(Started::Attached { seq: 42 })
    );
    assert_eq!(
        remote::start(&client, &spawn).await,
        Ok(Started::Spawned {
            mcp: Some(remote::RemoteScope {
                path: "/n/mcp.json".into(),
                servers: vec!["hangar".into()],
                missing: vec!["gone".into()],
            })
        })
    );
    server.await.unwrap();
}

/// The node's folders, for the workspace dialog: the sub-folders only.
#[tokio::test]
async fn a_node_folder_lists_its_sub_folders() {
    let (listener, client) = listen().await;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        assert!(
            request
                .line()
                .starts_with("GET /node/fs?path=%2Fhome%2Fhangar "),
            "{}",
            request.line()
        );
        let listing = json!({
            "path": "/home/hangar", "parent": "/home",
            "entries": [{ "name": "demo", "dir": true }, { "name": "a.txt", "dir": false }],
        });
        answer(&mut stream, 200, &listing.to_string()).await;
    });
    let listing = remote::list_folder(&client, Some("/home/hangar"))
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(listing.path, "/home/hangar");
    assert_eq!(listing.parent.as_deref(), Some("/home"));
    assert_eq!(listing.folders, vec!["demo".to_string()]);
}

/// A message in two fragments, a ping between them, reads as one.
#[tokio::test]
async fn a_socket_joins_fragments_and_answers_pings() {
    let (listener, client) = listen().await;
    let server = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let (mut socket, _) = accept_ws(&listener).await;
        socket
            .write_all(&[0x01, 3, b'a', b'b', b'c'])
            .await
            .unwrap();
        socket.write_all(&[0x89, 1, b'!']).await.unwrap();
        socket.write_all(&[0x80, 2, b'd', b'e']).await.unwrap();
        let mut buf = Vec::new();
        loop {
            if let Some((_, opcode, payload)) = parse_frame(&mut buf).unwrap() {
                return (opcode, payload);
            }
            let mut chunk = [0u8; 64];
            let read = tokio::io::AsyncReadExt::read(&mut socket, &mut chunk)
                .await
                .unwrap();
            assert!(read > 0);
            buf.extend_from_slice(&chunk[..read]);
        }
    });
    let mut socket = wire::Socket::connect(&client, "/node/events")
        .await
        .unwrap();
    let message = loop {
        if let Some(message) = socket.next().unwrap() {
            break message;
        }
        assert!(socket.fill().await.unwrap());
    };
    assert_eq!(message, Incoming::Text("abcde".into()));
    socket.flush().await.unwrap();
    assert_eq!(
        server.await.unwrap(),
        (0xA, b"!".to_vec()),
        "the ping is answered"
    );
}

/// A connected node's client is what the remote panes find; it goes when
/// the node does.
#[tokio::test]
async fn the_directory_follows_the_nodes_status() {
    let node = format!("dir-test-{}", std::process::id());
    let mut st = State::default();
    st.conns.insert(
        node.clone(),
        Conn {
            generation: 1,
            task: tokio::spawn(async {}),
            tunnel: Arc::new(Mutex::new(None)),
            events: None,
        },
    );
    let mut live = Live::default();
    let connected = NodeStatus::Connected {
        info: NodeInfo::default(),
        local_port: 21005,
        token: "tok".into(),
    };
    assert!(
        apply_status(&mut st, &mut live, node.clone(), 1, connected),
        "its events are to be followed"
    );
    assert_eq!(
        remote::directory(&node),
        Some(NodeClient {
            base_url: "http://127.0.0.1:21005".into(),
            token: "tok".into(),
        })
    );
    let dropped = NodeStatus::Connecting(super::model::Step::Tunnel);
    assert!(!apply_status(&mut st, &mut live, node.clone(), 1, dropped));
    assert_eq!(remote::directory(&node), None, "not while it reconnects");
    super::disconnect(&mut st, &mut live, &node);
    assert_eq!(remote::directory(&node), None);
}

/// The code that reaches panes by id finds a remote one: typed into while
/// its shell runs, refused once it ended.
#[test]
fn a_remote_pane_is_typed_into_by_id() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let pane = format!("registry-test-{}", std::process::id());
    let (outgoing, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let alive = Arc::new(AtomicBool::new(true));
    remote::register(&pane, "n", outgoing, alive.clone());
    assert_eq!(remote::node_of(&pane).as_deref(), Some("n"));
    assert_eq!(remote::write(&pane, "ls\r".into()), Some(Ok(())));
    assert_eq!(inbox.try_recv().unwrap(), Outgoing::Input("ls\r".into()));
    alive.store(false, Ordering::Release);
    assert_eq!(remote::alive(&pane), Some(false));
    assert!(matches!(remote::write(&pane, "x".into()), Some(Err(_))));
    remote::unregister(&pane);
    assert_eq!(
        remote::write(&pane, "x".into()),
        None,
        "a local pane's to write"
    );
    assert_eq!(remote::alive(&pane), None);
}
