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
//!
//! It also says what the plugin starts by itself beside its hooks (hooks.rs),
//! from the plugin schema of Claude Code 2.1.316: `running`. The person sees
//! that list before enabling a kit, so the reading leans the way the hooks' does:
//! whatever may run gets listed, and what cannot be read is listed as such. The
//! hash a workspace accepts covers it: `permission`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::commands::{self, CommandInfo};
use super::hooks::{self, Hook};
use super::manifest::{self, Tab};
use super::setup::sha256_hex;
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
    /// What the plugin runs by itself, as it writes it.
    pub hooks: Vec<Hook>,
    /// What else it starts by itself, or may, and what cannot be told.
    pub running: Vec<Running>,
    /// `hooks:<hash>` of those two lists, null when both are empty: what
    /// accepting them stores on a workspace.
    pub hooks_permission: Option<String>,
    pub tabs: Vec<Tab>,
    /// The steps that have a command for this OS.
    pub setup: Vec<SetupCommand>,
    /// What its tabs may run, with the argv for this OS, verbatim, placeholders
    /// included. `commands` above is the plugin's own: its slash commands.
    pub declared_commands: Vec<CommandInfo>,
    /// `commands:<hash>` of that list, null without any: what accepting these
    /// very commands stores on a workspace.
    pub commands_permission: Option<String>,
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

/// The kinds of `Running`, in the order the activation view shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    /// `.mcp.json`, `mcpServers`: a process, or a connection, for the session.
    Mcp,
    /// `monitors/monitors.json`, `monitors`, `experimental.monitors`.
    Monitor,
    /// `.lsp.json`, `lspServers`.
    Lsp,
    /// `hooks` in the frontmatter of a skill or a command.
    SkillHook,
    /// `settings.json`, `settings`: `agent` and `subagentStatusLine`.
    Settings,
    /// `experimental.syntaxHighlighting.hljsLanguages`: JavaScript Claude Code loads.
    Grammar,
    /// `bin/`: on the `PATH` of Claude's shell.
    Bin,
    /// `binaries`: fetched into `bin/` when Claude Code installs a plugin.
    Binary,
    /// `dependencies`: other plugins, enabled along with this one.
    Dependency,
    /// A key or a file this reading has no meaning for.
    Unknown,
}

/// Something a plugin starts by itself, or may, as `kit_inspect` lists it. Env
/// values and headers never leave Rust: only their names do, and `sealed` puts
/// them in the hash.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub kind: RunKind,
    pub name: String,
    /// The program, then its arguments, as written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    /// A shell command, an address, a path, as written: what `argv` is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// When it says so itself: a monitor's `when`, a language server's file types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The names of the variables it sets.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    /// The names of the headers it sends.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    /// What it runs is out of Hangar's reach: a bundle, a file that does not
    /// parse, remote code, another plugin.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unreadable: bool,
    /// All the plugin wrote for it, in a canonical form: hashed, never sent.
    #[serde(skip)]
    sealed: String,
}

impl Running {
    fn new(kind: RunKind, name: &str, sealed: String) -> Self {
        Running {
            kind,
            name: name.to_string(),
            argv: Vec::new(),
            text: None,
            when: None,
            description: None,
            env: Vec::new(),
            headers: Vec::new(),
            unreadable: false,
            sealed,
        }
    }
}

/// The keys of `plugin.json` that Claude Code 2.1.316 knows, the ones that start
/// nothing included. Any other holding text is listed as not understood.
const PLUGIN_KEYS: [&str; 34] = [
    "$schema",
    "name",
    "displayName",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "metadata",
    "icon",
    "documentationUrl",
    "supportUrl",
    "privacyPolicyUrl",
    "termsOfServiceUrl",
    "defaultEnabled",
    "dependencies",
    "settings",
    "userConfig",
    "types",
    "channels",
    "skills",
    "commands",
    "agents",
    "hooks",
    "mcpServers",
    "lspServers",
    "outputStyles",
    "workflows",
    "themes",
    "monitors",
    "binaries",
    "experimental",
];

/// The same for the keys of its `experimental`.
const EXPERIMENTAL_KEYS: [&str; 5] = [
    "themes",
    "monitors",
    "outputStyles",
    "evals",
    "syntaxHighlighting",
];

/// Keys that describe a file, in a file of servers.
const SCHEMA_KEYS: [&str; 2] = ["$schema", "description"];

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
    let (skills, commands, agents, mcp_servers, hooks, running) = match plugin_dir.as_deref() {
        Some(dir) => (
            skills(dir),
            markdown_names(&dir.join("commands")),
            markdown_names(&dir.join("agents")),
            servers(dir).into_iter().map(|(name, _)| name).collect(),
            hooks::read(dir),
            self::running(dir),
        ),
        None => Default::default(),
    };
    let hooks_permission = grant(&hooks, &running);
    Inspection {
        plugin,
        skills,
        commands,
        agents,
        mcp_servers,
        hooks,
        running,
        hooks_permission,
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
        declared_commands: commands::for_os(&kit.manifest, os),
        commands_permission: commands::grant(&kit.manifest, os),
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

/// The plugin's MCP servers, in name order: its `.mcp.json`, then `mcpServers` of
/// its `plugin.json` (inline, the relative path of a file, or a list of either),
/// a name declared later replacing an earlier one. A bundle (`.mcpb`) is not in
/// it: its servers are only known once Claude Code unpacks it.
pub fn servers(plugin_dir: &Path) -> Vec<(String, Value)> {
    let mut found = Map::new();
    for source in mcp_sources(plugin_dir, &manifest_of(plugin_dir), &mut Vec::new()) {
        found.extend(servers_in(&source));
    }
    let mut servers: Vec<(String, Value)> = found.into_iter().collect();
    servers.sort_by(|a, b| a.0.cmp(&b.0));
    servers
}

fn manifest_of(plugin_dir: &Path) -> Value {
    read_json(&plugin_dir.join(".claude-plugin").join("plugin.json")).unwrap_or(Value::Null)
}

/// What the plugin's MCP configuration reads as, in the order Claude Code loads it.
fn mcp_sources(plugin_dir: &Path, manifest: &Value, found: &mut Vec<Running>) -> Vec<Value> {
    let mut sources = Vec::new();
    if let Some(value) = read_listed(&plugin_dir.join(".mcp.json"), ".mcp.json", found) {
        sources.push(value);
    }
    if let Some(declared) = manifest.get("mcpServers") {
        declared_sources(plugin_dir, "mcpServers", declared, found, &mut sources);
    }
    sources
}

/// A key that takes an inline value, the relative path of a JSON file, or a list
/// of either. What it cannot use is listed, under the key.
fn declared_sources(
    plugin_dir: &Path,
    key: &str,
    declared: &Value,
    found: &mut Vec<Running>,
    sources: &mut Vec<Value>,
) {
    match declared {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                declared_sources(plugin_dir, key, item, found, sources);
            }
        }
        Value::Object(_) => sources.push(declared.clone()),
        Value::String(path) if key == "mcpServers" && is_bundle(path) => {
            found.push(bundle(plugin_dir, path));
        }
        Value::String(path) => match inside_plugin(plugin_dir, path) {
            Some(file) => {
                if let Some(value) = read_listed(&file, path, found) {
                    sources.push(value);
                }
            }
            None => found.push(not_understood(key, declared)),
        },
        other => found.push(not_understood(key, other)),
    }
}

