//! `src/lib/kitApi.test.ts`, case for case, through a page's host: the
//! envelope of the kit relay, the gate of hangar-kit-host and this module's
//! `TabApi`, as an HTML tab's calls go. A native tab's go through the same
//! gate and the same `TabApi` (its `KitTab` holds the gate).
//!
//! The frame of the TS was told apart by its window object and an opaque
//! origin; a page here is told apart by its window's name and by being a
//! document of its kit (`kit_relay::is_kit_document`), which is what the
//! cases about another frame or an origin hold.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::futures::executor::block_on;
use serde_json::{Value, json};

use super::*;
use crate::kit_tabs::tabs::workspace_kit_tabs;
use crate::kit_tabs::testing::{entry, github, kit, strings, tab};
use crate::kits::model::{KIT_METHODS, KIT_PERMISSIONS};
use crate::state::live::AgentActivity;
use crate::state::types::PaneRole;

const MINUTE: Duration = Duration::from_secs(60);
const WINDOW: &str = "kit-tab-1";

fn pane(id: &str, name: &str, agent: AgentId, role: Option<PaneRole>) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        role,
        ..Pane::default()
    }
}

fn workspace() -> Workspace {
    Workspace {
        id: "ws-1".into(),
        name: "Game".into(),
        cwd: "C:/projects/game".into(),
        panes: vec![
            pane(
                "p-ava",
                "Ava",
                AgentId::Claude,
                Some(PaneRole::Orchestrator),
            ),
            pane("p-ben", "Ben", AgentId::Shell, None),
            pane("p-cleo", "Cleo", AgentId::Claude, None),
            pane("p-sh", "Shell", AgentId::Shell, None),
        ],
        ..Workspace::default()
    }
}

fn activity(pane: &str) -> Option<PaneActivity> {
    let (activity, bridged) = match pane {
        "p-ava" => (AgentActivity::Working, true),
        "p-ben" => (AgentActivity::Idle, false),
        "p-cleo" => (AgentActivity::Idle, false),
        _ => return None,
    };
    Some(PaneActivity {
        workspace_id: "ws-1".into(),
        activity,
        bridged,
    })
}

/// Everything a tab can hold, but what makes `files.read` reach hidden files.
fn reading_only() -> Vec<String> {
    KIT_PERMISSIONS
        .iter()
        .filter(|permission| **permission != "files.write")
        .map(|permission| (*permission).to_owned())
        .collect()
}

fn command_answer() -> Value {
    json!({ "exitCode": 0, "stdout": "{}", "stderr": "", "timedOut": false, "truncated": false })
}

fn binary_answer() -> Value {
    json!({ "base64": "iVBORw==", "mime": "image/png" })
}

fn list_answer() -> Value {
    json!({ "files": [{ "path": "src/a.rs", "size": 3, "modified": 1793478269409u64 }], "truncated": false })
}

/// What a fake dependency answers instead of its usual.
#[derive(Clone)]
enum Answer {
    Refused(KitError),
    /// A refusal without a code, a sentence.
    Said(String),
    /// Never answers.
    Pending,
    Value(Value),
}

/// The world a tab's host sees, which each case changes as it goes.
struct World {
    granted: Vec<String>,
    current: Option<Workspace>,
    commands_accepted: Option<String>,
    params: KitTabParams,
    caller: Option<KitTabCaller>,
    /// What the tab's calls changed of the tab itself.
    changes: Vec<Value>,
    /// The dependencies called, with what.
    calls: Vec<(&'static str, Value)>,
    answers: HashMap<&'static str, Answer>,
}

impl Default for World {
    fn default() -> Self {
        World {
            granted: reading_only(),
            current: Some(workspace()),
            commands_accepted: None,
            params: KitTabParams::new(),
            caller: None,
            changes: Vec::new(),
            calls: Vec::new(),
            answers: HashMap::new(),
        }
    }
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<World>>);

impl Fake {
    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.0.lock().unwrap()
    }

    fn calls_of(&self, name: &str) -> Vec<Value> {
        self.world()
            .calls
            .iter()
            .filter(|(called, _)| *called == name)
            .map(|(_, args)| args.clone())
            .collect()
    }

    fn called(&self, name: &str) -> bool {
        !self.calls_of(name).is_empty()
    }

    fn answer_with(&self, name: &'static str, answer: Answer) {
        self.world().answers.insert(name, answer);
    }

    /// Records the call and answers it: the case's answer, or `usual`.
    fn answer<T: Send + 'static>(
        &self,
        name: &'static str,
        args: Value,
        usual: Value,
        read: fn(Value) -> T,
        refused: fn(Answer) -> T,
    ) -> BoxFuture<T> {
        let answer = {
            let mut world = self.world();
            world.calls.push((name, args));
            world.answers.get(name).cloned()
        };
        match answer {
            None => ready(read(usual)),
            Some(Answer::Value(value)) => ready(read(value)),
            Some(Answer::Pending) => Box::pin(std::future::pending()),
            Some(refusal) => ready(refused(refusal)),
        }
    }
}

fn coded(answer: Answer) -> KitError {
    match answer {
        Answer::Refused(refusal) => refusal,
        Answer::Said(said) => KitError::new(KitError::FAILED, said),
        _ => unreachable!(),
    }
}

fn plain(answer: Answer) -> String {
    match answer {
        Answer::Refused(refusal) => refusal.message,
        Answer::Said(said) => said,
        _ => unreachable!(),
    }
}

impl TabContext for Fake {
    fn kit_id(&self) -> &str {
        "hello-kit"
    }
    fn kit_name(&self) -> String {
        "Hello kit".into()
    }
    fn workspace(&self) -> Option<Workspace> {
        self.world().current.clone()
    }
    fn permissions(&self) -> Vec<String> {
        self.world().granted.clone()
    }
    fn commands_accepted(&self) -> Option<String> {
        self.world().commands_accepted.clone()
    }
    fn params(&self) -> KitTabParams {
        self.world().params.clone()
    }
    fn caller(&self) -> Option<KitTabCaller> {
        self.world().caller.clone()
    }
    fn set_title(&self, title: &str) {
        self.world().changes.push(json!({ "title": title }));
    }
    fn set_badge(&self, badge: Option<KitBadge>) {
        let badge = badge.map(|badge| {
            let state = match badge.state {
                KitBadgeState::Running => "running",
                KitBadgeState::Success => "success",
                KitBadgeState::Failure => "failure",
                KitBadgeState::Neutral => "neutral",
            };
            json!({ "state": state, "text": badge.text })
        });
        self.world().changes.push(json!({ "badge": badge }));
    }
}

impl Deps for Fake {
    fn read_file(&self, root: String, path: String) -> BoxFuture<Result<String, String>> {
        let usual = json!(format!("text of {path}"));
        self.answer(
            "readFile",
            json!([root, path]),
            usual,
            |v| Ok(v.as_str().unwrap_or_default().to_owned()),
            |a| Err(plain(a)),
        )
    }
    fn resolve_file(&self, root: String, path: String) -> BoxFuture<Result<String, String>> {
        let usual = json!(format!("{root}/{path}"));
        self.answer(
            "resolveFile",
            json!([root, path]),
            usual,
            |v| Ok(v.as_str().unwrap_or_default().to_owned()),
            |a| Err(plain(a)),
        )
    }
    fn read_hidden_file(&self, root: String, path: String) -> BoxFuture<Result<String, KitError>> {
        let usual = json!(format!("hidden text of {path}"));
        self.answer(
            "readHiddenFile",
            json!([root, path]),
            usual,
            |v| Ok(v.as_str().unwrap_or_default().to_owned()),
            |a| Err(coded(a)),
        )
    }
    fn read_binary_file(
        &self,
        root: String,
        path: String,
        hidden: bool,
    ) -> BoxFuture<Result<Value, KitError>> {
        self.answer(
            "readBinaryFile",
            json!([root, path, hidden]),
            binary_answer(),
            Ok,
            |a| Err(coded(a)),
        )
    }
    fn list_files(
        &self,
        root: String,
        path: Option<String>,
        pattern: Option<String>,
    ) -> BoxFuture<Result<Value, KitError>> {
        self.answer(
            "listFiles",
            json!([root, path, pattern]),
            list_answer(),
            Ok,
            |a| Err(coded(a)),
        )
    }
    fn write_file(
        &self,
        root: String,
        path: String,
        text: String,
        expected: Option<String>,
    ) -> BoxFuture<Result<(), KitError>> {
        self.answer(
            "writeFile",
            json!([root, path, text, expected]),
            Value::Null,
            |_| Ok(()),
            |a| Err(coded(a)),
        )
    }
    fn write_binary_file(
        &self,
        root: String,
        path: String,
        base64: String,
    ) -> BoxFuture<Result<(), KitError>> {
        self.answer(
            "writeBinaryFile",
            json!([root, path, base64]),
            Value::Null,
            |_| Ok(()),
            |a| Err(coded(a)),
        )
    }
    fn show_file(
        &self,
        ws: Workspace,
        path: String,
        opened_by: String,
    ) -> BoxFuture<Result<(), String>> {
        self.answer(
            "showFile",
            json!([ws.id, path, opened_by]),
            Value::Null,
            |_| Ok(()),
            |a| Err(plain(a)),
        )
    }
    fn enqueue(
        &self,
        pane_id: String,
        text: String,
        from: String,
    ) -> BoxFuture<Result<(), String>> {
        self.answer(
            "enqueue",
            json!([pane_id, text, from]),
            Value::Null,
            |_| Ok(()),
            |a| Err(plain(a)),
        )
    }
    fn activity(&self, pane_id: &str) -> Option<PaneActivity> {
        activity(pane_id)
    }
    fn notify(&self, title: String, body: String) -> BoxFuture<()> {
        self.answer("notify", json!([title, body]), Value::Null, |_| (), |_| ())
    }
    fn read_data(&self, kit: String, path: String) -> BoxFuture<Result<String, KitError>> {
        let usual = json!(format!("data of {path}"));
        self.answer(
            "readData",
            json!([kit, path]),
            usual,
            |v| Ok(v.as_str().unwrap_or_default().to_owned()),
            |a| Err(coded(a)),
        )
    }
    fn write_data(
        &self,
        kit: String,
        path: String,
        text: String,
    ) -> BoxFuture<Result<(), KitError>> {
        self.answer(
            "writeData",
            json!([kit, path, text]),
            Value::Null,
            |_| Ok(()),
            |a| Err(coded(a)),
        )
    }
    fn loopback_fetch(&self, call: LoopbackCall) -> BoxFuture<Result<Value, KitError>> {
        let method = match call.method {
            api::HttpMethod::Get => "GET",
            api::HttpMethod::Post => "POST",
        };
        let args = json!({
            "port": call.port, "path": call.path, "method": method,
            "body": call.body, "timeoutMs": call.timeout_ms,
        });
        self.answer(
            "loopbackFetch",
            args,
            json!({ "status": 200, "text": "{}" }),
            Ok,
            |a| Err(coded(a)),
        )
    }
    fn run_command(&self, call: CommandCall) -> BoxFuture<Result<Value, KitError>> {
        let args = json!({
            "kit": call.kit, "name": call.name, "args": call.args,
            "cwd": call.cwd, "accepted": call.accepted,
        });
        self.answer("runCommand", args, command_answer(), Ok, |a| Err(coded(a)))
    }
    fn ui(&self, call: UiCall) -> BoxFuture<Result<Value, KitError>> {
        let _ = call;
        ready(Ok(Value::Null))
    }
    fn image(&self, _kit: &str, _source: &ImageSource) -> Result<Vec<u8>, KitError> {
        Err(KitError::new(KitError::UNSUPPORTED, "no image"))
    }
    fn workspace_image(
        &self,
        root: String,
        path: String,
        hidden: bool,
    ) -> Result<Vec<u8>, KitError> {
        Ok(format!("{root}|{path}|{hidden}").into_bytes())
    }
}

