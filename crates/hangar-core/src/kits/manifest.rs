//! `hangar-kit.json`, schema 1: what a kit says it is and brings.
//!
//! Read field by field rather than through a derive, so a manifest that is
//! wrong in three places is told about all three at once, each by the path of
//! the field (`tabs[1].permissions`), and so a field Hangar does not know —
//! from a newer schema, or a kit's own notes — is simply left alone.
//!
//! Every path a manifest names is relative to the kit's root, kept normalized
//! (forward slashes, no `.` parts), and refused when it is absolute, climbs out
//! with `..`, or resolves outside the kit folder through a symlink.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

pub const FILE: &str = "hangar-kit.json";

/// The only schema this Hangar reads.
pub const SCHEMA: u64 = 1;

/// What a kit's tab may ask Hangar for, accepted by the person when the kit
/// is enabled on a workspace. Closed: beside these, only `loopback:<port>`
/// (`loopback_port`) and `command:<name>` for a command the kit declares, and
/// any other name is a manifest error.
pub const PERMISSIONS: &[&str] = &[
    "workspace.read",
    "panes.read",
    "panes.send",
    "files.read",
    "files.write",
    "data.read",
    "data.write",
    "preview.show",
    "notify",
];

/// The port of a `loopback:<port>` permission: written plainly, without a sign
/// or a leading zero, from 1024 to 65535. `kitLoopbackPort` in src/lib/kits.ts
/// holds the same rule.
pub fn loopback_port(permission: &str) -> Option<u16> {
    let digits = permission.strip_prefix("loopback:")?;
    if digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let port: u16 = digits.parse().ok()?;
    (port >= super::loopback::MIN_PORT).then_some(port)
}

/// How long a declared command may run, in seconds, unless it says otherwise,
/// and at most.
pub const COMMAND_TIMEOUT_SECONDS: u64 = 30;
pub const COMMAND_MAX_TIMEOUT_SECONDS: u64 = 120;