fn inside_plugin(plugin_dir: &Path, relative: &str) -> Option<PathBuf> {
    manifest::normalize(relative)
        .ok()
        .and_then(|path| manifest::inside(plugin_dir, &path).ok())
}

/// A JSON file. One that is missing holds nothing. One that cannot be read is
/// listed by its name alone, as it may hold an env value, and its text is hashed.
fn read_listed(path: &Path, label: &str, found: &mut Vec<Running>) -> Option<Value> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            found.push(unreadable_file(label, err.to_string()));
            return None;
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(value) => Some(value),
        Err(_) => {
            found.push(unreadable_file(label, text.into_owned()));
            None
        }
    }
}

fn unreadable_file(label: &str, sealed: String) -> Running {
    Running {
        unreadable: true,
        ..Running::new(RunKind::Unknown, label, sealed)
    }
}

/// Listed as it is written, env values and headers apart.
fn not_understood(name: &str, value: &Value) -> Running {
    Running {
        text: Some(as_written(&redacted(value))),
        ..Running::new(RunKind::Unknown, name, canonical(value))
    }
}

fn as_written(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Env values, headers and tokens replaced by their names, at any depth.
fn redacted(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| {
                    let shown = if matches!(key.as_str(), "env" | "headers" | "authToken") {
                        Value::from(key_names(Some(item)))
                    } else {
                        redacted(item)
                    };
                    (key.clone(), shown)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redacted).collect()),
        other => other.clone(),
    }
}

fn key_names(value: Option<&Value>) -> Vec<String> {
    let mut names: Vec<String> = match value {
        Some(Value::Object(map)) => map.keys().cloned().collect(),
        _ => Vec::new(),
    };
    names.sort();
    names
}

/// JSON with its keys sorted at every level: reformatting a file changes nothing.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by_key(|(key, _)| *key);
            let parts: Vec<String> = entries
                .into_iter()
                .map(|(key, item)| format!("{}:{}", Value::from(key.as_str()), canonical(item)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

fn holds_text(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(items) => items.iter().any(holds_text),
        Value::Object(map) => map.values().any(holds_text),
        _ => false,
    }
}

/// What a plugin starts by itself beside its hooks, or may, in the view's order
/// and each once.
pub fn running(plugin_dir: &Path) -> Vec<Running> {
    let manifest = manifest_of(plugin_dir);
    let mut found = Vec::new();
    mcp_items(plugin_dir, &manifest, &mut found);
    monitor_items(plugin_dir, &manifest, &mut found);
    lsp_items(plugin_dir, &manifest, &mut found);
    skill_hook_items(plugin_dir, &manifest, &mut found);
    settings_items(plugin_dir, &manifest, &mut found);
    grammar_items(&manifest, &mut found);
    bin_items(plugin_dir, &mut found);
    binary_items(&manifest, &mut found);
    dependency_items(&manifest, &mut found);
    unknown_keys(&manifest, &mut found);
    found.sort();
    found.dedup();
    found
}

/// `hooks:<hash>` of what a plugin runs by itself: what accepting it stores on a
/// workspace; `None` when it runs nothing.
pub fn permission(plugin_dir: &Path) -> Option<String> {
    grant(&hooks::read(plugin_dir), &running(plugin_dir))
}

/// A plugin that only has hooks keeps the hash it always had (hooks.rs), so a
/// workspace that accepted them is not asked again. Anything else makes it the
/// hash of `[hooks, [[kind, name, what it wrote], …]]`: the person was never shown
/// those, so holding the older hash cannot stand for having accepted them.
pub fn grant(hook_list: &[Hook], running: &[Running]) -> Option<String> {
    if running.is_empty() {
        return hooks::grant(hook_list);
    }
    let mut canonical_hooks = hook_list.to_vec();
    canonical_hooks.sort();
    canonical_hooks.dedup();
    let mut items: Vec<(RunKind, &str, &str)> = running
        .iter()
        .map(|item| (item.kind, item.name.as_str(), item.sealed.as_str()))
        .collect();
    items.sort();
    items.dedup();
    let text = serde_json::to_string(&(&canonical_hooks, &items)).ok()?;
    Some(format!("hooks:{}", &sha256_hex(&text)[..16]))
}

fn mcp_items(plugin_dir: &Path, manifest: &Value, found: &mut Vec<Running>) {
    let mut servers = Map::new();
    for source in mcp_sources(plugin_dir, manifest, found) {
        let map = match source.get("mcpServers") {
            Some(Value::Object(map)) => map,
            _ => match &source {
                Value::Object(map) => map,
                other => {
                    found.push(not_understood("mcpServers", other));
                    continue;
                }
            },
        };
        for (name, item) in map {
            if !item.is_object() && holds_text(item) && !SCHEMA_KEYS.contains(&name.as_str()) {
                found.push(not_understood(&format!("mcpServers › {name}"), item));
            }
        }
        servers.extend(servers_in(&source));
    }
    for (name, definition) in &servers {
        found.extend(mcp_server(name, definition));
    }
}

/// A server's command line or its address, and its `headersHelper`, which is a
/// command of its own.
fn mcp_server(name: &str, definition: &Value) -> Vec<Running> {
    let transport = definition.get("type").and_then(Value::as_str);
    let command = definition.get("command").and_then(Value::as_str);
    let url = definition.get("url").and_then(Value::as_str);
    let mut item = Running::new(RunKind::Mcp, name, canonical(definition));
    match (transport, command, url) {
        (None | Some("stdio"), Some(command), _) => {
            item.argv = argv_of(command, definition.get("args"));
        }
        (Some("stdio"), None, _) => {
            return vec![not_understood(&format!("mcpServers › {name}"), definition)];
        }
        (_, _, Some(url)) => item.text = Some(url.to_string()),
        _ => return vec![not_understood(&format!("mcpServers › {name}"), definition)],
    }
    item.env = key_names(definition.get("env"));
    item.headers = key_names(definition.get("headers"));
    let helper = definition.get("headersHelper").and_then(Value::as_str);
    let mut items = vec![item];
    if let Some(helper) = helper {
        items.push(Running {
            text: Some(helper.to_string()),
            ..Running::new(
                RunKind::Mcp,
                &format!("{name} · headersHelper"),
                helper.to_string(),
            )
        });
    }
    items
}

fn argv_of(command: &str, args: Option<&Value>) -> Vec<String> {
    let mut argv = vec![command.to_string()];
    match args {
        Some(Value::Array(items)) => argv.extend(items.iter().map(as_written)),
        Some(Value::String(text)) => argv.push(text.clone()),
        _ => {}
    }
    argv
}

fn is_bundle(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".mcpb") || lower.ends_with(".dxt")
}

