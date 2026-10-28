//! MCP server exposed over stdio.
//!
//! Runs inside the same executable as the GUI (`hangar-ai --mcp`), which keeps
//! agent configuration trivial: one absolute path, no sidecar to bundle or
//! locate. Every call is proxied to the running app's HTTP API so that task
//! claims stay atomic across all panes.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// ureq stops reading a body at 10 MB unless told otherwise, and a
/// model_snapshot answer carries its PNG in base64: at 2048 pixels a side, a
/// detailed model can pass that. The API is on loopback and its answers are
/// the app's own, so the limit only has to stop a runaway.
const MAX_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;

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
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .lossy_utf8(true)
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
    let mut tools = base_tool_definitions();
    if let (Some(all), Some(manager)) =
        (tools.as_array_mut(), manager_tool_definitions().as_array())
    {
        all.extend(manager.iter().cloned());
    }
    tools
}

fn base_tool_definitions() -> Value {
    let workspace_arg = json!({
        "type": "string",
        "description": "Workspace name or folder. Defaults to the one you are working in."
    });
    let pane_arg = json!({
        "type": "string",
        "description": "Pane name, as pane_list shows it (an id works too). Names are unique across workspaces, so a pane is found wherever it is."
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
            "description": "Take ownership of a task and move it to 'doing'. Fails if another agent already holds it, or if the task owns files that a task already in doing owns too — pick a different task in that case. Always claim before starting work.",
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
                    },
                    "owns": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Paths and globs, relative to the workspace, that the task's worker may change, tests included (e.g. ['src/export/', 'src/App.tsx']). A folder covers everything in it; a name with an extension is a file, so write a folder with a dot in its name with a trailing slash ('v1.2/'). Two tasks in 'doing' never share one."
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
                    "release": { "type": "boolean", "description": "Unassign the task so someone else can take it." },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Replaces the task's labels: pass the ones to keep too."
                    },
                    "depends_on": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Replaces the ids of the tasks that must be done first."
                    },
                    "owns": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Replaces the paths and globs the task's worker may change."
                    }
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
                    "name": { "type": "string", "description": "Name for the pane, unique across every workspace. Defaults to the next free first name." },
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
        },
        {
            "name": "model_show",
            "description": "Show the user a 3D model in Hangar's preview panel, beside the terminals. Each workspace has its own preview, and the model opens in yours: when the user is looking at another workspace, it waits in yours without covering theirs, and the answer says so ('visible': false, with a note) — tell them where to look if they should. Call it whenever you create, export, change or find a model the user should look at, instead of handing them a file path. It reads glTF (.glb, or .gltf with its .bin and textures beside it), .obj with its .mtl, .stl, .ply, .fbx and .usdz, up to 256 MB, from your workspace's folders and its git worktrees. Showing a model that is already open brings it forward and reloads it; an open model also reloads by itself when its file changes on disk, so re-exporting is enough to refresh it. The answer says what the viewer found: format, triangles, vertices, meshes, materials, animation clip names, and the bounding box size and centre in the file's units. Check those against what you meant to build: a centimetre/metre mix-up shows as a size a hundred times off. A very large model can answer status 'loading' before it is done. To see the model yourself, call model_snapshot.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The model file: absolute, or relative to your working directory." },
                    "title": { "type": "string", "description": "Name for its tab. Defaults to the file name." }
                },
                "required": ["path"]
            }
        },
        {
            "name": "model_snapshot",
            "description": "Render an open 3D model to a PNG and look at it: the image comes back in the result, followed by the model's stats as text. Use it to check your own work before telling the user it is done — shape, proportions, orientation, materials, missing or black textures — and again after each change. The camera frames the model's bounding box from a fixed angle: front, back, left, right, top, bottom, or iso, a three-quarter view from above. The user's own view in the panel is left as it was. The model must be open in your workspace's preview already: call model_show first. It renders whether or not your workspace is on screen. A bigger image costs more of your context, and the default 1024×768 is enough for most checks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Which open model, as you gave it to model_show. Defaults to the one in front." },
                    "angle": {
                        "type": "string",
                        "enum": ["front", "back", "left", "right", "top", "bottom", "iso"],
                        "description": "Where the camera looks from. Default iso."
                    },
                    "width": { "type": "integer", "maximum": 2048, "description": "Image width in pixels. Default 1024, maximum 2048." },
                    "height": { "type": "integer", "maximum": 2048, "description": "Image height in pixels. Default 768, maximum 2048." }
                }
            }
        },
        // orchestration: workers (worker_* tools)
        {
            "name": "worker_spawn",
            "description": "Start a worker on a board task. Hangar claims the task for it (assignee: the worker, column: doing, one more attempt), checks out a git worktree on a new branch from the workspace's target branch, opens a Claude Code pane there with the worker model and twenty ports of its own, and queues its brief: the task's title and description, the files it owns, the check command, how to report, what to do when blocked and how to deliver. The worker starts with a fresh context and knows nothing else, so write the task's description as its whole brief — goal, constraints, acceptance criteria. In a workspace that is not a git repository, the worker starts in the workspace's folder itself, beside the other workers, with no worktree and no branch; a task that does not say what it owns cannot run beside another worker there. Refused, with the reason, when the workspace already runs its maxWorkers; the task is not in todo or waits on depends_on that are not done; it owns files a task in doing owns too, or neither of them says what it owns; or it has had maxAttempts workers already. Only the workspace's orchestrator can call it. You hear what the worker does through events — its comments, its delivery, a wait or an exit — so never poll it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Id of the board task to hand over." },
                    "name": { "type": "string", "description": "Name for the worker's pane, unique across Hangar. Defaults to the next free first name." },
                    "model": { "type": "string", "description": "Model the worker runs (--model): sonnet, opus, haiku or a full model id. Defaults to the workspace's worker model." }
                },
                "required": ["task"]
            }
        },
        {
            "name": "worker_list",
            "description": "List the workers of your workspace: pane name, task (id, title, column, attempts), what the agent is doing (working, waiting for the person, yours = finished its turn, idle), whether it runs, its branch and worktree, model, ports, and where its delivery stands. Only the workspace's orchestrator can call it.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "worker_stop",
            "description": "Stop a worker: close its pane, give its task back to todo (left as it is in review or done; the attempt still counts), and remove its worktree and branch. They are kept instead when keepWorktree is true, or when they hold work — uncommitted changes, or commits the target branch does not have — and the answer says where. A worker in a workspace that is not a git repository has no worktree: what it changed stays in the folder. Only the workspace's orchestrator can call it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "worker": { "type": "string", "description": "The worker's pane name, as worker_list shows it." },
                    "keepWorktree": { "type": "boolean", "description": "Keep the worktree and its branch whatever they hold. Default false." }
                },
                "required": ["worker"]
            }
        },
        // orchestration: delivery (delivery_* tools)
        {
            "name": "delivery_submit",
            "description": "Hand in your task's work, once it is done and committed: only for a worker pane started on a board task. Hangar merges your branch onto the workspace's branch as it is now, in a separate worktree, runs the workspace's check command there when it has one, and has a reviewer that did not write it read the change against your task. If all passes it lands on the branch by itself (or waits for the person, when automatic landing is off), your task is done and your pane closes. Otherwise you get a message from Hangar saying exactly what failed — a conflict, a failing check, the reviewer's blocking items — and what to do; fix it on your branch, commit, and call this again. It refuses straight away when your worktree has uncommitted changes or your branch has nothing new. Call it once, then end your turn and wait for that message; do not change your branch meanwhile. In a workspace that is not a git repository there is no branch: nothing is merged, checked or reviewed, your task is done at once and your pane closes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "description": "What you did, in a few lines, for the reviewer and the task's comments: what changed, and the commands you ran with their result." }
                }
            }
        },
        {
            "name": "delivery_status",
            "description": "Where deliveries stand: from a worker pane, its own; otherwise every worker's in your workspace. Each says its stage while running (queued, merging, checking, reviewing, landing) or its last outcome. Outcomes arrive as messages by themselves: there is no need to poll this.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "model_close",
            "description": "Close a model in your workspace's 3D preview. Without a path it closes every model open there; other workspaces' previews are left as they are. The panel goes away with the last one. Call it once you are done showing a model, so it does not keep a part of the user's screen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The model to close, as you gave it to model_show. Omit it to close every model open in your workspace's preview." }
                }
            }
        }
    ])
}

