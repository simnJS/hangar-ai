//! One transcript in, one digest out: the calls the agent was billed for,
//! the times it was active, and what the session was about.
//!
//! The digest keeps facts, not totals. Totals depend on the period asked for,
//! on the time zone and on the price table, and the digest is what goes in
//! the cache — so it holds what can be summed differently later, and nothing
//! that would have to be re-read to change.
//!
//! A line is deserialized into a struct naming only the fields used. serde
//! still walks the rest of it, but skipping a field allocates nothing, and a
//! transcript is mostly tool output nobody here needs.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{BufRead, Read};

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

/// One API call, at the moment its answer started. Field names are one
/// letter because the cache holds tens of thousands of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Call {
    /// What identifies the call across files, so a transcript copied into a
    /// resumed or forked session is not counted twice. 0 when unknown.
    #[serde(rename = "k")]
    pub key: u64,
    #[serde(rename = "t")]
    pub at: i64,
    /// Index into `Digest::models`.
    #[serde(rename = "m")]
    pub model: u16,
    #[serde(rename = "i")]
    pub input: u64,
    #[serde(rename = "o")]
    pub output: u64,
    #[serde(rename = "w")]
    pub cache_write_5m: u64,
    #[serde(rename = "h")]
    pub cache_write_1h: u64,
    #[serde(rename = "r")]
    pub cache_read: u64,
    #[serde(rename = "f", default, skip_serializing_if = "is_false")]
    pub fast: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Cumulative token counters, as Codex reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CodexTotals {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub cache_write_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

/// How sure a title is: a name the user gave beats Claude Code's own, which
/// beats the first prompt.
pub const TITLE_PROMPT: u8 = 1;
pub const TITLE_AI: u8 = 2;
pub const TITLE_CUSTOM: u8 = 3;

/// What one transcript file contributes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub agent: Agent,
    /// The conversation it belongs to. A subagent's file names its parent's,
    /// so a session's subagents count as part of it.
    pub session: String,
    /// The folder the session was launched in: the project it belongs to.
    pub cwd: String,
    /// A subagent's own file: its tokens count, its prompts are not turns.
    pub sub: bool,
    pub title: String,
    pub title_rank: u8,
    pub models: Vec<String>,
    pub calls: Vec<Call>,
    /// The time of every message, the user's and the agent's, sorted as read.
    pub stamps: Vec<i64>,
    /// The time of every prompt a person typed: a turn.
    pub prompts: Vec<i64>,
    /// Lines that would not parse, skipped.
    pub bad_lines: u32,
    /// Bytes parsed so far, always on a line boundary: where an appended
    /// transcript picks up.
    pub offset: u64,
    /// Codex reports running totals; the last one seen, to take differences.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_totals: Option<CodexTotals>,
    /// The model Codex named last, which every following count is billed to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_model: Option<u16>,
}

impl Digest {
    pub fn new(agent: Agent, sub: bool) -> Self {
        Self {
            agent,
            sub,
            ..Self::default()
        }
    }

    fn model_index(&mut self, model: &str) -> u16 {
        if let Some(at) = self.models.iter().position(|known| known == model) {
            return at as u16;
        }
        self.models.push(model.to_string());
        (self.models.len() - 1) as u16
    }

    fn offer_title(&mut self, title: &str, rank: u8) {
        let clean: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if clean.is_empty() || rank < self.title_rank {
            return;
        }
        // A prompt only ever names the session the first time.
        if rank == TITLE_PROMPT && self.title_rank == TITLE_PROMPT {
            return;
        }
        self.title = clean.chars().take(120).collect();
        self.title_rank = rank;
    }
}

/// 64-bit FNV-1a: stable from one build to the next, unlike std's hasher,
/// which matters for keys that live in the cache on disk.
pub fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        // A separator, so ("ab", "c") and ("a", "bc") differ.
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    // 0 means "no key".
    hash.max(1)
}

/// RFC 3339 to milliseconds since the epoch.
pub fn parse_time(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.timestamp_millis())
}

// ---------------------------------------------------------------------------
// Reading lines
// ---------------------------------------------------------------------------

