//! What one tool call reads as on the phone: a line of title, the input that
//! matters, the result — each cut to a few KB — and, for AskUserQuestion, the
//! questions and what the person picked.

use serde_json::Value;

use super::{Answer, Question, QuestionOption};

/// Detail and result are cut here: past a few KB, a phone screen shows the
/// gist and the PC has the rest.
pub const CLIP_BYTES: usize = 4 * 1024;
/// What the person or Claude wrote is kept whole up to this: a pasted log
/// past it would cost every poll of the chat for nothing.
pub const TEXT_BYTES: usize = 32 * 1024;
/// A title is one line.
const TITLE_CHARS: usize = 80;

/// `text` cut to at most `max` bytes, on a character boundary, with an
/// ellipsis to say so.
pub fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The first line of `text`, cut to a title's length.
fn headline(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or("").trim();
    match line.char_indices().nth(TITLE_CHARS) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line.to_string(),
    }
}

/// A path as the person thinks of it: relative to the session's folder when
/// it lies inside, with forward slashes either way.
pub fn shorten(path: &str, cwd: Option<&str>) -> String {
    let path = path.replace('\\', "/");
    if let Some(cwd) = cwd {
        let root = cwd.replace('\\', "/");
        let root = root.trim_end_matches('/');
        // Windows paths come back with the drive letter cased either way.
        if !root.is_empty()
            && path.len() > root.len() + 1
            && path.is_char_boundary(root.len())
            && path[..root.len()].eq_ignore_ascii_case(root)
            && path.as_bytes()[root.len()] == b'/'
        {
            return path[root.len() + 1..].to_string();
        }
    }
    path
}

fn text_of<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

/// One tool call, described.
pub struct Described {
    pub title: String,
    pub detail: Option<String>,
    /// The file a Read call reads: consecutive reads fold into one line.
    pub read: Option<String>,
}

