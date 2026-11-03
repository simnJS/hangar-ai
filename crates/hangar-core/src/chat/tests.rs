//! Synthetic transcripts in the shapes Claude Code writes: user and
//! assistant lines with their content blocks, attachments, system lines, and
//! the bookkeeping lines around them that must stay out of the chat.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::thread::Thread;
use super::tools::{self, CLIP_BYTES};
use super::*;

const CWD: &str = "C:\\dev\\shop";

fn at(second: u32) -> String {
    format!("2026-10-28T10:{:02}:{:02}.000Z", second / 60, second % 60)
}

/// A transcript line: the fields every line carries, then its own.
fn line(kind: &str, uuid: &str, second: u32, extra: Value) -> String {
    let mut value = json!({
        "parentUuid": null,
        "isSidechain": false,
        "type": kind,
        "uuid": uuid,
        "timestamp": at(second),
        "cwd": CWD,
        "sessionId": "sess-1",
        "version": "2.9.0",
    });
    let map = value.as_object_mut().unwrap();
    for (key, val) in extra.as_object().unwrap() {
        map.insert(key.clone(), val.clone());
    }
    value.to_string()
}

fn person(uuid: &str, second: u32, text: &str) -> String {
    line(
        "user",
        uuid,
        second,
        json!({
            "message": { "role": "user", "content": text },
            "origin": { "kind": "human" },
            "promptSource": "typed",
        }),
    )
}

fn said(uuid: &str, second: u32, text: &str) -> String {
    assistant(uuid, second, json!([{ "type": "text", "text": text }]))
}

fn assistant(uuid: &str, second: u32, content: Value) -> String {
    line(
        "assistant",
        uuid,
        second,
        json!({
            "message": {
                "id": format!("msg_{uuid}"),
                "model": "claude-opus-5-5",
                "role": "assistant",
                "content": content,
                "usage": { "input_tokens": 1, "output_tokens": 1 },
            },
            "requestId": "req_1",
        }),
    )
}

fn tool_use(uuid: &str, second: u32, id: &str, name: &str, input: Value) -> String {
    assistant(
        uuid,
        second,
        json!([{ "type": "tool_use", "id": id, "name": name, "input": input, "caller": { "type": "direct" } }]),
    )
}

fn tool_result(uuid: &str, second: u32, id: &str, content: Value, is_error: bool) -> String {
    line(
        "user",
        uuid,
        second,
        json!({
            "message": {
                "role": "user",
                "content": [{ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error }],
            },
            "toolUseResult": { "stdout": "", "stderr": "" },
            "sourceToolAssistantUUID": "x",
        }),
    )
}

fn thread_of(lines: &[String]) -> Thread {
    let mut thread = Thread::default();
    let mut offset = 0;
    for text in lines {
        thread.feed(offset, text);
        offset += text.len() as u64 + 1;
    }
    thread
}

fn items_of(lines: &[String]) -> Vec<ChatItem> {
    thread_of(lines).page(None, PAGE_MAX).0
}

fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hangar-chat-{tag}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir.join("session.jsonl")
}

fn write_lines(path: &Path, lines: &[String]) {
    let mut body = lines.join("\n");
    body.push('\n');
    fs::write(path, body).unwrap();
}

