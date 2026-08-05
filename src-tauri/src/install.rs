//! One-click MCP registration across agent CLIs.
//!
//! There is no single standard. Most tools read a JSON file keyed by
//! `mcpServers`, VS Code uses `servers`, and Codex uses TOML. Rather than
//! pretending one format fits all, each target declares its own shape and we
//! patch its file in place, preserving whatever the user already had.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Map, Value};

const SERVER_KEY: &str = "iabench";

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    /// `{ "mcpServers": { ... } }` — Claude Code, Cursor, Gemini, Windsurf.
    JsonMcpServers,
    /// `{ "servers": { ... } }` — VS Code.
    JsonServers,
    /// `[mcp_servers.name]` — Codex.
    TomlCodex,
}

#[derive(Debug, Clone)]
struct Target {
    id: &'static str,
    label: &'static str,
    format: Format,
    /// Config lives in the project rather than the home directory.
    project_scoped: bool,
    relative_path: &'static str,
}

const TARGETS: &[Target] = &[
    Target {
        id: "claude-code",
        label: "Claude Code (projet)",
        format: Format::JsonMcpServers,
        project_scoped: true,
        relative_path: ".mcp.json",
    },
    Target {
        id: "codex",
        label: "Codex",
        format: Format::TomlCodex,
        project_scoped: false,
        relative_path: ".codex/config.toml",
    },
    Target {
        id: "gemini",
        label: "Gemini CLI",
        format: Format::JsonMcpServers,
        project_scoped: false,
        relative_path: ".gemini/settings.json",
    },
    Target {
        id: "cursor",
        label: "Cursor (projet)",
        format: Format::JsonMcpServers,
        project_scoped: true,
        relative_path: ".cursor/mcp.json",
    },
    Target {
        id: "vscode",
        label: "VS Code (projet)",
        format: Format::JsonServers,
        project_scoped: true,
        relative_path: ".vscode/mcp.json",
    },
];

#[derive(Debug, Serialize)]
pub struct TargetStatus {
    pub id: String,
    pub label: String,
    pub path: String,
    /// The tool looks installed on this machine.
    pub detected: bool,
    /// Our server is already registered in its config.
    pub configured: bool,
    pub project_scoped: bool,
}

#[derive(Debug, Serialize)]
pub struct InstallReport {
    pub id: String,
    pub ok: bool,
    pub message: String,
    pub path: String,
}

fn config_path(target: &Target, workspace_cwd: &str) -> Option<PathBuf> {
    let root = if target.project_scoped {
        PathBuf::from(workspace_cwd)
    } else {
        dirs::home_dir()?
    };
    Some(root.join(target.relative_path))
}

/// Presence of the tool's own config directory is a good enough signal, and
/// avoids shelling out to probe every CLI.
fn detected(target: &Target) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    match target.id {
        "claude-code" => home.join(".claude").exists(),
        "codex" => home.join(".codex").exists(),
        "gemini" => home.join(".gemini").exists(),
        "cursor" => home.join(".cursor").exists(),
        // VS Code keeps its user data per platform: %APPDATA% on Windows,
        // ~/Library/Application Support on macOS, ~/.config on Linux. The
        // ~/.vscode directory holding extensions is common to all three.
        "vscode" => {
            home.join(".vscode").exists()
                || home.join("AppData/Roaming/Code").exists()
                || home.join("Library/Application Support/Code").exists()
                || home.join(".config/Code").exists()
        }
        _ => false,
    }
}

fn executable() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

/// The entry every JSON-shaped target gets. Note it carries no port or token:
/// those are resolved at runtime from the endpoint file, so this stays valid
/// across app restarts.
fn server_entry(workspace_cwd: &str) -> Result<Value, String> {
    Ok(json!({
        "command": executable()?,
        "args": ["--mcp"],
        "env": { "IABENCH_WORKSPACE": workspace_cwd }
    }))
}

fn read_json(path: &PathBuf) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Keeps a one-shot copy of whatever was there before we touched it.
fn backup(path: &PathBuf) {
    if path.exists() {
        let backup = path.with_extension(format!(
            "{}.iabench-bak",
            path.extension().and_then(|e| e.to_str()).unwrap_or("")
        ));
        if !backup.exists() {
            let _ = fs::copy(path, backup);
        }
    }
}