/// `-`/`+` lines of an edit's old and new text.
fn diff(old: &str, new: &str) -> String {
    let mut out = String::new();
    for line in old.lines() {
        out.push_str("- ");
        out.push_str(line);
        out.push('\n');
    }
    for line in new.lines() {
        out.push_str("+ ");
        out.push_str(line);
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out
}

/// The input as compact JSON, for the tools nothing here knows.
fn raw_input(input: &Value) -> Option<String> {
    match input {
        Value::Object(map) if map.is_empty() => None,
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

pub fn describe(tool: &str, input: &Value, cwd: Option<&str>) -> Described {
    let path = |key: &str| text_of(input, key).map(|p| shorten(p, cwd));
    let plain = |title: String, detail: Option<String>| Described {
        title,
        detail,
        read: None,
    };
    let described = match tool {
        "Bash" | "PowerShell" => {
            let command = text_of(input, "command").unwrap_or("");
            plain(
                format!("Ran {}", headline(command)),
                Some(command.to_string()),
            )
        }
        "Read" => {
            let file = path("file_path").unwrap_or_default();
            Described {
                title: format!("Read {file}"),
                detail: None,
                read: Some(file),
            }
        }
        "Edit" => plain(
            format!("Edited {}", path("file_path").unwrap_or_default()),
            Some(diff(
                text_of(input, "old_string").unwrap_or(""),
                text_of(input, "new_string").unwrap_or(""),
            )),
        ),
        "MultiEdit" => {
            let edits = input.get("edits").and_then(Value::as_array);
            let detail = edits.map(|edits| {
                edits
                    .iter()
                    .map(|edit| {
                        diff(
                            text_of(edit, "old_string").unwrap_or(""),
                            text_of(edit, "new_string").unwrap_or(""),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            });
            plain(
                format!("Edited {}", path("file_path").unwrap_or_default()),
                detail,
            )
        }
        "Write" => plain(
            format!("Wrote {}", path("file_path").unwrap_or_default()),
            text_of(input, "content").map(str::to_string),
        ),
        "NotebookEdit" => plain(
            format!("Edited {}", path("notebook_path").unwrap_or_default()),
            text_of(input, "new_source").map(str::to_string),
        ),
        "Grep" | "Glob" => {
            let pattern = text_of(input, "pattern").unwrap_or("");
            let mut detail = pattern.to_string();
            for key in ["path", "glob", "type"] {
                if let Some(value) = text_of(input, key) {
                    let value = if key == "path" {
                        shorten(value, cwd)
                    } else {
                        value.to_string()
                    };
                    detail.push_str(&format!("\n{key}: {value}"));
                }
            }
            plain(format!("Searched for {}", headline(pattern)), Some(detail))
        }
        "WebFetch" => plain(
            format!("Fetched {}", headline(text_of(input, "url").unwrap_or(""))),
            text_of(input, "prompt").map(str::to_string),
        ),
        "WebSearch" => plain(
            format!(
                "Searched the web for {}",
                headline(text_of(input, "query").unwrap_or(""))
            ),
            None,
        ),
        "Agent" | "Task" => plain(
            format!(
                "Ran agent: {}",
                headline(text_of(input, "description").unwrap_or("subagent"))
            ),
            text_of(input, "prompt").map(str::to_string),
        ),
        "TodoWrite" => {
            let todos = input.get("todos").and_then(Value::as_array);
            let detail = todos.map(|todos| {
                todos
                    .iter()
                    .map(|todo| {
                        let mark = match text_of(todo, "status") {
                            Some("completed") => "[x]",
                            Some("in_progress") => "[~]",
                            _ => "[ ]",
                        };
                        format!("{mark} {}", text_of(todo, "content").unwrap_or(""))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
            plain("Updated the to-do list".to_string(), detail)
        }
        "Skill" => plain(
            format!("Used skill {}", text_of(input, "skill").unwrap_or("")),
            text_of(input, "args").map(str::to_string),
        ),
        "ToolSearch" => plain(
            "Loaded tools".to_string(),
            text_of(input, "query").map(str::to_string),
        ),
        "ExitPlanMode" => plain(
            "Presented a plan".to_string(),
            text_of(input, "plan").map(str::to_string),
        ),
        _ => {
            // mcp__<server>__<tool>: the tool's own name says what it did.
            let name = tool
                .strip_prefix("mcp__")
                .and_then(|rest| rest.split_once("__"))
                .map(|(_, name)| name)
                .unwrap_or(tool);
            plain(format!("Used {name}"), raw_input(input))
        }
    };
    Described {
        detail: described
            .detail
            .filter(|detail| !detail.trim().is_empty())
            .map(|detail| clip(&detail, CLIP_BYTES)),
        ..described
    }
}

/// A tool result's text: its text parts, a mark for each picture, the names
/// of the tools a search loaded.
pub fn result_text(content: Option<&Value>) -> String {
    let text = match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match text_of(part, "type") {
                Some("text") => text_of(part, "text").map(str::to_string),
                Some("image") => Some("[image]".to_string()),
                Some("tool_reference") => text_of(part, "tool_name").map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    clip(&text, CLIP_BYTES)
}

/// AskUserQuestion's questions, as its input lists them.
pub fn questions(input: &Value) -> Vec<Question> {
    let Some(list) = input.get("questions").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .map(|question| Question {
            question: text_of(question, "question").unwrap_or("").to_string(),
            header: text_of(question, "header").unwrap_or("").to_string(),
            multi_select: question
                .get("multiSelect")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            options: question
                .get("options")
                .and_then(Value::as_array)
                .map(|options| {
                    options
                        .iter()
                        .map(|option| QuestionOption {
                            label: text_of(option, "label").unwrap_or("").to_string(),
                            description: text_of(option, "description").unwrap_or("").to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// One question's answer as Claude Code wrote it — the labels picked, joined
/// by ", ", or the person's own words — split back into labels and the rest.
/// Labels that hold ", " themselves are matched longest first.
fn split_answer(question: &Question, answer: &str) -> Answer {
    let is_label = |text: &str| question.options.iter().any(|o| o.label == text);
    if is_label(answer) {
        return Answer {
            selected: vec![answer.to_string()],
            other: None,
        };
    }
    let parts: Vec<&str> = answer.split(", ").collect();
    let mut selected = Vec::new();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let found = (i + 1..=parts.len())
            .rev()
            .find(|&j| is_label(&parts[i..j].join(", ")));
        match found {
            Some(j) => {
                selected.push(parts[i..j].join(", "));
                i = j;
            }
            None => {
                rest.push(parts[i]);
                i += 1;
            }
        }
    }
    let other = rest.join(", ");
    Answer {
        selected,
        other: (!other.trim().is_empty()).then_some(other),
    }
}

/// What the person answered, one entry per question. Claude Code keeps the
/// answers by question text in the line's `toolUseResult` (`by_question`); a
/// line without it still has them in the result's text,
/// `"<question>"="<answer>"`.
pub fn answers(questions: &[Question], by_question: Option<&Value>, text: &str) -> Vec<Answer> {
    let by_question = by_question.and_then(Value::as_object);
    questions
        .iter()
        .map(|question| {
            let given = match by_question {
                Some(map) => map
                    .get(&question.question)
                    .and_then(Value::as_str)
                    .map(str::to_string),
                None => quoted_answer(text, &question.question),
            };
            match given {
                Some(answer) => split_answer(question, &answer),
                None => Answer {
                    selected: Vec::new(),
                    other: None,
                },
            }
        })
        .collect()
}

/// `"<question>"="<answer>"` out of the result's text: the answer runs to the
/// quote that closes it, before the next pair or the end of the sentence.
fn quoted_answer(text: &str, question: &str) -> Option<String> {
    let key = format!("\"{question}\"=\"");
    let start = text.find(&key)? + key.len();
    let rest = &text[start..];
    let end = [rest.find("\", \""), rest.find("\". "), rest.rfind('"')]
        .into_iter()
        .flatten()
        .min()?;
    Some(rest[..end].to_string())
}
