//! MCP server exposed over stdio.
//!
//! Runs inside the same executable as the GUI (`hangar-ai --mcp`), which keeps
//! agent configuration trivial: one absolute path, no sidecar to bundle or
//! locate. Every call is proxied to the running app's HTTP API so that task
//! claims stay atomic across all panes.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// First of `names` that is set and not empty.
///
/// The list exists for the rename: configs written before it still export the
/// `IABENCH_*` variables and still launch this binary, so the old names have to
/// keep working. Only the new name is ever written.
fn env_var(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

/// Workspace the caller is acting on: an explicit override, else the working
/// directory the agent was launched in — which is the workspace root.
fn workspace_cwd() -> String {
    env_var(&["HANGAR_WORKSPACE", "IABENCH_WORKSPACE"])
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// The name an agent signs with when it gives none. The pane's own name comes
/// before the generic fallback: it is what the board's assignee chip can lead
/// back to a pane with, and what pane_list calls that agent.
fn agent_name() -> String {
    env_var(&["HANGAR_AGENT", "IABENCH_AGENT", "HANGAR_PANE_NAME"])
        .unwrap_or_else(|| "agent".to_string())
}

struct Api {
    base: String,
    token: String,
    cwd: String,
}

impl Api {
    fn connect() -> Result<Self, String> {
        let endpoint =
            crate::endpoint::read().ok_or("Hangar.AI is not running (no endpoint file found)")?;
        Ok(Api {
            base: format!("http://127.0.0.1:{}", endpoint.port),
            token: endpoint.token,
            cwd: workspace_cwd(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}{}{}cwd={}",
            self.base,
            path,
            if path.contains('?') { "&" } else { "?" },
            urlencode(&self.cwd)
        )
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        read_api_response(
            ureq::get(&self.url(path))
                .config()
                .http_status_as_error(false)
                .build()
                .header("authorization", &format!("Bearer {}", self.token))
                .call()
                .map_err(|e| e.to_string())?,
        )
    }

    fn send(&self, method: &str, path: &str, body: Value) -> Result<Value, String> {
        // ureq types requests by whether they carry a body, so DELETE cannot
        // share this builder chain.
        let request = match method {
            "POST" => ureq::post(&self.url(path)),
            "PATCH" => ureq::patch(&self.url(path)),
            _ => return Err(format!("unsupported method {method}")),
        };
        read_api_response(
            request
                .config()
                .http_status_as_error(false)
                .build()
                .header("authorization", &format!("Bearer {}", self.token))
                .send_json(body)
                .map_err(|e| e.to_string())?,
        )
    }

    fn delete(&self, path: &str) -> Result<Value, String> {
        read_api_response(
            ureq::delete(&self.url(path))
                .config()
                .http_status_as_error(false)
                .build()
                .header("authorization", &format!("Bearer {}", self.token))
                .call()
                .map_err(|e| e.to_string())?,
        )
    }
}

/// The server's own words, not ureq's. With statuses left as errors, a 400
/// surfaces as the string "http status: 400" and the diagnostic the route put
/// in the body — "already claimed by claude-2", "content is 25000 characters,
/// the limit is 20000" — never reaches the model, which is left to retry the
/// identical call. Statuses are turned off above so the body survives the hop;
/// this reads it and hands back whichever half carries the answer.
fn read_api_response(mut response: ureq::http::Response<ureq::Body>) -> Result<Value, String> {
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;

    if status.is_success() {
        return serde_json::from_str(&text).map_err(|e| e.to_string());
    }

    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|payload| {
            payload
                .get("error")
                .and_then(|message| message.as_str())
                .map(str::to_string)
        })
        // axum's own rejections — a body that would not deserialize — are
        // plain text, and still say more than the number alone.
        .or_else(|| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .unwrap_or_else(|| format!("http status: {status}"));
    Err(message)
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn tool_definitions() -> Value {
    let workspace_arg = json!({
        "type": "string",
        "description": "Workspace name or folder. Defaults to the one you are working in."
    });
    let pane_arg = json!({
        "type": "string",
        "description": "Pane name, as pane_list shows it (an id works too)."
    });
    json!([
        {
            "name": "board_list_tasks",
            "description": "List every task on the workspace board, with column, priority, assignee and comments. Call this first to understand the current state of the work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "column": {
                        "type": "string",
                        "enum": ["todo", "doing", "review", "done"],
                        "description": "Optional filter."
                    }
                }
            }
        },
        {
            "name": "board_next_task",
            "description": "Return the highest-priority unassigned task in 'todo' whose dependencies are all done. Use this to pick up work without colliding with other agents.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "board_claim_task",
            "description": "Take ownership of a task and move it to 'doing'. Fails if another agent already holds it — pick a different task in that case. Always claim before starting work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "agent": { "type": "string", "description": "Your identity, e.g. 'claude-1'. Defaults to the pane name." }
                },
                "required": ["id"]
            }
        },
        {
            "name": "board_create_task",
            "description": "Add a task to the board. Use it to split work you discovered into items other agents can pick up.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "column": { "type": "string", "enum": ["todo", "doing", "review", "done"] },
                    "priority": { "type": "integer", "description": "Higher is more urgent. Default 1." },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "depends_on": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Task ids that must be done first."
                    }
                },
                "required": ["title"]
            }
        },
        {
            "name": "board_update_task",
            "description": "Change a task: move it between columns, edit the title or description, adjust priority, or hand it back with release=true. Move a task to 'review' or 'done' when you finish it so other agents see the progress.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "column": { "type": "string", "enum": ["todo", "doing", "review", "done"] },
                    "priority": { "type": "integer" },
                    "assignee": { "type": "string" },
                    "release": { "type": "boolean", "description": "Unassign the task so someone else can take it." }
                },
                "required": ["id"]
            }
        },
        {
            "name": "board_comment_task",
            "description": "Leave a note on a task. This is how agents talk to each other: record decisions, blockers, or what you finished.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "text": { "type": "string" },
                    "author": { "type": "string" }
                },
                "required": ["id", "text"]
            }
        },
        {
            "name": "board_delete_task",
            "description": "Remove a task from the board permanently.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        },
        {
            "name": "memory_search",
            "description": "Search the shared long-term memory: facts you and other agents recorded in earlier sessions, in this project and in every other one. Run this before you start a task and whenever you are about to investigate something that feels like it has been solved before — the answer may already be written down. Terms are ANDed, so add words to narrow the search. Results carry an excerpt, not the full text; call memory_read on the one you want.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Words to look for in titles, contents and tags." },
                    "tag": { "type": "string", "description": "Only entries carrying this tag." },
                    "workspace": { "type": "string", "description": "Only entries written from a workspace path containing this text — use it to narrow to one project." },
                    "limit": { "type": "integer", "description": "Default 20, maximum 100." }
                },
                "required": ["query"]
            }
        },
        {
            "name": "memory_write",
            "description": "Record something worth knowing next time: a decision and why it was taken, a trap in this codebase, a convention, a command that turned out to be the right one. Write facts that stay true after the current task is over — progress on a task belongs in a board comment instead. Writing a title that already exists REPLACES that entry, so re-recording something you refined keeps one good note instead of five near-duplicates. Keep the title specific enough to be searched for.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short and specific, e.g. 'ONNX cannot be shipped from this CI'. Max 200 characters." },
                    "content": { "type": "string", "description": "The fact itself, and enough context for it to be useful to someone who was not here. Max 20000 characters." },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Up to 10 short labels to search by, e.g. ['rust', 'ci']."
                    }
                },
                "required": ["title", "content"]
            }
        },
        {
            "name": "memory_update",
            "description": "Correct an entry you found with memory_search or memory_list, by id. Use it when a recorded fact has become wrong or incomplete — a stale memory is worse than none. Only the fields you pass are changed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string" },
                    "content": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["id"]
            }
        },
        {
            "name": "memory_list",
            "description": "List what is in the memory, titles and tags only, without the contents. Use it to get a feel for what has been recorded, or to find an id to read or delete; prefer memory_search when you have a question to answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tag": { "type": "string", "description": "Only entries carrying this tag." },
                    "workspace": { "type": "string", "description": "Only entries written from a workspace path containing this text." }
                }
            }
        },
        {
            "name": "memory_read",
            "description": "Read one memory entry in full, by id. This is the follow-up to memory_search when an excerpt looks like the answer.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        },
        {
            "name": "memory_delete",
            "description": "Delete a memory entry permanently. Use it for facts that are no longer true and cannot be corrected — an entry about code that no longer exists, for instance. When the fact is merely out of date, memory_update is the better move.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        },
        {
            "name": "workspace_list",
            "description": "List the workspaces open in Hangar.AI — name, folder, number of panes — and which one you are in. The pane tools act on your own workspace unless you name another one.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "pane_list",
            "description": "List the panes (terminals) of a workspace: name, the agent running in it, its folder, whether its process is alive, and — when Hangar can tell — what the agent is doing: working, waiting (for a permission or an answer), yours (finished its turn), idle. Every other pane tool addresses panes by these names. Your own pane is marked you=true.",
            "inputSchema": {
                "type": "object",
                "properties": { "workspace": workspace_arg }
            }
        },
        {
            "name": "pane_create",
            "description": "Open a new pane in a workspace and start an agent in it, or a plain shell. Use it to hand work to another agent: with a prompt, a Claude Code pane starts on it as soon as it is ready; for other agents, send the prompt with pane_send once pane_read shows them started. Give a cwd (a worktree, for instance) to keep two agents out of each other's files. A workspace holds at most 16 panes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "enum": ["claude", "codex", "gemini", "opencode", "shell"],
                        "description": "What to run. Defaults to the agent of the pane it is split from."
                    },
                    "name": { "type": "string", "description": "Name for the pane, unique in its workspace. Defaults to the next free first name." },
                    "cwd": { "type": "string", "description": "Folder to start in, absolute or relative to the workspace root. Defaults to the workspace root." },
                    "prompt": { "type": "string", "description": "First message for a Claude Code pane, submitted once its session is ready." },
                    "near": { "type": "string", "description": "Name of the pane to split. Defaults to the last one." },
                    "direction": { "type": "string", "enum": ["right", "down"], "description": "Which side of `near` the new pane goes. Default right." },
                    "focus": { "type": "boolean", "description": "Bring the new pane to the front. Default false." },
                    "workspace": workspace_arg
                }
            }
        },
        {
            "name": "pane_close",
            "description": "Close a pane: its terminal and whatever runs in it are killed. A Claude Code or Codex conversation stays on disk and can be resumed from the session picker. The last pane of a workspace cannot be closed. Closing your own pane ends your session.",
            "inputSchema": {
                "type": "object",
                "properties": { "pane": pane_arg, "workspace": workspace_arg },
                "required": ["pane"]
            }
        },
        {
            "name": "pane_restart",
            "description": "Restart a pane's terminal. By default its agent resumes the conversation it had; fresh=true starts a new conversation instead (a reset); agent switches the pane to another agent, also fresh. The pane keeps its name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pane": pane_arg,
                    "fresh": { "type": "boolean", "description": "Start a new conversation rather than resuming. Default false." },
                    "agent": { "type": "string", "enum": ["claude", "codex", "gemini", "opencode", "shell"] },
                    "workspace": workspace_arg
                },
                "required": ["pane"]
            }
        },
        {
            "name": "pane_send",
            "description": "Type into a pane as if at its keyboard. `text` is pasted and, unless submit=false, followed by Enter. `keys` are pressed after the text, in order — ['escape'] interrupts Claude Code, ['ctrl+c'] stops a command, ['down', 'enter'] picks the next option of a menu. With when_idle=true (Claude Code panes only) the text is queued instead, and submitted once that agent has finished its current turn rather than interrupting it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pane": pane_arg,
                    "text": { "type": "string" },
                    "submit": { "type": "boolean", "description": "Press Enter after the text. Default true." },
                    "keys": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["enter", "escape", "tab", "shift+tab", "backspace", "up", "down", "left", "right", "ctrl+c", "ctrl+d"]
                        }
                    },
                    "when_idle": { "type": "boolean", "description": "Queue the text until the Claude Code session is idle. Default false." },
                    "workspace": workspace_arg
                },
                "required": ["pane"]
            }
        },
        {
            "name": "pane_read",
            "description": "Read what a pane's terminal shows: its last lines, as plain text. Use it to check on an agent you started or on a command you sent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pane": pane_arg,
                    "lines": { "type": "integer", "description": "How many lines from the bottom. Default 60, maximum 1000." },
                    "workspace": workspace_arg
                },
                "required": ["pane"]
            }
        },
        {
            "name": "workspace_window",
            "description": "Move a workspace into a window of its own (detach=true) — to put it on a second screen, for instance — or back into Hangar's main window (detach=false). Its panes and agents keep running through the move.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "detach": { "type": "boolean", "description": "true for a window of its own, false for the main window. Default true." },
                    "workspace": workspace_arg
                }
            }
        },
        {
            "name": "pane_focus",
            "description": "Show a pane to the user: switch Hangar's window to its workspace and focus it.",
            "inputSchema": {
                "type": "object",
                "properties": { "pane": pane_arg, "workspace": workspace_arg },
                "required": ["pane"]
            }
        }
    ])
}