/// A packed server: what it starts is inside. A file of the plugin's own is
/// hashed for what it holds; an address only for what it says.
fn bundle(plugin_dir: &Path, path: &str) -> Running {
    let held = inside_plugin(plugin_dir, path)
        .and_then(|file| fs::read(file).ok())
        .map(|bytes| {
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .unwrap_or_default();
    Running {
        text: Some(path.to_string()),
        unreadable: true,
        ..Running::new(RunKind::Mcp, path, format!("{path}\n{held}"))
    }
}

fn monitor_items(plugin_dir: &Path, manifest: &Value, found: &mut Vec<Running>) {
    let mut sources = Vec::new();
    let default = plugin_dir.join("monitors").join("monitors.json");
    if let Some(value) = read_listed(&default, "monitors/monitors.json", found) {
        sources.push(value);
    }
    let experimental = manifest.get("experimental").and_then(|e| e.get("monitors"));
    for declared in [manifest.get("monitors"), experimental]
        .into_iter()
        .flatten()
    {
        match declared {
            Value::Null => {}
            Value::Array(_) => sources.push(declared.clone()),
            Value::String(path) => match inside_plugin(plugin_dir, path) {
                Some(file) => {
                    if let Some(value) = read_listed(&file, path, found) {
                        sources.push(value);
                    }
                }
                None => found.push(not_understood("monitors", declared)),
            },
            other => found.push(not_understood("monitors", other)),
        }
    }
    for source in sources {
        let Value::Array(entries) = &source else {
            found.push(not_understood("monitors", &source));
            continue;
        };
        for entry in entries {
            found.push(monitor(entry));
        }
    }
}

fn monitor(entry: &Value) -> Running {
    let text = |key: &str| entry.get(key).and_then(Value::as_str);
    match (text("name"), text("command")) {
        (Some(name), Some(command)) => Running {
            text: Some(command.to_string()),
            when: text("when").map(str::to_string),
            description: text("description").map(str::to_string),
            ..Running::new(RunKind::Monitor, name, canonical(entry))
        },
        _ => not_understood("monitors", entry),
    }
}

fn lsp_items(plugin_dir: &Path, manifest: &Value, found: &mut Vec<Running>) {
    let mut sources = Vec::new();
    if let Some(value) = read_listed(&plugin_dir.join(".lsp.json"), ".lsp.json", found) {
        sources.push(value);
    }
    if let Some(declared) = manifest.get("lspServers") {
        declared_sources(plugin_dir, "lspServers", declared, found, &mut sources);
    }
    let mut servers = Map::new();
    for source in &sources {
        let map = match source.get("lspServers") {
            Some(Value::Object(map)) => map,
            _ => match source {
                Value::Object(map) => map,
                other => {
                    found.push(not_understood("lspServers", other));
                    continue;
                }
            },
        };
        for (name, item) in map {
            if item.is_object() || !SCHEMA_KEYS.contains(&name.as_str()) {
                servers.insert(name.clone(), item.clone());
            }
        }
    }
    for (name, definition) in &servers {
        let Some(command) = definition.get("command").and_then(Value::as_str) else {
            if holds_text(definition) {
                found.push(not_understood(&format!("lspServers › {name}"), definition));
            }
            continue;
        };
        let extensions = key_names(definition.get("extensionToLanguage"));
        found.push(Running {
            argv: argv_of(command, definition.get("args")),
            when: (!extensions.is_empty()).then(|| extensions.join(", ")),
            env: key_names(definition.get("env")),
            ..Running::new(RunKind::Lsp, name, canonical(definition))
        });
    }
}

/// The `hooks` a skill or a command declares in its frontmatter, as they are
/// written: there is no YAML reader here, and no need for more than the lines.
fn skill_hook_items(plugin_dir: &Path, manifest: &Value, found: &mut Vec<Running>) {
    let mut roots: BTreeSet<PathBuf> = ["skills", "commands", "SKILL.md"]
        .map(|root| plugin_dir.join(root))
        .into_iter()
        .collect();
    for key in ["skills", "commands"] {
        for path in declared_paths(manifest.get(key)) {
            if let Some(file) = inside_plugin(plugin_dir, &path) {
                roots.insert(file);
            }
        }
    }
    let mut files = BTreeSet::new();
    let mut complete = true;
    for root in roots {
        complete &= markdown_files(&root, 0, &mut files);
    }
    for file in files {
        let relative = file.strip_prefix(plugin_dir).unwrap_or(&file);
        let name = relative.to_string_lossy().replace('\\', "/");
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(err) => {
                found.push(unreadable_file(&name, err.to_string()));
                continue;
            }
        };
        if let Some(block) = frontmatter_hooks(&String::from_utf8_lossy(&bytes)) {
            found.push(skill_hooks(&name, block));
        }
    }
    if let Some(Value::Object(commands)) = manifest.get("commands") {
        for (name, command) in commands {
            let content = command.get("content").and_then(Value::as_str);
            if let Some(block) = content.and_then(frontmatter_hooks) {
                found.push(skill_hooks(&format!("commands › {name}"), block));
            }
        }
    }
    if !complete {
        found.push(unreadable_file(
            &format!("skills, commands: more than {MAX_ITEMS} files, or folders too deep"),
            String::new(),
        ));
    }
}

fn skill_hooks(name: &str, block: String) -> Running {
    Running {
        text: Some(block.clone()),
        ..Running::new(RunKind::SkillHook, name, block)
    }
}

/// The paths a `skills` or `commands` key names: one, a list, or a command map's
/// `source`s.
fn declared_paths(declared: Option<&Value>) -> Vec<String> {
    match declared {
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::Object(map)) => map
            .values()
            .filter_map(|entry| entry.get("source").and_then(Value::as_str))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// The `.md` files at `path`, a file or a folder; false once there are too many
/// to read, or a folder lies too deep. Links, `node_modules` and hidden folders
/// are not followed.
fn markdown_files(path: &Path, depth: usize, files: &mut BTreeSet<PathBuf>) -> bool {
    if files.len() >= MAX_ITEMS {
        return false;
    }
    if path.is_file() {
        if path.extension().is_some_and(|ext| ext == "md") {
            files.insert(path.to_path_buf());
        }
        return true;
    }
    if depth > MAX_DEPTH {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return true;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let inner = entry.path();
        let complete = match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                name.starts_with('.')
                    || name == "node_modules"
                    || markdown_files(&inner, depth + 1, files)
            }
            Ok(kind) if kind.is_file() => markdown_files(&inner, depth + 1, files),
            _ => true,
        };
        if !complete {
            return false;
        }
    }
    true
}