/// A page's host over the fake world, with a clock of its own.
struct Setup {
    host: PageHost,
    fake: Fake,
    start: Instant,
    clock: Duration,
    next: u32,
    sent: Vec<Value>,
}

fn page() -> String {
    hangar_webview::kit_relay::kit_file_url("hello-kit", "tabs/status/index.html")
}

fn setup() -> Setup {
    setup_in(Fake::default())
}

fn setup_in(fake: Fake) -> Setup {
    let api = Arc::new(TabApi::new(Arc::new(fake.clone()), Arc::new(fake.clone())));
    Setup {
        host: PageHost::new(WINDOW, api),
        fake,
        start: Instant::now(),
        clock: Duration::ZERO,
        next: 0,
        sent: Vec::new(),
    }
}

impl Setup {
    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.fake.world()
    }

    fn grant(&self, permissions: &[&str]) {
        self.world().granted = strings(permissions);
    }

    fn now(&self) -> Instant {
        self.start + self.clock
    }

    /// What the window posted, a message of the page answered at once or once
    /// its call has run.
    fn receive_from(&mut self, window: &str, uri: &str, message: Value) -> Option<Value> {
        let now = self.now();
        let posted = match self.host.receive(window, uri, &message.to_string(), now)? {
            Post::Now(answer) => Some(answer),
            Post::Later(id, future) => {
                let answer = block_on(future);
                self.host.settle(id, answer)
            }
        };
        if let Some(posted) = &posted {
            self.sent.push(posted.clone());
        }
        posted
    }

    fn receive(&mut self, message: Value) -> Option<Value> {
        self.receive_from(WINDOW, &page(), message)
    }

    /// A request from the page, answered.
    fn call(&mut self, method: &str, params: Option<Value>) -> Option<Value> {
        self.next += 1;
        let mut message = json!({ "hangar": 1, "id": format!("r{}", self.next), "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.receive(message)
    }

    fn error_of(&mut self, method: &str, params: Value) -> Option<String> {
        error_of(self.call(method, Some(params)))
    }

    fn result_of(&mut self, method: &str, params: Value) -> Value {
        let answer = self.call(method, Some(params)).expect("an answer");
        assert!(answer.get("error").is_none(), "{answer}");
        answer["result"].clone()
    }

    fn emit(&mut self, name: &str, data: Value) {
        if let Some(event) = self.host.emit(name, data) {
            self.sent.push(event);
        }
    }
}

fn error_of(answer: Option<Value>) -> Option<String> {
    answer?
        .get("error")?
        .get("code")?
        .as_str()
        .map(str::to_owned)
}

fn error_body(answer: Option<Value>) -> Value {
    answer.expect("an answer")["error"].clone()
}

mod what_a_tab_may_ask_about_itself_with_no_permission {
    use super::*;

    fn bare() -> Setup {
        let setup = setup();
        setup.grant(&[]);
        setup
    }

    #[test]
    fn tab_params_answers_what_it_was_opened_with_and_nothing_without_any() {
        let mut s = bare();
        assert_eq!(
            s.call("tab.params", None).unwrap()["result"],
            json!({ "params": {}, "caller": null })
        );
        s.world().params = [("run".to_string(), "4567".to_string())].into();
        assert_eq!(
            s.result_of("tab.params", json!({})),
            json!({ "params": { "run": "4567" }, "caller": null })
        );
    }

    #[test]
    fn tab_params_says_who_asked_from_hangars_record_and_never_from_the_message() {
        let mut s = bare();
        s.world().caller = Some(KitTabCaller { pane: "Ava".into() });
        assert_eq!(
            s.call("tab.params", None).unwrap()["result"],
            json!({ "params": {}, "caller": { "pane": "Ava" } })
        );
        assert_eq!(
            s.result_of(
                "tab.params",
                json!({ "caller": { "pane": "Boss" }, "pane": "Boss" })
            ),
            json!({ "params": {}, "caller": { "pane": "Ava" } })
        );
        s.world().caller = None;
        assert_eq!(
            s.call("tab.params", None).unwrap()["result"],
            json!({ "params": {}, "caller": null })
        );
    }

    #[test]
    fn the_tab_params_event_reaches_its_page_whatever_it_was_granted() {
        let mut s = bare();
        s.emit(KIT_TAB_PARAMS_EVENT, json!({ "params": { "view": "b" } }));
        assert_eq!(
            s.sent,
            vec![
                json!({ "hangar": 1, "event": "tab.params", "data": { "params": { "view": "b" } } })
            ]
        );
        // The others still take their permission.
        s.emit("panes.changed", json!({}));
        assert_eq!(s.sent.len(), 1);
        assert!(api::may_hear("tab.params", &[]));
        assert!(!api::may_hear("panes.changed", &[]));
    }

    #[test]
    fn tab_set_title_takes_a_short_plain_text_trimmed() {
        let mut s = bare();
        let title = "x".repeat(limits::TAB_TITLE_MAX_CHARS);
        assert_eq!(
            s.result_of("tab.setTitle", json!({ "title": title })),
            json!({})
        );
        s.call("tab.setTitle", Some(json!({ "title": "  Run #4567 " })));
        assert_eq!(
            s.world().changes,
            vec![json!({ "title": title }), json!({ "title": "Run #4567" })]
        );
        assert_eq!(limits::TAB_TITLE_MAX_CHARS, 40);
    }

    #[test]
    fn tab_set_title_counts_what_a_person_sees_and_keeps_markup_as_text() {
        let mut s = bare();
        let emoji = "🚀".repeat(limits::TAB_TITLE_MAX_CHARS);
        assert_eq!(s.error_of("tab.setTitle", json!({ "title": emoji })), None);
        assert_eq!(
            s.error_of("tab.setTitle", json!({ "title": "<b>Run</b>" })),
            None
        );
        assert_eq!(
            s.world().changes,
            vec![json!({ "title": emoji }), json!({ "title": "<b>Run</b>" })]
        );
    }

    #[test]
    fn tab_set_title_refuses_what_is_too_long_empty_or_not_plain_text_on_one_line() {
        let mut s = bare();
        for title in [
            json!("x".repeat(limits::TAB_TITLE_MAX_CHARS + 1)),
            json!(""),
            json!("   "),
            json!("two\nlines"),
            json!("a\tb"),
            json!("\u{202e}reversed"),
            json!("zero\u{200b}width"),
            json!(42),
            json!(null),
        ] {
            let answer = error_body(s.call("tab.setTitle", Some(json!({ "title": title }))));
            assert_eq!(answer["code"], "invalid", "{title}");
            assert!(
                answer["message"].as_str().unwrap().starts_with("title "),
                "{title}"
            );
        }
        let left_out = error_body(s.call("tab.setTitle", Some(json!({}))));
        assert!(left_out["message"].as_str().unwrap().starts_with("title "));
        assert_eq!(
            error_of(s.call("tab.setTitle", None)).as_deref(),
            Some("invalid")
        );
        assert!(s.world().changes.is_empty());
    }

    #[test]
    fn tab_set_badge_takes_each_state_with_a_short_text_or_none() {
        let mut s = bare();
        for state in ["running", "success", "failure", "neutral"] {
            assert_eq!(
                s.result_of("tab.setBadge", json!({ "state": state })),
                json!({})
            );
        }
        s.call(
            "tab.setBadge",
            Some(
                json!({ "state": "running", "text": "x".repeat(limits::TAB_BADGE_TEXT_MAX_CHARS) }),
            ),
        );
        s.call(
            "tab.setBadge",
            Some(json!({ "state": "neutral", "text": null })),
        );
        s.call("tab.setBadge", Some(json!({ "state": null })));
        let badge =
            |state: &str, text: Option<&str>| json!({ "badge": { "state": state, "text": text } });
        assert_eq!(
            s.world().changes,
            vec![
                badge("running", None),
                badge("success", None),
                badge("failure", None),
                badge("neutral", None),
                badge("running", Some(&"x".repeat(12))),
                badge("neutral", None),
                json!({ "badge": null }),
            ]
        );
        assert_eq!(limits::TAB_BADGE_TEXT_MAX_CHARS, 12);
    }

    #[test]
    fn tab_set_badge_refuses_a_state_it_does_not_know_and_a_text_past_its_limit() {
        let mut s = bare();
        for params in [
            json!({}),
            json!({ "state": "pending" }),
            json!({ "state": "constructor" }),
            json!({ "state": 1 }),
            json!({ "state": "running", "text": "x".repeat(limits::TAB_BADGE_TEXT_MAX_CHARS + 1) }),
            json!({ "state": "running", "text": "" }),
            json!({ "state": "running", "text": "a\nb" }),
            json!({ "state": "running", "text": 12 }),
            json!("running"),
        ] {
            assert_eq!(
                s.error_of("tab.setBadge", params.clone()).as_deref(),
                Some("invalid"),
                "{params}"
            );
        }
        assert!(s.world().changes.is_empty());
    }

    #[test]
    fn limits_how_often_each_may_redraw_the_strip_and_lets_go_after_a_minute() {
        let mut s = bare();
        for _ in 0..limits::TAB_CHANGES_PER_MINUTE {
            assert_eq!(
                s.error_of("tab.setBadge", json!({ "state": "running" })),
                None
            );
        }
        assert_eq!(
            s.error_of("tab.setBadge", json!({ "state": "running" }))
                .as_deref(),
            Some("rate_limited")
        );
        // Each its own: a badge counting does not stop the title.
        assert_eq!(s.error_of("tab.setTitle", json!({ "title": "Run" })), None);
        s.clock += MINUTE;
        assert_eq!(
            s.error_of("tab.setBadge", json!({ "state": "running" })),
            None
        );
    }

    #[test]
    fn is_not_on_the_list_of_methods_a_permission_gives_and_an_unknown_name_stays_unknown() {
        for method in ["tab.params", "tab.setTitle", "tab.setBadge"] {
            assert!(!KIT_METHODS.iter().any(|(name, _)| *name == method));
        }
        let mut s = bare();
        for method in ["tab.close", "constructor", "__proto__"] {
            assert_eq!(error_of(s.call(method, None)).as_deref(), Some("unknown"));
        }
    }

    #[test]
    fn answers_nothing_from_another_window_about_this_tab_or_any() {
        let mut s = bare();
        let request = json!({ "hangar": 1, "id": "x", "method": "tab.params" });
        assert_eq!(s.receive_from("kit-tab-2", &page(), request), None);
        let request = json!({ "hangar": 1, "id": "y", "method": "tab.setTitle", "params": { "title": "Mine" } });
        assert_eq!(s.receive_from("preview-p1", &page(), request), None);
        assert!(s.sent.is_empty());
        assert!(s.world().changes.is_empty());
    }

    #[test]
    fn does_nothing_once_the_tab_is_closed() {
        let mut s = bare();
        s.host.close();
        let request = json!({ "hangar": 1, "id": "x", "method": "tab.setTitle", "params": { "title": "Late" } });
        assert_eq!(s.receive(request), None);
        s.emit(KIT_TAB_PARAMS_EVENT, json!({ "params": {} }));
        assert!(s.sent.is_empty());
        assert!(s.world().changes.is_empty());
    }

    #[test]
    fn leaves_to_a_page_what_it_has_in_its_own_document() {
        let mut s = bare();
        for method in [
            "ui.toast",
            "ui.confirm",
            "ui.copy",
            "files.pick",
            "ui.pasteImage",
            "canvas.export",
        ] {
            let answer = error_body(s.call(method, Some(json!({ "message": "hi", "text": "x" }))));
            assert_eq!(answer["code"], "unknown", "{method}");
        }
    }
}

/// Each method, with params it accepts and what its fake deps answer.
fn calls() -> Vec<(&'static str, Option<Value>, Value)> {
    vec![
        (
            "workspace.info",
            None,
            json!({ "id": "ws-1", "name": "Game", "folder": "C:/projects/game" }),
        ),
        (
            "panes.list",
            None,
            serde_json::to_value(kit_panes(&workspace(), activity)).unwrap(),
        ),
        (
            "panes.send",
            Some(json!({ "pane": "Ava", "text": "Build the place" })),
            json!({ "queued": true, "pane": "Ava" }),
        ),
        (
            "files.read",
            Some(json!({ "path": "src/main.luau" })),
            json!("text of src/main.luau"),
        ),
        (
            "files.readBinary",
            Some(json!({ "path": "shots/a.png" })),
            binary_answer(),
        ),
        (
            "files.list",
            Some(json!({ "path": "src", "pattern": "*.rs" })),
            list_answer(),
        ),
        (
            "files.write",
            Some(json!({ "path": ".env", "text": "A=1" })),
            json!({}),
        ),
        (
            "files.writeBinary",
            Some(json!({ "path": ".hangar/annotations/1.png", "base64": "iVBORw==" })),
            json!({}),
        ),
        (
            "preview.show",
            Some(json!({ "path": "docs/plan.md" })),
            json!({ "opened": true, "path": "docs/plan.md" }),
        ),
        (
            "data.read",
            Some(json!({ "path": "queue/state.json" })),
            json!({ "text": "data of queue/state.json" }),
        ),
        (
            "data.write",
            Some(json!({ "path": "queue/state.json", "text": "{}" })),
            json!({}),
        ),
        (
            "loopback.fetch",
            Some(json!({ "port": 8765, "path": "/queue" })),
            json!({ "status": 200, "text": "{}" }),
        ),
        (
            "command.run",
            Some(json!({ "name": "ci-status", "args": { "run": "42" } })),
            command_answer(),
        ),
        (
            "notify",
            Some(json!({ "title": "Built", "body": "The place is ready." })),
            Value::Null,
        ),
    ]
}

/// What a tab holds to make this very call.
fn permission_for(method: &str) -> String {
    let needs = KIT_METHODS
        .iter()
        .find(|(name, _)| *name == method)
        .map(|(_, needs)| *needs)
        .unwrap();
    match needs {
        "loopback:<port>" => "loopback:8765".into(),
        "command:<name>" => "command:ci-status".into(),
        needs => needs.into(),
    }
}

const EVERY_DEP: [&str; 14] = [
    "readFile",
    "readHiddenFile",
    "readBinaryFile",
    "listFiles",
    "writeFile",
    "writeBinaryFile",
    "resolveFile",
    "showFile",
    "enqueue",
    "notify",
    "readData",
    "writeData",
    "loopbackFetch",
    "runCommand",
];

mod each_permission_granted_and_refused {
    use super::*;

    #[test]
    fn answers_each_method_with_its_permission_granted() {
        for (method, params, result) in calls() {
            let permission = permission_for(method);
            let mut s = setup();
            s.world().granted = vec![permission.clone()];
            let answer = s.call(method, params).expect("an answer");
            assert_eq!(answer.get("error"), None, "{method} with {permission}");
            assert_eq!(answer["result"], result, "{method}");
        }
    }

    #[test]
    fn answers_each_method_denied_without_its_permission_and_does_nothing() {
        for (method, params, _) in calls() {
            let permission = permission_for(method);
            let mut s = setup();
            s.world().granted = KIT_PERMISSIONS
                .iter()
                .filter(|held| **held != permission)
                .map(|held| (*held).to_owned())
                .collect();
            assert_eq!(
                error_body(s.call(method, params)),
                json!({ "code": "denied", "message": format!("this tab was not granted {permission}") }),
                "{method}"
            );
            for dep in EVERY_DEP {
                assert!(!s.fake.called(dep), "{method} called {dep}");
            }
        }
    }

    #[test]
    fn reads_what_is_granted_at_each_call_a_grant_taken_back_denies_the_next_call() {
        let mut s = setup();
        assert_eq!(error_of(s.call("panes.list", None)), None);
        s.grant(&[]);
        assert_eq!(
            error_of(s.call("panes.list", None)).as_deref(),
            Some("denied")
        );
    }

    #[test]
    fn answers_an_unknown_method_as_such_whatever_is_granted() {
        let mut s = setup();
        assert_eq!(
            s.error_of("files.delete", json!({ "path": "x.txt", "text": "x" }))
                .as_deref(),
            Some("unknown")
        );
        assert_eq!(
            error_of(s.call("constructor", None)).as_deref(),
            Some("unknown")
        );
        assert_eq!(
            error_of(s.call("__proto__", None)).as_deref(),
            Some("unknown")
        );
    }
}

mod only_the_tabs_own_page_is_answered {
    use super::*;

    #[test]
    fn ignores_a_message_from_another_window_without_a_word() {
        let mut s = setup();
        let request = json!({ "hangar": 1, "id": "x", "method": "panes.send", "params": { "pane": "Ava", "text": "hi" } });
        assert_eq!(s.receive_from("kit-tab-9", &page(), request), None);
        let request = json!({ "hangar": 1, "id": "y", "method": "panes.list" });
        assert_eq!(s.receive_from("", &page(), request), None);
        assert!(s.sent.is_empty());
        assert!(!s.fake.called("enqueue"));
    }

    #[test]
    fn ignores_its_own_window_when_the_document_is_not_the_kits_page() {
        let mut s = setup();
        for uri in [
            "http://tauri.localhost/index.html",
            "https://example.com/",
            "",
            "hangar-kit://localhost/other-kit/tabs/a.html",
        ] {
            let request = json!({ "hangar": 1, "id": "x", "method": "panes.list" });
            assert_eq!(s.receive_from(WINDOW, uri, request), None, "{uri}");
        }
        assert!(s.sent.is_empty());
    }

    #[test]
    fn ignores_what_is_not_a_request_of_the_protocol() {
        let mut s = setup();
        for data in [
            json!(null),
            json!("panes.list"),
            json!([1, 2]),
            json!({ "id": "x", "method": "panes.list" }),
            json!({ "hangar": 2, "id": "x", "method": "panes.list" }),
            json!({ "hangar": 1, "method": "panes.list" }),
            json!({ "hangar": 1, "id": "", "method": "panes.list" }),
            json!({ "hangar": 1, "id": "x".repeat(200), "method": "panes.list" }),
            json!({ "hangar": 1, "id": null, "method": "panes.list" }),
            json!({ "hangar": 1, "id": { "nested": true }, "method": "panes.list" }),
        ] {
            assert_eq!(s.receive(data.clone()), None, "{data}");
        }
        assert!(s.sent.is_empty());
    }

    #[test]
    fn answers_a_request_without_a_method_name_as_invalid() {
        let mut s = setup();
        s.receive(json!({ "hangar": 1, "id": 7, "method": 42 }));
        assert_eq!(
            s.sent,
            vec![
                json!({ "hangar": 1, "id": 7, "error": { "code": "invalid", "message": "method must be a string" } })
            ]
        );
    }

    #[test]
    fn stops_answering_once_closed_a_call_in_flight_included() {
        let mut s = setup();
        let request = json!({ "hangar": 1, "id": "slow", "method": "files.read", "params": { "path": "a.txt" } });
        let Some(Post::Later(id, _)) =
            s.host
                .receive(WINDOW, &page(), &request.to_string(), Instant::now())
        else {
            panic!("a call to run")
        };
        s.host.close();
        assert_eq!(s.host.settle(id, Ok(json!("late"))), None);
        assert_eq!(s.call("panes.list", None), None);
        s.emit("panes.changed", json!({}));
        assert!(s.sent.is_empty());
    }
}

mod params_are_checked_before_anything_is_done {
    use super::*;

    #[test]
    fn refuses_params_that_are_not_an_object() {
        let mut s = setup();
        for params in [
            None,
            Some(json!(null)),
            Some(json!("Ava")),
            Some(json!(["Ava", "hi"])),
            Some(json!(3)),
        ] {
            for method in ["panes.send", "files.read", "notify"] {
                assert_eq!(
                    error_of(s.call(method, params.clone())).as_deref(),
                    Some("invalid"),
                    "{method} {params:?}"
                );
            }
        }
        assert!(!s.fake.called("enqueue"));
        assert!(!s.fake.called("readFile"));
    }

    pub(super) fn outside_paths() -> Vec<Value> {
        vec![
            json!(""),
            json!("   "),
            json!(42),
            json!(null),
            json!("/etc/passwd"),
            json!("\\\\server\\share\\x.txt"),
            json!("C:/Windows/win.ini"),
            json!("C:\\Windows\\win.ini"),
            json!("../secret.txt"),
            json!("src/../../secret.txt"),
            json!("src\\..\\..\\secret.txt"),
            json!("README.md:stream"),
            json!("README.md."),
            json!("a\u{0}b"),
            json!("."),
            json!("x/".repeat(600)),
        ]
    }

    #[test]
    fn files_read_and_preview_show_take_a_relative_path_inside_the_workspace_folder_only() {
        let mut s = setup();
        let mut paths = outside_paths();
        paths.extend([
            json!(".env"),
            json!(".git/config"),
            json!("src/.secrets/key.txt"),
        ]);
        for path in paths {
            assert_eq!(
                s.error_of("files.read", json!({ "path": path })).as_deref(),
                Some("invalid"),
                "{path}"
            );
            assert_eq!(
                s.error_of("preview.show", json!({ "path": path }))
                    .as_deref(),
                Some("invalid"),
                "{path}"
            );
        }
        assert_eq!(
            s.error_of("files.read", json!({})).as_deref(),
            Some("invalid")
        );
        assert!(!s.fake.called("readFile"));
        assert!(!s.fake.called("resolveFile"));
        assert!(!s.fake.called("showFile"));
    }

    #[test]
    fn hands_rust_the_path_relative_to_the_workspace_folder_normalized() {
        let mut s = setup();
        assert_eq!(
            s.result_of("files.read", json!({ "path": "src\\levels/./one.json" })),
            json!("text of src/levels/one.json")
        );
        assert_eq!(
            s.fake.calls_of("readFile"),
            vec![json!(["C:/projects/game", "src/levels/one.json"])]
        );
    }

    #[test]
    fn says_what_rust_refused_a_file_for() {
        let mut s = setup();
        s.fake.answer_with(
            "readFile",
            Answer::Said("big.log is larger than 1 MB".into()),
        );
        s.fake.answer_with(
            "resolveFile",
            Answer::Said("missing.png was not found in the workspace folder".into()),
        );
        assert_eq!(
            error_body(s.call("files.read", Some(json!({ "path": "big.log" })))),
            json!({ "code": "failed", "message": "big.log is larger than 1 MB" })
        );
        assert_eq!(
            error_body(s.call("preview.show", Some(json!({ "path": "missing.png" })))),
            json!({ "code": "failed", "message": "missing.png was not found in the workspace folder" })
        );
    }

    #[test]
    fn opens_the_file_rust_resolved_under_the_kits_name() {
        let mut s = setup();
        s.call("preview.show", Some(json!({ "path": "art/logo.png" })));
        assert_eq!(
            s.fake.calls_of("showFile"),
            vec![json!([
                "ws-1",
                "C:/projects/game/art/logo.png",
                "Hello kit"
            ])]
        );
    }

    #[test]
    fn panes_send_needs_a_pane_and_a_text_within_bounds() {
        let mut s = setup();
        for params in [
            json!({ "text": "hi" }),
            json!({ "pane": "Ava" }),
            json!({ "pane": "Ava", "text": "   " }),
            json!({ "pane": "Ava", "text": 3 }),
            json!({ "pane": "Ava", "text": "x".repeat(limits::SEND_MAX_CHARS + 1) }),
        ] {
            assert_eq!(s.error_of("panes.send", params).as_deref(), Some("invalid"));
        }
        assert!(!s.fake.called("enqueue"));
    }

    #[test]
    fn notify_needs_a_title_and_says_it_comes_from_the_kit() {
        let mut s = setup();
        for params in [
            json!({ "body": "x" }),
            json!({ "title": "x".repeat(121) }),
            json!({ "title": "Hi", "body": 5 }),
        ] {
            assert_eq!(s.error_of("notify", params).as_deref(), Some("invalid"));
        }
        assert_eq!(
            s.result_of("notify", json!({ "title": "Build done" })),
            Value::Null
        );
        assert_eq!(
            s.fake.calls_of("notify"),
            vec![json!(["Hello kit · Build done", ""])]
        );
    }
}

mod hidden_files {
    use super::*;

    const HIDDEN: [&str; 4] = [
        ".env",
        ".env.local",
        ".github/workflows/ci.yml",
        "src/.secrets/key.txt",
    ];
    const GIT: [&str; 6] = [
        ".git",
        ".git/config",
        ".GIT/config",
        "sub/.git/hooks/pre-commit",
        ".git\\config",
        "./.git/config",
    ];

    #[test]
    fn with_files_read_alone_refuses_them_as_it_always_has() {
        let mut s = setup();
        s.grant(&["files.read"]);
        for path in HIDDEN {
            assert_eq!(
                s.error_of("files.read", json!({ "path": path })).as_deref(),
                Some("invalid")
            );
            assert_eq!(
                s.error_of("files.readBinary", json!({ "path": format!("{path}.png") }))
                    .as_deref(),
                Some("invalid")
            );
        }
        for dep in ["readFile", "readHiddenFile", "readBinaryFile"] {
            assert!(!s.fake.called(dep));
        }
    }

    #[test]
    fn with_files_read_alone_reads_through_the_same_door_and_tells_rust_hidden_files_stay_out() {
        let mut s = setup();
        s.grant(&["files.read"]);
        assert_eq!(
            s.result_of("files.read", json!({ "path": "README.md" })),
            json!("text of README.md")
        );
        assert_eq!(
            s.fake.calls_of("readFile"),
            vec![json!(["C:/projects/game", "README.md"])]
        );
        assert!(!s.fake.called("readHiddenFile"));
        s.call("files.readBinary", Some(json!({ "path": "shots\\a.png" })));
        assert_eq!(
            s.fake.calls_of("readBinaryFile"),
            vec![json!(["C:/projects/game", "shots/a.png", false])]
        );
    }

    #[test]
    fn with_files_read_alone_still_lists_them() {
        let mut s = setup();
        s.grant(&["files.read"]);
        assert_eq!(
            s.result_of("files.list", json!({ "path": ".github" })),
            list_answer()
        );
        assert_eq!(
            s.fake.calls_of("listFiles"),
            vec![json!(["C:/projects/game", ".github", null])]
        );
    }

    #[test]
    fn with_both_reads_them_through_the_door_that_may() {
        let mut s = setup();
        s.grant(&["files.read", "files.write"]);
        for path in HIDDEN {
            assert_eq!(
                s.result_of("files.read", json!({ "path": path })),
                json!(format!("hidden text of {path}"))
            );
            assert_eq!(
                s.fake.calls_of("readHiddenFile").last(),
                Some(&json!(["C:/projects/game", path]))
            );
        }
        assert!(!s.fake.called("readFile"));
        s.call("files.readBinary", Some(json!({ "path": ".shots/a.png" })));
        assert_eq!(
            s.fake.calls_of("readBinaryFile"),
            vec![json!(["C:/projects/game", ".shots/a.png", true])]
        );
    }

    #[test]
    fn with_both_never_reaches_git_whatever_else_it_may() {
        let mut s = setup();
        s.grant(&["files.read", "files.write"]);
        for path in GIT {
            assert_eq!(
                s.error_of("files.read", json!({ "path": path })).as_deref(),
                Some("invalid"),
                "{path}"
            );
            assert_eq!(
                s.error_of(
                    "files.readBinary",
                    json!({ "path": format!("{path}/a.png") })
                )
                .as_deref(),
                Some("invalid")
            );
            assert_eq!(
                s.error_of("files.write", json!({ "path": path, "text": "x" }))
                    .as_deref(),
                Some("invalid")
            );
            assert_eq!(
                s.error_of("files.list", json!({ "path": path })).as_deref(),
                Some("invalid")
            );
        }
        for dep in ["readHiddenFile", "readBinaryFile", "writeFile", "listFiles"] {
            assert!(!s.fake.called(dep));
        }
    }

    #[test]
    fn with_both_hands_on_what_rust_says_of_a_missing_file() {
        let mut s = setup();
        s.grant(&["files.read", "files.write"]);
        s.fake.answer_with(
            "readHiddenFile",
            Answer::Refused(KitError::new(
                "not_found",
                ".env was not found in the workspace folder",
            )),
        );
        assert_eq!(
            error_body(s.call("files.read", Some(json!({ "path": ".env" })))),
            json!({ "code": "not_found", "message": ".env was not found in the workspace folder" })
        );
    }

    #[test]
    fn with_both_stops_the_next_call_a_grant_is_taken_back() {
        let mut s = setup();
        s.grant(&["files.read", "files.write"]);
        assert_eq!(
            s.result_of("files.read", json!({ "path": ".env" })),
            json!("hidden text of .env")
        );
        s.grant(&["files.read"]);
        assert_eq!(
            s.error_of("files.read", json!({ "path": ".env" }))
                .as_deref(),
            Some("invalid")
        );
        assert_eq!(s.fake.calls_of("readHiddenFile").len(), 1);
    }

    #[test]
    fn with_both_does_not_change_what_preview_show_opens() {
        let mut s = setup();
        s.grant(&["files.read", "files.write", "preview.show"]);
        assert_eq!(
            s.error_of("preview.show", json!({ "path": ".env" }))
                .as_deref(),
            Some("invalid")
        );
        assert!(!s.fake.called("showFile"));
    }

    #[test]
    fn files_write_alone_reads_nothing() {
        let mut s = setup();
        s.grant(&["files.write"]);
        assert_eq!(
            s.error_of("files.read", json!({ "path": ".env" }))
                .as_deref(),
            Some("denied")
        );
        assert_eq!(
            s.error_of("files.readBinary", json!({ "path": "a.png" }))
                .as_deref(),
            Some("denied")
        );
        assert_eq!(
            error_of(s.call("files.list", None)).as_deref(),
            Some("denied")
        );
        assert!(!s.fake.called("readHiddenFile"));
    }
}

mod files_write {
    use super::*;

    fn writer() -> Setup {
        let s = setup();
        s.grant(&["files.write"]);
        s
    }

    #[test]
    fn hands_rust_the_normalized_path_the_text_and_what_the_page_last_read() {
        let mut s = writer();
        assert_eq!(
            s.result_of(
                "files.write",
                json!({ "path": "config\\./app.toml", "text": "a=1\r\n" })
            ),
            json!({})
        );
        assert_eq!(
            s.fake.calls_of("writeFile").last(),
            Some(&json!([
                "C:/projects/game",
                "config/app.toml",
                "a=1\r\n",
                null
            ]))
        );
        s.call(
            "files.write",
            Some(json!({ "path": ".env", "text": "", "expected": "A=1\n" })),
        );
        assert_eq!(
            s.fake.calls_of("writeFile").last(),
            Some(&json!(["C:/projects/game", ".env", "", "A=1\n"]))
        );
        // An empty text is what the page may have read.
        s.call(
            "files.write",
            Some(json!({ "path": ".env", "text": "x", "expected": "" })),
        );
        assert_eq!(
            s.fake.calls_of("writeFile").last(),
            Some(&json!(["C:/projects/game", ".env", "x", ""]))
        );
    }

    #[test]
    fn refuses_what_leaves_the_folder_before_anything_is_done() {
        let mut s = writer();
        for path in super::params_are_checked_before_anything_is_done::outside_paths() {
            assert_eq!(
                s.error_of("files.write", json!({ "path": path, "text": "x" }))
                    .as_deref(),
                Some("invalid"),
                "{path}"
            );
        }
        assert!(!s.fake.called("writeFile"));
    }

    #[test]
    fn takes_a_text_of_a_megabyte_at_most_and_what_was_read_the_same() {
        let mut s = writer();
        let at_cap = "a".repeat(limits::FILE_MAX_BYTES);
        assert_eq!(
            s.error_of("files.write", json!({ "path": "a.txt", "text": at_cap })),
            None
        );
        assert_eq!(
            s.error_of(
                "files.write",
                json!({ "path": "a.txt", "text": format!("{at_cap}a") })
            )
            .as_deref(),
            Some("invalid")
        );
        // In bytes, not in characters.
        assert_eq!(
            s.error_of(
                "files.write",
                json!({ "path": "a.txt", "text": "é".repeat(limits::FILE_MAX_BYTES / 2 + 1) })
            )
            .as_deref(),
            Some("invalid")
        );
        assert_eq!(
            s.error_of(
                "files.write",
                json!({ "path": "a.txt", "text": "x", "expected": format!("{at_cap}a") })
            )
            .as_deref(),
            Some("invalid")
        );
        assert_eq!(s.fake.calls_of("writeFile").len(), 1);
    }

    #[test]
    fn needs_a_text_and_an_expected_that_is_one() {
        let mut s = writer();
        for params in [
            json!({ "path": "a.txt" }),
            json!({ "path": "a.txt", "text": 3 }),
            json!({ "path": "a.txt", "text": null }),
            json!({ "path": "a.txt", "text": "x", "expected": 3 }),
            json!({ "path": "a.txt", "text": "x", "expected": null }),
        ] {
            assert_eq!(
                s.error_of("files.write", params.clone()).as_deref(),
                Some("invalid"),
                "{params}"
            );
        }
        assert!(!s.fake.called("writeFile"));
    }

    #[test]
    fn says_so_when_the_file_changed_since() {
        let mut s = writer();
        s.fake.answer_with(
            "writeFile",
            Answer::Refused(KitError::new(
                "conflict",
                ".env changed since it was read: nothing was written",
            )),
        );
        assert_eq!(
            error_body(s.call(
                "files.write",
                Some(json!({ "path": ".env", "text": "x", "expected": "y" }))
            )),
            json!({ "code": "conflict", "message": ".env changed since it was read: nothing was written" })
        );
    }

    #[test]
    fn writes_at_most_ten_a_minute_apart_from_data_writes() {
        let mut s = setup();
        s.grant(&["files.write", "data.write"]);
        for i in 0..limits::WRITES_PER_MINUTE {
            assert_eq!(
                s.error_of(
                    "files.write",
                    json!({ "path": "a.txt", "text": i.to_string() })
                ),
                None
            );
        }
        assert_eq!(
            s.error_of("files.write", json!({ "path": "a.txt", "text": "over" }))
                .as_deref(),
            Some("rate_limited")
        );
        assert_eq!(
            s.fake.calls_of("writeFile").len(),
            limits::WRITES_PER_MINUTE
        );
        assert_eq!(
            s.error_of("data.write", json!({ "path": "a.txt", "text": "x" })),
            None
        );
        s.clock += MINUTE;
        assert_eq!(
            s.error_of("files.write", json!({ "path": "a.txt", "text": "later" })),
            None
        );
    }

    #[test]
    fn does_not_spend_the_quota_on_calls_that_fail_their_checks() {
        let mut s = writer();
        for _ in 0..limits::WRITES_PER_MINUTE * 2 {
            s.call("files.write", Some(json!({ "path": "../x", "text": "x" })));
        }
        assert_eq!(
            s.error_of("files.write", json!({ "path": "a.txt", "text": "x" })),
            None
        );
    }

    #[test]
    fn answers_unavailable_once_the_workspace_is_gone() {
        let mut s = writer();
        s.world().current = None;
        assert_eq!(
            s.error_of("files.write", json!({ "path": "a.txt", "text": "x" }))
                .as_deref(),
            Some("unavailable")
        );
        assert!(!s.fake.called("writeFile"));
    }
}

mod files_write_binary {
    use super::*;

    fn writer() -> Setup {
        let s = setup();
        s.grant(&["files.write"]);
        s
    }

    #[test]
    fn hands_rust_the_normalized_path_and_the_base64_and_nothing_else() {
        let mut s = writer();
        assert_eq!(
            s.result_of(
                "files.writeBinary",
                json!({ "path": ".hangar\\annotations/./1.png", "base64": "iVBORw==" })
            ),
            json!({})
        );
        assert_eq!(
            s.fake.calls_of("writeBinaryFile"),
            vec![json!([
                "C:/projects/game",
                ".hangar/annotations/1.png",
                "iVBORw=="
            ])]
        );
    }

    #[test]
    fn refuses_a_path_that_is_not_the_projects_or_goes_under_git() {
        let mut s = writer();
        for path in [
            json!("../x.png"),
            json!("/abs.png"),
            json!("C:/x.png"),
            json!(".git/x.png"),
            json!("a/.GIT/x.png"),
            json!("a:b.png"),
            json!(""),
            json!(null),
            json!(3),
        ] {
            assert_eq!(
                s.error_of(
                    "files.writeBinary",
                    json!({ "path": path, "base64": "iVBORw==" })
                )
                .as_deref(),
                Some("invalid"),
                "{path}"
            );
        }
        assert!(!s.fake.called("writeBinaryFile"));
    }

    #[test]
    fn refuses_a_base64_that_is_not_a_string_or_too_long_for_ten_mb() {
        let mut s = writer();
        for base64 in [json!(null), json!(3), json!(["a"])] {
            assert_eq!(
                s.error_of(
                    "files.writeBinary",
                    json!({ "path": "a.png", "base64": base64 })
                )
                .as_deref(),
                Some("invalid")
            );
        }
        assert_eq!(
            s.error_of("files.writeBinary", json!({ "path": "a.png" }))
                .as_deref(),
            Some("invalid")
        );
        let at_cap = "A".repeat(limits::BINARY_MAX_BYTES.div_ceil(3) * 4);
        assert_eq!(
            s.error_of(
                "files.writeBinary",
                json!({ "path": "a.png", "base64": format!("{at_cap}A") })
            )
            .as_deref(),
            Some("invalid")
        );
        assert!(!s.fake.called("writeBinaryFile"));
        assert_eq!(
            s.error_of(
                "files.writeBinary",
                json!({ "path": "a.png", "base64": at_cap })
            ),
            None
        );
    }

    #[test]
    fn says_conflict_when_the_file_exists_and_invalid_when_rust_says_the_content_is_no_image() {
        let mut s = writer();
        s.fake.answer_with(
            "writeBinaryFile",
            Answer::Refused(KitError::new(
                "conflict",
                "a.png already exists: nothing was written",
            )),
        );
        assert_eq!(
            error_body(s.call(
                "files.writeBinary",
                Some(json!({ "path": "a.png", "base64": "iVBORw==" }))
            )),
            json!({ "code": "conflict", "message": "a.png already exists: nothing was written" })
        );
        s.fake.answer_with(
            "writeBinaryFile",
            Answer::Refused(KitError::new("invalid", "the content is not a png image")),
        );
        assert_eq!(
            s.error_of(
                "files.writeBinary",
                json!({ "path": "a.png", "base64": "AAAA" })
            )
            .as_deref(),
            Some("invalid")
        );
    }

    #[test]
    fn shares_files_writes_quota_and_a_failed_check_spends_none() {
        let mut s = writer();
        for _ in 0..limits::WRITES_PER_MINUTE * 2 {
            s.call(
                "files.writeBinary",
                Some(json!({ "path": "../x.png", "base64": "AAAA" })),
            );
        }
        for i in 0..limits::WRITES_PER_MINUTE - 1 {
            assert_eq!(
                s.error_of(
                    "files.write",
                    json!({ "path": "a.txt", "text": i.to_string() })
                ),
                None
            );
        }
        assert_eq!(
            s.error_of(
                "files.writeBinary",
                json!({ "path": "a.png", "base64": "iVBORw==" })
            ),
            None
        );
        assert_eq!(
            s.error_of(
                "files.writeBinary",
                json!({ "path": "b.png", "base64": "iVBORw==" })
            )
            .as_deref(),
            Some("rate_limited")
        );
        assert_eq!(
            s.error_of("files.write", json!({ "path": "a.txt", "text": "over" }))
                .as_deref(),
            Some("rate_limited")
        );
        assert_eq!(s.fake.calls_of("writeBinaryFile").len(), 1);
    }

    #[test]
    fn answers_unavailable_once_the_workspace_is_gone() {
        let mut s = writer();
        s.world().current = None;
        assert_eq!(
            s.error_of(
                "files.writeBinary",
                json!({ "path": "a.png", "base64": "iVBORw==" })
            )
            .as_deref(),
            Some("unavailable")
        );
        assert!(!s.fake.called("writeBinaryFile"));
    }
}

mod files_read_binary {
    use super::*;

    fn reader() -> Setup {
        let s = setup();
        s.grant(&["files.read"]);
        s
    }

    #[test]
    fn answers_what_rust_read() {
        let mut s = reader();
        assert_eq!(
            s.result_of("files.readBinary", json!({ "path": "shots/a.png" })),
            binary_answer()
        );
        assert_eq!(
            s.fake.calls_of("readBinaryFile"),
            vec![json!(["C:/projects/game", "shots/a.png", false])]
        );
    }

    #[test]
    fn refuses_a_path_that_leaves_the_folder() {
        let mut s = reader();
        for path in [
            json!(""),
            json!(42),
            json!(null),
            json!("/x.png"),
            json!("C:/x.png"),
            json!("../x.png"),
            json!("a/../../x.png"),
            json!("x.png:s"),
            json!(".hidden.png"),
        ] {
            assert_eq!(
                s.error_of("files.readBinary", json!({ "path": path }))
                    .as_deref(),
                Some("invalid"),
                "{path}"
            );
        }
        assert_eq!(
            error_of(s.call("files.readBinary", None)).as_deref(),
            Some("invalid")
        );
        assert!(!s.fake.called("readBinaryFile"));
    }

    #[test]
    fn hands_on_a_type_rust_does_not_read_by_its_code() {
        let mut s = reader();
        s.fake.answer_with(
            "readBinaryFile",
            Answer::Refused(KitError::new(
                "unsupported",
                "a.txt is not an image or a PDF",
            )),
        );
        assert_eq!(
            error_body(s.call("files.readBinary", Some(json!({ "path": "a.txt" })))),
            json!({ "code": "unsupported", "message": "a.txt is not an image or a PDF" })
        );
    }
}

mod files_list {
    use super::*;

    fn reader() -> Setup {
        let s = setup();
        s.grant(&["files.read"]);
        s
    }

    #[test]
    fn lists_the_workspace_folder_when_given_no_folder() {
        let mut s = reader();
        for params in [
            None,
            Some(json!({})),
            Some(json!({ "path": "." })),
            Some(json!({ "path": "./" })),
            Some(json!({ "path": ".\\" })),
        ] {
            assert_eq!(
                s.call("files.list", params.clone()).unwrap()["result"],
                list_answer(),
                "{params:?}"
            );
            assert_eq!(
                s.fake.calls_of("listFiles").last(),
                Some(&json!(["C:/projects/game", null, null]))
            );
        }
    }

    #[test]
    fn hands_rust_a_normalized_folder_and_the_pattern() {
        let mut s = reader();
        s.call(
            "files.list",
            Some(json!({ "path": "config\\deep/", "pattern": "*.json" })),
        );
        assert_eq!(
            s.fake.calls_of("listFiles").last(),
            Some(&json!(["C:/projects/game", "config/deep", "*.json"]))
        );
        s.call("files.list", Some(json!({ "pattern": ".env*" })));
        assert_eq!(
            s.fake.calls_of("listFiles").last(),
            Some(&json!(["C:/projects/game", null, ".env*"]))
        );
    }

    #[test]
    fn refuses_a_folder_that_leaves_the_workspace_and_a_pattern_that_is_not_one() {
        let mut s = reader();
        for path in [
            json!(""),
            json!("   "),
            json!(42),
            json!(null),
            json!("/etc"),
            json!("C:/Windows"),
            json!(".."),
            json!("../x"),
            json!("./.."),
            json!("a/../.."),
            json!("a:b"),
        ] {
            assert_eq!(
                s.error_of("files.list", json!({ "path": path })).as_deref(),
                Some("invalid"),
                "{path}"
            );
        }
        for pattern in [
            json!(""),
            json!("   "),
            json!(42),
            json!(null),
            json!("*".repeat(limits::LIST_PATTERN_MAX_CHARS + 1)),
        ] {
            assert_eq!(
                s.error_of("files.list", json!({ "pattern": pattern }))
                    .as_deref(),
                Some("invalid"),
                "{pattern}"
            );
        }
        assert_eq!(
            s.error_of("files.list", json!("src")).as_deref(),
            Some("invalid")
        );
        assert!(!s.fake.called("listFiles"));
    }

    #[test]
    fn answers_unavailable_once_the_workspace_is_gone() {
        let mut s = reader();
        s.world().current = None;
        assert_eq!(
            error_of(s.call("files.list", None)).as_deref(),
            Some("unavailable")
        );
    }
}

mod panes_send_reaches_the_workspaces_claude_code_agents_only {
    use super::*;

    #[test]
    fn queues_the_text_for_the_agent_labelled_as_the_kits() {
        let mut s = setup();
        assert_eq!(
            s.result_of(
                "panes.send",
                json!({ "pane": "ava", "text": "  Build the place\n" })
            ),
            json!({ "queued": true, "pane": "Ava" })
        );
        assert_eq!(
            s.fake.calls_of("enqueue"),
            vec![json!(["p-ava", "  Build the place\n", "kit:hello-kit"])]
        );
        assert_eq!(kit_sender("hello-kit"), "kit:hello-kit");
    }

    #[test]
    fn refuses_a_shell_a_pane_of_another_workspace_and_an_agent_without_the_bridge() {
        let mut s = setup();
        let code = |s: &mut Setup, pane: &str| {
            s.error_of("panes.send", json!({ "pane": pane, "text": "hi" }))
        };
        assert_eq!(code(&mut s, "Ben").as_deref(), Some("denied"));
        assert_eq!(code(&mut s, "Shell").as_deref(), Some("denied"));
        assert_eq!(code(&mut s, "Dan").as_deref(), Some("not_found"));
        assert_eq!(code(&mut s, "p-ava").as_deref(), Some("not_found"));
        assert_eq!(code(&mut s, "Cleo").as_deref(), Some("unavailable"));
        assert!(!s.fake.called("enqueue"));
    }

    #[test]
    fn says_so_when_the_queue_would_not_take_the_text() {
        let mut s = setup();
        s.fake
            .answer_with("enqueue", Answer::Said("Ava's pane was closed".into()));
        let answer = s.call(
            "panes.send",
            Some(json!({ "pane": "Ava", "text": "Build the place" })),
        );
        let error = error_body(answer);
        assert_eq!(error["code"], "failed");
        assert_eq!(error["message"], "Ava's pane was closed");
        // It was tried: the call counted against the minute, as it did in the
        // TypeScript, whose `await deps.enqueue` threw after the rate was taken.
        assert_eq!(s.fake.calls_of("enqueue").len(), 1);
    }

    #[test]
    fn sends_at_most_ten_a_minute() {
        let mut s = setup();
        for i in 0..limits::WRITES_PER_MINUTE {
            assert_eq!(
                s.error_of(
                    "panes.send",
                    json!({ "pane": "Ava", "text": format!("n{i}") })
                ),
                None
            );
        }
        assert_eq!(
            s.error_of("panes.send", json!({ "pane": "Ava", "text": "one more" }))
                .as_deref(),
            Some("rate_limited")
        );
        assert_eq!(s.fake.calls_of("enqueue").len(), limits::WRITES_PER_MINUTE);
        // A refused call does not count: the window frees up on time.
        s.clock += MINUTE;
        assert_eq!(
            s.error_of("panes.send", json!({ "pane": "Ava", "text": "later" })),
            None
        );
    }

    #[test]
    fn does_not_spend_the_quota_on_calls_that_fail_their_checks() {
        let mut s = setup();
        for _ in 0..limits::WRITES_PER_MINUTE * 2 {
            s.call("panes.send", Some(json!({ "pane": "Ben", "text": "hi" })));
        }
        assert_eq!(
            s.error_of("panes.send", json!({ "pane": "Ava", "text": "hi" })),
            None
        );
    }

    #[test]
    fn raises_at_most_five_notifications_a_minute() {
        let mut s = setup();
        for i in 0..limits::NOTIFY_PER_MINUTE {
            s.call("notify", Some(json!({ "title": format!("n{i}") })));
        }
        assert_eq!(
            s.error_of("notify", json!({ "title": "one more" }))
                .as_deref(),
            Some("rate_limited")
        );
        assert_eq!(s.fake.calls_of("notify").len(), limits::NOTIFY_PER_MINUTE);
    }
}

mod what_a_tab_sees {
    use super::*;

    #[test]
    fn lists_the_panes_in_order_with_what_panes_send_can_reach() {
        assert_eq!(
            serde_json::to_value(kit_panes(&workspace(), activity)).unwrap(),
            json!([
                { "name": "Ava", "agent": "claude", "role": "orchestrator", "state": "working", "canSend": true },
                { "name": "Ben", "agent": "shell", "role": null, "state": "idle", "canSend": false },
                { "name": "Cleo", "agent": "claude", "role": null, "state": "idle", "canSend": false },
                { "name": "Shell", "agent": "shell", "role": null, "state": null, "canSend": false },
            ])
        );
    }

    #[test]
    fn says_the_workspace_is_gone_rather_than_answering_for_another() {
        let mut s = setup();
        s.world().current = None;
        assert_eq!(
            error_of(s.call("workspace.info", None)).as_deref(),
            Some("unavailable")
        );
        assert_eq!(
            error_of(s.call("panes.list", None)).as_deref(),
            Some("unavailable")
        );
    }

    #[test]
    fn tells_panes_changed_only_to_a_tab_granted_panes_read() {
        let mut s = setup();
        s.emit("panes.changed", json!({}));
        assert_eq!(
            s.sent,
            vec![json!({ "hangar": 1, "event": "panes.changed", "data": {} })]
        );
        s.grant(&["workspace.read"]);
        s.emit("panes.changed", json!({}));
        assert_eq!(s.sent.len(), 1);
    }

    #[test]
    fn answers_at_most_sixteen_calls_at_once() {
        let mut s = setup();
        s.fake.answer_with("readFile", Answer::Pending);
        let mut answered = Vec::new();
        for i in 0..=limits::MAX_PENDING {
            let request = json!({ "hangar": 1, "id": format!("f{i}"), "method": "files.read", "params": { "path": "a.txt" } });
            if let Some(Post::Now(answer)) =
                s.host
                    .receive(WINDOW, &page(), &request.to_string(), Instant::now())
            {
                answered.push(answer);
            }
        }
        assert_eq!(
            answered,
            vec![json!({
                "hangar": 1,
                "id": format!("f{}", limits::MAX_PENDING),
                "error": { "code": "rate_limited", "message": "at most 16 calls at once" },
            })]
        );
    }

    #[test]
    fn answers_with_the_requests_own_id_a_number_included() {
        let mut s = setup();
        s.receive(json!({ "hangar": 1, "id": 12, "method": "workspace.info" }));
        assert_eq!(
            s.sent[0],
            json!({ "hangar": 1, "id": 12, "result": { "id": "ws-1", "name": "Game", "folder": "C:/projects/game" } })
        );
    }
}

mod a_kits_own_data {
    use super::*;

    #[test]
    fn reads_and_writes_under_the_hosts_own_record_of_the_tab_whatever_the_message_says() {
        let mut s = setup();
        s.grant(&["data.read", "data.write"]);
        s.call(
            "data.read",
            Some(json!({ "path": "a.txt", "kit": "other-kit", "kitId": "other-kit" })),
        );
        assert_eq!(
            s.fake.calls_of("readData"),
            vec![json!(["hello-kit", "a.txt"])]
        );
        s.call(
            "data.write",
            Some(json!({ "path": "a.txt", "text": "x", "kit": "other-kit", "kitId": "other-kit" })),
        );
        assert_eq!(
            s.fake.calls_of("writeData"),
            vec![json!(["hello-kit", "a.txt", "x"])]
        );
    }

    #[test]
    fn holds_each_permission_apart() {
        let mut s = setup();
        s.grant(&["data.read"]);
        assert_eq!(
            s.error_of("data.write", json!({ "path": "a.txt", "text": "x" }))
                .as_deref(),
            Some("denied")
        );
        assert!(!s.fake.called("writeData"));
        s.grant(&["data.write"]);
        assert_eq!(
            s.error_of("data.read", json!({ "path": "a.txt" }))
                .as_deref(),
            Some("denied")
        );
        assert!(!s.fake.called("readData"));
    }

    #[test]
    fn takes_a_relative_path_inside_the_kits_folder_only() {
        let mut s = setup();
        s.grant(&["data.read", "data.write"]);
        let mut paths = super::params_are_checked_before_anything_is_done::outside_paths();
        paths.extend([
            json!("../other-kit/state.json"),
            json!("a/../../other-kit/state.json"),
            json!("a\\..\\..\\other-kit\\state.json"),
        ]);
        for path in paths {
            assert_eq!(
                s.error_of("data.read", json!({ "path": path })).as_deref(),
                Some("invalid"),
                "{path}"
            );
            assert_eq!(
                s.error_of("data.write", json!({ "path": path, "text": "x" }))
                    .as_deref(),
                Some("invalid"),
                "{path}"
            );
        }
        assert!(!s.fake.called("readData"));
        assert!(!s.fake.called("writeData"));
        for params in [
            None,
            Some(json!(null)),
            Some(json!("a.txt")),
            Some(json!(["a.txt"])),
            Some(json!(3)),
        ] {
            assert_eq!(
                error_of(s.call("data.read", params.clone())).as_deref(),
                Some("invalid")
            );
            assert_eq!(
                error_of(s.call("data.write", params)).as_deref(),
                Some("invalid")
            );
        }
    }

    #[test]
    fn hands_rust_the_path_normalized_and_lets_a_kit_keep_hidden_names_in_its_own_folder() {
        let mut s = setup();
        s.grant(&["data.read"]);
        s.call(
            "data.read",
            Some(json!({ "path": "queue\\today/./log.txt" })),
        );
        assert_eq!(
            s.fake.calls_of("readData"),
            vec![json!(["hello-kit", "queue/today/log.txt"])]
        );
        s.call("data.read", Some(json!({ "path": ".cache/state" })));
        assert_eq!(
            s.fake.calls_of("readData").last(),
            Some(&json!(["hello-kit", ".cache/state"]))
        );
    }

    #[test]
    fn writes_a_text_of_a_megabyte_at_most_counted_in_bytes() {
        let mut s = setup();
        s.grant(&["data.write"]);
        assert_eq!(
            s.error_of("data.write", json!({ "path": "a.txt" }))
                .as_deref(),
            Some("invalid")
        );
        assert_eq!(
            s.error_of("data.write", json!({ "path": "a.txt", "text": 3 }))
                .as_deref(),
            Some("invalid")
        );
        let over = error_body(s.call(
            "data.write",
            Some(json!({ "path": "a.txt", "text": "x".repeat(limits::DATA_MAX_BYTES + 1) })),
        ));
        assert_eq!(over["code"], "invalid");
        assert!(over["message"].as_str().unwrap().contains("1 MB"));
        // Fits by characters, not by bytes: é is two.
        assert_eq!(
            s.error_of(
                "data.write",
                json!({ "path": "a.txt", "text": "é".repeat(limits::DATA_MAX_BYTES / 2 + 1) })
            )
            .as_deref(),
            Some("invalid")
        );
        assert!(!s.fake.called("writeData"));
        assert_eq!(
            s.error_of(
                "data.write",
                json!({ "path": "a.txt", "text": "x".repeat(limits::DATA_MAX_BYTES) })
            ),
            None
        );
        assert_eq!(
            s.error_of("data.write", json!({ "path": "empty.txt", "text": "" })),
            None
        );
    }

    #[test]
    fn keeps_rusts_code_absent_is_not_found_and_a_refused_path_says_why() {
        let mut s = setup();
        s.grant(&["data.read", "data.write"]);
        s.fake.answer_with(
            "readData",
            Answer::Refused(KitError::new(
                "not_found",
                "a.txt is not in this kit's data",
            )),
        );
        s.fake.answer_with(
            "writeData",
            Answer::Refused(KitError::new(
                "failed",
                "away/a.txt leads outside the kit's data folder",
            )),
        );
        assert_eq!(
            error_body(s.call("data.read", Some(json!({ "path": "a.txt" })))),
            json!({ "code": "not_found", "message": "a.txt is not in this kit's data" })
        );
        assert_eq!(
            error_body(s.call(
                "data.write",
                Some(json!({ "path": "away/a.txt", "text": "x" }))
            )),
            json!({ "code": "failed", "message": "away/a.txt leads outside the kit's data folder" })
        );
    }

    #[test]
    fn says_failed_for_what_comes_without_a_code() {
        let mut s = setup();
        s.grant(&["data.read"]);
        s.fake
            .answer_with("readData", Answer::Said("the app is closing".into()));
        assert_eq!(
            error_body(s.call("data.read", Some(json!({ "path": "a.txt" })))),
            json!({ "code": "failed", "message": "the app is closing" })
        );
    }

    #[test]
    fn writes_at_most_ten_a_minute_and_reads_as_often_as_asked() {
        let mut s = setup();
        s.grant(&["data.read", "data.write"]);
        for i in 0..limits::WRITES_PER_MINUTE {
            assert_eq!(
                s.error_of(
                    "data.write",
                    json!({ "path": "a.txt", "text": i.to_string() })
                ),
                None
            );
        }
        assert_eq!(
            s.error_of("data.write", json!({ "path": "a.txt", "text": "more" }))
                .as_deref(),
            Some("rate_limited")
        );
        assert_eq!(
            s.fake.calls_of("writeData").len(),
            limits::WRITES_PER_MINUTE
        );
        // A bad call spends nothing: it is refused for what it is.
        assert_eq!(
            s.error_of("data.write", json!({ "path": "../a.txt", "text": "x" }))
                .as_deref(),
            Some("invalid")
        );
        // Reading has no such limit, here with the write limit reached and no
        // time gone by at all.
        for _ in 0..limits::WRITES_PER_MINUTE * 2 {
            assert_eq!(s.error_of("data.read", json!({ "path": "a.txt" })), None);
        }
        s.clock += MINUTE;
        assert_eq!(
            s.error_of("data.write", json!({ "path": "a.txt", "text": "later" })),
            None
        );
    }

    #[test]
    fn does_not_answer_a_foreign_window_nor_spend_anything_for_it() {
        let mut s = setup();
        s.grant(&["data.read", "data.write", "loopback:8765"]);
        for (method, params) in [
            ("data.read", json!({ "path": "a.txt" })),
            ("data.write", json!({ "path": "a.txt", "text": "x" })),
            ("loopback.fetch", json!({ "port": 8765, "path": "/" })),
        ] {
            let request = json!({ "hangar": 1, "id": "x", "method": method, "params": params });
            assert_eq!(s.receive_from("kit-other", &page(), request), None);
        }
        assert!(s.sent.is_empty());
        for dep in ["readData", "writeData", "loopbackFetch"] {
            assert!(!s.fake.called(dep));
        }
    }
}

mod loopback_fetch {
    use super::*;

    fn fetcher() -> Setup {
        let s = setup();
        s.grant(&["loopback:8765"]);
        s
    }

    #[test]
    fn asks_rust_for_a_get_by_default_with_the_default_timeout() {
        let mut s = fetcher();
        assert_eq!(
            s.result_of(
                "loopback.fetch",
                json!({ "port": 8765, "path": "/queue?since=1" })
            ),
            json!({ "status": 200, "text": "{}" })
        );
        assert_eq!(
            s.fake.calls_of("loopbackFetch"),
            vec![
                json!({ "port": 8765, "path": "/queue?since=1", "method": "GET", "body": null, "timeoutMs": limits::LOOPBACK_TIMEOUT_MS })
            ]
        );
    }

    #[test]
    fn carries_a_posts_body_and_timeout_and_nothing_else_of_the_pages() {
        let mut s = fetcher();
        s.call(
            "loopback.fetch",
            Some(json!({
                "port": 8765, "path": "/run", "method": "POST", "body": "{\"go\":true}",
                "timeoutMs": 15_000, "headers": { "Cookie": "a=b", "Host": "example.com" },
                "host": "example.com", "url": "http://example.com/",
            })),
        );
        assert_eq!(
            s.fake.calls_of("loopbackFetch"),
            vec![
                json!({ "port": 8765, "path": "/run", "method": "POST", "body": "{\"go\":true}", "timeoutMs": 15_000 })
            ]
        );
    }

    #[test]
    fn refuses_a_port_not_granted_even_with_another_loopback_granted() {
        let mut s = fetcher();
        s.grant(&["loopback:8765", "loopback:9000"]);
        assert_eq!(
            s.error_of("loopback.fetch", json!({ "port": 9000, "path": "/" })),
            None
        );
        assert_eq!(
            error_body(s.call("loopback.fetch", Some(json!({ "port": 8766, "path": "/" })))),
            json!({ "code": "denied", "message": "this tab was not granted loopback:8766" })
        );
        assert_eq!(
            s.error_of("loopback.fetch", json!({ "port": 80, "path": "/" }))
                .as_deref(),
            Some("invalid")
        );
        assert_eq!(s.fake.calls_of("loopbackFetch").len(), 1);
    }

    #[test]
    fn reads_the_port_from_the_permission_as_written() {
        let mut s = fetcher();
        s.grant(&[
            "data.read",
            "panes.send",
            "notify",
            "loopback:80",
            "loopback:08765",
        ]);
        for port in [8765, 80] {
            assert_eq!(
                s.error_of("loopback.fetch", json!({ "port": port, "path": "/" }))
                    .as_deref(),
                Some("denied")
            );
        }
        assert!(!s.fake.called("loopbackFetch"));
    }

    #[test]
    fn tells_a_tab_with_no_loopback_at_all_only_that_it_was_not_granted_one() {
        let mut s = fetcher();
        s.grant(&["notify"]);
        assert_eq!(
            error_body(s.call(
                "loopback.fetch",
                Some(json!({ "port": "abc", "path": "/" }))
            )),
            json!({ "code": "denied", "message": "this tab was not granted loopback:<port>" })
        );
        assert_eq!(
            error_of(s.call("loopback.fetch", None)).as_deref(),
            Some("denied")
        );
    }

    #[test]
    fn refuses_a_port_that_is_no_port() {
        let mut s = fetcher();
        for port in [
            None,
            Some(json!(null)),
            Some(json!("8765")),
            Some(json!(8765.5)),
            Some(json!(1023)),
            Some(json!(0)),
            Some(json!(-1)),
            Some(json!(65536)),
            Some(json!([8765])),
            Some(json!({})),
        ] {
            let mut params = json!({ "path": "/" });
            if let Some(port) = &port {
                params["port"] = port.clone();
            }
            assert_eq!(
                s.error_of("loopback.fetch", params).as_deref(),
                Some("invalid"),
                "{port:?}"
            );
        }
        for params in [
            None,
            Some(json!(null)),
            Some(json!("/queue")),
            Some(json!(["/queue"])),
            Some(json!(3)),
        ] {
            assert_eq!(
                error_of(s.call("loopback.fetch", params)).as_deref(),
                Some("invalid")
            );
        }
        assert!(!s.fake.called("loopbackFetch"));
    }

    #[test]
    fn takes_a_path_that_cannot_change_the_host() {
        let mut s = fetcher();
        for path in [
            None,
            Some(json!(3)),
            Some(json!("")),
            Some(json!("queue")),
            Some(json!("//example.com/queue")),
            Some(json!("http://example.com/")),
            Some(json!("/a b")),
            Some(json!("/a\r\nHost: example.com")),
            Some(json!("/a\\b")),
            Some(json!("/\\example.com")),
            Some(json!("/é")),
            Some(json!("/a#b")),
            Some(json!(format!(
                "/{}",
                "a".repeat(limits::LOOPBACK_PATH_MAX_CHARS)
            ))),
        ] {
            let mut params = json!({ "port": 8765 });
            if let Some(path) = &path {
                params["path"] = path.clone();
            }
            assert_eq!(
                s.error_of("loopback.fetch", params).as_deref(),
                Some("invalid"),
                "{path:?}"
            );
        }
        assert!(!s.fake.called("loopbackFetch"));
    }

    #[test]
    fn takes_get_or_post_a_body_on_a_post_only_and_a_timeout_up_to_fifteen_seconds() {
        let mut s = fetcher();
        let at = |s: &mut Setup, patch: Value| {
            let mut params = json!({ "port": 8765, "path": "/" });
            for (key, value) in patch.as_object().unwrap() {
                params[key] = value.clone();
            }
            s.error_of("loopback.fetch", params)
        };
        for method in [
            json!("PUT"),
            json!("DELETE"),
            json!("get"),
            json!(""),
            json!(3),
            json!(null),
        ] {
            assert_eq!(
                at(&mut s, json!({ "method": method })).as_deref(),
                Some("invalid"),
                "{method}"
            );
        }
        assert_eq!(
            at(&mut s, json!({ "body": "x" })).as_deref(),
            Some("invalid")
        );
        assert_eq!(
            at(&mut s, json!({ "method": "GET", "body": "x" })).as_deref(),
            Some("invalid")
        );
        assert_eq!(
            at(&mut s, json!({ "method": "POST", "body": 3 })).as_deref(),
            Some("invalid")
        );
        assert_eq!(
            at(
                &mut s,
                json!({ "method": "POST", "body": "x".repeat(limits::LOOPBACK_MAX_BODY_BYTES + 1) })
            )
            .as_deref(),
            Some("invalid")
        );
        for timeout in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("5000"),
            json!(null),
            json!(limits::LOOPBACK_MAX_TIMEOUT_MS + 1),
        ] {
            assert_eq!(
                at(&mut s, json!({ "timeoutMs": timeout })).as_deref(),
                Some("invalid"),
                "{timeout}"
            );
        }
        assert!(!s.fake.called("loopbackFetch"));
        assert_eq!(
            at(
                &mut s,
                json!({ "timeoutMs": limits::LOOPBACK_MAX_TIMEOUT_MS })
            ),
            None
        );
    }

    #[test]
    fn answers_unreachable_when_nothing_is_there() {
        let mut s = fetcher();
        s.fake.answer_with(
            "loopbackFetch",
            Answer::Refused(KitError::new("unreachable", "nothing answers on port 8765")),
        );
        assert_eq!(
            error_body(s.call("loopback.fetch", Some(json!({ "port": 8765, "path": "/" })))),
            json!({ "code": "unreachable", "message": "nothing answers on port 8765" })
        );
    }

    #[test]
    fn is_not_held_back_when_polled_it_has_no_limit_of_its_own() {
        let mut s = fetcher();
        let polls = limits::WRITES_PER_MINUTE * 2;
        for _ in 0..polls {
            assert_eq!(
                s.error_of("loopback.fetch", json!({ "port": 8765, "path": "/queue" })),
                None
            );
        }
        assert_eq!(s.fake.calls_of("loopbackFetch").len(), polls);
    }
}

