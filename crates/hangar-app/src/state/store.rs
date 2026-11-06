//! The only way the persisted model changes: `Model::apply(StoreAction)`,
//! one action per store action of `src/store.tsx` (inventory §1.3).
//!
//! `apply` is synchronous and answers the ids an action makes (the new pane's,
//! the new workspace's), as the TS functions returned them. It runs against
//! the state as the last action left it, which is what the TS `update` gave
//! its callers through a ref: two actions in a row each see the other's
//! change. A change bumps the model's revision; the shell then republishes
//! the snapshot and schedules the debounced save (persist.rs), so no package
//! has to remember either.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use super::Model;
use super::folders::place_before;
use super::layout::{
    MAX_PANES, arranged_tree, grid_order, grid_tree, insert_leaf, leaf, move_leaf,
    preset_terminals, preset_tree, reading_order, remove_leaf, rename_leaf,
};
use super::pane_names::{all_pane_names, pane_names, pick_pane_name, unique_pane_names};
use super::types::{
    AgentId, AppState, DropZone, Pane, PaneId, PaneRole, SavedCommand, Settings, SplitDir,
    SplitNode, ViewPane, Workspace, WorkspaceFolder, WorkspaceKit, WsId,
};
use super::unread::{Kept, Unread};

/// A shallow merge, as the TS spreads were (`{...pane, ...patch}`): a
/// function that sets the fields it names. Cloned and printed so it can ride
/// in a `Message`.
///
/// ```ignore
/// StoreAction::UpdateSettings(Patch::new(|s: &mut Settings| s.navigator_open = false))
/// ```
pub struct Patch<T>(Arc<dyn Fn(&mut T) + Send + Sync>);

impl<T> Patch<T> {
    pub fn new(edit: impl Fn(&mut T) + Send + Sync + 'static) -> Self {
        Patch(Arc::new(edit))
    }

    /// The patch that changes nothing (`respawnPane` without a patch).
    pub fn none() -> Self {
        Patch(Arc::new(|_| {}))
    }

    pub fn apply(&self, target: &mut T) {
        (self.0)(target)
    }
}

impl<T> Clone for Patch<T> {
    fn clone(&self) -> Self {
        Patch(self.0.clone())
    }
}

impl<T> fmt::Debug for Patch<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Patch(..)")
    }
}

pub type PanePatch = Patch<Pane>;
pub type WorkspacePatch = Patch<Workspace>;
pub type SettingsPatch = Patch<Settings>;
pub type FolderPatch = Patch<WorkspaceFolder>;
pub type SavedCommandPatch = Patch<SavedCommand>;

/// The layouts offered when a workspace is made (`LayoutSize`): its number
/// of panes.
pub type LayoutSize = u8;

/// What creating a workspace takes (`WorkspaceDraft` of store.tsx).
#[derive(Debug, Clone, Default)]
pub struct WorkspaceDraft {
    pub name: String,
    pub cwd: String,
    /// Empty unless it came from a multi-root `.code-workspace`.
    pub extra_roots: Vec<String>,
    pub layout: LayoutSize,
    /// One agent per pane, in grid order.
    pub agents: Vec<AgentId>,
    pub shell_id: Option<String>,
    pub folder_id: Option<String>,
    /// Written only when set (absent rather than empty).
    pub kits: Option<Vec<WorkspaceKit>>,
    /// Written only when set.
    pub claude_account: Option<String>,
}

/// Where a saved command is kept (`CommandScope`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandScope {
    /// The workspace's own list, listed first.
    Workspace,
    /// `settings.savedCommands`, offered by every workspace.
    Global,
}

/// A saved command as the dialog hands it over: the id is the store's.
#[derive(Debug, Clone, Default)]
pub struct SavedCommandDraft {
    pub label: String,
    pub command: String,
    pub auto_run: bool,
    pub broadcast: bool,
}

