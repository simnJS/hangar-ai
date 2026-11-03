//! Where each pane's Claude Code stands in its context window.
//!
//! Claude Code already writes the answer into its transcript: it stamps
//! `usage` on every assistant message. Reading that file is the sturdy path —
//! parsing the terminal output would break on the next redraw of its status
//! line.
//!
//! Transcripts grow fast, so a file is never re-read whole on every poll: a
//! cursor per file remembers how far parsing got, and each look consumes only
//! what was appended since. First contact starts near the end of the file —
//! only the last exchange matters — and pays for one full scan only when the
//! tail turns out to hold no usage line at all.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

/// What the last exchange of a transcript says about the context. The window
/// it fills is not in the transcript: the frontend knows it by model.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Tokens occupying the window after the last exchange: the prompt side,
    /// fresh and cached alike, plus the reply that joins it on the next turn.
    pub used_tokens: u64,
    /// Model the transcript names, when it names one.
    pub model: Option<String>,
}

/// One Claude Code transcript line: the tokens it leaves in the window and
/// the model it names. Only main-chain assistant messages count: subagents
/// write into the same file flagged `isSidechain`, and their context is not
/// the pane's.
fn parse_claude_line(line: &str) -> Option<(u64, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    if value.get("isSidechain").and_then(|s| s.as_bool()) == Some(true) {
        return None;
    }
    let message = value.get("message")?;
    let usage = message.get("usage")?;
    let count = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    let used = count("input_tokens")
        + count("cache_read_input_tokens")
        + count("cache_creation_input_tokens")
        + count("output_tokens");
    if used == 0 {
        return None;
    }
    let model = message
        .get("model")
        .and_then(|m| m.as_str())
        .map(str::to_string);
    Some((used, model))
}

/// Parsing state of one transcript file.
#[derive(Default)]
struct Cursor {
    /// Bytes already parsed, always ending on a line boundary. A line still
    /// being written stays unconsumed until its newline lands.
    offset: u64,
    /// File length at the previous look, to return early when nothing moved.
    len: u64,
    used: Option<u64>,
    model: Option<String>,
}

impl Cursor {
    fn snapshot(&self) -> Option<ContextUsage> {
        self.used.map(|used_tokens| ContextUsage {
            used_tokens,
            model: self.model.clone(),
        })
    }
}

/// How much of a first-contact file is worth reading. The last exchange sits
/// at the end; on a transcript that has grown for days, the megabytes before
/// it are history the gauge does not need.
const TAIL_BYTES: u64 = 4 * 1024 * 1024;

/// One cursor per transcript, for the life of the process. Entries for panes
/// that closed linger, but each is a few dozen bytes against the megabytes of
/// re-reading they prevent.
static CURSORS: OnceLock<Mutex<HashMap<PathBuf, Cursor>>> = OnceLock::new();

/// Reads whatever the file gained since the last look and folds it into the
/// cursor. Returns the current snapshot, `None` while no usage line has been
/// seen yet.
fn advance(path: &Path, cursors: &mut HashMap<PathBuf, Cursor>) -> Option<ContextUsage> {
    let len = fs::metadata(path).ok()?.len();
    let cursor = cursors.entry(path.to_path_buf()).or_default();

    // Shorter than what was already parsed: the file was rewritten from
    // scratch (a restarted session), so the parse starts over with it.
    if len < cursor.offset {
        *cursor = Cursor::default();
    }

    if len > cursor.offset {
        // First contact skips to the tail; everything before is history.
        let mut skipped_head = false;
        if cursor.offset == 0 && len > TAIL_BYTES {
            cursor.offset = len - TAIL_BYTES;
            skipped_head = true;
        }
        scan(path, cursor, skipped_head);
        if skipped_head && cursor.used.is_none() {
            // The whole tail was one giant tool line, or usage only appears
            // earlier: pay for the full read, once.
            *cursor = Cursor::default();
            scan(path, cursor, false);
        }
    }
    cursor.len = len;
    cursor.snapshot()
}

/// Parses the complete lines between the cursor and the end of the file.
/// `drop_first` discards the partial line a tail seek landed inside of.
fn scan(path: &Path, cursor: &mut Cursor, drop_first: bool) {
    let mut buf = Vec::new();
    let Ok(mut file) = fs::File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(cursor.offset)).is_err() {
        return;
    }
    if file.read_to_end(&mut buf).is_err() {
        return;
    }
    // Only complete lines move the cursor; a line mid-write waits its turn.
    let Some(last_newline) = buf.iter().rposition(|&b| b == b'\n') else {
        return;
    };
    let text = String::from_utf8_lossy(&buf[..=last_newline]);
    let mut lines = text.split('\n');
    if drop_first {
        lines.next();
    }
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((used, model)) = parse_claude_line(line) {
            cursor.used = Some(used);
            // A line that names no model must not erase the one an earlier
            // line established.
            if model.is_some() {
                cursor.model = model;
            }
        }
    }
    cursor.offset += last_newline as u64 + 1;
}

/// `~/.claude/projects/<flattened cwd>/<session>.jsonl` — the same layout
/// `claude_sessions` lists from. The chat (chat/) reads the same file.
pub(crate) fn claude_transcript(cwd: &str, session_id: &str) -> Option<PathBuf> {
    let path = dirs::home_dir()?
        .join(".claude")
        .join("projects")
        .join(crate::sessions::encode_project_dir(cwd))
        .join(format!("{session_id}.jsonl"));
    path.is_file().then_some(path)
}

