//! The MCP servers a pane's Claude Code starts, chosen per workspace.
//!
//! Every Claude Code session starts its own copy of every stdio MCP server it
//! is configured with. With a dozen panes open, servers only one project needs
//! — a game engine's bridge, a ticket tracker — run a dozen times over, in
//! every workspace. A workspace can instead name the servers its panes need:
//! they are copied out of Claude Code's own configuration into a file of the
//! app's, and the pane starts Claude Code with `--mcp-config <that file>
//! --strict-mcp-config`, which loads that file and nothing else.
//!
//! Hangar reads Claude Code's configuration and never writes it: the copy is
//! made again at every launch, so a server edited in `~/.claude.json` reaches
//! the next pane as it would have without any of this.
//!
//! What a session in a folder loads, as Claude Code 2.1 resolves it (checked
//! against `claude mcp list` with an isolated `CLAUDE_CONFIG_DIR`):
//!
//! - **user** scope: `mcpServers` at the top of `~/.claude.json`;
//! - **local** scope: `projects[<root>].mcpServers` in the same file, where
//!   `<root>` is the folder's git checkout — the main one, for a linked
//!   worktree — or the folder itself outside git;
//! - **project** scope: every `.mcp.json` from the folder up to the drive's
//!   root, the closest one winning a name. Such a server only loads once it is
//!   approved, and is skipped while it is not.
//!
//! A name defined more than once takes its whole entry from local, then an
//! approved project entry, then user. A name in the project's
//! `disabledMcpServers` (the `/mcp` toggle) is defined but not started.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use crate::install::{server_entry, LEGACY_SERVER_KEYS, SERVER_KEY};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    User,
    Local,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    Http,
    Sse,
    Ws,
}

/// Whether Claude Code would start the server on its own, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    /// A `.mcp.json` server nobody approved yet: Claude Code asks first.
    Unapproved,
    /// Named in a `disabledMcpjsonServers` list.
    Rejected,
    /// Turned off for this project from `/mcp`.
    Disabled,
}

/// One server as the panel lists it. Nothing in it is a secret: no
/// environment, no headers, and only as much of the command or URL as says
/// what the server is.
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub scope: Scope,
    pub transport: Transport,
    pub summary: String,
    pub status: Status,
    /// Hangar's own server, under its current name or a former one.
    pub hangar: bool,
    /// The file the definition comes from.
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Discovery {
    pub servers: Vec<ServerInfo>,
    /// Files that exist but could not be read as JSON. Their servers are
    /// missing from the list, which the panel has to say rather than show a
    /// shorter list as if it were the whole story.
    pub unreadable: Vec<String>,
}

/// A server and the full entry it was defined with, which only ever goes into
/// the generated file and never back to the window.
struct Resolved {
    info: ServerInfo,
    definition: Value,
}

/// Where Claude Code keeps its configuration on this machine.
struct ClaudeFiles {
    /// `~/.claude.json`: user and local servers, per-project state.
    config: PathBuf,
    /// `~/.claude/settings.json`: the user's approvals of project servers.
    settings: PathBuf,
}

impl ClaudeFiles {
    /// `CLAUDE_CONFIG_DIR` moves both, as it does for Claude Code — and a pane
    /// inherits the app's environment, so the variable the app sees is the
    /// one its Claude Code sees.
    fn locate() -> Option<Self> {
        let home = dirs::home_dir()?;
        let custom = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
        let config_home = custom.clone().unwrap_or_else(|| home.join(".claude"));
        // A configuration written by old versions, which Claude Code still
        // prefers when it is there.
        let legacy = config_home.join(".config.json");
        let config = if legacy.is_file() {
            legacy
        } else {
            custom.unwrap_or(home).join(".claude.json")
        };
        Some(ClaudeFiles {
            config,
            settings: config_home.join("settings.json"),
        })
    }
}

/// `Ok(None)` for a file that is not there, which is ordinary; `Err` for one
/// that is there and cannot be read as a JSON object.
fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, ()> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    // Claude Code writes its files with a byte order mark on some setups.
    match serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        _ => Err(()),
    }
}

/// Reads a file, noting it as unreadable when it is broken.
fn load(path: &Path, unreadable: &mut Vec<String>) -> Map<String, Value> {
    match read_object(path) {
        Ok(found) => found.unwrap_or_default(),
        Err(()) => {
            unreadable.push(path.to_string_lossy().into_owned());
            Map::new()
        }
    }
}

