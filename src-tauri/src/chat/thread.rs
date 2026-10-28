//! The conversation of one transcript, built line by line: what was said,
//! each tool call paired with its result, what was left out.
//!
//! A thread covers one span of the file. The newest span is read first and
//! older spans are put in front of it as the phone scrolls back, so a result
//! can arrive before its call is known: it waits among the orphans until the
//! span holding the call is read.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::Value;

use super::tools::{self, clip, TEXT_BYTES};
use super::{Body, CallResult, CallState, ChatItem, OpenCall, PendingQuestion, Question, Sender};

const COMPACTED: &str = "Conversation compacted";
const INTERRUPTED: &str = "Turn interrupted";
const RESUMED: &str = "Session resumed";

/// What Claude Code writes around a message a plugin sends between turns.
const PLUGIN_HEAD: &str = "plugin sent a message:";
const PLUGIN_TAIL: &str = "This is how Claude Code surfaces";
/// Hangar's own plugin: what it sends is Hangar's, not the person's.
const HANGAR_PLUGIN: &str = "hangar-bridge";
const HANGAR_MARK: &str = "[Hangar]";

/// Messages that open with one of these were written by Claude Code or a
/// tool, not typed: command output, notifications, reminders.
const NOT_TYPED: &[&str] = &[
    "<task-notification>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<local-command-caveat>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<system-reminder>",
    "<cross-session-message",
    "<agent-message",
    "<ci-monitor-event>",
];

/// The fields of a transcript line that matter here. The heavy parts —
/// message, attachment, structured tool result — stay raw until the line
/// turns out to need them: most lines of a long session are bookkeeping.
#[derive(Deserialize)]
struct Line<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    uuid: Option<Cow<'a, str>>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow)]
    subtype: Option<Cow<'a, str>>,
    // Options rather than bools: a `null` there must not cost the line.
    #[serde(rename = "isSidechain")]
    sidechain: Option<bool>,
    #[serde(rename = "isMeta")]
    meta: Option<bool>,
    #[serde(rename = "isCompactSummary")]
    compact_summary: Option<bool>,
    #[serde(rename = "isVisibleInTranscriptOnly")]
    transcript_only: Option<bool>,
    #[serde(rename = "isApiErrorMessage")]
    api_error: Option<bool>,
    #[serde(borrow)]
    origin: Option<&'a RawValue>,
    #[serde(borrow)]
    message: Option<&'a RawValue>,
    #[serde(borrow)]
    attachment: Option<&'a RawValue>,
    #[serde(rename = "toolUseResult", borrow)]
    tool_use_result: Option<&'a RawValue>,
}

/// Who a message comes from, by Claude Code's own account.
#[derive(Deserialize, Default)]
struct Origin {
    kind: Option<String>,
    name: Option<String>,
}

fn raw(value: Option<&RawValue>) -> Option<Value> {
    serde_json::from_str(value?.get()).ok()
}

fn time_of(line: &Line) -> i64 {
    line.timestamp
        .as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.timestamp_millis())
        .unwrap_or(0)
}

/// One tool call: its line, and what came of it.
#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub detail: Option<String>,
    /// The file, for a Read: consecutive reads share one line.
    pub read: Option<String>,
    pub result: Option<CallResult>,
    pub state: CallState,
}

#[derive(Debug, Clone)]
enum Node {
    /// Final as it is: what the person, Hangar or Claude said, a notice.
    Said(Body),
    /// One tool call, or several reads in a row folded into one line.
    Calls(Vec<Call>),
    Question {
        questions: Vec<Question>,
        answer: Option<Vec<super::Answer>>,
    },
}

#[derive(Debug, Clone)]
struct Entry {
    /// Where the line that started it begins in the file: the cursor.
    offset: u64,
    id: String,
    at: i64,
    node: Node,
}

/// What a result says once its call is known.
struct Outcome {
    ok: bool,
    text: String,
    /// AskUserQuestion's answers, by question, from the line's `toolUseResult`.
    answers: Option<Value>,
}

