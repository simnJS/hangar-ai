//! One-click MCP registration across agent CLIs.
//!
//! There is no single standard. Most tools read a JSON file keyed by
//! `mcpServers`, VS Code uses `servers`, and Codex uses TOML. Rather than
//! pretending one format fits all, each target declares its own shape and we
//! patch its file in place, preserving whatever the user already had.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

const SERVER_KEY: &str = "hangar";

/// Names this server has been registered under before, newest first.
///
/// They are still sitting in configs installed on users' machines, and every one
/// of them points at a binary path from that era. Leaving them in place would
/// declare a second server that fails to start on every agent launch, so an
/// install removes them rather than adding beside them.
const LEGACY_SERVER_KEYS: &[&str] = &["iabench"];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    /// `{ "mcpServers": { ... } }` — Claude Code, Cursor, Gemini, Windsurf.
    JsonMcpServers,
    /// `{ "servers": { ... } }` — VS Code.
    JsonServers,
    /// `[mcp_servers.name]` — Codex.
    TomlCodex,
}

/// Top-level key holding the servers, for the two JSON shapes.
fn json_section(format: Format) -> &'static str {
    match format {
        Format::JsonServers => "servers",
        _ => "mcpServers",
    }
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
    /// Registered, but pointing at an executable that is no longer there.
    ///
    /// A config that names a missing binary looks identical to a working one
    /// until an agent tries to start the server and silently gets nothing, so
    /// the distinction has to reach the UI.
    pub stale: bool,
    /// The executable the config currently names, when there is one. Shown next
    /// to a stale entry so the user can see what it is still pointing at.
    pub command: Option<String>,
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
        "env": { "HANGAR_WORKSPACE": workspace_cwd }
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
            "{}.hangar-bak",
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
    for legacy in LEGACY_SERVER_KEYS {
        servers.remove(*legacy);
    }
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
    // The approval is by server name, so an entry under the old name approves a
    // server that no longer exists. Drop it, keeping everything else the user
    // has already approved.
    list.retain(|v| {
        !v.as_str()
            .is_some_and(|name| LEGACY_SERVER_KEYS.contains(&name))
    });
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
    env.insert("HANGAR_WORKSPACE", value(workspace_cwd));

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
    for legacy in LEGACY_SERVER_KEYS {
        servers.remove(legacy);
    }
    servers.insert(SERVER_KEY, Item::Table(server));

    fs::write(path, doc.to_string()).map_err(|e| e.to_string())
}

/// The entry we own in a target's config.
struct Registration {
    /// Sits under an old server key, so it predates the rename.
    legacy: bool,
    /// The executable the entry names, when it declares one.
    command: Option<String>,
}

impl Registration {
    /// An entry nothing can start: the wrong key, no command, or a command that
    /// is not on disk any more. All three look "configured" in a config file,
    /// which is exactly how a dead registration goes unnoticed.
    fn stale(&self) -> bool {
        self.legacy
            || match &self.command {
                Some(command) => !Path::new(command).is_file(),
                None => true,
            }
    }
}