mod command_run {
    use super::*;

    const GRANT: &str = "commands:0123456789abcdef";

    fn run() -> Value {
        json!({ "name": "ci-status", "args": { "run": "42", "repo": "o/r" } })
    }

    fn runner() -> Setup {
        let s = setup();
        s.grant(&["command:ci-status"]);
        s
    }

    #[test]
    fn asks_rust_for_that_command_with_the_kit_the_folder_and_the_grant_of_the_hosts_own_record() {
        let mut s = runner();
        s.world().commands_accepted = Some(GRANT.into());
        let mut params = run();
        params["kit"] = json!("another-kit");
        params["cwd"] = json!("C:/elsewhere");
        params["accepted"] = json!("commands:ffffffffffffffff");
        assert_eq!(s.result_of("command.run", params), command_answer());
        assert_eq!(
            s.fake.calls_of("runCommand"),
            vec![json!({
                "kit": "hello-kit", "name": "ci-status", "args": { "run": "42", "repo": "o/r" },
                "cwd": "C:/projects/game", "accepted": GRANT,
            })]
        );
    }

    #[test]
    fn hands_back_whatever_the_program_answered() {
        let mut s = runner();
        let answered = json!({ "exitCode": null, "stdout": "partial", "stderr": "", "timedOut": true, "truncated": true });
        s.fake
            .answer_with("runCommand", Answer::Value(answered.clone()));
        assert_eq!(s.result_of("command.run", run()), answered);
    }

