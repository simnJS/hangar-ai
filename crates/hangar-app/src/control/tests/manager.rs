//! managerControl.test.ts, against the fake port; the projects come from a
//! fake of P19's engine, which reads them as `loadProjects` would.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Value, json};

use super::super::fake::{Fake, lock, pane, workspace};
use super::super::manager::{RELAY_PREFIX, control};
use super::super::port::{BoxFuture, OrchestrationHooks, Port, ProjectView, Projects};
use super::super::types::{Caller, OrchestratorEvent};
use super::contains;
use crate::state::roles::orchestrator_of;
use crate::state::types::PaneRole;
use crate::state::{AgentId, AppState, Workspace, WsId};

/// P19's engine as project_* reaches it: every project but the HQ, its
/// orchestrator, and an overview naming them.
struct Engine;

impl OrchestrationHooks for Engine {
    fn worker_control(
        &self,
        _port: Port,
        _op: String,
        _ws: Workspace,
        _args: Value,
        _caller: Caller,
    ) -> BoxFuture<Result<Value, String>> {
        Box::pin(std::future::ready(Err("not here".into())))
    }

    fn briefing(&self, _port: Port, _args: Value) -> BoxFuture<Result<Value, String>> {
        Box::pin(std::future::ready(Ok(Value::Null)))
    }

    fn post(&self, _ws: &WsId, _event: OrchestratorEvent, _quiet: bool) {}

    fn projects(&self, port: Port) -> BoxFuture<Result<Projects, String>> {
        let snapshot = port.snapshot();
        let projects: Vec<ProjectView> = snapshot
            .state
            .workspaces
            .iter()
            .filter(|ws| ws.hq != Some(true))
            .map(|ws| {
                let orchestrator = orchestrator_of(ws).map(|pane| pane.name.clone());
                let text = format!(
                    "{} — {}",
                    ws.name,
                    orchestrator.as_deref().unwrap_or("no orchestrator")
                );
                ProjectView {
                    id: ws.id.clone(),
                    summary: json!({"name": ws.name, "orchestrator": orchestrator}),
                    text,
                }
            })
            .collect();
        let overview = projects
            .iter()
            .map(|project| project.text.clone())
            .collect::<Vec<_>>()
            .join("\n");
        Box::pin(std::future::ready(Ok(Projects { projects, overview })))
    }
}

fn role(mut pane: crate::state::Pane, role: PaneRole) -> crate::state::Pane {
    pane.role = Some(role);
    pane
}

fn ws(id: &str, name: &str, panes: Vec<crate::state::Pane>) -> Workspace {
    workspace(id, name, &format!("C:\\dev\\{id}"), panes)
}

fn setup(asleep: &[&str], open: &[&str]) -> Arc<Fake> {
    let mut hq = ws(
        "hq",
        "HQ",
        vec![role(
            pane("m", "boss", AgentId::Claude, None),
            PaneRole::Manager,
        )],
    );
    hq.hq = Some(true);
    let fake = Fake::with_state(AppState {
        workspaces: vec![
            hq,
            ws(
                "shop",
                "Shop",
                vec![
                    role(
                        pane("o", "Jack", AgentId::Claude, None),
                        PaneRole::Orchestrator,
                    ),
                    role(pane("w", "Eli", AgentId::Claude, None), PaneRole::Worker),
                ],
            ),
            ws(
                "blog",
                "Blog",
                vec![pane("b", "Ava", AgentId::Claude, None)],
            ),
            ws(
                "wiki",
                "Wiki",
                vec![role(
                    pane("k", "Kim", AgentId::Claude, None),
                    PaneRole::Orchestrator,
                )],
            ),
        ],
        active_workspace_id: Some("hq".into()),
        ..AppState::default()
    });
    *lock(&fake.opened) = open.iter().map(|id| id.to_string()).collect::<HashSet<_>>();
    *lock(&fake.asleep) = asleep.iter().map(|id| id.to_string()).collect();
    *lock(&fake.engine) = Some(Arc::new(Engine));
    fake
}

fn opened() -> Arc<Fake> {
    setup(&[], &["shop", "blog", "wiki", "hq"])
}

fn manager() -> Caller {
    Caller {
        pane_id: Some("m".into()),
        pane_name: Some("boss".into()),
        cwd: Some("C:\\dev\\hq".into()),
    }
}

async fn ask(op: &str, args: Value, caller: Caller, fake: &Arc<Fake>) -> Result<Value, String> {
    control(op, &args, &caller, &fake.port()).await
}