/// `[a-z][a-z0-9-]*`: how a declared command is named. `kitCommandName` in
/// src/lib/kits.ts holds the same rule.
pub fn valid_command_name(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The placeholder Hangar fills in itself, with the kit's root folder: it has no
/// `args` entry and no rule, and a kit cannot declare an arg by this name. It lets
/// a kit run a script of its own: `["node", "{kitDir}/tools/audit.js"]`.
pub const KIT_DIR: &str = "kitDir";

/// One piece of an argv item: text as written, or the place of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgPart<'a> {
    Text(&'a str),
    Value(&'a str),
}

/// The placeholders of an argv item. A placeholder is `{name}`, a name being
/// letters, digits, `_` and `-`, starting with a letter or `_`: other braces
/// are text, which is what `docker ps --format {{.Names}}` needs.
pub fn arg_parts(item: &str) -> Vec<ArgPart<'_>> {
    let mut parts = Vec::new();
    let mut text_from = 0;
    let mut at = 0;
    while let Some(open) = item[at..].find('{').map(|found| at + found) {
        let name = item[open + 1..]
            .find('}')
            .map(|close| &item[open + 1..open + 1 + close])
            .filter(|name| valid_arg_name(name));
        match name {
            Some(name) => {
                if text_from < open {
                    parts.push(ArgPart::Text(&item[text_from..open]));
                }
                parts.push(ArgPart::Value(name));
                at = open + name.len() + 2;
                text_from = at;
            }
            None => at = open + 1,
        }
    }
    if text_from < item.len() {
        parts.push(ArgPart::Text(&item[text_from..]));
    }
    parts
}

/// What a `{name}` may be called.
fn valid_arg_name(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A rule, compiled to match a whole value whatever its author wrote. Compiled
/// alone first: `a)|(?:b` only parses once wrapped, and would then match past
/// the anchors. `\A` and `\z` are what `(?m)` cannot move.
pub fn whole_value_rule(pattern: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pattern).map_err(|_| "is not a valid regular expression".to_string())?;
    regex::Regex::new(&format!(r"\A(?:{pattern})\z"))
        .map_err(|_| "is not a valid regular expression".to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u64,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// The Hangar versions the kit works with, as a semver requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hangar: Option<String>,
    /// The folder holding the kit's Claude Code plugin, `.` for the root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub tabs: Vec<Tab>,
    pub setup: Vec<SetupStep>,
    /// What a tab may run, by name: nothing else, and never through a shell.
    pub commands: BTreeMap<String, DeclaredCommand>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub permissions: Vec<String>,
    /// What a caller may pass it, by name: listed for agents, and held to at
    /// `kit_tab_open`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, TabParam>,
    /// One tab whatever its params, which new ones replace.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub singleton: bool,
}

/// What a caller may pass a tab under one name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TabParam {
    pub description: String,
    /// As written; matched against whole values (`whole_value_rule`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl Tab {
    /// The first value of `params` its rule refuses, said for the caller.
    /// Asked before the window hears of a `kit_tab_open`: this is where a rule
    /// runs in linear time, whatever the kit wrote, and the window must not
    /// hang on one that backtracks for ever. Everything else about the call is
    /// the window's to refuse (src/lib/kitTabs.ts), which says it in the same
    /// words.
    pub fn param_refusal(&self, params: &Value) -> Option<String> {
        params.as_object()?.iter().find_map(|(name, value)| {
            let pattern = self.params.get(name)?.pattern.as_deref()?;
            let value = value.as_str()?;
            let rule = whole_value_rule(pattern).ok()?;
            (!rule.is_match(value)).then(|| {
                let shown: String = value.chars().take(60).collect();
                format!("Param {name:?} must match its rule {pattern}, and {shown:?} does not.")
            })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetupStep {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub run: SetupRun,
}

/// One shell command per OS, kept exactly as written: it is shown to the
/// person verbatim, and its hash is what tells a changed step from a done one.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SetupRun {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<String>,
}

impl SetupRun {
    pub fn for_os(&self, os: &str) -> Option<&str> {
        match os {
            "windows" => self.windows.as_deref(),
            "macos" => self.macos.as_deref(),
            "linux" => self.linux.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredCommand {
    pub description: String,
    pub run: CommandRun,
    pub args: BTreeMap<String, CommandArg>,
    pub timeout_seconds: u64,
}

/// What a `{name}` may hold.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandArg {
    /// As written: it is shown to the person, and matched against whole values
    /// (`whole_value_rule`).
    pub pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One argv per OS: the program first, then literal items and placeholders. A
/// `run` written as a single array holds it for every OS.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CommandRun {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<Vec<String>>,
}

impl CommandRun {
    pub fn for_os(&self, os: &str) -> Option<&[String]> {
        match os {
            "windows" => self.windows.as_deref(),
            "macos" => self.macos.as_deref(),
            "linux" => self.linux.as_deref(),
            _ => None,
        }
    }
}

/// This machine, as `setup[].run` names it. Every unix that is not a Mac
/// counts as `linux`.
pub fn current_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// `[a-z0-9-]`, between `min` and `max` characters.
pub fn valid_id(id: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn valid_kit_id(id: &str) -> bool {
    valid_id(id, 2, 40)
}

/// A manifest path as Hangar keeps it: forward slashes and no `.` parts, `.`
/// for the root itself. Either slash separates, whatever the OS, so a kit
/// written on Windows reads the same elsewhere.
pub fn normalize(raw: &str) -> Result<String, String> {
    if raw.trim().is_empty() {
        return Err("is empty".into());
    }
    if raw.contains('\0') {
        return Err("holds a NUL character".into());
    }
    if raw.starts_with(['/', '\\']) {
        return Err("must be relative to the kit folder".into());
    }
    let mut parts = Vec::new();
    for part in raw.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err("must not climb out with `..`".into()),
            // A drive (`C:`) or an alternate data stream: never relative.
            part if part.contains(':') => {
                return Err("must be relative to the kit folder".into());
            }
            part => parts.push(part),
        }
    }
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

/// Where a normalized path lands on disk, once symlinks are followed; refused
/// when that is outside `root` (itself canonical) or when nothing is there.
pub fn inside(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let joined = if relative == "." {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    let real =
        dunce::canonicalize(&joined).map_err(|_| "not found in the kit folder".to_string())?;
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err("leads outside the kit folder".into())
    }
}

/// Reads and checks `<root>/hangar-kit.json`.
pub fn read(root: &Path) -> Result<Manifest, String> {
    let root = dunce::canonicalize(root)
        .map_err(|_| format!("the kit folder {} is not there", root.display()))?;
    let text = match fs::read_to_string(root.join(FILE)) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("no {FILE} in {}", root.display()));
        }
        Err(err) => return Err(format!("{FILE} could not be read: {err}")),
    };
    parse(&text, &root)
}

/// Checks a manifest's text against the kit folder it sits in, `root` being
/// canonical. Every problem found is reported, joined by `; `.
pub fn parse(text: &str, root: &Path) -> Result<Manifest, String> {
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|err| format!("{FILE} is not valid JSON: {err}"))?;
    let Value::Object(object) = value else {
        return Err(format!("{FILE} must hold a JSON object"));
    };
    let mut check = Check {
        root,
        problems: Vec::new(),
        declared_commands: HashSet::new(),
    };
    let manifest = check.manifest(&object);
    if check.problems.is_empty() {
        Ok(manifest)
    } else {
        Err(check.problems.join("; "))
    }
}

/// Whether this Hangar is one the kit's `hangar` requirement accepts.
pub fn compatible(manifest: &Manifest, hangar_version: &str) -> Result<(), String> {
    let Some(requirement) = &manifest.hangar else {
        return Ok(());
    };
    let (Ok(requirement), Ok(version)) = (
        semver::VersionReq::parse(requirement),
        semver::Version::parse(hangar_version),
    ) else {
        return Ok(());
    };
    if requirement.matches(&version) {
        Ok(())
    } else {
        Err(format!(
            "needs Hangar {requirement}, and this is Hangar {version}"
        ))
    }
}

/// The version a manifest declares, read without checking anything else: what
/// an update check compares against the installed one.
pub fn version_of(text: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let id = value.get("id")?.as_str()?.to_string();
    let version = value.get("version")?.as_str()?.to_string();
    semver::Version::parse(&version).ok()?;
    Some((id, version))
}

struct Check<'a> {
    root: &'a Path,
    problems: Vec<String>,
    /// Every name under `commands`, well-formed or not: a tab naming one that
    /// is wrong is told about it there, not a second time here.
    declared_commands: HashSet<String>,
}

impl Check<'_> {
    fn problem(&mut self, at: &str, what: impl std::fmt::Display) {
        self.problems.push(format!("{at}: {what}"));
    }

    /// A field that has to be a string, `None` when it is missing or is not
    /// one (and that is reported).
    fn required(&mut self, object: &Map<String, Value>, key: &str, at: &str) -> Option<String> {
        match object.get(key) {
            Some(Value::String(text)) => Some(text.clone()),
            None | Some(Value::Null) => {
                self.problem(&field(at, key), "is missing");
                None
            }
            Some(_) => {
                self.problem(&field(at, key), "must be a string");
                None
            }
        }
    }

    /// Like `required`, and blank counts as missing.
    fn required_text(
        &mut self,
        object: &Map<String, Value>,
        key: &str,
        at: &str,
    ) -> Option<String> {
        let text = self.required(object, key, at)?;
        if text.trim().is_empty() {
            self.problem(&field(at, key), "is empty");
            return None;
        }
        Some(text)
    }

    fn optional(&mut self, object: &Map<String, Value>, key: &str, at: &str) -> Option<String> {
        match object.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(_) => {
                self.problem(&field(at, key), "must be a string");
                None
            }
        }
    }

    /// A relative path to a file inside the kit, with one of `extensions`.
    fn file(&mut self, raw: &str, at: &str, extensions: &[&str]) -> Option<String> {
        let path = match normalize(raw) {
            Ok(path) => path,
            Err(why) => {
                self.problem(at, why);
                return None;
            }
        };
        let lower = path.to_ascii_lowercase();
        if !extensions.iter().any(|ext| lower.ends_with(ext)) {
            self.problem(at, format!("must be a {} file", extensions.join(" or ")));
            return None;
        }
        match inside(self.root, &path) {
            Ok(real) if real.is_file() => Some(path),
            Ok(_) => {
                self.problem(at, "is not a file");
                None
            }
            Err(why) => {
                self.problem(at, why);
                None
            }
        }
    }

    fn manifest(&mut self, object: &Map<String, Value>) -> Manifest {
        let schema = match object.get("schema") {
            Some(Value::Number(number)) if number.as_u64() == Some(SCHEMA) => SCHEMA,
            Some(Value::Number(number)) if number.as_u64().is_some_and(|n| n > SCHEMA) => {
                self.problem(
                    "schema",
                    format!("{number} needs a newer Hangar (this one reads {SCHEMA})"),
                );
                SCHEMA
            }
            None => {
                self.problem("schema", "is missing");
                SCHEMA
            }
            Some(_) => {
                self.problem("schema", format!("must be {SCHEMA}"));
                SCHEMA
            }
        };

        let id = self.required(object, "id", "").unwrap_or_default();
        if !id.is_empty() && !valid_kit_id(&id) {
            self.problem("id", "must be 2 to 40 characters among a-z, 0-9 and -");
        }
        let name = self.required_text(object, "name", "").unwrap_or_default();
        let version = self.required(object, "version", "").unwrap_or_default();
        if !version.is_empty() && semver::Version::parse(&version).is_err() {
            self.problem(
                "version",
                format!("{version:?} is not a semver version (like 1.2.0)"),
            );
        }
        let description = self.required(object, "description", "").unwrap_or_default();
        let author = self.optional(object, "author", "");
        let homepage = self.optional(object, "homepage", "");
        if let Some(homepage) = &homepage {
            let lower = homepage.to_ascii_lowercase();
            if !lower.starts_with("https://") && !lower.starts_with("http://") {
                self.problem("homepage", "must be an http(s) link");
            }
        }
        let icon = self
            .optional(object, "icon", "")
            .and_then(|raw| self.file(&raw, "icon", &[".svg", ".png"]));
        let hangar = self.optional(object, "hangar", "");
        if let Some(requirement) = &hangar {
            if semver::VersionReq::parse(requirement).is_err() {
                self.problem(
                    "hangar",
                    format!("{requirement:?} is not a version requirement (like >=1.6.0)"),
                );
            }
        }
        let plugin = self.plugin(object);
        // Before the tabs, whose permissions may name these.
        let commands = self.commands(object);
        let tabs = self.tabs(object);
        let setup = self.setup(object);

        Manifest {
            schema,
            id,
            name,
            version,
            description,
            author,
            homepage,
            icon,
            hangar,
            plugin,
            tabs,
            setup,
            commands,
        }
    }

    /// Named, or `.` when the root holds a `.claude-plugin` folder.
    fn plugin(&mut self, object: &Map<String, Value>) -> Option<String> {
        let raw = match self.optional(object, "plugin", "") {
            Some(raw) => raw,
            None if object.get("plugin").is_none_or(Value::is_null)
                && self.root.join(".claude-plugin").is_dir() =>
            {
                ".".to_string()
            }
            None => return None,
        };
        let path = match normalize(&raw) {
            Ok(path) => path,
            Err(why) => {
                self.problem("plugin", why);
                return None;
            }
        };
        match inside(self.root, &path) {
            Ok(dir) if dir.join(".claude-plugin").join("plugin.json").is_file() => Some(path),
            Ok(_) => {
                self.problem("plugin", "holds no .claude-plugin/plugin.json");
                None
            }
            Err(why) => {
                self.problem("plugin", why);
                None
            }
        }
    }

    fn list<'v>(&mut self, object: &'v Map<String, Value>, key: &str) -> &'v [Value] {
        match object.get(key) {
            None | Some(Value::Null) => &[],
            Some(Value::Array(items)) => items,
            Some(_) => {
                self.problem(key, "must be a list");
                &[]
            }
        }
    }

    /// An item of a list, which has to be an object with an `id`.
    fn item<'v>(
        &mut self,
        value: &'v Value,
        at: &str,
        seen: &mut HashSet<String>,
    ) -> Option<(&'v Map<String, Value>, String)> {
        let Value::Object(object) = value else {
            self.problem(at, "must be an object");
            return None;
        };
        let id = self.required(object, "id", at)?;
        if !valid_id(&id, 1, 40) {
            self.problem(
                &field(at, "id"),
                "must be 1 to 40 characters among a-z, 0-9 and -",
            );
        } else if !seen.insert(id.clone()) {
            self.problem(&field(at, "id"), format!("{id:?} is used twice"));
        }
        Some((object, id))
    }

    fn tabs(&mut self, object: &Map<String, Value>) -> Vec<Tab> {
        let mut seen = HashSet::new();
        let mut tabs = Vec::new();
        for (index, value) in self.list(object, "tabs").iter().enumerate() {
            let at = format!("tabs[{index}]");
            let Some((tab, id)) = self.item(value, &at, &mut seen) else {
                continue;
            };
            let title = self.required_text(tab, "title", &at);
            let entry = self
                .required(tab, "entry", &at)
                .and_then(|raw| self.file(&raw, &field(&at, "entry"), &[".html"]));
            let icon = self
                .optional(tab, "icon", &at)
                .and_then(|raw| self.file(&raw, &field(&at, "icon"), &[".svg", ".png"]));
            let permissions = self.permissions(tab, &at);
            let params = self.tab_params(tab, &at);
            let singleton = self.tab_singleton(tab, &at);
            if let (Some(title), Some(entry), Some(permissions), Some(params)) =
                (title, entry, permissions, params)
            {
                tabs.push(Tab {
                    id,
                    title,
                    entry,
                    icon,
                    permissions,
                    params,
                    singleton,
                });
            }
        }
        tabs
    }

    /// `params`: what a caller may pass a tab, by name. `None` when one is wrong.
    fn tab_params(
        &mut self,
        tab: &Map<String, Value>,
        at: &str,
    ) -> Option<BTreeMap<String, TabParam>> {
        let at = field(at, "params");
        let declared = match tab.get("params") {
            None | Some(Value::Null) => return Some(BTreeMap::new()),
            Some(Value::Object(declared)) => declared,
            Some(_) => {
                self.problem(&at, "must be an object of params by name");
                return None;
            }
        };
        let mut params = BTreeMap::new();
        let mut ok = true;
        for (name, value) in declared {
            let at = field(&at, name);
            if !valid_arg_name(name) {
                self.problem(
                    &at,
                    "must be named with a letter or _ first, then letters, digits, _ and -",
                );
                ok = false;
            }
            let Value::Object(param) = value else {
                self.problem(&at, "must be an object with a description");
                ok = false;
                continue;
            };
            let Some(description) = self.required_text(param, "description", &at) else {
                ok = false;
                continue;
            };
            let pattern = self.optional(param, "pattern", &at);
            if let Some(pattern) = &pattern {
                let invalid = if pattern.trim().is_empty() {
                    Some("is empty".to_string())
                } else {
                    whole_value_rule(pattern).err()
                };
                if let Some(why) = invalid {
                    self.problem(&field(&at, "pattern"), why);
                    ok = false;
                    continue;
                }
            }
            params.insert(
                name.clone(),
                TabParam {
                    description,
                    pattern,
                },
            );
        }
        ok.then_some(params)
    }

    fn tab_singleton(&mut self, tab: &Map<String, Value>, at: &str) -> bool {
        match tab.get("singleton") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(singleton)) => *singleton,
            Some(_) => {
                self.problem(&field(at, "singleton"), "must be true or false");
                false
            }
        }
    }

    fn permissions(&mut self, tab: &Map<String, Value>, at: &str) -> Option<Vec<String>> {
        let at = field(at, "permissions");
        let Some(value) = tab.get("permissions") else {
            self.problem(&at, "is missing (an empty list asks for nothing)");
            return None;
        };
        let Value::Array(items) = value else {
            self.problem(&at, "must be a list");
            return None;
        };
        let mut permissions: Vec<String> = Vec::new();
        let mut ok = true;
        for item in items {
            match item.as_str() {
                Some(name) if PERMISSIONS.contains(&name) || loopback_port(name).is_some() => {
                    if !permissions.iter().any(|known| known == name) {
                        permissions.push(name.to_string());
                    }
                }
                Some(name) if name.starts_with("loopback:") => {
                    self.problem(&at, format!("{name:?} must name a port from 1024 to 65535"));
                    ok = false;
                }
                Some(name) if name.starts_with("command:") => {
                    if self.declared_commands.contains(&name["command:".len()..]) {
                        if !permissions.iter().any(|known| known == name) {
                            permissions.push(name.to_string());
                        }
                    } else {
                        self.problem(
                            &at,
                            format!("{name:?} names a command this kit does not declare"),
                        );
                        ok = false;
                    }
                }
                Some(name) => {
                    self.problem(&at, format!("unknown permission {name:?}"));
                    ok = false;
                }
                None => {
                    self.problem(&at, "must list strings");
                    ok = false;
                }
            }
        }
        ok.then_some(permissions)
    }

    fn setup(&mut self, object: &Map<String, Value>) -> Vec<SetupStep> {
        let mut seen = HashSet::new();
        let mut steps = Vec::new();
        for (index, value) in self.list(object, "setup").iter().enumerate() {
            let at = format!("setup[{index}]");
            let Some((step, id)) = self.item(value, &at, &mut seen) else {
                continue;
            };
            let title = self.required_text(step, "title", &at);
            let description = self.optional(step, "description", &at);
            let run = self.run(step, &at);
            if let (Some(title), Some(run)) = (title, run) {
                steps.push(SetupStep {
                    id,
                    title,
                    description,
                    run,
                });
            }
        }
        steps
    }

    fn run(&mut self, step: &Map<String, Value>, at: &str) -> Option<SetupRun> {
        let at = field(at, "run");
        let Some(Value::Object(run)) = step.get("run") else {
            let why = if step.get("run").is_none() {
                "is missing"
            } else {
                "must be an object of commands by OS"
            };
            self.problem(&at, why);
            return None;
        };
        let mut command = |os: &str| -> Option<String> {
            let text = self.optional(run, os, &at)?;
            if text.trim().is_empty() {
                self.problem(&field(&at, os), "is empty");
                return None;
            }
            Some(text)
        };
        let parsed = SetupRun {
            windows: command("windows"),
            macos: command("macos"),
            linux: command("linux"),
        };
        if parsed == SetupRun::default() {
            self.problem(&at, "needs a command for windows, macos or linux");
            return None;
        }
        Some(parsed)
    }

    /// `commands`: an object of commands by name.
    fn commands(&mut self, object: &Map<String, Value>) -> BTreeMap<String, DeclaredCommand> {
        let mut commands = BTreeMap::new();
        let declared = match object.get("commands") {
            None | Some(Value::Null) => return commands,
            Some(Value::Object(declared)) => declared,
            Some(_) => {
                self.problem("commands", "must be an object of commands by name");
                return commands;
            }
        };
        self.declared_commands = declared.keys().cloned().collect();
        for (name, value) in declared {
            let at = format!("commands.{name}");
            if !valid_command_name(name) {
                self.problem(&at, "must be named with a-z first, then a-z, 0-9 and -");
            }
            let Value::Object(command) = value else {
                self.problem(&at, "must be an object");
                continue;
            };
            let description = self.required_text(command, "description", &at);
            let args = self.command_args(command, &at);
            let run = self.command_run(command, &at, args.as_ref());
            let timeout_seconds = self.command_timeout(command, &at);
            if let (Some(description), Some(run), Some(args), Some(timeout_seconds)) =
                (description, run, args, timeout_seconds)
            {
                commands.insert(
                    name.clone(),
                    DeclaredCommand {
                        description,
                        run,
                        args,
                        timeout_seconds,
                    },
                );
            }
        }
        commands
    }

    /// `args`: what each placeholder may hold. `None` when one is wrong; the
    /// placeholders of `run` are then not looked up, which would only say more
    /// of the same.
    fn command_args(
        &mut self,
        command: &Map<String, Value>,
        at: &str,
    ) -> Option<BTreeMap<String, CommandArg>> {
        let at = field(at, "args");
        let declared = match command.get("args") {
            None | Some(Value::Null) => return Some(BTreeMap::new()),
            Some(Value::Object(declared)) => declared,
            Some(_) => {
                self.problem(&at, "must be an object of args by name");
                return None;
            }
        };
        let mut args = BTreeMap::new();
        let mut ok = true;
        for (name, value) in declared {
            let at = field(&at, name);
            if !valid_arg_name(name) {
                self.problem(
                    &at,
                    "must be named with a letter or _ first, then letters, digits, _ and -",
                );
                ok = false;
            }
            if name == KIT_DIR {
                self.problem(
                    &at,
                    "is reserved: Hangar fills {kitDir} in with the kit's folder",
                );
                ok = false;
                continue;
            }
            let Value::Object(arg) = value else {
                self.problem(&at, "must be an object with a pattern");
                ok = false;
                continue;
            };
            let Some(pattern) = self.required_text(arg, "pattern", &at) else {
                ok = false;
                continue;
            };
            if let Err(why) = whole_value_rule(&pattern) {
                self.problem(&field(&at, "pattern"), why);
                ok = false;
                continue;
            }
            let description = self.optional(arg, "description", &at);
            args.insert(
                name.clone(),
                CommandArg {
                    pattern,
                    description,
                },
            );
        }
        ok.then_some(args)
    }

    /// `run`: one argv, or one per OS. Never a command line: nothing would be
    /// there to say which words a value may become.
    fn command_run(
        &mut self,
        command: &Map<String, Value>,
        at: &str,
        args: Option<&BTreeMap<String, CommandArg>>,
    ) -> Option<CommandRun> {
        let at = field(at, "run");
        match command.get("run") {
            Some(value @ Value::Array(_)) => {
                let argv = self.argv(value, &at, args)?;
                Some(CommandRun {
                    windows: Some(argv.clone()),
                    macos: Some(argv.clone()),
                    linux: Some(argv),
                })
            }
            Some(Value::Object(by_os)) => {
                let mut ok = true;
                let mut for_os = |os: &str| -> Option<Vec<String>> {
                    let value = by_os.get(os).filter(|value| !value.is_null())?;
                    let argv = self.argv(value, &field(&at, os), args);
                    ok &= argv.is_some();
                    argv
                };
                let run = CommandRun {
                    windows: for_os("windows"),
                    macos: for_os("macos"),
                    linux: for_os("linux"),
                };
                if !ok {
                    return None;
                }
                if run == CommandRun::default() {
                    self.problem(&at, "needs an argv for windows, macos or linux");
                    return None;
                }
                Some(run)
            }
            None | Some(Value::Null) => {
                self.problem(&at, "is missing");
                None
            }
            Some(_) => {
                self.problem(
                    &at,
                    "must be an argv array, or one per OS: never a shell command line",
                );
                None
            }
        }
    }

    /// One argv: the program, then literal items and placeholders.
    fn argv(
        &mut self,
        value: &Value,
        at: &str,
        args: Option<&BTreeMap<String, CommandArg>>,
    ) -> Option<Vec<String>> {
        let Value::Array(items) = value else {
            self.problem(at, "must be an argv array");
            return None;
        };
        let mut argv = Vec::with_capacity(items.len());
        let mut ok = true;
        for (index, item) in items.iter().enumerate() {
            let at = format!("{at}[{index}]");
            let Value::String(item) = item else {
                self.problem(&at, "must be a string");
                ok = false;
                continue;
            };
            if item.contains('\0') {
                self.problem(&at, "holds a NUL character");
                ok = false;
                continue;
            }
            argv.push(item.clone());
            if index == 0 {
                if let Err(why) = program_name(item) {
                    self.problem(&at, why);
                    ok = false;
                }
                continue;
            }
            let Some(args) = args else { continue };
            for part in arg_parts(item) {
                if let ArgPart::Value(name) = part {
                    if name != KIT_DIR && !args.contains_key(name) {
                        self.problem(
                            &at,
                            format!("the placeholder {{{name}}} is not declared in args"),
                        );
                        ok = false;
                    }
                }
            }
        }
        if items.is_empty() {
            self.problem(at, "needs at least the program");
            ok = false;
        }
        ok.then_some(argv)
    }

    fn command_timeout(&mut self, command: &Map<String, Value>, at: &str) -> Option<u64> {
        match command.get("timeoutSeconds") {
            None | Some(Value::Null) => Some(COMMAND_TIMEOUT_SECONDS),
            Some(value) => {
                let seconds = value
                    .as_u64()
                    .filter(|seconds| (1..=COMMAND_MAX_TIMEOUT_SECONDS).contains(seconds));
                if seconds.is_none() {
                    self.problem(
                        &field(at, "timeoutSeconds"),
                        format!(
                            "must be a whole number of seconds from 1 to {COMMAND_MAX_TIMEOUT_SECONDS}"
                        ),
                    );
                }
                seconds
            }
        }
    }
}

