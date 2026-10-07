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

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Extracts a display label from a `.jsonl` transcript by finding the first
/// real user message. Reads only the head of the file.
fn label_from_jsonl(path: &Path, user_field: &str) -> String {
    let Ok(file) = fs::File::open(path) else {
        return String::new();
    };
    let reader = BufReader::new(file);
    for line in reader.lines().take(60).map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let is_user = value.get("type").and_then(|t| t.as_str()) == Some(user_field)
            || value
                .get("payload")
                .and_then(|p| p.get("role"))
                .and_then(|r| r.as_str())
                == Some("user");

        if !is_user {
            continue;
        }

        let content = value
            .get("message")
            .and_then(|m| m.get("content"))
            .or_else(|| value.get("payload").and_then(|p| p.get("content")));

        let text = match content {
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

fn claude_sessions(cwd: &str) -> Vec<AgentSession> {
    let Some(home) = home() else {
        return vec![];
    };
    let dir = home
        .join(".claude")
        .join("projects")
        .join(encode_project_dir(cwd));

    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };

    let mut sessions: Vec<AgentSession> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .map(|p| AgentSession {
            id: p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            label: label_from_jsonl(&p, "user"),
            modified_ms: modified_ms(&p),
        })
        .collect();

    sessions.sort_by_key(|s| std::cmp::Reverse(s.modified_ms));
    sessions
}

/// Walks `~/.codex/sessions/YYYY/MM/DD/` and keeps rollouts whose recorded
/// working directory matches the pane's.
fn codex_sessions(cwd: &str) -> Vec<AgentSession> {
    let Some(home) = home() else {
        return vec![];
    };
    let root = home.join(".codex").join("sessions");

    let mut files: Vec<PathBuf> = Vec::new();
    collect_jsonl(&root, &mut files, 0);
    files.sort_by_key(|p| std::cmp::Reverse(modified_ms(p)));
    files.truncate(80);

    let target = cwd.replace('\\', "/").to_lowercase();

    files
        .into_iter()
        .filter_map(|path| {
            let file = fs::File::open(&path).ok()?;
            let mut first = String::new();
            BufReader::new(file).read_line(&mut first).ok()?;
            let meta: serde_json::Value = serde_json::from_str(&first).ok()?;

            let session_cwd = meta
                .get("payload")
                .and_then(|p| p.get("cwd"))
                .or_else(|| meta.get("cwd"))
                .and_then(|c| c.as_str())?;

            if session_cwd.replace('\\', "/").to_lowercase() != target {
                return None;
            }

            let id = meta
                .get("payload")
                .and_then(|p| p.get("id"))
                .or_else(|| meta.get("id"))
                .and_then(|i| i.as_str())
                .map(str::to_string)
                .or_else(|| uuid_from_filename(&path))?;

            Some(AgentSession {
                id,
                label: label_from_jsonl(&path, "user"),
                modified_ms: modified_ms(&path),
            })
        })
        .collect()
}

/// `rollout-2025-01-01T10-00-00-<uuid>.jsonl` -> `<uuid>`
fn uuid_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy();
    let parts: Vec<&str> = stem.split('-').collect();
    if parts.len() < 5 {
        return None;
    }
    Some(parts[parts.len() - 5..].join("-"))
}

pub(crate) fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out, depth + 1);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

#[tauri::command]
pub fn list_sessions(agent: String, cwd: String) -> Vec<AgentSession> {
    match agent.as_str() {
        "claude" => claude_sessions(&cwd),
        "codex" => codex_sessions(&cwd),
        _ => vec![],
    }
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
#[tauri::command]
pub fn session_exists(agent: String, id: String) -> Option<bool> {
    // Joined into a path below: anything but a plain id is not ours to judge.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let home = home()?;
    match agent.as_str() {
        "claude" => claude_transcript_exists(&home.join(".claude").join("projects"), &id),
        "codex" => codex_transcript_exists(&home.join(".codex").join("sessions"), &id),
        _ => None,
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

fn codex_transcript_exists(root: &Path, id: &str) -> Option<bool> {
    // An unreadable root says nothing about the session; an empty walk would.
    fs::read_dir(root).ok()?;
    let mut files = Vec::new();
    collect_jsonl(root, &mut files, 0);
    Some(files.iter().any(|path| {
        path.file_stem()
            .is_some_and(|stem| stem.to_string_lossy().ends_with(id))
    }))
}

/// Reports which agent CLIs are actually on PATH, so the UI can grey out the rest.
#[tauri::command]
pub fn detect_agents() -> Vec<String> {
    // Without this the answer on macOS is "none of them": a bundled app is
    // handed the bare system PATH, and no agent CLI installs into it.
    crate::path_env::ensure();

    let candidates: [(&str, &[&str]); 4] = [
        ("claude", &["claude.exe", "claude.cmd", "claude"]),
        ("codex", &["codex.exe", "codex.cmd", "codex"]),
        ("gemini", &["gemini.exe", "gemini.cmd", "gemini"]),
        ("opencode", &["opencode.exe", "opencode.cmd", "opencode"]),
    ];

    let paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();

    candidates
        .iter()
        .filter(|(_, bins)| {
            bins.iter()
                .any(|bin| paths.iter().any(|dir| dir.join(bin).is_file()))
        })
        .map(|(name, _)| name.to_string())
        .collect()
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

    #[test]
    fn a_codex_session_is_found_by_the_id_ending_its_rollout_name() {
        let root = temp_dir("codex");
        let day = root.join("2026").join("10").join("07");
        fs::create_dir_all(&day).expect("day dir");
        fs::write(
            day.join("rollout-2026-10-07T10-00-00-0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b.jsonl"),
            "{}\n",
        )
        .expect("rollout");

        assert_eq!(
            codex_transcript_exists(&root, "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"),
            Some(true)
        );
        assert_eq!(
            codex_transcript_exists(&root, "0199a1b2-0000-7e5f-8a9b-0c1d2e3f4a5b"),
            Some(false)
        );
        assert_eq!(codex_transcript_exists(&root.join("missing"), "x"), None);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_id_that_is_not_plain_is_left_to_the_agent() {
        assert_eq!(session_exists("claude".into(), "../escape".into()), None);
        assert_eq!(session_exists("claude".into(), String::new()), None);
        assert_eq!(session_exists("gemini".into(), "abc".into()), None);
    }
}