/// The project manager's tools, apart from the rest: one `json!` this long
/// would run out of the macro's recursion limit.
fn manager_tool_definitions() -> Value {
    json!([
        // orchestration: the project manager (project_* tools)
        {
            "name": "project_list",
            "description": "List every project of Hangar except the HQ: for each, whether it is open or asleep, its orchestrator (or that it has none) and what it is doing, its workers, the panes waiting for the person, its board counts, the tasks that need the person or are ready to land, its plan's status and its latest events. Returns the structured summaries and a short text overview, those with something for the person first. Only the project manager (the pane of the HQ workspace) can call it. You never act on a project without the person asking: report, and wait for their instruction.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "project_status",
            "description": "One project in detail: its orchestrator, each worker and its task, the board, what is in doing, what needs the person or waits to land, the panes waiting, its plan's status and its latest events. Returns the structured summary and the text. An unknown project is refused with the list of names. Only the project manager (the pane of the HQ workspace) can call it. You never act on a project without the person asking.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace": { "type": "string", "description": "The project's name or folder, as project_list shows it." }
                },
                "required": ["workspace"]
            }
        },
        {
            "name": "project_send",
            "description": "Pass the person's instruction on to a project's orchestrator: the text is queued, framed as relayed by the project manager from the person, and delivered once the orchestrator is idle. A workspace that is asleep or not open is opened first. Refused when the project has no orchestrator (only the person can pin one) and for the HQ itself. The orchestrator's progress comes back to you as events; do not poll. Only the project manager (the pane of the HQ workspace) can call it, and only when the person asked for what you relay: never send on your own initiative.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace": { "type": "string", "description": "The project's name or folder, as project_list shows it." },
                    "text": { "type": "string", "description": "The person's instruction, complete enough for the orchestrator to act on without asking you." }
                },
                "required": ["workspace", "text"]
            }
        }
    ])
}

