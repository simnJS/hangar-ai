//! remoteControl.test.ts, case for case. Its four `usageReader` cases are
//! P11's cache's (`stats::cache`, where the reader lives), ported there.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use chrono::{Local, TimeZone};
use serde_json::{Value, json};

use hangar_core::chat::{
    Body, CallState, ChatItem, ChatPage, OpenCall, PendingQuestion, Question, QuestionOption,
    Sender,
};
use hangar_core::remote::ActivityEntry;
use hangar_core::stats::{Agent, Bucket, ModelInfo, ScanInfo, SessionInfo, UsageStats};
use hangar_i18n::Locale;

use crate::control::port::{MobileHooks, Port};
use crate::orchestration::manager_feed::{ManagerEvent, ManagerJournalEntry};
use crate::orchestration::testkit::{strings, task};
use crate::state::live::{BridgeEvent, RateLimit, ToolPhase, TurnPhase, UsageContext};
use crate::state::types::PaneRole;
use crate::state::{AgentActivity, AgentId, AppState, Pane, Workspace};
use crate::stats::model::model_label;
use crate::stats::time::{add_days, start_of_day};

use super::super::engine::{ChatRead, Hook, Mobile, locked, run};
use super::fake::{App, Fakes, assert_equal, assert_matches, claude, pane, workspace};

struct Rig {
    app: Arc<App>,
    fakes: Arc<Fakes>,
    mobile: Arc<Mobile>,
    port: Port,
}

impl Rig {
    async fn run(&self, op: &str, args: Value) -> Result<Value, String> {
        run(op, &args, &self.port, &self.mobile).await
    }

    async fn ok(&self, op: &str, args: Value) -> Value {
        self.run(op, args)
            .await
            .unwrap_or_else(|error| panic!("{op}: {error}"))
    }

    #[track_caller]
    fn err(&self, answer: Result<Value, String>, part: &str) {
        match answer {
            Err(error) => assert!(error.contains(part), "{error:?} does not say {part:?}"),
            Ok(value) => panic!("answered {value} where {part:?} was expected"),
        }
    }

    fn act(&self, pane: &str, ws: &str, activity: AgentActivity) {
        self.app.act(pane, ws, activity);
    }

    fn note(&self, pane: &str, event: BridgeEvent, at: u64) {
        self.mobile.note_bridge_event(pane, &event, at);
    }

    fn typed(&self) -> Vec<String> {
        self.app.typed()
    }

    fn opened(&self) -> Vec<String> {
        self.app.logged("open:")
    }

    fn touched(&self) -> Vec<String> {
        self.app.logged("touch:")
    }
}

/// `rig(workspaces, board, withManager)`: every workspace open but
/// "asleepWs", every pane with a terminal, the agents' switch off.
fn rig(
    workspaces: Vec<Workspace>,
    board: Vec<hangar_core::board::Task>,
    with_manager: bool,
) -> Rig {
    let mut all = Vec::new();
    if with_manager {
        let mut hq = workspace(
            "hq",
            "HQ",
            vec![claude("pm", "Manager", Some(PaneRole::Manager))],
        );
        hq.hq = Some(true);
        all.push(hq);
    }
    all.extend(workspaces);
    let mut state = AppState {
        workspaces: all,
        ..AppState::default()
    };
    state.settings.agent_pane_control = false;
    let app = App::new(state);
    if locked(&app.state)
        .workspaces
        .iter()
        .any(|ws| ws.id == "asleepWs")
    {
        app.sleep_ws("asleepWs");
    } else {
        locked(&app.asleep).insert("asleepWs".into());
    }
    let fakes = Fakes::new(app.clone());
    *locked(&fakes.board) = board;
    let mobile = fakes.mobile();
    let port = app.port();
    Rig {
        app,
        fakes,
        mobile,
        port,
    }
}

fn shop() -> Workspace {
    workspace(
        "shop",
        "Shop",
        vec![
            claude("orch", "Jack", Some(PaneRole::Orchestrator)),
            claude("w1", "Ben", Some(PaneRole::Worker)),
            pane("sh", "term", None, AgentId::Shell),
        ],
    )
}

fn labelled(id: &str, labels: &[&str], column: &str) -> hangar_core::board::Task {
    hangar_core::board::Task {
        labels: strings(labels),
        column: column.into(),
        ..task(id)
    }
}

/* ── remote_state ─────────────────────────────────────────────── */

