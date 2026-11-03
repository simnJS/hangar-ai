//! What the navigator lists, worked out from the persisted state and the
//! filter without touching the view, so the rules can be tested on their own
//! (`components/NavigatorModel.ts` and `lib/folders.ts`, with their tests).
//!
//! Rows are addressed by a key that says what they are, `folder:<id>` or
//! `ws:<id>`, because the keyboard walks folders and workspaces as one list
//! and ids alone could not tell which kind a row is.
//!
//! The workspaces filed in no folder come first, then each folder: a folder is
//! a heading over the workspaces under it, so whatever follows its last one
//! would read as filed in it too.

use std::collections::{HashMap, HashSet};

use hangar_ui::matching::fold;

use crate::state::types::{AGENTS, Pane, SplitNode, Workspace, WorkspaceFolder};
use crate::state::{PaneId, WsId, layout};

/// A folder with what the filter lets through of its workspaces.
#[derive(Debug)]
pub struct NavFolder<'a> {
    pub folder: &'a WorkspaceFolder,
    pub workspaces: Vec<&'a Workspace>,
    pub expanded: bool,
    /// Workspaces filed in it, whatever the filter lets through.
    pub size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Folder,
    Ws,
}

/// One row the keyboard can land on, in screen order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRow {
    pub key: String,
    pub kind: RowKind,
    pub id: String,
    /// The row it sits under, for Left; `None` at the top level.
    pub parent: Option<String>,
    /// Folders: whether their workspaces are on screen.
    pub expanded: bool,
    pub foldable: bool,
}

#[derive(Debug)]
pub struct NavTree<'a> {
    pub loose: Vec<&'a Workspace>,
    pub folders: Vec<NavFolder<'a>>,
    pub rows: Vec<NavRow>,
    /// A filter is narrowing the list, so an empty one means "nothing
    /// matches".
    pub filtering: bool,
}

pub struct NavInput<'a, 'f> {
    pub workspaces: &'f [&'a Workspace],
    pub folders: &'a [WorkspaceFolder],
    /// The filter field, as typed.
    pub query: &'f str,
    /// Keep only the workspaces with an agent waiting for an answer or done
    /// and waiting for you.
    pub attention_only: bool,
    /// Whether a pane wants you now; only asked when `attention_only` is on.
    pub wants_you: &'f dyn Fn(&str) -> bool,
}

pub fn row_key(kind: RowKind, id: &str) -> String {
    match kind {
        RowKind::Folder => format!("folder:{id}"),
        RowKind::Ws => format!("ws:{id}"),
    }
}

fn agent_label(pane: &Pane) -> &'static str {
    AGENTS
        .iter()
        .find(|agent| agent.id == pane.agent)
        .map_or(pane.agent.as_str(), |agent| agent.label)
}

/// A workspace's panes as the grid shows them, left to right then top to
/// bottom, which is the order people read them in (`readingOrder`). The split
/// tree only lists ids; an empty workspace would otherwise come back as one
/// nameless leaf.
pub fn reading_order(ws: &Workspace) -> Vec<&Pane> {
    let by_id: HashMap<&str, &Pane> = ws
        .panes
        .iter()
        .map(|pane| (pane.id.as_str(), pane))
        .collect();
    reading_order_ids(&ws.panes, ws.tree.as_ref())
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect()
}

