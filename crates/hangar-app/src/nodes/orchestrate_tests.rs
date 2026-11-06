//! Orchestration in a remote workspace, the desktop's side: what the node's
//! events become here, and the engine's calls on the node (`engine.rs`)
//! against a fake node.

use serde_json::{Value, json};

use hangar_core::board::TaskPatch;
use hangar_core::delivery::{Delivery, Kind, TaskBrief};

use super::engine;
use super::remote;
use super::testing::{TOKEN, answer, listen, read_request};
use crate::message::Message;
use crate::services::{Channel, CoreEvent};
use crate::state::types::{AppState, Pane, Workspace};

fn state() -> AppState {
    AppState {
        workspaces: vec![
            Workspace {
                id: "w-local".into(),
                cwd: "/home/me/site".into(),
                panes: vec![Pane {
                    id: "p-local".into(),
                    ..Pane::default()
                }],
                ..Workspace::default()
            },
            Workspace {
                id: "w-remote".into(),
                cwd: "/home/hangar/demo".into(),
                node: Some("n-orch".into()),
                panes: vec![Pane {
                    id: "p-remote".into(),
                    ..Pane::default()
                }],
                ..Workspace::default()
            },
        ],
        ..AppState::default()
    }
}

#[test]
fn a_node_s_control_request_is_run_here_and_answered_there() {
    let request = json!({
        "event": "control:request",
        "id": "r1",
        "payload": { "id": "r1", "op": "worker_spawn", "args": { "task": "t1" } },
    });
    let messages = super::event_messages("n-orch", &state(), request);
    let [Message::Control(crate::control::Msg::Node(asked))] = messages.as_slice() else {
        panic!("one node request: {messages:?}");
    };
    assert_eq!(asked.node, "n-orch");
    assert_eq!(asked.id, "r1");
    assert_eq!(asked.op, "worker_spawn");
    assert_eq!(asked.args, json!({ "task": "t1" }));
    assert_eq!(asked.channel, Channel::Agent);

    let briefing = json!({
        "event": "control:internal",
        "payload": { "id": "r2", "op": "orchestrator_briefing", "args": { "paneId": "p-remote" } },
    });
    let messages = super::event_messages("n-orch", &state(), briefing);
    let [Message::Control(crate::control::Msg::Node(asked))] = messages.as_slice() else {
        panic!("one node request: {messages:?}");
    };
    assert_eq!(
        (asked.id.as_str(), asked.channel),
        ("r2", Channel::Internal)
    );

    // A request with no id has nothing to answer it with.
    let blind = json!({ "event": "control:request", "payload": { "op": "pane_list" } });
    assert!(super::event_messages("n-orch", &state(), blind).is_empty());
}

#[test]
fn a_node_reports_for_its_own_panes_only() {
    let report = |pane: &str| json!({ "event": "bridge:event", "paneId": pane, "payload": { "kind": "stop" } });
    let messages = super::event_messages("n-orch", &state(), report("p-remote"));
    let [Message::Core(CoreEvent::Bridge { pane, event })] = messages.as_slice() else {
        panic!("one bridge report: {messages:?}");
    };
    assert_eq!(pane, "p-remote");
    assert_eq!(event, &json!({ "kind": "stop" }));
    assert!(
        super::event_messages("n-orch", &state(), report("p-local")).is_empty(),
        "a pane of this machine"
    );
    assert!(
        super::event_messages("n-other", &state(), report("p-remote")).is_empty(),
        "another node's pane"
    );

    let changed = json!({ "event": "board:changed", "cwd": "/home/hangar/demo" });
    let messages = super::event_messages("n-orch", &state(), changed);
    assert!(matches!(
        messages.as_slice(),
        [Message::Core(CoreEvent::BoardChanged { cwd })] if cwd == "/home/hangar/demo"
    ));
    let lost = super::event_messages("n-orch", &state(), json!({ "lagged": 3 }));
    assert!(matches!(
        lost.as_slice(),
        [Message::Board(crate::board::Msg::Reload)]
    ));
}

#[test]
fn which_node_a_folder_or_a_pane_is_on() {
    let state = state();
    assert_eq!(
        engine::node_of_cwd(&state, "/home/hangar/demo").as_deref(),
        Some("n-orch")
    );
    assert_eq!(engine::node_of_cwd(&state, "/home/me/site"), None);
    assert_eq!(
        engine::node_of_pane(&state, "p-remote").as_deref(),
        Some("n-orch")
    );
    assert_eq!(engine::node_of_pane(&state, "p-local"), None);
}