/// Only the answers of a `toolUseResult`: for any other tool it holds the
/// whole output again — a file read, a command's log — left unparsed.
#[derive(Deserialize)]
struct Answered {
    answers: Option<Value>,
}

#[derive(Default)]
pub struct Thread {
    entries: Vec<Entry>,
    /// Calls still without a result, by tool_use id: their entry's index.
    open: HashMap<String, usize>,
    /// Results whose call lies in a span not read yet.
    orphans: HashMap<String, Outcome>,
    /// Ids already shown: a line written twice is shown once.
    seen: HashSet<String>,
    /// Whether a new turn started anywhere in this span: a call before it
    /// that never got its result was left behind, not still running.
    turn_seen: bool,
}

impl Thread {
    /// Feeds one complete line, which starts at `offset` in the file.
    pub fn feed(&mut self, offset: u64, text: &str) {
        let Ok(line) = serde_json::from_str::<Line>(text) else {
            return;
        };
        // Subagents' own steps: the call that started them is the one line.
        if line.sidechain == Some(true) {
            return;
        }
        match line.kind.as_deref() {
            Some("user") => self.on_user(offset, &line),
            Some("assistant") => self.on_assistant(offset, &line),
            Some("system") => {
                if line.subtype.as_deref() == Some("compact_boundary") {
                    self.notice(offset, &line, COMPACTED);
                }
            }
            Some("attachment") => self.on_attachment(offset, &line),
            _ => {}
        }
    }