/// The ids of `panes` in reading order: the tree's leaves when it still
/// covers exactly these panes, otherwise the order `normalizeTree` repairs it
/// to, which keeps the leaves that are still panes and appends the new ones.
///
/// TODO(P0b): this is `layout.ts`'s `readingOrder` and belongs with the rest
/// of `state::layout`, which P0b ports; the navigator, the toolbar and the
/// preset confirmation need it before then. Only the order is needed here, not
/// the rebuilt tree: `presetTree` lays ids out in the order it is given.
pub fn reading_order_ids(panes: &[Pane], tree: Option<&SplitNode>) -> Vec<PaneId> {
    let ids: Vec<&PaneId> = panes.iter().map(|pane| &pane.id).collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let Some(tree) = tree else {
        return ids.into_iter().cloned().collect();
    };
    let known: HashSet<&str> = ids.iter().map(|id| id.as_str()).collect();
    let leaves = layout::leaf_ids(tree);
    let mut seen: HashSet<&str> = HashSet::new();
    let mut kept: Vec<PaneId> = Vec::new();
    for id in &leaves {
        if !known.contains(id.as_str()) || !seen.insert(id.as_str()) {
            continue;
        }
        kept.push(id.clone());
    }
    if kept.len() == leaves.len() && kept.len() == ids.len() {
        return kept;
    }
    let missing = ids
        .into_iter()
        .filter(|id| !seen.contains(id.as_str()))
        .cloned();
    kept.into_iter().chain(missing).collect()
}

/// The panes a preset of `count` closes, in reading order (`closedByPreset`).
pub fn closed_by_preset(ws: &Workspace, count: u8) -> Vec<&Pane> {
    let size = usize::from(count).clamp(1, crate::state::types::MAX_PANES);
    reading_order(ws).into_iter().skip(size).collect()
}

fn segments(path: &str) -> Vec<&str> {
    path.split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect()
}

/// What tells apart workspaces that share a name, since their folder is no
/// longer written under it: for each, the last folder of its path that none of
/// its namesakes has at the same depth ("api" in work and in perso reads
/// "api · work" and "api · perso"), and no more of the path than that. A
/// folder that only repeats the name is passed over; namesakes opened on the
/// same folder are numbered instead. A name no other workspace has gets
/// nothing, which is almost always.
pub fn namesake_hints(workspaces: &[&Workspace]) -> HashMap<WsId, String> {
    let mut hints = HashMap::new();
    // Groups in the order their first member appears, so the numbering of
    // namesakes follows the list.
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Vec<&Workspace>> = HashMap::new();
    for ws in workspaces {
        let key = fold(ws.name.trim());
        if !by_name.contains_key(&key) {
            order.push(key.clone());
        }
        by_name.entry(key).or_default().push(ws);
    }
    for name in order {
        let group = &by_name[&name];
        if group.len() < 2 {
            continue;
        }
        let paths: Vec<Vec<&str>> = group.iter().map(|ws| segments(&ws.cwd)).collect();
        let depth = paths.iter().map(Vec::len).max().unwrap_or(0);
        let mut unhinted: Vec<&Workspace> = Vec::new();
        'each: for (index, ws) in group.iter().enumerate() {
            let own = &paths[index];
            for k in 1..=own.len().min(depth) {
                let segment = own[own.len() - k];
                if fold(segment) == name {
                    continue;
                }
                let shared = paths.iter().enumerate().any(|(at, other)| {
                    at != index
                        && other
                            .len()
                            .checked_sub(k)
                            .map_or(String::new(), |i| fold(other[i]))
                            == fold(segment)
                });
                if !shared {
                    hints.insert(ws.id.clone(), segment.to_string());
                    continue 'each;
                }
            }
            unhinted.push(ws);
        }
        // One left without a word is told apart by the others' already.
        if unhinted.len() > 1 {
            for (index, ws) in unhinted.iter().enumerate() {
                hints.insert(ws.id.clone(), (index + 1).to_string());
            }
        }
    }
    hints
}