/// Every write to the persisted model (store.tsx, inventory §1.3).
#[derive(Debug, Clone)]
pub enum StoreAction {
    /// Makes it the active workspace; answers its id.
    AddWorkspace(WorkspaceDraft),
    /// If it was active, the first remaining one becomes active.
    RemoveWorkspace(WsId),
    UpdateWorkspace(WsId, WorkspacePatch),
    SetActiveWorkspace(Option<WsId>),
    /// Answers the folder's id.
    AddFolder(String),
    /// The folder's id stays what it was, whatever the patch sets.
    UpdateFolder(String, FolderPatch),
    /// Its workspaces go back to the top level.
    RemoveFolder(String),
    MoveFolder {
        id: String,
        before: Option<String>,
    },
    /// `placeBefore`; a folder deleted meanwhile means the top level.
    MoveWorkspace {
        id: WsId,
        folder: Option<String>,
        before: Option<WsId>,
    },
    /// Keeps panes in reading order; new ones copy the focused pane's agent.
    /// The count is of terminals: every view pane stays, laid out in the
    /// even grid with them, and the terminals have the room `MAX_PANES`
    /// leaves (`layout::preset_terminals`).
    ApplyPreset {
        ws: WsId,
        count: usize,
        model_pane: Option<PaneId>,
    },
    /// `presetTree(leafIds(normalizeTree))`: same ids, nothing restarts.
    TidyLayout(WsId),
    /// The layout after an agent opened or closed a pane
    /// (`layout::arranged_tree`): the orchestrator, else the manager, on the
    /// left and the biggest, the others in an even grid beside it, in the
    /// order they are read. Same ids, nothing restarts.
    ArrangePanes(WsId),
    /// Divider drags.
    SetTree(WsId, SplitNode),
    /// Answers the new pane's id, or none at `MAX_PANES`. `init` is applied
    /// to the new pane before anything spawns it (a worker's role, task and
    /// port base, which its shell must start with: inventory A6); it cannot
    /// change the pane's id.
    AddPane {
        ws: WsId,
        agent: Option<AgentId>,
        near: Option<PaneId>,
        dir: Option<SplitDir>,
        cwd: Option<String>,
        name: Option<String>,
        init: Option<PanePatch>,
    },
    /// The last pane stays.
    ClosePane {
        ws: WsId,
        pane: PaneId,
    },
    /// A view pane (what a preview shows) for the preview entry `id`, which
    /// is its id: beside `near` (else the last pane) on the side `dir` says,
    /// as `AddPane` splits. Answers the id, or none at `MAX_PANES` or when
    /// the workspace has a pane of that id already.
    AddView {
        ws: WsId,
        id: PaneId,
        near: Option<PaneId>,
        dir: Option<SplitDir>,
    },
    /// A view pane goes, its sibling taking its room. Unlike a terminal, the
    /// last one may: the terminals are what a workspace keeps.
    CloseView {
        ws: WsId,
        id: PaneId,
    },
    /// `moveLeaf`; `Center` swaps. Terminals and views alike.
    MovePane {
        ws: WsId,
        drag: PaneId,
        target: PaneId,
        zone: DropZone,
    },
    UpdatePane {
        ws: WsId,
        pane: PaneId,
        patch: PanePatch,
    },
    /// A new pane id, which kills the PTY and spawns it again: restart,
    /// agent change, session change. Answers the new id, or none when the
    /// pane is no longer there. The shell then sends
    /// `Intent::PaneRespawned`, whoever applied it.
    RespawnPane {
        ws: WsId,
        pane: PaneId,
        patch: PanePatch,
    },
    UpdateSettings(SettingsPatch),
    /// `updateSettings(DEFAULT_SETTINGS)`: `projectsFolder`, `kitsEverywhere`,
    /// `voiceApiKey` and the keys this version does not know survive.
    ResetSettings,
    /// Answers the command's id.
    AddSavedCommand {
        scope: CommandScope,
        ws: WsId,
        draft: SavedCommandDraft,
    },
    /// The command's id stays what it was, whatever the patch sets.
    UpdateSavedCommand {
        scope: CommandScope,
        ws: WsId,
        id: String,
        patch: SavedCommandPatch,
    },
    RemoveSavedCommand {
        scope: CommandScope,
        ws: WsId,
        id: String,
    },
    /// Keeps the id, lands last in the other list.
    MoveSavedCommand {
        from: CommandScope,
        to: CommandScope,
        ws: WsId,
        id: String,
    },
    /// The workspace shown as the chat view (`on`) or as its grid. Off is
    /// written as the field's absence. Nothing restarts: the terminals stay
    /// mounted either way, only what draws them changes.
    SetChatView {
        ws: WsId,
        on: bool,
    },
}