    fn id_of(line: &Line, offset: u64) -> String {
        line.uuid
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| format!("line-{offset}"))
    }

    fn push(&mut self, offset: u64, id: String, at: i64, node: Node) {
        if !self.seen.insert(id.clone()) {
            return;
        }
        self.entries.push(Entry {
            offset,
            id,
            at,
            node,
        });
    }

    fn notice(&mut self, offset: u64, line: &Line, text: &str) {
        let id = Self::id_of(line, offset);
        self.push(
            offset,
            id,
            time_of(line),
            Node::Said(Body::Notice {
                text: text.to_string(),
            }),
        );
    }

    /// A new turn: whatever call is still open was left behind — the
    /// session ended or was interrupted before its result.
    fn turn(&mut self) {
        self.turn_seen = true;
        for (_, index) in std::mem::take(&mut self.open) {
            abandon(&mut self.entries[index].node);
        }
    }

    fn on_user(&mut self, offset: u64, line: &Line) {
        let Some(message) = raw(line.message) else {
            return;
        };
        let content = message.get("content");

        // Tool results come back as the user's turn.
        if let Some(blocks) = content.and_then(Value::as_array) {
            let mut settled = false;
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                    continue;
                }
                settled = true;
                let Some(id) = block.get("tool_use_id").and_then(Value::as_str) else {
                    continue;
                };
                let outcome = Outcome {
                    ok: block.get("is_error").and_then(Value::as_bool) != Some(true),
                    text: tools::result_text(block.get("content")),
                    answers: line
                        .tool_use_result
                        .and_then(|result| serde_json::from_str::<Answered>(result.get()).ok())
                        .and_then(|result| result.answers),
                };
                self.settle(id, outcome);
            }
            if settled {
                return;
            }
        }

        if line.meta == Some(true)
            || line.compact_summary == Some(true)
            || line.transcript_only == Some(true)
        {
            return;
        }
        let origin: Origin = line
            .origin
            .and_then(|origin| serde_json::from_str(origin.get()).ok())
            .unwrap_or_default();
        let (text, images) = said(content);
        self.prompt(offset, line, &origin, text, images, true);
    }

    /// A message typed by the person or sent by Hangar — or something else
    /// wearing the user's role, left out. `starts_turn` is false for a message
    /// handed over mid-turn: the calls around it are not left behind.
    fn prompt(
        &mut self,
        offset: u64,
        line: &Line,
        origin: &Origin,
        text: String,
        images: u32,
        starts_turn: bool,
    ) {
        let hangar_plugin = match origin.kind.as_deref() {
            None | Some("human") => false,
            Some("plugin") if origin.name.as_deref() == Some(HANGAR_PLUGIN) => true,
            // Notifications, other sessions, other plugins, the coordinator.
            Some(_) => return,
        };
        let trimmed = text.trim();
        if trimmed.starts_with("[Request interrupted by user") {
            if starts_turn {
                self.turn();
            }
            self.notice(offset, line, INTERRUPTED);
            return;
        }
        let text = if hangar_plugin {
            unframe(trimmed)
        } else if let Some(command) = slash_command(trimmed) {
            command
        } else if let Some(command) = trimmed
            .starts_with("<bash-input>")
            .then(|| tag(trimmed, "bash-input"))
            .flatten()
        {
            format!("! {command}")
        } else if NOT_TYPED.iter().any(|mark| trimmed.starts_with(mark)) {
            return;
        } else {
            trimmed.to_string()
        };
        if text.is_empty() && images == 0 {
            return;
        }
        let from = if hangar_plugin || text.starts_with(HANGAR_MARK) {
            Sender::Hangar
        } else {
            Sender::Person
        };
        if starts_turn {
            self.turn();
        }
        let id = Self::id_of(line, offset);
        self.push(
            offset,
            id,
            time_of(line),
            Node::Said(Body::User {
                text: clip(&text, TEXT_BYTES),
                from,
                images,
            }),
        );
    }

    fn on_attachment(&mut self, offset: u64, line: &Line) {
        let Some(attachment) = raw(line.attachment) else {
            return;
        };
        let kind = attachment.get("type").and_then(Value::as_str);
        // A message the person typed while Claude worked, handed over between
        // two tool calls.
        if kind == Some("queued_command") {
            let mode = attachment.get("commandMode").and_then(Value::as_str);
            let meta = attachment.get("isMeta").and_then(Value::as_bool) == Some(true);
            if meta || mode.is_some_and(|mode| mode != "prompt") {
                return;
            }
            let origin: Origin = attachment
                .get("origin")
                .and_then(|origin| serde_json::from_value(origin.clone()).ok())
                .unwrap_or_default();
            let (text, images) = said(attachment.get("prompt"));
            self.prompt(offset, line, &origin, text, images, false);
            return;
        }
        // The hooks that run when a session starts again say so by name; one
        // notice for however many hooks there are.
        let hook = attachment.get("hookName").and_then(Value::as_str);
        if hook == Some("SessionStart:resume") {
            let already = matches!(
                self.entries.last(),
                Some(Entry { node: Node::Said(Body::Notice { text }), .. }) if text == RESUMED
            );
            if !already {
                self.turn();
                self.notice(offset, line, RESUMED);
            }
        }
    }

    fn on_assistant(&mut self, offset: u64, line: &Line) {
        let Some(message) = raw(line.message) else {
            return;
        };
        let single;
        let blocks: &[Value] = match message.get("content") {
            Some(Value::Array(blocks)) => blocks,
            Some(Value::String(text)) => {
                single = [serde_json::json!({ "type": "text", "text": text })];
                &single
            }
            _ => return,
        };
        let at = time_of(line);
        let base = Self::id_of(line, offset);
        let synthetic = message.get("model").and_then(Value::as_str) == Some("<synthetic>");

        for (index, block) in blocks.iter().enumerate() {
            let id = if index == 0 {
                base.clone()
            } else {
                format!("{base}:{index}")
            };
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if text.trim().is_empty() {
                        continue;
                    }
                    let body = if line.api_error == Some(true) {
                        // The limit hit, the login expired, the connection lost.
                        Body::Notice {
                            text: clip(text.trim(), TEXT_BYTES),
                        }
                    } else if synthetic {
                        // "No response requested." after an interruption.
                        continue;
                    } else {
                        Body::Assistant {
                            text: clip(text, TEXT_BYTES),
                        }
                    };
                    self.push(offset, id, at, Node::Said(body));
                }
                Some("tool_use") => self.call(offset, at, line, block),
                // Thinking stays Claude's own.
                _ => {}
            }
        }
    }

    fn call(&mut self, offset: u64, at: i64, line: &Line, block: &Value) {
        let Some(id) = block.get("id").and_then(Value::as_str) else {
            return;
        };
        if self.seen.contains(id) {
            return;
        }
        let tool = block.get("name").and_then(Value::as_str).unwrap_or("");
        let input = block.get("input").cloned().unwrap_or(Value::Null);

        if tool == "AskUserQuestion" {
            self.push(
                offset,
                id.to_string(),
                at,
                Node::Question {
                    questions: tools::questions(&input),
                    answer: None,
                },
            );
            self.open.insert(id.to_string(), self.entries.len() - 1);
            return;
        }

        let described = tools::describe(tool, &input, line.cwd.as_deref());
        let call = Call {
            id: id.to_string(),
            tool: tool.to_string(),
            title: described.title,
            detail: described.detail,
            read: described.read,
            result: None,
            state: CallState::Running,
        };
        // Reads in a row read as one line.
        if call.read.is_some() {
            if let Some(Entry {
                node: Node::Calls(calls),
                ..
            }) = self.entries.last_mut()
            {
                if calls.iter().all(|c| c.read.is_some()) {
                    calls.push(call);
                    self.seen.insert(id.to_string());
                    self.open.insert(id.to_string(), self.entries.len() - 1);
                    return;
                }
            }
        }
        self.push(offset, id.to_string(), at, Node::Calls(vec![call]));
        self.open.insert(id.to_string(), self.entries.len() - 1);
    }

    /// A result for the call `id`: paired now, or kept until its call is read.
    fn settle(&mut self, id: &str, outcome: Outcome) {
        match self.open.remove(id) {
            Some(index) => apply(&mut self.entries[index].node, id, outcome),
            None => {
                if !self.seen.contains(id) {
                    self.orphans.insert(id.to_string(), outcome);
                }
            }
        }
    }

    /// Puts an older span in front of this one.
    pub fn prepend(&mut self, mut older: Thread) {
        let shift = older.entries.len();
        for index in self.open.values_mut() {
            *index += shift;
        }
        for (id, index) in older.open.drain() {
            match self.orphans.remove(&id) {
                Some(outcome) => apply(&mut older.entries[index].node, &id, outcome),
                // The conversation went on without it: left behind.
                None if self.turn_seen => abandon(&mut older.entries[index].node),
                // Nothing since says otherwise: it may still run.
                None => {
                    self.open.insert(id, index);
                }
            }
        }
        older.entries.append(&mut self.entries);
        self.entries = older.entries;
        self.orphans.extend(older.orphans);
        self.seen.extend(older.seen);
        self.turn_seen |= older.turn_seen;
    }

    /// How many items start before `before` (all of them without one).
    pub fn count_before(&self, before: Option<u64>) -> usize {
        match before {
            Some(before) => self.entries.partition_point(|e| e.offset < before),
            None => self.entries.len(),
        }
    }

    /// The `limit` items just before `before`, oldest first, with the offset
    /// of the first one — the cursor of the page before — and whether items
    /// older than the page were read. Items from one line share its offset:
    /// a page takes them all or none, or the next page would skip some.
    pub fn page(&self, before: Option<u64>, limit: usize) -> (Vec<ChatItem>, Option<u64>, bool) {
        let end = self.count_before(before);
        let mut start = end.saturating_sub(limit);
        while start > 0
            && start < end
            && self.entries[start - 1].offset == self.entries[start].offset
        {
            start -= 1;
        }
        let items = self.entries[start..end].iter().map(render).collect();
        let first = self.entries.get(start).filter(|_| start < end);
        (items, first.map(|e| e.offset), start > 0)
    }

    /// The AskUserQuestion still waiting for the person, if one does.
    pub fn pending_question(&self) -> Option<PendingQuestion> {
        self.open
            .values()
            .copied()
            .filter_map(|index| match &self.entries[index].node {
                Node::Question { questions, .. } => Some((index, questions)),
                _ => None,
            })
            .max_by_key(|(index, _)| *index)
            .map(|(index, questions)| PendingQuestion {
                tool_use_id: self.entries[index].id.clone(),
                questions: questions.clone(),
            })
    }

    /// The conversation's last tool call, when it has no result yet.
    pub fn open_call(&self) -> Option<OpenCall> {
        let last = self
            .entries
            .iter()
            .rev()
            .find_map(|entry| match &entry.node {
                Node::Calls(calls) => calls.last().map(Some),
                Node::Question { .. } => Some(None),
                Node::Said(_) => None,
            })??;
        (last.state == CallState::Running && self.open.contains_key(&last.id)).then(|| OpenCall {
            tool: last.tool.clone(),
            title: last.title.clone(),
            detail: last.detail.clone(),
        })
    }
}