/// Feeds every complete line to `on_line` and returns how many bytes that
/// consumed. A last line without its newline is only taken when `take_tail`
/// says the file is finished: an agent still writing it would otherwise be
/// read half-way and never again.
pub fn for_each_line(
    reader: impl Read,
    take_tail: bool,
    mut on_line: impl FnMut(&str),
) -> std::io::Result<u64> {
    let mut reader = std::io::BufReader::with_capacity(256 * 1024, reader);
    let mut buf = Vec::with_capacity(64 * 1024);
    let mut consumed = 0u64;
    loop {
        buf.clear();
        let read = reader.read_until(b'\n', &mut buf)?;
        if read == 0 {
            break;
        }
        let complete = buf.last() == Some(&b'\n');
        if !complete && !take_tail {
            break;
        }
        consumed += read as u64;
        let mut line = &buf[..];
        while let [rest @ .., b'\n' | b'\r'] = line {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }
        // A line that is not UTF-8 is not JSON either.
        if let Ok(text) = std::str::from_utf8(line) {
            on_line(text);
        }
    }
    Ok(consumed)
}

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ClaudeLine<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(rename = "sessionId", borrow)]
    session_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    // Options rather than bools: a `null` there must not cost the line.
    #[serde(rename = "isSidechain")]
    sidechain: Option<bool>,
    #[serde(rename = "isMeta")]
    meta: Option<bool>,
    #[serde(rename = "isCompactSummary")]
    compact: Option<bool>,
    #[serde(rename = "requestId", borrow)]
    request_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    message: Option<ClaudeMessage<'a>>,
    #[serde(rename = "customTitle", borrow)]
    custom_title: Option<Cow<'a, str>>,
    #[serde(rename = "aiTitle", borrow)]
    ai_title: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct ClaudeMessage<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    usage: Option<ClaudeUsage<'a>>,
    /// Left as raw JSON: on an assistant line it is the whole answer, which
    /// nothing here reads.
    #[serde(borrow)]
    content: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct ClaudeUsage<'a> {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    cache_creation: Option<CacheCreation>,
    #[serde(borrow)]
    speed: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct CacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

/// What a user line's content says about who wrote it.
enum UserContent {
    /// Typed by a person: text, maybe with an image.
    Prompt(String),
    /// The result of a tool the agent ran, sent back as a user message.
    ToolResult,
}

impl<'de> Deserialize<'de> for UserContent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ContentVisitor;

        #[derive(Deserialize)]
        struct Block<'a> {
            #[serde(rename = "type", borrow)]
            kind: Option<Cow<'a, str>>,
            #[serde(borrow)]
            text: Option<Cow<'a, str>>,
        }

        impl<'de> Visitor<'de> for ContentVisitor {
            type Value = UserContent;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a string or a list of content blocks")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<UserContent, E> {
                Ok(UserContent::Prompt(text.to_string()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<UserContent, A::Error> {
                let mut text = String::new();
                let mut tool = false;
                while let Some(block) = seq.next_element::<Block>()? {
                    match block.kind.as_deref() {
                        Some("tool_result") => tool = true,
                        Some("text") if text.is_empty() => {
                            text = block.text.map(Cow::into_owned).unwrap_or_default();
                        }
                        _ => {}
                    }
                }
                Ok(if tool {
                    UserContent::ToolResult
                } else {
                    UserContent::Prompt(text)
                })
            }
        }

        deserializer.deserialize_any(ContentVisitor)
    }
}

/// What Claude Code writes back into the conversation on its own — a local
/// command's output, a caveat — looks like a user message but is not a turn.
fn is_injected(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("<local-command")
        || text.starts_with("<command-stdout")
        || text.starts_with("<command-stderr")
        || text.starts_with("Caveat:")
}

/// The fields a Claude line can matter for: a cheap test that lets most
/// attachment and bookkeeping lines through without a parse.
fn claude_line_matters(line: &str) -> bool {
    line.contains("\"assistant\"") || line.contains("\"user\"") || line.contains("Title\"")
}

