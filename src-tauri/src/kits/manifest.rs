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

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

pub const FILE: &str = "hangar-kit.json";

/// The only schema this Hangar reads.
pub const SCHEMA: u64 = 1;

/// What a kit's tab may ask Hangar for, accepted by the person when the kit
/// is enabled on a workspace. Closed: beside these, only `loopback:<port>`
/// (`loopback_port`), and any other name is a manifest error.
pub const PERMISSIONS: &[&str] = &[
    "workspace.read",
    "panes.read",
    "panes.send",
    "files.read",
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub permissions: Vec<String>,
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
            if let (Some(title), Some(entry), Some(permissions)) = (title, entry, permissions) {
                tabs.push(Tab {
                    id,
                    title,
                    entry,
                    icon,
                    permissions,
                });
            }
        }
        tabs
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
        let why = problems(kit.parse(&tab(json!(["panes.read", "files.write"]))));
        assert_eq!(
            why,
            "tabs[0].permissions: unknown permission \"files.write\""
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