fn call_tool(api: &Api, name: &str, args: &Value) -> Result<Value, String> {
    let str_arg = |key: &str| args.get(key).and_then(|v| v.as_str()).map(str::to_string);

    match name {
        "board_list_tasks" => {
            let mut result = api.get("/api/board")?;
            if let Some(column) = str_arg("column") {
                if let Some(tasks) = result.get_mut("tasks").and_then(|t| t.as_array_mut()) {
                    tasks.retain(|t| t.get("column").and_then(|c| c.as_str()) == Some(&column));
                }
            }
            Ok(result)
        }
        "board_next_task" => api.get("/api/next"),
        "board_claim_task" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            let agent = str_arg("agent").unwrap_or_else(agent_name);
            api.send(
                "POST",
                &format!("/api/tasks/{id}/claim"),
                json!({ "agent": agent }),
            )
        }
        "board_create_task" => api.send("POST", "/api/tasks", args.clone()),
        "board_update_task" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            let mut patch = args.clone();
            if let Some(obj) = patch.as_object_mut() {
                obj.remove("id");
            }
            api.send("PATCH", &format!("/api/tasks/{id}"), patch)
        }
        "board_comment_task" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            let text = str_arg("text").ok_or("missing 'text'")?;
            let author = str_arg("author").unwrap_or_else(agent_name);
            api.send(
                "POST",
                &format!("/api/tasks/{id}/comment"),
                json!({ "author": author, "text": text }),
            )
        }
        "board_delete_task" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            api.delete(&format!("/api/tasks/{id}"))
        }
        "memory_search" => {
            let query = str_arg("query").ok_or("missing 'query'")?;
            let mut path = format!("/api/memory/search?q={}", urlencode(&query));
            if let Some(tag) = str_arg("tag") {
                path.push_str(&format!("&tag={}", urlencode(&tag)));
            }
            if let Some(workspace) = str_arg("workspace") {
                path.push_str(&format!("&workspace={}", urlencode(&workspace)));
            }
            if let Some(limit) = args.get("limit").and_then(|v| v.as_u64()) {
                path.push_str(&format!("&limit={limit}"));
            }
            api.get(&path)
        }
        "memory_write" => {
            let title = str_arg("title").ok_or("missing 'title'")?;
            let content = str_arg("content").ok_or("missing 'content'")?;
            // The origin is not the model's to state: the workspace comes from
            // the cwd Api::url appends, the agent from the pane's environment.
            let mut body = json!({ "title": title, "content": content, "agent": agent_name() });
            // Only a real list goes through. A model spelling "no tags" as an
            // explicit null would otherwise fail the whole write: the server
            // deserializes `tags` into a Vec, and serde's default only covers
            // an absent field, never a null one.
            if let Some(tags) = args.get("tags").filter(|tags| tags.is_array()) {
                body["tags"] = tags.clone();
            }
            api.send("POST", "/api/memory", body)
        }
        "memory_update" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            let mut patch = args.clone();
            if let Some(obj) = patch.as_object_mut() {
                obj.remove("id");
            }
            api.send("PATCH", &format!("/api/memory/{id}"), patch)
        }
        "memory_list" => {
            // The route filters and strips the contents itself; this side only
            // forwards the parameters. An empty string is not forwarded — it
            // means "no filter", the way it does on the search route: a model
            // that fills every property of the schema would otherwise ask for
            // the entries tagged with nothing and be told the memory is empty.
            let mut path = String::from("/api/memory");
            let mut sep = '?';
            for key in ["tag", "workspace"] {
                if let Some(value) = str_arg(key)
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                {
                    path.push(sep);
                    path.push_str(&format!("{key}={}", urlencode(&value)));
                    sep = '&';
                }
            }
            api.get(&path)
        }
        "memory_read" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            api.get(&format!("/api/memory/{id}"))
        }
        "memory_delete" => {
            let id = str_arg("id").ok_or("missing 'id'")?;
            api.delete(&format!("/api/memory/{id}"))
        }
        op if crate::control::OPERATIONS.contains(&op) => {
            let mut body = if args.is_object() {
                args.clone()
            } else {
                json!({})
            };
            // Who is asking, so "my workspace" and "you" mean something. Read
            // from the pane's environment, which the model cannot dress up.
            body["caller"] = json!({
                "paneId": env_var(&["HANGAR_PANE_ID"]),
                "paneName": env_var(&["HANGAR_PANE_NAME"]),
                "cwd": workspace_cwd(),
            });
            api.send("POST", &format!("/api/control/{op}"), body)
        }
        other => Err(format!("unknown tool '{other}'")),
    }
}

fn respond(id: Option<&Value>, result: Value) -> Option<Value> {
    // Notifications carry no id and must not be answered.
    id.map(|id| json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn handle(request: &Value) -> Option<Value> {
    let id = request.get("id");
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => respond(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "hangar-ai-board", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "tools/list" => respond(id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));

            let outcome = Api::connect().and_then(|api| call_tool(&api, name, &args));

            // Tool failures are reported inside the result with isError, not as
            // JSON-RPC errors, so the model can read and react to them.
            let (text, is_error) = match outcome {
                Ok(value) => (
                    serde_json::to_string_pretty(&value).unwrap_or_default(),
                    false,
                ),
                Err(err) => (err, true),
            };

            respond(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": is_error
                }),
            )
        }
        "ping" => respond(id, json!({})),
        "notifications/initialized" => None,
        _ => id.map(|id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") }
            })
        }),
    }
}

/// Blocking stdio loop: one JSON-RPC message per line.
pub fn run_stdio() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if let Some(response) = handle(&request) {
            let _ = writeln!(stdout, "{response}");
            let _ = stdout.flush();
        }
    }
}
