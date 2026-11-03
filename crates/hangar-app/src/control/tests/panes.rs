//! control.test.ts: "which workspace a caller is in" and "runControl".

use serde_json::json;

use super::super::fake::{Fake, lock};
use super::super::run::{DISABLED, caller_workspace};
use super::super::types::Caller;
use super::{contains, control, from_ava, from_shop, matches, refusal, with};

fn caller(pane: Option<&str>, cwd: Option<&str>) -> Caller {
    Caller {
        pane_id: pane.map(str::to_string),
        pane_name: None,
        cwd: cwd.map(str::to_string),
    }
}

#[test]
fn follows_the_callers_pane_first() {
    let fake = Fake::setup();
    let state = lock(&fake.state).clone();
    let found = caller_workspace(&state, &caller(Some("p3"), Some("C:/dev/hangar")));
    assert_eq!(found.map(|ws| ws.id.as_str()), Some("w2"));
}

#[test]
fn falls_back_on_the_folder_in_any_spelling_and_on_a_folder_inside_it() {
    let fake = Fake::setup();
    let state = lock(&fake.state).clone();
    let id = |cwd: &str| caller_workspace(&state, &caller(None, Some(cwd))).map(|ws| ws.id.clone());
    assert_eq!(id("c:/DEV/shop/").as_deref(), Some("w2"));
    assert_eq!(id("C:\\dev\\hangar\\src").as_deref(), Some("w1"));
    assert_eq!(id("D:\\elsewhere"), None);
}

#[tokio::test]
async fn refuses_everything_when_the_setting_is_off() {
    let fake = Fake::setup();
    lock(&fake.state).settings.agent_pane_control = false;
    assert_eq!(refusal("pane_list", from_ava(), &fake).await, DISABLED);
}

#[tokio::test]
async fn hands_project_ops_to_the_managers_module_which_refuses_an_ordinary_agent() {
    let fake = Fake::setup();
    contains(
        &refusal("project_list", from_ava(), &fake).await,
        "is for the project manager",
    );
}