/// The program of an argv: a name to find on the PATH, and only that. A path
/// would run whatever a project keeps in its folder, which is where a command
/// starts; the name is also what the person is shown and agrees to. A trailing
/// dot or space, and a stream, are ways for Windows to open another file than
/// the one named.
fn program_name(program: &str) -> Result<(), &'static str> {
    if program.trim().is_empty() {
        return Err("is empty: the program comes first");
    }
    if program.contains(['{', '}']) {
        return Err("must not hold a placeholder: the program is fixed");
    }
    if program.contains(['/', '\\', ':']) || program.ends_with(['.', ' ']) {
        return Err("must be a program's name, found on the PATH, not a path");
    }
    Ok(())
}

fn field(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_string()
    } else {
        format!("{at}.{key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::testing::Folder;
    use serde_json::json;

    fn minimal() -> Value {
        json!({
            "schema": 1,
            "id": "roblox",
            "name": "Roblox",
            "version": "1.0.0",
            "description": "Roblox Studio in Hangar."
        })
    }

    fn with(mut base: Value, patch: Value) -> Value {
        for (key, value) in patch.as_object().unwrap() {
            base[key] = value.clone();
        }
        base
    }

    fn problems(result: Result<Manifest, String>) -> String {
        result.expect_err("the manifest should have been refused")
    }

    fn tab_with(patch: Value) -> Value {
        let mut tab =
            json!({ "id": "run", "title": "CI run", "entry": "tab.html", "permissions": [] });
        for (key, value) in patch.as_object().unwrap() {
            tab[key] = value.clone();
        }
        with(minimal(), json!({ "tabs": [tab] }))
    }

    #[test]
    fn a_tab_declares_its_params_and_whether_it_is_one_tab() {
        let kit = Folder::new("tab-params");
        kit.file("tab.html", "<!doctype html>");
        let manifest = kit
            .parse(&tab_with(json!({
                "singleton": true,
                "params": {
                    "run": { "description": "The run's number", "pattern": "[0-9]+" },
                    "repo": { "description": "owner/name" }
                }
            })))
            .unwrap();
        let tab = &manifest.tabs[0];
        assert!(tab.singleton);
        assert_eq!(tab.params["run"].description, "The run's number");
        assert_eq!(tab.params["run"].pattern.as_deref(), Some("[0-9]+"));
        assert_eq!(tab.params["repo"].pattern, None);

        // As the window reads it: nothing there when nothing was written.
        let bare = kit.parse(&tab_with(json!({}))).unwrap();
        assert!(!bare.tabs[0].singleton);
        assert!(bare.tabs[0].params.is_empty());
        let written = serde_json::to_value(&bare.tabs[0]).unwrap();
        assert!(written.get("params").is_none());
        assert!(written.get("singleton").is_none());
        let written = serde_json::to_value(tab).unwrap();
        assert_eq!(written["singleton"], true);
        assert_eq!(written["params"]["run"]["pattern"], "[0-9]+");
    }

    #[test]
    fn a_tab_s_params_are_checked_with_the_rest_of_the_manifest() {
        let kit = Folder::new("tab-params-bad");
        kit.file("tab.html", "<!doctype html>");
        let refused = |patch: Value| problems(kit.parse(&tab_with(patch)));

        assert_eq!(
            refused(json!({ "params": ["run"] })),
            "tabs[0].params: must be an object of params by name"
        );
        assert_eq!(
            refused(json!({ "params": { "1run": { "description": "x" } } })),
            "tabs[0].params.1run: must be named with a letter or _ first, then letters, digits, _ and -"
        );
        assert_eq!(
            refused(json!({ "params": { "run": "x" } })),
            "tabs[0].params.run: must be an object with a description"
        );
        assert_eq!(
            refused(json!({ "params": { "run": { "pattern": "[0-9]+" } } })),
            "tabs[0].params.run.description: is missing"
        );
        assert_eq!(
            refused(json!({ "params": { "run": { "description": "  " } } })),
            "tabs[0].params.run.description: is empty"
        );
        assert_eq!(
            refused(json!({ "params": { "run": { "description": "x", "pattern": "[" } } })),
            "tabs[0].params.run.pattern: is not a valid regular expression"
        );
        assert_eq!(
            refused(json!({ "params": { "run": { "description": "x", "pattern": " " } } })),
            "tabs[0].params.run.pattern: is empty"
        );
        assert_eq!(
            refused(json!({ "singleton": "yes" })),
            "tabs[0].singleton: must be true or false"
        );
    }

    #[test]
    fn a_param_is_held_to_its_rule_whole() {
        let kit = Folder::new("tab-params-rule");
        kit.file("tab.html", "<!doctype html>");
        let manifest = kit
            .parse(&tab_with(json!({
                "params": {
                    "run": { "description": "x", "pattern": "[0-9]+" },
                    "note": { "description": "x" }
                }
            })))
            .unwrap();
        let tab = &manifest.tabs[0];

        assert_eq!(
            tab.param_refusal(&json!({ "run": "abc" })).as_deref(),
            Some(r#"Param "run" must match its rule [0-9]+, and "abc" does not."#)
        );
        // Anchored: a value is not a match for holding one.
        assert!(tab.param_refusal(&json!({ "run": "12a" })).is_some());
        assert!(tab.param_refusal(&json!({ "run": "12\n" })).is_some());
        assert_eq!(tab.param_refusal(&json!({ "run": "4567" })), None);
        // No rule, nothing to hold it to.
        assert_eq!(tab.param_refusal(&json!({ "note": "anything" })), None);
        // Undeclared, not a string, or no params at all: the window's to say.
        assert_eq!(tab.param_refusal(&json!({ "other": "x" })), None);
        assert_eq!(tab.param_refusal(&json!({ "run": 4567 })), None);
        assert_eq!(tab.param_refusal(&Value::Null), None);
    }

    /// The reason the rule runs here: the same value in a backtracking engine
    /// would take longer than anyone waits, in the window that draws every pane.
    #[test]
    fn a_rule_that_would_backtrack_for_ever_answers_at_once() {
        let kit = Folder::new("tab-params-backtrack");
        kit.file("tab.html", "<!doctype html>");
        let manifest = kit
            .parse(&tab_with(json!({
                "params": { "x": { "description": "x", "pattern": "(a+)+b" } }
            })))
            .unwrap();
        let value = "a".repeat(4000);
        assert!(manifest.tabs[0]
            .param_refusal(&json!({ "x": value }))
            .is_some());
    }

    #[test]
    fn a_complete_manifest_reads_whole() {
        let kit = Folder::new("good");
        kit.plugin(".", "roblox")
            .file("icon.svg", "<svg/>")
            .file("tabs/explorer/index.html", "<!doctype html>")
            .file("tabs/explorer/icon.png", "png");
        let manifest = kit
            .parse(&with(
                minimal(),
                json!({
                    "author": "simnJS",
                    "homepage": "https://example.com/kit",
                    "icon": "./icon.svg",
                    "hangar": ">=1.6.0",
                    "tabs": [{
                        "id": "explorer",
                        "title": "Explorer",
                        "entry": "tabs\\explorer\\index.html",
                        "icon": "tabs/explorer/icon.png",
                        "permissions": ["workspace.read", "panes.send", "panes.send"]
                    }],
                    "setup": [{
                        "id": "plugin",
                        "title": "Install the Studio plugin",
                        "description": "Copies the bridge plugin into Studio.",
                        "run": { "windows": "npx rbx-bridge install", "macos": "npx rbx-bridge install" }
                    }],
                    "somethingNew": { "ignored": true }
                }),
            ))
            .unwrap();

        assert_eq!(manifest.id, "roblox");
        assert_eq!(manifest.icon.as_deref(), Some("icon.svg"));
        assert_eq!(manifest.plugin.as_deref(), Some("."), "found by itself");
        assert_eq!(manifest.tabs[0].entry, "tabs/explorer/index.html");
        assert_eq!(
            manifest.tabs[0].permissions,
            vec!["workspace.read", "panes.send"],
            "listed once each"
        );
        assert_eq!(manifest.setup[0].run.linux, None);
        assert_eq!(
            manifest.setup[0].run.for_os("windows"),
            Some("npx rbx-bridge install")
        );
    }

    #[test]
    fn a_kit_without_a_plugin_folder_brings_none() {
        let kit = Folder::new("noplugin");
        assert_eq!(kit.parse(&minimal()).unwrap().plugin, None);
        kit.plugin("claude", "roblox");
        let named = kit
            .parse(&with(minimal(), json!({ "plugin": "claude/" })))
            .unwrap();
        assert_eq!(named.plugin.as_deref(), Some("claude"));
    }

    #[test]
    fn missing_fields_are_each_named() {
        let kit = Folder::new("missing");
        let why = problems(kit.parse(&json!({ "id": "roblox", "name": "  " })));
        for expected in [
            "schema: is missing",
            "name: is empty",
            "version: is missing",
            "description: is missing",
        ] {
            assert!(why.contains(expected), "{expected} not in {why}");
        }
        assert!(problems(kit.parse(&json!([1]))).contains("JSON object"));
        assert!(problems(parse("{ nope", &kit.root)).contains("not valid JSON"));
    }

    #[test]
    fn a_bad_id_or_version_is_refused() {
        let kit = Folder::new("ids");
        for id in ["R", "Roblox", "rob_lox", "a", &"x".repeat(41), "kit.v2"] {
            let why = problems(kit.parse(&with(minimal(), json!({ "id": id }))));
            assert!(why.starts_with("id: "), "{id}: {why}");
        }
        for id in ["ro", "roblox-studio", "k2", &"x".repeat(40)] {
            assert!(
                kit.parse(&with(minimal(), json!({ "id": id }))).is_ok(),
                "{id}"
            );
        }
        assert!(
            problems(kit.parse(&with(minimal(), json!({ "version": "1.0" })))).contains("semver")
        );
        assert!(
            problems(kit.parse(&with(minimal(), json!({ "schema": 2 })))).contains("newer Hangar")
        );
        assert!(
            problems(kit.parse(&with(minimal(), json!({ "hangar": "soon" })))).contains("hangar")
        );
        assert!(problems(kit.parse(&with(
            minimal(),
            json!({ "homepage": "javascript:alert(1)" })
        )))
        .contains("homepage"));
    }

    #[test]
    fn an_unknown_permission_is_an_error() {
        let kit = Folder::new("perms");
        kit.file("tab.html", "");
        let tab = |permissions: Value| {
            with(
                minimal(),
                json!({ "tabs": [{ "id": "t", "title": "T", "entry": "tab.html", "permissions": permissions }] }),
            )
        };
        let why = problems(kit.parse(&tab(json!(["panes.read", "files.delete"]))));
        assert_eq!(
            why,
            "tabs[0].permissions: unknown permission \"files.delete\""
        );
        assert!(problems(kit.parse(&tab(Value::Null))).contains("permissions"));
        assert!(kit.parse(&tab(json!([]))).unwrap().tabs[0]
            .permissions
            .is_empty());
        for permission in PERMISSIONS {
            assert!(kit.parse(&tab(json!([permission]))).is_ok(), "{permission}");
        }
    }

    #[test]
    fn data_and_a_local_server_can_be_asked_for() {
        let kit = Folder::new("perms-new");
        kit.file("tab.html", "");
        let tab = |permissions: Value| {
            with(
                minimal(),
                json!({ "tabs": [{ "id": "t", "title": "T", "entry": "tab.html", "permissions": permissions }] }),
            )
        };
        let manifest = kit
            .parse(&tab(json!([
                "data.read",
                "data.write",
                "loopback:8765",
                "loopback:1024",
                "loopback:65535",
                "loopback:8765"
            ])))
            .unwrap();
        assert_eq!(
            manifest.tabs[0].permissions,
            [
                "data.read",
                "data.write",
                "loopback:8765",
                "loopback:1024",
                "loopback:65535"
            ],
            "listed once each"
        );
        for bad in [
            "loopback:80",
            "loopback:1023",
            "loopback:0",
            "loopback:abc",
            "loopback:65536",
            "loopback:99999",
            "loopback:",
            "loopback:08765",
            "loopback:+8765",
            "loopback:-8765",
            "loopback: 8765",
            "loopback:8765 ",
            "loopback:8765:1",
        ] {
            let why = problems(kit.parse(&tab(json!([bad]))));
            assert!(why.contains("port from 1024 to 65535"), "{bad}: {why}");
        }
        // Not to be declared: a workspace stores it, a tab never holds it.
        for bad in [
            "hooks:0123456789abcdef",
            "loopback",
            "Loopback:8765",
            "data",
        ] {
            let why = problems(kit.parse(&tab(json!([bad]))));
            assert!(why.contains("unknown permission"), "{bad}: {why}");
        }
    }

    fn with_commands(commands: Value) -> Value {
        with(minimal(), json!({ "commands": commands }))
    }

    /// A declared command, and a tab of the same kit holding `permissions`.
    fn command_kit(permissions: Value) -> Value {
        with(
            minimal(),
            json!({
                "commands": {
                    "ci-status": {
                        "description": "The state of a CI run.",
                        "run": ["gh", "run", "view", "{run}", "--repo={repo}", "--json", "status"],
                        "args": {
                            "run": { "pattern": "[0-9]+", "description": "The run's number" },
                            "repo": { "pattern": "[\\w.-]+/[\\w.-]+" }
                        },
                        "timeoutSeconds": 60
                    }
                },
                "tabs": [{ "id": "t", "title": "T", "entry": "tab.html", "permissions": permissions }]
            }),
        )
    }

    #[test]
    fn a_declared_command_reads_whole() {
        let kit = Folder::new("cmd-good");
        kit.file("tab.html", "");
        let manifest = kit
            .parse(&command_kit(json!([
                "command:ci-status",
                "command:ci-status",
                "notify"
            ])))
            .unwrap();
        assert_eq!(
            manifest.tabs[0].permissions,
            ["command:ci-status", "notify"],
            "listed once each"
        );
        let command = &manifest.commands["ci-status"];
        assert_eq!(command.description, "The state of a CI run.");
        assert_eq!(command.timeout_seconds, 60);
        assert_eq!(command.args["run"].pattern, "[0-9]+");
        assert_eq!(
            command.args["run"].description.as_deref(),
            Some("The run's number")
        );
        assert_eq!(command.args["repo"].description, None);
        let argv = command.run.for_os("linux").unwrap();
        assert_eq!(argv[0], "gh");
        assert_eq!(argv[4], "--repo={repo}");
        assert_eq!(command.run.for_os("windows"), Some(argv), "for every OS");
        assert_eq!(command.run.for_os("freebsd"), None);
        let sent = serde_json::to_value(&manifest).unwrap();
        assert_eq!(sent["commands"]["ci-status"]["timeoutSeconds"], 60);
    }

    #[test]
    fn a_command_may_differ_per_os_and_times_out_after_30_seconds_by_default() {
        let kit = Folder::new("cmd-os");
        let manifest = kit
            .parse(&with_commands(json!({
                "tail": {
                    "description": "The log.",
                    "run": { "windows": ["logview.exe", "log"], "linux": ["tail", "log"] }
                }
            })))
            .unwrap();
        let command = &manifest.commands["tail"];
        assert_eq!(command.timeout_seconds, 30);
        assert!(command.args.is_empty());
        assert_eq!(command.run.for_os("windows").unwrap()[0], "logview.exe");
        assert_eq!(command.run.for_os("macos"), None);
        assert_eq!(command.run.for_os("linux").unwrap()[0], "tail");
        // The longest timeout and the shortest are fine.
        for seconds in [1, 120] {
            assert!(kit
                .parse(&with_commands(json!({
                    "t": { "description": "d", "run": ["x"], "timeoutSeconds": seconds }
                })))
                .is_ok());
        }
    }

    #[test]
    fn a_kit_without_commands_has_none() {
        let kit = Folder::new("cmd-none");
        assert!(kit.parse(&minimal()).unwrap().commands.is_empty());
        assert!(kit
            .parse(&with(minimal(), json!({ "commands": null })))
            .unwrap()
            .commands
            .is_empty());
    }

    #[test]
    fn a_placeholder_must_be_declared_and_an_arg_must_have_a_pattern() {
        let kit = Folder::new("cmd-args");
        let undeclared = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": ["gh", "run", "view", "{run}", "--repo={repo}"],
                      "args": { "run": { "pattern": "[0-9]+" } } }
        }))));
        assert_eq!(
            undeclared,
            "commands.view.run[4]: the placeholder {repo} is not declared in args"
        );
        let per_os = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": { "linux": ["gh", "{run}"] } }
        }))));
        assert_eq!(
            per_os,
            "commands.view.run.linux[1]: the placeholder {run} is not declared in args"
        );
        let no_pattern = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": ["gh", "{run}"],
                      "args": { "run": { "description": "The run" } } }
        }))));
        assert_eq!(no_pattern, "commands.view.args.run.pattern: is missing");
        for arg in [
            json!("[0-9]+"),
            json!({ "pattern": "  " }),
            json!({ "pattern": 3 }),
        ] {
            let why = problems(kit.parse(&with_commands(json!({
                "view": { "description": "d", "run": ["gh", "{run}"], "args": { "run": arg } }
            }))));
            assert!(why.starts_with("commands.view.args.run"), "{why}");
        }
        let bad_name = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": ["gh"], "args": { "a b": { "pattern": "x" } } }
        }))));
        assert!(
            bad_name.contains("commands.view.args.a b: must be named"),
            "{bad_name}"
        );
    }

    #[test]
    fn kit_dir_is_filled_in_by_hangar_and_needs_no_arg() {
        let kit = Folder::new("cmd-kit-dir");
        let manifest = kit
            .parse(&with_commands(json!({
                "audit": { "description": "d", "run": ["node", "{kitDir}/tools/audit.js", "--root={kitDir}", "{id}"],
                           "args": { "id": { "pattern": "[0-9]+" } } },
                "plain": { "description": "d", "run": { "windows": ["node", "{kitDir}\\x.js"], "linux": ["node", "{kitDir}/x.js"] } }
            })))
            .unwrap();
        assert_eq!(
            manifest.commands["audit"].run.for_os("linux").unwrap(),
            ["node", "{kitDir}/tools/audit.js", "--root={kitDir}", "{id}"],
            "kept as written"
        );
        assert_eq!(
            manifest.commands["audit"].args.keys().collect::<Vec<_>>(),
            ["id"],
            "and it is no arg"
        );
    }

    #[test]
    fn kit_dir_is_never_the_program() {
        let kit = Folder::new("cmd-kit-dir-program");
        for program in ["{kitDir}", "{kitDir}/tools/audit.exe", "x{kitDir}"] {
            let why = problems(kit.parse(&with_commands(json!({
                "view": { "description": "d", "run": [program, "x"] }
            }))));
            assert_eq!(
                why, "commands.view.run[0]: must not hold a placeholder: the program is fixed",
                "{program}"
            );
        }
    }

    #[test]
    fn kit_dir_cannot_be_declared_as_an_arg() {
        let kit = Folder::new("cmd-kit-dir-arg");
        for arg in [
            json!({ "pattern": ".*" }),
            json!({ "pattern": "x", "description": "d" }),
            json!("x"),
        ] {
            let why = problems(kit.parse(&with_commands(json!({
                "view": { "description": "d", "run": ["node", "{kitDir}/x.js"], "args": { "kitDir": arg } }
            }))));
            assert_eq!(
                why,
                "commands.view.args.kitDir: is reserved: Hangar fills {kitDir} in with the kit's folder"
            );
        }
        // Another spelling is any other name, held to the same rules.
        let why = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": ["node", "{kitdir}/x.js"] }
        }))));
        assert_eq!(
            why,
            "commands.view.run[1]: the placeholder {kitdir} is not declared in args"
        );
    }

    #[test]
    fn a_pattern_that_is_no_rule_or_escapes_its_anchors_is_refused() {
        let kit = Folder::new("cmd-pattern");
        for pattern in ["(", "a)|(?:b", "(?<=a)b", "[", "\\"] {
            let why = problems(kit.parse(&with_commands(json!({
                "view": { "description": "d", "run": ["gh", "{x}"], "args": { "x": { "pattern": pattern } } }
            }))));
            assert_eq!(
                why, "commands.view.args.x.pattern: is not a valid regular expression",
                "{pattern}"
            );
        }
    }

    #[test]
    fn the_program_is_a_fixed_name_on_the_path() {
        let kit = Folder::new("cmd-program");
        let why = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": ["{tool}", "x"], "args": { "tool": { "pattern": "gh|glab" } } }
        }))));
        assert_eq!(
            why,
            "commands.view.run[0]: must not hold a placeholder: the program is fixed"
        );
        let inside = problems(kit.parse(&with_commands(json!({
            "view": { "description": "d", "run": ["g{tool}"], "args": { "tool": { "pattern": "h" } } }
        }))));
        assert!(inside.contains("must not hold a placeholder"), "{inside}");
        for program in [
            "./gh",
            "bin/gh",
            "bin\\gh.exe",
            "/usr/bin/gh",
            "C:\\gh.exe",
            "gh.exe:stream",
            "gh.cmd.",
            "gh.cmd ",
            "..",
        ] {
            let why = problems(kit.parse(&with_commands(json!({
                "view": { "description": "d", "run": [program] }
            }))));
            assert_eq!(
                why,
                "commands.view.run[0]: must be a program's name, found on the PATH, not a path",
                "{program}"
            );
        }
        for empty in ["", "  "] {
            let why = problems(kit.parse(&with_commands(json!({
                "view": { "description": "d", "run": [empty] }
            }))));
            assert!(why.contains("the program comes first"), "{why}");
        }
        for program in ["gh", "gh.exe", "git-lfs", "python3.12"] {
            assert!(
                kit.parse(&with_commands(json!({
                    "view": { "description": "d", "run": [program] }
                })))
                .is_ok(),
                "{program}"
            );
        }
    }

    #[test]
    fn a_bad_name_is_refused() {
        let kit = Folder::new("cmd-name");
        for name in [
            "Run",
            "run_it",
            "1run",
            "-run",
            "ci status",
            "ci.status",
            "é",
        ] {
            let commands = json!({ name: { "description": "d", "run": ["gh"] } });
            let why = problems(kit.parse(&with_commands(commands)));
            assert_eq!(
                why,
                format!("commands.{name}: must be named with a-z first, then a-z, 0-9 and -")
            );
        }
        for name in ["run", "r", "ci-status", "ci-2", "a-"] {
            let commands = json!({ name: { "description": "d", "run": ["gh"] } });
            assert!(kit.parse(&with_commands(commands)).is_ok(), "{name}");
        }
    }

    #[test]
    fn a_timeout_above_the_maximum_is_refused() {
        let kit = Folder::new("cmd-timeout");
        for seconds in [json!(121), json!(0), json!(-1), json!(1.5), json!("30")] {
            let why = problems(kit.parse(&with_commands(json!({
                "t": { "description": "d", "run": ["x"], "timeoutSeconds": seconds }
            }))));
            assert_eq!(
                why,
                "commands.t.timeoutSeconds: must be a whole number of seconds from 1 to 120"
            );
        }
    }

    #[test]
    fn a_run_is_an_argv_and_never_a_command_line() {
        let kit = Folder::new("cmd-run");
        let line = problems(kit.parse(&with_commands(json!({
            "t": { "description": "d", "run": "gh run view" }
        }))));
        assert_eq!(
            line,
            "commands.t.run: must be an argv array, or one per OS: never a shell command line"
        );
        let per_os_line = problems(kit.parse(&with_commands(json!({
            "t": { "description": "d", "run": { "linux": "gh run view" } }
        }))));
        assert_eq!(per_os_line, "commands.t.run.linux: must be an argv array");
        for (run, expected) in [
            (json!([]), "commands.t.run: needs at least the program"),
            (json!(["gh", 3]), "commands.t.run[1]: must be a string"),
            (
                json!(["gh", "a\0b"]),
                "commands.t.run[1]: holds a NUL character",
            ),
            (
                json!({ "freebsd": ["gh"] }),
                "commands.t.run: needs an argv for windows, macos or linux",
            ),
            (
                json!({}),
                "commands.t.run: needs an argv for windows, macos or linux",
            ),
        ] {
            let why = problems(kit.parse(&with_commands(json!({
                "t": { "description": "d", "run": run }
            }))));
            assert_eq!(why, expected);
        }
        let missing = problems(kit.parse(&with_commands(json!({
            "t": { "description": " " }
        }))));
        assert!(
            missing.contains("commands.t.description: is empty"),
            "{missing}"
        );
        assert!(missing.contains("commands.t.run: is missing"), "{missing}");
        assert_eq!(
            problems(kit.parse(&with_commands(json!(["x"])))),
            "commands: must be an object of commands by name"
        );
        assert_eq!(
            problems(kit.parse(&with_commands(json!({ "t": "gh" })))),
            "commands.t: must be an object"
        );
    }

    #[test]
    fn a_tab_may_only_name_a_command_its_kit_declares() {
        let kit = Folder::new("cmd-perm");
        kit.file("tab.html", "");
        let why = problems(kit.parse(&command_kit(json!(["command:ci-status", "command:deploy"]))));
        assert_eq!(
            why,
            "tabs[0].permissions: \"command:deploy\" names a command this kit does not declare"
        );
        for bad in [
            "command:",
            "command:CI-STATUS",
            "command:ci-status ",
            "command:ci status",
        ] {
            let why = problems(kit.parse(&command_kit(json!([bad]))));
            assert!(why.contains("does not declare"), "{bad}: {why}");
        }
        // No command at all, and a tab naming one.
        let none = with(
            minimal(),
            json!({ "tabs": [{ "id": "t", "title": "T", "entry": "tab.html", "permissions": ["command:ci-status"] }] }),
        );
        assert!(problems(kit.parse(&none)).contains("does not declare"));
        // Stored by a workspace, never asked for by a tab.
        let why = problems(kit.parse(&command_kit(json!(["commands:0123456789abcdef"]))));
        assert!(why.contains("unknown permission"), "{why}");
        // A command that is itself wrong is told about once.
        let why = problems(kit.parse(&with(
            minimal(),
            json!({
                "commands": { "wrong": { "description": "d", "run": "gh" } },
                "tabs": [{ "id": "t", "title": "T", "entry": "tab.html", "permissions": ["command:wrong"] }]
            }),
        )));
        assert_eq!(
            why,
            "commands.wrong.run: must be an argv array, or one per OS: never a shell command line"
        );
    }

    #[test]
    fn a_placeholder_is_a_name_in_braces_and_other_braces_are_text() {
        use ArgPart::{Text, Value as Hole};
        assert_eq!(arg_parts("{run}"), [Hole("run")]);
        assert_eq!(arg_parts("--repo={repo}"), [Text("--repo="), Hole("repo")]);
        assert_eq!(
            arg_parts("{owner}/{repo}.git"),
            [Hole("owner"), Text("/"), Hole("repo"), Text(".git")]
        );
        assert_eq!(arg_parts("plain"), [Text("plain")]);
        assert_eq!(arg_parts(""), []);
        assert_eq!(arg_parts("{{.Names}}"), [Text("{{.Names}}")]);
        assert_eq!(arg_parts("{{json .}}"), [Text("{{json .}}")]);
        assert_eq!(arg_parts("{}"), [Text("{}")]);
        assert_eq!(arg_parts("{"), [Text("{")]);
        assert_eq!(arg_parts("{a"), [Text("{a")]);
        assert_eq!(arg_parts("{{a}}"), [Text("{"), Hole("a"), Text("}")]);
        assert_eq!(arg_parts("{1a}"), [Text("{1a}")]);
        assert_eq!(
            arg_parts("é{a_b-c}é"),
            [Text("é"), Hole("a_b-c"), Text("é")]
        );
    }

    #[test]
    fn a_rule_matches_a_whole_value_whatever_its_author_wrote() {
        let matches =
            |pattern: &str, value: &str| whole_value_rule(pattern).unwrap().is_match(value);
        assert!(matches("[a-z]+", "abc"));
        assert!(!matches("[a-z]+", "abc1"), "unanchored, it would match");
        assert!(!matches("[a-z]+", "1abc"));
        assert!(!matches("[a-z]+", "abc\n"), "not even before a line's end");
        assert!(!matches("^[a-z]+$", "abc\ndef"));
        assert!(!matches("(?m)^[a-z]+$", "abc\ndef"), "(?m) stays inside");
        assert!(!matches("a|b", "ab"), "the alternatives stay inside");
        assert!(!matches("a|b", "xb"));
        assert!(matches("a|ab", "ab"));
        assert!(matches("a|b", "b"));
        assert!(matches("^abc$", "abc"), "anchors of its own do no harm");
    }

    #[test]
    fn a_loopback_permission_names_its_port() {
        assert_eq!(loopback_port("loopback:8765"), Some(8765));
        assert_eq!(loopback_port("loopback:1024"), Some(1024));
        assert_eq!(loopback_port("loopback:65535"), Some(65535));
        assert_eq!(loopback_port("loopback:1023"), None);
        assert_eq!(loopback_port("notify"), None);
    }

    #[test]
    fn paths_that_leave_the_kit_are_refused() {
        let kit = Folder::new("escape");
        kit.file("ok.html", "");
        for entry in [
            "../outside.html",
            "web/../../outside.html",
            "/etc/x.html",
            "\\\\server\\share\\x.html",
            "C:\\x.html",
            "C:x.html",
        ] {
            let manifest = with(
                minimal(),
                json!({ "tabs": [{ "id": "t", "title": "T", "entry": entry, "permissions": [] }] }),
            );
            assert!(
                problems(kit.parse(&manifest)).starts_with("tabs[0].entry"),
                "{entry}"
            );
        }
        assert!(
            problems(kit.parse(&with(minimal(), json!({ "plugin": ".." })))).starts_with("plugin")
        );
        assert!(
            problems(kit.parse(&with(minimal(), json!({ "icon": "missing.svg" }))))
                .contains("not found")
        );
        assert!(
            problems(kit.parse(&with(minimal(), json!({ "icon": "ok.html" }))))
                .contains(".svg or .png")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_kit_is_refused() {
        let kit = Folder::new("symlink");
        let outside = Folder::new("symlink-target");
        outside.file("page.html", "");
        std::os::unix::fs::symlink(&outside.root, kit.root.join("web")).unwrap();
        let manifest = with(
            minimal(),
            json!({ "tabs": [{ "id": "t", "title": "T", "entry": "web/page.html", "permissions": [] }] }),
        );
        assert!(problems(kit.parse(&manifest)).contains("outside the kit folder"));
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_out_of_the_kit_is_refused() {
        let kit = Folder::new("junction");
        let outside = Folder::new("junction-target");
        outside.file("page.html", "");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(kit.root.join("web"))
            .arg(&outside.root)
            .output()
            .unwrap();
        assert!(status.status.success(), "mklink /J failed");
        let manifest = with(
            minimal(),
            json!({ "tabs": [{ "id": "t", "title": "T", "entry": "web/page.html", "permissions": [] }] }),
        );
        assert!(problems(kit.parse(&manifest)).contains("outside the kit folder"));
        let _ = fs::remove_dir(kit.root.join("web"));
    }

    #[test]
    fn setup_commands_are_kept_per_os() {
        let kit = Folder::new("setup");
        let manifest = kit
            .parse(&with(
                minimal(),
                json!({ "setup": [
                    { "id": "unix-only", "title": "Unix", "run": { "macos": "brew install x", "linux": "apt install x" } },
                    { "id": "everywhere", "title": "All", "run": { "windows": "winget install x", "macos": "m", "linux": "l", "freebsd": "ignored" } }
                ] }),
            ))
            .unwrap();
        assert_eq!(manifest.setup[0].run.for_os("windows"), None);
        assert_eq!(manifest.setup[0].run.for_os("linux"), Some("apt install x"));
        assert_eq!(
            manifest.setup[1].run.for_os("windows"),
            Some("winget install x")
        );
        assert_eq!(manifest.setup[1].run.for_os("freebsd"), None);

        let why = problems(kit.parse(&with(
            minimal(),
            json!({ "setup": [
                { "id": "none", "title": "Nothing", "run": { "freebsd": "x" } },
                { "id": "blank", "title": "Blank", "run": { "linux": "  " } },
                { "id": "none", "title": "Twice", "run": { "linux": "x" } },
                { "id": "norun", "title": "No run" }
            ] }),
        )));
        assert!(why.contains("setup[0].run: needs a command"), "{why}");
        assert!(why.contains("setup[1].run.linux: is empty"), "{why}");
        assert!(why.contains("setup[2].id: \"none\" is used twice"), "{why}");
        assert!(why.contains("setup[3].run: is missing"), "{why}");
    }

    #[test]
    fn the_hangar_requirement_is_checked_against_this_version() {
        let kit = Folder::new("compat");
        let manifest = kit
            .parse(&with(minimal(), json!({ "hangar": ">=1.6.0" })))
            .unwrap();
        assert!(compatible(&manifest, "1.6.0").is_ok());
        assert!(compatible(&manifest, "2.0.1").is_ok());
        assert_eq!(
            compatible(&manifest, "1.5.0").unwrap_err(),
            "needs Hangar >=1.6.0, and this is Hangar 1.5.0"
        );
        assert!(compatible(&kit.parse(&minimal()).unwrap(), "0.1.0").is_ok());
    }

    #[test]
    fn paths_are_normalized() {
        assert_eq!(normalize("./a//b/./c.html").unwrap(), "a/b/c.html");
        assert_eq!(normalize("a\\b").unwrap(), "a/b");
        assert_eq!(normalize(".").unwrap(), ".");
        assert_eq!(normalize("./").unwrap(), ".");
        assert!(normalize("").is_err());
        assert!(normalize("a/../b").is_err());
        assert!(normalize("file.txt:stream").is_err());
    }
}