/// A relative `path` argument, made absolute against `base`.
///
/// The window would resolve it against `caller.cwd`, and that is the
/// workspace root whenever the MCP config pins `HANGAR_WORKSPACE` — not the
/// worktree or subfolder the agent is working in. This process was started in
/// the agent's own folder, so resolving here is what "relative to your working
/// directory" means. A path with a root is left alone, `/c/Users/…` from Git
/// Bash included: the app reads that one as a Windows path itself.
fn absolutize_path(args: &mut Value, base: &Path) {
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        return;
    };
    let given = Path::new(path);
    if path.trim().is_empty() || given.is_absolute() || given.has_root() {
        return;
    }
    // Rebuilt from its components, so `models/chair.glb` joins a Windows
    // folder with backslashes throughout rather than half of each.
    let joined: PathBuf = base.join(given).components().collect();
    args["path"] = json!(joined.to_string_lossy());
}

/// The content of a call that succeeded.
///
/// A result is its JSON as text, except one that carries a `png`: that is a
/// picture the model asked to see (model_snapshot renders one), and as text it
/// would be a few hundred kilobytes of base64 that nobody can look at. It
/// becomes an image block, and the rest of the result follows as text.
fn success_content(mut value: Value) -> Value {
    let png = match value
        .as_object_mut()
        .and_then(|fields| fields.remove("png"))
    {
        Some(Value::String(png)) => png,
        Some(other) => {
            // Not a picture after all; the result stays as it came.
            value["png"] = other;
            return json!([text_block(&value)]);
        }
        None => return json!([text_block(&value)]),
    };
    let declared = value
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("image/png")
        .to_string();
    // The block wants bare base64; a data URL carries its own type.
    let (mime_type, data) = match png
        .strip_prefix("data:")
        .and_then(|url| url.split_once(";base64,"))
    {
        Some((mime_type, data)) => (mime_type.to_string(), data.to_string()),
        None => (declared, png),
    };
    json!([
        { "type": "image", "data": data, "mimeType": mime_type },
        text_block(&value)
    ])
}

