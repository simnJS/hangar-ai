//! Every preview is a pane of its workspace's grid (a view pane,
//! `state::types::ViewPane`), beside the terminals: what the store holds of a
//! workspace is what its grid shows, one pane per entry, the entry's id being
//! the pane's.
//!
//! The store stays the one record of what is open, as it was when its entries
//! were the tabs of a side panel: the person, the agents (the control bus),
//! the kits and the dev hooks open and close entries there, from any thread.
//! [`sync`] brings the model's view panes in line with it, on the UI thread,
//! after each change: an entry without a pane gets one beside the focused
//! pane, split along its longer side as the + button splits, and a pane whose
//! entry closed goes. Who opened it decides the rest, at the call site: the
//! person's own open focuses the pane (`Intent::OpenKitTab`,
//! `Intent::ShowPreview`), an agent's lays the workspace out again
//! (`control::port::ModelCmd::ViewsChanged`).
//!
//! A workspace whose saved kit tabs have not been read back yet
//! (`Store::is_restored`) keeps its view panes: their entries are on their
//! way. Once they are read, a pane without an entry (a file, which the
//! previews never kept across a restart) goes. A grid at `MAX_PANES` has no
//! room for another: the entry closes again, and the person is told.

use std::collections::{BTreeSet, HashSet};

use hangar_i18n::{Key, args};

use super::store::Store;
use crate::state::{Model, PaneId, StoreAction, WsId};

/// What [`sync`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Synced {
    /// The panes it opened, by workspace, in the order their entries were.
    pub placed: Vec<(WsId, PaneId)>,
    /// The panes it closed, their entries gone.
    pub removed: Vec<(WsId, PaneId)>,
    /// The entries a full grid had no room for, closed again: their titles.
    pub refused: Vec<String>,
}

impl Synced {
    /// What the person is told of the entries a full grid refused.
    pub fn notice(&self, m: &Model) -> Option<String> {
        if self.refused.is_empty() {
            return None;
        }
        Some(
            hangar_i18n::t_with(
                m.locale(),
                Key::PreviewNoRoom,
                &args!(
                    "title" => self.refused.join(", "),
                    "max" => crate::state::layout::MAX_PANES.to_string()
                ),
            )
            .into_owned(),
        )
    }
}

/// One workspace's entries as the store holds them: their ids in order, and
/// whether its saved tabs have been read.
struct Held {
    entries: Vec<(String, String)>,
    restored: bool,
}

fn held(store: &Store, ws: &str) -> Held {
    Held {
        entries: store
            .scope(ws)
            .entries
            .iter()
            .map(|entry| (entry.id.clone(), entry.title.clone()))
            .collect(),
        restored: store.is_restored(ws),
    }
}

/// Brings every workspace's view panes in line with its preview (see the
/// module's comment).
pub fn sync(m: &mut Model) -> Synced {
    let shared = m.live.preview.shared().clone();
    let workspaces: Vec<WsId> = m
        .state()
        .workspaces
        .iter()
        .map(|ws| ws.id.clone())
        .collect();
    let held: Vec<(WsId, Held)> = shared.read(|store| {
        workspaces
            .iter()
            .map(|ws| (ws.clone(), held(store, ws)))
            .collect()
    });
    let mut synced = Synced::default();
    let mut refused: Vec<String> = Vec::new();
    for (ws, held) in held {
        let open: HashSet<&str> = held.entries.iter().map(|(id, _)| id.as_str()).collect();
        if held.restored {
            let gone: Vec<PaneId> = m
                .workspace(&ws)
                .map(|w| {
                    w.view_panes
                        .iter()
                        .filter(|view| !open.contains(view.id.as_str()))
                        .map(|view| view.id.clone())
                        .collect()
                })
                .unwrap_or_default();
            for id in gone {
                m.apply(StoreAction::CloseView {
                    ws: ws.clone(),
                    id: id.clone(),
                });
                synced.removed.push((ws.clone(), id));
            }
        }
        for (id, title) in &held.entries {
            let Some(workspace) = m.workspace(&ws) else {
                break;
            };
            if workspace.has_view(id) {
                continue;
            }
            let near = m
                .focused_leaf(workspace)
                .cloned()
                .or_else(|| crate::state::layout::grid_order(workspace).pop());
            let dir = m.live.panes.split_dir(workspace, near.as_deref());
            let added = m.apply(StoreAction::AddView {
                ws: ws.clone(),
                id: id.clone(),
                near,
                dir: Some(dir),
            });
            match added.pane {
                Some(pane) => synced.placed.push((ws.clone(), pane)),
                None => {
                    refused.push(id.clone());
                    synced.refused.push(title.clone());
                }
            }
        }
    }
    if !refused.is_empty() {
        shared.edit(|store| store.close_models(&refused));
        m.live.preview.sync();
    }
    synced
}

/// The entries whose panes are on screen: every view pane of a workspace
/// whose grid a window draws, but those a zoom on another pane hides.
pub fn shown(m: &Model, grids: &BTreeSet<WsId>) -> BTreeSet<String> {
    let mut shown = BTreeSet::new();
    for ws in grids {
        let Some(workspace) = m.workspace(ws) else {
            continue;
        };
        let zoomed = crate::panes::zoomed_pane(m, workspace);
        for view in &workspace.view_panes {
            if zoomed.as_deref().is_none_or(|zoomed| zoomed == view.id) {
                shown.insert(view.id.clone());
            }
        }
    }
    shown
}