    #[test]
    fn takes_a_command_without_args() {
        let mut s = runner();
        assert_eq!(
            s.error_of("command.run", json!({ "name": "ci-status" })),
            None
        );
        assert_eq!(s.fake.calls_of("runCommand")[0]["args"], json!({}));
    }

    #[test]
    fn does_not_let_another_commands_permission_open_it() {
        let mut s = runner();
        s.grant(&["command:deploy", "workspace.read", "loopback:8765"]);
        assert_eq!(
            error_body(s.call("command.run", Some(run()))),
            json!({ "code": "denied", "message": "this tab was not granted command:ci-status" })
        );
        assert!(!s.fake.called("runCommand"));
    }

    #[test]
    fn does_not_take_the_patterns_own_spelling_or_a_grant_of_the_kits_commands_for_a_permission() {
        for held in [
            "command:<name>",
            GRANT,
            "commands:ci-status",
            "command:ci-status2",
        ] {
            let mut s = setup();
            s.grant(&[held]);
            assert_eq!(s.error_of("command.run", run()).as_deref(), Some("denied"));
            assert!(!s.fake.called("runCommand"));
        }
    }

    #[test]
    fn answers_a_message_from_another_window_or_document_with_nothing_and_runs_nothing() {
        let mut s = runner();
        let request = json!({ "hangar": 1, "id": "x", "method": "command.run", "params": run() });
        assert_eq!(s.receive_from("kit-other", &page(), request.clone()), None);
        assert_eq!(
            s.receive_from(WINDOW, "https://example.com/", request),
            None
        );
        assert!(s.sent.is_empty());
        assert!(!s.fake.called("runCommand"));
    }

