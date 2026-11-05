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
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

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

/// A line longer than this is skipped unparsed: a usage line is a few hundred
/// bytes, and a giant tool result is not worth holding in memory.
const MAX_LINE: usize = 16 * 1024 * 1024;

/// One cursor per transcript, for the life of the process. Entries for panes
/// that closed linger, but each is a few dozen bytes against the megabytes of
/// re-reading they prevent. The map is locked only to look a cursor up; each
/// cursor has its own lock, held across that file's reads alone.
static CURSORS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<Cursor>>>>> = OnceLock::new();

/// Reads whatever the file gained since the last look and folds it into the
/// cursor. Returns the current snapshot, `None` while no usage line has been
/// seen yet.
fn advance(path: &Path, cursor: &mut Cursor) -> Option<ContextUsage> {
    let len = fs::metadata(path).ok()?.len();

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
            // earlier: pay for the full read, once, streamed.
            *cursor = Cursor::default();
            scan(path, cursor, false);
        }
    }
    cursor.snapshot()
}

/// Calls `each` with every complete line from the reader's position on and
/// returns the bytes those lines (and the ones skipped) took. Memory stays
/// bounded by `max_line` whatever the file holds: a longer line is drained
/// without being kept, and `each` never sees it. A line without its newline
/// yet — the agent mid-write — is left unconsumed.
fn stream_lines<R: BufRead>(mut reader: R, max_line: usize, mut each: impl FnMut(&[u8])) -> u64 {
    let mut consumed = 0u64;
    let mut line = Vec::new();
    loop {
        line.clear();
        let Ok(read) = (&mut reader)
            .take(max_line as u64 + 1)
            .read_until(b'\n', &mut line)
        else {
            break;
        };
        if read == 0 {
            break;
        }
        let mut taken = read as u64;
        if line.last() == Some(&b'\n') {
            line.pop();
            each(&line);
        } else if read > max_line {
            // Too long to keep: drain up to its newline.
            let mut ended = false;
            let mut sink = Vec::new();
            loop {
                sink.clear();
                let Ok(more) = (&mut reader).take(64 * 1024).read_until(b'\n', &mut sink) else {
                    break;
                };
                if more == 0 {
                    break;
                }
                taken += more as u64;
                if sink.last() == Some(&b'\n') {
                    ended = true;
                    break;
                }
            }
            if !ended {
                break;
            }
        } else {
            break;
        }
        consumed += taken;
    }
    consumed
}

/// Parses the complete lines between the cursor and the end of the file,
/// line by line. `drop_first` discards the partial line a tail seek landed
/// inside of.
fn scan(path: &Path, cursor: &mut Cursor, drop_first: bool) {
    let Ok(mut file) = fs::File::open(path) else {
        return;
    };
    if file.seek(SeekFrom::Start(cursor.offset)).is_err() {
        return;
    }
    let mut skip = drop_first;
    let (mut used, mut model) = (cursor.used, cursor.model.clone());
    let consumed = stream_lines(BufReader::new(file), MAX_LINE, |line| {
        if std::mem::take(&mut skip) {
            return;
        }
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        if let Some((tokens, named)) = parse_claude_line(line) {
            used = Some(tokens);
            // A line that names no model must not erase the one an earlier
            // line established.
            if named.is_some() {
                model = named;
            }
        }
    });
    cursor.used = used;
    cursor.model = model;
    cursor.offset += consumed;
}

/// The transcript of a session: `<projects>/<encoded cwd>/<id>.jsonl` in the
/// first of `roots` that holds it.
fn find_transcript(roots: &[PathBuf], cwd: &str, session_id: &str) -> Option<PathBuf> {
    let folder = crate::sessions::encode_project_dir(cwd);
    roots
        .iter()
        .map(|root| root.join(&folder).join(format!("{session_id}.jsonl")))
        .find(|path| path.is_file())
}

