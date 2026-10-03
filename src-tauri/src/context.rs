//! Where each pane's agent stands in its context window.
//!
//! The agents already write the answer into their transcripts: Claude Code
//! stamps `usage` on every assistant message, Codex logs `token_count` events
//! with the window size included. Reading those files is the sturdy path —
//! parsing the terminal output would break on the next redraw of an agent's
//! status line.
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

/// What the last exchange of a transcript says about the context.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Tokens occupying the window after the last exchange: the prompt side,
    /// fresh and cached alike, plus the reply that joins it on the next turn.
    pub used_tokens: u64,
    /// Model the transcript names, when it names one.
    pub model: Option<String>,
    /// Window size when the transcript itself declares it. Codex does;
    /// Claude Code leaves the window to configuration on the frontend side.
    pub context_window: Option<u64>,
}

/// What one transcript line contributes. The pieces arrive on different
/// lines — Codex names its model on `turn_context`, its numbers on
/// `token_count` — so the cursor accumulates facts instead of expecting one
/// line to carry them all.
enum LineFact {
    Usage {
        used: u64,
        window: Option<u64>,
        model: Option<String>,
    },
    Model(String),
}

fn parse_line(agent: &str, line: &str) -> Option<LineFact> {
    match agent {
        "claude" => parse_claude_line(line),
        "codex" => parse_codex_line(line),
        _ => None,
    }
}

/// One Claude Code transcript line. Only main-chain assistant messages
/// count: subagents write into the same file flagged `isSidechain`, and
/// their context is not the pane's.
fn parse_claude_line(line: &str) -> Option<LineFact> {
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
    Some(LineFact::Usage {
        used,
        window: None,
        model: message
            .get("model")
            .and_then(|m| m.as_str())
            .map(str::to_string),
    })
}

/// One Codex rollout line. `token_count` events carry the numbers — and the
/// window — while the model name travels on `turn_context` lines.
fn parse_codex_line(line: &str) -> Option<LineFact> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let payload = value.get("payload")?;
    match value.get("type").and_then(|t| t.as_str())? {
        "turn_context" => payload
            .get("model")
            .and_then(|m| m.as_str())
            .map(|m| LineFact::Model(m.to_string())),
        "event_msg" => {
            if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                return None;
            }
            // Rate-limit heartbeats reuse the event with `info: null`.
            let info = payload.get("info")?;
            let used = info
                .get("last_token_usage")?
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .filter(|&n| n > 0)?;
            Some(LineFact::Usage {
                used,
                window: info.get("model_context_window").and_then(|v| v.as_u64()),
                model: None,
            })
        }
        _ => None,
    }
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
    window: Option<u64>,
}