/// The ids an action made or found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreOutcome {
    pub pane: Option<PaneId>,
    pub workspace: Option<WsId>,
    pub folder: Option<String>,
    /// A saved command's id (`addSavedCommand`). Not in §8.2, which forgot it.
    pub command: Option<String>,
    /// Whether the persisted state changed.
    pub changed: bool,
}

/// A pane given a new id by `RespawnPane`, for `Intent::PaneRespawned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Respawn {
    pub ws: WsId,
    pub from: PaneId,
    pub to: PaneId,
    /// The respawn kept the session the pane had: a restart rather than a
    /// fresh conversation (`patch.sessionId !== null` in App.tsx's
    /// `carryResume` call).
    pub keeps_session: bool,
}

/// A workspace `RemoveWorkspace` took away: what cleaning up after it needs
/// (`hangar_core::delivery::remove_workspace_check`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub ws: WsId,
    /// Its folder, the repository its check worktree was made from.
    pub cwd: String,
}

/// A fresh id, as `crypto.randomUUID()` made them.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A pane running `agent`, named against `taken`, the names in use in every
/// workspace (`makePane`).
pub fn make_pane<S: AsRef<str>>(agent: AgentId, taken: impl IntoIterator<Item = S>) -> Pane {
    Pane {
        id: new_id(),
        name: pick_pane_name(taken),
        agent,
        ..Pane::default()
    }
}

