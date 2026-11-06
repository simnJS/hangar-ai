//! Readable one-line titles for Hangar's own MCP calls.
//!
//! The core's chat titles a `mcp__hangar__*` call by its tool's name ("Used
//! board_create_task"), with the call's input as raw JSON for its detail.
//! The orchestrator's conversation is mostly these, so the chat view reads
//! the input and says what the call did ("Created task “…”", "Started worker
//! Ada on “…”"), in the person's language. A task named by its id takes its
//! title from the workspace's board when the board has it. Any other tool,
//! or an input the core clipped too short to read, keeps the core's title.

use hangar_i18n::{Key, Translator, args};
use serde_json::Value;

/// The prefix of Hangar's own tools as Claude Code names them.
const HANGAR: &str = "mcp__hangar__";

/// The words of a call's title: the readable one for Hangar's tools, the
/// core's otherwise. `task_title` looks a task up by id.
pub fn title(
    i18n: Translator,
    tool: &str,
    core_title: &str,
    detail: Option<&str>,
    task_title: &dyn Fn(&str) -> Option<String>,
) -> String {
    readable(i18n, tool, detail, task_title).unwrap_or_else(|| core_title.to_string())
}

/// The readable title of a Hangar call, when its input reads.
fn readable(
    i18n: Translator,
    tool: &str,
    detail: Option<&str>,
    task_title: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let name = tool.strip_prefix(HANGAR)?;
    // No input reads as an empty object: the tools that take none.
    let input: Value = match detail {
        Some(detail) => serde_json::from_str(detail).ok()?,
        None => Value::Object(Default::default()),
    };
    let field = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    // A task by its id: its title when the board has it, else the id's head.
    let task = |key: &str| {
        let id = field(key)?;
        Some(task_title(&id).unwrap_or_else(|| short_id(&id)))
    };
    let with_task = |key: Key, by: &str| {
        let task = task(by)?;
        Some(
            i18n.t_with(key, &args!("task" => task.as_str()))
                .into_owned(),
        )
    };
    let with_name =
        |key: Key, name: &str| Some(i18n.t_with(key, &args!("name" => name)).into_owned());
    match name {
        "board_create_task" => {
            let title = field("title")?;
            Some(
                i18n.t_with(
                    Key::ChatConvToolCreateTask,
                    &args!("task" => title.as_str()),
                )
                .into_owned(),
            )
        }
        "board_update_task" => {
            let task = task("id")?;
            match field("column") {
                Some(column) => Some(
                    i18n.t_with(
                        Key::ChatConvToolMoveTask,
                        &args!("task" => task.as_str(), "column" => column.as_str()),
                    )
                    .into_owned(),
                ),
                None => Some(
                    i18n.t_with(Key::ChatConvToolUpdateTask, &args!("task" => task.as_str()))
                        .into_owned(),
                ),
            }
        }
        "board_comment_task" => with_task(Key::ChatConvToolCommentTask, "id"),
        "board_claim_task" => with_task(Key::ChatConvToolClaimTask, "id"),
        "board_delete_task" => with_task(Key::ChatConvToolDeleteTask, "id"),
        "board_block_task" => with_task(Key::ChatConvToolBlockTask, "id"),
        "board_unblock_task" => with_task(Key::ChatConvToolUnblockTask, "id"),
        "board_report_progress" => with_task(Key::ChatConvToolProgressTask, "id"),
        "board_list_tasks" => Some(i18n.t(Key::ChatConvToolListTasks).into_owned()),
        "board_next_task" => Some(i18n.t(Key::ChatConvToolNextTask).into_owned()),
        "worker_spawn" => {
            let on = task("task")?;
            match field("name") {
                Some(worker) => Some(
                    i18n.t_with(
                        Key::ChatConvToolSpawnWorker,
                        &args!("name" => worker.as_str(), "task" => on.as_str()),
                    )
                    .into_owned(),
                ),
                None => Some(
                    i18n.t_with(
                        Key::ChatConvToolSpawnAnyWorker,
                        &args!("task" => on.as_str()),
                    )
                    .into_owned(),
                ),
            }
        }
        "worker_stop" => with_name(Key::ChatConvToolStopWorker, &field("worker")?),
        "worker_list" => Some(i18n.t(Key::ChatConvToolListWorkers).into_owned()),
        "pane_send" => with_name(Key::ChatConvToolSendPane, &field("pane")?),
        "pane_read" => with_name(Key::ChatConvToolReadPane, &field("pane")?),
        "pane_create" => match field("name") {
            Some(pane) => with_name(Key::ChatConvToolCreatePane, &pane),
            None => Some(i18n.t(Key::ChatConvToolCreateAnyPane).into_owned()),
        },
        "memory_search" => {
            let query = field("query")?;
            Some(
                i18n.t_with(
                    Key::ChatConvToolSearchMemory,
                    &args!("query" => query.as_str()),
                )
                .into_owned(),
            )
        }
        "memory_write" | "memory_update" => Some(i18n.t(Key::ChatConvToolWriteMemory).into_owned()),
        "memory_read" | "memory_list" => Some(i18n.t(Key::ChatConvToolReadMemory).into_owned()),
        "delivery_submit" => Some(i18n.t(Key::ChatConvToolSubmitDelivery).into_owned()),
        "delivery_status" => Some(i18n.t(Key::ChatConvToolDeliveryStatus).into_owned()),
        "preview_show" => Some(i18n.t(Key::ChatConvToolShowPreview).into_owned()),
        _ => None,
    }
}

