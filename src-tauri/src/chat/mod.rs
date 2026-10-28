//! A Claude Code pane's conversation as chat items, read from its transcript:
//! what Hangar Mobile shows instead of a copy of the terminal screen.
//!
//! The phone asks every second while a chat is open, and a session's
//! transcript runs to tens of megabytes, so a file is never read whole. First
//! contact reads its end, back to enough items for a page; scrolling back
//! reads further, span by span; each later look reads only what was appended
//! since — and nothing at all while the file's size and time are unchanged.
//!
//! - thread.rs: the lines of a span made into items, calls paired with their
//!   results.
//! - tools.rs: what one tool call reads as — title, input, result, answers.
//!
//! The transcript is read in file order. A rewind that forks the
//! conversation leaves both branches in the file, and both show.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::Serialize;

mod thread;
mod tools;

#[cfg(test)]
mod tests;

use thread::Thread;

/// Items per page unless the phone asks otherwise, and at most.
const PAGE_DEFAULT: usize = 60;
const PAGE_MAX: usize = 200;
/// How far back one read goes. A span whose lines are all one giant line —
/// a screenshot's base64 — doubles until a line starts in it.
const SPAN_BYTES: u64 = 512 * 1024;
/// Transcripts kept parsed: a phone or two, each on a chat or two.
const CACHED: usize = 4;