/// `..` and `.` taken out without touching the disk, so a path compares equal
/// to the way Claude Code spells the same folder.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A path as a key of `projects`: forward slashes, no trailing one, and — on
/// Windows, where `C:\Dev` and `c:/dev` are the same folder — one case.
fn path_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') && !key.ends_with(":/") {
        key.pop();
    }
    if cfg!(windows) {
        key = key.to_lowercase();
    }
    key
}

/// The folder Claude Code files a session's local servers under: the git
/// checkout the folder belongs to — the main checkout, for a linked worktree,
/// since Claude Code keeps one entry per repository — or the folder itself
/// outside git.
fn project_root(cwd: &Path) -> PathBuf {
    for dir in cwd.ancestors() {
        let dot = dir.join(".git");
        if dot.is_dir() {
            return dir.to_path_buf();
        }
        if dot.is_file() {
            return main_checkout(dir, &dot).unwrap_or_else(|| dir.to_path_buf());
        }
    }
    cwd.to_path_buf()
}

/// A linked worktree's `.git` is a file pointing into the main repository's
/// `.git/worktrees/<name>`, whose `commondir` leads back to that `.git`.
fn main_checkout(dir: &Path, dot: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(dot).ok()?;
    let gitdir = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    let gitdir = dir.join(gitdir);
    let common = fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = lexical(&gitdir.join(common.trim()));
    if common.file_name()? == ".git" {
        common.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

fn names(map: &Map<String, Value>, key: &str) -> BTreeSet<String> {
    map.get(key)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn flag(map: &Map<String, Value>, key: &str) -> bool {
    map.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn servers_of(map: &Map<String, Value>) -> Map<String, Value> {
    map.get("mcpServers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// Who may approve a `.mcp.json` server, and who turned which off.
struct Approvals {
    /// Approves every project server.
    all: bool,
    approved: BTreeSet<String>,
    rejected: BTreeSet<String>,
}

impl Approvals {
    fn status(&self, name: &str) -> Status {
        if self.rejected.contains(name) {
            Status::Rejected
        } else if self.all || self.approved.contains(name) {
            Status::Active
        } else {
            Status::Unapproved
        }
    }
}

/// The user's own settings always count. The folder's — its
/// `.claude/settings.json` and `settings.local.json`, and the approvals
/// Claude Code saved for the project — only once the folder is trusted: a
/// repository cannot approve the servers it ships by itself. A rejection
/// counts wherever it is written.
fn approvals(user: &Map<String, Value>, folder: &[Map<String, Value>], trusted: bool) -> Approvals {
    let mut out = Approvals {
        all: flag(user, "enableAllProjectMcpServers"),
        approved: names(user, "enabledMcpjsonServers"),
        rejected: names(user, "disabledMcpjsonServers"),
    };
    for settings in folder {
        out.rejected
            .extend(names(settings, "disabledMcpjsonServers"));
        if trusted {
            out.all |= flag(settings, "enableAllProjectMcpServers");
            out.approved
                .extend(names(settings, "enabledMcpjsonServers"));
        }
    }
    out
}

fn transport(definition: &Value) -> Transport {
    match definition.get("type").and_then(Value::as_str) {
        Some("http" | "streamable-http") => Transport::Http,
        Some("sse") => Transport::Sse,
        Some("ws") => Transport::Ws,
        Some(_) => Transport::Stdio,
        // Claude Code reads an entry without a type as stdio. One with a URL
        // and no command can only have meant HTTP, so it is shown as that.
        None if definition.get("command").is_none() && definition.get("url").is_some() => {
            Transport::Http
        }
        None => Transport::Stdio,
    }
}

/// The program a command names: its last path component, without the
/// extension Windows launchers carry — `C:\Program Files\nodejs\npx.cmd` is
/// `npx`.
///
/// A path can hold spaces, so the command is cut after its extension when it
/// has one, and only otherwise at its first space: some entries put a whole
/// command line in the one field.
fn program_name(command: &str) -> String {
    const EXTENSIONS: [&str; 4] = [".exe", ".cmd", ".bat", ".ps1"];
    let lower = command.to_lowercase();
    let end = EXTENSIONS
        .iter()
        .filter_map(|ext| {
            lower
                .match_indices(ext)
                .map(|(at, _)| at + ext.len())
                .find(|&end| lower[end..].chars().next().is_none_or(char::is_whitespace))
        })
        .min();
    let program = match end {
        Some(end) => &command[..end],
        None => command.split_whitespace().next().unwrap_or(command),
    };
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let trimmed = EXTENSIONS
        .iter()
        .find(|ext| base.to_lowercase().ends_with(*ext))
        .map_or(base, |ext| &base[..base.len() - ext.len()]);
    safe_name(trimmed).unwrap_or_else(|| "stdio".to_string())
}

/// A package or script name that is safe to show, or nothing.
///
/// What comes after a launcher is meant to be a package, but it is free text
/// in a file the user wrote, and a token pasted as an argument would land on
/// screen — and in every screenshot of it. Anything shaped like a value
/// rather than a name is dropped: URLs, `key=value`, and long unbroken runs
/// of letters and digits.
fn safe_name(arg: &str) -> Option<String> {
    if arg.contains("://") || arg.contains('=') || arg.chars().any(char::is_whitespace) {
        return None;
    }
    // `@scope/pkg` is a name; any other path is shown by its last part.
    let name = if arg.starts_with('@') && arg.matches('/').count() == 1 {
        arg.to_string()
    } else {
        arg.rsplit(['/', '\\']).next().unwrap_or(arg).to_string()
    };
    if name.is_empty() || name.len() > 60 {
        return None;
    }
    let tokenish = name.len() >= 20
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && name.chars().any(|c| c.is_ascii_digit())
        && name.chars().any(|c| c.is_ascii_alphabetic());
    (!tokenish).then_some(name)
}

/// Flags of the launchers that take the next argument as their value.
const VALUE_FLAGS: &[&str] = &[
    "--python",
    "--with",
    "--index-url",
    "--extra-index-url",
    "--registry",
    "--cache",
    "--prefix",
    "--node-options",
    "-c",
    "--call",
];

/// Flags whose value is the package itself.
const PACKAGE_FLAGS: &[&str] = &["--from", "--package", "-p"];

/// The first argument that is not a flag, or the value of a flag that names
/// the package outright.
fn package_arg(args: &[String]) -> Option<&str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if PACKAGE_FLAGS.contains(&arg.as_str()) {
            return iter.next().map(String::as_str);
        }
        if VALUE_FLAGS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

fn stdio_summary(command: &str, args: &[String]) -> String {
    let program = program_name(command);
    let lower = program.to_lowercase();
    // A wrapper shell says nothing; what it runs does.
    if lower == "cmd" {
        if let Some(at) = args
            .iter()
            .position(|arg| arg.eq_ignore_ascii_case("/c") || arg.eq_ignore_ascii_case("/k"))
        {
            if let Some((inner, rest)) = args[at + 1..].split_first() {
                return stdio_summary(inner, rest);
            }
        }
        return program;
    }
    // Launchers whose first word is a subcommand: `npm exec pkg`, `uv run x`.
    let subcommand = match lower.as_str() {
        "npm" | "pnpm" | "yarn" | "bun" | "uv" | "pipx" | "deno" => args
            .first()
            .filter(|first| matches!(first.as_str(), "exec" | "dlx" | "x" | "run" | "tool"))
            .map(|first| first.as_str()),
        _ => None,
    };
    let launcher = matches!(
        lower.as_str(),
        "npx"
            | "pnpx"
            | "bunx"
            | "uvx"
            | "node"
            | "nodejs"
            | "python"
            | "python3"
            | "py"
            | "pythonw"
            | "ruby"
            | "php"
            | "java"
            | "dotnet"
    ) || subcommand.is_some();
    if !launcher {
        return program;
    }
    let rest: Vec<String> = match subcommand {
        // `uv tool run pkg`: both words are the subcommand.
        Some("tool") => args.iter().skip(2).cloned().collect(),
        Some(_) => args.iter().skip(1).cloned().collect(),
        None => args.to_vec(),
    };
    match package_arg(&rest).and_then(safe_name) {
        Some(name) => format!("{program} {name}"),
        None => program,
    }
}

/// Where a remote server lives, and nothing else: no credentials before the
/// host, and no path or query after it, both of which can hold a token.
fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    (!host.is_empty()).then(|| host.to_string())
}

/// A line that says what a server is — `npx @scope/pkg`, `mcp.example.com` —
/// built only from the parts of its entry that cannot hold a secret.
fn summary(definition: &Value, transport: Transport) -> String {
    match transport {
        Transport::Stdio => {
            let command = definition
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args: Vec<String> = definition
                .get("args")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if command.is_empty() {
                "stdio".to_string()
            } else {
                stdio_summary(command, &args)
            }
        }
        remote => definition
            .get("url")
            .and_then(Value::as_str)
            .and_then(host_of)
            .unwrap_or_else(|| {
                match remote {
                    Transport::Sse => "sse",
                    Transport::Ws => "ws",
                    _ => "http",
                }
                .to_string()
            }),
    }
}

fn is_hangar(name: &str) -> bool {
    name == SERVER_KEY || LEGACY_SERVER_KEYS.contains(&name)
}

/// Everything a Claude Code session started in `cwd` would be configured
/// with, one entry per name, in name order.
fn resolve(files: &ClaudeFiles, cwd: &Path) -> (Vec<Resolved>, Vec<String>) {
    let mut unreadable = Vec::new();
    let config = load(&files.config, &mut unreadable);
    let user_settings = load(&files.settings, &mut unreadable);

    let root = project_root(cwd);
    let root_key = path_key(&root);
    let cwd_key = path_key(cwd);
    let projects = config
        .get("projects")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let project = projects
        .iter()
        .find(|(key, _)| path_key(Path::new(key)) == root_key)
        .and_then(|(_, entry)| entry.as_object().cloned())
        .unwrap_or_default();
    // Trust given to the folder or to any folder above it.
    let trusted = projects.iter().any(|(key, entry)| {
        let key = path_key(Path::new(key));
        (cwd_key == key || cwd_key.starts_with(&format!("{}/", key.trim_end_matches('/'))))
            && entry
                .get("hasTrustDialogAccepted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    });

    let folder_settings: Vec<Map<String, Value>> = [
        cwd.join(".claude").join("settings.json"),
        cwd.join(".claude").join("settings.local.json"),
    ]
    .iter()
    .map(|path| load(path, &mut unreadable))
    .chain(std::iter::once(project.clone()))
    .collect();
    let approvals = approvals(&user_settings, &folder_settings, trusted);
    let disabled = names(&project, "disabledMcpServers");

    let config_source = files.config.to_string_lossy().into_owned();
    let user = servers_of(&config);
    let local = servers_of(&project);

    // Every `.mcp.json` from the drive's root down to the folder, so a closer
    // file overwrites a farther one's entry of the same name.
    let mut shared: BTreeMap<String, (Value, String)> = BTreeMap::new();
    let chain: Vec<&Path> = cwd.ancestors().collect();
    for dir in chain.into_iter().rev() {
        let path = dir.join(".mcp.json");
        if !path.is_file() {
            continue;
        }
        let source = path.to_string_lossy().into_owned();
        for (name, definition) in servers_of(&load(&path, &mut unreadable)) {
            shared.insert(name, (definition, source.clone()));
        }
    }

    let all: BTreeSet<&String> = user
        .keys()
        .chain(local.keys())
        .chain(shared.keys())
        .collect();
    let mut resolved = Vec::new();
    for name in all {
        let project_entry = shared.get(name);
        let project_status = approvals.status(name);
        let (scope, definition, source, status) = if let Some(definition) = local.get(name) {
            (
                Scope::Local,
                definition,
                config_source.clone(),
                Status::Active,
            )
        } else if let Some((definition, source)) =
            project_entry.filter(|_| project_status == Status::Active)
        {
            (Scope::Project, definition, source.clone(), Status::Active)
        } else if let Some(definition) = user.get(name) {
            (
                Scope::User,
                definition,
                config_source.clone(),
                Status::Active,
            )
        } else if let Some((definition, source)) = project_entry {
            (Scope::Project, definition, source.clone(), project_status)
        } else {
            continue;
        };
        // An SDK server only exists inside the program that registered it.
        if definition.get("type").and_then(Value::as_str) == Some("sdk") {
            continue;
        }
        let status = if status == Status::Active && disabled.contains(name) {
            Status::Disabled
        } else {
            status
        };
        let transport = transport(definition);
        resolved.push(Resolved {
            info: ServerInfo {
                name: name.clone(),
                scope,
                transport,
                summary: summary(definition, transport),
                status,
                hangar: is_hangar(name),
                source,
            },
            definition: definition.clone(),
        });
    }
    (resolved, unreadable)
}

fn discover(files: &ClaudeFiles, cwd: &Path) -> Discovery {
    let (resolved, unreadable) = resolve(files, cwd);
    Discovery {
        servers: resolved.into_iter().map(|entry| entry.info).collect(),
        unreadable,
    }
}

/// The servers a pane gets: Hangar's own, then each enabled name as it is
/// defined right now. Names that are no longer defined anywhere are reported
/// rather than invented.
fn scoped_config(resolved: &[Resolved], enabled: &[String], hangar: Value) -> ScopedConfig {
    let mut servers = Map::new();
    servers.insert(SERVER_KEY.to_string(), hangar);
    let mut missing = Vec::new();
    for name in enabled {
        // Hangar's entry is the running app's, whatever the files say: an
        // old one points at a binary from before an update or a rename.
        if is_hangar(name) || servers.contains_key(name) {
            continue;
        }
        match resolved.iter().find(|entry| &entry.info.name == name) {
            Some(entry) => {
                servers.insert(name.clone(), entry.definition.clone());
            }
            None => missing.push(name.clone()),
        }
    }
    ScopedConfig {
        names: servers.keys().cloned().collect(),
        body: json!({ "mcpServers": servers }),
        missing,
    }
}

struct ScopedConfig {
    names: Vec<String>,
    body: Value,
    missing: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ScopeFile {
    /// What `--mcp-config` is given.
    pub path: String,
    /// The servers in it, Hangar's included.
    pub servers: Vec<String>,
    /// Enabled names nothing defines any more.
    pub missing: Vec<String>,
}

/// A workspace id as a file name. Ids are UUIDs; anything else is kept to
/// characters every file system takes.
fn file_stem(workspace_id: &str) -> String {
    let stem: String = workspace_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if stem.is_empty() {
        "workspace".to_string()
    } else {
        stem
    }
}

/// FNV-1a: stable across runs and platforms, unlike the std hasher, which is
/// all a file name needs.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// One file per workspace, and one more per folder a pane of it runs in
/// apart from the workspace's own — a worktree has servers of its own.
fn file_name(workspace_id: &str, workspace_cwd: &str, cwd: &str) -> String {
    let stem = file_stem(workspace_id);
    if path_key(Path::new(cwd)) == path_key(Path::new(workspace_cwd)) {
        format!("{stem}.json")
    } else {
        format!("{stem}-{:016x}.json", fnv(&path_key(Path::new(cwd))))
    }
}

fn belongs_to(file: &str, stem: &str) -> bool {
    if file == format!("{stem}.json") {
        return true;
    }
    file.strip_prefix(stem)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|hash| hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Several panes of a workspace start together and write the same file. The
/// content is compared first, so they mostly find it already written; when
/// it did change, a uniquely named temporary file is renamed over it, so a
/// Claude Code reading it at that moment gets either version whole.
fn write_if_changed(path: &Path, body: &str) -> Result<(), String> {
    if fs::read_to_string(path).ok().as_deref() == Some(body) {
        return Ok(());
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let tmp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    // The copies carry whatever the servers' entries carry, tokens included,
    // so they are kept as private as ~/.claude.json itself.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn scope_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {e}"))?
        .join("mcp");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// The servers a Claude Code session started in `cwd` would load by itself.
#[tauri::command]
pub fn mcp_servers(cwd: String) -> Discovery {
    match ClaudeFiles::locate() {
        Some(files) => discover(&files, Path::new(&cwd)),
        None => Discovery::default(),
    }
}

/// Writes the file a pane's Claude Code is pointed at with `--mcp-config`,
/// from the definitions as they stand right now.
///
/// `workspace_cwd` is what Hangar's own server is told its workspace is — the
/// board it reads — and `cwd` is where the pane runs, which decides the
/// servers it would otherwise have.
#[tauri::command]
pub fn mcp_scope_write(
    app: AppHandle,
    workspace_id: String,
    workspace_cwd: String,
    cwd: String,
    enabled: Vec<String>,
) -> Result<ScopeFile, String> {
    let files = ClaudeFiles::locate().ok_or("no home directory")?;
    let (resolved, _) = resolve(&files, Path::new(&cwd));
    let config = scoped_config(&resolved, &enabled, server_entry(&workspace_cwd)?);
    let path = scope_dir(&app)?.join(file_name(&workspace_id, &workspace_cwd, &cwd));
    let body = serde_json::to_string_pretty(&config.body).map_err(|e| e.to_string())?;
    write_if_changed(&path, &body)?;
    Ok(ScopeFile {
        path: path.to_string_lossy().into_owned(),
        servers: config.names,
        missing: config.missing,
    })
}

/// Deletes a workspace's files once it goes back to Claude Code's own
/// configuration: they hold copies of the servers' entries, and nothing reads
/// them any more.
#[tauri::command]
pub fn mcp_scope_clear(app: AppHandle, workspace_id: String) -> Result<(), String> {
    let dir = scope_dir(&app)?;
    let stem = file_stem(&workspace_id);
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        if belongs_to(&name.to_string_lossy(), &stem) {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch tree standing in for a machine: a home with Claude Code's two
    /// files, and folders to start sessions in.
    struct Machine {
        root: PathBuf,
        files: ClaudeFiles,
    }

    impl Machine {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "hangar-mcp-scope-{tag}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir_all(root.join("home/.claude")).unwrap();
            let files = ClaudeFiles {
                config: root.join("home/.claude.json"),
                settings: root.join("home/.claude/settings.json"),
            };
            Machine { root, files }
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let dir = self.root.join(relative);
            fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn write(&self, relative: &str, body: &Value) {
            let path = self.root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body.to_string()).unwrap();
        }

        /// `~/.claude.json`, with `projects` keyed the way Claude Code keys
        /// them: forward slashes.
        fn config(&self, user: Value, projects: &[(&Path, Value)]) {
            let projects: Map<String, Value> = projects
                .iter()
                .map(|(dir, entry)| (dir.to_string_lossy().replace('\\', "/"), entry.clone()))
                .collect();
            self.write(
                "home/.claude.json",
                &json!({ "mcpServers": user, "projects": projects }),
            );
        }

        fn discover(&self, cwd: &Path) -> Discovery {
            discover(&self.files, cwd)
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn find<'a>(found: &'a Discovery, name: &str) -> &'a ServerInfo {
        found
            .servers
            .iter()
            .find(|server| server.name == name)
            .unwrap_or_else(|| panic!("{name} not listed"))
    }

    fn stdio(command: &str) -> Value {
        json!({ "command": command })
    }

    #[test]
    fn local_beats_project_beats_user() {
        let m = Machine::new("precedence");
        let repo = m.dir("repo");
        m.config(
            json!({ "both": stdio("from-user"), "all": stdio("from-user"), "solo": stdio("u") }),
            &[(
                repo.as_path(),
                json!({ "mcpServers": { "all": stdio("from-local") }, "hasTrustDialogAccepted": true }),
            )],
        );
        m.write(
            "repo/.mcp.json",
            &json!({ "mcpServers": { "both": stdio("from-project"), "all": stdio("from-project") } }),
        );
        m.write(
            "home/.claude/settings.json",
            &json!({ "enableAllProjectMcpServers": true }),
        );

        let found = m.discover(&repo);

        assert_eq!(find(&found, "all").scope, Scope::Local);
        assert_eq!(find(&found, "all").summary, "from-local");
        assert_eq!(find(&found, "both").scope, Scope::Project);
        assert_eq!(find(&found, "both").summary, "from-project");
        assert_eq!(find(&found, "solo").scope, Scope::User);
        assert!(found.servers.iter().all(|s| s.status == Status::Active));
    }

    /// An unapproved `.mcp.json` server is not loaded, so it does not hide
    /// the user's server of the same name either.
    #[test]
    fn an_unapproved_project_server_leaves_the_user_one_in_place() {
        let m = Machine::new("unapproved");
        let repo = m.dir("repo");
        m.config(json!({ "both": stdio("from-user") }), &[]);
        m.write(
            "repo/.mcp.json",
            &json!({ "mcpServers": { "both": stdio("from-project"), "only": stdio("p") } }),
        );

        let found = m.discover(&repo);

        assert_eq!(find(&found, "both").scope, Scope::User);
        assert_eq!(find(&found, "both").status, Status::Active);
        assert_eq!(find(&found, "only").status, Status::Unapproved);
    }

    #[test]
    fn the_closest_mcp_json_wins_and_parents_count() {
        let m = Machine::new("walk");
        let deep = m.dir("repo/sub/deeper");
        m.config(json!({}), &[]);
        m.write(
            "repo/.mcp.json",
            &json!({ "mcpServers": { "shared": stdio("far"), "above": stdio("a") } }),
        );
        m.write(
            "repo/sub/.mcp.json",
            &json!({ "mcpServers": { "shared": stdio("near") } }),
        );
        m.write(
            "home/.claude/settings.json",
            &json!({ "enabledMcpjsonServers": ["shared", "above"] }),
        );

        let found = m.discover(&deep);

        assert_eq!(find(&found, "shared").summary, "near");
        assert!(find(&found, "shared").source.ends_with(
            &Path::new("sub")
                .join(".mcp.json")
                .to_string_lossy()
                .into_owned()
        ));
        assert_eq!(find(&found, "above").status, Status::Active);
    }

    /// The folder's own settings cannot approve its servers until the user
    /// trusted it; a rejection counts either way.
    #[test]
    fn folder_approvals_need_trust() {
        let m = Machine::new("trust");
        let repo = m.dir("repo");
        m.write(
            "repo/.mcp.json",
            &json!({ "mcpServers": { "ok": stdio("a"), "no": stdio("b") } }),
        );
        m.write(
            "repo/.claude/settings.local.json",
            &json!({ "enabledMcpjsonServers": ["ok"], "disabledMcpjsonServers": ["no"] }),
        );

        m.config(json!({}), &[]);
        let untrusted = m.discover(&repo);
        assert_eq!(find(&untrusted, "ok").status, Status::Unapproved);
        assert_eq!(find(&untrusted, "no").status, Status::Rejected);

        m.config(
            json!({}),
            &[(repo.as_path(), json!({ "hasTrustDialogAccepted": true }))],
        );
        let trusted = m.discover(&repo);
        assert_eq!(find(&trusted, "ok").status, Status::Active);
        assert_eq!(find(&trusted, "no").status, Status::Rejected);
    }

    #[test]
    fn a_server_turned_off_in_mcp_reads_as_disabled() {
        let m = Machine::new("disabled");
        let repo = m.dir("repo");
        m.config(
            json!({ "roblox": stdio("npx") }),
            &[(repo.as_path(), json!({ "disabledMcpServers": ["roblox"] }))],
        );

        assert_eq!(find(&m.discover(&repo), "roblox").status, Status::Disabled);
    }

    /// Claude Code files local servers under the repository, and under the
    /// main checkout for a linked worktree.
    #[test]
    fn local_servers_are_keyed_by_the_main_checkout() {
        let m = Machine::new("worktree");
        let repo = m.dir("repo");
        fs::create_dir_all(repo.join(".git/worktrees/wt")).unwrap();
        fs::write(repo.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        let wt = m.dir("wt");
        fs::write(
            wt.join(".git"),
            format!("gitdir: {}\n", repo.join(".git/worktrees/wt").display()),
        )
        .unwrap();
        let sub = m.dir("repo/src/deep");
        m.config(
            json!({}),
            &[(
                repo.as_path(),
                json!({ "mcpServers": { "loc": stdio("x") } }),
            )],
        );

        assert_eq!(find(&m.discover(&sub), "loc").scope, Scope::Local);
        assert_eq!(find(&m.discover(&wt), "loc").scope, Scope::Local);
        assert_eq!(project_root(&wt), repo);
    }

    #[test]
    fn missing_files_list_nothing_and_complain_about_nothing() {
        let m = Machine::new("missing");
        let found = m.discover(&m.dir("anywhere"));
        assert!(found.servers.is_empty());
        assert!(found.unreadable.is_empty());
    }

    #[test]
    fn a_malformed_file_is_reported_and_the_others_still_count() {
        let m = Machine::new("malformed");
        let repo = m.dir("repo");
        fs::write(&m.files.config, "{ \"mcpServers\": { oops").unwrap();
        m.write(
            "home/.claude/settings.json",
            &json!({ "enableAllProjectMcpServers": true }),
        );
        m.write(
            "repo/.mcp.json",
            &json!({ "mcpServers": { "p": stdio("p") } }),
        );
        fs::write(m.root.join(".mcp.json"), "[1, 2").unwrap();

        let found = m.discover(&repo);

        assert_eq!(found.servers.len(), 1);
        assert_eq!(find(&found, "p").status, Status::Active);
        assert_eq!(found.unreadable.len(), 2);
        assert!(found
            .unreadable
            .iter()
            .any(|path| path.ends_with(".claude.json")));
    }

    /// The panel is on screen and in screenshots: a summary is built from the
    /// command and at most one name, never from env, headers, a URL's path or
    /// query, or anything shaped like a token.
    #[test]
    fn summaries_never_carry_secrets() {
        let m = Machine::new("secrets");
        let repo = m.dir("repo");
        let secret = "sk-live-9f8e7d6c5b4a39281706";
        m.config(
            json!({
                "roblox": {
                    "type": "stdio",
                    "command": "C:\\Program Files\\nodejs\\npx.cmd",
                    "args": ["-y", "@scope/roblox-mcp@latest"],
                    "env": { "API_KEY": secret }
                },
                "atlassian": {
                    "command": "uvx",
                    "args": ["--from", "mcp-atlassian", "mcp-atlassian", "--token", secret],
                    "env": { "JIRA_TOKEN": secret }
                },
                "wrapped": { "command": "cmd", "args": ["/c", "npx", "-y", "pkg-wrapped"] },
                "keyarg": { "command": "npx", "args": [format!("--api-key={secret}")] },
                "tokenarg": { "command": "node", "args": ["9f8e7d6c5b4a3928170655aa"] },
                "script": { "command": "node", "args": ["C:/tools/my-server/index.js", secret] },
                "remote": {
                    "type": "http",
                    "url": format!("https://user:{secret}@mcp.example.com/v1/{secret}?key={secret}"),
                    "headers": { "Authorization": format!("Bearer {secret}") }
                },
                "events": { "type": "sse", "url": "http://127.0.0.1:13337/sse" },
                "bare": { "command": "C:/bin/rea.exe", "args": [secret] }
            }),
            &[],
        );

        let found = m.discover(&repo);
        let summary = |name: &str| find(&found, name).summary.clone();

        assert_eq!(summary("roblox"), "npx @scope/roblox-mcp@latest");
        assert_eq!(summary("atlassian"), "uvx mcp-atlassian");
        assert_eq!(summary("wrapped"), "npx pkg-wrapped");
        assert_eq!(summary("keyarg"), "npx");
        assert_eq!(summary("tokenarg"), "node");
        assert_eq!(summary("script"), "node index.js");
        assert_eq!(summary("remote"), "mcp.example.com");
        assert_eq!(find(&found, "remote").transport, Transport::Http);
        assert_eq!(summary("events"), "127.0.0.1:13337");
        assert_eq!(find(&found, "events").transport, Transport::Sse);
        assert_eq!(summary("bare"), "rea");

        let shown = serde_json::to_string(&found).unwrap();
        assert!(
            !shown.contains("9f8e7d6c5b4a"),
            "a secret reached the panel: {shown}"
        );
    }

    #[test]
    fn hangar_is_recognised_under_every_name() {
        let m = Machine::new("hangar");
        let repo = m.dir("repo");
        m.config(json!({ "iabench": stdio("old"), "other": stdio("o") }), &[]);
        let found = m.discover(&repo);
        assert!(find(&found, "iabench").hangar);
        assert!(!find(&found, "other").hangar);
    }

    /// Only what was chosen goes in, with its whole entry — env included, or
    /// the server could not start — and Hangar's own entry always.
    #[test]
    fn the_generated_config_holds_the_choice_and_hangar() {
        let m = Machine::new("generate");
        let repo = m.dir("repo");
        let roblox = json!({ "command": "npx", "args": ["-y", "roblox"], "env": { "K": "v" } });
        m.config(
            json!({
                "roblox": roblox,
                "atlassian": stdio("uvx"),
                "hangar": stdio("C:/old/hangar.exe")
            }),
            &[],
        );
        let (resolved, _) = resolve(&m.files, &repo);
        let ours = json!({ "command": "C:/new/hangar-ai.exe", "args": ["--mcp"] });

        let config = scoped_config(
            &resolved,
            &["roblox".into(), "hangar".into(), "gone".into()],
            ours.clone(),
        );

        let servers = config.body["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers["roblox"], roblox);
        assert_eq!(
            servers["hangar"], ours,
            "Hangar's entry is the running app's"
        );
        assert!(servers.get("atlassian").is_none());
        assert_eq!(config.missing, vec!["gone".to_string()]);
        assert_eq!(
            config.names,
            vec!["hangar".to_string(), "roblox".to_string()]
        );
    }

    #[test]
    fn nothing_chosen_still_keeps_hangar() {
        let config = scoped_config(&[], &[], json!({ "command": "h" }));
        assert_eq!(config.names, vec!["hangar".to_string()]);
    }

    #[test]
    fn file_names_are_per_workspace_and_per_folder() {
        assert_eq!(file_name("abc-1", "C:\\ws", "C:/ws/"), "abc-1.json");
        let other = file_name("abc-1", "C:\\ws", "C:\\ws-wt");
        assert!(belongs_to(&other, "abc-1"), "{other}");
        assert!(!belongs_to(&other, "abc"));
        assert!(!belongs_to("abc-1.json", "abc"));
        assert_eq!(file_stem("../../x"), "______x");
    }

    #[test]
    fn rewriting_the_same_content_leaves_the_file_alone() {
        let m = Machine::new("write");
        let path = m.dir("out").join("w.json");
        write_if_changed(&path, "{}").unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_if_changed(&path, "{}").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        write_if_changed(&path, "{\"a\":1}").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"a\":1}");
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file left behind");
    }
}