/// A session's transcript, in whichever folder Claude Code files
/// conversations in (`CLAUDE_CONFIG_DIR`, a profile's own `projects/`): the
/// same roots `claude_sessions` lists from. The chat (chat/) reads the same
/// file.
pub(crate) fn claude_transcript(cwd: &str, session_id: &str) -> Option<PathBuf> {
    find_transcript(&crate::accounts::projects_dirs(), cwd, session_id)
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
    let cursor = {
        let mut cursors = CURSORS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .ok()?;
        cursors.entry(path.clone()).or_default().clone()
    };
    let mut cursor = cursor.lock().ok()?;
    advance(&path, &mut cursor)
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
        // Each test keeps its own cursor: the static map would leak state
        // between tests sharing a path.
        advance(path, &mut Cursor::default())
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

        let mut cursor = Cursor::default();
        let first = advance(&path, &mut cursor).unwrap();
        assert_eq!(first.used_tokens, 165960);

        // A partial line — the agent mid-write — changes nothing yet.
        let updated = CLAUDE_LINE.replace("\"input_tokens\":2", "\"input_tokens\":900002");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{}", &updated[..40]).unwrap();
        let mid = advance(&path, &mut cursor).unwrap();
        assert_eq!(mid.used_tokens, 165960);

        // The newline lands: the new figure replaces the old.
        writeln!(file, "{}", &updated[40..]).unwrap();
        let after = advance(&path, &mut cursor).unwrap();
        assert_eq!(after.used_tokens, 165960 - 2 + 900002);
    }

    #[test]
    fn rewritten_file_resets_the_cursor() {
        let path = temp_file("rewrite");
        fs::write(&path, format!("{CLAUDE_LINE}\n{CLAUDE_LINE}\n")).unwrap();
        let mut cursor = Cursor::default();
        advance(&path, &mut cursor).unwrap();

        // A fresh, shorter transcript at the same path — a restarted session.
        let smaller = CLAUDE_LINE.replace(
            "\"cache_read_input_tokens\":152141",
            "\"cache_read_input_tokens\":10",
        );
        fs::write(&path, format!("{smaller}\n")).unwrap();
        let after = advance(&path, &mut cursor).unwrap();
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
    fn the_gauge_reads_a_transcript_from_a_custom_config_root() {
        let base = temp_file("roots").parent().unwrap().to_path_buf();
        let (main, custom) = (base.join("main"), base.join("custom").join("projects"));
        let folder = crate::sessions::encode_project_dir("/work/app");
        fs::create_dir_all(&main).unwrap();
        fs::create_dir_all(custom.join(&folder)).unwrap();
        fs::write(
            custom.join(&folder).join("sess-1.jsonl"),
            format!("{CLAUDE_LINE}\n"),
        )
        .unwrap();
        let roots = [main, custom];
        let path = find_transcript(&roots, "/work/app", "sess-1").expect("found in 2nd root");
        assert_eq!(advance_once(&path).unwrap().used_tokens, 165960);
        assert!(find_transcript(&roots, "/work/app", "other").is_none());
        assert!(find_transcript(&roots, "/elsewhere", "sess-1").is_none());
    }

    #[test]
    fn streaming_never_hands_over_a_line_past_the_bound() {
        let data = format!("short\n{}\nafter\npartial", "x".repeat(300));
        let mut seen = Vec::new();
        let consumed = stream_lines(std::io::Cursor::new(data.as_bytes()), 16, |l| {
            seen.push(String::from_utf8_lossy(l).into_owned())
        });
        // The 300-byte line is drained unseen; the line mid-write stays.
        assert_eq!(seen, ["short", "after"]);
        assert_eq!(consumed as usize, data.len() - "partial".len());
    }

    #[test]
    fn the_full_fallback_skips_a_giant_line_and_still_finds_usage() {
        let path = temp_file("giant");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "{CLAUDE_LINE}").unwrap();
        // Bigger than the tail window and than any line worth keeping.
        let giant = vec![b'x'; MAX_LINE + 1024];
        file.write_all(&giant).unwrap();
        writeln!(file).unwrap();
        let mut cursor = Cursor::default();
        let snapshot = advance(&path, &mut cursor).unwrap();
        assert_eq!(snapshot.used_tokens, 165960);
        assert_eq!(cursor.offset, fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn session_ids_that_look_like_paths_are_refused() {
        assert!(usage_snapshot("../../etc/passwd", "C:\\x").is_none());
        assert!(usage_snapshot("", "C:\\x").is_none());
    }
}