#[tokio::test]
async fn lists_the_callers_panes_marking_the_caller_and_its_activity() {
    let fake = Fake::setup();
    let result = control("pane_list", from_ava(), &fake).await.unwrap();
    assert_eq!(result["workspace"]["name"], "Hangar");
    let rows: Vec<_> = result["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pane| json!([pane["name"], pane["you"], pane["activity"], pane["running"]]))
        .collect();
    assert_eq!(
        rows,
        vec![
            json!(["Ava", true, "working", true]),
            json!(["Max", false, null, true])
        ]
    );
}

#[tokio::test]
async fn names_a_workspace_that_is_not_open_and_says_nothing_runs_there() {
    let fake = Fake::setup();
    let result = control(
        "pane_list",
        with(from_ava(), json!({"workspace": "shop"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(result["workspace"]["open"], false);
    contains(result["note"].as_str().unwrap(), "not open");
    assert_eq!(result["panes"][0]["running"], false);
}

#[tokio::test]
async fn creates_a_claude_pane_with_a_queued_prompt_opening_its_workspace() {
    let fake = Fake::setup();
    let result = control(
        "pane_create",
        with(
            from_ava(),
            json!({"workspace": "Shop", "agent": "claude", "name": "Reviewer",
                   "cwd": "packages/api", "prompt": "Review the API"}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(result["pane"]["name"], "Reviewer");
    assert_eq!(result["pane"]["cwd"], "C:\\dev\\shop\\packages\\api");
    let log = fake.log();
    assert!(log.contains(&"open:w2".into()));
    assert!(log.contains(&"enqueue:p10:Review the API".into()));
    // The agent that asked for the pane wrote the prompt, not the person.
    assert_eq!(*lock(&fake.senders), vec!["Review the API <- Ava"]);
    assert!(log.contains(&"announce:created:Reviewer:Ava".into()));
}

#[tokio::test]
async fn refuses_a_prompt_for_an_agent_the_queue_cannot_reach_a_taken_name_and_a_missing_folder() {
    let fake = Fake::setup();
    contains(
        &refusal(
            "pane_create",
            with(from_ava(), json!({"agent": "codex", "prompt": "hi"})),
            &fake,
        )
        .await,
        "Claude Code",
    );
    contains(
        &refusal(
            "pane_create",
            with(from_ava(), json!({"name": "max"})),
            &fake,
        )
        .await,
        "already named",
    );
    // Taken in another workspace counts too: names are unique across Hangar.
    contains(
        &refusal(
            "pane_create",
            with(from_ava(), json!({"workspace": "Shop", "name": "Max"})),
            &fake,
        )
        .await,
        "in \"Hangar\"",
    );
    contains(
        &refusal(
            "pane_create",
            with(from_ava(), json!({"cwd": "missing"})),
            &fake,
        )
        .await,
        "No such folder",
    );
    contains(
        &refusal(
            "pane_create",
            with(from_ava(), json!({"agent": "vim"})),
            &fake,
        )
        .await,
        "Unknown agent",
    );
    assert!(fake.logged("add:").is_empty());
}

#[tokio::test]
async fn closes_a_pane_by_name_but_never_the_last_one_of_a_workspace() {
    let fake = Fake::setup();
    control(
        "pane_close",
        with(from_ava(), json!({"pane": "max"})),
        &fake,
    )
    .await
    .unwrap();
    assert!(fake.log().contains(&"close:w1:p2".into()));
    contains(
        &refusal(
            "pane_close",
            with(from_ava(), json!({"workspace": "Shop", "pane": "Ava"})),
            &fake,
        )
        .await,
        "last pane",
    );
    let fake = Fake::setup();
    let refused = refusal(
        "pane_close",
        with(from_ava(), json!({"pane": "Zed"})),
        &fake,
    )
    .await;
    contains(&refused, "No pane \"Zed\"");
    contains(&refused, "\"Ava\", \"Max\"");
}

#[tokio::test]
async fn finds_a_pane_by_name_in_another_workspace_when_the_callers_has_none() {
    let fake = Fake::setup();
    control(
        "pane_close",
        with(from_shop(), json!({"pane": "Max"})),
        &fake,
    )
    .await
    .unwrap();
    assert!(fake.log().contains(&"close:w1:p2".into()));
}

#[tokio::test]
async fn restarts_resets_and_switches_agents() {
    let fake = Fake::setup();
    for args in [
        json!({"pane": "Ava"}),
        json!({"pane": "Ava", "fresh": true}),
        json!({"pane": "Max", "agent": "claude"}),
    ] {
        control("pane_restart", with(from_ava(), args), &fake)
            .await
            .unwrap();
    }
    assert_eq!(
        fake.logged("respawn:"),
        vec![
            "respawn:w1:p1:{}",
            "respawn:w1:p1:{\"sessionId\":null}",
            "respawn:w1:p2:{\"agent\":\"claude\",\"sessionId\":null}",
        ]
    );
    assert!(fake.log().contains(&"announce:reset:Ava:Ava".into()));
}

#[tokio::test]
async fn types_text_then_enter_then_the_keys_in_order() {
    let fake = Fake::setup();
    control(
        "pane_send",
        with(
            from_ava(),
            json!({"pane": "Max", "text": "npm test", "keys": ["up", "ctrl+c"]}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        fake.typed(),
        vec![
            "paste:npm test",
            "write:p2:\"\\r\"",
            "write:p2:\"\\u001b[A\"",
            "write:p2:\"\\u0003\"",
        ]
    );
}

#[tokio::test]
async fn sends_nothing_at_all_when_a_key_is_unknown() {
    let fake = Fake::setup();
    let refused = refusal(
        "pane_send",
        with(
            from_ava(),
            json!({"pane": "Max", "text": "x", "keys": ["f13"]}),
        ),
        &fake,
    )
    .await;
    contains(&refused, "Unknown key");
    assert!(fake.typed().is_empty());
}

#[tokio::test]
async fn queues_for_an_idle_claude_code_session_only() {
    let fake = Fake::setup();
    control(
        "pane_send",
        with(
            from_ava(),
            json!({"pane": "Ava", "text": "next task", "when_idle": true}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert!(fake.log().contains(&"enqueue:p1:next task".into()));
    assert_eq!(*lock(&fake.senders), vec!["next task <- Ava"]);
    contains(
        &refusal(
            "pane_send",
            with(
                from_ava(),
                json!({"pane": "Max", "text": "x", "when_idle": true}),
            ),
            &fake,
        )
        .await,
        "Claude Code",
    );
}

#[tokio::test]
async fn opens_a_closed_workspace_when_sent_to_and_asks_to_retry() {
    let fake = Fake::setup();
    contains(
        &refusal(
            "pane_send",
            with(
                from_ava(),
                json!({"workspace": "Shop", "pane": "Ava", "text": "hi"}),
            ),
            &fake,
        )
        .await,
        "starting now",
    );
    assert!(fake.log().contains(&"open:w2".into()));
}

#[tokio::test]
async fn says_a_sleeping_workspace_is_asleep_and_how_it_wakes_without_waking_it() {
    let fake = Fake::setup();
    lock(&fake.asleep).insert("w2".into());
    let list = control("workspace_list", from_ava(), &fake).await.unwrap();
    let rows: Vec<_> = list["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ws| json!([ws["name"], ws["asleep"]]))
        .collect();
    assert_eq!(rows, vec![json!(["Hangar", false]), json!(["Shop", true])]);
    let panes = control(
        "pane_list",
        with(from_ava(), json!({"workspace": "Shop"})),
        &fake,
    )
    .await
    .unwrap();
    matches(&panes["workspace"], json!({"open": false, "asleep": true}));
    let note = panes["note"].as_str().unwrap();
    contains(note, "asleep");
    contains(note, "resume their sessions");
    contains(note, "pane_send");
    contains(
        &refusal(
            "pane_read",
            with(from_ava(), json!({"workspace": "Shop", "pane": "Ava"})),
            &fake,
        )
        .await,
        "is asleep",
    );
    // Looking is not using: neither wakes it, and neither counts as use.
    assert!(
        fake.log()
            .iter()
            .all(|line| !line.starts_with("open:") && !line.starts_with("touch:"))
    );
}

#[tokio::test]
async fn wakes_a_sleeping_workspace_when_sent_to_and_counts_that_as_use() {
    let fake = Fake::setup();
    lock(&fake.asleep).insert("w2".into());
    contains(
        &refusal(
            "pane_send",
            with(
                from_ava(),
                json!({"workspace": "Shop", "pane": "Ava", "text": "hi"}),
            ),
            &fake,
        )
        .await,
        "resuming their sessions",
    );
    let log = fake.log();
    assert!(log.contains(&"touch:w2".into()) && log.contains(&"open:w2".into()));
}

#[tokio::test]
async fn counts_creating_and_restarting_panes_as_use_of_their_workspace() {
    let fake = Fake::setup();
    control(
        "pane_restart",
        with(from_ava(), json!({"pane": "Max"})),
        &fake,
    )
    .await
    .unwrap();
    control(
        "pane_create",
        with(from_ava(), json!({"workspace": "Shop"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(fake.logged("touch:"), vec!["touch:w1", "touch:w2"]);
}

#[tokio::test]
async fn starts_a_closed_workspace_for_a_prompt_queued_until_its_claude_code_is_free() {
    let fake = Fake::setup();
    fake.patch("w2", |ws| ws.panes[0].agent = crate::state::AgentId::Claude);
    let out = control(
        "pane_send",
        with(
            from_ava(),
            json!({"workspace": "Shop", "pane": "Ava", "text": "go", "when_idle": true}),
        ),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        json!({"queued": true, "pane": "Ava", "starting": true})
    );
    let log = fake.log();
    assert!(log.contains(&"enqueue:p3:go".into()) && log.contains(&"open:w2".into()));
}

#[tokio::test]
async fn reads_the_bottom_of_a_terminal() {
    let fake = Fake::setup();
    let result = control(
        "pane_read",
        with(from_ava(), json!({"pane": "Max", "lines": 1})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(result["text"], "a.txt");
}

#[tokio::test]
async fn moves_a_workspace_to_its_own_window_and_back_once() {
    let fake = Fake::setup();
    let out = control(
        "workspace_window",
        with(from_ava(), json!({"workspace": "Shop"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(
        out,
        json!({"workspace": "Shop", "detached": true, "changed": true})
    );
    control(
        "workspace_window",
        with(from_ava(), json!({"detach": false})),
        &fake,
    )
    .await
    .unwrap();
    let windows: Vec<String> = fake
        .log()
        .into_iter()
        .filter(|line| line.contains("detach") || line.contains("reattach"))
        .collect();
    assert_eq!(windows, vec!["detach:w2"]);
}

#[test]
fn every_operation_core_relays_is_answered_here() {
    // A name core lets through and the window does not know would be an
    // agent told "Unknown operation" for a tool it was offered.
    let handled = [
        "workspace_list",
        "workspace_create",
        "workspace_set_check",
        "pane_list",
        "pane_create",
        "pane_close",
        "pane_restart",
        "pane_send",
        "pane_read",
        "pane_focus",
        "workspace_window",
        "model_show",
        "model_snapshot",
        "model_close",
        "preview_show",
        "preview_close",
        "kit_tab_list",
        "kit_tab_open",
        "kit_tab_close",
        "worker_spawn",
        "worker_list",
        "worker_stop",
        "delivery_submit",
        "delivery_status",
        "project_list",
        "project_status",
        "project_send",
    ];
    assert_eq!(handled.len(), 27);
    assert_eq!(hangar_core::control::OPERATIONS, handled);
}

#[tokio::test]
async fn hands_worker_ops_to_the_orchestration_engine_and_says_so_without_one() {
    let fake = Fake::setup();
    assert_eq!(
        refusal("worker_list", from_ava(), &fake).await,
        "Workers cannot be started from this Hangar window."
    );
}

#[tokio::test]
async fn focuses_a_pane_in_its_workspace() {
    let fake = Fake::setup();
    let out = control(
        "pane_focus",
        with(from_ava(), json!({"pane": "max"})),
        &fake,
    )
    .await
    .unwrap();
    assert_eq!(out, json!({"focused": "Max", "workspace": "Hangar"}));
    assert!(fake.log().contains(&"activate:w1:p2".into()));
}
