//! The Claude Code conversations a pane can resume. Claude Code is the only
//! agent Hangar runs (owner's decision, 2026-11-03): a plain shell pane keeps
//! no session, so nothing here takes an agent.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AgentSession {
    pub id: String,
    /// First human message, used as a human-readable label in the UI.
    pub label: String,
    pub modified_ms: u64,
}

/// Claude Code flattens the project path into a single directory name by
/// replacing every non-alphanumeric character with a dash.
/// `C:\Users\me\Proj` -> `C--Users-me-Proj`
pub(crate) fn encode_project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn modified_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Extracts a display label from a `.jsonl` transcript by finding the first
/// real user message. Reads only the head of the file.
fn label_from_jsonl(path: &Path) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    let reader = BufReader::new(file);
    for line in reader.lines().take(60).map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }

        let text = match value.get("message").and_then(|m| m.get("content")) {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(" "),
            _ => continue,
        };

        let text = text.trim();
        // Skip slash commands and system-injected preambles.
        if text.is_empty() || text.starts_with('<') || text.starts_with("Caveat:") {
            continue;
        }
        let clean: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        return clean.chars().take(80).collect();
    }
    String::new()
}

/// The conversations Claude Code keeps for `cwd`, on every Claude account:
/// they share the main account's `projects/`, and a profile that could not
/// share it is read as well (accounts.rs).
fn claude_sessions(cwd: &str) -> Vec<AgentSession> {
    claude_sessions_in(&crate::accounts::projects_dirs(), cwd)
}

fn claude_sessions_in(roots: &[PathBuf], cwd: &str) -> Vec<AgentSession> {
    let mut sessions: Vec<AgentSession> = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root.join(encode_project_dir(cwd))) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|e| e.path()) {
            if !path.extension().is_some_and(|e| e == "jsonl") {
                continue;
            }
            let id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if sessions.iter().any(|known| known.id == id) {
                continue;
            }
            sessions.push(AgentSession {
                id,
                label: label_from_jsonl(&path),
                modified_ms: modified_ms(&path),
            });
        }
    }

    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified_ms));
    sessions
}

/// The Claude Code conversations kept for `cwd`, newest first.
pub fn list_sessions(cwd: String) -> Vec<AgentSession> {
    claude_sessions(&cwd)
}

/// Whether the conversation a pane is about to resume has a transcript left:
/// `Some(false)` only when it is nowhere on disk, `None` when that cannot be
/// told.
///
/// Claude Code takes its session id at startup but writes nothing until the
/// first message. A pane reset and then left alone keeps an id with no
/// conversation behind it, and `--resume` on that id only prints "No
/// conversation found". Every project directory is searched, not just the
/// pane's: a transcript filed under another spelling of the path is still
/// worth handing to the agent.
pub fn session_exists(id: String) -> Option<bool> {
    // Joined into a path below: anything but a plain id is not ours to judge.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    claude_transcript_exists_in(&crate::accounts::projects_dirs(), &id)
}

/// Found under any of `roots` — the folders every Claude account files its
/// conversations in — or `None` when none of them could be read.
fn claude_transcript_exists_in(roots: &[PathBuf], id: &str) -> Option<bool> {
    let answers: Vec<bool> = roots
        .iter()
        .filter_map(|root| claude_transcript_exists(root, id))
        .collect();
    if answers.is_empty() {
        None
    } else {
        Some(answers.contains(&true))
    }
}

fn claude_transcript_exists(projects: &Path, id: &str) -> Option<bool> {
    let file = format!("{id}.jsonl");
    let entries = fs::read_dir(projects).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .any(|entry| entry.path().join(&file).is_file()),
    )
}

/// Whether Claude Code is on PATH, so the UI can say when a pane will not
/// find it.
pub fn claude_installed() -> bool {
    // Without this the answer on macOS is "no": a bundled app is handed the
    // bare system PATH, and Claude Code does not install into it.
    crate::path_env::ensure();

    let paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    ["claude.exe", "claude.cmd", "claude"]
        .iter()
        .any(|bin| paths.iter().any(|dir| dir.join(bin).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!("hangar-sessions-{tag}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// The case a reset leaves behind: an id Claude Code handed out, and no
    /// transcript anywhere because nobody wrote to it.
    #[test]
    fn a_claude_session_with_no_transcript_is_reported_gone() {
        let projects = temp_dir("claude");
        let here = projects.join("C--repo");
        let elsewhere = projects.join("C--other-spelling");
        fs::create_dir_all(&here).expect("project dir");
        fs::create_dir_all(&elsewhere).expect("project dir");
        fs::write(elsewhere.join("kept-1.jsonl"), "{}\n").expect("transcript");

        assert_eq!(claude_transcript_exists(&projects, "kept-1"), Some(true));
        assert_eq!(
            claude_transcript_exists(&projects, "never-written"),
            Some(false)
        );
        // No directory to look in is not proof of anything.
        assert_eq!(
            claude_transcript_exists(&projects.join("missing"), "kept-1"),
            None
        );

        fs::remove_dir_all(&projects).ok();
    }

    /// The main account's folder, which every account's profile shares, and
    /// a profile that kept one of its own: each conversation listed once.
    #[test]
    fn claude_sessions_are_read_from_every_accounts_folder_once() {
        let main = temp_dir("claude-main");
        let own = temp_dir("claude-own");
        let project = encode_project_dir(r"C:\repo");
        for (root, id) in [(&main, "shared-1"), (&own, "own-2"), (&own, "shared-1")] {
            let dir = root.join(&project);
            fs::create_dir_all(&dir).expect("project dir");
            fs::write(dir.join(format!("{id}.jsonl")), "{}\n").expect("transcript");
        }
        let roots = [main.clone(), own.clone()];

        let mut ids: Vec<String> = claude_sessions_in(&roots, r"C:\repo")
            .into_iter()
            .map(|session| session.id)
            .collect();
        ids.sort();
        assert_eq!(ids, ["own-2", "shared-1"]);
        assert_eq!(claude_transcript_exists_in(&roots, "own-2"), Some(true));
        assert_eq!(claude_transcript_exists_in(&roots, "nowhere"), Some(false));
        assert_eq!(
            claude_transcript_exists_in(&[main.join("missing")], "own-2"),
            None
        );

        fs::remove_dir_all(&main).ok();
        fs::remove_dir_all(&own).ok();
    }

    /// The label is the first prompt the person typed, not a slash command
    /// or the preamble Claude Code injects before it.
    #[test]
    fn a_session_is_labelled_by_its_first_real_prompt() {
        let root = temp_dir("label");
        let dir = root.join(encode_project_dir("/work/app"));
        fs::create_dir_all(&dir).expect("project dir");
        let transcript = [
            r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"user","message":{"content":[{"type":"text","text":"fix   the bug"}]}}"#,
        ]
        .join("\n");
        fs::write(dir.join("s-1.jsonl"), transcript).expect("transcript");

        let listed = claude_sessions_in(std::slice::from_ref(&root), "/work/app");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "s-1");
        assert_eq!(listed[0].label, "fix the bug");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_id_that_is_not_plain_is_left_to_the_agent() {
        assert_eq!(session_exists("../escape".into()), None);
        assert_eq!(session_exists(String::new()), None);
    }
}