/// `readAccountChoice` (lib/accounts.ts): a chosen account's id, trimmed, or
/// none for the default.
fn account_choice(choice: Option<&str>) -> Option<String> {
    choice
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// A new workspace from the dialog's draft (`makeWorkspace`). `taken` holds
/// the pane names the other workspaces use. `kits` and `claudeAccount` are
/// written only when set, as in every workspace saved before they existed.
pub fn make_workspace<S: AsRef<str>>(
    draft: &WorkspaceDraft,
    taken: impl IntoIterator<Item = S>,
) -> Workspace {
    let others: Vec<String> = taken.into_iter().map(|s| s.as_ref().to_string()).collect();
    let mut panes: Vec<Pane> = Vec::new();
    for i in 0..usize::from(draft.layout) {
        let agent = draft.agents.get(i).copied().unwrap_or(AgentId::Shell);
        let names = others.iter().cloned().chain(pane_names(&panes));
        panes.push(make_pane(agent, names));
    }
    let ids: Vec<PaneId> = panes.iter().map(|pane| pane.id.clone()).collect();
    Workspace {
        id: new_id(),
        name: draft.name.clone(),
        cwd: draft.cwd.clone(),
        extra_roots: draft.extra_roots.clone(),
        panes,
        theme_id: None,
        shell_id: draft.shell_id.clone(),
        tree: Some(preset_tree(&ids)),
        saved_commands: Vec::new(),
        folder_id: draft.folder_id.clone(),
        kits: draft.kits.clone().filter(|kits| !kits.is_empty()),
        claude_account: account_choice(draft.claude_account.as_deref()),
        ..Workspace::default()
    }
}

impl Model {
    /// Applies one store action.
    ///
    /// A change is told by `reduce` itself: every arm writes the state
    /// (`state_mut`) only when what it writes differs from what is there, so
    /// no copy of the state is kept to compare with. The state is shared
    /// with the last published snapshot, if any: the first write after it
    /// copies it (`Arc::make_mut`), later ones write in place.
    pub fn apply(&mut self, action: StoreAction) -> StoreOutcome {
        // Tests hold `reduce` to its word: a write is a change, and no change
        // goes unwritten. A deep copy, not an `Arc` clone, so the state's
        // sharing is the same as in a build.
        #[cfg(test)]
        let before = (*self.state).clone();
        self.wrote = false;
        let mut outcome = reduce(self, action);
        outcome.changed = self.wrote;
        #[cfg(test)]
        assert_eq!(
            outcome.changed,
            before != *self.state,
            "a store action wrote the state without changing it, or changed it unwritten"
        );
        if outcome.changed {
            self.revision += 1;
        }
        outcome
    }

    /// Takes in what `load_state` read (inventory §1.4 rule 3, §1.6 steps
    /// 5–6), with the fix-ups of store.tsx's `load`. Not a store action:
    /// hydrating is not a change to save.
    pub fn hydrate(&mut self, state: AppState) {
        self.hydrate_read(state, Vec::new());
    }

    /// `hydrate`, with the values the reader could not read (`lenient::read`),
    /// to be written back on every save until the app writes those fields.
    pub fn hydrate_read(&mut self, mut state: AppState, kept: Vec<Kept>) {
        fix_up(&mut state);
        // The active one is on screen at once. A detached one joins when its
        // window opens again (`Model::window_opened`), so its panes start
        // there and not before.
        if let Some(active) = &state.active_workspace_id {
            self.mount(active);
        }
        self.unread = if kept.is_empty() {
            Unread::default()
        } else {
            Unread::settle(kept, &serde_json::to_value(&state).unwrap_or_default())
        };
        self.state = Arc::new(state);
        self.live.hydrated = true;
    }

    /// The respawns since the last call, oldest first (the shell turns each
    /// into an `Intent::PaneRespawned`).
    pub(crate) fn take_respawns(&mut self) -> Vec<Respawn> {
        std::mem::take(&mut self.respawned)
    }

    /// The workspaces removed since the last call, oldest first (the shell
    /// removes their delivery check worktree off the UI thread).
    pub(crate) fn take_removed(&mut self) -> Vec<Removed> {
        std::mem::take(&mut self.removed)
    }

    /// The persisted state for writing. Private to `state`: packages go
    /// through `apply`. Only for a write that changes something: calling it
    /// is what makes the action a change.
    fn state_mut(&mut self) -> &mut AppState {
        self.wrote = true;
        Arc::make_mut(&mut self.state)
    }
}

/// store.tsx's `load` and `hydrateWorkspace`: missing lists are empty (serde's
/// defaults), a folder id naming no folder is the top level, pane names are
/// made unique across every workspace, and the settings are read over
/// `DEFAULT_SETTINGS` (serde's default too).
fn fix_up(state: &mut AppState) {
    let folders: HashSet<String> = state
        .folders
        .iter()
        .map(|folder| folder.id.clone())
        .collect();
    for ws in &mut state.workspaces {
        if ws
            .folder_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || !folders.contains(id))
        {
            ws.folder_id = None;
        }
    }
    unique_pane_names(&mut state.workspaces);
}

/// The pane names every workspace but `ws` uses.
fn names_besides(state: &AppState, ws: &str) -> Vec<String> {
    all_pane_names(state.workspaces.iter().filter(|entry| entry.id != ws))
}

/// Runs `edit` on workspace `id` as the last write left it, if it is there,
/// and writes the result only when it differs.
fn map_workspace<R>(
    model: &mut Model,
    id: &str,
    edit: impl FnOnce(&Workspace) -> Option<(Workspace, R)>,
) -> Option<R> {
    let at = model.state.workspaces.iter().position(|ws| ws.id == id)?;
    let (next, answer) = edit(&model.state.workspaces[at])?;
    if next != model.state.workspaces[at] {
        model.state_mut().workspaces[at] = next;
    }
    Some(answer)
}

