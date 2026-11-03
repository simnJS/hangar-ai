//! How a tool call reads on the phone before the transcript has it
//! (`src/remote/calls.ts`): Claude Code writes a call there only once it has
//! its result, so a call running, a permission dialog open and a question
//! waiting are known only from what the hangar-bridge mod reports. Titled as
//! core's `chat/tools.rs` titles the transcript's calls ("Ran pnpm test"); a
//! permission asks in the imperative ("Run pnpm test").
//!
//! The inventory proposed this for core, beside `chat/tools.rs`; it stays
//! here, where its only caller is, and core's titling module stays private.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use hangar_core::chat::{Question, QuestionOption};

/// As chat/tools.rs: a detail past a few KB shows the gist, the PC has the
/// rest. Counted in characters, as the TS counted its string's length.
const CLIP_CHARS: usize = 4 * 1024;
const TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribedCall {
    /// As a done or running action: "Ran pnpm test".
    pub title: String,
    /// As a permission asks it: "Run pnpm test".
    pub ask: String,
    /// The input that matters: the command, the diff, the pattern.
    pub detail: Option<String>,
}

fn clip(text: &str) -> String {
    if text.chars().count() <= CLIP_CHARS {
        return text.to_string();
    }
    let kept: String = text.chars().take(CLIP_CHARS).collect();
    format!("{kept}…")
}

/// The first line, cut to a title's length.
fn headline(text: &str) -> String {
    let line = text.trim().split('\n').next().unwrap_or("").trim();
    if line.chars().count() > TITLE_CHARS {
        let kept: String = line.chars().take(TITLE_CHARS).collect();
        format!("{kept}…")
    } else {
        line.to_string()
    }
}

/// Relative to the session's folder when it lies inside, with forward
/// slashes either way.
fn shorten(path: &str, cwd: Option<&str>) -> String {
    let slashed = path.replace('\\', "/");
    let root = cwd.unwrap_or("").replace('\\', "/");
    let root = root.trim_end_matches('/');
    if !root.is_empty()
        && slashed.len() > root.len() + 1
        && slashed.is_char_boundary(root.len())
        && slashed.as_bytes()[root.len()] == b'/'
        // Windows paths come back with the drive letter cased either way.
        && slashed[..root.len()].to_lowercase() == root.to_lowercase()
    {
        return slashed[root.len() + 1..].to_string();
    }
    slashed
}

/// Lines, a last line break left out; none for an empty text.
fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// `-`/`+` lines of an edit's old and new text.
fn diff(old: &str, new: &str) -> String {
    let lines: Vec<String> = lines_of(old)
        .into_iter()
        .map(|line| format!("- {line}"))
        .chain(lines_of(new).into_iter().map(|line| format!("+ {line}")))
        .collect();
    lines.join("\n").trim_end().to_string()
}

/// A value as JavaScript's `String(value ?? "")` writes it.
fn string_of(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(_)) => "[object Object]".into(),
        Some(other) => other.to_string(),
    }
}

static MCP_TOOL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^mcp__.+?__(.+)$").unwrap());