/// The text and the number of pictures of a message's content.
fn said(content: Option<&Value>) -> (String, u32) {
    match content {
        Some(Value::String(text)) => (text.clone(), 0),
        Some(Value::Array(blocks)) => {
            let mut texts = Vec::new();
            let mut images = 0;
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            texts.push(text);
                        }
                    }
                    Some("image") => images += 1,
                    _ => {}
                }
            }
            (texts.join("\n\n"), images)
        }
        _ => (String::new(), 0),
    }
}

/// What a plugin sent, without the lines Claude Code wraps it in.
fn unframe(text: &str) -> String {
    let mut text = text;
    if let Some((first, rest)) = text.split_once('\n') {
        if first.trim_end().ends_with(PLUGIN_HEAD) {
            text = rest;
        }
    }
    if let Some(at) = text.rfind(PLUGIN_TAIL) {
        text = &text[..at];
    }
    text.trim().to_string()
}

/// The text inside the first `<name>…</name>` of `text`.
fn tag(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(text[start..end].trim().to_string())
}

/// A slash command the person ran, as they typed it: `/name args`.
fn slash_command(text: &str) -> Option<String> {
    if !(text.starts_with("<command-name>") || text.starts_with("<command-message>")) {
        return None;
    }
    let name = tag(text, "command-name")?;
    let args = tag(text, "command-args").unwrap_or_default();
    Some(if args.is_empty() {
        name
    } else {
        format!("{name} {args}")
    })
}