    #[test]
    fn refuses_a_name_that_names_no_command_and_tells_a_tab_holding_none_only_that() {
        let mut s = runner();
        for name in [
            None,
            Some(json!(3)),
            Some(json!("")),
            Some(json!("CI")),
            Some(json!("ci status")),
            Some(json!("../x")),
            Some(json!("command:ci-status")),
            Some(json!(null)),
        ] {
            let mut params = json!({ "args": {} });
            if let Some(name) = name {
                params["name"] = name;
            }
            assert_eq!(
                s.error_of("command.run", params).as_deref(),
                Some("invalid")
            );
        }
        assert_eq!(
            error_of(s.call("command.run", None)).as_deref(),
            Some("invalid")
        );
        assert_eq!(
            s.error_of("command.run", json!("ci-status")).as_deref(),
            Some("invalid")
        );
        s.grant(&["panes.read"]);
        assert_eq!(
            error_body(s.call("command.run", Some(json!({ "name": 3 })))),
            json!({ "code": "denied", "message": "this tab was not granted command:<name>" })
        );
        assert!(!s.fake.called("runCommand"));
    }

    #[test]
    fn refuses_args_that_are_not_an_object_and_a_value_that_is_not_a_string() {
        let mut s = runner();
        for args in [
            json!([]),
            json!("run=42"),
            json!(42),
            json!(null),
            json!(["42"]),
        ] {
            assert_eq!(
                s.error_of("command.run", json!({ "name": "ci-status", "args": args }))
                    .as_deref(),
                Some("invalid"),
                "{args}"
            );
        }
        for value in [
            json!(42),
            json!(null),
            json!(true),
            json!({ "a": 1 }),
            json!(["42"]),
        ] {
            assert_eq!(
                error_body(s.call(
                    "command.run",
                    Some(json!({ "name": "ci-status", "args": { "run": value } }))
                )),
                json!({ "code": "bad_args", "message": "run must be a string" })
            );
        }
        assert!(!s.fake.called("runCommand"));
    }

