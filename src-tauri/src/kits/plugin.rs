//! The Claude Code plugin inside a kit: what it brings, and its MCP servers as
//! a `--mcp-config` file has to spell them.
//!
//! Claude Code started with `--strict-mcp-config` loads the servers of that
//! file and nothing else — a plugin's `.mcp.json` included (checked with
//! Claude Code 2.1.316: the plugin's skills load, its servers do not). A
//! workspace in `custom` MCP mode therefore gets its kits' servers copied into
//! its file, under the very name Claude Code gives a plugin's server,
//! `plugin:<plugin>:<server>`, so the tools are called the same in both modes
//! (`mcp__plugin_<plugin>_<server>__<tool>`), and with `${CLAUDE_PLUGIN_ROOT}`
//! written out as the plugin's folder, which nothing expands there.

use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use super::manifest::{self, Tab};
use super::store::Loaded;

/// What a kit brings, as the person is shown it before enabling it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginInfo>,
    pub skills: Vec<String>,
    pub commands: Vec<String>,
    pub agents: Vec<String>,
    /// Their names in the plugin's configuration, not prefixed.
    pub mcp_servers: Vec<String>,
    pub tabs: Vec<Tab>,
    /// The steps that have a command for this OS.
    pub setup: Vec<SetupCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PluginInfo {
    /// As its `plugin.json` names it: the prefix of its skills and commands.
    pub name: String,
    pub dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetupCommand {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub command: String,
}

/// Deep enough for any plugin's layout, short of walking a stray
/// `node_modules`.
const MAX_DEPTH: usize = 4;
const MAX_ITEMS: usize = 500;

pub fn inspect(kit: &Loaded, os: &str) -> Inspection {
    let plugin_dir = kit.plugin_dir();
    let plugin = plugin_dir.as_deref().map(|dir| PluginInfo {
        name: plugin_name(dir).unwrap_or_else(|| kit.manifest.id.clone()),
        dir: dir.to_string_lossy().into_owned(),
    });
    let (skills, commands, agents, mcp_servers) = match plugin_dir.as_deref() {
        Some(dir) => (
            skills(dir),
            markdown_names(&dir.join("commands")),
            markdown_names(&dir.join("agents")),
            servers(dir).into_iter().map(|(name, _)| name).collect(),
        ),
        None => Default::default(),
    };
    Inspection {
        plugin,
        skills,
        commands,
        agents,
        mcp_servers,
        tabs: kit.manifest.tabs.clone(),
        setup: kit
            .manifest
            .setup
            .iter()
            .filter_map(|step| {
                Some(SetupCommand {
                    id: step.id.clone(),
                    title: step.title.clone(),
                    description: step.description.clone(),
                    command: step.run.for_os(os)?.to_string(),
                })
            })
            .collect(),
    }
}

fn read_json(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// The `name` of `.claude-plugin/plugin.json`.
pub fn plugin_name(plugin_dir: &Path) -> Option<String> {
    read_json(&plugin_dir.join(".claude-plugin").join("plugin.json"))?
        .get("name")?
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// The servers a file declares: under `mcpServers`, or at its top level as a
/// plugin's `.mcp.json` may have them.
fn servers_in(value: &Value) -> Map<String, Value> {
    let map = match value.get("mcpServers") {
        Some(Value::Object(map)) => map,
        _ => match value {
            Value::Object(map) => map,
            _ => return Map::new(),
        },
    };
    map.iter()
        .filter(|(_, definition)| definition.is_object())
        .map(|(name, definition)| (name.clone(), definition.clone()))
        .collect()
}

/// The plugin's MCP servers, in name order: `mcpServers` of its
/// `plugin.json` — inline, or the relative path of a file — then its
/// `.mcp.json` for the names that did not already come.
pub fn servers(plugin_dir: &Path) -> Vec<(String, Value)> {
    let mut found = Map::new();
    let declared = read_json(&plugin_dir.join(".claude-plugin").join("plugin.json"))
        .and_then(|plugin| plugin.get("mcpServers").cloned());
    let mut sources = Vec::new();
    match declared {
        Some(Value::Object(inline)) => sources.push(Value::Object(inline)),
        Some(Value::String(relative)) => {
            if let Some(value) = manifest::normalize(&relative)
                .ok()
                .and_then(|relative| manifest::inside(plugin_dir, &relative).ok())
                .and_then(|path| read_json(&path))
            {
                sources.push(value);
            }
        }
        _ => {}
    }
    if let Some(value) = read_json(&plugin_dir.join(".mcp.json")) {
        sources.push(value);
    }
    for source in sources {
        for (name, definition) in servers_in(&source) {
            found.entry(name).or_insert(definition);
        }
    }
    let mut servers: Vec<(String, Value)> = found.into_iter().collect();
    servers.sort_by(|a, b| a.0.cmp(&b.0));
    servers
}

/// Every server of the kits' plugins, named and written out as a
/// `--mcp-config` file holds them.
pub fn scoped_servers(kits: &[Loaded]) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for kit in kits {
        let Some(dir) = kit.plugin_dir() else {
            continue;
        };
        let name = plugin_name(&dir).unwrap_or_else(|| kit.manifest.id.clone());
        // Claude Code spells the folder with forward slashes on every OS.
        let root = dir.to_string_lossy().replace('\\', "/");
        for (server, mut definition) in servers(&dir) {
            expand(&mut definition, &root);
            out.push((format!("plugin:{name}:{server}"), definition));
        }
    }
    out
}

fn expand(value: &mut Value, root: &str) {
    match value {
        Value::String(text) => {
            if text.contains("${CLAUDE_PLUGIN_ROOT}") {
                *text = text.replace("${CLAUDE_PLUGIN_ROOT}", root);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| expand(item, root)),
        Value::Object(map) => map.values_mut().for_each(|item| expand(item, root)),
        _ => {}
    }
}

/// `skills/<name>/SKILL.md`, by folder name.
fn skills(plugin_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(plugin_dir.join("skills"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .take(MAX_ITEMS)
        .collect();
    names.sort();
    names
}

/// The `.md` files under `dir`, by path without the extension, `:` between
/// folders as Claude Code namespaces them. Links are not followed.
fn markdown_names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    walk(dir, &mut Vec::new(), &mut names);
    names.sort();
    names
}

fn walk(dir: &Path, prefix: &mut Vec<String>, names: &mut Vec<String>) {
    if prefix.len() > MAX_DEPTH || names.len() >= MAX_ITEMS {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            prefix.push(name);
            walk(&entry.path(), prefix, names);
            prefix.pop();
        } else if kind.is_file() {
            if let Some(stem) = name.strip_suffix(".md") {
                let mut parts = prefix.clone();
                parts.push(stem.to_string());
                names.push(parts.join(":"));
            }
        }
        if names.len() >= MAX_ITEMS {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::store::Store;
    use crate::kits::testing::Folder;
    use serde_json::json;

    fn loaded(folder: &Folder, store: &Store) -> Loaded {
        store
            .add_folder(&folder.root.to_string_lossy(), 1)
            .and_then(|entry| store.loaded(&entry))
            .unwrap()
    }

    #[test]
    fn a_kit_is_inspected_for_what_it_brings() {
        let home = Folder::new("inspect");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("inspect-src");
        kit.kit(".", "studio", "1.0.0");
        kit.file("commands/build.md", "Build.")
            .file("commands/place/publish.md", "Publish.")
            .file("commands/notes.txt", "not a command")
            .file("agents/scripter.md", "An agent.")
            .file("skills/empty/README.md", "no SKILL.md")
            .file("tab.html", "")
            .file(
                manifest::FILE,
                &json!({
                    "schema": 1, "id": "studio", "name": "Studio", "version": "1.0.0", "description": "d",
                    "tabs": [{ "id": "explorer", "title": "Explorer", "entry": "tab.html", "permissions": ["panes.read"] }],
                    "setup": [
                        { "id": "a", "title": "A", "run": { "windows": "a.cmd", "linux": "./a" } },
                        { "id": "b", "title": "B", "run": { "macos": "./b" } }
                    ]
                })
                .to_string(),
            );
        let found = inspect(&loaded(&kit, &store), "windows");
        assert_eq!(found.plugin.as_ref().unwrap().name, "studio");
        assert_eq!(found.skills, ["hello"]);
        assert_eq!(found.commands, ["build", "place:publish"]);
        assert_eq!(found.agents, ["scripter"]);
        assert_eq!(found.mcp_servers, ["studio"]);
        assert_eq!(found.tabs[0].id, "explorer");
        assert_eq!(found.setup.len(), 1);
        assert_eq!(found.setup[0].command, "a.cmd");
    }

    #[test]
    fn a_kit_without_a_plugin_brings_only_its_tabs_and_steps() {
        let home = Folder::new("inspect-bare");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("inspect-bare-src");
        kit.manifest(".", "tabs-only", "1.0.0");
        let found = inspect(&loaded(&kit, &store), "linux");
        assert_eq!(found.plugin, None);
        assert!(found.skills.is_empty() && found.mcp_servers.is_empty());
        assert!(scoped_servers(&[loaded_again(&store, "tabs-only")]).is_empty());
    }

    fn loaded_again(store: &Store, id: &str) -> Loaded {
        store.loaded(&store.entry(id).unwrap()).unwrap()
    }

    #[test]
    fn servers_are_written_out_for_a_strict_config() {
        let home = Folder::new("scoped");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("scoped-src");
        kit.kit(".", "studio", "1.0.0");
        // The plugin is named apart from the kit, and declares one more
        // server inline.
        kit.file(
            ".claude-plugin/plugin.json",
            &json!({ "name": "roblox-studio", "mcpServers": {
                "assets": { "type": "http", "url": "http://127.0.0.1:44755/mcp" },
                "studio": { "command": "ignored, the inline one wins" }
            } })
            .to_string(),
        );
        let loaded = loaded(&kit, &store);
        let servers = scoped_servers(&[loaded]);
        let names: Vec<&str> = servers.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            ["plugin:roblox-studio:assets", "plugin:roblox-studio:studio"]
        );
        let root = kit.root.to_string_lossy().replace('\\', "/");
        assert_eq!(servers[1].1["command"], "ignored, the inline one wins");

        // The `.mcp.json` alone, with its root expanded.
        kit.plugin(".", "roblox-studio");
        let servers = scoped_servers(&[loaded_again(&store, "studio")]);
        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0].1,
            json!({ "command": "node", "args": [format!("{root}/server.js")] })
        );
    }

    #[test]
    fn a_flat_mcp_json_is_read_too() {
        let dir = Folder::new("flat");
        dir.plugin(".", "p").file(
            ".mcp.json",
            &json!({ "one": { "command": "x" }, "bad": 3 }).to_string(),
        );
        let servers = servers(&dir.root);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].0, "one");
    }
}