impl Cursor {
    fn snapshot(&self) -> Option<ContextUsage> {
        self.used.map(|used_tokens| ContextUsage {
            used_tokens,
            model: self.model.clone(),
            context_window: self.window,
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

/// Codex rollout paths by session id: the id is inside the file name, but
/// finding it means walking every dated directory, which is only worth doing
/// once. Hits are re-checked on use — a cleaned-up file must not pin a path.
static CODEX_PATHS: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

/// Reads whatever the file gained since the last look and folds it into the
/// cursor. Returns the current snapshot, `None` while no usage line has been
/// seen yet.
fn advance(
    agent: &str,
    path: &Path,
    cursors: &mut HashMap<PathBuf, Cursor>,
) -> Option<ContextUsage> {
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
        scan(agent, path, cursor, skipped_head);
        if skipped_head && cursor.used.is_none() {
            // The whole tail was one giant tool line, or usage only appears
            // earlier: pay for the full read, once.
            *cursor = Cursor::default();
            scan(agent, path, cursor, false);
        }
    }
    cursor.len = len;
    cursor.snapshot()
}

/// Parses the complete lines between the cursor and the end of the file.
/// `drop_first` discards the partial line a tail seek landed inside of.
fn scan(agent: &str, path: &Path, cursor: &mut Cursor, drop_first: bool) {
    let Ok(mut file) = fs::File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(cursor.offset)).is_err() {
        return;
    }
    let mut buf = Vec::new();
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
        match parse_line(agent, line) {
            Some(LineFact::Usage {
                used,
                window,
                model,
            }) => {
                cursor.used = Some(used);
                // Facts arrive on different lines; a line missing one must
                // not erase what an earlier line established.
                if window.is_some() {
                    cursor.window = window;
                }
                if model.is_some() {
                    cursor.model = model;
                }
            }
            Some(LineFact::Model(model)) => cursor.model = Some(model),
            None => {}
        }
    }
    cursor.offset += last_newline as u64 + 1;
}

/// `~/.claude/projects/<flattened cwd>/<session>.jsonl` — the same layout
/// `claude_sessions` lists from.
fn claude_transcript(cwd: &str, session_id: &str) -> Option<PathBuf> {
    let path = dirs::home_dir()?
        .join(".claude")
        .join("projects")
        .join(crate::sessions::encode_project_dir(cwd))
        .join(format!("{session_id}.jsonl"));
    path.is_file().then_some(path)
}

/// Finds the rollout whose file name ends in the session id, walking the
/// dated directories once and caching the answer.
fn codex_transcript(session_id: &str) -> Option<PathBuf> {
    let cache = CODEX_PATHS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().ok()?;
    if let Some(path) = cache.get(session_id) {
        if path.is_file() {
            return Some(path.clone());
        }
        cache.remove(session_id);
    }

    let root = dirs::home_dir()?.join(".codex").join("sessions");
    let mut files = Vec::new();
    crate::sessions::collect_jsonl(&root, &mut files, 0);
    let suffix = format!("-{session_id}.jsonl");
    let path = files.into_iter().find(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().ends_with(&suffix))
            .unwrap_or(false)
    })?;
    cache.insert(session_id.to_string(), path.clone());
    Some(path)
}

fn usage_snapshot(agent: &str, session_id: &str, cwd: &str) -> Option<ContextUsage> {
    // The id lands in a path; anything beyond what our own watchers produce
    // (uuids, hex, dashes) is refused rather than resolved.
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return None;
    }
    let path = match agent {
        "claude" => claude_transcript(cwd, session_id)?,
        "codex" => codex_transcript(session_id)?,
        // Gemini and opencode keep no transcript worth reading: no answer
        // beats a made-up zero.
        _ => return None,
    };
    let cursors = CURSORS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cursors = cursors.lock().ok()?;
    advance(agent, &path, &mut cursors)
}