#[tokio::test]
async fn state_lists_projects_without_the_hq_with_panes_board_and_manager() {
    let r = rig(
        vec![shop()],
        vec![labelled("t1", &[], "doing"), task("t2")],
        true,
    );
    r.act("orch", "shop", AgentActivity::Working);
    r.act("w1", "shop", AgentActivity::Idle);
    r.act("pm", "hq", AgentActivity::Yours);
    let state = r.ok("remote_state", json!({})).await;
    assert_equal(
        &state["manager"],
        json!({"paneId": "pm", "name": "Manager", "activity": "yours"}),
    );
    let ids: Vec<&str> = state["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["shop"]);
    let project = &state["projects"][0];
    assert_matches(
        project,
        json!({
            "name": "Shop", "git": true, "asleep": false,
            "orchestrator": {"name": "Jack", "activity": "working"},
            "board": {"todo": 1, "doing": 1, "review": 0, "done": 0},
            "planStatus": null,
        }),
    );
    assert_equal(
        &project["panes"],
        json!([
            {"id": "orch", "name": "Jack", "agent": "claude", "role": "orchestrator", "activity": "working"},
            {"id": "w1", "name": "Ben", "agent": "claude", "role": "worker", "activity": "idle"},
            {"id": "sh", "name": "term", "agent": "shell", "role": null, "activity": null},
        ]),
    );
    assert!(state["at"].is_u64());
}

#[tokio::test]
async fn state_has_no_manager_when_none_is_open() {
    let r = rig(vec![shop()], vec![], false);
    let state = r.ok("remote_state", json!({})).await;
    assert!(state["manager"].is_null());
}

#[tokio::test]
async fn state_lists_what_waits_for_the_person_oldest_first() {
    let r = rig(
        vec![shop()],
        vec![
            labelled("n1", &["needs-you"], "todo"),
            labelled("r1", &["ready-to-land"], "review"),
            labelled("d1", &["needs-you"], "done"),
        ],
        true,
    );
    r.act("w1", "shop", AgentActivity::Waiting);
    *locked(&r.fakes.manager) = vec![ManagerJournalEntry {
        at: 1000,
        event: ManagerEvent::PaneWaiting {
            workspace_id: "shop".into(),
            workspace: "Shop".into(),
            pane_id: "w1".into(),
            pane: "Ben".into(),
            for_ms: 0,
        },
    }];
    let state = r.ok("remote_state", json!({})).await;
    let waiting: Vec<(String, String)> = state["waiting"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            let what = w.get("paneId").or_else(|| w.get("taskId")).unwrap();
            (
                w["kind"].as_str().unwrap().into(),
                what.as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        waiting,
        [
            ("pane".into(), "w1".into()),
            ("needsYou".into(), "n1".into()),
            ("readyToLand".into(), "r1".into())
        ]
    );
    assert_matches(
        &state["waiting"][0],
        json!({"workspaceId": "shop", "workspace": "Shop", "pane": "Ben", "since": 1000}),
    );
    assert_matches(
        &state["waiting"][1],
        json!({"title": "Task n1", "workspace": "Shop"}),
    );
    assert_equal(
        &state["projects"][0]["needsYou"],
        json!([{"id": "n1", "title": "Task n1"}]),
    );
    assert_equal(
        &state["projects"][0]["readyToLand"],
        json!([{"id": "r1", "title": "Task r1"}]),
    );
}

/* ── remote_screen ────────────────────────────────────────────── */

#[tokio::test]
async fn screen_returns_the_pane_s_text_found_by_name_or_id() {
    let r = rig(vec![shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Working);
    let by_name = r
        .ok("remote_screen", json!({"pane": "ben", "lines": 2}))
        .await;
    assert_equal(
        &by_name,
        json!({"paneId": "w1", "pane": "Ben", "workspace": "Shop", "agent": "claude", "activity": "working", "text": "two\nthree"}),
    );
    let by_id = r.ok("remote_screen", json!({"pane": "w1"})).await;
    assert_eq!(by_id["text"], "one\ntwo\nthree");
}

#[tokio::test]
async fn screen_names_an_unknown_pane() {
    let r = rig(vec![shop()], vec![], true);
    r.err(
        r.run("remote_screen", json!({"pane": "ghost"})).await,
        "No pane \"ghost\"",
    );
}

/* ── remote_send ──────────────────────────────────────────────── */

#[tokio::test]
async fn send_types_text_then_enter_as_the_person() {
    let r = rig(vec![shop()], vec![], true);
    assert_equal(
        &r.ok("remote_send", json!({"pane": "Ben", "text": "hello"}))
            .await,
        json!({"sent": true}),
    );
    assert_eq!(r.typed(), ["paste:w1:hello", r#"write:w1:"\r""#]);
}

#[tokio::test]
async fn send_sends_keys_and_text_without_submitting() {
    let r = rig(vec![shop()], vec![], true);
    r.ok(
        "remote_send",
        json!({"pane": "w1", "text": "draft", "submit": false, "keys": ["up", "ctrl+c"]}),
    )
    .await;
    assert_eq!(
        r.typed(),
        [
            "paste:w1:draft",
            r#"write:w1:"\u001b[A""#,
            r#"write:w1:"\u0003""#
        ]
    );
}

#[tokio::test]
async fn send_refuses_a_key_outside_the_contract_and_sends_nothing() {
    let r = rig(vec![shop()], vec![], true);
    r.err(
        r.run("remote_send", json!({"pane": "w1", "keys": ["backspace"]}))
            .await,
        "Unknown key",
    );
    assert!(r.typed().is_empty());
}

#[tokio::test]
async fn send_opens_a_closed_workspace_first_and_types_into_a_shell_once_it_runs() {
    let r = rig(vec![shop()], vec![], true);
    locked(&r.app.live).remove("sh");
    r.ok("remote_send", json!({"pane": "term", "text": "ls"}))
        .await;
    assert_eq!(r.opened(), ["shop"]);
    assert_eq!(r.typed(), ["paste:sh:ls", r#"write:sh:"\r""#]);
}

#[tokio::test]
async fn send_names_an_unknown_pane() {
    let r = rig(vec![shop()], vec![], true);
    r.err(
        r.run("remote_send", json!({"pane": "nobody", "text": "x"}))
            .await,
        "No pane \"nobody\"",
    );
}

/* ── remote_permission ────────────────────────────────────────── */

#[tokio::test]
async fn permission_answers_enter_to_allow_and_escape_to_refuse_a_waiting_pane() {
    let r = rig(vec![shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Waiting);
    assert_equal(
        &r.ok("remote_permission", json!({"pane": "Ben", "allow": true}))
            .await,
        json!({"answered": true}),
    );
    r.ok("remote_permission", json!({"pane": "Ben", "allow": false}))
        .await;
    assert_eq!(r.typed(), [r#"write:w1:"\r""#, r#"write:w1:"\u001b""#]);
}

#[tokio::test]
async fn permission_is_refused_unless_the_pane_is_waiting() {
    let r = rig(vec![shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Working);
    r.err(
        r.run("remote_permission", json!({"pane": "Ben", "allow": true}))
            .await,
        "not waiting",
    );
    assert!(r.typed().is_empty());
}

/* ── remote_manager ───────────────────────────────────────────── */

#[tokio::test]
async fn manager_types_into_the_manager_s_pane() {
    let r = rig(vec![shop()], vec![], true);
    assert_equal(
        &r.ok("remote_manager", json!({"text": "status?"})).await,
        json!({"sent": true}),
    );
    assert_eq!(r.typed(), ["paste:pm:status?", r#"write:pm:"\r""#]);
}

#[tokio::test]
async fn manager_is_refused_without_a_manager() {
    let r = rig(vec![shop()], vec![], false);
    r.err(
        r.run("remote_manager", json!({"text": "hi"})).await,
        "Open the project manager on the PC",
    );
}

/* ── remote_history ───────────────────────────────────────────── */

fn line(at: u64, workspace: &str, kind: &str, text: &str) -> ActivityEntry {
    ActivityEntry {
        at,
        workspace: workspace.into(),
        kind: kind.into(),
        text: text.into(),
    }
}

/// The log on disk: what is after `since`.
fn disk(r: &Rig) {
    let stored = [
        line(1000, "Shop", "landed", "Landed: Cart"),
        line(3000, "Shop", "needsYou", "Needs you: Ship"),
        line(2000, "", "appStopped", ""),
    ];
    *locked(&r.fakes.history) = Box::new(move |since| {
        Ok(stored
            .iter()
            .filter(|entry| entry.at > since)
            .cloned()
            .collect())
    });
}

fn rows(answer: &Value) -> Vec<(u64, String, String)> {
    answer["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["at"].as_u64().unwrap(),
                e["workspace"].as_str().unwrap().into(),
                e["kind"].as_str().unwrap().into(),
            )
        })
        .collect()
}

#[tokio::test]
async fn history_answers_from_the_log_on_disk_newest_first_after_since() {
    let r = rig(vec![shop()], vec![], true);
    disk(&r);
    let all = r.ok("remote_history", json!({})).await;
    assert_eq!(
        rows(&all),
        [
            (3000, "Shop".into(), "needsYou".into()),
            (2000, "".into(), "appStopped".into()),
            (1000, "Shop".into(), "landed".into())
        ]
    );
    let recent = r.ok("remote_history", json!({"since": 1500})).await;
    let ats: Vec<u64> = rows(&recent).into_iter().map(|(at, _, _)| at).collect();
    assert_eq!(ats, [3000, 2000]);
    assert_eq!(*locked(&r.fakes.reads), [0, 1500]);
}

#[tokio::test]
async fn history_does_not_read_the_journals_in_memory_which_a_restart_forgets() {
    let r = rig(vec![shop()], vec![], true);
    *locked(&r.fakes.manager) = vec![ManagerJournalEntry {
        at: 9000,
        event: ManagerEvent::ReadyToLand {
            workspace_id: "shop".into(),
            workspace: "Shop".into(),
            task_id: "t9".into(),
            title: "Ship".into(),
        },
    }];
    let answer = r.ok("remote_history", json!({})).await;
    assert_equal(&answer["entries"], json!([]));
}

#[tokio::test]
async fn history_words_a_line_written_without_text_in_the_language_of_the_pc() {
    let r = rig(vec![shop()], vec![], true);
    disk(&r);
    r.app
        .change(|state| state.settings.locale = Some(Locale::En));
    let en = r.ok("remote_history", json!({})).await;
    let text = |answer: &Value, kind: &str| {
        answer["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == kind)
            .unwrap()["text"]
            .clone()
    };
    assert_eq!(text(&en, "appStopped"), "Hangar stopped");
    r.app
        .change(|state| state.settings.locale = Some(Locale::Fr));
    let fr = r.ok("remote_history", json!({})).await;
    assert_eq!(text(&fr, "appStopped"), "Hangar s'est arrêté");
    assert_eq!(text(&fr, "landed"), "Landed: Cart");
}

#[tokio::test]
async fn history_is_an_error_when_the_log_cannot_be_read() {
    let r = rig(vec![shop()], vec![], true);
    *locked(&r.fakes.history) = Box::new(|_| Err("disk".into()));
    r.err(r.run("remote_history", json!({})).await, "disk");
}

/* ── remote_chat ──────────────────────────────────────────────── */

fn items() -> Vec<ChatItem> {
    vec![
        ChatItem {
            id: "u1".into(),
            at: 1000,
            body: Body::User {
                text: "Fix the cart".into(),
                from: Sender::Person,
                images: 0,
            },
        },
        ChatItem {
            id: "t1".into(),
            at: 2000,
            body: Body::Action {
                tool: "Bash".into(),
                title: "Ran pnpm test".into(),
                detail: Some("pnpm test".into()),
                result: None,
                state: CallState::Running,
            },
        },
    ]
}

fn page() -> ChatPage {
    ChatPage {
        items: items(),
        older: true,
        cursor: Some("4096".into()),
        question: None,
        open_call: None,
    }
}

fn items_json() -> Value {
    serde_json::to_value(items()).unwrap()
}

fn chat_shop() -> Workspace {
    workspace(
        "shop",
        "Shop",
        vec![
            Pane {
                session_id: Some("sess-1".into()),
                cwd: Some("C:\\wt\\ben".into()),
                ..claude("w1", "Ben", Some(PaneRole::Worker))
            },
            Pane {
                session_id: Some("sess-2".into()),
                ..claude("orch", "Jack", Some(PaneRole::Orchestrator))
            },
            claude("new", "Ava", Some(PaneRole::Worker)),
            Pane {
                session_id: Some("nope".into()),
                ..pane("sh", "term", None, AgentId::Shell)
            },
        ],
    )
}

fn reads(r: &Rig, answer: Option<ChatPage>) {
    *locked(&r.fakes.chat) = Box::new(move |_| Ok(answer.clone()));
}

fn read_of(session: &str, cwd: &str, before: Option<&str>, limit: Option<usize>) -> ChatRead {
    ChatRead {
        session_id: session.into(),
        cwd: cwd.into(),
        before: before.map(str::to_string),
        limit,
    }
}

#[tokio::test]
async fn chat_reads_a_claude_pane_s_transcript_in_its_own_folder_and_passes_the_cursor_on() {
    let r = rig(vec![chat_shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Working);
    reads(&r, Some(page()));
    let answer = r
        .ok(
            "remote_chat",
            json!({"pane": "Ben", "before": "9000", "limit": 20}),
        )
        .await;
    assert_eq!(
        *locked(&r.fakes.chats),
        [read_of("sess-1", "C:\\wt\\ben", Some("9000"), Some(20))]
    );
    assert_equal(
        &answer,
        json!({
            "paneId": "w1", "pane": "Ben", "workspace": "Shop", "agent": "claude", "activity": "working",
            "sessionId": "sess-1", "items": items_json(), "older": true, "cursor": "4096", "pending": null,
        }),
    );
}

#[tokio::test]
async fn chat_reads_a_pane_without_a_folder_of_its_own_in_its_workspace_s_newest_page_first() {
    let r = rig(vec![chat_shop()], vec![], true);
    reads(
        &r,
        Some(ChatPage {
            older: false,
            cursor: None,
            ..page()
        }),
    );
    let answer = r.ok("remote_chat", json!({"pane": "orch"})).await;
    assert_eq!(
        *locked(&r.fakes.chats),
        [read_of("sess-2", "C:\\dev\\shop", None, None)]
    );
    assert_matches(
        &answer,
        json!({"older": false, "cursor": null, "activity": null}),
    );
}

#[tokio::test]
async fn chat_answers_no_items_and_why_for_a_shell_a_pane_without_a_session_a_session_not_written_yet()
 {
    let r = rig(vec![chat_shop()], vec![], true);
    reads(&r, None);
    let shell = r.ok("remote_chat", json!({"pane": "term"})).await;
    assert_matches(
        &shell,
        json!({"agent": "shell", "items": [], "older": false, "cursor": null, "pending": null, "reason": "notClaude"}),
    );
    let fresh = r.ok("remote_chat", json!({"pane": "Ava"})).await;
    assert_matches(
        &fresh,
        json!({"sessionId": null, "items": [], "pending": null, "reason": "noSession"}),
    );
    assert!(locked(&r.fakes.chats).is_empty());
    let unwritten = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert_matches(
        &unwritten,
        json!({"sessionId": "sess-1", "items": [], "reason": "noTranscript"}),
    );
    assert_eq!(locked(&r.fakes.chats).len(), 1);
}

fn open_call() -> OpenCall {
    OpenCall {
        tool: "Bash".into(),
        title: "Ran pnpm test".into(),
        detail: Some("pnpm test".into()),
    }
}

#[tokio::test]
async fn chat_derives_a_pending_permission_from_the_pane_waiting_on_a_call_without_its_result() {
    let r = rig(vec![chat_shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Waiting);
    reads(
        &r,
        Some(ChatPage {
            open_call: Some(open_call()),
            ..page()
        }),
    );
    let waiting = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert_equal(
        &waiting["pending"],
        json!({"kind": "permission", "tool": "Bash", "title": "Ran pnpm test", "detail": "pnpm test"}),
    );

    // Running on its own: the call is under way, nothing asks the person.
    r.act("w1", "shop", AgentActivity::Working);
    let working = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert!(working["pending"].is_null());

    // Waiting with every call answered: nothing this chat can name.
    r.act("w1", "shop", AgentActivity::Waiting);
    reads(&r, Some(page()));
    let settled = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert!(settled["pending"].is_null());
}

fn question(header: &str, text: &str, labels: &[(&str, &str)], multi: bool) -> Question {
    Question {
        question: text.into(),
        header: header.into(),
        multi_select: multi,
        options: labels
            .iter()
            .map(|(label, description)| QuestionOption {
                label: (*label).into(),
                description: (*description).into(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn chat_puts_an_unanswered_question_first_whatever_the_activity() {
    let r = rig(vec![chat_shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Waiting);
    let asked = PendingQuestion {
        tool_use_id: "toolu_q".into(),
        questions: vec![question(
            "Currency",
            "Which currency?",
            &[("EUR", "Euro")],
            false,
        )],
    };
    reads(
        &r,
        Some(ChatPage {
            question: Some(asked.clone()),
            open_call: Some(OpenCall {
                tool: "Bash".into(),
                title: "Ran ls".into(),
                detail: None,
            }),
            ..page()
        }),
    );
    let answer = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    let mut expected = serde_json::to_value(&asked).unwrap();
    expected["kind"] = json!("question");
    assert_equal(&answer["pending"], expected);
}

#[tokio::test]
async fn chat_refuses_a_cursor_that_is_not_one_and_an_unknown_pane() {
    let r = rig(vec![chat_shop()], vec![], true);
    reads(&r, Some(page()));
    r.err(
        r.run("remote_chat", json!({"pane": "Ben", "before": 12}))
            .await,
        "cursor",
    );
    r.err(
        r.run("remote_chat", json!({"pane": "ghost"})).await,
        "No pane \"ghost\"",
    );
}

/* What the pane's mod reports before the transcript has it. */

fn color() -> Question {
    question(
        "Color",
        "Which color?",
        &[("Red", "Warm"), ("Blue", "Cold")],
        false,
    )
}

fn started(r: &Rig, id: &str, tool: &str, input: Value, at: u64) {
    r.note(
        "w1",
        BridgeEvent::Tool(ToolPhase::Start {
            tool_use_id: id.into(),
            tool: tool.into(),
            input,
        }),
        at,
    );
}

fn ended(r: &Rig, id: &str) {
    r.note(
        "w1",
        BridgeEvent::Tool(ToolPhase::End {
            tool_use_id: id.into(),
        }),
        0,
    );
}

fn dialog(r: &Rig, waiting: bool, tool: Option<(&str, Value)>) {
    let (tool, input) = match tool {
        Some((tool, input)) => (Some(tool.to_string()), Some(input)),
        None => (None, None),
    };
    r.note(
        "w1",
        BridgeEvent::Permission {
            waiting,
            tool,
            input,
        },
        0,
    );
}

#[tokio::test]
async fn chat_gives_the_question_an_ask_user_question_call_waits_on_until_the_call_ends() {
    let r = rig(vec![chat_shop()], vec![], true);
    reads(&r, Some(page()));
    let shown = json!({"questions": [{"question": "Which color?", "header": "Color", "multiSelect": false,
        "options": [{"label": "Red", "description": "Warm", "preview": "<b>x</b>"},
                    {"label": "Blue", "description": "Cold", "preview": "<b>x</b>"}]}]});
    started(&r, "toolu_q", "AskUserQuestion", shown.clone(), 5000);
    dialog(&r, true, Some(("AskUserQuestion", shown)));
    r.act("w1", "shop", AgentActivity::Waiting);
    let asking = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    let mut expected = serde_json::to_value(PendingQuestion {
        tool_use_id: "toolu_q".into(),
        questions: vec![color()],
    })
    .unwrap();
    expected["kind"] = json!("question");
    assert_equal(&asking["pending"], expected);
    // The card shows it: no action line for it.
    assert_equal(&asking["items"], items_json());

    ended(&r, "toolu_q");
    dialog(&r, false, None);
    r.act("w1", "shop", AgentActivity::Working);
    assert!(r.ok("remote_chat", json!({"pane": "Ben"})).await["pending"].is_null());
}

#[tokio::test]
async fn chat_gives_the_permission_a_dialog_asks_titled_as_it_would_run_and_the_call_once_it_runs()
{
    let r = rig(vec![chat_shop()], vec![], true);
    reads(&r, Some(page()));
    let input = json!({"command": "pnpm test --run\npnpm build", "description": "Run the tests"});
    started(&r, "toolu_b", "Bash", input.clone(), 5000);
    dialog(&r, true, Some(("Bash", input)));
    r.act("w1", "shop", AgentActivity::Waiting);
    let asking = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert_equal(
        &asking["pending"],
        json!({"kind": "permission", "tool": "Bash", "title": "Run pnpm test --run", "detail": "pnpm test --run\npnpm build"}),
    );
    // Held by the dialog, it has not run yet.
    assert_equal(&asking["items"], items_json());

    dialog(&r, false, None);
    r.act("w1", "shop", AgentActivity::Working);
    let running = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert!(running["pending"].is_null());
    let mut expected = items_json().as_array().unwrap().clone();
    expected.push(json!({"id": "toolu_b", "at": 5000, "kind": "action", "tool": "Bash", "title": "Ran pnpm test --run",
        "detail": "pnpm test --run\npnpm build", "state": "running"}));
    assert_equal(&running["items"], Value::Array(expected));
}

#[tokio::test]
async fn chat_names_an_edit_s_file_in_the_session_s_folder_with_its_diff() {
    let r = rig(vec![chat_shop()], vec![], true);
    reads(&r, Some(page()));
    dialog(
        &r,
        true,
        Some((
            "Edit",
            json!({"file_path": "c:\\wt\\ben\\src\\cart.ts", "old_string": "a\nb", "new_string": "c"}),
        )),
    );
    r.act("w1", "shop", AgentActivity::Waiting);
    let asking = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    assert_equal(
        &asking["pending"],
        json!({"kind": "permission", "tool": "Edit", "title": "Edit src/cart.ts", "detail": "- a\n- b\n+ c"}),
    );
}

#[tokio::test]
async fn chat_lists_the_calls_running_after_the_transcript_s_on_the_newest_page_each_once() {
    let r = rig(vec![chat_shop()], vec![], true);
    reads(&r, Some(page()));
    r.act("w1", "shop", AgentActivity::Working);
    started(&r, "t1", "Bash", json!({"command": "pnpm test"}), 4000);
    started(
        &r,
        "toolu_r",
        "Read",
        json!({"file_path": "C:\\wt\\ben\\src\\x.ts"}),
        6000,
    );
    started(
        &r,
        "toolu_m",
        "mcp__hangar__board_list_tasks",
        json!({}),
        7000,
    );
    let newest = r.ok("remote_chat", json!({"pane": "Ben"})).await;
    let mut expected = items_json().as_array().unwrap().clone();
    expected.push(json!({"id": "toolu_r", "at": 6000, "kind": "action", "tool": "Read", "title": "Read src/x.ts", "state": "running"}));
    expected.push(json!({"id": "toolu_m", "at": 7000, "kind": "action", "tool": "mcp__hangar__board_list_tasks", "title": "Used board_list_tasks", "state": "running"}));
    assert_equal(&newest["items"], Value::Array(expected));
    let older = r
        .ok("remote_chat", json!({"pane": "Ben", "before": "4096"}))
        .await;
    assert_equal(&older["items"], items_json());

    ended(&r, "toolu_r");
    r.note(
        "w1",
        BridgeEvent::Turn(TurnPhase::End {
            reason: "aborted".into(),
            duration_ms: 1,
            answer: String::new(),
        }),
        0,
    );
    assert_equal(
        &r.ok("remote_chat", json!({"pane": "Ben"})).await["items"],
        items_json(),
    );
}

#[tokio::test]
async fn chat_leaves_the_transcript_to_speak_when_the_mod_reported_nothing() {
    let r = rig(vec![chat_shop()], vec![], true);
    r.act("w1", "shop", AgentActivity::Waiting);
    r.note(
        "orch",
        BridgeEvent::Permission {
            waiting: true,
            tool: Some("Bash".into()),
            input: Some(json!({"command": "ls"})),
        },
        0,
    );
    reads(
        &r,
        Some(ChatPage {
            open_call: Some(open_call()),
            ..page()
        }),
    );
    assert_equal(
        &r.ok("remote_chat", json!({"pane": "Ben"})).await["pending"],
        json!({"kind": "permission", "tool": "Bash", "title": "Ran pnpm test", "detail": "pnpm test"}),
    );
}

/* ── A project put to sleep ───────────────────────────────────── */

/// A project put to sleep: none of its panes has a terminal until it opens.
fn billing() -> Workspace {
    workspace(
        "asleepWs",
        "billing-api",
        vec![
            Pane {
                session_id: Some("sess-noe".into()),
                ..claude("noe", "Noé", Some(PaneRole::Orchestrator))
            },
            pane("logs", "logs", None, AgentId::Shell),
        ],
    )
}

fn asleep_rig() -> Rig {
    rig(vec![shop(), billing()], vec![], true)
}

/// What a pane's Claude Code says once its session is up.
fn hello(r: &Rig, pane: &str) {
    r.note(
        pane,
        BridgeEvent::Hello {
            session_id: "sess".into(),
            model: "opus".into(),
            version: "2.1.0".into(),
        },
        0,
    );
}

#[tokio::test]
async fn wake_wakes_a_project_asleep_as_a_click_on_the_pc_does() {
    let r = asleep_rig();
    assert_equal(
        &r.ok("remote_wake", json!({"workspace": "asleepWs"})).await,
        json!({"waking": true}),
    );
    assert_eq!(r.opened(), ["asleepWs"]);
    assert!(r.touched().contains(&"asleepWs".to_string()));
    assert!(!locked(&r.app.asleep).contains("asleepWs"));
}

#[tokio::test]
async fn wake_finds_the_project_by_its_name_too() {
    let r = asleep_rig();
    assert_equal(
        &r.ok("remote_wake", json!({"workspace": "Billing-API"}))
            .await,
        json!({"waking": true}),
    );
    assert_eq!(r.opened(), ["asleepWs"]);
}

#[tokio::test]
async fn wake_leaves_a_project_that_is_awake_alone() {
    let r = asleep_rig();
    assert_equal(
        &r.ok("remote_wake", json!({"workspace": "shop"})).await,
        json!({"waking": false}),
    );
    assert!(r.opened().is_empty());
    assert!(r.touched().is_empty());
}

#[tokio::test]
async fn wake_names_an_unknown_project() {
    let r = asleep_rig();
    r.err(
        r.run("remote_wake", json!({"workspace": "ghost"})).await,
        "No project \"ghost\"",
    );
    r.err(r.run("remote_wake", json!({})).await, "Name the project");
    assert!(r.opened().is_empty());
}

#[tokio::test]
async fn asleep_queues_a_message_to_a_claude_pane_for_its_session_never_typing_it_into_the_shell() {
    let r = asleep_rig();
    assert_equal(
        &r.ok(
            "remote_send",
            json!({"pane": "Noé", "text": "Ship the invoices"}),
        )
        .await,
        json!({"sent": true, "queued": true}),
    );
    // As the person's own: no `from`, so the session reads it as typed by them.
    assert_eq!(r.app.queued(), ["noe:Ship the invoices"]);
    assert!(r.typed().is_empty());
    assert_eq!(r.opened(), ["asleepWs"]);
    assert!(r.touched().contains(&"asleepWs".to_string()));
}

#[tokio::test]
async fn asleep_keeps_queueing_while_claude_resumes_in_its_new_terminal_and_types_once_it_is_up() {
    let r = asleep_rig();
    r.ok("remote_wake", json!({"workspace": "asleepWs"})).await;
    // The terminal is there, the shell runs, Claude Code has not said hello yet.
    assert_equal(
        &r.ok("remote_send", json!({"pane": "Noé", "text": "first"}))
            .await,
        json!({"sent": true, "queued": true}),
    );
    assert_equal(
        &r.ok("remote_manager", json!({"text": "status?"})).await,
        json!({"sent": true}),
    );
    assert_eq!(r.app.queued(), ["noe:first"]);
    assert_eq!(r.typed(), ["paste:pm:status?", r#"write:pm:"\r""#]);

    hello(&r, "noe");
    assert_equal(
        &r.ok("remote_send", json!({"pane": "Noé", "text": "second"}))
            .await,
        json!({"sent": true}),
    );
    assert_eq!(r.app.queued(), ["noe:first"]);
    assert_eq!(
        r.typed()[2..],
        [
            "paste:noe:second".to_string(),
            r#"write:noe:"\r""#.to_string()
        ]
    );
}

#[tokio::test]
async fn asleep_types_into_a_pane_the_mod_reports_up_hello_or_not() {
    let r = asleep_rig();
    r.ok("remote_wake", json!({"workspace": "asleepWs"})).await;
    r.act("noe", "asleepWs", AgentActivity::Idle);
    r.ok("remote_send", json!({"pane": "Noé", "text": "hi"}))
        .await;
    assert!(r.app.queued().is_empty());
    assert_eq!(r.typed(), ["paste:noe:hi", r#"write:noe:"\r""#]);
}

#[tokio::test]
async fn asleep_types_into_a_shell_pane_once_its_terminal_and_its_shell_are_there() {
    let r = asleep_rig();
    // Its terminal mounts a moment after the workspace opens, its shell a
    // moment later still.
    r.app.slow.store(true, Ordering::SeqCst);
    assert_equal(
        &r.ok(
            "remote_send",
            json!({"pane": "logs", "text": "tail app.log"}),
        )
        .await,
        json!({"sent": true}),
    );
    assert_eq!(r.opened(), ["asleepWs"]);
    assert!(r.app.queued().is_empty());
    assert_eq!(
        r.typed(),
        ["paste:logs:tail app.log after 3 naps", r#"write:logs:"\r""#]
    );
}

#[tokio::test]
async fn asleep_refuses_keys_to_a_claude_pane_not_up_yet_and_wakes_it() {
    let r = asleep_rig();
    r.err(
        r.run("remote_send", json!({"pane": "Noé", "keys": ["escape"]}))
            .await,
        "was asleep",
    );
    assert_eq!(r.opened(), ["asleepWs"]);
    r.err(
        r.run("remote_send", json!({"pane": "Noé", "keys": ["enter"]}))
            .await,
        "\"Noé\" is resuming its session",
    );
    r.err(
        r.run(
            "remote_send",
            json!({"pane": "Noé", "text": "draft", "submit": false}),
        )
        .await,
        "Nothing was sent",
    );
    assert!(r.typed().is_empty());
    assert!(r.app.queued().is_empty());
}

#[tokio::test]
async fn asleep_without_the_mod_to_hold_it_wakes_the_project_and_asks_to_send_again() {
    let r = asleep_rig();
    r.app.change(|state| state.settings.claude_bridge = false);
    r.err(
        r.run("remote_send", json!({"pane": "Noé", "text": "hi"}))
            .await,
        "was asleep",
    );
    assert!(r.typed().is_empty());
    assert!(r.app.queued().is_empty());
    assert_eq!(r.opened(), ["asleepWs"]);
}

#[tokio::test]
async fn asleep_delivers_a_question_s_answers_through_the_queue_as_the_person_would_write_them() {
    let r = asleep_rig();
    let asked = PendingQuestion {
        tool_use_id: "toolu_q".into(),
        questions: vec![
            question("Currency", "Which currency?", &[("EUR", "")], false),
            question(
                "Colors",
                "Which colors?",
                &[("Red", ""), ("Blue", "")],
                true,
            ),
        ],
    };
    reads(
        &r,
        Some(ChatPage {
            items: Vec::new(),
            older: false,
            cursor: None,
            question: Some(asked),
            open_call: None,
        }),
    );
    let answers =
        json!([{"selected": ["EUR"]}, {"selected": ["Blue", "Red"], "other": "and green"}]);
    assert_equal(
        &r.ok(
            "remote_answer",
            json!({"pane": "Noé", "toolUseId": "toolu_q", "answers": answers}),
        )
        .await,
        json!({"answered": true, "queued": true}),
    );
    assert_eq!(
        r.app.queued(),
        ["noe:Which currency?\n→ EUR\n\nWhich colors?\n→ Red, Blue, and green"]
    );
    assert!(r.typed().is_empty());
    assert_eq!(r.opened(), ["asleepWs"]);
}

#[tokio::test]
async fn asleep_wakes_the_project_for_a_permission_and_says_nothing_waits_for_one_any_more() {
    let r = asleep_rig();
    r.err(
        r.run("remote_permission", json!({"pane": "Noé", "allow": true}))
            .await,
        "was asleep",
    );
    assert!(r.typed().is_empty());
    assert_eq!(r.opened(), ["asleepWs"]);
}

#[tokio::test]
async fn asleep_shows_a_pane_asleep_with_an_empty_screen_not_as_gone() {
    let r = asleep_rig();
    assert_equal(
        &r.ok("remote_screen", json!({"pane": "Noé"})).await,
        json!({"paneId": "noe", "pane": "Noé", "workspace": "billing-api", "agent": "claude", "activity": null, "text": ""}),
    );
    // Reading is not using it.
    assert!(r.opened().is_empty());
}

/* ── remote_workspace ─────────────────────────────────────────── */

#[tokio::test]
async fn workspace_finds_a_workspace_by_id() {
    let r = rig(vec![shop()], vec![], true);
    assert_equal(
        &r.ok("remote_workspace", json!({"workspace": "shop"})).await,
        json!({"id": "shop", "name": "Shop", "cwd": "C:\\dev\\shop"}),
    );
}

#[tokio::test]
async fn workspace_finds_one_by_folder_either_separator_any_case_with_or_without_a_trailing_one() {
    let r = rig(vec![shop()], vec![], true);
    for folder in [
        "C:\\dev\\shop",
        "c:/DEV/shop",
        "C:/dev/shop/",
        " c:\\dev\\SHOP\\ ",
    ] {
        assert_eq!(
            r.ok("remote_workspace", json!({"folder": folder})).await["id"],
            "shop",
            "{folder}"
        );
    }
}

#[tokio::test]
async fn workspace_refuses_an_unknown_workspace_an_unknown_folder_and_a_request_naming_neither() {
    let r = rig(vec![shop()], vec![], true);
    r.err(
        r.run("remote_workspace", json!({"workspace": "nope"}))
            .await,
        "No workspace with id \"nope\"",
    );
    r.err(
        r.run("remote_workspace", json!({"folder": "C:\\dev\\sho"}))
            .await,
        "No workspace at",
    );
    r.err(
        r.run("remote_workspace", json!({})).await,
        "Name the workspace",
    );
}

#[tokio::test]
async fn workspace_is_answered_as_an_internal_request() {
    let r = rig(vec![shop()], vec![], true);
    let hook = Hook(r.mobile.clone());
    let answer = hook
        .remote(
            r.port.clone(),
            "remote_workspace".into(),
            json!({"workspace": "shop"}),
        )
        .await;
    assert!(answer.is_ok());
}

#[test]
fn workspace_is_not_among_the_calls_a_phone_may_make() {
    // The server's own list, read from its source as the TS test read it.
    let server = include_str!("../../../../hangar-core/src/remote/server.rs");
    let start = server
        .find("pub const CALLS: &[&str] = &[")
        .expect("the CALLS list");
    let list = &server[start..start + server[start..].find("\n];").unwrap()];
    let ops: Vec<&str> = list
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix("\","))
        })
        .collect();
    assert!(ops.len() > 5);
    assert!(!ops.contains(&"workspace"));
    assert!(!ops.contains(&"remote_workspace"));
}

/* ── Internal requests ────────────────────────────────────────── */

#[tokio::test]
async fn internal_answers_remote_ops() {
    let r = rig(vec![shop()], vec![], true);
    let hook = Hook(r.mobile.clone());
    assert!(
        hook.remote(r.port.clone(), "remote_state".into(), json!({}))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn internal_answers_an_error_for_an_unknown_remote_op() {
    let r = rig(vec![shop()], vec![], true);
    let hook = Hook(r.mobile.clone());
    let answer = hook
        .remote(r.port.clone(), "remote_nope".into(), json!({}))
        .await;
    assert_eq!(
        answer,
        Err("Unknown remote request \"remote_nope\".".into())
    );
}

#[test]
fn internal_the_briefing_is_not_this_package_s() {
    // P18's bus hands this package only the `remote_*` ops: the briefing and
    // anything else go elsewhere (control::answer), as each TS listener
    // claimed its own ops by name. What is left to check here is the name.
    assert!(!"orchestrator_briefing".starts_with("remote_"));
}

/* ── remote_stats ─────────────────────────────────────────────── */

const HOUR: i64 = 3_600_000;

/// A Wednesday afternoon, on the PC's clock.
fn now() -> i64 {
    Local
        .with_ymd_and_hms(2026, 10, 28, 15, 0, 0)
        .earliest()
        .unwrap()
        .timestamp_millis()
}

/// 10 a.m., `days` days before today.
fn days_ago(days: i64) -> i64 {
    add_days(start_of_day(now()), -days) + 10 * HOUR
}

fn bucket(session: u32, at: i64, model: i32, edit: impl FnOnce(&mut Bucket)) -> Bucket {
    let mut row = Bucket {
        session,
        at,
        model,
        ..Bucket::default()
    };
    edit(&mut row);
    row
}

const FOLDERS: [&str; 3] = [
    "C:\\dev\\shop",
    "C:\\dev\\shop-worktrees\\feature",
    "C:\\elsewhere\\scratch",
];

fn usage(buckets: Vec<Bucket>, cwds: &[&str]) -> UsageStats {
    UsageStats {
        generated_at: now() - 5_000,
        scan: ScanInfo {
            files: 3,
            parsed: 1,
            reused: 2,
            failed: 0,
            bytes: 1_000,
            ms: 40,
            cached_only: false,
        },
        sources: Vec::new(),
        active_gap_cap_ms: 5 * 60_000,
        sessions: cwds
            .iter()
            .enumerate()
            .map(|(index, cwd)| SessionInfo {
                agent: if index == 2 {
                    Agent::Codex
                } else {
                    Agent::Claude
                },
                id: format!("s{index}"),
                cwd: (*cwd).into(),
                title: format!("Session {index}"),
                first_at: now() - 50 * 24 * HOUR,
                last_at: now(),
            })
            .collect(),
        models: vec![
            ModelInfo {
                id: "claude-opus-5-5".into(),
                priced: true,
            },
            ModelInfo {
                id: "gpt-5.6-sol".into(),
                priced: true,
            },
            ModelInfo {
                id: "local-llama".into(),
                priced: false,
            },
        ],
        buckets,
    }
}

/// Shop's own session today, one in a worktree of it three days ago, Codex
/// in a folder of no workspace ten days ago, and a model without a price
/// forty days ago.
fn rows_of_usage() -> Vec<Bucket> {
    vec![
        bucket(0, days_ago(0), 0, |b| {
            b.input = 1_000;
            b.output = 500;
            b.cache_read = 3_000;
            b.cost = 1.5;
            b.calls = 3;
        }),
        bucket(0, days_ago(0), -1, |b| {
            b.turns = 2;
            b.active_ms = 30 * 60_000;
        }),
        bucket(1, days_ago(3), 0, |b| {
            b.input = 2_000;
            b.output = 1_000;
            b.cost = 3.0;
            b.calls = 2;
        }),
        bucket(1, days_ago(3), -1, |b| {
            b.turns = 1;
            b.active_ms = 60 * 60_000;
        }),
        bucket(2, days_ago(10), 1, |b| {
            b.input = 10_000;
            b.cost = 0.5;
            b.calls = 1;
        }),
        bucket(2, days_ago(10), -1, |b| {
            b.turns = 1;
            b.active_ms = 20 * 60_000;
        }),
        bucket(0, days_ago(40), 2, |b| {
            b.input = 50_000;
            b.calls = 4;
        }),
    ]
}

fn stats_rig(data: UsageStats) -> Rig {
    let r = rig(vec![shop()], vec![], true);
    *locked(&r.fakes.usage) = Some(data);
    *locked(&r.fakes.now) = now();
    *locked(&r.fakes.worktrees) = Box::new(|cwd| {
        Ok(if cwd == "C:\\dev\\shop" {
            vec!["C:/dev/shop".into(), "C:/dev/shop-worktrees/feature".into()]
        } else {
            Vec::new()
        })
    });
    r
}

fn labels(ranking: &Value) -> Vec<String> {
    ranking["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["label"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn stats_sums_up_the_last_7_days_the_statistics_page_s_way() {
    let r = stats_rig(usage(rows_of_usage(), &FOLDERS));
    let answer = r.ok("remote_stats", json!({"period": "7d"})).await;
    let today = start_of_day(now());
    assert_matches(
        &answer,
        json!({
            "period": "7d", "measure": "tokens", "readAt": now() - 5_000, "cachedOnly": false, "granularity": "day",
            "range": {"from": add_days(today, -6), "to": add_days(today, 1), "days": 7},
            "totals": {"tokens": 7_500, "cacheRead": 3_000, "cost": 4.5, "activeMs": 90 * 60_000, "sessions": 2, "turns": 3},
            "previous": {"tokens": 10_000, "cost": 0.5, "sessions": 1, "turns": 1},
            "activeDays": 2,
        }),
    );
    let series = answer["series"].as_array().unwrap();
    assert_eq!(series.len(), 7);
    assert_equal(&series[6], json!({"at": today, "value": 4_500}));
    assert_equal(
        &series[3],
        json!({"at": add_days(today, -3), "value": 3_000}),
    );
    assert_eq!(
        series
            .iter()
            .filter(|point| point["value"].as_f64() == Some(0.0))
            .count(),
        5
    );
    assert_equal(
        &answer["projects"],
        json!({"rows": [{"key": "ws:shop", "label": "Shop", "tokens": 7_500, "cost": 4.5, "activeMs": 90 * 60_000, "sessions": 2, "share": 1}], "other": null}),
    );
    let agents: Vec<(String, u64, u64)> = answer["agents"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["label"].as_str().unwrap().into(),
                row["tokens"].as_u64().unwrap(),
                row["sessions"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(agents, [("Claude Code".to_string(), 7_500, 2)]);
    let models: Vec<(String, String, f64)> = answer["models"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["key"].as_str().unwrap().into(),
                row["label"].as_str().unwrap().into(),
                row["share"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        models,
        [(
            "claude-opus-5-5".to_string(),
            model_label("claude-opus-5-5"),
            1.0
        )]
    );
    // Never the hourly rows themselves.
    assert!(!answer.to_string().contains("costCacheRead"));
}

#[tokio::test]
async fn stats_ranks_and_draws_by_the_measure_asked_for() {
    let r = stats_rig(usage(rows_of_usage(), &FOLDERS));
    let by_tokens = r
        .ok(
            "remote_stats",
            json!({"period": "30d", "measure": "tokens"}),
        )
        .await;
    let by_cost = r
        .ok("remote_stats", json!({"period": "30d", "measure": "cost"}))
        .await;
    let shares = |answer: &Value| -> Vec<(String, f64)> {
        answer["projects"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["label"].as_str().unwrap().into(),
                    row["share"].as_f64().unwrap(),
                )
            })
            .collect()
    };
    let tokens = shares(&by_tokens);
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].0, "scratch");
    assert!((tokens[0].1 - 10_000.0 / 17_500.0).abs() < 1e-9);
    assert_eq!(tokens[1].0, "Shop");
    assert!((tokens[1].1 - 7_500.0 / 17_500.0).abs() < 1e-9);
    let cost = shares(&by_cost);
    assert_eq!(cost[0].0, "Shop");
    assert!((cost[0].1 - 0.9).abs() < 1e-9);
    assert_eq!(cost[1].0, "scratch");
    assert!((cost[1].1 - 0.1).abs() < 1e-9);
    assert_eq!(labels(&by_cost["agents"]), ["Claude Code", "Codex"]);
    assert_eq!(by_cost["series"].as_array().unwrap().len(), 30);
    assert_eq!(by_cost["series"][29]["value"].as_f64(), Some(1.5));
    assert_eq!(by_tokens["series"][29]["value"].as_f64(), Some(4_500.0));
    assert_eq!(by_cost["measure"], "cost");
}

#[tokio::test]
async fn stats_takes_30_days_and_tokens_by_default_90_days_and_all_of_it_when_asked() {
    let r = stats_rig(usage(rows_of_usage(), &FOLDERS));
    let plain = r.ok("remote_stats", json!({})).await;
    assert_eq!(
        (
            plain["period"].clone(),
            plain["measure"].clone(),
            plain["range"]["days"].clone()
        ),
        (json!("30d"), json!("tokens"), json!(30))
    );
    assert_eq!(plain["totals"]["tokens"], 17_500);

    let quarter = r.ok("remote_stats", json!({"period": "90d"})).await;
    assert_eq!(quarter["range"]["days"], 90);
    assert_eq!(quarter["totals"]["tokens"], 67_500);
    assert_matches(&quarter["previous"], json!({"tokens": 0, "sessions": 0}));

    let all = r.ok("remote_stats", json!({"period": "all"})).await;
    assert_matches(
        &all,
        json!({"range": {"from": add_days(start_of_day(now()), -40), "days": 41}, "firstAt": days_ago(40), "previous": null, "unpricedTokens": 50_000}),
    );
    assert_eq!(all["series"].as_array().unwrap().len(), 41);
    let llama = all["models"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["key"] == "local-llama")
        .unwrap()
        .clone();
    assert_matches(&llama, json!({"unpriced": true, "cost": 0}));
}

#[tokio::test]
async fn stats_folds_the_rows_past_the_sixth_into_one() {
    let folders: Vec<String> = (0..8).map(|index| format!("D:\\p{index}")).collect();
    let folder_refs: Vec<&str> = folders.iter().map(String::as_str).collect();
    let data = usage(
        (0..8)
            .map(|index| {
                bucket(index, days_ago(1), 0, |b| {
                    b.input = u64::from(index + 1) * 100
                })
            })
            .collect(),
        &folder_refs,
    );
    let r = stats_rig(data);
    let answer = r.ok("remote_stats", json!({"period": "7d"})).await;
    assert_eq!(labels(&answer["projects"]), ["p7", "p6", "p5", "p4", "p3"]);
    assert_matches(
        &answer["projects"]["other"],
        json!({"count": 3, "tokens": 600, "cost": 0, "activeMs": 0}),
    );
    let share = answer["projects"]["other"]["share"].as_f64().unwrap();
    assert!((share - 600.0 / 3_600.0).abs() < 1e-9);
}

#[tokio::test]
async fn stats_answers_zeros_and_empty_rankings_without_any_data() {
    let r = stats_rig(usage(Vec::new(), &FOLDERS));
    let answer = r.ok("remote_stats", json!({"period": "30d"})).await;
    assert_matches(
        &answer,
        json!({
            "firstAt": null,
            "totals": {"tokens": 0, "cost": 0, "activeMs": 0, "sessions": 0, "turns": 0},
            "previous": {"tokens": 0}, "activeDays": 0,
            "projects": {"rows": [], "other": null}, "agents": {"rows": [], "other": null}, "models": {"rows": [], "other": null},
            "unpricedTokens": 0, "plan": null,
        }),
    );
    let series = answer["series"].as_array().unwrap();
    assert_eq!(series.len(), 30);
    assert!(
        series
            .iter()
            .all(|point| point["value"].as_f64() == Some(0.0))
    );
    let all = r.ok("remote_stats", json!({"period": "all"})).await;
    let today = start_of_day(now());
    assert_equal(
        &all["range"],
        json!({"from": today, "to": add_days(today, 1), "days": 1}),
    );
}

fn usage_report(limits: Vec<RateLimit>) -> BridgeEvent {
    BridgeEvent::Usage {
        context: UsageContext {
            tokens: Some(1),
            window: 200_000,
            percent: Some(0.0),
        },
        rate_limits: limits,
        cost_usd: None,
    }
}

fn limit(kind: &str, percent: f64, resets: Option<&str>) -> RateLimit {
    RateLimit {
        kind: kind.into(),
        percent_used: percent,
        resets_at: resets.map(str::to_string),
    }
}

fn parse(at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(at)
        .unwrap()
        .timestamp_millis()
}

#[tokio::test]
async fn stats_says_nothing_of_the_plan_until_a_session_reported_it_then_its_windows_whole() {
    let r = stats_rig(usage(rows_of_usage(), &FOLDERS));
    assert!(r.ok("remote_stats", json!({})).await["plan"].is_null());
    r.note(
        "orch",
        usage_report(vec![
            limit("five_hour", 34.4, Some("2026-10-28T17:00:00Z")),
            limit("seven_day", 140.0, None),
            limit("seven_day_opus", 12.0, Some("not a date")),
        ]),
        (now() - 60_000) as u64,
    );
    assert_equal(
        &r.ok("remote_stats", json!({})).await["plan"],
        json!({"at": now() - 60_000, "windows": [
            {"kind": "five_hour", "percentUsed": 34, "resetsAt": parse("2026-10-28T17:00:00Z")},
            {"kind": "seven_day", "percentUsed": 100, "resetsAt": null},
            {"kind": "seven_day_opus", "percentUsed": 12, "resetsAt": null},
        ]}),
    );
}

#[tokio::test]
async fn stats_keeps_the_plan_its_sessions_mods_report_one_without_limits_does_not_erase_it() {
    let r = stats_rig(usage(Vec::new(), &FOLDERS));
    r.note(
        "orch",
        usage_report(vec![limit("five_hour", 81.0, Some("2026-10-28T19:30:00Z"))]),
        (now() - 1_000) as u64,
    );
    r.note("w1", usage_report(Vec::new()), (now() - 500) as u64);
    assert_equal(
        &r.ok("remote_stats", json!({})).await["plan"],
        json!({"at": now() - 1_000, "windows": [{"kind": "five_hour", "percentUsed": 81, "resetsAt": parse("2026-10-28T19:30:00Z")}]}),
    );
}

#[tokio::test]
async fn stats_refuses_a_period_or_a_measure_it_does_not_know() {
    let r = stats_rig(usage(Vec::new(), &FOLDERS));
    assert_eq!(
        r.run("remote_stats", json!({"period": "1y"})).await,
        Err("Unknown period \"1y\". One of: 7d, 30d, 90d, all.".into())
    );
    assert_eq!(
        r.run("remote_stats", json!({"measure": "time"})).await,
        Err("Unknown measure \"time\". One of: tokens, cost.".into())
    );
}

#[tokio::test]
async fn stats_places_sessions_by_the_workspace_s_own_folders_when_git_cannot_list_its_worktrees() {
    let r = stats_rig(usage(rows_of_usage(), &FOLDERS));
    *locked(&r.fakes.worktrees) = Box::new(|_| Err("no git".into()));
    let answer = r.ok("remote_stats", json!({"period": "7d"})).await;
    assert_eq!(labels(&answer["projects"]), ["Shop", "feature"]);
}