/// Rewrites one of the two saved-command lists, whichever `scope` names
/// (`mapCommands`).
fn map_commands(
    model: &mut Model,
    scope: CommandScope,
    ws: &str,
    edit: impl FnOnce(&mut Vec<SavedCommand>),
) {
    match scope {
        CommandScope::Global => {
            let mut commands = model.state.settings.saved_commands.clone();
            edit(&mut commands);
            if commands != model.state.settings.saved_commands {
                model.state_mut().settings.saved_commands = commands;
            }
        }
        CommandScope::Workspace => {
            map_workspace(model, ws, |current| {
                let mut next = current.clone();
                edit(&mut next.saved_commands);
                Some((next, ()))
            });
        }
    }
}

fn reduce(model: &mut Model, action: StoreAction) -> StoreOutcome {
    let mut outcome = StoreOutcome::default();
    match action {
        StoreAction::AddWorkspace(draft) => {
            let ws = make_workspace(&draft, all_pane_names(&model.state.workspaces));
            outcome.workspace = Some(ws.id.clone());
            let state = model.state_mut();
            state.active_workspace_id = Some(ws.id.clone());
            state.workspaces.push(ws);
        }

        StoreAction::RemoveWorkspace(id) => {
            if let Some(ws) = model.state.workspaces.iter().find(|ws| ws.id == id) {
                model.removed.push(Removed {
                    ws: ws.id.clone(),
                    cwd: ws.cwd.clone(),
                });
                let state = model.state_mut();
                state.workspaces.retain(|ws| ws.id != id);
                if state.active_workspace_id.as_deref() == Some(id.as_str()) {
                    state.active_workspace_id = state.workspaces.first().map(|ws| ws.id.clone());
                }
            }
        }

        StoreAction::UpdateWorkspace(id, patch) => {
            map_workspace(model, &id, |ws| {
                let mut next = ws.clone();
                patch.apply(&mut next);
                Some((next, ()))
            });
        }

        StoreAction::SetActiveWorkspace(id) => {
            if model.state.active_workspace_id != id {
                model.state_mut().active_workspace_id = id;
            }
        }

        StoreAction::AddFolder(name) => {
            let folder = WorkspaceFolder {
                id: new_id(),
                name,
                collapsed: false,
                ..WorkspaceFolder::default()
            };
            outcome.folder = Some(folder.id.clone());
            model.state_mut().folders.push(folder);
        }

        StoreAction::UpdateFolder(id, patch) => {
            if let Some(at) = model
                .state
                .folders
                .iter()
                .position(|folder| folder.id == id)
            {
                let mut next = model.state.folders[at].clone();
                patch.apply(&mut next);
                next.id = id;
                if next != model.state.folders[at] {
                    model.state_mut().folders[at] = next;
                }
            }
        }

        StoreAction::RemoveFolder(id) => {
            let filed = model
                .state
                .workspaces
                .iter()
                .any(|ws| ws.folder_id.as_deref() == Some(id.as_str()));
            if filed || model.state.folders.iter().any(|folder| folder.id == id) {
                let state = model.state_mut();
                state.folders.retain(|folder| folder.id != id);
                for ws in &mut state.workspaces {
                    if ws.folder_id.as_deref() == Some(id.as_str()) {
                        ws.folder_id = None;
                    }
                }
            }
        }

        StoreAction::MoveFolder { id, before } => {
            let folder = model
                .state
                .folders
                .iter()
                .find(|folder| folder.id == id)
                .cloned();
            if let Some(folder) = folder
                && before.as_deref() != Some(id.as_str())
            {
                let mut folders = model.state.folders.clone();
                place_before(&mut folders, folder, before.as_deref());
                if folders != model.state.folders {
                    model.state_mut().folders = folders;
                }
            }
        }

        StoreAction::MoveWorkspace { id, folder, before } => {
            let ws = model
                .state
                .workspaces
                .iter()
                .find(|ws| ws.id == id)
                .cloned();
            if let Some(mut ws) = ws
                && before.as_deref() != Some(id.as_str())
            {
                // A folder deleted while the workspace was on its way leaves it
                // at the top level rather than filed under nothing.
                ws.folder_id = folder
                    .filter(|folder| model.state.folders.iter().any(|entry| &entry.id == folder));
                let mut workspaces = model.state.workspaces.clone();
                place_before(&mut workspaces, ws, before.as_deref());
                if workspaces != model.state.workspaces {
                    model.state_mut().workspaces = workspaces;
                }
            }
        }

        StoreAction::ApplyPreset {
            ws,
            count,
            model_pane,
        } => {
            let others = names_besides(&model.state, &ws);
            map_workspace(model, &ws, |current| {
                let size = preset_terminals(count, current.view_panes.len());
                // Reading order, not creation order: the panes seen first are
                // those a smaller preset keeps, and those the toolbar names
                // before closing the others (`closedByPreset`).
                let arranged: Vec<Pane> = reading_order(&current.panes, current.tree.as_ref())
                    .into_iter()
                    .cloned()
                    .collect();
                // Same panes, same ids: the survivors keep their shell and are
                // only resized.
                let mut panes: Vec<Pane> = arranged.iter().take(size).cloned().collect();
                // The pane the person was in leads; failing that, the last one
                // kept, so a grid grown from one agent is that agent throughout.
                let agent = arranged
                    .iter()
                    .find(|pane| Some(&pane.id) == model_pane.as_ref())
                    .or(panes.last())
                    .map_or(AgentId::Shell, |pane| pane.agent);
                while panes.len() < size {
                    let names: Vec<String> =
                        others.iter().cloned().chain(pane_names(&panes)).collect();
                    panes.push(make_pane(agent, names));
                }
                // The views keep their place in the reading order among the
                // terminals that stay; the new terminals come last.
                let kept: HashSet<&str> = panes.iter().map(|pane| pane.id.as_str()).collect();
                let mut ids: Vec<PaneId> = grid_order(current)
                    .into_iter()
                    .filter(|id| kept.contains(id.as_str()) || current.has_view(id))
                    .collect();
                for pane in &panes {
                    if !ids.contains(&pane.id) {
                        ids.push(pane.id.clone());
                    }
                }
                let mut next = current.clone();
                next.panes = panes;
                next.tree = Some(preset_tree(&ids));
                Some((next, ()))
            });
        }

        StoreAction::TidyLayout(ws) => {
            map_workspace(model, &ws, |current| {
                let mut next = current.clone();
                next.tree = Some(preset_tree(&grid_order(current)));
                Some((next, ()))
            });
        }

        StoreAction::ArrangePanes(ws) => {
            map_workspace(model, &ws, |current| {
                // The views take their place in the grid beside the lead, as
                // the terminals do.
                let order = grid_order(current);
                let lead = [PaneRole::Orchestrator, PaneRole::Manager]
                    .into_iter()
                    .find_map(|role| {
                        current
                            .panes
                            .iter()
                            .find(|pane| pane.role == Some(role))
                            .map(|pane| pane.id.as_str())
                    });
                let mut next = current.clone();
                next.tree = Some(arranged_tree(&order, lead));
                Some((next, ()))
            });
        }

        StoreAction::SetTree(ws, tree) => {
            map_workspace(model, &ws, |current| {
                let mut next = current.clone();
                next.tree = Some(tree);
                Some((next, ()))
            });
        }

        StoreAction::AddPane {
            ws,
            agent,
            near,
            dir,
            cwd,
            name,
            init,
        } => {
            let others = names_besides(&model.state, &ws);
            let zone = match dir.unwrap_or(SplitDir::Row) {
                SplitDir::Row => DropZone::Right,
                SplitDir::Col => DropZone::Bottom,
            };
            outcome.pane = map_workspace(model, &ws, |current| {
                if current.pane_count() >= MAX_PANES {
                    return None;
                }
                let tree = grid_tree(current);
                let ids = grid_order(current);
                let near = near
                    .filter(|near| ids.contains(near))
                    .or_else(|| ids.last().cloned())
                    .unwrap_or_default();
                // Splitting a Claude Code pane asks for another one, not for a
                // bare prompt; an agent the caller names still wins.
                let inherited = current
                    .panes
                    .iter()
                    .find(|pane| pane.id == near)
                    .map(|pane| pane.agent);
                let mut taken = others.clone();
                taken.extend(pane_names(&current.panes));
                let wanted = name.as_deref().map(str::trim).unwrap_or("");
                let mut pane = Pane {
                    id: new_id(),
                    name: if !wanted.is_empty() && !taken.iter().any(|t| t == wanted) {
                        wanted.to_string()
                    } else {
                        pick_pane_name(&taken)
                    },
                    agent: agent.or(inherited).unwrap_or(AgentId::Shell),
                    // Left alone by default: a pane follows the workspace root.
                    cwd,
                    ..Pane::default()
                };
                let id = pane.id.clone();
                if let Some(init) = &init {
                    init.apply(&mut pane);
                    pane.id = id.clone();
                }
                let mut next = current.clone();
                next.panes.push(pane);
                next.tree = Some(if current.pane_count() == 0 {
                    leaf(id.clone())
                } else {
                    insert_leaf(&tree, &near, &id, zone)
                });
                Some((next, id))
            });
        }

        StoreAction::AddView { ws, id, near, dir } => {
            let zone = match dir.unwrap_or(SplitDir::Row) {
                SplitDir::Row => DropZone::Right,
                SplitDir::Col => DropZone::Bottom,
            };
            outcome.pane = map_workspace(model, &ws, |current| {
                if id.is_empty() || current.pane_count() >= MAX_PANES || current.has_leaf(&id) {
                    return None;
                }
                let tree = grid_tree(current);
                let ids = grid_order(current);
                let near = near
                    .filter(|near| ids.contains(near))
                    .or_else(|| ids.last().cloned())
                    .unwrap_or_default();
                let mut next = current.clone();
                next.view_panes.push(ViewPane::new(id.clone()));
                next.tree = Some(if current.pane_count() == 0 {
                    leaf(id.clone())
                } else {
                    insert_leaf(&tree, &near, &id, zone)
                });
                Some((next, id.clone()))
            });
        }

        StoreAction::CloseView { ws, id } => {
            map_workspace(model, &ws, |current| {
                if !current.has_view(&id) {
                    return None;
                }
                let mut next = current.clone();
                next.view_panes.retain(|view| view.id != id);
                let tree = remove_leaf(&grid_tree(current), &id);
                next.tree = Some(tree.unwrap_or_else(|| preset_tree(&next.leaf_ids())));
                Some((next, ()))
            });
        }

        StoreAction::ClosePane { ws, pane } => {
            map_workspace(model, &ws, |current| {
                // The last pane stays: an empty workspace has nothing to act on.
                if current.panes.len() <= 1 || !current.panes.iter().any(|p| p.id == pane) {
                    return None;
                }
                let mut next = current.clone();
                next.panes.retain(|p| p.id != pane);
                let tree = remove_leaf(&grid_tree(current), &pane);
                next.tree = Some(tree.unwrap_or_else(|| preset_tree(&next.leaf_ids())));
                Some((next, ()))
            });
        }

        StoreAction::MovePane {
            ws,
            drag,
            target,
            zone,
        } => {
            if drag != target {
                map_workspace(model, &ws, |current| {
                    let mut next = current.clone();
                    next.tree = Some(move_leaf(&grid_tree(current), &drag, &target, zone));
                    Some((next, ()))
                });
            }
        }

        StoreAction::UpdatePane { ws, pane, patch } => {
            map_workspace(model, &ws, |current| {
                let at = current.panes.iter().position(|p| p.id == pane)?;
                let mut next = current.clone();
                patch.apply(&mut next.panes[at]);
                Some((next, ()))
            });
        }

        StoreAction::RespawnPane { ws, pane, patch } => {
            let to = new_id();
            let respawn = map_workspace(model, &ws, |current| {
                // The pane may have been closed, or respawned already, which
                // changed its id, since the caller last looked.
                let at = current.panes.iter().position(|p| p.id == pane)?;
                let mut next = current.clone();
                let had_session = current.panes[at].session_id.is_some();
                patch.apply(&mut next.panes[at]);
                next.panes[at].id = to.clone();
                let keeps_session = !(had_session && next.panes[at].session_id.is_none());
                // The arrangement is keyed by pane id: follow the rename.
                next.tree = Some(rename_leaf(&grid_tree(current), &pane, &to));

                Some((next, keeps_session))
            });
            if let Some(keeps_session) = respawn {
                model.unread.rename_pane(&ws, &pane, &to);
                model.respawned.push(Respawn {
                    ws,
                    from: pane,
                    to: to.clone(),
                    keeps_session,
                });
                outcome.pane = Some(to);
            }
        }

        StoreAction::UpdateSettings(patch) => {
            let mut next = model.state.settings.clone();
            patch.apply(&mut next);
            if next != model.state.settings {
                model.state_mut().settings = next;
            }
        }

        StoreAction::ResetSettings => {
            let current = &model.state.settings;
            // What `{...current, ...DEFAULT_SETTINGS}` keeps: the fields the
            // defaults do not name.
            let next = Settings {
                voice_api_key: current.voice_api_key.clone(),
                kits_everywhere: current.kits_everywhere.clone(),
                projects_folder: current.projects_folder.clone(),
                // The machines are not a preference: a reset of the
                // preferences does not forget them.
                nodes: current.nodes.clone(),
                extra: current.extra.clone(),
                ..Settings::default()
            };
            if next != *current {
                model.state_mut().settings = next;
            }
        }

        StoreAction::AddSavedCommand { scope, ws, draft } => {
            let command = SavedCommand {
                id: new_id(),
                label: draft.label,
                command: draft.command,
                auto_run: draft.auto_run,
                broadcast: draft.broadcast,
                ..SavedCommand::default()
            };
            outcome.command = Some(command.id.clone());
            map_commands(model, scope, &ws, |commands| commands.push(command));
        }

        StoreAction::UpdateSavedCommand {
            scope,
            ws,
            id,
            patch,
        } => {
            map_commands(model, scope, &ws, |commands| {
                for command in commands.iter_mut().filter(|command| command.id == id) {
                    patch.apply(command);
                    command.id = id.clone();
                }
            });
        }

        StoreAction::RemoveSavedCommand { scope, ws, id } => {
            map_commands(model, scope, &ws, |commands| {
                commands.retain(|command| command.id != id)
            });
        }

        StoreAction::MoveSavedCommand { from, to, ws, id } => {
            move_saved_command(model, from, to, &ws, &id);
        }

        StoreAction::SetChatView { ws, on } => {
            map_workspace(model, &ws, |workspace| {
                let mut next = workspace.clone();
                next.chat_view = on.then_some(true);
                Some((next, ()))
            });
        }
    }
    outcome
}