/// The head of an id, as the board shows one it cannot name.
fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// A call's detail as the person reads it expanded: Hangar's input JSON
/// pretty-printed, anything else as it is.
pub fn detail(tool: &str, detail: &str) -> String {
    if tool.starts_with(HANGAR)
        && let Ok(input) = serde_json::from_str::<Value>(detail)
        && let Ok(pretty) = serde_json::to_string_pretty(&input)
    {
        return pretty;
    }
    detail.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    fn en() -> Translator {
        Translator::new(Locale::En)
    }

    fn known(id: &str) -> Option<String> {
        (id == "af17e40a-12f7").then(|| "Chat view".to_string())
    }

    fn of(tool: &str, detail: Option<&str>) -> String {
        title(en(), tool, "Used it", detail, &known)
    }

    #[test]
    fn hangar_calls_read_as_what_they_did() {
        assert_eq!(
            of(
                "mcp__hangar__board_create_task",
                Some(r#"{"title":"Add a search box"}"#)
            ),
            "Created task “Add a search box”"
        );
        assert_eq!(
            of(
                "mcp__hangar__worker_spawn",
                Some(r#"{"task":"af17e40a-12f7","name":"Ada"}"#)
            ),
            "Started worker Ada on “Chat view”"
        );
        assert_eq!(
            of(
                "mcp__hangar__board_comment_task",
                Some(r#"{"id":"0123456789abcdef","text":"ok"}"#)
            ),
            "Commented on “01234567”",
            "a task the board does not have: the id's head"
        );
        assert_eq!(
            of(
                "mcp__hangar__board_update_task",
                Some(r#"{"id":"af17e40a-12f7","column":"review"}"#)
            ),
            "Moved “Chat view” to review"
        );
        assert_eq!(of("mcp__hangar__board_list_tasks", None), "Read the board");
    }

    #[test]
    fn french_titles_too() {
        let fr = Translator::new(Locale::Fr);
        let words = title(
            fr,
            "mcp__hangar__board_create_task",
            "Used board_create_task",
            Some(r#"{"title":"Recherche"}"#),
            &known,
        );
        assert!(
            words.contains("Recherche") && words.starts_with("Tâche"),
            "{words}"
        );
    }

    #[test]
    fn other_tools_and_unreadable_inputs_keep_the_core_title() {
        assert_eq!(of("Bash", Some("ls")), "Used it");
        assert_eq!(of("mcp__other__thing", Some("{}")), "Used it");
        // Clipped by the core: no longer JSON.
        assert_eq!(
            of(
                "mcp__hangar__board_create_task",
                Some(r#"{"title":"Add a sea"#)
            ),
            "Used it"
        );
        assert_eq!(of("mcp__hangar__board_create_task", Some("{}")), "Used it");
    }

    #[test]
    fn the_detail_of_a_hangar_call_is_pretty_printed() {
        assert_eq!(
            detail("mcp__hangar__pane_send", r#"{"pane":"Ada","text":"go"}"#),
            "{\n  \"pane\": \"Ada\",\n  \"text\": \"go\"\n}"
        );
        assert_eq!(detail("Bash", "ls -la"), "ls -la");
    }
}