    #[test]
    fn keeps_a_key_called_proto_a_key_for_rust_to_refuse() {
        let mut s = runner();
        s.call(
            "command.run",
            Some(json!({ "name": "ci-status", "args": { "__proto__": "x", "run": "42" } })),
        );
        let sent = &s.fake.calls_of("runCommand")[0]["args"];
        let mut keys: Vec<&String> = sent.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["__proto__", "run"]);
    }

    #[test]
    fn keeps_the_codes_rust_refuses_with() {
        for code in [
            "bad_args",
            "not_found",
            "busy",
            "denied",
            "failed",
            "unavailable",
        ] {
            let mut s = runner();
            s.fake.answer_with(
                "runCommand",
                Answer::Refused(KitError::new(code, format!("because {code}"))),
            );
            assert_eq!(
                error_body(s.call("command.run", Some(run()))),
                json!({ "code": code, "message": format!("because {code}") })
            );
        }
    }

    #[test]
    fn says_so_when_the_workspace_is_gone() {
        let mut s = runner();
        s.world().current = None;
        assert_eq!(
            s.error_of("command.run", run()).as_deref(),
            Some("unavailable")
        );
        assert!(!s.fake.called("runCommand"));
    }

    #[test]
    fn is_not_held_back_when_polled_it_has_no_limit_of_its_own() {
        let mut s = runner();
        let polls = limits::WRITES_PER_MINUTE * 2;
        for _ in 0..polls {
            assert_eq!(s.error_of("command.run", run()), None);
        }
        assert_eq!(s.fake.calls_of("runCommand").len(), polls);
    }

    mod once_a_kits_commands_have_to_be_accepted {
        use super::*;

        const CHANGED: &str = "commands:fedcba9876543210";

        fn kit_from(
            source: hangar_core::kits::Source,
            commands_permission: &str,
        ) -> hangar_core::kits::KitInfo {
            hangar_core::kits::KitInfo {
                source,
                commands_permission: Some(commands_permission.into()),
                ..kit(
                    "hello-kit",
                    vec![tab("ci", "CI", "tabs/ci.html", &["command:ci-status"])],
                )
            }
        }

        /// A call from the tab, holding what `workspace_kit_tabs` says it may
        /// hold.
        fn run_as(kit: hangar_core::kits::KitInfo, held: &[&str]) -> (Option<Value>, Fake) {
            let choices = workspace_kit_tabs(&[entry("hello-kit", held)], Some(&[kit]));
            let choice = &choices[0];
            let mut s = setup();
            s.world().granted = choice.permissions.clone();
            s.world().commands_accepted = choice.commands_accepted.clone();
            let answer = s.call("command.run", Some(run()));
            (answer, s.fake)
        }

        #[test]
        fn answers_denied_for_a_github_kit_whose_commands_changed_until_they_are_accepted_again() {
            let (answer, fake) = run_as(kit_from(github(), CHANGED), &["command:ci-status", GRANT]);
            assert_eq!(
                error_body(answer),
                json!({ "code": "denied", "message": "this tab was not granted command:ci-status" })
            );
            assert!(!fake.called("runCommand"));

            let (again, fake) =
                run_as(kit_from(github(), CHANGED), &["command:ci-status", CHANGED]);
            assert_eq!(error_of(again), None);
            assert_eq!(fake.calls_of("runCommand")[0]["accepted"], CHANGED);
        }

        #[test]
        fn answers_denied_for_a_github_kit_whose_commands_were_never_accepted() {
            let (answer, fake) = run_as(kit_from(github(), GRANT), &["command:ci-status"]);
            assert_eq!(error_of(answer).as_deref(), Some("denied"));
            assert!(!fake.called("runCommand"));
        }

        #[test]
        fn does_not_ask_a_linked_folder_kit_whatever_its_commands_became() {
            let folder = crate::kit_tabs::testing::folder("hello-kit");
            let (answer, fake) = run_as(kit_from(folder, CHANGED), &["command:ci-status"]);
            assert_eq!(error_of(answer), None);
            assert_eq!(fake.calls_of("runCommand")[0]["accepted"], Value::Null);
        }
    }
}

