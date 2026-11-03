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
pub mod unread;

// The logic modules of `src/lib/*` the store is built on, with their tests.
pub mod folders;
pub mod layout;
pub mod pane_names;
pub mod roles;

#[cfg(test)]
mod roundtrip_tests;

use std::sync::Arc;

use iced::window;

#[allow(unused_imports)]
pub use lenient::{app_state as read_state, read as read_state_keeping, read_checked};

/// What the boot read from `state.json`: the state, and the values it held
/// that no field could read (`hydrate_read`).
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub state: AppState,
    pub kept: Vec<unread::Kept>,
}
// What packages name most, at `crate::state::*`; some have no user until the
// packages land.
#[allow(unused_imports)]
pub use live::{
    AgentActivity, BridgeEvent, GlyphState, Live, PaneActivity, PaneInsight, PanelDrag, PlanLimits,
    PreviewPanel, Signals, Ui, WindowUi,
};
pub use snapshot::Snapshot;
#[allow(unused_imports)]
pub use store::{
    CommandScope, FolderPatch, PanePatch, Patch, Respawn, SavedCommandDraft, SettingsPatch,
    StoreAction, StoreOutcome, WorkspaceDraft, WorkspacePatch,
};
#[allow(unused_imports)]
pub use types::{AgentId, AppState, Pane, PaneId, Settings, SplitDir, SplitNode, Workspace, WsId};

use crate::shell::{Drawer, View, WindowKind};

/// Everything the window knows.
#[derive(Debug, Default)]
pub struct Model {
    /// `state.json`'s content. An `Arc` so a snapshot shares it until the
    /// next write (`Arc::make_mut` in store.rs).
    state: Arc<AppState>,
    /// Bumped by every store action that changed `state`: what the save and
    /// the snapshot follow.
    revision: u64,
    /// What `state.json` held that no field could read, written back on
    /// every save (unread.rs).
    unread: unread::Unread,
    /// Respawns not yet announced (`Intent::PaneRespawned`).
    respawned: Vec<Respawn>,
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

    /// The JSON `save_state` writes: the state, with what `state.json` held
    /// that this version could not read put back where it was.
    pub fn to_json(&self) -> serde_json::Value {
        persist::to_json(&self.state, &self.unread)
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

    /// The theme window `w` is drawn in: its workspace's own, else the
    /// settings'. The main window showing a workspace drawn elsewhere takes
    /// the settings' (App.tsx: `activeWorkspace` is null there).
    pub fn theme_in(&self, w: window::Id) -> &'static hangar_theme::Theme {
        let id = self
            .window_workspace(w)
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

    /// The main window's id, once it is open.
    pub fn main_window(&self) -> Option<window::Id> {
        self.live
            .windows
            .iter()
            .find(|(_, kind)| **kind == WindowKind::Main)
            .map(|(id, _)| *id)
    }

    /// The window a workspace is drawn in: its own when it is detached and
    /// that window is open, else the main one.
    pub fn window_of(&self, ws: &str) -> Option<window::Id> {
        self.live
            .windows
            .iter()
            .find(|(_, kind)| matches!(kind, WindowKind::Workspace(id) if id == ws))
            .map(|(id, _)| *id)
            .or_else(|| self.main_window())
    }

    /// The workspace window `w` draws (App.tsx's `activeWorkspace`): a
    /// workspace window's own; the main window's active one, unless that one
    /// is shown in a window of its own.
    pub fn window_workspace(&self, w: window::Id) -> Option<&Workspace> {
        match self.live.windows.get(&w) {
            Some(WindowKind::Workspace(id)) => self.workspace(id),
            Some(WindowKind::Main) | None => self
                .active_workspace()
                .filter(|ws| ws.detached != Some(true)),
        }
    }

    /// Whether `w` is the window the person is in: where commands run and
    /// whose transient state is `ui.view`, `ui.drawer` and `ui.focus`.
    pub fn is_focused_window(&self, w: window::Id) -> bool {
        match self.ui.focused_window {
            Some(focused) => focused == w,
            None => self.main_window() == Some(w),
        }
    }

    /// The window the person is in (the main one until another takes the
    /// focus).
    pub fn focused_window(&self) -> Option<window::Id> {
        self.ui.focused_window.or_else(|| self.main_window())
    }

    /// What the main area of window `w` shows. Each window has its own: the
    /// focused one's is `ui.view`, the others' wait in `ui.parked`.
    pub fn view_in(&self, w: window::Id) -> View {
        if self.is_focused_window(w) {
            self.ui.view
        } else {
            self.ui.parked.get(&w).map(|p| p.view).unwrap_or_default()
        }
    }

    /// The overlays of window `w`, bottom first: the focused one's are
    /// `ui.overlays`.
    pub fn overlays_in(&self, w: window::Id) -> &[crate::shell::Overlay] {
        if self.is_focused_window(w) {
            &self.ui.overlays
        } else {
            self.ui.parked.get(&w).map_or(&[], |p| &p.overlays)
        }
    }

    /// The side panel open as a drawer in window `w`.
    pub fn drawer_in(&self, w: window::Id) -> Option<Drawer> {
        if self.is_focused_window(w) {
            self.ui.drawer
        } else {
            self.ui.parked.get(&w).and_then(|p| p.drawer)
        }
    }

    /// The focused pane of workspace `ws`, when it is one of its panes.
    pub fn focused_pane<'a>(&self, ws: &'a Workspace) -> Option<&'a Pane> {
        let id = self.ui.focus_by_workspace.get(&ws.id)?;
        ws.panes.iter().find(|pane| &pane.id == id)
    }

    /// What window `w`'s broadcast field holds. Each window has its own, as
    /// each Tauri window's App had its own `broadcast`: the focused one's is
    /// `ui.broadcast`, the others' wait in `ui.parked`.
    pub fn broadcast_in(&self, w: window::Id) -> &str {
        if self.is_focused_window(w) {
            &self.ui.broadcast
        } else {
            self.ui.parked.get(&w).map_or("", |p| p.broadcast.as_str())
        }
    }

    /// Workspace `ws` is drawn from now on: it joins `ui.opened_ids`, so its
    /// panes start and stay alive while it is hidden. The main window mounts
    /// a workspace when it is first shown there (App.tsx's `openedIds`); a
    /// workspace window mounts its own as it opens, as its own App did with
    /// the one workspace it was opened for, whether the detach happened now
    /// or before the app last quit.
    pub fn mount(&mut self, ws: &str) {
        if !self.ui.opened_ids.iter().any(|id| id == ws) {
            self.ui.opened_ids.push(ws.to_string());
        }
    }

    /// Window `id` now draws `kind`; a workspace window mounts its workspace.
    pub fn window_opened(&mut self, id: window::Id, kind: WindowKind) {
        if let WindowKind::Workspace(ws) = &kind {
            let ws = ws.clone();
            self.mount(&ws);
        }
        self.live.windows.insert(id, kind);
    }

    /// The workspaces that were in a window of their own when the app last
    /// quit, whose windows open again at boot.
    pub fn detached_workspaces(&self) -> Vec<WsId> {
        self.state
            .workspaces
            .iter()
            .filter(|ws| ws.detached == Some(true))
            .map(|ws| ws.id.clone())
            .collect()
    }
}