fn text_block(value: &Value) -> Value {
    json!({ "type": "text", "text": serde_json::to_string_pretty(value).unwrap_or_default() })
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
            if op.starts_with("model_") {
                if let Ok(here) = std::env::current_dir() {
                    absolutize_path(&mut body, &here);
                }
            }
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
            let (content, is_error) = match outcome {
                Ok(value) => (success_content(value), false),
                Err(err) => (json!([{ "type": "text", "text": err }]), true),
            };

            respond(
                id,
                json!({
                    "content": content,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> Value {
        tool_definitions()
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no tool named {name}"))
    }

    /// The window's answer to model_snapshot, as the agent must receive it: a
    /// picture it can look at, and the stats beside it without the base64.
    #[test]
    fn a_result_carrying_a_png_becomes_an_image_and_its_stats_as_text() {
        let content = success_content(json!({
            "png": "iVBORw0KGgo=",
            "mimeType": "image/png",
            "path": "C:\\proj\\chair.glb",
            "angle": "iso",
            "info": { "triangles": 1200 }
        }));

        let blocks = content.as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[0],
            json!({ "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" })
        );
        assert_eq!(blocks[1]["type"], "text");
        let stats: Value = serde_json::from_str(blocks[1]["text"].as_str().unwrap()).unwrap();
        assert!(stats.get("png").is_none());
        assert_eq!(stats["angle"], "iso");
        assert_eq!(stats["info"]["triangles"], 1200);
    }

    #[test]
    fn a_png_without_a_declared_type_is_a_png_and_a_data_url_keeps_its_own() {
        let bare = success_content(json!({ "png": "AAAA" }));
        assert_eq!(bare[0]["mimeType"], "image/png");
        assert_eq!(bare[1]["text"], "{}");

        let url = success_content(json!({ "png": "data:image/jpeg;base64,/9j/", "w": 1 }));
        assert_eq!(url[0]["data"], "/9j/");
        assert_eq!(url[0]["mimeType"], "image/jpeg");
    }

    #[test]
    fn a_result_without_a_picture_stays_text() {
        for value in [
            json!({ "panes": [{ "name": "Ava" }] }),
            json!({ "png": null, "note": "nothing rendered" }),
            json!([1, 2, 3]),
        ] {
            let content = success_content(value.clone());
            assert_eq!(content.as_array().unwrap().len(), 1, "{value}");
            assert_eq!(content[0]["type"], "text");
            let text: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(text, value);
        }
    }

    /// An operation the window knows but no tool offers is one no agent can
    /// ever call.
    #[test]
    fn every_window_operation_is_offered_as_a_tool() {
        for op in crate::control::OPERATIONS {
            assert_eq!(tool(op)["name"], *op);
        }
    }

    #[test]
    fn the_model_tools_ask_for_what_the_window_reads() {
        assert_eq!(
            tool("model_show")["inputSchema"]["required"],
            json!(["path"])
        );
        let snapshot = &tool("model_snapshot")["inputSchema"];
        assert_eq!(
            snapshot["properties"]["angle"]["enum"],
            json!(["front", "back", "left", "right", "top", "bottom", "iso"])
        );
        assert_eq!(snapshot["properties"]["width"]["maximum"], 2048);
        assert_eq!(snapshot["properties"]["height"]["maximum"], 2048);
        assert!(snapshot.get("required").is_none());
        assert!(tool("model_close")["inputSchema"].get("required").is_none());
    }

    /// An orchestrator plans with these: files a task owns, the label that
    /// hands a task to the person, the order tasks run in.
    #[test]
    fn the_board_tools_take_what_an_orchestrator_writes() {
        let create = &tool("board_create_task")["inputSchema"]["properties"];
        assert_eq!(create["owns"]["type"], "array");
        assert_eq!(create["depends_on"]["type"], "array");
        let update = &tool("board_update_task")["inputSchema"]["properties"];
        for field in ["owns", "labels", "depends_on"] {
            assert_eq!(update[field]["items"]["type"], "string", "{field}");
        }
    }

    #[test]
    fn the_project_manager_s_tools_are_offered() {
        for name in ["project_list", "project_status", "project_send"] {
            let description = tool(name)["description"].as_str().unwrap().to_owned();
            assert!(description.contains("project manager"), "{name}");
        }
        assert_eq!(
            tool("project_status")["inputSchema"]["required"],
            json!(["workspace"])
        );
        assert_eq!(
            tool("project_send")["inputSchema"]["required"],
            json!(["workspace", "text"])
        );
        assert!(tool("project_list")["inputSchema"]
            .get("required")
            .is_none());
    }

    #[test]
    fn a_relative_model_path_is_resolved_where_the_agent_works() {
        let here = std::env::temp_dir().join("work");
        let expected = json!(here.join("models").join("chair.glb").to_string_lossy());

        for given in ["models/chair.glb", "./models/chair.glb"] {
            let mut args = json!({ "path": given, "title": "Chair" });
            absolutize_path(&mut args, &here);
            assert_eq!(args["path"], expected, "{given}");
            assert_eq!(args["title"], "Chair");
        }

        let absolute = here.join("chair.glb").to_string_lossy().into_owned();
        for untouched in [
            json!({ "path": absolute }),
            // Git Bash's spelling of a Windows path; the app translates it.
            json!({ "path": "/c/Users/me/chair.glb" }),
            json!({ "path": "" }),
            json!({ "angle": "iso" }),
        ] {
            let mut args = untouched.clone();
            absolutize_path(&mut args, &here);
            assert_eq!(args, untouched);
        }
    }
}