mod hosts_are_bound_to_their_own_window {
    use super::*;

    #[test]
    fn does_not_answer_a_second_tabs_window_from_the_first_tabs_host() {
        let mut first = setup();
        let request = json!({ "hangar": 1, "id": "x", "method": "panes.list" });
        assert_eq!(first.receive_from("kit-tab-2", &page(), request), None);
        assert!(first.sent.is_empty());
    }
}

mod a_native_tabs_calls {
    use super::*;

    /// The images of the workspace a view shows are read from its folder,
    /// as it is now, hidden ones with `files.write` too; the chooser opens
    /// there, and neither once the workspace is gone.
    #[test]
    fn reach_the_workspace_as_it_is_now_for_its_images_and_the_chooser() {
        let fake = Fake::default();
        let api = TabApi::new(Arc::new(fake.clone()), Arc::new(fake.clone()));
        assert_eq!(
            KitApi::image(&api, &ImageSource::File("shots/a.png".into())).unwrap(),
            b"C:/projects/game|shots/a.png|false".to_vec()
        );
        fake.world().granted = vec!["files.read".into(), "files.write".into()];
        assert_eq!(
            KitApi::image(&api, &ImageSource::File(".shots/a.png".into())).unwrap(),
            b"C:/projects/game|.shots/a.png|true".to_vec()
        );
        fake.world().current = None;
        assert_eq!(
            KitApi::image(&api, &ImageSource::File("a.png".into()))
                .unwrap_err()
                .code,
            "unavailable"
        );
        assert_eq!(
            block_on(KitApi::pick_file(&api, &api::FilesPick::default()))
                .unwrap_err()
                .code,
            "unavailable"
        );
    }

    /// The same `TabApi` answers a native tab, the gate being its `KitTab`'s:
    /// here through a gate of its own, with a click behind it.
    #[test]
    fn go_through_the_same_api_and_reach_the_window_for_what_a_page_has_in_its_document() {
        let fake = Fake::default();
        let api = TabApi::new(Arc::new(fake.clone()), Arc::new(fake.clone()));
        let mut gate = Gate::new();
        let call = Call {
            id: 1,
            method: "ui.copy".into(),
            params: json!({ "text": "x" }),
        };
        let admitted = gate
            .admit(&call, &api.granted(), true, Instant::now(), |c| {
                api.check(c)
            })
            .expect("a click lets it through");
        assert_eq!(block_on(api.call(admitted)), Ok(Value::Null));
        let call = Call {
            id: 2,
            method: "panes.send".into(),
            params: json!({ "pane": "Ben", "text": "hi" }),
        };
        assert_eq!(
            gate.admit(&call, &api.granted(), false, Instant::now(), |c| api
                .check(c))
                .unwrap_err()
                .code,
            "denied"
        );
    }
}
