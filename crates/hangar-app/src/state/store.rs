//! The only way the persisted model changes: `Model::apply(StoreAction)`,
//! one action per store action of `src/store.tsx` (inventory §1.3).
//!
//! `apply` is synchronous and answers the ids an action makes (the new pane's,
//! the new workspace's), as the TS functions returned them. A change bumps the
//! model's revision; the shell then republishes the snapshot and schedules the
//! debounced save (persist.rs), so no package has to remember either.
//!
//! TODO(P0b): the semantics of each action, with `store.test.ts` ported. Until
//! then every action is accepted and changes nothing, so the skeleton never
//! writes `state.json`.

use std::fmt;
use std::sync::Arc;

use super::Model;
use super::types::{
    AgentId, AppState, DropZone, Pane, PaneId, SavedCommand, Settings, SplitDir, SplitNode,
    Workspace, WorkspaceFolder, WorkspaceKit, WsId,
};

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

/// The layouts offered when a workspace is made (`LayoutSize`).
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
    ApplyPreset {
        ws: WsId,
        count: usize,
        model_pane: Option<PaneId>,
    },
    /// `presetTree(leafIds(normalizeTree))`: same ids, nothing restarts.
    TidyLayout(WsId),
    /// Divider drags.
    SetTree(WsId, SplitNode),
    /// Answers the new pane's id, or none at `MAX_PANES`. `init` is applied
    /// to the new pane before anything spawns it (a worker's role, task and
    /// port base, which its shell must start with: inventory A6).
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
    /// `moveLeaf`; `Center` swaps.
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
    /// agent change, session change. Answers the new id.
    RespawnPane {
        ws: WsId,
        pane: PaneId,
        patch: PanePatch,
    },
    UpdateSettings(SettingsPatch),
    /// `updateSettings(DEFAULT_SETTINGS)`: `projectsFolder`, `kitsEverywhere`
    /// and `voiceApiKey` survive.
    ResetSettings,
    /// Answers the command's id.
    AddSavedCommand {
        scope: CommandScope,
        ws: WsId,
        draft: SavedCommandDraft,
    },
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

impl Model {
    /// Applies one store action.
    pub fn apply(&mut self, action: StoreAction) -> StoreOutcome {
        let outcome = reduce(self, action);
        if outcome.changed {
            self.revision += 1;
        }
        outcome
    }

    /// Takes in what `load_state` read (inventory §1.4 rule 3, §1.6 steps
    /// 5–6). Not a store action: hydrating is not a change to save.
    pub fn hydrate(&mut self, state: AppState) {
        // TODO(P0b): `uniquePaneNames`, folder ids naming no folder → null.
        if let Some(active) = state.active_workspace_id.clone()
            && !self.ui.opened_ids.contains(&active)
        {
            self.ui.opened_ids.push(active);
        }
        self.state = Arc::new(state);
        self.live.hydrated = true;
    }

    /// The persisted state for writing. Private to `state`: packages go
    /// through `apply`.
    fn state_mut(&mut self) -> &mut AppState {
        Arc::make_mut(&mut self.state)
    }
}

/// TODO(P0b): every action, from store.tsx and its tests.
fn reduce(model: &mut Model, action: StoreAction) -> StoreOutcome {
    let _ = model;
    match action {
        StoreAction::AddWorkspace(_)
        | StoreAction::RemoveWorkspace(_)
        | StoreAction::UpdateWorkspace(..)
        | StoreAction::SetActiveWorkspace(_)
        | StoreAction::AddFolder(_)
        | StoreAction::UpdateFolder(..)
        | StoreAction::RemoveFolder(_)
        | StoreAction::MoveFolder { .. }
        | StoreAction::MoveWorkspace { .. }
        | StoreAction::ApplyPreset { .. }
        | StoreAction::TidyLayout(_)
        | StoreAction::SetTree(..)
        | StoreAction::AddPane { .. }
        | StoreAction::ClosePane { .. }
        | StoreAction::MovePane { .. }
        | StoreAction::UpdatePane { .. }
        | StoreAction::RespawnPane { .. }
        | StoreAction::UpdateSettings(_)
        | StoreAction::ResetSettings
        | StoreAction::AddSavedCommand { .. }
        | StoreAction::UpdateSavedCommand { .. }
        | StoreAction::RemoveSavedCommand { .. }
        | StoreAction::MoveSavedCommand { .. } => StoreOutcome::default(),
    }
}