fn apply(node: &mut Node, id: &str, outcome: Outcome) {
    match node {
        Node::Calls(calls) => {
            if let Some(call) = calls.iter_mut().find(|call| call.id == id) {
                call.state = if outcome.ok {
                    CallState::Done
                } else {
                    CallState::Error
                };
                call.result = Some(CallResult {
                    ok: outcome.ok,
                    text: outcome.text,
                });
            }
        }
        Node::Question { questions, answer } => {
            // Refused or interrupted: no answer to show.
            if outcome.ok {
                *answer = Some(tools::answers(
                    questions,
                    outcome.answers.as_ref(),
                    &outcome.text,
                ));
            }
        }
        Node::Said(_) => {}
    }
}

fn abandon(node: &mut Node) {
    if let Node::Calls(calls) = node {
        for call in calls.iter_mut().filter(|c| c.state == CallState::Running) {
            call.state = CallState::Error;
        }
    }
}

fn render(entry: &Entry) -> ChatItem {
    let body = match &entry.node {
        Node::Said(body) => body.clone(),
        Node::Calls(calls) if calls.len() == 1 => {
            let call = &calls[0];
            Body::Action {
                tool: call.tool.clone(),
                title: call.title.clone(),
                detail: call.detail.clone(),
                result: call.result.clone(),
                state: call.state,
            }
        }
        Node::Calls(calls) => {
            let state = if calls.iter().any(|c| c.state == CallState::Running) {
                CallState::Running
            } else if calls.iter().any(|c| c.state == CallState::Error) {
                CallState::Error
            } else {
                CallState::Done
            };
            let files: Vec<&str> = calls.iter().filter_map(|c| c.read.as_deref()).collect();
            Body::Action {
                tool: calls[0].tool.clone(),
                title: format!("Read {} files", calls.len()),
                detail: Some(clip(&files.join("\n"), tools::CLIP_BYTES)),
                result: None,
                state,
            }
        }
        Node::Question { questions, answer } => Body::Question {
            tool_use_id: entry.id.clone(),
            questions: questions.clone(),
            answer: answer.clone(),
        },
    };
    ChatItem {
        id: entry.id.clone(),
        at: entry.at,
        body,
    }
}