pub fn describe_call(tool: &str, input: &Value, cwd: Option<&str>) -> DescribedCall {
    let empty = Map::new();
    let args = input.as_object().unwrap_or(&empty);
    let text = |key: &str| args.get(key).and_then(Value::as_str);
    let path = |key: &str| shorten(text(key).unwrap_or(""), cwd);
    let said = |title: String, ask: String, detail: Option<&str>| DescribedCall {
        title,
        ask,
        detail: detail.filter(|detail| !detail.trim().is_empty()).map(clip),
    };
    match tool {
        "Bash" | "PowerShell" => {
            let command = text("command").unwrap_or("");
            said(
                format!("Ran {}", headline(command)),
                format!("Run {}", headline(command)),
                Some(command),
            )
        }
        "Read" => said(
            format!("Read {}", path("file_path")),
            format!("Read {}", path("file_path")),
            None,
        ),
        "Edit" => said(
            format!("Edited {}", path("file_path")),
            format!("Edit {}", path("file_path")),
            Some(&diff(
                text("old_string").unwrap_or(""),
                text("new_string").unwrap_or(""),
            )),
        ),
        "MultiEdit" => {
            let edits = args.get("edits").and_then(Value::as_array);
            let detail = edits
                .into_iter()
                .flatten()
                .map(|edit| {
                    diff(
                        &string_of(edit.get("old_string")),
                        &string_of(edit.get("new_string")),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            said(
                format!("Edited {}", path("file_path")),
                format!("Edit {}", path("file_path")),
                Some(&detail),
            )
        }
        "Write" => said(
            format!("Wrote {}", path("file_path")),
            format!("Write {}", path("file_path")),
            text("content"),
        ),
        "NotebookEdit" => said(
            format!("Edited {}", path("notebook_path")),
            format!("Edit {}", path("notebook_path")),
            text("new_source"),
        ),
        "Grep" | "Glob" => {
            let pattern = text("pattern").unwrap_or("");
            let mut detail = vec![pattern.to_string()];
            for key in ["path", "glob", "type"] {
                if let Some(value) = text(key) {
                    let value = if key == "path" {
                        shorten(value, cwd)
                    } else {
                        value.to_string()
                    };
                    detail.push(format!("{key}: {value}"));
                }
            }
            said(
                format!("Searched for {}", headline(pattern)),
                format!("Search for {}", headline(pattern)),
                Some(&detail.join("\n")),
            )
        }
        "WebFetch" => {
            let url = headline(text("url").unwrap_or(""));
            said(
                format!("Fetched {url}"),
                format!("Fetch {url}"),
                text("prompt"),
            )
        }
        "WebSearch" => {
            let query = headline(text("query").unwrap_or(""));
            said(
                format!("Searched the web for {query}"),
                format!("Search the web for {query}"),
                None,
            )
        }
        "Agent" | "Task" => {
            let what = headline(text("description").unwrap_or("subagent"));
            said(
                format!("Ran agent: {what}"),
                format!("Run agent: {what}"),
                text("prompt"),
            )
        }
        "TodoWrite" => {
            let todos = args.get("todos").and_then(Value::as_array);
            let detail = todos
                .into_iter()
                .flatten()
                .map(|todo| {
                    let mark = match todo.get("status").and_then(Value::as_str) {
                        Some("completed") => "[x]",
                        Some("in_progress") => "[~]",
                        _ => "[ ]",
                    };
                    format!("{mark} {}", string_of(todo.get("content")))
                })
                .collect::<Vec<_>>()
                .join("\n");
            said(
                "Updated the to-do list".into(),
                "Update the to-do list".into(),
                Some(&detail),
            )
        }
        "Skill" => {
            let skill = text("skill").unwrap_or("");
            said(
                format!("Used skill {skill}"),
                format!("Use skill {skill}"),
                text("args"),
            )
        }
        "ToolSearch" => said("Loaded tools".into(), "Load tools".into(), text("query")),
        "ExitPlanMode" => said(
            "Presented a plan".into(),
            "Present a plan".into(),
            text("plan"),
        ),
        _ => {
            // mcp__<server>__<tool>: the tool's own name says what it does.
            let name = MCP_TOOL
                .captures(tool)
                .and_then(|caps| caps.get(1))
                .map_or(tool, |name| name.as_str());
            let raw = match input {
                Value::Object(map) if !map.is_empty() => Some(input.to_string()),
                Value::Array(list) if !list.is_empty() => Some(input.to_string()),
                _ => None,
            };
            said(
                format!("Used {name}"),
                format!("Use {name}"),
                raw.as_deref(),
            )
        }
    }
}

/// AskUserQuestion's questions, as its input lists them, whatever else they
/// carry (a preview of an option, for one).
pub fn asked_questions(input: &Value) -> Vec<Question> {
    let Some(list) = input.get("questions").and_then(Value::as_array) else {
        return Vec::new();
    };
    let text_of = |value: Option<&Value>| value.and_then(Value::as_str).unwrap_or("").to_string();
    list.iter()
        .map(|entry| Question {
            question: text_of(entry.get("question")),
            header: text_of(entry.get("header")),
            multi_select: entry.get("multiSelect") == Some(&Value::Bool(true)),
            options: entry
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|option| QuestionOption {
                    label: text_of(option.get("label")),
                    description: text_of(option.get("description")),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! calls.test.ts, case for case.

    use serde_json::json;

    use super::*;

    #[test]
    fn titles_it_as_chat_tools_rs_does_and_as_a_permission_asks_it() {
        let cwd = Some("C:\\dev\\shop");
        let cases: Vec<(&str, Value, &str, &str, Option<&str>)> = vec![
            (
                "Bash",
                json!({"command": "pnpm test\npnpm build"}),
                "Ran pnpm test",
                "Run pnpm test",
                Some("pnpm test\npnpm build"),
            ),
            (
                "PowerShell",
                json!({"command": "Get-ChildItem"}),
                "Ran Get-ChildItem",
                "Run Get-ChildItem",
                Some("Get-ChildItem"),
            ),
            (
                "Read",
                json!({"file_path": "C:\\dev\\shop\\src\\a.ts"}),
                "Read src/a.ts",
                "Read src/a.ts",
                None,
            ),
            (
                "Write",
                json!({"file_path": "D:\\other\\b.md", "content": "# B"}),
                "Wrote D:/other/b.md",
                "Write D:/other/b.md",
                Some("# B"),
            ),
            (
                "MultiEdit",
                json!({"file_path": "c:\\DEV\\shop\\a.ts", "edits": [
                    {"old_string": "x", "new_string": "y"},
                    {"old_string": "", "new_string": "z\n"}]}),
                "Edited a.ts",
                "Edit a.ts",
                Some("- x\n+ y\n\n+ z"),
            ),
            (
                "NotebookEdit",
                json!({"notebook_path": "C:\\dev\\shop\\n.ipynb", "new_source": "print(1)"}),
                "Edited n.ipynb",
                "Edit n.ipynb",
                Some("print(1)"),
            ),
            (
                "Grep",
                json!({"pattern": "TODO", "path": "C:\\dev\\shop\\src", "glob": "*.ts"}),
                "Searched for TODO",
                "Search for TODO",
                Some("TODO\npath: src\nglob: *.ts"),
            ),
            (
                "WebFetch",
                json!({"url": "https://example.com", "prompt": "Summary"}),
                "Fetched https://example.com",
                "Fetch https://example.com",
                Some("Summary"),
            ),
            (
                "WebSearch",
                json!({"query": "tauri 2"}),
                "Searched the web for tauri 2",
                "Search the web for tauri 2",
                None,
            ),
            (
                "Agent",
                json!({"description": "Find the bug", "prompt": "Look"}),
                "Ran agent: Find the bug",
                "Run agent: Find the bug",
                Some("Look"),
            ),
            (
                "TodoWrite",
                json!({"todos": [
                    {"content": "A", "status": "completed"},
                    {"content": "B", "status": "in_progress"},
                    {"content": "C", "status": "pending"}]}),
                "Updated the to-do list",
                "Update the to-do list",
                Some("[x] A\n[~] B\n[ ] C"),
            ),
            (
                "Skill",
                json!({"skill": "simplify"}),
                "Used skill simplify",
                "Use skill simplify",
                None,
            ),
            (
                "mcp__hangar__pane_send",
                json!({"pane": "Ava"}),
                "Used pane_send",
                "Use pane_send",
                Some("{\"pane\":\"Ava\"}"),
            ),
            ("Mystery", json!({}), "Used Mystery", "Use Mystery", None),
        ];
        for (tool, input, title, ask, detail) in cases {
            assert_eq!(
                describe_call(tool, &input, cwd),
                DescribedCall {
                    title: title.into(),
                    ask: ask.into(),
                    detail: detail.map(str::to_string),
                },
                "{tool}"
            );
        }
    }

    #[test]
    fn cuts_a_long_title_to_a_line_and_a_long_detail_to_a_few_kb() {
        let command = format!("echo {}", "x".repeat(100));
        let described = describe_call(
            "Bash",
            &json!({"command": format!("{command}\n{}", "y".repeat(5000))}),
            None,
        );
        assert_eq!(described.title, format!("Ran {}…", &command[..80]));
        let detail = described.detail.unwrap();
        assert_eq!(detail.chars().count(), 4096 + 1);
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn reads_ask_user_question_s_questions_whatever_else_they_carry() {
        assert_eq!(
            asked_questions(&json!({"questions": [
                {"question": "Which?", "header": "Pick", "multiSelect": true,
                 "options": [{"label": "A", "description": "First", "preview": "<p>"}, {"label": "B"}]},
                null,
            ]})),
            vec![
                Question {
                    question: "Which?".into(),
                    header: "Pick".into(),
                    multi_select: true,
                    options: vec![
                        QuestionOption {
                            label: "A".into(),
                            description: "First".into()
                        },
                        QuestionOption {
                            label: "B".into(),
                            description: String::new()
                        },
                    ],
                },
                Question {
                    question: String::new(),
                    header: String::new(),
                    multi_select: false,
                    options: Vec::new(),
                },
            ]
        );
        assert_eq!(asked_questions(&json!({})), Vec::new());
        assert_eq!(asked_questions(&Value::Null), Vec::new());
    }
}