/// The id lands in a path; anything beyond what our own watchers produce
/// (uuids, hex, dashes) is refused rather than resolved.
pub(crate) fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn usage_snapshot(session_id: &str, cwd: &str) -> Option<ContextUsage> {
    if !valid_session_id(session_id) {
        return None;
    }
    let path = claude_transcript(cwd, session_id)?;
    let cursors = CURSORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cursors = cursors.lock().ok()?;
    advance(&path, &mut cursors)
}

/// Where a pane's Claude Code stands in its context window, according to its
/// own transcript. `None` covers every "show nothing" case: a session not
/// written yet, a file with no usage line so far.
pub async fn context_usage(session_id: String, cwd: String) -> Option<ContextUsage> {
    crate::git::off_thread(move || Ok(usage_snapshot(&session_id, &cwd)))
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // Shapes lifted from real transcripts on disk, trimmed to the fields
    // that matter plus the neighbours they sit among.
    const CLAUDE_LINE: &str = r#"{"type":"assistant","isSidechain":false,"message":{"model":"claude-fable-5","usage":{"input_tokens":2,"cache_creation_input_tokens":10143,"cache_read_input_tokens":152141,"output_tokens":3674,"service_tier":"standard"}},"uuid":"x"}"#;
    const CLAUDE_SIDECHAIN: &str = r#"{"type":"assistant","isSidechain":true,"message":{"model":"claude-fable-5","usage":{"input_tokens":9,"cache_creation_input_tokens":0,"cache_read_input_tokens":50000,"output_tokens":1,"service_tier":"standard"}}}"#;

    fn temp_file(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hangar-context-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("transcript.jsonl")
    }

    fn advance_once(path: &Path) -> Option<ContextUsage> {
        // Each test keeps its own cursor map: the static one would leak
        // state between tests sharing a path.
        let mut cursors = HashMap::new();
        advance(path, &mut cursors)
    }

    #[test]
    fn claude_line_sums_all_four_token_kinds() {
        let (used, model) = parse_claude_line(CLAUDE_LINE).expect("usage");
        assert_eq!(used, 2 + 10143 + 152141 + 3674);
        assert_eq!(model.as_deref(), Some("claude-fable-5"));
    }

    #[test]
    fn claude_sidechain_lines_are_ignored() {
        assert!(parse_claude_line(CLAUDE_SIDECHAIN).is_none());
    }

    #[test]
    fn incremental_reads_only_consume_complete_lines() {
        let path = temp_file("incremental");
        fs::write(&path, format!("{CLAUDE_LINE}\n")).unwrap();

        let mut cursors = HashMap::new();
        let first = advance(&path, &mut cursors).unwrap();
        assert_eq!(first.used_tokens, 165960);

        // A partial line — the agent mid-write — changes nothing yet.
        let updated = CLAUDE_LINE.replace("\"input_tokens\":2", "\"input_tokens\":900002");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{}", &updated[..40]).unwrap();
        let mid = advance(&path, &mut cursors).unwrap();
        assert_eq!(mid.used_tokens, 165960);

        // The newline lands: the new figure replaces the old.
        writeln!(file, "{}", &updated[40..]).unwrap();
        let after = advance(&path, &mut cursors).unwrap();
        assert_eq!(after.used_tokens, 165960 - 2 + 900002);
    }

    #[test]
    fn rewritten_file_resets_the_cursor() {
        let path = temp_file("rewrite");
        fs::write(&path, format!("{CLAUDE_LINE}\n{CLAUDE_LINE}\n")).unwrap();
        let mut cursors = HashMap::new();
        advance(&path, &mut cursors).unwrap();

        // A fresh, shorter transcript at the same path — a restarted session.
        let smaller = CLAUDE_LINE.replace(
            "\"cache_read_input_tokens\":152141",
            "\"cache_read_input_tokens\":10",
        );
        fs::write(&path, format!("{smaller}\n")).unwrap();
        let after = advance(&path, &mut cursors).unwrap();
        assert_eq!(after.used_tokens, 2 + 10143 + 10 + 3674);
    }

    #[test]
    fn first_contact_on_a_large_file_still_finds_an_early_usage_line() {
        let path = temp_file("tail");
        // The only usage line sits at the head, followed by more filler than
        // the tail window covers: the fallback full read has to kick in.
        let filler = format!("{{\"type\":\"noise\",\"pad\":\"{}\"}}\n", "x".repeat(4096));
        let mut body = format!("{CLAUDE_LINE}\n");
        while (body.len() as u64) <= TAIL_BYTES {
            body.push_str(&filler);
        }
        fs::write(&path, &body).unwrap();
        let snapshot = advance_once(&path).unwrap();
        assert_eq!(snapshot.used_tokens, 165960);
    }

    #[test]
    fn the_model_survives_lines_that_do_not_name_it() {
        let path = temp_file("model");
        let unnamed = CLAUDE_LINE.replace("\"model\":\"claude-fable-5\",", "");
        fs::write(&path, format!("{CLAUDE_LINE}\n{unnamed}\n")).unwrap();
        let snapshot = advance_once(&path).unwrap();
        assert_eq!(snapshot.used_tokens, 165960);
        assert_eq!(snapshot.model.as_deref(), Some("claude-fable-5"));
    }

    #[test]
    fn session_ids_that_look_like_paths_are_refused() {
        assert!(usage_snapshot("../../etc/passwd", "C:\\x").is_none());
        assert!(usage_snapshot("", "C:\\x").is_none());
    }
}