/// Where the pane's agent stands in its context window, according to the
/// agent's own transcript. `None` covers every "show nothing" case: an agent
/// without a usable transcript, a session not written yet, a file with no
/// usage line so far.
#[tauri::command]
pub async fn context_usage(agent: String, session_id: String, cwd: String) -> Option<ContextUsage> {
    crate::git::off_thread(move || Ok(usage_snapshot(&agent, &session_id, &cwd)))
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
    const CODEX_COUNT: &str = r#"{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":29917611},"last_token_usage":{"input_tokens":98340,"cached_input_tokens":97024,"output_tokens":483,"total_tokens":98823},"model_context_window":353400},"rate_limits":{}}}"#;
    const CODEX_NULL_INFO: &str = r#"{"timestamp":"t","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{}}}"#;
    const CODEX_TURN: &str = r#"{"timestamp":"t","type":"turn_context","payload":{"cwd":"C:\\x","model":"gpt-5.6-sol","effort":"ultra"}}"#;

    fn temp_file(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hangar-context-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("transcript.jsonl")
    }

    fn advance_once(agent: &str, path: &Path) -> Option<ContextUsage> {
        // Each test keeps its own cursor map: the static one would leak
        // state between tests sharing a path.
        let mut cursors = HashMap::new();
        advance(agent, path, &mut cursors)
    }

    #[test]
    fn claude_line_sums_all_four_token_kinds() {
        match parse_claude_line(CLAUDE_LINE) {
            Some(LineFact::Usage {
                used,
                window,
                model,
            }) => {
                assert_eq!(used, 2 + 10143 + 152141 + 3674);
                assert_eq!(window, None);
                assert_eq!(model.as_deref(), Some("claude-fable-5"));
            }
            _ => panic!("expected usage"),
        }
    }

    #[test]
    fn claude_sidechain_lines_are_ignored() {
        assert!(parse_claude_line(CLAUDE_SIDECHAIN).is_none());
    }

    #[test]
    fn codex_count_carries_window_and_null_info_does_not_parse() {
        match parse_codex_line(CODEX_COUNT) {
            Some(LineFact::Usage {
                used,
                window,
                model,
            }) => {
                assert_eq!(used, 98823);
                assert_eq!(window, Some(353400));
                assert_eq!(model, None);
            }
            _ => panic!("expected usage"),
        }
        assert!(parse_codex_line(CODEX_NULL_INFO).is_none());
        match parse_codex_line(CODEX_TURN) {
            Some(LineFact::Model(model)) => assert_eq!(model, "gpt-5.6-sol"),
            _ => panic!("expected model"),
        }
    }

    #[test]
    fn incremental_reads_only_consume_complete_lines() {
        let path = temp_file("incremental");
        fs::write(&path, format!("{CLAUDE_LINE}\n")).unwrap();

        let mut cursors = HashMap::new();
        let first = advance("claude", &path, &mut cursors).unwrap();
        assert_eq!(first.used_tokens, 165960);

        // A partial line — the agent mid-write — changes nothing yet.
        let updated = CLAUDE_LINE.replace("\"input_tokens\":2", "\"input_tokens\":900002");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{}", &updated[..40]).unwrap();
        let mid = advance("claude", &path, &mut cursors).unwrap();
        assert_eq!(mid.used_tokens, 165960);

        // The newline lands: the new figure replaces the old.
        writeln!(file, "{}", &updated[40..]).unwrap();
        let after = advance("claude", &path, &mut cursors).unwrap();
        assert_eq!(after.used_tokens, 165960 - 2 + 900002);
    }

    #[test]
    fn rewritten_file_resets_the_cursor() {
        let path = temp_file("rewrite");
        fs::write(&path, format!("{CLAUDE_LINE}\n{CLAUDE_LINE}\n")).unwrap();
        let mut cursors = HashMap::new();
        advance("claude", &path, &mut cursors).unwrap();

        // A fresh, shorter transcript at the same path — a restarted session.
        let smaller = CLAUDE_LINE.replace(
            "\"cache_read_input_tokens\":152141",
            "\"cache_read_input_tokens\":10",
        );
        fs::write(&path, format!("{smaller}\n")).unwrap();
        let after = advance("claude", &path, &mut cursors).unwrap();
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
        let snapshot = advance_once("claude", &path).unwrap();
        assert_eq!(snapshot.used_tokens, 165960);
    }

    #[test]
    fn codex_model_survives_lines_that_do_not_name_it() {
        let path = temp_file("codex");
        fs::write(
            &path,
            format!("{CODEX_TURN}\n{CODEX_COUNT}\n{CODEX_NULL_INFO}\n"),
        )
        .unwrap();
        let snapshot = advance_once("codex", &path).unwrap();
        assert_eq!(snapshot.used_tokens, 98823);
        assert_eq!(snapshot.context_window, Some(353400));
        assert_eq!(snapshot.model.as_deref(), Some("gpt-5.6-sol"));
    }

    #[test]
    fn session_ids_that_look_like_paths_are_refused() {
        assert!(usage_snapshot("claude", "../../etc/passwd", "C:\\x").is_none());
        assert!(usage_snapshot("claude", "", "C:\\x").is_none());
        assert!(usage_snapshot("gemini", "abc-123", "C:\\x").is_none());
    }
}