/// One request to a fake node, answered `status` with `body`: what the
/// client sent, `(request line, body)`.
async fn one_call<T: Send + 'static>(
    status: u16,
    body: Value,
    call: impl FnOnce(super::NodeClient) -> tokio::task::JoinHandle<T>,
) -> (String, Value, T) {
    let (listener, client) = listen().await;
    let caller = call(client);
    let (mut stream, _) = listener.accept().await.expect("a connection");
    let request = read_request(&mut stream).await;
    assert!(
        request
            .head
            .contains(&format!("Authorization: Bearer {TOKEN}")),
        "the token is sent"
    );
    answer(&mut stream, status, &body.to_string()).await;
    let sent = serde_json::from_str(&request.body).unwrap_or(Value::Null);
    (request.line().to_string(), sent, caller.await.unwrap())
}

#[tokio::test]
async fn a_worker_s_worktree_is_made_on_its_node() {
    let (line, sent, made) = one_call(200, json!({ "value": null }), |client| {
        tokio::spawn(async move {
            engine::worktree_add(
                &client,
                "/home/hangar/demo",
                "/home/hangar/demo-worktrees/w1",
                "hangar/w1",
                true,
                Some("main"),
            )
            .await
        })
    })
    .await;
    assert!(line.starts_with("POST /node/worktree "), "{line}");
    assert_eq!(
        sent,
        json!({
            "repo": "/home/hangar/demo", "path": "/home/hangar/demo-worktrees/w1",
            "branch": "hangar/w1", "create": true, "base": "main",
        })
    );
    assert_eq!(made, Ok(()));

    let (_, _, refused) = one_call(409, json!({ "error": "branch exists" }), |client| {
        tokio::spawn(
            async move { engine::worktree_add(&client, "/r", "/r-w", "b", true, None).await },
        )
    })
    .await;
    assert_eq!(refused, Err("branch exists".into()));
}

#[tokio::test]
async fn the_engine_reads_a_node_s_repository_through_its_known_questions() {
    let info = json!({ "value": {
        "installed": true, "isRepo": true, "root": "/home/hangar/demo",
        "mainRoot": "/home/hangar/demo", "isLinkedWorktree": false,
        "branch": "main", "head": "abc",
    }});
    let (line, sent, read) = one_call(200, info, |client| {
        tokio::spawn(async move { engine::repo_info(&client, "/home/hangar/demo").await })
    })
    .await;
    assert!(line.starts_with("POST /node/git "), "{line}");
    assert_eq!(
        sent,
        json!({ "op": "repo_info", "path": "/home/hangar/demo" })
    );
    assert!(read.installed && read.is_repo);
    assert_eq!(read.branch.as_deref(), Some("main"));

    let trees = json!({ "value": [
        { "path": "/home/hangar/demo", "head": "abc", "branch": "main", "isMain": true,
          "isCurrent": true, "bare": false, "detached": false, "locked": null, "prunable": null },
        { "path": "/home/hangar/demo-worktrees/w1", "head": "def", "branch": "hangar/w1",
          "isMain": false, "isCurrent": false, "bare": false, "detached": false,
          "locked": null, "prunable": null },
    ]});
    let (_, sent, listed) = one_call(200, trees, |client| {
        tokio::spawn(async move { engine::worktree_list(&client, "/home/hangar/demo").await })
    })
    .await;
    assert_eq!(sent["op"], "worktree_list");
    let listed = listed.unwrap();
    assert_eq!(listed[1].branch.as_deref(), Some("hangar/w1"));
    assert!(listed[0].is_main);

    let (_, sent, ahead) = one_call(200, json!({ "value": 2 }), |client| {
        tokio::spawn(async move { engine::commits_ahead(&client, "/r", "main", "hangar/w1").await })
    })
    .await;
    assert_eq!(
        sent,
        json!({ "op": "commits_ahead", "path": "/r", "base": "main", "head": "hangar/w1" })
    );
    assert_eq!(ahead, Ok(2));

    // A node that does not answer is git missing, as a local failure is.
    let (_, _, missing) = one_call(500, json!({ "error": "down" }), |client| {
        tokio::spawn(async move { engine::repo_info(&client, "/r").await })
    })
    .await;
    assert!(!missing.installed);
}