pub fn build_nav_tree<'a>(input: &NavInput<'a, '_>) -> NavTree<'a> {
    let needle = fold(input.query.trim());
    let filtering = !needle.is_empty() || input.attention_only;
    let attention_only = input.attention_only;

    // Found by its own name, its folder's, or one of its agents': their names
    // are not on screen any more, but "eve" still finds the workspace Eve is
    // in.
    let shown = |ws: &Workspace, folder_hit: bool| -> bool {
        if attention_only && !ws.panes.iter().any(|pane| (input.wants_you)(&pane.id)) {
            return false;
        }
        if needle.is_empty() || folder_hit || fold(&ws.name).contains(&needle) {
            return true;
        }
        ws.panes.iter().any(|pane| {
            fold(&pane.name).contains(&needle) || fold(agent_label(pane)).contains(&needle)
        })
    };

    let known: HashSet<&str> = input
        .folders
        .iter()
        .map(|folder| folder.id.as_str())
        .collect();
    let loose: Vec<&Workspace> = input
        .workspaces
        .iter()
        .copied()
        .filter(|ws| {
            ws.folder_id
                .as_deref()
                .is_none_or(|id| id.is_empty() || !known.contains(id))
                && shown(ws, false)
        })
        .collect();

    let mut folders = Vec::new();
    for folder in input.folders {
        let filed: Vec<&Workspace> = input
            .workspaces
            .iter()
            .copied()
            .filter(|ws| ws.folder_id.as_deref() == Some(folder.id.as_str()))
            .collect();
        let folder_hit = !needle.is_empty() && fold(&folder.name).contains(&needle);
        let kept: Vec<&Workspace> = filed
            .iter()
            .copied()
            .filter(|ws| shown(ws, folder_hit))
            .collect();
        // Under a filter a folder is only a heading for what it lets through,
        // held open so that is visible, unless its own name is the match.
        if filtering && kept.is_empty() && !(folder_hit && !attention_only) {
            continue;
        }
        folders.push(NavFolder {
            folder,
            workspaces: kept,
            expanded: filtering || !folder.collapsed,
            size: filed.len(),
        });
    }

    let mut rows: Vec<NavRow> = loose
        .iter()
        .map(|ws| NavRow {
            key: row_key(RowKind::Ws, &ws.id),
            kind: RowKind::Ws,
            id: ws.id.clone(),
            parent: None,
            expanded: false,
            foldable: false,
        })
        .collect();
    for entry in &folders {
        let key = row_key(RowKind::Folder, &entry.folder.id);
        rows.push(NavRow {
            key: key.clone(),
            kind: RowKind::Folder,
            id: entry.folder.id.clone(),
            parent: None,
            expanded: entry.expanded,
            foldable: !filtering,
        });
        if !entry.expanded {
            continue;
        }
        for ws in &entry.workspaces {
            rows.push(NavRow {
                key: row_key(RowKind::Ws, &ws.id),
                kind: RowKind::Ws,
                id: ws.id.clone(),
                parent: Some(key.clone()),
                expanded: false,
                foldable: false,
            });
        }
    }

    NavTree {
        loose,
        folders,
        rows,
        filtering,
    }
}

/// The workspaces the way the sidebar lists them: the ones at the top level,
/// then each folder's in folder order (`sidebarOrder`).
///
/// Shortcuts that step through workspaces follow this rather than the stored
/// order, so Ctrl+3 is the third row on screen. A collapsed folder still
/// counts: numbering that shifted every time one was folded would be no use.
///
/// TODO(P0b): `state::folders` ports this module for the shortcuts; the
/// navigator and Needs-you need it before then.
pub fn sidebar_order<'a>(
    workspaces: &'a [Workspace],
    folders: &[WorkspaceFolder],
) -> Vec<&'a Workspace> {
    let known: HashSet<&str> = folders.iter().map(|folder| folder.id.as_str()).collect();
    let mut ordered: Vec<&Workspace> = workspaces
        .iter()
        .filter(|ws| ws.folder_id.as_deref().is_none_or(|id| !known.contains(id)))
        .collect();
    for folder in folders {
        ordered.extend(
            workspaces
                .iter()
                .filter(|ws| ws.folder_id.as_deref() == Some(folder.id.as_str())),
        );
    }
    ordered
}

#[cfg(test)]
mod tests;