#[tokio::test]
async fn refuses_everyone_but_the_manager() {
    let fake = opened();
    for op in ["project_list", "project_status", "project_send"] {
        for caller in [
            Caller {
                pane_id: Some("o".into()),
                pane_name: Some("Jack".into()),
                cwd: None,
            },
            Caller {
                pane_id: Some("w".into()),
                pane_name: Some("Eli".into()),
                cwd: None,
            },
            Caller::person(),
        ] {
            let refused = ask(
                op,
                json!({"workspace": "Shop", "text": "hi"}),
                caller,
                &fake,
            )
            .await
            .unwrap_err();
            contains(&refused, "project manager");
        }
    }
}

#[tokio::test]
async fn project_list_leaves_the_hq_out_and_keeps_a_workspace_without_orchestrator() {
    let fake = opened();
    let answer = ask("project_list", json!({}), manager(), &fake)
        .await
        .unwrap();
    let names: Vec<_> = answer["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["name"].clone())
        .collect();
    assert_eq!(names, vec![json!("Shop"), json!("Blog"), json!("Wiki")]);
    assert_eq!(answer["projects"][1]["orchestrator"], Value::Null);
    let text = answer["text"].as_str().unwrap();
    contains(text, "Blog");
    contains(text, "no orchestrator");
    assert!(!text.contains("HQ —"));
}

#[tokio::test]
async fn project_status_answers_one_project_found_by_name_or_folder() {
    let fake = opened();
    for reference in ["shop", "C:/DEV/shop/"] {
        let answer = ask(
            "project_status",
            json!({"workspace": reference}),
            manager(),
            &fake,
        )
        .await
        .unwrap();
        assert_eq!(answer["project"]["name"], "Shop");
        contains(answer["text"].as_str().unwrap(), "Shop");
    }
}

#[tokio::test]
async fn project_status_lists_the_names_when_the_project_is_unknown_the_hq_left_out() {
    let fake = opened();
    assert_eq!(
        ask(
            "project_status",
            json!({"workspace": "Nope"}),
            manager(),
            &fake
        )
        .await
        .unwrap_err(),
        "No project \"Nope\". Projects: \"Shop\", \"Blog\", \"Wiki\"."
    );
}

#[tokio::test]
async fn project_send_queues_the_text_for_the_orchestrator_framed_as_relayed() {
    let fake = opened();
    let answer = ask(
        "project_send",
        json!({"workspace": "Shop", "text": "  ship the cart first  "}),
        manager(),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        fake.logged("enqueue:"),
        vec![format!("enqueue:o:{RELAY_PREFIX} ship the cart first")]
    );
    assert_eq!(
        *lock(&fake.senders),
        vec![format!("{RELAY_PREFIX} ship the cart first <- boss")]
    );
    assert_eq!(fake.logged("touch:"), vec!["touch:shop"]);
    assert!(fake.logged("open:").is_empty());
    super::matches(
        &answer,
        json!({"queued": true, "workspace": "Shop", "orchestrator": "Jack"}),
    );
    contains(answer["note"].as_str().unwrap(), "events");
}

#[tokio::test]
async fn project_send_wakes_an_asleep_workspace() {
    let fake = setup(&["wiki"], &["hq", "shop", "blog"]);
    let answer = ask(
        "project_send",
        json!({"workspace": "Wiki", "text": "go"}),
        manager(),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(fake.logged("open:"), vec!["open:wiki"]);
    assert_eq!(fake.logged("enqueue:").len(), 1);
    super::matches(&answer, json!({"queued": true, "woke": true}));
}

#[tokio::test]
async fn project_send_refuses_a_workspace_without_orchestrator_and_says_only_the_person_can_pin_one()
 {
    let fake = opened();
    let refused = ask(
        "project_send",
        json!({"workspace": "Blog", "text": "go"}),
        manager(),
        &fake,
    )
    .await
    .unwrap_err();
    contains(&refused, "has no orchestrator");
    contains(&refused, "Only the person can pin one");
    assert!(fake.logged("enqueue:").is_empty());
}

#[tokio::test]
async fn project_send_refuses_the_hq_itself() {
    let fake = opened();
    contains(
        &ask(
            "project_send",
            json!({"workspace": "HQ", "text": "go"}),
            manager(),
            &fake,
        )
        .await
        .unwrap_err(),
        "is the HQ",
    );
    assert!(fake.logged("enqueue:").is_empty());
}

#[tokio::test]
async fn project_send_refuses_an_empty_text() {
    let fake = opened();
    contains(
        &ask(
            "project_send",
            json!({"workspace": "Shop", "text": " "}),
            manager(),
            &fake,
        )
        .await
        .unwrap_err(),
        "Nothing to send",
    );
}
