//! What enabling a kit on a workspace means, as pure functions: `lib/kitActivation.ts`.
//!
//! Which permissions a kit's tabs ask for, which of them the workspace has
//! already accepted, what a toggle writes into `Workspace.kits`, and which
//! panes need a restart to see a kit. The screens (`views/`) only draw it.
//!
//! What a GitHub kit's plugin runs by itself goes through the same diff, as
//! the pseudo permission `hooks:<hash>` ([`kit_hooks_grant`]): its hooks and
//! all else it starts, changed, are something new to accept, and the kit stays
//! out of the workspace's new panes until they are. Its declared commands do
//! too, as `commands:<hash>` ([`kit_commands_grant`]): changed, its tabs run
//! none of them until they are accepted again.

use hangar_core::kits::{Hook, Inspection, KitInfo, RunKind, Running, Source, Tab};
use hangar_i18n::{Key, Translator, args};

use super::model::{
    is_kit_commands_grant, is_kit_hooks_grant, kit_command_name, kit_commands_grant,
    kit_hooks_grant, kit_loopback_port, kit_permissions_in,
};
use crate::state::types::{AgentId, PaneId, Workspace, WorkspaceKit};

/// Every permission has its sentence in the dictionary, in plain words.
pub const PERMISSION_LABEL: [(&str, Key); 9] = [
    ("workspace.read", Key::KitsManagePermWorkspaceRead),
    ("panes.read", Key::KitsManagePermPanesRead),
    ("panes.send", Key::KitsManagePermPanesSend),
    ("files.read", Key::KitsManagePermFilesRead),
    ("files.write", Key::KitsTabPermFilesWrite),
    ("data.read", Key::KitsManagePermDataRead),
    ("data.write", Key::KitsManagePermDataWrite),
    ("preview.show", Key::KitsManagePermPreviewShow),
    ("notify", Key::KitsManagePermNotify),
];

/// A permission in plain words, a local server's by its port, a command's by
/// its name. A name that is none of these is shown as it is: nothing is hidden
/// from the person.
pub fn permission_label(t: Translator, permission: &str) -> String {
    if let Some(name) = kit_command_name(permission) {
        return t
            .t_with(Key::KitsTabPermCommand, &args!("name" => name))
            .into_owned();
    }
    if let Some(port) = kit_loopback_port(permission) {
        // As text: a number is written the way the language does, and a port
        // has no thousands.
        return t
            .t_with(
                Key::KitsManagePermLoopback,
                &args!("port" => port.to_string().as_str()),
            )
            .into_owned();
    }
    match PERMISSION_LABEL
        .iter()
        .find(|(name, _)| *name == permission)
    {
        Some((_, key)) => t.t(*key).into_owned(),
        None => permission.to_owned(),
    }
}

