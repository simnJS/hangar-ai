//! The requests of a node's agents (remote.rs): run through the same
//! handlers as a local one, against the node's workspaces alone.

use std::sync::Arc;

use serde_json::{Value, json};

use super::super::fake::{Fake, pane, workspace};
use super::super::port::{AppPort, Port};
use super::super::remote::{RemotePort, narrowed, refused};
use super::contains;
use crate::nodes::testing::{answer, listen, read_request};
use crate::services::Channel;
use crate::state::AgentId;

/// "Hangar" and "Shop" of this machine, and "Demo" on node `node`, with
/// Nova, its orchestrator's pane.
fn with_remote(node: &str) -> Arc<Fake> {
    let fake = Fake::setup();
    let mut demo = workspace(
        "w9",
        "Demo",
        "/home/hangar/demo",
        vec![pane("p9", "Nova", AgentId::Claude, None)],
    );
    demo.node = Some(node.into());
    super::super::fake::lock(&fake.state).workspaces.push(demo);
    super::super::fake::lock(&fake.opened).insert("w9".into());
    fake
}

fn from_nova() -> Value {
    json!({"caller": {"paneId": "p9", "paneName": "Nova", "cwd": "/home/hangar/demo"}})
}

async fn on_node(node: &str, op: &str, args: Value, fake: &Arc<Fake>) -> Result<Value, String> {
    let port: Port = Arc::new(RemotePort::new(fake.port(), node.into()));
    super::super::answer(op, args, Channel::Agent, &port).await
}

#[test]
fn a_node_sees_its_own_workspaces_only() {
    let fake = with_remote("n-a");
    let narrowed = narrowed(&fake.snapshot(), "n-a");
    let ids: Vec<&str> = narrowed
        .state
        .workspaces
        .iter()
        .map(|ws| ws.id.as_str())
        .collect();
    assert_eq!(ids, ["w9"]);
    assert!(narrowed.opened.contains("w9"));
    assert!(!narrowed.opened.contains("w1"), "this machine's");
    assert!(narrowed_of("n-b", &fake).state.workspaces.is_empty());
}

fn narrowed_of(node: &str, fake: &Arc<Fake>) -> crate::state::Snapshot {
    narrowed(&fake.snapshot(), node)
}

#[tokio::test]
async fn a_node_s_orchestrator_lists_and_drives_its_workspace_and_nothing_else() {
    let fake = with_remote("n-a");
    let listed = on_node("n-a", "workspace_list", from_nova(), &fake)
        .await
        .unwrap();
    let names: Vec<&str> = listed["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|ws| ws["name"].as_str())
        .collect();
    assert_eq!(names, ["Demo"], "{listed}");

    let panes = on_node("n-a", "pane_list", from_nova(), &fake)
        .await
        .unwrap();
    assert_eq!(panes["workspace"]["name"], "Demo");
    assert_eq!(panes["panes"][0]["you"], true);

    // This machine's panes and workspaces are not the node's to reach.
    let refusal = on_node(
        "n-a",
        "pane_send",
        json!({"caller": from_nova()["caller"], "pane": "Max", "text": "hi"}),
        &fake,
    )
    .await
    .unwrap_err();
    contains(&refusal, "No pane \"Max\"");
    let refusal = on_node(
        "n-a",
        "pane_list",
        json!({"caller": from_nova()["caller"], "workspace": "Hangar"}),
        &fake,
    )
    .await
    .unwrap_err();
    contains(&refusal, "No workspace \"Hangar\"");
    assert!(fake.typed().is_empty(), "{:?}", fake.typed());
}

#[test]
fn what_reaches_past_the_node_is_refused() {
    assert!(refused("workspace_create").is_some());
    assert!(refused("project_list").is_some());
    assert!(refused("project_send").is_some());
    assert!(refused("worker_spawn").is_none());
    assert!(refused("delivery_submit").is_none());
    assert!(refused("pane_send").is_none());
}

/// What the node's agents queue goes to the node's queue, where the pane's
/// mod reads it.
#[tokio::test]
async fn a_message_for_a_node_s_pane_is_queued_on_the_node() {
    let (listener, client) = listen().await;
    crate::nodes::remote::publish("n-queue", Some(client));
    let fake = with_remote("n-queue");
    let port = RemotePort::new(fake.port(), "n-queue".into());
    let queued = tokio::spawn(async move {
        port.enqueue(&"p9".to_string(), "Your brief", Some("Nova"))
            .await
    });
    let (mut stream, _) = listener.accept().await.unwrap();
    let request = read_request(&mut stream).await;
    answer(
        &mut stream,
        200,
        r#"{"value":{"id":"q1","text":"Your brief"}}"#,
    )
    .await;
    assert!(
        request.line().starts_with("POST /node/bridge/p9 "),
        "{}",
        request.line()
    );
    assert_eq!(
        serde_json::from_str::<Value>(&request.body).unwrap(),
        json!({ "text": "Your brief", "from": "Nova" })
    );
    assert_eq!(queued.await.unwrap(), Ok(()));
    assert!(
        fake.logged("enqueue:").is_empty(),
        "not this machine's queue"
    );
    crate::nodes::remote::publish("n-queue", None);
}