/// Who wrote a user message: the person, or Hangar (its messages start with
/// "[Hangar]").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sender {
    Person,
    Hangar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CallState {
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CallResult {
    pub ok: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub question: String,
    pub header: String,
    pub multi_select: bool,
    pub options: Vec<QuestionOption>,
}

/// What the person picked for one question: labels, and their own words when
/// they wrote some.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Answer {
    pub selected: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Body {
    User {
        text: String,
        from: Sender,
        images: u32,
    },
    /// Markdown as Claude wrote it.
    Assistant { text: String },
    /// One tool call, or reads in a row folded into one.
    Action {
        tool: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<CallResult>,
        state: CallState,
    },
    #[serde(rename_all = "camelCase")]
    Question {
        tool_use_id: String,
        questions: Vec<Question>,
        /// Once the call has its result; absent when it was refused.
        #[serde(skip_serializing_if = "Option::is_none")]
        answer: Option<Vec<Answer>>,
    },
    /// Conversation compacted, turn interrupted, session resumed, an error
    /// Claude Code reported.
    Notice { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatItem {
    pub id: String,
    /// When its line was written, in ms.
    pub at: i64,
    #[serde(flatten)]
    pub body: Body,
}

/// An AskUserQuestion still without its answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestion {
    pub tool_use_id: String,
    pub questions: Vec<Question>,
}

/// The last tool call when it has no result yet: what a permission prompt,
/// if the pane shows one, is about.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenCall {
    pub tool: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One page of the conversation, and what waits at its end.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPage {
    /// Oldest first.
    pub items: Vec<ChatItem>,
    /// Whether there is anything before this page.
    pub older: bool,
    /// What to pass as `before` for the page before; null without one.
    pub cursor: Option<String>,
    /// Both from the end of the conversation, whatever the page.
    pub question: Option<PendingQuestion>,
    pub open_call: Option<OpenCall>,
}

/// One transcript as far as it was read.
struct Transcript {
    /// The file at the last look: unchanged, the parse stands as it is.
    len: u64,
    modified: Option<SystemTime>,
    /// The span read, from a line's start to the end of the last complete
    /// line. `start` reaches 0 once the head has been read.
    start: u64,
    end: u64,
    thread: Thread,
    /// When it was last asked for, to drop the coldest past `CACHED`.
    used: u64,
}

impl Transcript {
    /// Nothing read yet: the span sits at the end of the file's last
    /// complete line, ready to grow both ways.
    fn at_end(path: &Path, len: u64) -> std::io::Result<Self> {
        let end = last_line_end(path, len)?;
        Ok(Self {
            len: 0,
            modified: None,
            start: end,
            end,
            thread: Thread::default(),
            used: 0,
        })
    }
}

static TRANSCRIPTS: OnceLock<Mutex<HashMap<PathBuf, Transcript>>> = OnceLock::new();
static TICK: OnceLock<Mutex<u64>> = OnceLock::new();

fn read_range(path: &Path, from: u64, to: u64) -> std::io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::with_capacity((to - from) as usize);
    file.take(to - from).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Where the file's last complete line ends: a line mid-write waits for its
/// newline before it counts.
fn last_line_end(path: &Path, len: u64) -> std::io::Result<u64> {
    let mut to = len;
    let mut span = SPAN_BYTES;
    while to > 0 {
        let from = to.saturating_sub(span);
        let buf = read_range(path, from, to)?;
        if let Some(at) = buf.iter().rposition(|&b| b == b'\n') {
            return Ok(from + at as u64 + 1);
        }
        to = from;
        span *= 2;
    }
    Ok(0)
}

/// Feeds the complete lines of `buf`, which starts at `base` in the file.
fn feed(thread: &mut Thread, base: u64, buf: &[u8]) {
    let mut at = 0;
    for line in buf.split_inclusive(|&b| b == b'\n') {
        let offset = base + at as u64;
        at += line.len();
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        // A line that is not UTF-8 is not JSON either.
        if let Ok(text) = std::str::from_utf8(line) {
            if !text.trim().is_empty() {
                thread.feed(offset, text);
            }
        }
    }
}

/// Reads what was appended since the last look.
fn read_new(path: &Path, transcript: &mut Transcript, len: u64) -> std::io::Result<()> {
    if len <= transcript.end {
        return Ok(());
    }
    let buf = read_range(path, transcript.end, len)?;
    // Only complete lines: one mid-write waits for its newline.
    let Some(last) = buf.iter().rposition(|&b| b == b'\n') else {
        return Ok(());
    };
    feed(&mut transcript.thread, transcript.end, &buf[..=last]);
    transcript.end += last as u64 + 1;
    Ok(())
}

/// Reads further back until `wanted` items lie before `before`, or the
/// head of the file is reached.
fn read_back(
    path: &Path,
    transcript: &mut Transcript,
    before: Option<u64>,
    wanted: usize,
) -> std::io::Result<()> {
    let mut span = SPAN_BYTES;
    while transcript.start > 0 && transcript.thread.count_before(before) < wanted {
        let from = transcript.start.saturating_sub(span);
        let buf = read_range(path, from, transcript.start)?;
        // The span begins inside a line that started earlier: that line is
        // the next span's, unless the span reaches the head. The span's last
        // byte is the newline closing the line before `start`; only a newline
        // before it starts a line inside the span.
        let skip = if from == 0 {
            0
        } else {
            match buf[..buf.len() - 1].iter().position(|&b| b == b'\n') {
                Some(at) => at + 1,
                None => {
                    // One line longer than the span: look further.
                    span *= 2;
                    continue;
                }
            }
        };
        let mut older = Thread::default();
        feed(&mut older, from + skip as u64, &buf[skip..]);
        transcript.thread.prepend(older);
        transcript.start = from + skip as u64;
    }
    Ok(())
}

fn modified(meta: &fs::Metadata) -> Option<SystemTime> {
    meta.modified().ok()
}

/// One page of the transcript at `path`; `None` when there is no such file.
fn page_of(
    path: &Path,
    transcripts: &mut HashMap<PathBuf, Transcript>,
    before: Option<u64>,
    limit: usize,
) -> std::io::Result<Option<ChatPage>> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            transcripts.remove(path);
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    let len = meta.len();
    let time = modified(&meta);

    // Shorter than what was read, or rewritten in place at the same size:
    // the file is not the one parsed, so the parse starts over.
    let stale = transcripts
        .get(path)
        .is_some_and(|t| len < t.end || (len == t.len && time != t.modified));
    if stale {
        transcripts.remove(path);
    }
    if !transcripts.contains_key(path) {
        transcripts.insert(path.to_path_buf(), Transcript::at_end(path, len)?);
    }
    let transcript = transcripts.get_mut(path).expect("inserted above");

    if len != transcript.len || time != transcript.modified {
        read_new(path, transcript, len)?;
        transcript.len = len;
        transcript.modified = time;
    }
    read_back(path, transcript, before, limit)?;

    let (items, first, more) = transcript.thread.page(before, limit);
    let older = more || transcript.start > 0;
    Ok(Some(ChatPage {
        items,
        older,
        cursor: first.filter(|_| older).map(|offset| offset.to_string()),
        question: transcript.thread.pending_question(),
        open_call: transcript.thread.open_call(),
    }))
}

/// Keeps at most `CACHED` transcripts parsed, dropping the coldest.
fn evict(transcripts: &mut HashMap<PathBuf, Transcript>) {
    while transcripts.len() > CACHED {
        let coldest = transcripts
            .iter()
            .min_by_key(|(_, t)| t.used)
            .map(|(path, _)| path.clone());
        match coldest {
            Some(path) => transcripts.remove(&path),
            None => break,
        };
    }
}

fn chat_of(
    session_id: &str,
    cwd: &str,
    before: Option<&str>,
    limit: Option<usize>,
) -> Result<Option<ChatPage>, String> {
    if !crate::context::valid_session_id(session_id) {
        return Err(format!("\"{session_id}\" is not a session id."));
    }
    let before = match before {
        Some(cursor) => Some(
            cursor
                .parse::<u64>()
                .map_err(|_| format!("\"{cursor}\" is not a cursor this chat gave."))?,
        ),
        None => None,
    };
    let limit = limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
    let Some(path) = crate::context::claude_transcript(cwd, session_id) else {
        return Ok(None);
    };
    let transcripts = TRANSCRIPTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut transcripts = transcripts.lock().map_err(|_| "chat cache poisoned")?;
    let page = page_of(&path, &mut transcripts, before, limit).map_err(|err| err.to_string())?;
    if let Some(transcript) = transcripts.get_mut(&path) {
        let tick = TICK.get_or_init(|| Mutex::new(0));
        let mut tick = tick.lock().map_err(|_| "chat cache poisoned")?;
        *tick += 1;
        transcript.used = *tick;
    }
    evict(&mut transcripts);
    Ok(page)
}

/// One page of a Claude Code session's conversation: the newest without
/// `before`, else the one before that cursor. `None` while the session has
/// no transcript yet.
#[tauri::command]
pub async fn chat_page(
    session_id: String,
    cwd: String,
    before: Option<String>,
    limit: Option<usize>,
) -> Result<Option<ChatPage>, String> {
    crate::git::off_thread(move || chat_of(&session_id, &cwd, before.as_deref(), limit)).await
}