/// What the tabs of a kit ask for altogether, once each, in the closed set's
/// order, then the ports, then the commands.
pub fn requested_permissions(tabs: &[Tab]) -> Vec<String> {
    kit_permissions_in(tabs.iter().flat_map(|tab| tab.permissions.iter()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionDiff {
    /// Asked for and already accepted.
    pub kept: Vec<String>,
    /// Asked for and not accepted: a new kit, or an update that asks for more
    /// or changes its hooks.
    pub added: Vec<String>,
}

/// Splits what a kit asks for by what the workspace accepted. A permission the
/// kit no longer asks for is simply dropped: nothing is lost by it.
pub fn permission_diff<R: AsRef<str>, G: AsRef<str>>(
    requested: &[R],
    granted: &[G],
) -> PermissionDiff {
    let mut diff = PermissionDiff::default();
    for permission in requested {
        let permission = permission.as_ref();
        let accepted = granted.iter().any(|g| g.as_ref() == permission);
        if accepted {
            diff.kept.push(permission.to_owned());
        } else {
            diff.added.push(permission.to_owned());
        }
    }
    diff
}

/// Whether what a kit asks for waits for its hooks to be accepted on this
/// workspace.
pub fn hooks_to_accept(diff: &PermissionDiff) -> bool {
    diff.added.iter().any(|name| is_kit_hooks_grant(name))
}

/// Whether what a kit asks for waits for its commands to be accepted on this
/// workspace.
pub fn commands_to_accept(diff: &PermissionDiff) -> bool {
    diff.added.iter().any(|name| is_kit_commands_grant(name))
}

/// Whether a kit's commands are all it waits for: its plugin still reaches the
/// panes then.
pub fn only_commands_to_accept(diff: &PermissionDiff) -> bool {
    !diff.added.is_empty() && diff.added.iter().all(|name| is_kit_commands_grant(name))
}

// ---------------------------------------------------------------------------
// Text that must not lie
// ---------------------------------------------------------------------------

/// Characters that do not show, or turn the text around them: a line could
/// read as another. The control characters and the format characters
/// (`\p{Cc}`, `\p{Cf}`), and the line and paragraph separators, which the web
/// app let through and a person reading a command should not have to guess at.
pub fn is_unseen(c: char) -> bool {
    c.is_control()
        || matches!(c as u32,
            0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x08E2 | 0x180E
            | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
            | 0xFEFF | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

fn with_unseen_written(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if is_unseen(c) {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// An argv on one line, as it is. An item holding a space, a quote or nothing
/// at all is quoted, so `["gh", "run view"]` does not read as
/// `["gh", "run", "view"]`, and what would not show is written out.
pub fn kit_argv_line<S: AsRef<str>>(argv: &[S]) -> String {
    argv.iter()
        .map(|item| {
            let item = item.as_ref();
            let quote = item.is_empty()
                || item
                    .chars()
                    .any(|c| c.is_whitespace() || c == '"' || c == '\'')
                || item.chars().any(is_unseen);
            if quote {
                // JSON's quoting is the one JavaScript's `JSON.stringify` has.
                with_unseen_written(&serde_json::to_string(item).unwrap_or_default())
            } else {
                item.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A rule as the person is shown it: as written, with what would not show
/// written out.
pub fn kit_rule_text(pattern: &str) -> String {
    with_unseen_written(pattern)
}

// ---------------------------------------------------------------------------
// What a plugin runs by itself
// ---------------------------------------------------------------------------

/// What the activation view groups: `kit_inspect`'s kinds, and a hooks file's
/// `modules`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunGroupKind {
    Module,
    Mcp,
    Monitor,
    Lsp,
    SkillHook,
    Settings,
    Grammar,
    Bin,
    Binary,
    Dependency,
    Unknown,
}

impl From<RunKind> for RunGroupKind {
    fn from(kind: RunKind) -> Self {
        match kind {
            RunKind::Mcp => RunGroupKind::Mcp,
            RunKind::Monitor => RunGroupKind::Monitor,
            RunKind::Lsp => RunGroupKind::Lsp,
            RunKind::SkillHook => RunGroupKind::SkillHook,
            RunKind::Settings => RunGroupKind::Settings,
            RunKind::Grammar => RunGroupKind::Grammar,
            RunKind::Bin => RunGroupKind::Bin,
            RunKind::Binary => RunGroupKind::Binary,
            RunKind::Dependency => RunGroupKind::Dependency,
            RunKind::Unknown => RunGroupKind::Unknown,
        }
    }
}

/// The order of the groups: what starts on its own first, what Hangar cannot
/// make out last.
pub const RUNNING_GROUP_ORDER: [RunGroupKind; 11] = [
    RunGroupKind::Module,
    RunGroupKind::Mcp,
    RunGroupKind::Monitor,
    RunGroupKind::Lsp,
    RunGroupKind::SkillHook,
    RunGroupKind::Settings,
    RunGroupKind::Grammar,
    RunGroupKind::Bin,
    RunGroupKind::Binary,
    RunGroupKind::Dependency,
    RunGroupKind::Unknown,
];

impl RunGroupKind {
    /// The title of the group, and the one sentence saying when it runs.
    pub fn label(self) -> (Key, Key) {
        match self {
            RunGroupKind::Module => (Key::KitsManageRunModule, Key::KitsManageRunModuleWhen),
            RunGroupKind::Mcp => (Key::KitsManageRunMcp, Key::KitsManageRunMcpWhen),
            RunGroupKind::Monitor => (Key::KitsManageRunMonitor, Key::KitsManageRunMonitorWhen),
            RunGroupKind::Lsp => (Key::KitsManageRunLsp, Key::KitsManageRunLspWhen),
            RunGroupKind::SkillHook => {
                (Key::KitsManageRunSkillHook, Key::KitsManageRunSkillHookWhen)
            }
            RunGroupKind::Settings => (Key::KitsManageRunSettings, Key::KitsManageRunSettingsWhen),
            RunGroupKind::Grammar => (Key::KitsManageRunGrammar, Key::KitsManageRunGrammarWhen),
            RunGroupKind::Bin => (Key::KitsManageRunBin, Key::KitsManageRunBinWhen),
            RunGroupKind::Binary => (Key::KitsManageRunBinary, Key::KitsManageRunBinaryWhen),
            RunGroupKind::Dependency => (
                Key::KitsManageRunDependency,
                Key::KitsManageRunDependencyWhen,
            ),
            RunGroupKind::Unknown => (Key::KitsManageRunUnknown, Key::KitsManageRunUnknownWhen),
        }
    }
}

/// Something a plugin starts by itself, or may, exactly as it writes it. Env
/// values and headers never reach here: only their names. (`Running` of
/// hangar-core, without its kind and its private hash.)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunItem {
    pub name: String,
    /// The program, then its arguments.
    pub argv: Vec<String>,
    /// A shell command, an address, a path: what `argv` is not.
    pub text: Option<String>,
    /// When it says so itself: a monitor's `when`, a language server's file
    /// types.
    pub when: Option<String>,
    pub description: Option<String>,
    /// The names of the variables it sets.
    pub env: Vec<String>,
    /// The names of the headers it sends.
    pub headers: Vec<String>,
    /// What it runs is out of Hangar's reach: a bundle, a broken file, remote
    /// code, another plugin.
    pub unreadable: bool,
}

impl From<&Running> for RunItem {
    fn from(item: &Running) -> Self {
        RunItem {
            name: item.name.clone(),
            argv: item.argv.clone(),
            text: item.text.clone(),
            when: item.when.clone(),
            description: item.description.clone(),
            env: item.env.clone(),
            headers: item.headers.clone(),
            unreadable: item.unreadable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunGroup {
    pub kind: RunGroupKind,
    pub items: Vec<RunItem>,
}

/// A hooks file's `modules` entry is listed among its hooks, under this event
/// (kits/hooks.rs).
const MODULES_EVENT: &str = "modules";

/// The hooks proper: a hooks module is code of its own, and has its own group.
pub fn kit_plain_hooks(inspection: Option<&Inspection>) -> Vec<&Hook> {
    inspection
        .map(|found| found.hooks.iter())
        .into_iter()
        .flatten()
        .filter(|hook| hook.event != MODULES_EVENT)
        .collect()
}

/// [`kit_running_groups`] from the pieces: a hooks file's hooks, and what else
/// runs, each with its kind.
pub fn running_groups_of(
    hooks: &[Hook],
    running: impl IntoIterator<Item = (RunGroupKind, RunItem)>,
) -> Vec<RunGroup> {
    let mut by_kind: Vec<(RunGroupKind, Vec<RunItem>)> = Vec::new();
    let mut push = |kind: RunGroupKind, item: RunItem| match by_kind
        .iter_mut()
        .find(|(have, _)| *have == kind)
    {
        Some((_, items)) => items.push(item),
        None => by_kind.push((kind, vec![item])),
    };
    for hook in hooks.iter().filter(|hook| hook.event == MODULES_EVENT) {
        push(
            RunGroupKind::Module,
            RunItem {
                name: hook.event.clone(),
                text: Some(hook.command.clone()),
                ..RunItem::default()
            },
        );
    }
    for (kind, item) in running {
        push(kind, item);
    }
    RUNNING_GROUP_ORDER
        .iter()
        .filter_map(|kind| {
            let at = by_kind.iter().position(|(have, _)| have == kind)?;
            let (_, items) = by_kind.swap_remove(at);
            Some(RunGroup { kind: *kind, items })
        })
        .collect()
}

/// What a plugin starts by itself beside its hooks, by kind, in the view's
/// order. Nothing is left out: a kind that is not one of the known ones would
/// be listed as not understood.
pub fn kit_running_groups(inspection: Option<&Inspection>) -> Vec<RunGroup> {
    let Some(found) = inspection else {
        return Vec::new();
    };
    running_groups_of(
        &found.hooks,
        found
            .running
            .iter()
            .map(|item| (RunGroupKind::from(item.kind), RunItem::from(item))),
    )
}

/// What an item starts, for its block: `None` when all Hangar can give is its
/// name. A line break stays one: what a skill declares takes several lines.
pub fn kit_running_line(item: &RunItem) -> Option<String> {
    if !item.argv.is_empty() {
        return Some(kit_argv_line(&item.argv));
    }
    item.text
        .as_deref()
        .filter(|text| !text.is_empty())
        .map(|text| {
            text.split('\n')
                .map(kit_rule_text)
                .collect::<Vec<_>>()
                .join("\n")
        })
}

// ---------------------------------------------------------------------------
// Where a kit stands on a workspace
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KitState {
    /// Not enabled on the workspace.
    Off,
    /// Enabled, with every permission its tabs ask for, and its hooks,
    /// accepted.
    On,
    /// Enabled, but an update asks for permissions not accepted yet, or
    /// changed its hooks.
    Review,
    /// Enabled, but the kit cannot be used now (broken, gone).
    Broken,
}

impl KitState {
    /// The tag that says it.
    pub fn key(self) -> Key {
        match self {
            KitState::On => Key::KitsManageStatusOn,
            KitState::Off => Key::KitsManageStatusOff,
            KitState::Review => Key::KitsManageStatusReview,
            KitState::Broken => Key::KitsManageStatusBroken,
        }
    }
}

/// Where a kit stands on a workspace; `requested` is `None` for a kit that
/// cannot be used.
pub fn kit_state(entry: Option<&WorkspaceKit>, requested: Option<&[String]>) -> KitState {
    let Some(entry) = entry else {
        return KitState::Off;
    };
    let Some(requested) = requested else {
        return KitState::Broken;
    };
    if permission_diff(requested, &entry.granted).added.is_empty() {
        KitState::On
    } else {
        KitState::Review
    }
}

/// What a workspace has to have accepted of a kit: the permissions its tabs
/// ask for, from its manifest, and its hooks and commands when they are asked
/// about. `None` when the kit cannot be used.
pub fn kit_requested(kit: &KitInfo) -> Option<Vec<String>> {
    let manifest = kit.manifest.as_ref().filter(|_| kit.error.is_none())?;
    let mut requested = requested_permissions(&manifest.tabs);
    requested.extend(kit_hooks_grant(kit).map(str::to_owned));
    requested.extend(kit_commands_grant(kit).map(str::to_owned));
    Some(requested)
}

/// What accepting a kit stores, from what the person was shown: its hashes are
/// the inspection's, not the kit's as it may be now, so hooks or commands that
/// changed while the view was open stay to be accepted. A linked folder's
/// store nothing.
pub fn accepted_grants(source: &Source, inspection: Option<&Inspection>) -> Vec<String> {
    let mut granted = requested_permissions(inspection.map_or(&[][..], |found| &found.tabs));
    if matches!(source, Source::Git(_))
        && let Some(found) = inspection
    {
        granted.extend(
            found
                .hooks_permission
                .iter()
                .filter(|grant| is_kit_hooks_grant(grant))
                .cloned(),
        );
        granted.extend(
            found
                .commands_permission
                .iter()
                .filter(|grant| is_kit_commands_grant(grant))
                .cloned(),
        );
    }
    granted
}

/// The workspace's kits with `id` enabled, accepting exactly `granted`.
pub fn enable_kit(kits: &[WorkspaceKit], id: &str, granted: &[String]) -> Vec<WorkspaceKit> {
    let entry = WorkspaceKit {
        id: id.to_owned(),
        granted: granted.to_vec(),
    };
    if kits.iter().any(|kit| kit.id == id) {
        kits.iter()
            .map(|kit| {
                if kit.id == id {
                    entry.clone()
                } else {
                    kit.clone()
                }
            })
            .collect()
    } else {
        kits.iter().cloned().chain(Some(entry)).collect()
    }
}

/// The workspace's kits without `id`.
pub fn disable_kit(kits: &[WorkspaceKit], id: &str) -> Vec<WorkspaceKit> {
    kits.iter().filter(|kit| kit.id != id).cloned().collect()
}

/// The panes that carry kits: Claude Code's. Other agents and plain shells
/// never load a kit's plugin, so they have nothing to restart for.
pub fn kit_panes(ws: &Workspace) -> impl Iterator<Item = &crate::state::types::Pane> {
    ws.panes.iter().filter(|pane| pane.agent == AgentId::Claude)
}

/// The panes a kit change still needs a restart for: those running now keep
/// the kits they started with. The respawn keeps each pane's `sessionId`, so
/// the conversation resumes.
pub fn panes_to_restart(ws: &Workspace) -> Vec<PaneId> {
    kit_panes(ws).map(|pane| pane.id.clone()).collect()
}

/// Whether two lists hold the same kits with the same grants.
#[cfg(test)]
pub fn same_kits(a: &[WorkspaceKit], b: &[WorkspaceKit]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let sorted = |kit: &WorkspaceKit| {
        let mut granted = kit.granted.clone();
        granted.sort();
        granted
    };
    a.iter().all(|kit| {
        b.iter()
            .find(|other| other.id == kit.id)
            .is_some_and(|other| sorted(other) == sorted(kit))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Folder,
    Git,
}

/// The source of a kit in one line, for a list: the folder's path or the
/// GitHub link.
pub fn source_line(source: &Source) -> (SourceKind, String) {
    match source {
        Source::Folder { path } => (SourceKind::Folder, path.clone()),
        Source::Git(git) => {
            let place: Vec<&str> = [git.git_ref.as_deref(), git.subdir.as_deref()]
                .into_iter()
                .flatten()
                .filter(|part| !part.is_empty())
                .collect();
            let text = if place.is_empty() {
                git.url.clone()
            } else {
                format!("{} ({})", git.url, place.join(" · "))
            };
            (SourceKind::Git, text)
        }
    }
}

/// A kit's name for a list: its manifest's, or its id when the manifest cannot
/// be read.
pub fn kit_name(kit: &KitInfo) -> &str {
    kit.manifest
        .as_ref()
        .map_or(kit.id.as_str(), |manifest| manifest.name.as_str())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOutcome {
    pub from: String,
    pub to: String,
    /// Permissions the new version asks for that the old one did not, or its
    /// changed hooks.
    pub new_permissions: Vec<String>,
}

/// What an update changed, for the message after it.
pub fn update_outcome(before: &KitInfo, after: &KitInfo) -> UpdateOutcome {
    let was = kit_requested(before).unwrap_or_default();
    let now = kit_requested(after).unwrap_or_default();
    UpdateOutcome {
        from: before.installed_version.clone(),
        to: after.installed_version.clone(),
        new_permissions: now
            .into_iter()
            .filter(|permission| !was.contains(permission))
            .collect(),
    }
}

#[cfg(test)]
mod tests;