/// Folds one Claude Code line into the digest. `seen` maps a message's key to
/// its call: Claude Code writes one line per content block of an answer, each
/// carrying the usage so far, and only the last one is final.
pub fn claude_line(line: &str, digest: &mut Digest, seen: &mut HashMap<u64, usize>) {
    if !claude_line_matters(line) {
        return;
    }
    let Ok(parsed) = serde_json::from_str::<ClaudeLine>(line) else {
        digest.bad_lines += 1;
        return;
    };

    if let Some(title) = parsed.custom_title.as_deref() {
        digest.offer_title(title, TITLE_CUSTOM);
    }
    if let Some(title) = parsed.ai_title.as_deref() {
        digest.offer_title(title, TITLE_AI);
    }

    let kind = parsed.kind.as_deref().unwrap_or_default();
    if kind != "assistant" && kind != "user" {
        return;
    }
    if digest.session.is_empty() {
        if let Some(id) = parsed.session_id.as_deref() {
            digest.session = id.to_string();
        }
    }
    if digest.cwd.is_empty() {
        if let Some(cwd) = parsed.cwd.as_deref() {
            digest.cwd = cwd.to_string();
        }
    }
    let Some(at) = parsed.timestamp.as_deref().and_then(parse_time) else {
        return;
    };
    digest.stamps.push(at);
    let Some(message) = parsed.message else {
        return;
    };

    if kind == "user" {
        let flagged = |flag: Option<bool>| flag.unwrap_or(false);
        if flagged(parsed.sidechain)
            || flagged(parsed.meta)
            || flagged(parsed.compact)
            || digest.sub
        {
            return;
        }
        let Some(content) = message.content else {
            return;
        };
        if let Ok(UserContent::Prompt(text)) = serde_json::from_str::<UserContent>(content.get()) {
            if is_injected(&text) {
                return;
            }
            digest.prompts.push(at);
            if !text.trim_start().starts_with('<') {
                digest.offer_title(&text, TITLE_PROMPT);
            }
        }
        return;
    }

    let Some(usage) = message.usage else {
        return;
    };
    let model = message.model.as_deref().unwrap_or("unknown");
    // Claude Code's own placeholder for a message no model wrote.
    if model == "<synthetic>" {
        return;
    }
    let (mut write_5m, write_1h) = match &usage.cache_creation {
        Some(split) => (
            split.ephemeral_5m_input_tokens,
            split.ephemeral_1h_input_tokens,
        ),
        None => (usage.cache_creation_input_tokens, 0),
    };
    // Whatever the split leaves out of the total is billed as a 5-minute write.
    write_5m += usage
        .cache_creation_input_tokens
        .saturating_sub(write_5m + write_1h);
    if usage.input_tokens
        + usage.output_tokens
        + write_5m
        + write_1h
        + usage.cache_read_input_tokens
        == 0
    {
        return;
    }
    let key = match (message.id.as_deref(), parsed.request_id.as_deref()) {
        (Some(id), request) => fnv1a(&[
            b"claude",
            id.as_bytes(),
            request.unwrap_or_default().as_bytes(),
        ]),
        (None, _) => 0,
    };
    let call = Call {
        key,
        at,
        model: digest.model_index(model),
        input: usage.input_tokens,
        output: usage.output_tokens,
        cache_write_5m: write_5m,
        cache_write_1h: write_1h,
        cache_read: usage.cache_read_input_tokens,
        fast: usage.speed.as_deref() == Some("fast"),
    };
    if key != 0 {
        if let Some(&index) = seen.get(&key) {
            // The same answer, further along: its counts replace the earlier
            // ones, and it keeps the time it started at.
            let first = digest.calls[index].at;
            digest.calls[index] = Call { at: first, ..call };
            return;
        }
        seen.insert(key, digest.calls.len());
    }
    digest.calls.push(call);
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CodexLine<'a> {
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct SessionMeta<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    source: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct TurnContext<'a> {
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct EventMsg<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    info: Option<TokenInfo>,
}

#[derive(Deserialize)]
struct TokenInfo {
    total_token_usage: Option<CodexTotals>,
    last_token_usage: Option<CodexTotals>,
}

#[derive(Deserialize)]
struct ResponseItem<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    content: Option<Vec<CodexText<'a>>>,
}

#[derive(Deserialize)]
struct CodexText<'a> {
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

/// The thread that spawned a subagent's rollout, when it is one.
fn codex_parent(source: &serde_json::Value) -> Option<String> {
    source
        .get("subagent")?
        .get("thread_spawn")?
        .get("parent_thread_id")?
        .as_str()
        .map(str::to_string)
}