fn delivery() -> Delivery {
    Delivery {
        workspace: "/home/hangar/demo".into(),
        worktree: "/home/hangar/demo-worktrees/w1".into(),
        branch: "hangar/w1".into(),
        target: "main".into(),
        task: TaskBrief {
            id: "t1".into(),
            title: "The task".into(),
            description: String::new(),
            owns: vec!["src/".into()],
        },
        summary: Some("done".into()),
        check_command: Some("cargo test".into()),
        shell: Some(hangar_core::shells::ShellInfo {
            id: "zsh".into(),
            label: "zsh".into(),
            program: "/opt/homebrew/bin/zsh".into(),
            args: Vec::new(),
        }),
        reviewer_model: "opus".into(),
        claude_account: None,
        land: true,
        reuse: false,
        port_base: Some(21000),
        workspace_id: "w-remote".into(),
        build_env: vec![("CARGO_TARGET_DIR".into(), "/Users/me/cache".into())],
        check_root: None,
        data_dir: None,
    }
}

/// The delivery runs on the node with the workspace's check command, and
/// without what only means something on this machine; what it did reads
/// back as core's outcome.
#[tokio::test]
async fn a_delivery_runs_on_the_node_with_the_workspace_s_check() {
    let outcome = json!({ "value": {
        "outcome": "checkFailed", "detail": "The check failed.", "files": [],
        "inCheckout": false, "changed": ["src/a.rs"], "outsideOwns": [],
        "check": { "command": "cargo test", "passed": false, "exitCode": 101,
                   "timedOut": false, "seconds": 42, "tail": "1 failed" },
        "checkSkipped": null, "review": null, "merge": "m1", "base": "b1", "landedIn": null,
    }});
    let request = crate::control::remote::for_node(delivery());
    let (line, sent, ran) = one_call(200, outcome, move |client| {
        tokio::spawn(async move { engine::delivery_run(&client, &request).await })
    })
    .await;
    assert!(line.starts_with("POST /node/delivery/run "), "{line}");
    assert_eq!(sent["checkCommand"], "cargo test");
    assert_eq!(sent["reviewerModel"], "opus");
    assert_eq!(sent["land"], true);
    assert_eq!(sent["task"]["owns"], json!(["src/"]));
    assert_eq!(sent["shell"], Value::Null, "the node's own shell");
    assert_eq!(sent["buildEnv"], json!([]), "no cache of this machine's");
    let ran = ran.unwrap();
    assert_eq!(ran.outcome, Kind::CheckFailed);
    let check = ran.check.expect("the check ran");
    assert_eq!((check.exit_code, check.seconds), (Some(101), 42));
    assert_eq!(check.tail, "1 failed");
    assert_eq!(ran.merge.as_deref(), Some("m1"));
    // What core's Delivery reads, it reads.
    let read: Delivery = serde_json::from_value(sent).unwrap();
    assert_eq!(read.check_command.as_deref(), Some("cargo test"));
}

#[tokio::test]
async fn a_control_answer_goes_back_to_its_node() {
    let (listener, client) = listen().await;
    remote::publish("n-reply", Some(client));
    let replying = tokio::spawn(async move {
        engine::control_reply("n-reply", "r 1", Ok(json!({ "panes": [] }))).await
    });
    let (mut stream, _) = listener.accept().await.unwrap();
    let request = read_request(&mut stream).await;
    answer(&mut stream, 200, r#"{"ok":true}"#).await;
    assert!(
        request
            .line()
            .starts_with("POST /node/control/r%201/reply "),
        "{}",
        request.line()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&request.body).unwrap(),
        json!({ "ok": true, "value": { "panes": [] } })
    );
    assert_eq!(replying.await.unwrap(), Ok(()));
    remote::publish("n-reply", None);
    assert!(
        engine::control_reply("n-reply", "r2", Err("no".into()))
            .await
            .unwrap_err()
            .contains("not connected")
    );
}

#[test]
fn a_patch_is_sent_as_the_node_reads_it() {
    let patch = TaskPatch {
        column: Some("review".into()),
        labels: Some(vec!["ready-to-land".into()]),
        ..TaskPatch::default()
    };
    let sent = engine::patch_json(&patch).unwrap();
    assert_eq!(
        sent,
        json!({ "column": "review", "labels": ["ready-to-land"] })
    );
    let read: TaskPatch = serde_json::from_value(sent).unwrap();
    assert_eq!(read.column.as_deref(), Some("review"));
    let blocked = TaskPatch {
        block: Some(Default::default()),
        ..TaskPatch::default()
    };
    assert!(engine::patch_json(&blocked).is_err());
}