/// One write rather than a remove and an add: both lists change against the
/// same state, so the command is never in both and never in neither
/// (`moveSavedCommand`).
fn move_saved_command(model: &mut Model, from: CommandScope, to: CommandScope, ws: &str, id: &str) {
    if from == to {
        return;
    }
    let workspace = model
        .state
        .workspaces
        .iter()
        .position(|entry| entry.id == ws);
    // A workspace closed since the menu opened has nowhere to take it.
    if to == CommandScope::Workspace && workspace.is_none() {
        return;
    }
    let source = match from {
        CommandScope::Global => &model.state.settings.saved_commands,
        CommandScope::Workspace => match workspace {
            Some(at) => &model.state.workspaces[at].saved_commands,
            None => return,
        },
    };
    let Some(moved) = source.iter().find(|command| command.id == id).cloned() else {
        return;
    };
    let state = model.state_mut();
    let drop = |commands: &mut Vec<SavedCommand>| commands.retain(|command| command.id != id);
    drop(&mut state.settings.saved_commands);
    if to == CommandScope::Global {
        state.settings.saved_commands.push(moved.clone());
    }
    if let Some(at) = workspace {
        let commands = &mut state.workspaces[at].saved_commands;
        drop(commands);
        if to == CommandScope::Workspace {
            commands.push(moved);
        }
    }
}

#[cfg(test)]
mod tests;