/// Folds one Codex rollout line into the digest. Codex reports cumulative
/// totals, sometimes the same one twice in a row; a call is the difference
/// between two of them.
pub fn codex_line(line: &str, digest: &mut Digest) {
    let Ok(parsed) = serde_json::from_str::<CodexLine>(line) else {
        digest.bad_lines += 1;
        return;
    };
    let at = parsed.timestamp.as_deref().and_then(parse_time);
    if let Some(at) = at {
        digest.stamps.push(at);
    }
    let (Some(kind), Some(payload)) = (parsed.kind.as_deref(), parsed.payload) else {
        return;
    };
    match kind {
        "session_meta" => {
            let Ok(meta) = serde_json::from_str::<SessionMeta>(payload.get()) else {
                return;
            };
            let parent = meta.source.as_ref().and_then(codex_parent);
            digest.sub = parent.is_some();
            if digest.session.is_empty() {
                if let Some(id) = parent.or_else(|| meta.id.as_deref().map(str::to_string)) {
                    digest.session = id;
                }
            }
            if digest.cwd.is_empty() {
                if let Some(cwd) = meta.cwd.as_deref() {
                    digest.cwd = cwd.to_string();
                }
            }
        }
        "turn_context" => {
            let Ok(context) = serde_json::from_str::<TurnContext>(payload.get()) else {
                return;
            };
            if let Some(model) = context.model.as_deref() {
                digest.codex_model = Some(digest.model_index(model));
            }
            // One turn_context opens every turn, in every version of the
            // rollout format.
            if let (Some(at), false) = (at, digest.sub) {
                digest.prompts.push(at);
            }
        }
        "event_msg" => {
            // Most events are not token counts; a substring is cheaper than
            // a parse to tell.
            if !payload.get().contains("token_count") {
                return;
            }
            let Ok(event) = serde_json::from_str::<EventMsg>(payload.get()) else {
                return;
            };
            if event.kind.as_deref() != Some("token_count") {
                return;
            }
            // Rate-limit heartbeats reuse the event with `info: null`.
            let (Some(info), Some(at)) = (event.info, at) else {
                return;
            };
            let Some(total) = info.total_token_usage else {
                return;
            };
            codex_count(digest, at, total, info.last_token_usage);
        }
        "response_item" if digest.title_rank == 0 => {
            let Ok(item) = serde_json::from_str::<ResponseItem>(payload.get()) else {
                return;
            };
            if item.kind.as_deref() != Some("message") || item.role.as_deref() != Some("user") {
                return;
            }
            // The environment and instructions Codex sends first are wrapped
            // in tags; the person's own words are not.
            let text = item
                .content
                .unwrap_or_default()
                .into_iter()
                .filter_map(|part| part.text)
                .find(|text| !text.trim_start().starts_with('<'));
            if let Some(text) = text {
                digest.offer_title(&text, TITLE_PROMPT);
            }
        }
        _ => {}
    }
}

fn codex_count(digest: &mut Digest, at: i64, total: CodexTotals, last: Option<CodexTotals>) {
    let previous = digest.codex_totals.unwrap_or_default();
    digest.codex_totals = Some(total);
    let delta = if total.total_tokens < previous.total_tokens {
        // The running total went back: a new thread in the same file. What
        // this event adds is its own last call.
        match last {
            Some(last) => last,
            None => total,
        }
    } else {
        CodexTotals {
            input_tokens: total.input_tokens.saturating_sub(previous.input_tokens),
            cached_input_tokens: total
                .cached_input_tokens
                .saturating_sub(previous.cached_input_tokens),
            cache_write_input_tokens: total
                .cache_write_input_tokens
                .saturating_sub(previous.cache_write_input_tokens),
            output_tokens: total.output_tokens.saturating_sub(previous.output_tokens),
            total_tokens: total.total_tokens - previous.total_tokens,
        }
    };
    // The same total reported again.
    if delta.total_tokens == 0 && delta.input_tokens == 0 && delta.output_tokens == 0 {
        return;
    }
    let model = match digest.codex_model {
        Some(model) => model,
        None => digest.model_index("unknown"),
    };
    // OpenAI counts cached input inside input; here they are kept apart, as
    // they are billed.
    let fresh = delta
        .input_tokens
        .saturating_sub(delta.cached_input_tokens + delta.cache_write_input_tokens);
    digest.calls.push(Call {
        // A forked rollout replays its parent's events verbatim: same time,
        // same running total, so the copy is recognised.
        key: fnv1a(&[
            b"codex",
            &at.to_le_bytes(),
            &total.total_tokens.to_le_bytes(),
        ]),
        at,
        model,
        input: fresh,
        output: delta.output_tokens,
        cache_write_5m: delta.cache_write_input_tokens,
        cache_write_1h: 0,
        cache_read: delta.cached_input_tokens,
        fast: false,
    });
}

