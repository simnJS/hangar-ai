//! What a kit's Claude Code plugin runs by itself: its hooks, read from
//! `hooks/hooks.json` and from `hooks` in `.claude-plugin/plugin.json`.
//!
//! The person sees this list before enabling a kit, so the reading leans one
//! way: whatever may run gets listed. A hook that is not a `command` (a prompt,
//! a URL, a type a newer Claude Code knows) is written out as its JSON, a
//! `modules` entry (a hooks module, code Claude Code loads) by its path, and a
//! file this cannot read as it stands, under the file's name: Claude Code may
//! read what this does not, and nothing should run unseen.
//!
//! The list's hash is what a workspace accepts (docs/kits.md): the pseudo
//! permission `hooks:<first 16 hex of its SHA-256>`.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::manifest;
use super::setup::sha256_hex;

const PLUGIN_FILE: &str = ".claude-plugin/plugin.json";
const HOOKS_FILE: &str = "hooks/hooks.json";

/// The field order is the canonical form's, and the sorting's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Hook {
    pub event: String,
    /// `""`: each time the event happens.
    pub matcher: String,
    /// What it runs, exactly as the plugin writes it.
    pub command: String,
}

fn listed(event: &str, matcher: &str, command: &str) -> Hook {
    Hook {
        event: event.to_string(),
        matcher: matcher.to_string(),
        command: command.to_string(),
    }
}

/// A plugin's hooks, in canonical order and each once. No hooks, no list.
pub fn read(plugin_dir: &Path) -> Vec<Hook> {
    let mut found = Vec::new();
    let default = plugin_dir.join("hooks").join("hooks.json");
    if let Some(config) = read_file(&default, HOOKS_FILE, &mut found) {
        from_config(&config, HOOKS_FILE, &mut found);
    }
    let plugin = plugin_dir.join(".claude-plugin").join("plugin.json");
    if let Some(plugin) = read_file(&plugin, PLUGIN_FILE, &mut found) {
        if let Some(declared) = plugin.get("hooks") {
            from_declared(plugin_dir, declared, &mut found);
        }
    }
    found.sort();
    found.dedup();
    found
}

/// What accepting these hooks stores on a workspace; `None` for no hooks, which
/// store nothing.
pub fn grant(hooks: &[Hook]) -> Option<String> {
    if hooks.is_empty() {
        return None;
    }
    let mut canonical = hooks.to_vec();
    canonical.sort();
    canonical.dedup();
    let text = serde_json::to_string(&canonical).ok()?;
    Some(format!("hooks:{}", &sha256_hex(&text)[..16]))
}

/// A JSON file. One that is missing holds nothing; one that cannot be read is
/// listed as it stands under `label`.
fn read_file(path: &Path, label: &str, found: &mut Vec<Hook>) -> Option<Value> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            found.push(listed(label, "", &err.to_string()));
            return None;
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(value) => Some(value),
        Err(_) => {
            found.push(listed(label, "", &text));
            None
        }
    }
}

/// `hooks` in `plugin.json`: inline, the relative path of a file, or a list of
/// either.
fn from_declared(plugin_dir: &Path, declared: &Value, found: &mut Vec<Hook>) {
    match declared {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                from_declared(plugin_dir, item, found);
            }
        }
        Value::String(relative) => {
            let file = manifest::normalize(relative)
                .ok()
                .and_then(|path| manifest::inside(plugin_dir, &path).ok());
            match file {
                Some(file) => {
                    if let Some(config) = read_file(&file, relative, found) {
                        from_config(&config, relative, found);
                    }
                }
                None => found.push(listed(PLUGIN_FILE, "", relative)),
            }
        }
        inline => from_config(inline, PLUGIN_FILE, found),
    }
}

/// `{ "hooks": { <event>: […] }, "modules": […] }`, or the events at the top,
/// as `plugin.json` may have them inline.
fn from_config(config: &Value, label: &str, found: &mut Vec<Hook>) {
    let Value::Object(top) = config else {
        found.push(listed(label, "", &config.to_string()));
        return;
    };
    match top.get("hooks") {
        Some(Value::Object(events)) => {
            for (event, value) in events {
                from_event(event, value, found);
            }
            if let Some(modules) = top.get("modules") {
                from_event("modules", modules, found);
            }
        }
        _ => {
            for (event, value) in top {
                from_event(event, value, found);
            }
        }
    }
}

