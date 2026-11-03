//! What a kit is to the app, and what a workspace holds for it: the pure part
//! of `lib/kits.ts`.
//!
//! A kit is a folder with a `hangar-kit.json`, linked from disk or cloned from
//! a GitHub link, that brings a domain into Hangar: a Claude Code plugin every
//! Claude Code pane of a workspace it is enabled on loads, tabs of its own, and
//! setup commands run once per machine (docs/kits.md). The workspace holds
//! which kits it has (`Workspace.kits`); hangar-core holds what they are
//! (`hangar_core::kits`), and its types are used as they are.
//!
//! This file is what every package that reads kits builds on: the permission
//! names and how they are told apart, the defensive readers of the entries a
//! person may have edited by hand in `state.json`, which kits a pane starts
//! with, and the tab protocol's method table. The screens only draw it.

use std::future::Future;

use serde_json::Value;

use hangar_core::kits::{KitInfo, LaunchIssue, Source};
use hangar_i18n::{Key, Translator, args};

use crate::state::types::{Settings, Workspace, WorkspaceKit};

/// What a kit's tab may ask Hangar for, by name. Closed: beside these, only
/// `loopback:<port>` ([`kit_loopback_port`]) and `command:<name>`
/// ([`kit_command_name`]), and any other name is a manifest error.
pub const KIT_PERMISSIONS: [&str; 9] = [
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

/// Below this, a port belongs to the system.
pub const KIT_LOOPBACK_MIN_PORT: u32 = 1024;

/// Whether `port` is one a tab may name: a whole number from 1024 to 65535.
pub fn is_kit_loopback_port(port: u64) -> bool {
    (u64::from(KIT_LOOPBACK_MIN_PORT)..=65535).contains(&port)
}

/// The port of a `loopback:<port>` permission, written plainly (no sign, no
/// leading zero); `None` for anything else. `loopback_port` in
/// kits/manifest.rs holds the same rule.
pub fn kit_loopback_port(name: &str) -> Option<u16> {
    let digits = name.strip_prefix("loopback:")?;
    let mut chars = digits.chars();
    if !matches!(chars.next(), Some('1'..='9')) || !chars.all(|c| c.is_ascii_digit()) {
        return None;
    }
    // A run of digits too long for a number is no port; `parse` says so.
    let port: u64 = digits.parse().ok()?;
    is_kit_loopback_port(port).then_some(port as u16)
}

/// How a declared command is named: `^[a-z][a-z0-9-]*$`. `valid_command_name`
/// in kits/manifest.rs holds the same rule.
pub fn is_kit_command_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The name in a `command:<name>` permission; `None` for anything else.
pub fn kit_command_name(permission: &str) -> Option<&str> {
    let name = permission.strip_prefix("command:")?;
    is_kit_command_name(name).then_some(name)
}

/// Whether `name` is a permission a kit can hold: one of the closed set, a
/// local server's, or a command's.
pub fn is_kit_permission(name: &str) -> bool {
    KIT_PERMISSIONS.contains(&name)
        || kit_loopback_port(name).is_some()
        || kit_command_name(name).is_some()
}

/// `hooks:` and the first 16 hex of the hash of what a plugin runs by itself.
pub fn is_kit_hooks_grant(name: &str) -> bool {
    is_hash_grant(name, "hooks:")
}

/// `commands:` and the first 16 hex of the hash of the commands a kit's tabs
/// may run, for this OS.
pub fn is_kit_commands_grant(name: &str) -> bool {
    is_hash_grant(name, "commands:")
}

fn is_hash_grant(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(|hash| {
        hash.len() == 16
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Whether a tab holding `permissions` reads hidden files, `.env` and its
/// secrets included: it asked for both `files.read` and `files.write` on the
/// same tab, which is what the person sees and accepts at activation.
/// `files.read` alone never does, so no grant given before `files.write`
/// existed reaches further than it did.
pub fn kit_reads_hidden_files<S: AsRef<str>>(permissions: &[S]) -> bool {
    let has = |wanted: &str| permissions.iter().any(|p| p.as_ref() == wanted);
    has("files.read") && has("files.write")
}

/// The placeholder of a declared command that Hangar fills in with the kit's
/// folder.
pub const KIT_DIR_PLACEHOLDER: &str = "{kitDir}";

/// Whether an argv runs something of the kit's own, which the person is shown
/// by name only. The program itself does not count: it is the first item.
pub fn kit_argv_uses_kit_dir<S: AsRef<str>>(argv: &[S]) -> bool {
    argv.iter()
        .skip(1)
        .any(|item| item.as_ref().contains(KIT_DIR_PLACEHOLDER))
}

/// The permissions among `names`, once each: the closed set's order, then the
/// ports, then the commands, in order.
pub fn kit_permissions_in<I, S>(names: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let known: std::collections::BTreeSet<String> = names
        .into_iter()
        .map(|name| name.as_ref().to_owned())
        .collect();
    let mut ports: Vec<u16> = known.iter().filter_map(|n| kit_loopback_port(n)).collect();
    ports.sort_unstable();
    // The set is ordered already, so the names come out sorted.
    let commands = known.iter().filter_map(|n| kit_command_name(n));
    KIT_PERMISSIONS
        .iter()
        .filter(|permission| known.contains(**permission))
        .map(|permission| (*permission).to_owned())
        .chain(ports.into_iter().map(|port| format!("loopback:{port}")))
        .chain(commands.map(|name| format!("command:{name}")))
        .collect()
}

/// The tag a git kit is pinned to, `None` for any other kit. Such a kit moves
/// only when the catalog names another tag (kits/source.rs).
pub fn kit_pinned_tag(source: &Source) -> Option<&str> {
    match source {
        Source::Git(git) => git.git_ref.as_deref()?.strip_prefix("refs/tags/"),
        Source::Folder { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// What a workspace holds for a kit
// ---------------------------------------------------------------------------

/// A kit's id: `^[a-z0-9-]{2,40}$`.
fn is_kit_id(id: &str) -> bool {
    (2..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// What a workspace may hold in `granted`: a permission Hangar knows, or the
/// pseudo permissions of accepted hooks and commands.
fn is_kept_grant(name: &str) -> bool {
    is_kit_permission(name) || is_kit_hooks_grant(name) || is_kit_commands_grant(name)
}

/// Entries cleaned the way every reader below needs them: `state.json` is a
/// file people edit, and an entry that is not a kit id with its grants is no
/// kit. A permission Hangar does not know is never granted; the pseudo
/// permissions of accepted hooks and commands are kept with the others; a kit
/// listed twice counts once, by its first entry.
fn clean_entries<'a>(entries: impl Iterator<Item = (&'a str, Vec<&'a str>)>) -> Vec<WorkspaceKit> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (id, granted) in entries {
        if !is_kit_id(id) || !seen.insert(id) {
            continue;
        }
        let mut kept: Vec<String> = Vec::new();
        for name in granted.into_iter().filter(|name| is_kept_grant(name)) {
            if !kept.iter().any(|have| have == name) {
                kept.push(name.to_owned());
            }
        }
        out.push(WorkspaceKit {
            id: id.to_owned(),
            granted: kept,
        });
    }
    out
}

fn read_kit_entries(kits: Option<&[WorkspaceKit]>) -> Vec<WorkspaceKit> {
    clean_entries(kits.unwrap_or_default().iter().map(|kit| {
        (
            kit.id.as_str(),
            kit.granted.iter().map(String::as_str).collect(),
        )
    }))
}

/// The same reader for entries as `state.json` may hold them before they are
/// typed: anything that is not a list, and any entry that is not
/// `{ id, granted: [..] }`, reads as no kit. The state loader types the file
/// first, so the app itself goes through [`workspace_kits`]; this one is for
/// whoever is handed raw JSON (an agent's call, a file read by hand).
pub fn read_kit_entries_json(kits: &Value) -> Vec<WorkspaceKit> {
    let Some(list) = kits.as_array() else {
        return Vec::new();
    };
    let typed: Vec<(&str, Vec<&str>)> = list
        .iter()
        .filter_map(|entry| {
            let object = entry.as_object()?;
            let id = object.get("id")?.as_str()?;
            let granted = object.get("granted")?.as_array()?;
            Some((id, granted.iter().filter_map(Value::as_str).collect()))
        })
        .collect();
    clean_entries(typed.into_iter())
}

/// The kits enabled on a workspace, read defensively.
pub fn workspace_kits(ws: Option<&Workspace>) -> Vec<WorkspaceKit> {
    read_kit_entries(ws.and_then(|ws| ws.kits.as_deref()))
}

/// The kits enabled on every workspace (`Settings.kitsEverywhere`), read the
/// same way.
pub fn everywhere_kits(settings: Option<&Settings>) -> Vec<WorkspaceKit> {
    read_kit_entries(settings.and_then(|settings| settings.kits_everywhere.as_deref()))
}

/// What decides anything about a workspace's kits: its own entries, then the
/// everywhere ones it does not list itself. A workspace's own entry wins for
/// its grants. [`workspace_kits`] stays the workspace's own list, which is what
/// the workspace's Kits dialog edits.
pub fn effective_kits(ws: Option<&Workspace>, settings: Option<&Settings>) -> Vec<WorkspaceKit> {
    let mut kits = workspace_kits(ws);
    let listed: std::collections::HashSet<String> = kits.iter().map(|kit| kit.id.clone()).collect();
    kits.extend(
        everywhere_kits(settings)
            .into_iter()
            .filter(|kit| !listed.contains(&kit.id)),
    );
    kits
}

/// The kits enabled on a workspace, by id.
pub fn workspace_kit_ids(ws: Option<&Workspace>, settings: Option<&Settings>) -> Vec<String> {
    effective_kits(ws, settings)
        .into_iter()
        .map(|kit| kit.id)
        .collect()
}

/// What a workspace has to hold for what a kit runs by itself to reach its
/// panes; `None` when nothing has to be held. Only a GitHub kit is asked: a
/// linked folder is the person's own, read live, and every edit would ask
/// again. A kit that runs nothing has nothing to accept, and one installed
/// before this was asked keeps working as it did until an update changes what
/// it runs.
pub fn kit_hooks_grant(kit: &KitInfo) -> Option<&str> {
    if !matches!(kit.source, Source::Git(_)) || !kit.hooks_consent {
        return None;
    }
    kit.hooks_permission
        .as_deref()
        .filter(|grant| is_kit_hooks_grant(grant))
}

/// What a workspace has to hold for a kit's tabs to run its commands; `None`
/// when nothing has to be held. As for hooks, only a GitHub kit is asked: a
/// linked folder is the person's own, read live. No kit predates this, as
/// commands are new, so every GitHub kit that has some is asked.
pub fn kit_commands_grant(kit: &KitInfo) -> Option<&str> {
    if !matches!(kit.source, Source::Git(_)) {
        return None;
    }
    kit.commands_permission
        .as_deref()
        .filter(|grant| is_kit_commands_grant(grant))
}

/// The kits a pane starts with and the ones held out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneKits {
    /// What the pane hands `pty_spawn`, and `mcp_scope_write` in `custom`
    /// mode.
    pub ids: Vec<String>,
    /// Enabled, and held out: what they would run by themselves was not
    /// accepted here.
    pub held: Vec<String>,
}

/// The kits a pane of the workspace starts with, `installed` being the kits as
/// they are now. Whatever else is wrong with a kit (not installed, broken)
/// Rust says when the pane starts, so those are passed on.
pub fn pane_kits(
    ws: Option<&Workspace>,
    installed: Option<&[KitInfo]>,
    settings: Option<&Settings>,
) -> PaneKits {
    let mut out = PaneKits::default();
    for WorkspaceKit { id, granted } in effective_kits(ws, settings) {
        let kit = installed.and_then(|list| list.iter().find(|entry| entry.id == id));
        let held = kit
            .and_then(kit_hooks_grant)
            .is_some_and(|grant| !granted.iter().any(|g| g == grant));
        if held {
            out.held.push(id);
        } else {
            out.ids.push(id);
        }
    }
    out
}

/// [`pane_kits`] from the kits on disk right now, `list` being how they are
/// read (`hangar_core::kits::kits_list` in the app). A workspace without kits
/// reads nothing. When they cannot be read at all, the ids go on: Rust reads
/// the same index for `pty_spawn` and keeps every kit out, saying why.
pub async fn load_pane_kits_with<F, Fut>(
    ws: Option<&Workspace>,
    settings: Option<&Settings>,
    list: F,
) -> PaneKits
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<KitInfo>, String>>,
{
    if effective_kits(ws, settings).is_empty() {
        return PaneKits::default();
    }
    let installed = list().await.ok();
    pane_kits(ws, installed.as_deref(), settings)
}

/// [`load_pane_kits_with`] on the real index.
pub async fn load_pane_kits(ws: Option<&Workspace>, settings: Option<&Settings>) -> PaneKits {
    load_pane_kits_with(ws, settings, hangar_core::kits::kits_list).await
}

/// Why a kit stays out of a pane: `pty_spawn`'s `kit_issues`, and `held` from
/// [`pane_kits`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitLaunchIssue {
    pub id: String,
    pub reason: LaunchReason,
    /// In English, from Rust, for `Broken`.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchReason {
    Missing,
    Broken,
    /// The machine's Claude Code policy forbids sideloaded plugin folders.
    Forbidden,
    /// What it runs by itself was not accepted on this workspace.
    Hooks,
}

impl From<&LaunchIssue> for KitLaunchIssue {
    fn from(issue: &LaunchIssue) -> Self {
        use hangar_core::kits::IssueReason;
        KitLaunchIssue {
            id: issue.id.clone(),
            reason: match issue.reason {
                IssueReason::Missing => LaunchReason::Missing,
                IssueReason::Broken => LaunchReason::Broken,
                IssueReason::Forbidden => LaunchReason::Forbidden,
            },
            detail: issue.detail.clone(),
        }
    }
}

/// The lines a Claude Code pane prints before its agent starts, one per kit
/// left out, and one for every kit a sideloading policy kept out.
pub fn kit_issue_notes(issues: &[KitLaunchIssue], t: Translator) -> Vec<String> {
    let mut notes = Vec::new();
    let mut forbidden: Vec<&str> = Vec::new();
    for issue in issues {
        let id = issue.id.as_str();
        match issue.reason {
            LaunchReason::Forbidden => forbidden.push(id),
            LaunchReason::Missing => notes.push(
                t.t_with(Key::KitsWarnMissing, &args!("id" => id))
                    .into_owned(),
            ),
            LaunchReason::Hooks => notes.push(
                t.t_with(Key::KitsManageWarnHooks, &args!("id" => id))
                    .into_owned(),
            ),
            LaunchReason::Broken => notes.push(
                t.t_with(
                    Key::KitsWarnBroken,
                    &args!("id" => id, "detail" => issue.detail.as_deref().unwrap_or("")),
                )
                .into_owned(),
            ),
        }
    }
    if !forbidden.is_empty() {
        notes.push(
            t.t_with(
                Key::KitsWarnForbidden,
                &args!("ids" => forbidden.join(", ").as_str()),
            )
            .into_owned(),
        );
    }
    notes
}

// ---------------------------------------------------------------------------
// The tab message protocol (docs/kits.md)
// ---------------------------------------------------------------------------
//
// A kit's tab talks to Hangar by messages and the host answers. The table and
// the limits are here, with the model, because the permission names are; the
// host that answers (P16, `kit_tabs`) builds on them.

/// How [`KIT_METHODS`] writes the permission of a call that names its port.
pub const KIT_LOOPBACK_PATTERN: &str = "loopback:<port>";

/// How [`KIT_METHODS`] writes the permission of a call that names its command.
pub const KIT_COMMAND_PATTERN: &str = "command:<name>";

/// The method a tab calls, and the permission it needs.
pub const KIT_METHODS: [(&str, &str); 14] = [
    ("workspace.info", "workspace.read"),
    ("panes.list", "panes.read"),
    ("panes.send", "panes.send"),
    ("files.read", "files.read"),
    ("files.readBinary", "files.read"),
    ("files.list", "files.read"),
    ("files.write", "files.write"),
    ("files.writeBinary", "files.write"),
    ("data.read", "data.read"),
    ("data.write", "data.write"),
    ("loopback.fetch", KIT_LOOPBACK_PATTERN),
    ("command.run", KIT_COMMAND_PATTERN),
    ("preview.show", "preview.show"),
    ("notify", "notify"),
];

/// The events a tab can listen to, and the permission each needs.
pub const KIT_EVENTS: [(&str, &str); 1] = [("panes.changed", "panes.read")];

/// The most `files.read` returns, in bytes.
pub const KIT_FILE_MAX_BYTES: usize = 1024 * 1024;
/// The most `files.readBinary` reads and `files.writeBinary` takes, in bytes
/// (kits/files.rs).
pub const KIT_BINARY_MAX_BYTES: usize = 10 * 1024 * 1024;
/// The most files `files.list` gives (kits/files.rs).
pub const KIT_LIST_MAX_FILES: usize = 2000;
/// The longest glob `files.list` takes (kits/files.rs).
pub const KIT_LIST_PATTERN_MAX_CHARS: usize = 256;
/// The most `data.read` returns and `data.write` takes, in bytes.
pub const KIT_DATA_MAX_BYTES: usize = 1024 * 1024;
/// The most a `loopback.fetch` answer may hold, in bytes (kits/loopback.rs).
pub const KIT_LOOPBACK_MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// The most a `loopback.fetch` may send, in bytes (kits/loopback.rs).
pub const KIT_LOOPBACK_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// How long a `loopback.fetch` waits unless it says otherwise.
pub const KIT_LOOPBACK_TIMEOUT_MS: u64 = 5_000;
pub const KIT_LOOPBACK_MAX_TIMEOUT_MS: u64 = 15_000;

/// The error code of a call the tab has no permission for.
pub const KIT_DENIED: &str = "denied";

/// Whether a tab granted `granted` may call `method`; unknown methods never.
/// `loopback.fetch` needs the permission of the very port its `params` name,
/// and `command.run` that of the very command.
pub fn kit_may_call<S: AsRef<str>>(method: &str, granted: &[S], params: Option<&Value>) -> bool {
    let Some((_, permission)) = KIT_METHODS.iter().find(|(name, _)| *name == method) else {
        return false;
    };
    let has = |wanted: &str| granted.iter().any(|g| g.as_ref() == wanted);
    match *permission {
        KIT_LOOPBACK_PATTERN => {
            // A port is a number: the text "8765" is not one.
            let port = params
                .and_then(|p| p.get("port"))
                .and_then(Value::as_u64)
                .filter(|port| is_kit_loopback_port(*port));
            port.is_some_and(|port| has(&format!("loopback:{port}")))
        }
        KIT_COMMAND_PATTERN => {
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(Value::as_str)
                .filter(|name| is_kit_command_name(name));
            name.is_some_and(|name| has(&format!("command:{name}")))
        }
        permission => has(permission),
    }
}

#[cfg(test)]
mod tests;