/// The lines of a top-level `hooks` key in a Markdown file's frontmatter.
fn frontmatter_hooks(text: &str) -> Option<String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut block: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim_end() == "---" {
            break;
        }
        if block.is_empty() {
            let top_level = !line.starts_with([' ', '\t']);
            let key = line.split(':').next().unwrap_or("").trim();
            if top_level && line.contains(':') && key.trim_matches(['"', '\'']) == "hooks" {
                block.push(line);
            }
        } else if line.trim().is_empty() || line.starts_with([' ', '\t', '-']) {
            block.push(line);
        } else {
            break;
        }
    }
    while block.last().is_some_and(|line| line.trim().is_empty()) {
        block.pop();
    }
    let first = block.first()?;
    let value = first.split_once(':').map_or("", |(_, value)| value.trim());
    if block.len() == 1 && matches!(value, "" | "null" | "~" | "{}" | "[]") {
        return None;
    }
    Some(block.join("\n"))
}

/// `agent` and `subagentStatusLine` take effect from a plugin. The rest is dropped
/// today, and listed as not understood when it holds text.
fn settings_items(plugin_dir: &Path, manifest: &Value, found: &mut Vec<Running>) {
    let mut sources = Vec::new();
    if let Some(value) = read_listed(&plugin_dir.join("settings.json"), "settings.json", found) {
        sources.push(("settings.json", value));
    }
    if let Some(value) = manifest.get("settings") {
        sources.push(("settings", value.clone()));
    }
    for (label, source) in sources {
        let Value::Object(map) = &source else {
            if holds_text(&source) {
                found.push(not_understood(label, &source));
            }
            continue;
        };
        for (key, value) in map {
            match key.as_str() {
                "agent" => found.push(Running {
                    text: Some(as_written(value)),
                    ..Running::new(RunKind::Settings, key, canonical(value))
                }),
                "subagentStatusLine" => {
                    let command = value.get("command").and_then(Value::as_str);
                    found.push(Running {
                        text: Some(command.map_or_else(|| value.to_string(), str::to_string)),
                        ..Running::new(RunKind::Settings, key, canonical(value))
                    });
                }
                _ if holds_text(value) => {
                    found.push(not_understood(&format!("{label} › {key}"), value));
                }
                _ => {}
            }
        }
    }
}

fn grammar_items(manifest: &Value, found: &mut Vec<Running>) {
    let highlighting = manifest
        .get("experimental")
        .and_then(|experimental| experimental.get("syntaxHighlighting"));
    let Some(highlighting) = highlighting else {
        return;
    };
    let label = "experimental › syntaxHighlighting";
    let Some(Value::Array(languages)) = highlighting.get("hljsLanguages") else {
        if holds_text(highlighting) {
            found.push(not_understood(label, highlighting));
        }
        return;
    };
    for language in languages {
        let Some(id) = language.get("id").and_then(Value::as_str) else {
            found.push(not_understood(label, language));
            continue;
        };
        found.push(Running {
            text: language
                .get("remote")
                .and_then(Value::as_str)
                .map(str::to_string),
            unreadable: true,
            ..Running::new(RunKind::Grammar, id, canonical(language))
        });
    }
}