fn append(path: &Path, text: &str) {
    let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn read(
    path: &Path,
    cache: &mut HashMap<PathBuf, Transcript>,
    before: Option<u64>,
    limit: usize,
) -> ChatPage {
    page_of(path, cache, before, limit)
        .unwrap()
        .expect("the file exists")
}

fn ids(items: &[ChatItem]) -> Vec<&str> {
    items.iter().map(|item| item.id.as_str()).collect()
}

fn action(item: &ChatItem) -> (&str, &str, Option<&str>, Option<&CallResult>, CallState) {
    match &item.body {
        Body::Action {
            tool,
            title,
            detail,
            result,
            state,
        } => (tool, title, detail.as_deref(), result.as_ref(), *state),
        other => panic!("expected an action, got {other:?}"),
    }
}

// -- Items, one kind at a time ------------------------------------------------

#[test]
fn every_item_kind_serializes_to_the_contract() {
    let lines = [
        person("u1", 1, "Fix the cart"),
        said("a1", 2, "On it."),
        tool_use(
            "a2",
            3,
            "toolu_1",
            "Bash",
            json!({ "command": "pnpm test", "description": "Run tests" }),
        ),
        tool_result("r1", 4, "toolu_1", json!("12 passed"), false),
        tool_use(
            "a3",
            5,
            "toolu_q",
            "AskUserQuestion",
            json!({ "questions": [{
                "question": "Which currency?",
                "header": "Currency",
                "multiSelect": false,
                "options": [
                    { "label": "EUR", "description": "Euro", "preview": "€" },
                    { "label": "USD", "description": "Dollar" },
                ],
            }] }),
        ),
        line(
            "system",
            "s1",
            6,
            json!({ "subtype": "compact_boundary", "level": "info", "content": "Conversation compacted" }),
        ),
    ];
    let items: Vec<Value> = items_of(&lines)
        .iter()
        .map(|item| serde_json::to_value(item).unwrap())
        .collect();
    let second = |s: u32| {
        chrono::DateTime::parse_from_rfc3339(&at(s))
            .unwrap()
            .timestamp_millis()
    };
    assert_eq!(
        items,
        vec![
            json!({ "id": "u1", "at": second(1), "kind": "user", "text": "Fix the cart", "from": "person", "images": 0 }),
            json!({ "id": "a1", "at": second(2), "kind": "assistant", "text": "On it." }),
            json!({
                "id": "toolu_1", "at": second(3), "kind": "action", "tool": "Bash", "title": "Ran pnpm test",
                "detail": "pnpm test", "result": { "ok": true, "text": "12 passed" }, "state": "done",
            }),
            json!({
                "id": "toolu_q", "at": second(5), "kind": "question", "toolUseId": "toolu_q",
                "questions": [{
                    "question": "Which currency?", "header": "Currency", "multiSelect": false,
                    "options": [
                        { "label": "EUR", "description": "Euro" },
                        { "label": "USD", "description": "Dollar" },
                    ],
                }],
            }),
            json!({ "id": "s1", "at": second(6), "kind": "notice", "text": "Conversation compacted" }),
        ]
    );
}

#[test]
fn hangar_messages_are_told_from_the_person_s() {
    let plugin = line(
        "user",
        "u1",
        1,
        json!({
            "message": { "role": "user", "content": "The hangar-bridge plugin sent a message:\n[Hangar] From agent \"Jack\": land it.\n\nThis is how Claude Code surfaces a prompt a plugin submits between turns — it starts this turn in the user's place. Address the message above." },
            "origin": { "kind": "plugin", "name": "hangar-bridge" },
        }),
    );
    let plugin_blocks = line(
        "user",
        "u2",
        2,
        json!({
            "message": { "role": "user", "content": [{ "type": "text", "text": "[Hangar] Briefing — not typed by the person.\nYour task." }] },
            "origin": { "kind": "plugin", "name": "hangar-bridge" },
        }),
    );
    let typed = person("u3", 3, "[Hangar] queued through the prompt");
    let mine = person("u4", 4, "Looks good");
    let items = items_of(&[plugin, plugin_blocks, typed, mine]);
    let users: Vec<(&str, Sender)> = items
        .iter()
        .map(|item| match &item.body {
            Body::User { text, from, .. } => (text.as_str(), *from),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        users,
        vec![
            ("[Hangar] From agent \"Jack\": land it.", Sender::Hangar),
            (
                "[Hangar] Briefing — not typed by the person.\nYour task.",
                Sender::Hangar
            ),
            ("[Hangar] queued through the prompt", Sender::Hangar),
            ("Looks good", Sender::Person),
        ]
    );
}

#[test]
fn a_message_with_pictures_counts_them() {
    let lines = [line(
        "user",
        "u1",
        1,
        json!({
            "message": { "role": "user", "content": [
                { "type": "text", "text": "[Image #1] the blue boxes are off" },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" } },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" } },
            ] },
            "origin": { "kind": "human" },
        }),
    )];
    match &items_of(&lines)[0].body {
        Body::User { text, images, from } => {
            assert_eq!(text, "[Image #1] the blue boxes are off");
            assert_eq!(*images, 2);
            assert_eq!(*from, Sender::Person);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn what_is_not_the_conversation_stays_out() {
    let meta = line(
        "user",
        "m1",
        1,
        json!({ "isMeta": true, "message": { "role": "user", "content": "<local-command-caveat>Caveat</local-command-caveat>" } }),
    );
    let notification = line(
        "user",
        "m2",
        2,
        json!({
            "message": { "role": "user", "content": "<task-notification>\n<task-id>b1</task-id>\n</task-notification>" },
            "origin": { "kind": "task-notification" },
        }),
    );
    let stdout = line(
        "user",
        "m3",
        3,
        json!({ "message": { "role": "user", "content": "<local-command-stdout>Set model</local-command-stdout>" } }),
    );
    let peer = line(
        "user",
        "m4",
        4,
        json!({ "message": { "role": "user", "content": "hello" }, "origin": { "kind": "peer", "from": "x" } }),
    );
    let summary = line(
        "user",
        "m5",
        5,
        json!({ "isCompactSummary": true, "message": { "role": "user", "content": "This session is being continued…" } }),
    );
    let thinking = assistant(
        "m6",
        6,
        json!([{ "type": "thinking", "thinking": "hmm", "signature": "sig" }]),
    );
    let mut sidechain: Value = serde_json::from_str(&said("m7", 7, "subagent step")).unwrap();
    sidechain["isSidechain"] = json!(true);
    let blob = json!({
        "type": "api-request-blob",
        "sessionId": "sess-1",
        "hash": "h",
        "message": { "role": "assistant", "content": [{ "type": "text", "text": "copy of the request" }] },
    })
    .to_string();
    let bookkeeping = [
        json!({ "type": "last-prompt", "lastPrompt": "x", "sessionId": "sess-1" }).to_string(),
        json!({ "type": "queue-operation", "operation": "enqueue", "content": "x", "sessionId": "sess-1" }).to_string(),
        line("system", "m8", 8, json!({ "subtype": "turn_duration", "durationMs": 10 })),
        line("attachment", "m9", 9, json!({ "attachment": { "type": "total_tokens_reminder" } })),
    ];
    let synthetic = line(
        "assistant",
        "m10",
        10,
        json!({ "message": { "model": "<synthetic>", "role": "assistant", "content": [{ "type": "text", "text": "No response requested." }] } }),
    );
    let mut lines = vec![
        meta,
        notification,
        stdout,
        peer,
        summary,
        thinking,
        sidechain.to_string(),
        blob,
        synthetic,
    ];
    lines.extend(bookkeeping);
    lines.push("not json at all".to_string());
    lines.push(said("kept", 11, "Only this"));
    assert_eq!(ids(&items_of(&lines)), vec!["kept"]);
}

#[test]
fn commands_the_person_ran_read_as_typed() {
    let slash = line(
        "user",
        "c1",
        1,
        json!({ "message": { "role": "user", "content": "<command-message>model</command-message>\n<command-name>/model</command-name>\n<command-args>opus</command-args>" } }),
    );
    let bash = line(
        "user",
        "c2",
        2,
        json!({ "message": { "role": "user", "content": "<bash-input>git status</bash-input>" } }),
    );
    let texts: Vec<String> = items_of(&[slash, bash])
        .into_iter()
        .map(|item| match item.body {
            Body::User { text, .. } => text,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(texts, ["/model opus", "! git status"]);
}

#[test]
fn notices_for_compaction_interruption_resumption_and_errors() {
    let interrupted = line(
        "user",
        "n2",
        2,
        json!({ "message": { "role": "user", "content": [{ "type": "text", "text": "[Request interrupted by user for tool use]" }] } }),
    );
    let hook = |uuid: &str, kind: &str| {
        line(
            "attachment",
            uuid,
            3,
            json!({ "attachment": { "type": kind, "hookName": "SessionStart:resume", "hookEvent": "SessionStart" } }),
        )
    };
    let api_error = line(
        "assistant",
        "n4",
        4,
        json!({
            "isApiErrorMessage": true,
            "message": { "model": "<synthetic>", "role": "assistant", "content": [{ "type": "text", "text": "API Error: Connection lost mid-response." }] },
        }),
    );
    let startup = line(
        "attachment",
        "n5",
        5,
        json!({ "attachment": { "type": "hook_success", "hookName": "SessionStart:startup", "hookEvent": "SessionStart" } }),
    );
    let lines = [
        line(
            "system",
            "n1",
            1,
            json!({ "subtype": "compact_boundary", "compactMetadata": { "trigger": "auto" } }),
        ),
        interrupted,
        // Every SessionStart hook leaves its line; one notice says it.
        hook("n3", "hook_success"),
        hook("n3b", "hook_cancelled"),
        api_error,
        startup,
    ];
    let notices: Vec<(String, String)> = items_of(&lines)
        .into_iter()
        .map(|item| match item.body {
            Body::Notice { text } => (item.id, text),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        notices,
        vec![
            ("n1".to_string(), "Conversation compacted".to_string()),
            ("n2".to_string(), "Turn interrupted".to_string()),
            ("n3".to_string(), "Session resumed".to_string()),
            (
                "n4".to_string(),
                "API Error: Connection lost mid-response.".to_string()
            ),
        ]
    );
}

#[test]
fn a_subagent_is_one_action() {
    let lines = [
        tool_use(
            "a1",
            1,
            "toolu_agent",
            "Agent",
            json!({ "description": "Survey the API", "prompt": "Find every route", "subagent_type": "Explore" }),
        ),
        {
            let mut step: Value = serde_json::from_str(&tool_use(
                "s1",
                2,
                "toolu_inner",
                "Read",
                json!({ "file_path": "C:\\dev\\shop\\a.ts" }),
            ))
            .unwrap();
            step["isSidechain"] = json!(true);
            step.to_string()
        },
        tool_result(
            "r1",
            3,
            "toolu_agent",
            json!([{ "type": "text", "text": "Three routes." }]),
            false,
        ),
    ];
    let items = items_of(&lines);
    assert_eq!(items.len(), 1);
    let (tool, title, detail, result, state) = action(&items[0]);
    assert_eq!(
        (tool, title, detail),
        (
            "Agent",
            "Ran agent: Survey the API",
            Some("Find every route")
        )
    );
    assert_eq!(result.unwrap().text, "Three routes.");
    assert_eq!(state, CallState::Done);
}

// -- Tool calls ---------------------------------------------------------------

#[test]
fn calls_read_as_one_line_with_the_input_that_matters() {
    let cases = [
        (
            "Edit",
            json!({ "file_path": "C:\\dev\\shop\\src\\a.ts", "old_string": "let a = 1;", "new_string": "const a = 1;\nexport { a };" }),
            "Edited src/a.ts",
            Some("- let a = 1;\n+ const a = 1;\n+ export { a };"),
        ),
        (
            "Write",
            json!({ "file_path": "c:\\dev\\shop\\notes.md", "content": "# Notes" }),
            "Wrote notes.md",
            Some("# Notes"),
        ),
        (
            "Read",
            json!({ "file_path": "C:\\elsewhere\\x.ts" }),
            "Read C:/elsewhere/x.ts",
            None,
        ),
        (
            "Grep",
            json!({ "pattern": "foo", "path": "C:\\dev\\shop\\src", "glob": "*.ts" }),
            "Searched for foo",
            Some("foo\npath: src\nglob: *.ts"),
        ),
        (
            "PowerShell",
            json!({ "command": "Get-ChildItem\nWrite-Output done" }),
            "Ran Get-ChildItem",
            Some("Get-ChildItem\nWrite-Output done"),
        ),
        (
            "mcp__hangar__project_list",
            json!({}),
            "Used project_list",
            None,
        ),
        (
            "mcp__hangar__board_comment_task",
            json!({ "id": "t1" }),
            "Used board_comment_task",
            Some("{\"id\":\"t1\"}"),
        ),
        (
            "WebSearch",
            json!({ "query": "tauri events" }),
            "Searched the web for tauri events",
            None,
        ),
        (
            "TodoWrite",
            json!({ "todos": [{ "content": "Ship", "status": "completed" }, { "content": "Test", "status": "pending" }] }),
            "Updated the to-do list",
            Some("[x] Ship\n[ ] Test"),
        ),
        (
            "SomethingNew",
            json!({ "x": 1 }),
            "Used SomethingNew",
            Some("{\"x\":1}"),
        ),
    ];
    for (tool, input, title, detail) in cases {
        let described = tools::describe(tool, &input, Some(CWD));
        assert_eq!(described.title, title, "{tool}");
        assert_eq!(described.detail.as_deref(), detail, "{tool}");
    }
}

#[test]
fn results_pair_with_their_calls_in_any_order() {
    let lines = [
        // Two calls in parallel, answered in the other order.
        tool_use(
            "a1",
            1,
            "toolu_1",
            "Bash",
            json!({ "command": "pnpm test" }),
        ),
        tool_use(
            "a2",
            2,
            "toolu_2",
            "Bash",
            json!({ "command": "cargo test" }),
        ),
        tool_result("r2", 3, "toolu_2", json!("error: 1 failed"), true),
        tool_result(
            "r1",
            4,
            "toolu_1",
            json!([{ "type": "text", "text": "ok" }, { "type": "image", "source": {} }]),
            false,
        ),
        // A result twice, or for a call never made: no harm.
        tool_result("r3", 5, "toolu_1", json!("again"), false),
        tool_result("r4", 6, "toolu_ghost", json!("?"), false),
    ];
    let items = items_of(&lines);
    assert_eq!(ids(&items), ["toolu_1", "toolu_2"]);
    let (_, _, _, first, first_state) = action(&items[0]);
    assert_eq!(
        first,
        Some(&CallResult {
            ok: true,
            text: "ok\n[image]".to_string()
        })
    );
    assert_eq!(first_state, CallState::Done);
    let (_, _, _, second, second_state) = action(&items[1]);
    assert_eq!(
        second,
        Some(&CallResult {
            ok: false,
            text: "error: 1 failed".to_string()
        })
    );
    assert_eq!(second_state, CallState::Error);
}

#[test]
fn reads_in_a_row_fold_into_one_line() {
    let read = |uuid: &str, id: &str, file: &str| {
        tool_use(
            uuid,
            1,
            id,
            "Read",
            json!({ "file_path": format!("C:\\dev\\shop\\{file}") }),
        )
    };
    let lines = [
        read("a1", "toolu_r1", "src\\a.ts"),
        read("a2", "toolu_r2", "src\\b.ts"),
        tool_result("r1", 2, "toolu_r1", json!("1\tconst a"), false),
        read("a3", "toolu_r3", "README.md"),
        tool_result("r2", 3, "toolu_r2", json!("1\tconst b"), false),
        said("t1", 4, "Now the test."),
        read("a4", "toolu_r4", "src\\a.test.ts"),
    ];
    let mut thread = thread_of(&lines);
    let items = thread.page(None, 10).0;
    assert_eq!(ids(&items), ["toolu_r1", "t1", "toolu_r4"]);
    let (tool, title, detail, result, state) = action(&items[0]);
    assert_eq!((tool, title), ("Read", "Read 3 files"));
    assert_eq!(detail, Some("src/a.ts\nsrc/b.ts\nREADME.md"));
    assert_eq!(result, None);
    // One of the three still reads.
    assert_eq!(state, CallState::Running);
    let (_, title, _, _, _) = action(&items[2]);
    assert_eq!(title, "Read src/a.test.ts");

    thread.feed(
        9_000,
        &tool_result("r3", 5, "toolu_r3", json!("# Shop"), false),
    );
    let (_, _, _, _, state) = action(&thread.page(None, 10).0[0]);
    assert_eq!(state, CallState::Done);
}

#[test]
fn details_and_results_are_cut_to_a_few_kb() {
    let command = format!("echo {}", "x".repeat(10_000));
    // Two-byte characters: the cut lands on a boundary, never inside one.
    let output = "é".repeat(10_000);
    let lines = [
        tool_use("a1", 1, "toolu_1", "Bash", json!({ "command": command })),
        tool_result("r1", 2, "toolu_1", json!(output), false),
    ];
    let items = items_of(&lines);
    let (_, title, detail, result, _) = action(&items[0]);
    let detail = detail.unwrap();
    assert!(
        detail.len() <= CLIP_BYTES + '…'.len_utf8(),
        "{}",
        detail.len()
    );
    assert!(detail.ends_with('…'));
    assert!(title.chars().count() <= 4 + 80 + 1, "{title}");
    let text = &result.unwrap().text;
    assert!(text.len() <= CLIP_BYTES + '…'.len_utf8());
    assert!(text.trim_end_matches('…').chars().all(|c| c == 'é'));
}

#[test]
fn a_call_left_behind_by_a_new_turn_is_no_longer_running() {
    let lines = [
        tool_use(
            "a1",
            1,
            "toolu_1",
            "Bash",
            json!({ "command": "sleep 100" }),
        ),
        // Typed while Claude worked: handed over mid-turn, nothing ends.
        line(
            "attachment",
            "q1",
            2,
            json!({ "attachment": { "type": "queued_command", "prompt": "also check the logs", "commandMode": "prompt", "origin": { "kind": "human" } } }),
        ),
    ];
    let mut thread = thread_of(&lines);
    let items = thread.page(None, 10).0;
    assert_eq!(ids(&items), ["toolu_1", "q1"]);
    assert_eq!(action(&items[0]).4, CallState::Running);
    assert!(
        matches!(&items[1].body, Body::User { text, from: Sender::Person, .. } if text == "also check the logs")
    );
    assert_eq!(
        thread.open_call().map(|call| call.title),
        Some("Ran sleep 100".to_string())
    );

    // A new prompt: the session ended without the result.
    thread.feed(10_000, &person("u2", 3, "Are you there?"));
    assert_eq!(action(&thread.page(None, 10).0[0]).4, CallState::Error);
    assert_eq!(thread.open_call(), None);
}

// -- Questions ----------------------------------------------------------------

fn ask(uuid: &str, id: &str) -> String {
    tool_use(
        uuid,
        1,
        id,
        "AskUserQuestion",
        json!({ "questions": [
            {
                "question": "Which currency?",
                "header": "Currency",
                "multiSelect": false,
                "options": [{ "label": "EUR", "description": "" }, { "label": "USD", "description": "" }],
            },
            {
                "question": "Which checks?",
                "header": "Checks",
                "multiSelect": true,
                "options": [
                    { "label": "Lint", "description": "" },
                    { "label": "Unit, fast", "description": "" },
                    { "label": "E2E", "description": "" },
                ],
            },
        ] }),
    )
}

fn answered(uuid: &str, id: &str, answers: Value) -> String {
    line(
        "user",
        uuid,
        2,
        json!({
            "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": "User has answered your questions: …" }] },
            "toolUseResult": { "questions": [], "answers": answers, "annotations": {} },
        }),
    )
}

#[test]
fn a_question_waits_until_its_answer_comes() {
    let mut thread = thread_of(&[person("u1", 0, "Set up checkout"), ask("a1", "toolu_q")]);
    let pending = thread.pending_question().expect("pending");
    assert_eq!(pending.tool_use_id, "toolu_q");
    assert_eq!(pending.questions.len(), 2);
    assert!(pending.questions[1].multi_select);
    // A question is not a permission prompt.
    assert_eq!(thread.open_call(), None);
    match &thread.page(None, 10).0[1].body {
        Body::Question { answer, .. } => assert_eq!(answer, &None),
        other => panic!("{other:?}"),
    }

    thread.feed(
        5_000,
        &answered(
            "r1",
            "toolu_q",
            json!({ "Which currency?": "EUR", "Which checks?": "Lint, Unit, fast, nightly only" }),
        ),
    );
    assert_eq!(thread.pending_question(), None);
    match &thread.page(None, 10).0[1].body {
        Body::Question { answer, .. } => assert_eq!(
            answer.as_deref(),
            Some(
                &[
                    Answer {
                        selected: vec!["EUR".to_string()],
                        other: None
                    },
                    Answer {
                        selected: vec!["Lint".to_string(), "Unit, fast".to_string()],
                        other: Some("nightly only".to_string()),
                    },
                ][..]
            )
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_answer_is_read_from_the_text_when_the_line_has_no_structure() {
    let questions = tools::questions(
        &serde_json::from_str::<Value>(&ask("a1", "toolu_q")).unwrap()["message"]["content"][0]
            ["input"],
    );
    let text = "User has answered your questions: \"Which currency?\"=\"USD\", \"Which checks?\"=\"E2E\". You can now continue with the user's answers in mind.";
    let answers = tools::answers(&questions, None, text);
    assert_eq!(answers[0].selected, ["USD"]);
    assert_eq!(answers[1].selected, ["E2E"]);
}

#[test]
fn a_refused_question_is_settled_without_an_answer() {
    let refused = tool_result(
        "r1",
        2,
        "toolu_q",
        json!("The user doesn't want to proceed with this tool use."),
        true,
    );
    let thread = thread_of(&[ask("a1", "toolu_q"), refused]);
    assert_eq!(thread.pending_question(), None);
    match &thread.page(None, 10).0[0].body {
        Body::Question { answer, .. } => assert_eq!(answer, &None),
        other => panic!("{other:?}"),
    }
}

// -- Pages --------------------------------------------------------------------

#[test]
fn pages_go_back_by_cursor_until_the_first_item() {
    let path = temp_path("pages");
    let lines: Vec<String> = (0..25)
        .map(|n| said(&format!("a{n:02}"), n, &format!("message {n}")))
        .collect();
    write_lines(&path, &lines);
    let mut cache = HashMap::new();

    let newest = read(&path, &mut cache, None, 10);
    assert_eq!(ids(&newest.items).first(), Some(&"a15"));
    assert_eq!(ids(&newest.items).last(), Some(&"a24"));
    assert!(newest.older);

    let cursor = newest.cursor.as_ref().unwrap().parse::<u64>().unwrap();
    let middle = read(&path, &mut cache, Some(cursor), 10);
    assert_eq!(
        ids(&middle.items),
        (5..15).map(|n| format!("a{n:02}")).collect::<Vec<_>>()
    );
    assert!(middle.older);

    let cursor = middle.cursor.unwrap().parse::<u64>().unwrap();
    let oldest = read(&path, &mut cache, Some(cursor), 10);
    assert_eq!(ids(&oldest.items), ["a00", "a01", "a02", "a03", "a04"]);
    assert!(!oldest.older);
    assert_eq!(oldest.cursor, None);

    // Polling the newest page while scrolled back changes nothing.
    assert_eq!(read(&path, &mut cache, None, 10), newest);
}

#[test]
fn a_page_never_splits_the_items_of_one_line() {
    let lines = [
        said("a0", 0, "first"),
        assistant(
            "a1",
            1,
            json!([
                { "type": "text", "text": "Running it." },
                { "type": "tool_use", "id": "toolu_1", "name": "Bash", "input": { "command": "ls" } },
            ]),
        ),
    ];
    let thread = thread_of(&lines);
    let (items, cursor, more) = thread.page(None, 1);
    assert_eq!(ids(&items), ["a1", "toolu_1"]);
    assert!(more);
    let (before, _, more) = thread.page(cursor, 1);
    assert_eq!(ids(&before), ["a0"]);
    assert!(!more);
}

#[test]
fn a_large_transcript_is_read_from_its_end_and_further_back_on_demand() {
    let path = temp_path("large");
    // A call at the head, its result past more filler than one span holds:
    // the newest page must not need the head, and the head must still pair.
    let filler = json!({ "type": "api-request-blob", "sessionId": "sess-1", "message": { "content": "x".repeat(8 * 1024) } }).to_string();
    let mut lines = vec![
        person("u1", 1, "Build it"),
        tool_use(
            "a1",
            2,
            "toolu_old",
            "Bash",
            json!({ "command": "cargo build" }),
        ),
    ];
    while lines.iter().map(|l| l.len() as u64 + 1).sum::<u64>() < 3 * SPAN_BYTES {
        lines.push(filler.clone());
    }
    lines.extend([
        tool_result("r1", 3, "toolu_old", json!("Finished"), false),
        said("a2", 4, "Built."),
        person("u2", 5, "Thanks"),
        said("a3", 6, "Welcome."),
    ]);
    write_lines(&path, &lines);
    let len = fs::metadata(&path).unwrap().len();
    let mut cache = HashMap::new();

    let newest = read(&path, &mut cache, None, 2);
    assert_eq!(ids(&newest.items), ["u2", "a3"]);
    assert!(newest.older);
    let transcript = &cache[&path];
    assert!(transcript.start > 0, "only the end was read");
    assert!(len - transcript.start <= SPAN_BYTES);

    let cursor = newest.cursor.unwrap().parse::<u64>().unwrap();
    let back = read(&path, &mut cache, Some(cursor), 10);
    assert_eq!(ids(&back.items), ["u1", "toolu_old", "a2"]);
    assert!(!back.older);
    let (_, _, _, result, state) = action(&back.items[1]);
    assert_eq!(result.map(|r| r.text.as_str()), Some("Finished"));
    assert_eq!(state, CallState::Done);
    assert_eq!(cache[&path].start, 0);
}

#[test]
fn a_line_longer_than_a_span_is_read_whole_going_back() {
    let path = temp_path("giant-line");
    // A screenshot comes back as base64 inside one line, megabytes long.
    let picture = json!([{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "A".repeat(3 * SPAN_BYTES as usize) } }]);
    let lines = [
        person("u1", 1, "Take a screenshot"),
        tool_use(
            "a1",
            2,
            "toolu_shot",
            "mcp__claude-in-chrome__computer",
            json!({ "action": "screenshot" }),
        ),
        tool_result("r1", 3, "toolu_shot", picture, false),
        said("a2", 4, "Here it is."),
        person("u2", 5, "Thanks"),
        said("a3", 6, "Welcome."),
    ];
    write_lines(&path, &lines);
    let mut cache = HashMap::new();
    let newest = read(&path, &mut cache, None, 2);
    assert_eq!(ids(&newest.items), ["u2", "a3"]);
    let cursor = newest.cursor.unwrap().parse::<u64>().unwrap();
    let back = read(&path, &mut cache, Some(cursor), 10);
    assert_eq!(ids(&back.items), ["u1", "toolu_shot", "a2"]);
    let (_, title, _, result, state) = action(&back.items[1]);
    assert_eq!(title, "Used computer");
    assert_eq!(result.map(|r| r.text.as_str()), Some("[image]"));
    assert_eq!(state, CallState::Done);
}

#[test]
fn a_call_in_an_older_span_without_a_result_was_left_behind() {
    let path = temp_path("left-behind");
    let filler = json!({ "type": "progress", "pad": "y".repeat(8 * 1024) }).to_string();
    let mut lines = vec![tool_use(
        "a1",
        1,
        "toolu_lost",
        "Bash",
        json!({ "command": "sleep" }),
    )];
    while lines.iter().map(|l| l.len() as u64 + 1).sum::<u64>() < 2 * SPAN_BYTES {
        lines.push(filler.clone());
    }
    lines.extend([person("u1", 2, "Hello?"), said("a2", 3, "Here.")]);
    write_lines(&path, &lines);
    let mut cache = HashMap::new();
    let newest = read(&path, &mut cache, None, 1);
    let cursor = newest.cursor.unwrap().parse::<u64>().unwrap();
    let back = read(&path, &mut cache, Some(cursor), 10);
    assert_eq!(ids(&back.items), ["toolu_lost", "u1"]);
    assert_eq!(action(&back.items[0]).4, CallState::Error);
    assert_eq!(back.open_call, None);
}

#[test]
fn a_growing_file_is_read_incrementally() {
    let path = temp_path("growing");
    write_lines(&path, &[person("u1", 1, "Run the tests")]);
    let mut cache = HashMap::new();
    assert_eq!(ids(&read(&path, &mut cache, None, 60).items), ["u1"]);

    append(
        &path,
        &format!(
            "{}\n",
            tool_use(
                "a1",
                2,
                "toolu_1",
                "Bash",
                json!({ "command": "pnpm test" })
            )
        ),
    );
    let running = read(&path, &mut cache, None, 60);
    assert_eq!(action(&running.items[1]).4, CallState::Running);
    assert_eq!(
        running.open_call.as_ref().map(|c| c.title.as_str()),
        Some("Ran pnpm test")
    );
    let start = cache[&path].start;

    // The result mid-write: nothing changes until its newline lands.
    let result = tool_result("r1", 3, "toolu_1", json!("12 passed"), false);
    append(&path, &result[..40]);
    let mid = read(&path, &mut cache, None, 60);
    assert_eq!(action(&mid.items[1]).4, CallState::Running);
    let parsed = cache[&path].end;
    assert!(parsed < fs::metadata(&path).unwrap().len());

    append(
        &path,
        &format!("{}\n{}\n", &result[40..], said("a2", 4, "All green.")),
    );
    let done = read(&path, &mut cache, None, 60);
    assert_eq!(ids(&done.items), ["u1", "toolu_1", "a2"]);
    assert_eq!(action(&done.items[1]).4, CallState::Done);
    assert_eq!(done.open_call, None);
    // Read from where the last look stopped: the span only grew forward.
    assert_eq!(cache[&path].start, start);
    assert_eq!(cache[&path].end, fs::metadata(&path).unwrap().len());
}

#[test]
fn a_rewritten_file_is_read_again() {
    let path = temp_path("rewritten");
    write_lines(&path, &[said("a1", 1, "one"), said("a2", 2, "two")]);
    let mut cache = HashMap::new();
    assert_eq!(ids(&read(&path, &mut cache, None, 60).items), ["a1", "a2"]);
    write_lines(&path, &[said("b1", 3, "fresh")]);
    assert_eq!(ids(&read(&path, &mut cache, None, 60).items), ["b1"]);
}

#[test]
fn a_missing_file_is_no_page() {
    let path = temp_path("missing").with_file_name("nothing-here.jsonl");
    let mut cache = HashMap::new();
    assert_eq!(page_of(&path, &mut cache, None, 60).unwrap(), None);
    assert!(cache.is_empty());
}

#[test]
fn the_cache_keeps_the_most_recently_asked_transcripts() {
    let mut cache = HashMap::new();
    for n in 0..(CACHED as u64 + 2) {
        let path = PathBuf::from(format!("t{n}.jsonl"));
        cache.insert(
            path,
            Transcript {
                len: 0,
                modified: None,
                start: 0,
                end: 0,
                thread: Thread::default(),
                used: n,
            },
        );
    }
    evict(&mut cache);
    assert_eq!(cache.len(), CACHED);
    assert!(!cache.contains_key(Path::new("t0.jsonl")));
    assert!(!cache.contains_key(Path::new("t1.jsonl")));
}

#[test]
fn ids_and_cursors_that_could_reach_elsewhere_are_refused() {
    assert!(chat_of("../../etc/passwd", "C:\\x", None, None).is_err());
    assert!(chat_of("", "C:\\x", None, None).is_err());
    assert!(chat_of("abc-123", "C:\\x", Some("not-a-cursor"), None).is_err());
    // A session that never wrote its transcript: no page, not an error.
    assert_eq!(
        chat_of("no-such-session-0000", "C:\\nowhere\\at-all", None, None),
        Ok(None)
    );
}