// ---------------------------------------------------------------------------
// Whole files
// ---------------------------------------------------------------------------

/// Parses `reader` into `digest`, which may already hold what an earlier read
/// of the same file found: an appended transcript resumes where it stopped.
/// Returns the bytes consumed, to add to `digest.offset`.
pub fn read_into(digest: &mut Digest, reader: impl Read, finished: bool) -> std::io::Result<u64> {
    match digest.agent {
        Agent::Claude => {
            let mut seen: HashMap<u64, usize> = digest
                .calls
                .iter()
                .enumerate()
                .filter(|(_, call)| call.key != 0)
                .map(|(index, call)| (call.key, index))
                .collect();
            for_each_line(reader, finished, |line| {
                claude_line(line, digest, &mut seen)
            })
        }
        Agent::Codex => {
            let consumed = for_each_line(reader, finished, |line| codex_line(line, digest))?;
            name_early_calls(digest);
            Ok(consumed)
        }
    }
}

/// Codex can report a count before the turn that names its model: those
/// calls go to the first model the rollout names, once it has named one.
fn name_early_calls(digest: &mut Digest) {
    let Some(unknown) = digest.models.iter().position(|model| model == "unknown") else {
        return;
    };
    let Some(named) = digest.models.iter().position(|model| model != "unknown") else {
        return;
    };
    for call in &mut digest.calls {
        if call.model as usize == unknown {
            call.model = named as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic lines in the shapes Claude Code and Codex write, trimmed to
    // the fields that matter plus a few neighbours they sit among.
    fn assistant(id: &str, at: &str, output: u64) -> String {
        format!(
            r#"{{"parentUuid":"p","isSidechain":false,"type":"assistant","timestamp":"{at}","sessionId":"s-1","cwd":"C:\\dev\\app","requestId":"req-{id}","message":{{"id":"{id}","model":"claude-opus-5-5","role":"assistant","content":[{{"type":"text","text":"done"}}],"usage":{{"input_tokens":3,"output_tokens":{output},"cache_creation_input_tokens":100,"cache_read_input_tokens":2000,"cache_creation":{{"ephemeral_5m_input_tokens":40,"ephemeral_1h_input_tokens":60}},"service_tier":"standard","speed":"standard"}}}}}}"#
        )
    }

    fn user(at: &str, content: &str) -> String {
        format!(
            r#"{{"type":"user","timestamp":"{at}","sessionId":"s-1","cwd":"C:\\dev\\app","isSidechain":false,"message":{{"role":"user","content":{content}}}}}"#
        )
    }

    fn read(agent: Agent, text: &str) -> Digest {
        let mut digest = Digest::new(agent, false);
        let consumed = read_into(&mut digest, text.as_bytes(), true).unwrap();
        digest.offset += consumed;
        digest
    }

    #[test]
    fn claude_blocks_of_one_answer_make_one_call_with_the_final_counts() {
        let text = [
            user("2026-10-01T10:00:00Z", r#""Fix the login bug""#),
            assistant("m1", "2026-10-01T10:00:05Z", 10),
            assistant("m1", "2026-10-01T10:00:09Z", 250),
            user(
                "2026-10-01T10:00:10Z",
                r#"[{"type":"tool_result","tool_use_id":"t","content":"ok"}]"#,
            ),
            assistant("m2", "2026-10-01T10:00:30Z", 40),
        ]
        .join("\n");
        let digest = read(Agent::Claude, &text);
        assert_eq!(digest.session, "s-1");
        assert_eq!(digest.cwd, "C:\\dev\\app");
        assert_eq!(digest.calls.len(), 2);
        let first = digest.calls[0];
        assert_eq!(first.output, 250);
        assert_eq!(first.at, parse_time("2026-10-01T10:00:05Z").unwrap());
        assert_eq!((first.cache_write_5m, first.cache_write_1h), (40, 60));
        assert_eq!(first.cache_read, 2000);
        assert_eq!(digest.models, vec!["claude-opus-5-5"]);
        // The prompt is a turn; the tool result is not.
        assert_eq!(digest.prompts.len(), 1);
        assert_eq!(digest.stamps.len(), 5);
        assert_eq!(digest.title, "Fix the login bug");
    }

    #[test]
    fn titles_rank_custom_over_ai_over_the_first_prompt() {
        let text = [
            user(
                "2026-10-01T10:00:00Z",
                r#"[{"type":"text","text":"first words"}]"#,
            ),
            r#"{"type":"ai-title","aiTitle":"Login bug","sessionId":"s-1"}"#.to_string(),
            r#"{"type":"custom-title","customTitle":"Auth rework","sessionId":"s-1"}"#.to_string(),
            r#"{"type":"ai-title","aiTitle":"Later guess","sessionId":"s-1"}"#.to_string(),
        ]
        .join("\n");
        assert_eq!(read(Agent::Claude, &text).title, "Auth rework");
    }

    #[test]
    fn injected_and_meta_messages_are_not_turns() {
        let text = [
            user("2026-10-01T10:00:00Z", r#""<local-command-stdout>ok</local-command-stdout>""#),
            r#"{"type":"user","isMeta":true,"timestamp":"2026-10-01T10:00:01Z","message":{"role":"user","content":"Caveat: x"}}"#.to_string(),
            user("2026-10-01T10:00:02Z", r#""<command-name>/clear</command-name>""#),
        ]
        .join("\n");
        let digest = read(Agent::Claude, &text);
        // The slash command was typed; the command's output and the meta line were not.
        assert_eq!(digest.prompts.len(), 1);
        // A tag is no title.
        assert_eq!(digest.title, "");
    }

    #[test]
    fn malformed_lines_are_skipped_and_counted() {
        let text = [
            assistant("m1", "2026-10-01T10:00:05Z", 10),
            r#"{"type":"assistant","message":{"usage":"#.to_string(),
            "not json at all, but it mentions \"user\"".to_string(),
            assistant("m2", "2026-10-01T10:01:05Z", 10),
        ]
        .join("\n");
        let digest = read(Agent::Claude, &text);
        assert_eq!(digest.calls.len(), 2);
        assert_eq!(digest.bad_lines, 2);
    }

    #[test]
    fn synthetic_and_empty_usage_bill_nothing() {
        let synthetic =
            assistant("m1", "2026-10-01T10:00:05Z", 10).replace("claude-opus-5-5", "<synthetic>");
        let empty = r#"{"type":"assistant","timestamp":"2026-10-01T10:00:06Z","message":{"id":"m2","model":"claude-opus-5-5","usage":{"input_tokens":0,"output_tokens":0}}}"#;
        let digest = read(Agent::Claude, &format!("{synthetic}\n{empty}\n"));
        assert!(digest.calls.is_empty());
        assert_eq!(digest.stamps.len(), 2);
    }

    #[test]
    fn a_line_still_being_written_waits_for_its_newline() {
        let whole = assistant("m1", "2026-10-01T10:00:05Z", 10);
        let partial = assistant("m2", "2026-10-01T10:00:06Z", 10);
        let text = format!("{whole}\n{}", &partial[..50]);
        let mut digest = Digest::new(Agent::Claude, false);
        let consumed = read_into(&mut digest, text.as_bytes(), false).unwrap();
        assert_eq!(consumed, whole.len() as u64 + 1);
        assert_eq!(digest.calls.len(), 1);
    }

    const META: &str = r#"{"timestamp":"2026-10-02T09:00:00.000Z","type":"session_meta","payload":{"id":"thread-1","timestamp":"2026-10-02T09:00:00Z","cwd":"/home/me/app","originator":"codex-tui","cli_version":"1.0","source":"cli"}}"#;

    fn count(at: &str, input: u64, cached: u64, output: u64) -> String {
        format!(
            r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"cache_write_input_tokens":0,"output_tokens":{output},"reasoning_output_tokens":0,"total_tokens":{}}},"last_token_usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"total_tokens":2}},"model_context_window":272000}},"rate_limits":{{}}}}}}"#,
            input + output
        )
    }

    fn turn(at: &str, model: &str) -> String {
        format!(
            r#"{{"timestamp":"{at}","type":"turn_context","payload":{{"cwd":"/home/me/app","model":"{model}","effort":"high"}}}}"#
        )
    }

    #[test]
    fn codex_calls_are_the_differences_between_running_totals() {
        let text = [
            META.to_string(),
            turn("2026-10-02T09:00:01Z", "gpt-5.5"),
            r#"{"timestamp":"2026-10-02T09:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"},{"type":"input_text","text":"Add a cache"}]}}"#.to_string(),
            count("2026-10-02T09:00:10Z", 1000, 800, 50),
            // The same total again: nothing new.
            count("2026-10-02T09:00:11Z", 1000, 800, 50),
            r#"{"timestamp":"2026-10-02T09:00:12Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{}}}"#.to_string(),
            turn("2026-10-02T09:05:00Z", "gpt-6-astra"),
            count("2026-10-02T09:05:10Z", 3000, 2500, 80),
        ]
        .join("\n");
        let digest = read(Agent::Codex, &text);
        assert_eq!(digest.session, "thread-1");
        assert_eq!(digest.cwd, "/home/me/app");
        assert_eq!(digest.title, "Add a cache");
        assert_eq!(digest.prompts.len(), 2);
        assert_eq!(digest.calls.len(), 2);
        let first = digest.calls[0];
        assert_eq!(
            (first.input, first.cache_read, first.output),
            (200, 800, 50)
        );
        assert_eq!(digest.models[first.model as usize], "gpt-5.5");
        let second = digest.calls[1];
        assert_eq!(
            (second.input, second.cache_read, second.output),
            (300, 1700, 30)
        );
        assert_eq!(digest.models[second.model as usize], "gpt-6-astra");
    }

    #[test]
    fn a_count_before_the_first_turn_goes_to_the_model_named_next() {
        let text = [
            META.to_string(),
            count("2026-10-02T09:00:01Z", 500, 0, 20),
            turn("2026-10-02T09:00:02Z", "gpt-5.5"),
            count("2026-10-02T09:00:10Z", 1000, 0, 50),
        ]
        .join(
            "
",
        );
        let digest = read(Agent::Codex, &text);
        assert_eq!(digest.calls.len(), 2);
        assert!(digest
            .calls
            .iter()
            .all(|call| digest.models[call.model as usize] == "gpt-5.5"));
    }

    #[test]
    fn a_codex_subagent_belongs_to_its_parent_and_has_no_turns() {
        let meta = r#"{"timestamp":"2026-10-02T09:00:00Z","type":"session_meta","payload":{"id":"child","cwd":"/home/me/app","source":{"subagent":{"thread_spawn":{"parent_thread_id":"thread-1","depth":1}}}}}"#;
        let text = [
            meta.to_string(),
            turn("2026-10-02T09:00:01Z", "gpt-5.5"),
            count("2026-10-02T09:00:10Z", 100, 0, 10),
        ]
        .join("\n");
        let digest = read(Agent::Codex, &text);
        assert!(digest.sub);
        assert_eq!(digest.session, "thread-1");
        assert!(digest.prompts.is_empty());
        assert_eq!(digest.calls.len(), 1);
    }

    #[test]
    fn a_running_total_that_goes_back_counts_its_last_call() {
        let reset = count("2026-10-02T09:10:00Z", 10, 0, 5).replace(
            r#""last_token_usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"total_tokens":2}"#,
            r#""last_token_usage":{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"total_tokens":15}"#,
        );
        let text = [
            META.to_string(),
            count("2026-10-02T09:00:10Z", 1000, 0, 50),
            reset,
        ]
        .join("\n");
        let digest = read(Agent::Codex, &text);
        assert_eq!(digest.calls.len(), 2);
        assert_eq!((digest.calls[1].input, digest.calls[1].output), (10, 5));
    }
}
