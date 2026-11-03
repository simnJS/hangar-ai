//! The model: the persisted state (`types.rs`), the window's transient state
//! and the packages' read-models (`live.rs`), and the one door through which
//! the persisted state changes (`store.rs`).
//!
//! `Model` is what every package's `update` and view is handed. Its persisted
//! state is private: a package reads it with `state()` and writes it with
//! `apply(StoreAction)` (PACKAGES.md, rule 3), so a save and a snapshot never
//! miss a change.

mod lenient;
pub mod live;
pub mod persist;
pub mod snapshot;
pub mod store;
pub mod types;

// The logic modules of `src/lib/*` the store is built on; P0b ports them with
// their tests.
pub mod folders;
pub mod layout;
pub mod pane_names;
pub mod roles;

use std::sync::Arc;

pub use lenient::app_state as read_state;
// What packages name most, at `crate::state::*`; some have no user until the
// packages land.
#[allow(unused_imports)]
pub use live::{
    AgentActivity, BridgeEvent, GlyphState, Live, PaneActivity, PaneInsight, PlanLimits, Ui,
};
pub use snapshot::Snapshot;
#[allow(unused_imports)]
pub use store::{
    CommandScope, FolderPatch, PanePatch, Patch, SettingsPatch, StoreAction, StoreOutcome,
    WorkspaceDraft, WorkspacePatch,
};
#[allow(unused_imports)]
pub use types::{AgentId, AppState, Pane, PaneId, Settings, SplitDir, SplitNode, Workspace, WsId};

/// Everything the window knows.
#[derive(Debug, Default)]
pub struct Model {
    /// `state.json`'s content. An `Arc` so a snapshot shares it until the
    /// next write (`Arc::make_mut` in store.rs).
    state: Arc<AppState>,
    /// Bumped by every store action that changed `state`: what the save and
    /// the snapshot follow.
    revision: u64,
    pub ui: Ui,
    pub live: Live,
}

impl Model {
    /// The persisted state, to read.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// The persisted state, shared (for a snapshot or a save off the UI
    /// thread).
    pub fn state_arc(&self) -> Arc<AppState> {
        self.state.clone()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn settings(&self) -> &Settings {
        &self.state.settings
    }

    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.state.workspaces.iter().find(|ws| ws.id == id)
    }

    /// The workspace the main window shows, if it still exists.
    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.state
            .active_workspace_id
            .as_deref()
            .and_then(|id| self.workspace(id))
    }

    /// The theme the chrome is drawn in: the active workspace's own, else the
    /// settings' (inventory §1.11, "Theme application").
    pub fn theme(&self) -> &'static hangar_theme::Theme {
        let id = self
            .active_workspace()
            .and_then(|ws| ws.theme_id.as_deref())
            .unwrap_or(&self.state.settings.theme_id);
        hangar_theme::by_id_or_default(id)
    }

    /// The interface language: the settings' choice, else the system's.
    pub fn locale(&self) -> hangar_i18n::Locale {
        self.state
            .settings
            .locale
            .unwrap_or_else(hangar_i18n::Locale::system)
    }
}