fn install_json(path: &PathBuf, key: &str, workspace_cwd: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    backup(path);

    let mut root = read_json(path);
    let servers = root
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    let Some(servers) = servers.as_object_mut() else {
        return Err(format!("'{key}' exists but is not an object"));
    };
    servers.insert(SERVER_KEY.to_string(), server_entry(workspace_cwd)?);

    let body = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

/// Claude Code gates project-scoped `.mcp.json` servers behind an approval that
/// is only remembered when the workspace is explicitly trusted. Without this,
/// the user re-approves the server on every single session. Whitelisting it in
/// the global settings is the documented way to make the choice stick.
fn allow_project_server_globally() -> Result<(), String> {
    let Some(home) = dirs::home_dir() else {
        return Err("no home directory".into());
    };
    let path = home.join(".claude").join("settings.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    backup(&path);

    let mut root = read_json(&path);
    let list = root
        .entry("enabledMcpjsonServers".to_string())
        .or_insert_with(|| Value::Array(vec![]));

    let Some(list) = list.as_array_mut() else {
        return Err("'enabledMcpjsonServers' exists but is not an array".into());
    };
    // Preserve any server the user already approved.
    if !list.iter().any(|v| v.as_str() == Some(SERVER_KEY)) {
        list.push(Value::String(SERVER_KEY.to_string()));
    }

    let body = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    fs::write(&path, body).map_err(|e| e.to_string())
}

fn install_toml(path: &PathBuf, workspace_cwd: &str) -> Result<(), String> {
    use toml_edit::{value, Array, DocumentMut, Item, Table};

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    backup(path);

    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut doc: DocumentMut = existing
        .parse()
        .map_err(|e| format!("config.toml is not valid TOML: {e}"))?;

    let mut args = Array::new();
    args.push("--mcp");

    let mut env = Table::new();
    env.insert("IABENCH_WORKSPACE", value(workspace_cwd));

    let mut server = Table::new();
    server.insert("command", value(executable()?));
    server.insert("args", value(args));
    server.insert("env", Item::Table(env));

    // Ensure [mcp_servers] exists as a real table before adding our subtable.
    if !doc.contains_key("mcp_servers") {
        let mut parent = Table::new();
        parent.set_implicit(true);
        doc.insert("mcp_servers", Item::Table(parent));
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or("'mcp_servers' exists but is not a table")?;
    servers.insert(SERVER_KEY, Item::Table(server));

    fs::write(path, doc.to_string()).map_err(|e| e.to_string())
}

fn is_configured(target: &Target, path: &PathBuf) -> bool {
    match target.format {
        Format::JsonMcpServers => read_json(path)
            .get("mcpServers")
            .and_then(|v| v.get(SERVER_KEY))
            .is_some(),
        Format::JsonServers => read_json(path)
            .get("servers")
            .and_then(|v| v.get(SERVER_KEY))
            .is_some(),
        Format::TomlCodex => fs::read_to_string(path)
            .ok()
            .and_then(|raw| raw.parse::<toml_edit::DocumentMut>().ok())
            .map(|doc| {
                doc.get("mcp_servers")
                    .and_then(|s| s.get(SERVER_KEY))
                    .is_some()
            })
            .unwrap_or(false),
    }
}

#[tauri::command]
pub fn mcp_targets(workspace_cwd: String) -> Vec<TargetStatus> {
    TARGETS
        .iter()
        .filter_map(|target| {
            let path = config_path(target, &workspace_cwd)?;
            Some(TargetStatus {
                id: target.id.to_string(),
                label: target.label.to_string(),
                configured: is_configured(target, &path),
                path: path.to_string_lossy().into_owned(),
                detected: detected(target),
                project_scoped: target.project_scoped,
            })
        })
        .collect()
}

#[tauri::command]
pub fn mcp_install(ids: Vec<String>, workspace_cwd: String) -> Vec<InstallReport> {
    TARGETS
        .iter()
        .filter(|t| ids.iter().any(|id| id == t.id))
        .map(|target| {
            let path = match config_path(target, &workspace_cwd) {
                Some(p) => p,
                None => {
                    return InstallReport {
                        id: target.id.to_string(),
                        ok: false,
                        message: "chemin de configuration introuvable".into(),
                        path: String::new(),
                    }
                }
            };

            let mut outcome = match target.format {
                Format::JsonMcpServers => install_json(&path, "mcpServers", &workspace_cwd),
                Format::JsonServers => install_json(&path, "servers", &workspace_cwd),
                Format::TomlCodex => install_toml(&path, &workspace_cwd),
            };

            // Writing .mcp.json alone leaves the user re-approving every session.
            let mut note = "configuré".to_string();
            if outcome.is_ok() && target.id == "claude-code" {
                match allow_project_server_globally() {
                    Ok(()) => note = "configuré et approuvé globalement".into(),
                    Err(err) => {
                        outcome = Err(format!(
                            "configuré, mais l'approbation globale a échoué : {err}"
                        ))
                    }
                }
            }

            InstallReport {
                id: target.id.to_string(),
                ok: outcome.is_ok(),
                message: outcome.err().unwrap_or(note),
                path: path.to_string_lossy().into_owned(),
            }
        })
        .collect()
}

/// Shown in the UI so the user can register the server by hand if they prefer.
#[tauri::command]
pub fn mcp_manual_commands(workspace_cwd: String) -> Result<serde_json::Value, String> {
    let exe = executable()?;
    Ok(json!({
        "executable": exe,
        "claude": format!("claude mcp add {SERVER_KEY} --scope project -- \"{exe}\" --mcp"),
        "codex": format!("codex mcp add {SERVER_KEY} -- \"{exe}\" --mcp"),
        "workspace": workspace_cwd,
    }))
}
