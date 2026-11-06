//! Where each pane is in the persisted state, kept between messages so that a
//! pane is found by its id without walking every workspace.
//!
//! The index is rebuilt when the state it was built from is no longer the
//! model's (`refresh`: another revision, another state, another count of
//! panes), which only an update can do. A reader that holds the model
//! borrowed (a view, a subscription) may meet an index a store action has
//! since outdated: every hit is checked against the state before it is
//! believed, and a miss walks the state as before. A stale index is slower,
//! never wrong.

use std::collections::HashMap;
use std::sync::Arc;

use super::PaneInfo;
use crate::state::types::{AppState, Pane, PaneId, Workspace, WsId};

/// What was built from one state: the place of every pane and workspace,
/// and each pane's `PaneInfo`, made once and shared with the machines.
#[derive(Debug, Default)]
pub(super) struct PaneIndex {
    /// The state it was built from: its revision, its address, its count of
    /// panes. `None` before the first build.
    built: Option<(u64, usize, usize)>,
    panes: HashMap<PaneId, Entry>,
    workspaces: HashMap<WsId, usize>,
}

#[derive(Debug)]
struct Entry {
    ws: usize,
    pane: usize,
    info: Arc<PaneInfo>,
}

/// What `built` compares: hydrating replaces the state without a revision,
/// and the count of panes catches a state written in place at the same
/// address.
fn stamp(state: &AppState, revision: u64) -> (u64, usize, usize) {
    let panes = state.workspaces.iter().map(|ws| ws.panes.len()).sum();
    (revision, std::ptr::from_ref(state) as usize, panes)
}

impl PaneIndex {
    /// Rebuilds the index when `state` is not the one it was built from, and
    /// says which panes of the last build the state no longer has.
    pub(super) fn refresh(&mut self, state: &AppState, revision: u64) -> Vec<PaneId> {
        let stamp = stamp(state, revision);
        if self.built == Some(stamp) {
            return Vec::new();
        }
        let before = std::mem::take(&mut self.panes);
        self.workspaces.clear();
        for (w, ws) in state.workspaces.iter().enumerate() {
            self.workspaces.entry(ws.id.clone()).or_insert(w);
            for (p, pane) in ws.panes.iter().enumerate() {
                self.panes.entry(pane.id.clone()).or_insert_with(|| Entry {
                    ws: w,
                    pane: p,
                    info: Arc::new(info_of(ws, pane)),
                });
            }
        }
        self.built = Some(stamp);
        before
            .into_keys()
            .filter(|id| !self.panes.contains_key(id))
            .collect()
    }

    /// The workspace and the pane of `id`, and its `PaneInfo` when the one
    /// built is still what the state says.
    pub(super) fn find<'a>(
        &self,
        state: &'a AppState,
        id: &str,
    ) -> Option<(&'a Workspace, &'a Pane, Option<Arc<PaneInfo>>)> {
        if let Some(entry) = self.panes.get(id)
            && let Some(ws) = state.workspaces.get(entry.ws)
            && let Some(pane) = ws.panes.get(entry.pane)
            && pane.id == id
        {
            let info = describes(&entry.info, ws, pane).then(|| entry.info.clone());
            return Some((ws, pane, info));
        }
        let (ws, pane) = state
            .workspaces
            .iter()
            .find_map(|ws| ws.panes.iter().find(|p| p.id == id).map(|p| (ws, p)))?;
        Some((ws, pane, None))
    }

    /// The `PaneInfo` of a pane already found, the shared one when it still
    /// describes it.
    pub(super) fn info(&self, ws: &Workspace, pane: &Pane) -> Arc<PaneInfo> {
        match self.panes.get(&pane.id) {
            Some(entry) if describes(&entry.info, ws, pane) => entry.info.clone(),
            _ => Arc::new(info_of(ws, pane)),
        }
    }

    /// The workspace `id`.
    pub(super) fn workspace<'a>(&self, state: &'a AppState, id: &str) -> Option<&'a Workspace> {
        if let Some(&w) = self.workspaces.get(id)
            && let Some(ws) = state.workspaces.get(w)
            && ws.id == id
        {
            return Some(ws);
        }
        state.workspaces.iter().find(|ws| ws.id == id)
    }
}

/// Whether `info` is still what the state says of the pane.
fn describes(info: &PaneInfo, ws: &Workspace, pane: &Pane) -> bool {
    info.id == pane.id
        && info.workspace_id == ws.id
        && info.name == pane.name
        && info.agent == pane.agent
        && info.session_id == pane.session_id
}

pub(super) fn info_of(ws: &Workspace, pane: &Pane) -> PaneInfo {
    PaneInfo {
        id: pane.id.clone(),
        workspace_id: ws.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent,
        session_id: pane.session_id.clone(),
    }
}