fn registration(target: &Target, path: &PathBuf) -> Option<Registration> {
    match target.format {
        Format::JsonMcpServers | Format::JsonServers => {
            let command_of = |entry: &Value| {
                entry
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let root = read_json(path);
            let servers = root.get(json_section(target.format))?;
            if let Some(entry) = servers.get(SERVER_KEY) {
                return Some(Registration {
                    legacy: false,
                    command: command_of(entry),
                });
            }
            LEGACY_SERVER_KEYS.iter().find_map(|key| {
                servers.get(*key).map(|entry| Registration {
                    legacy: true,
                    command: command_of(entry),
                })
            })
        }
        Format::TomlCodex => {
            let command_of = |entry: &toml_edit::Item| {
                entry
                    .get("command")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
            };
            let doc = fs::read_to_string(path)
                .ok()?
                .parse::<toml_edit::DocumentMut>()
                .ok()?;
            let servers = doc.get("mcp_servers")?;
            if let Some(entry) = servers.get(SERVER_KEY) {
                return Some(Registration {
                    legacy: false,
                    command: command_of(entry),
                });
            }
            LEGACY_SERVER_KEYS.iter().find_map(|key| {
                servers.get(*key).map(|entry| Registration {
                    legacy: true,
                    command: command_of(entry),
                })
            })
        }
    }
}

#[tauri::command]
pub fn mcp_targets(workspace_cwd: String) -> Vec<TargetStatus> {
    TARGETS
        .iter()
        .filter_map(|target| {
            let path = config_path(target, &workspace_cwd)?;
            let found = registration(target, &path);
            Some(TargetStatus {
                id: target.id.to_string(),
                label: target.label.to_string(),
                configured: found.is_some(),
                stale: found.as_ref().is_some_and(Registration::stale),
                command: found.and_then(|found| found.command),
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
                Format::TomlCodex => install_toml(&path, &workspace_cwd),
                format => install_json(&path, json_section(format), &workspace_cwd),
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

#[cfg(test)]
mod tests {
    use super::*;

    const JSON_TARGET: Target = Target {
        id: "test",
        label: "test",
        format: Format::JsonMcpServers,
        project_scoped: true,
        relative_path: "mcp.json",
    };

    fn temp_config(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hangar-install-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir.join("mcp.json")
    }

    fn write_server(path: &PathBuf, key: &str, command: &str) {
        let body = json!({ "mcpServers": { key: { "command": command, "args": ["--mcp"] } } });
        fs::write(path, body.to_string()).unwrap();
    }

    /// Leaving the old entry behind would declare a second server pointing at a
    /// binary from before the rename, which fails on every agent launch.
    #[test]
    fn installing_drops_the_entry_from_the_previous_server_name() {
        let path = temp_config("purge");
        let body = json!({
            "mcpServers": {
                "iabench": { "command": "C:/gone/hangar-ia.exe", "args": ["--mcp"] },
                "unrelated": { "command": "C:/other.exe" }
            }
        });
        fs::write(&path, body.to_string()).unwrap();

        install_json(&path, "mcpServers", "C:/ws").expect("install");

        let root = read_json(&path);
        let servers = root.get("mcpServers").unwrap();
        assert!(servers.get("iabench").is_none());
        assert!(servers.get(SERVER_KEY).is_some());
        assert!(
            servers.get("unrelated").is_some(),
            "other servers must survive"
        );
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn an_entry_under_the_previous_name_reads_as_stale() {
        let path = temp_config("legacy-key");
        let exe = std::env::current_exe().unwrap();
        write_server(&path, "iabench", &exe.to_string_lossy());

        let found = registration(&JSON_TARGET, &path).expect("registration");

        assert!(found.legacy);
        assert!(found.stale(), "the old key needs reinstalling either way");
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    /// The failure this whole change exists for: the key is there, so the UI
    /// said "configured", while the binary it named had been renamed away.
    #[test]
    fn an_entry_naming_a_missing_executable_is_stale() {
        let path = temp_config("dead-path");
        write_server(&path, SERVER_KEY, "C:/nowhere/hangar-ia.exe");

        let found = registration(&JSON_TARGET, &path).expect("registration");

        assert!(!found.legacy);
        assert!(found.stale());
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn an_entry_naming_a_real_executable_is_healthy() {
        let path = temp_config("live-path");
        let exe = std::env::current_exe().unwrap();
        write_server(&path, SERVER_KEY, &exe.to_string_lossy());

        let found = registration(&JSON_TARGET, &path).expect("registration");

        assert!(!found.stale());
        assert_eq!(found.command.as_deref(), Some(&*exe.to_string_lossy()));
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn a_config_that_never_had_us_reports_nothing() {
        let path = temp_config("absent");
        fs::write(&path, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();

        assert!(registration(&JSON_TARGET, &path).is_none());
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }
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