/// `bin/` by name: only its direct children are on the `PATH`.
fn bin_items(plugin_dir: &Path, found: &mut Vec<Running>) {
    let mut names: Vec<String> = fs::read_dir(plugin_dir.join("bin"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    if names.len() > MAX_ITEMS {
        found.push(unreadable_file(
            &format!("bin: more than {MAX_ITEMS} files"),
            names.join("\n"),
        ));
    }
    for name in names.into_iter().take(MAX_ITEMS) {
        found.push(Running {
            text: Some(format!("bin/{name}")),
            ..Running::new(RunKind::Bin, &name, name.clone())
        });
    }
}

fn binary_items(manifest: &Value, found: &mut Vec<Running>) {
    let Some(binaries) = manifest.get("binaries") else {
        return;
    };
    let Value::Object(map) = binaries else {
        if holds_text(binaries) {
            found.push(not_understood("binaries", binaries));
        }
        return;
    };
    for (name, entry) in map {
        let hash = entry.get("sha256").and_then(Value::as_str);
        found.push(Running {
            text: Some(hash.map_or_else(|| entry.to_string(), |hash| format!("sha256:{hash}"))),
            unreadable: true,
            ..Running::new(RunKind::Binary, name, canonical(entry))
        });
    }
}

fn dependency_items(manifest: &Value, found: &mut Vec<Running>) {
    let Some(dependencies) = manifest.get("dependencies") else {
        return;
    };
    let Value::Array(entries) = dependencies else {
        if holds_text(dependencies) {
            found.push(not_understood("dependencies", dependencies));
        }
        return;
    };
    for entry in entries {
        let name = entry
            .as_str()
            .or_else(|| entry.get("name").and_then(Value::as_str));
        let Some(name) = name else {
            found.push(not_understood("dependencies", entry));
            continue;
        };
        found.push(Running {
            text: Some(as_written(entry)),
            unreadable: true,
            ..Running::new(RunKind::Dependency, name, canonical(entry))
        });
    }
}

/// A key Claude Code does not know today may mean something in its next version:
/// every one that holds text is listed, a path and a command among them.
fn unknown_keys(manifest: &Value, found: &mut Vec<Running>) {
    let Value::Object(map) = manifest else {
        return;
    };
    for (key, value) in map {
        if !PLUGIN_KEYS.contains(&key.as_str()) && holds_text(value) {
            found.push(not_understood(&format!("plugin.json › {key}"), value));
        }
    }
    if let Some(Value::Object(experimental)) = map.get("experimental") {
        for (key, value) in experimental {
            if !EXPERIMENTAL_KEYS.contains(&key.as_str()) && holds_text(value) {
                found.push(not_understood(
                    &format!("plugin.json › experimental › {key}"),
                    value,
                ));
            }
        }
    }
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
        assert!(found.hooks.is_empty());
        assert_eq!(
            found.running.len(),
            1,
            "its MCP server: {:?}",
            found.running
        );
        assert_eq!(found.hooks_permission, grant(&[], &found.running));
        assert!(found.hooks_permission.is_some());
    }

    #[test]
    fn an_inspection_lists_the_plugins_hooks_and_what_accepting_them_stores() {
        let home = Folder::new("inspect-hooks");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("inspect-hooks-src");
        kit.kit(".", "studio", "1.0.0");
        kit.file(
            "hooks/hooks.json",
            &json!({ "hooks": { "PostToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": "echo ran >> \"$HANGAR_KITS_DATA/studio/log\"" }]
            }] } })
            .to_string(),
        );
        let found = inspect(&loaded(&kit, &store), "linux");
        assert_eq!(found.hooks.len(), 1);
        assert_eq!(found.hooks[0].event, "PostToolUse");
        assert_eq!(found.hooks[0].matcher, "Bash");
        assert_eq!(
            found.hooks[0].command,
            "echo ran >> \"$HANGAR_KITS_DATA/studio/log\""
        );
        assert_eq!(found.hooks_permission, grant(&found.hooks, &found.running));
        let sent = serde_json::to_value(&found).unwrap();
        assert_eq!(sent["hooks"][0]["matcher"], "Bash");
        assert_eq!(
            sent["hooksPermission"],
            json!(found.hooks_permission.unwrap())
        );
    }

    #[test]
    fn an_inspection_lists_the_commands_verbatim_and_what_accepting_them_stores() {
        let home = Folder::new("inspect-commands");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("inspect-commands-src");
        kit.kit(".", "studio", "1.0.0");
        kit.file("commands/build.md", "A slash command.")
            .file("tab.html", "")
            .file(
                manifest::FILE,
                &json!({
                    "schema": 1, "id": "studio", "name": "Studio", "version": "1.0.0", "description": "d",
                    "tabs": [{ "id": "ci", "title": "CI", "entry": "tab.html", "permissions": ["command:ci-run"] }],
                    "commands": {
                        "ci-run": {
                            "description": "A CI run.",
                            "run": {
                                "windows": ["gh.exe", "run", "view", "{run}", "--repo={repo}"],
                                "linux": ["gh", "run", "view", "{run}", "--repo={repo}"]
                            },
                            "args": {
                                "run": { "pattern": "[0-9]+", "description": "Its number" },
                                "repo": { "pattern": "[\\w.-]+/[\\w.-]+" }
                            }
                        }
                    }
                })
                .to_string(),
            );
        let found = inspect(&loaded(&kit, &store), "windows");
        assert_eq!(found.commands, ["build"], "the plugin's own stay apart");
        assert_eq!(found.declared_commands.len(), 1);
        assert_eq!(
            found.declared_commands[0].argv,
            ["gh.exe", "run", "view", "{run}", "--repo={repo}"]
        );
        let on_linux = inspect(&loaded_again(&store, "studio"), "linux");
        assert_eq!(on_linux.declared_commands[0].argv[0], "gh");
        assert_ne!(on_linux.commands_permission, found.commands_permission);

        let sent = serde_json::to_value(&found).unwrap();
        let command = &sent["declaredCommands"][0];
        assert_eq!(command["name"], "ci-run");
        assert_eq!(command["description"], "A CI run.");
        assert_eq!(command["argv"][3], "{run}");
        assert_eq!(command["args"]["run"]["pattern"], "[0-9]+");
        assert_eq!(command["args"]["run"]["description"], "Its number");
        assert_eq!(command["timeoutSeconds"], 30);
        assert_eq!(
            sent["commandsPermission"],
            json!(found.commands_permission.unwrap())
        );
        assert_eq!(sent["tabs"][0]["permissions"][0], "command:ci-run");
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

    #[test]
    fn servers_declared_in_a_list_are_read_and_a_later_name_replaces_an_earlier_one() {
        let dir = Folder::new("mcp-list");
        dir.file(
            ".claude-plugin/plugin.json",
            &json!({ "name": "p", "mcpServers": [
                "./config/servers.json",
                { "two": { "command": "inline" } },
                "./packed.mcpb"
            ] })
            .to_string(),
        )
        .file(
            ".mcp.json",
            &json!({ "one": { "command": "default" }, "two": { "command": "default" } })
                .to_string(),
        )
        .file(
            "config/servers.json",
            &json!({ "mcpServers": { "one": { "command": "file" } } }).to_string(),
        );
        let servers = servers(&dir.root);
        let written: Vec<(&str, &str)> = servers
            .iter()
            .map(|(name, server)| (name.as_str(), server["command"].as_str().unwrap()))
            .collect();
        assert_eq!(written, [("one", "file"), ("two", "inline")]);
    }

    fn of<'a>(found: &'a [Running], kind: RunKind, name: &str) -> &'a Running {
        found
            .iter()
            .find(|item| item.kind == kind && item.name == name)
            .unwrap_or_else(|| panic!("no {kind:?} {name} in {found:#?}"))
    }

    /// One plugin holding every kind that runs by itself, each where Claude Code
    /// 2.1.316 reads it.
    fn every_kind(tag: &str) -> Folder {
        let dir = Folder::new(tag);
        dir.file(
            ".claude-plugin/plugin.json",
            &json!({
                "name": "everything",
                "version": "1.0.0",
                "metadata": { "catalog": "./ignored" },
                "homepage": "https://example.com",
                "dependencies": ["audit-logger", { "name": "vault", "version": "~2.1.0" }],
                "binaries": { "fetched-tool": { "sha256": "ab".repeat(32) } },
                "mcpServers": "./packed.mcpb",
                "settings": { "subagentStatusLine": { "type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}\"/line.sh" } },
                "skills": ["./extra-skills"],
                "commands": { "inline": { "content": "---\nhooks:\n  Stop:\n    - hooks:\n        - command: inline-cmd.sh\n---\nBody." } },
                "experimental": {
                    "syntaxHighlighting": { "hljsLanguages": [
                        { "id": "gdscript", "remote": "npm:hljs-gdscript@1.0.0", "integrity": "sha256-AAAA" }
                    ] },
                    "launcher": { "run": "./start.sh" }
                },
                "startup": "node ./boot.js",
                "retries": 3
            })
            .to_string(),
        )
        .file(
            ".mcp.json",
            &json!({ "mcpServers": {
                "local": {
                    "command": "node",
                    "args": ["${CLAUDE_PLUGIN_ROOT}/server.js", "--port", "4 4"],
                    "env": { "API_TOKEN": "s3cret-env-value", "ROOT": "${CLAUDE_PLUGIN_ROOT}" }
                },
                "remote": {
                    "type": "http",
                    "url": "https://api.example.com/mcp",
                    "headers": { "Authorization": "Bearer s3cret-header-value" },
                    "headersHelper": "node ./headers.js"
                },
                "odd": { "type": "grpc", "env": { "PASS": "s3cret-odd-value" } }
            } })
            .to_string(),
        )
        .file(
            ".lsp.json",
            &json!({ "gopls": {
                "command": "gopls",
                "args": ["serve"],
                "extensionToLanguage": { ".go": "go", ".mod": "go.mod" },
                "env": { "GOFLAGS": "s3cret-lsp-value" }
            } })
            .to_string(),
        )
        .file(
            "monitors/monitors.json",
            &json!([
                { "name": "error-log", "command": "tail -F ./logs/error.log", "description": "The error log" },
                { "name": "deploy", "command": "./poll.sh", "description": "Deploys", "when": "on-skill-invoke:deploy" }
            ])
            .to_string(),
        )
        .file(
            "skills/guarded/SKILL.md",
            "---\ndescription: A skill.\nhooks:\n  PreToolUse:\n    - matcher: Bash\n      hooks:\n        - command: check.sh\nmodel: sonnet\n---\nBody.\n",
        )
        .file("skills/plain/SKILL.md", "---\ndescription: No hook.\n---\nThe word hooks: in the body.\n")
        .file(
            "extra-skills/other/SKILL.md",
            "---\nhooks:\n  Stop:\n    - hooks:\n        - command: extra.sh\n---\n",
        )
        .file("commands/old.md", "---\nhooks:\n  Stop: []\n---\n")
        .file("settings.json", &json!({ "agent": "reviewer", "statusLine": { "command": "ignored.sh" } }).to_string())
        .file("bin/tool", "#!/bin/sh\n")
        .file("bin/sub/not-on-the-path", "")
        .file(
            "hooks/hooks.json",
            &json!({ "modules": ["./register.js"], "hooks": { "Stop": [{ "hooks": [{ "command": "stop.sh" }] }] } })
                .to_string(),
        )
        .file("packed.mcpb", "bundle");
        dir
    }

    #[test]
    fn every_kind_is_listed_verbatim_with_what_it_starts() {
        let dir = every_kind("running-all");
        let found = running(&dir.root);

        let local = of(&found, RunKind::Mcp, "local");
        assert_eq!(
            local.argv,
            ["node", "${CLAUDE_PLUGIN_ROOT}/server.js", "--port", "4 4"]
        );
        assert_eq!(local.env, ["API_TOKEN", "ROOT"]);
        assert!(!local.unreadable);
        assert!(of(&found, RunKind::Mcp, "./packed.mcpb").unreadable);

        let remote = of(&found, RunKind::Mcp, "remote");
        assert_eq!(remote.text.as_deref(), Some("https://api.example.com/mcp"));
        assert_eq!(remote.headers, ["Authorization"]);
        assert_eq!(
            of(&found, RunKind::Mcp, "remote · headersHelper")
                .text
                .as_deref(),
            Some("node ./headers.js")
        );

        let gopls = of(&found, RunKind::Lsp, "gopls");
        assert_eq!(gopls.argv, ["gopls", "serve"]);
        assert_eq!(gopls.when.as_deref(), Some(".go, .mod"));
        assert_eq!(gopls.env, ["GOFLAGS"]);

        let log = of(&found, RunKind::Monitor, "error-log");
        assert_eq!(log.text.as_deref(), Some("tail -F ./logs/error.log"));
        assert_eq!(log.when, None, "without `when`: at session start");
        assert_eq!(log.description.as_deref(), Some("The error log"));
        let deploy = of(&found, RunKind::Monitor, "deploy");
        assert_eq!(deploy.when.as_deref(), Some("on-skill-invoke:deploy"));

        let guarded = of(&found, RunKind::SkillHook, "skills/guarded/SKILL.md");
        assert_eq!(
            guarded.text.as_deref(),
            Some("hooks:\n  PreToolUse:\n    - matcher: Bash\n      hooks:\n        - command: check.sh")
        );
        assert!(
            of(&found, RunKind::SkillHook, "extra-skills/other/SKILL.md")
                .text
                .as_deref()
                .unwrap()
                .contains("extra.sh")
        );
        assert!(of(&found, RunKind::SkillHook, "commands/old.md")
            .text
            .as_deref()
            .unwrap()
            .contains("Stop"));
        assert!(of(&found, RunKind::SkillHook, "commands › inline")
            .text
            .as_deref()
            .unwrap()
            .contains("inline-cmd.sh"));
        assert!(
            !found
                .iter()
                .any(|item| item.name == "skills/plain/SKILL.md"),
            "the word in a body is no hook"
        );

        assert_eq!(
            of(&found, RunKind::Settings, "agent").text.as_deref(),
            Some("reviewer")
        );
        assert_eq!(
            of(&found, RunKind::Settings, "subagentStatusLine")
                .text
                .as_deref(),
            Some("\"${CLAUDE_PLUGIN_ROOT}\"/line.sh")
        );

        let grammar = of(&found, RunKind::Grammar, "gdscript");
        assert_eq!(grammar.text.as_deref(), Some("npm:hljs-gdscript@1.0.0"));
        assert!(grammar.unreadable, "remote code cannot be read");

        assert_eq!(
            of(&found, RunKind::Bin, "tool").text.as_deref(),
            Some("bin/tool")
        );
        assert!(
            !found.iter().any(|item| item.name == "not-on-the-path"),
            "only bin/'s direct children are on the PATH"
        );

        let fetched = of(&found, RunKind::Binary, "fetched-tool");
        assert_eq!(fetched.text, Some(format!("sha256:{}", "ab".repeat(32))));
        assert!(fetched.unreadable);

        assert!(of(&found, RunKind::Dependency, "audit-logger").unreadable);
        assert!(of(&found, RunKind::Dependency, "vault")
            .text
            .as_deref()
            .unwrap()
            .contains("~2.1.0"));

        assert_eq!(
            of(&found, RunKind::Unknown, "plugin.json › startup")
                .text
                .as_deref(),
            Some("node ./boot.js")
        );
        assert!(of(
            &found,
            RunKind::Unknown,
            "plugin.json › experimental › launcher"
        )
        .text
        .as_deref()
        .unwrap()
        .contains("./start.sh"));
        assert!(of(&found, RunKind::Unknown, "settings.json › statusLine")
            .text
            .as_deref()
            .unwrap()
            .contains("ignored.sh"));
        assert!(
            !found.iter().any(|item| item.name.contains("retries")),
            "a number names nothing"
        );
        assert!(
            !found.iter().any(|item| item.name.contains("metadata")),
            "metadata is known and read by nothing"
        );
    }

    #[test]
    fn env_values_and_headers_never_leave_rust_only_their_names() {
        let dir = every_kind("running-secrets");
        let home = Folder::new("running-secrets-home");
        dir.manifest(".", "everything", "1.0.0");
        let store = Store::at(home.root.join("kits"));
        let sent = serde_json::to_string(&inspect(&loaded(&dir, &store), "linux")).unwrap();
        assert!(!sent.contains("s3cret"), "{sent}");
        assert!(!sent.contains("sealed"), "{sent}");
        assert!(sent.contains("API_TOKEN"), "{sent}");
        assert!(sent.contains("Authorization"), "{sent}");
        assert!(sent.contains("GOFLAGS"), "{sent}");
        // A definition this reading does not understand keeps the names too.
        let found = running(&dir.root);
        let odd = of(&found, RunKind::Unknown, "mcpServers › odd");
        assert!(odd.text.as_deref().unwrap().contains("PASS"));
        assert!(!odd.text.as_deref().unwrap().contains("s3cret"));
    }

    #[test]
    fn what_hangar_cannot_read_says_so_and_is_never_left_out() {
        let dir = Folder::new("running-unreadable");
        dir.file(
            ".claude-plugin/plugin.json",
            &json!({ "name": "p", "mcpServers": [
                "./packed.mcpb",
                "https://example.com/server.dxt",
                "../outside.json"
            ], "lspServers": 3, "monitors": { "name": "x" }, "binaries": "./fetch.sh" })
            .to_string(),
        )
        .file("packed.mcpb", "bundle")
        .file(".lsp.json", "{ not json: \"TOKEN=s3cret\" ");
        let found = running(&dir.root);

        for name in ["./packed.mcpb", "https://example.com/server.dxt"] {
            let bundle = of(&found, RunKind::Mcp, name);
            assert!(bundle.unreadable, "{name}");
            assert_eq!(bundle.text.as_deref(), Some(name));
        }
        assert_eq!(
            of(&found, RunKind::Unknown, "mcpServers").text.as_deref(),
            Some("../outside.json")
        );
        let broken = of(&found, RunKind::Unknown, ".lsp.json");
        assert!(broken.unreadable);
        assert_eq!(broken.text, None, "a broken file may hold an env value");
        assert_eq!(
            of(&found, RunKind::Unknown, "lspServers").text.as_deref(),
            Some("3")
        );
        assert!(of(&found, RunKind::Unknown, "monitors")
            .text
            .as_deref()
            .unwrap()
            .contains("name"));
        assert_eq!(
            of(&found, RunKind::Unknown, "binaries").text.as_deref(),
            Some("./fetch.sh")
        );
    }

    #[test]
    fn monitors_are_read_from_each_place_claude_code_takes_them() {
        let top = Folder::new("monitors-top");
        top.file(
            ".claude-plugin/plugin.json",
            &json!({ "name": "p", "monitors": [
                { "name": "inline", "command": "inline.sh", "description": "d" }
            ] })
            .to_string(),
        );
        assert_eq!(
            of(&running(&top.root), RunKind::Monitor, "inline")
                .text
                .as_deref(),
            Some("inline.sh")
        );

        let nested = Folder::new("monitors-nested");
        nested
            .file(
                ".claude-plugin/plugin.json",
                &json!({ "name": "p", "experimental": { "monitors": "./config/monitors.json" } })
                    .to_string(),
            )
            .file(
                "config/monitors.json",
                &json!([{ "name": "by-path", "command": "path.sh", "description": "d" }])
                    .to_string(),
            );
        assert_eq!(
            of(&running(&nested.root), RunKind::Monitor, "by-path")
                .text
                .as_deref(),
            Some("path.sh")
        );
    }

    #[test]
    fn a_plugin_with_only_hooks_keeps_the_hash_a_workspace_already_holds() {
        let dir = Folder::new("hash-hooks-only");
        dir.plugin(".", "p").file(
            "hooks/hooks.json",
            &json!({ "hooks": { "Stop": [{ "hooks": [{ "command": "x.sh" }] }] } }).to_string(),
        );
        // The very text the hash of 1.8.0 was taken from.
        let canonical = r#"[{"event":"Stop","matcher":"","command":"x.sh"}]"#;
        assert_eq!(running(&dir.root), []);
        assert_eq!(
            permission(&dir.root).unwrap(),
            format!("hooks:{}", &sha256_hex(canonical)[..16])
        );

        let quiet = Folder::new("hash-nothing");
        quiet.plugin(".", "p");
        assert_eq!(permission(&quiet.root), None);
    }

    #[test]
    fn the_hash_changes_with_each_kind_and_not_with_the_formatting() {
        let dir = every_kind("hash-kinds");
        let first = permission(&dir.root).unwrap();
        assert!(first.starts_with("hooks:"), "{first}");
        assert_eq!(first.len(), "hooks:".len() + 16, "{first}");
        assert_eq!(permission(&dir.root).unwrap(), first, "stable");

        // Reformatted, its keys in another order: the same.
        dir.file(
            ".lsp.json",
            &serde_json::to_string_pretty(&json!({ "gopls": {
                "env": { "GOFLAGS": "s3cret-lsp-value" },
                "extensionToLanguage": { ".mod": "go.mod", ".go": "go" },
                "args": ["serve"],
                "command": "gopls"
            } }))
            .unwrap(),
        );
        assert_eq!(permission(&dir.root).unwrap(), first, "reformatted");

        let change = |tag: &str, file: &str, body: String| {
            let changed = every_kind(tag);
            changed.file(file, &body);
            assert_ne!(permission(&changed.root).unwrap(), first, "{tag}: {file}");
        };
        change(
            "hash-mcp-args",
            ".mcp.json",
            json!({ "local": { "command": "node", "args": ["other.js"] } }).to_string(),
        );
        change(
            "hash-lsp",
            ".lsp.json",
            json!({ "gopls": { "command": "gopls", "args": ["other"], "extensionToLanguage": { ".go": "go" } } }).to_string(),
        );
        change(
            "hash-monitor",
            "monitors/monitors.json",
            json!([{ "name": "error-log", "command": "other.sh", "description": "d" }]).to_string(),
        );
        change("hash-bin", "bin/another", String::new());
        change(
            "hash-settings",
            "settings.json",
            json!({ "agent": "other" }).to_string(),
        );
        change(
            "hash-skill-hook",
            "skills/guarded/SKILL.md",
            "---\nhooks:\n  Stop: other\n---\n".to_string(),
        );
        change("hash-bundle", "packed.mcpb", String::new());
    }

    #[test]
    fn a_bundle_is_hashed_for_what_it_holds_when_it_is_the_plugins_own() {
        let dir = Folder::new("hash-bundle-bytes");
        dir.file(
            ".claude-plugin/plugin.json",
            &json!({ "name": "p", "mcpServers": "./packed.mcpb" }).to_string(),
        )
        .file("packed.mcpb", "one");
        let first = permission(&dir.root).unwrap();
        dir.file("packed.mcpb", "two");
        assert_ne!(permission(&dir.root).unwrap(), first);
    }

    #[test]
    fn the_hash_covers_env_values_and_headers_that_are_never_shown() {
        let hash = |env: &str, header: &str| {
            let dir = Folder::new("hash-sealed");
            dir.plugin(".", "p").file(
                ".mcp.json",
                &json!({
                    "local": { "command": "node", "env": { "PATH": env } },
                    "remote": { "type": "http", "url": "https://x", "headers": { "X": header } }
                })
                .to_string(),
            );
            permission(&dir.root).unwrap()
        };
        let first = hash("a", "b");
        assert_eq!(hash("a", "b"), first);
        assert_ne!(hash("other", "b"), first, "an env value");
        assert_ne!(hash("a", "other"), first, "a header");
    }

    #[test]
    fn the_inspection_sends_the_list_and_the_same_hash_as_the_kit() {
        let home = Folder::new("inspect-running");
        let store = Store::at(home.root.join("kits"));
        let kit = every_kind("inspect-running-src");
        kit.manifest(".", "everything", "1.0.0");
        let loaded = loaded(&kit, &store);
        let found = inspect(&loaded, "linux");
        assert_eq!(found.hooks_permission, loaded.hooks_permission());
        assert!(found.hooks_permission.is_some());

        let sent = serde_json::to_value(&found).unwrap();
        let items = sent["running"].as_array().unwrap();
        assert!(items
            .iter()
            .any(|item| item["kind"] == "skillHook" && item["name"] == "commands/old.md"));
        let first = items.first().unwrap();
        assert_eq!(first["kind"], "mcp", "in the view's order");
        let local = items.iter().find(|item| item["name"] == "local").unwrap();
        assert_eq!(local["argv"][0], "node");
        assert_eq!(local["env"][0], "API_TOKEN");
        assert!(local.get("text").is_none() && local.get("unreadable").is_none());
    }

    #[test]
    fn skills_too_deep_to_read_are_said_rather_than_passed_over() {
        let dir = Folder::new("skills-deep");
        dir.plugin(".", "p").file(
            "commands/a/b/c/d/e/f/deep.md",
            "---\nhooks:\n  Stop: x\n---\n",
        );
        let found = running(&dir.root);
        assert!(
            found.iter().any(|item| item.kind == RunKind::Unknown
                && item.unreadable
                && item.name.contains("too deep")),
            "{found:#?}"
        );
    }

    #[test]
    fn a_markdown_file_that_is_not_utf8_is_read_all_the_same() {
        let dir = Folder::new("skills-bytes");
        dir.plugin(".", "p");
        let skill = dir.root.join("skills/odd");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            b"---\ndescription: caf\xe9\nhooks:\n  Stop: x\n---\n".as_slice(),
        )
        .unwrap();
        assert_eq!(
            of(
                &running(&dir.root),
                RunKind::SkillHook,
                "skills/odd/SKILL.md"
            )
            .text
            .as_deref(),
            Some("hooks:\n  Stop: x")
        );
    }

    #[test]
    fn frontmatter_hooks_are_only_a_top_level_key_of_the_frontmatter() {
        assert_eq!(frontmatter_hooks("hooks: no frontmatter"), None);
        assert_eq!(frontmatter_hooks("---\nname: x\n---\nhooks: body"), None);
        assert_eq!(
            frontmatter_hooks("---\nmeta:\n  hooks: nested\n---\n"),
            None
        );
        assert_eq!(frontmatter_hooks("---\nhooks: {}\n---\n"), None);
        assert_eq!(frontmatter_hooks("---\nhooks:\n---\n"), None);
        assert_eq!(
            frontmatter_hooks("---\r\n\"hooks\": { Stop: [] }\r\nname: x\r\n---\r\n").as_deref(),
            Some("\"hooks\": { Stop: [] }")
        );
    }
}