fn from_event(event: &str, value: &Value, found: &mut Vec<Hook>) {
    match value {
        Value::Array(items) => {
            for item in items {
                from_group(event, item, found);
            }
        }
        Value::Object(_) => from_group(event, value, found),
        // One module, without a list.
        Value::String(_) if event == "modules" => from_group(event, value, found),
        // `description`, `$schema`: no event holds a bare value.
        _ => {}
    }
}

/// `{ "matcher": "…", "hooks": [ … ] }`, or a hook given without a group.
fn from_group(event: &str, group: &Value, found: &mut Vec<Hook>) {
    let matcher = match group.get("matcher") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    };
    match group.get("hooks") {
        Some(Value::Array(hooks)) => {
            for hook in hooks {
                found.push(listed(event, &matcher, &command_of(hook)));
            }
        }
        _ => found.push(listed(event, &matcher, &command_of(group))),
    }
}

fn command_of(hook: &Value) -> String {
    match hook {
        Value::String(text) => text.clone(),
        other => match other.get("command") {
            Some(Value::String(command)) => command.clone(),
            _ => other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::testing::Folder;
    use serde_json::json;

    fn plugin(tag: &str, plugin_json: Value) -> Folder {
        let dir = Folder::new(tag);
        dir.file(PLUGIN_FILE, &plugin_json.to_string());
        dir
    }

    fn hook_file(matcher: &str, command: &str) -> String {
        json!({
            "description": "The kit's hooks",
            "hooks": {
                "PostToolUse": [{
                    "matcher": matcher,
                    "hooks": [{ "type": "command", "command": command, "timeout": 30 }]
                }],
                "SessionStart": [{
                    "hooks": [{ "type": "command", "command": "node ${CLAUDE_PLUGIN_ROOT}/start.js" }]
                }]
            }
        })
        .to_string()
    }

    #[test]
    fn hooks_are_read_from_their_file_verbatim() {
        let dir = plugin("hooks-file", json!({ "name": "p" }));
        dir.file(
            HOOKS_FILE,
            &hook_file("Write|Edit", "echo \"edited\" >> log.txt"),
        );
        assert_eq!(
            read(&dir.root),
            vec![
                listed("PostToolUse", "Write|Edit", "echo \"edited\" >> log.txt"),
                listed("SessionStart", "", "node ${CLAUDE_PLUGIN_ROOT}/start.js"),
            ]
        );
    }

    #[test]
    fn hooks_are_read_from_the_plugin_manifest_inline_or_by_path() {
        let events =
            json!({ "Stop": [{ "hooks": [{ "type": "command", "command": "inline.sh" }] }] });
        let flat = plugin("hooks-inline", json!({ "name": "p", "hooks": events }));
        assert_eq!(read(&flat.root), vec![listed("Stop", "", "inline.sh")]);

        let wrapped = plugin(
            "hooks-wrapped",
            json!({ "name": "p", "hooks": { "hooks": events } }),
        );
        assert_eq!(read(&wrapped.root), vec![listed("Stop", "", "inline.sh")]);

        let by_path = plugin(
            "hooks-path",
            json!({ "name": "p", "hooks": ["./config/more.json", { "Stop": events["Stop"] }] }),
        );
        by_path.file("config/more.json", &hook_file("Bash", "more.sh"));
        let found = read(&by_path.root);
        assert!(found.contains(&listed("PostToolUse", "Bash", "more.sh")));
        assert!(found.contains(&listed("Stop", "", "inline.sh")));
    }

    #[test]
    fn both_places_add_up_and_a_hook_listed_twice_counts_once() {
        let dir = plugin(
            "hooks-both",
            json!({ "name": "p", "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "inline.sh" }] }],
                "SessionStart": [{ "hooks": [{ "command": "node ${CLAUDE_PLUGIN_ROOT}/start.js" }] }]
            } }),
        );
        dir.file(HOOKS_FILE, &hook_file("", "file.sh"));
        assert_eq!(
            read(&dir.root),
            vec![
                listed("PostToolUse", "", "file.sh"),
                listed("SessionStart", "", "node ${CLAUDE_PLUGIN_ROOT}/start.js"),
                listed("Stop", "", "inline.sh"),
            ]
        );
    }

    #[test]
    fn a_plugin_without_hooks_has_an_empty_list_and_no_grant() {
        let dir = plugin("hooks-none", json!({ "name": "p" }));
        assert!(read(&dir.root).is_empty());
        assert_eq!(grant(&read(&dir.root)), None);
        dir.file(
            HOOKS_FILE,
            &json!({ "description": "nothing yet", "hooks": {} }).to_string(),
        );
        assert_eq!(grant(&read(&dir.root)), None);
    }

    #[test]
    fn whatever_else_may_run_is_listed_too() {
        let dir = plugin("hooks-other", json!({ "name": "p" }));
        dir.file(
            HOOKS_FILE,
            &json!({
                "hooks": { "Stop": [{ "hooks": [
                    { "type": "http", "url": "https://example.com/report" },
                    { "type": "prompt", "prompt": "Check the build." }
                ] }] },
                "modules": ["./register.js"]
            })
            .to_string(),
        );
        let found = read(&dir.root);
        assert!(
            found.contains(&listed("modules", "", "./register.js")),
            "{found:?}"
        );
        assert!(found
            .iter()
            .any(|hook| hook.command.contains("https://example.com/report")));
        assert!(found
            .iter()
            .any(|hook| hook.command.contains("Check the build.")));
    }

    #[test]
    fn a_hooks_file_with_only_modules_lists_them() {
        let dir = plugin("hooks-modules", json!({ "name": "p" }));
        dir.file(
            HOOKS_FILE,
            &json!({ "modules": ["./register.tsx"] }).to_string(),
        );
        assert_eq!(
            read(&dir.root),
            vec![listed("modules", "", "./register.tsx")]
        );
    }

    #[test]
    fn what_cannot_be_read_is_shown_rather_than_passed_over() {
        let dir = plugin(
            "hooks-broken",
            json!({ "name": "p", "hooks": "../outside.json" }),
        );
        dir.file(HOOKS_FILE, "{ not json: rm -rf");
        let found = read(&dir.root);
        assert!(
            found.contains(&listed(HOOKS_FILE, "", "{ not json: rm -rf")),
            "{found:?}"
        );
        assert!(
            found.contains(&listed(PLUGIN_FILE, "", "../outside.json")),
            "{found:?}"
        );
    }

    #[test]
    fn the_hash_is_stable_and_changes_with_a_command() {
        let dir = plugin("hooks-hash", json!({ "name": "p" }));
        dir.file(HOOKS_FILE, &hook_file("Bash", "one.sh"));
        let first = grant(&read(&dir.root)).unwrap();
        assert!(first.starts_with("hooks:"), "{first}");
        assert_eq!(first.len(), "hooks:".len() + 16, "{first}");
        assert!(first["hooks:".len()..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));

        // Read again, reformatted, its keys in another order: the same list.
        assert_eq!(grant(&read(&dir.root)).unwrap(), first);
        dir.file(
            HOOKS_FILE,
            &serde_json::to_string_pretty(&json!({
                "hooks": {
                    "SessionStart": [{
                        "hooks": [{ "command": "node ${CLAUDE_PLUGIN_ROOT}/start.js", "type": "command" }]
                    }],
                    "PostToolUse": [{
                        "hooks": [{ "command": "one.sh", "type": "command", "timeout": 5 }],
                        "matcher": "Bash"
                    }]
                }
            }))
            .unwrap(),
        );
        assert_eq!(grant(&read(&dir.root)).unwrap(), first);

        dir.file(HOOKS_FILE, &hook_file("Bash", "two.sh"));
        assert_ne!(grant(&read(&dir.root)).unwrap(), first, "another command");
        dir.file(HOOKS_FILE, &hook_file("Write", "one.sh"));
        assert_ne!(grant(&read(&dir.root)).unwrap(), first, "another matcher");
    }

    #[test]
    fn the_hash_is_the_documented_one() {
        let hooks = [listed("Stop", "", "x.sh")];
        let canonical = r#"[{"event":"Stop","matcher":"","command":"x.sh"}]"#;
        assert_eq!(
            grant(&hooks).unwrap(),
            format!("hooks:{}", &sha256_hex(canonical)[..16])
        );
    }
}
