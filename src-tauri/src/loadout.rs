//! Loadout: the plugin catalog a team equips its Claude Code projects from.
//!
//! Hangar does not reimplement any of it. The catalog ships its own engine,
//! `loadout.mjs`, which already knows how to check every capability on this
//! machine and how to write a project's `.claude/settings.json`; the tab only
//! runs it with `--json` and draws what comes back. A catalog that changes its
//! checks therefore needs no Hangar release.
//!
//! The engine is found where Claude Code installs the marketplace. When it is
//! not there, `loadout_locate` says so as data and the tab stays hidden: most
//! people running Hangar have never heard of Loadout, and should not have to.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::git::off_thread;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Points at a catalog clone instead of the installed marketplace — the same
/// thing `loadout.mjs --catalog` does, for whoever works on the catalog itself.
const CATALOG_ENV: &str = "LOADOUT_CATALOG";

fn script_in(catalog: &Path) -> PathBuf {
    catalog
        .join("plugins")
        .join("loadout")
        .join("scripts")
        .join("loadout.mjs")
}

fn locate() -> Option<PathBuf> {
    let catalog = match std::env::var_os(CATALOG_ENV) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => dirs::home_dir()?
            .join(".claude")
            .join("plugins")
            .join("marketplaces")
            .join("loadout"),
    };
    let script = script_in(&catalog);
    script.is_file().then_some(script)
}

fn script() -> Result<PathBuf, String> {
    locate().ok_or_else(|| "Loadout is not installed".to_string())
}

/// The line of a failed run worth showing: the engine throws plain `Error`s
/// whose message is written for people, and node prints it under a stack.
fn diagnostic(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    lines
        .iter()
        .find_map(|l| l.strip_prefix("Error: "))
        .or_else(|| lines.last().copied())
        .unwrap_or("loadout.mjs failed")
        .to_string()
}

fn run(args: &[&str]) -> Result<String, String> {
    let script = script()?;
    // Without it a bundled macOS app looks at the bare `launchd` PATH, where a
    // Homebrew node does not exist. See path_env.rs.
    crate::path_env::ensure();

    let mut command = Command::new("node");
    command.arg(&script).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "Node.js was not found on PATH".to_string(),
        _ => format!("could not run node: {err}"),
    })?;
    if !output.status.success() {
        return Err(diagnostic(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_json(args: &[&str]) -> Result<Value, String> {
    let out = run(args)?;
    serde_json::from_str(&out).map_err(|err| format!("loadout.mjs printed no JSON: {err}"))
}

/// Gives each provider entry the catalog's "how to enable it" line when the
/// engine did not include it (engines older than the `how` field). Entries
/// that already carry one are left alone.
fn fill_how(status: &mut Value, providers: &Value) {
    let how = |id: &str| providers.pointer(&format!("/providers/{id}/how")).cloned();
    let Some(plugins) = status.get_mut("plugins").and_then(Value::as_array_mut) else {
        return;
    };
    for capability in plugins
        .iter_mut()
        .filter_map(|p| p.get_mut("capabilities").and_then(Value::as_array_mut))
        .flatten()
    {
        for key in ["requires", "chain"] {
            let Some(entries) = capability.get_mut(key).and_then(Value::as_array_mut) else {
                continue;
            };
            for entry in entries.iter_mut().filter_map(Value::as_object_mut) {
                if entry.contains_key("how") {
                    continue;
                }
                let found = entry.get("provider").and_then(Value::as_str).and_then(&how);
                if let Some(found) = found {
                    entry.insert("how".into(), found);
                }
            }
        }
    }
}

/// Where the engine is, or `None` — in which case the tab is not shown.
#[tauri::command]
pub fn loadout_locate() -> Option<String> {
    locate().map(|p| p.to_string_lossy().into_owned())
}

/// Every capability of the catalog, checked on this machine, with what the
/// workspace has equipped. Slow by nature: the engine asks GitHub whether the
/// private plugins are reachable.
#[tauri::command]
pub async fn loadout_status(cwd: String) -> Result<Value, String> {
    off_thread(move || {
        let mut status = run_json(&["status", "--project", &cwd, "--json"])?;
        let providers = script()?
            .parent()
            .and_then(Path::parent)
            .map(|dir| dir.join("providers.json"));
        if let Some(providers) = providers
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        {
            fill_how(&mut status, &providers);
        }
        Ok(status)
    })
    .await
}

/// Project types and the plugins each one equips.
#[tauri::command]
pub async fn loadout_presets() -> Result<Value, String> {
    off_thread(|| run_json(&["presets", "--json"])).await
}

/// Ready-to-copy prompts for keeping a setup lean (the catalog's `prompts`),
/// or `None` from an engine that predates them — the panel is then hidden.
#[tauri::command]
pub async fn loadout_prompts() -> Result<Option<Value>, String> {
    off_thread(|| match run_json(&["prompts", "--json"]) {
        Ok(prompts) => Ok(Some(prompts)),
        Err(err) if is_unknown_command(&err) => Ok(None),
        Err(err) => Err(err),
    })
    .await
}

/// How an engine says it has no such command: `loadout: unknown command "x"`.
fn is_unknown_command(err: &str) -> bool {
    err.contains("unknown command")
}

/// Equips the workspace with a preset (or a single plugin): the engine writes
/// its `.claude/settings.json` and reports `{ file, plugins, written }`.
#[tauri::command]
pub async fn loadout_equip(cwd: String, preset: String) -> Result<Value, String> {
    off_thread(move || {
        let report = run_json(&["equip", &cwd, &preset, "--json"])?;
        // Engines older than `written` treated `--json` as a preview and wrote
        // nothing: the plain command is the one that writes there.
        if report.get("written").and_then(Value::as_bool) != Some(true) {
            run(&["equip", &cwd, &preset])?;
        }
        Ok(report)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_error_message_wins_over_the_stack() {
        let stderr = b"file:///x/loadout.mjs:132\n  throw new Error('nope')\n\nError: unknown plugin or preset \"x\" (see: loadout presets)\n    at resolvePlugins (file:///x:1:1)\n\nNode.js v22.0.0\n";
        assert_eq!(
            diagnostic(stderr),
            "unknown plugin or preset \"x\" (see: loadout presets)"
        );
    }

    #[test]
    fn without_an_error_line_the_last_word_is_kept() {
        assert_eq!(diagnostic(b"something\nwent wrong\n"), "went wrong");
        assert_eq!(diagnostic(b""), "loadout.mjs failed");
    }

    #[test]
    fn an_older_engine_is_told_apart_from_a_failure() {
        assert!(is_unknown_command(
            "loadout: unknown command \"prompts\" (status, equip, presets, lint, readme)"
        ));
        assert!(!is_unknown_command("Node.js was not found on PATH"));
    }

    #[test]
    fn how_is_filled_only_where_missing() {
        let mut status = json!({ "plugins": [{ "capabilities": [{
            "requires": [{ "provider": "node" }],
            "chain": [{ "provider": "blender", "how": "kept" }, { "provider": "unknown" }]
        }]}]});
        let providers = json!({ "providers": {
            "node": { "how": "install Node 20" },
            "blender": { "how": "replaced?" }
        }});
        fill_how(&mut status, &providers);
        let cap = &status["plugins"][0]["capabilities"][0];
        assert_eq!(cap["requires"][0]["how"], "install Node 20");
        assert_eq!(cap["chain"][0]["how"], "kept");
        assert!(cap["chain"][1].get("how").is_none());
    }
}
