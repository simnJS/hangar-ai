//! Drag reorder in the navigator (`Sidebar.tsx`: `beginDrag`, `targetAt`,
//! `commit`).
//!
//! Rows are dragged with the pointer, not with a platform drag and drop: the
//! window takes file drops itself. A press starts it; it becomes a drag only
//! after the pointer has travelled a few pixels, so a click stays a click; and
//! the move is committed only on release. A cancelled gesture, a window that
//! lost the keyboard or a release outside the list puts everything back where
//! it was.
//!
//! The web app found what was under the pointer with `elementFromPoint`. Rows
//! here have fixed heights, so each row reports where inside it the pointer
//! is, and whether that is its upper or its lower half is all `after` needs.

use iced::Point;

use crate::state::types::{Workspace, WorkspaceFolder};
use crate::state::{StoreAction, WsId};

/// Pointer travel before a press becomes a drag, so a click stays a click.
pub const THRESHOLD: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragKind {
    Workspace,
    Folder,
}

/// What is being dragged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragItem {
    pub kind: DragKind,
    pub id: String,
}

/// Where a drag lands if released now. `Row` sits beside another entry of the
/// same kind, before it or, with `after`, after it. `Into` files a workspace
/// last in a folder. `Root` sends it last to the top level (or a folder last
/// among folders), dropped on the heading or under the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropTarget {
    Row { id: String, after: bool },
    Into { folder: String },
    Root { heading: bool },
}

/// What is under the pointer: a row of the list, or one of the drop zones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Workspace(String),
    Folder(String),
    /// The placeholder of an empty folder.
    FolderBody(String),
    Root {
        heading: bool,
    },
}

/// A drag in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    pub item: DragItem,
    /// The pointer has travelled far enough for this to be a drag.
    pub active: bool,
    /// Where the pointer was first seen after the press, in the coordinates of
    /// the row it was over: the threshold is measured from here.
    anchor: Option<(String, Point)>,
    pub target: Option<DropTarget>,
    /// The button came up somewhere nothing took the release: a press that
    /// never became a drag stays inert until the next press replaces it.
    pub ended: bool,
}

impl Drag {
    /// A press on a row: not a drag yet.
    pub fn pressed(item: DragItem) -> Drag {
        Drag {
            item,
            active: false,
            anchor: None,
            target: None,
            ended: false,
        }
    }

    /// The pointer moved over `row` at `point` (in that row's coordinates),
    /// over `hit`. Returns whether the drag changed.
    ///
    /// Distance is only measured inside one row: leaving the row the press
    /// was on is travel enough, since rows are 28 px tall and a click does not
    /// leave one.
    pub fn moved(&mut self, row: &str, point: Point, row_height: f32, hit: Option<Hit>) -> bool {
        if self.ended {
            return false;
        }
        if !self.active {
            match &self.anchor {
                None => {
                    self.anchor = Some((row.to_string(), point));
                    return false;
                }
                Some((from, origin)) if from == row => {
                    let travelled = (point.x - origin.x).hypot(point.y - origin.y);
                    if travelled < THRESHOLD {
                        return false;
                    }
                }
                Some(_) => {}
            }
            self.active = true;
        }
        let after = point.y > row_height / 2.0;
        let target = hit.and_then(|hit| target_at(&self.item, &hit, after));
        // A gap between two rows belongs to no row and keeps what the pointer
        // was over just before, rather than making the marker blink on every
        // row boundary: no row reports it, so nothing lands here for it.
        let changed = self.target != target;
        self.target = target;
        changed
    }

    /// The pointer left the list: nothing would be dropped.
    pub fn left_list(&mut self) {
        self.target = None;
    }
}

/// What a drag of `item` lands on, over `hit` (`targetAt`).
pub fn target_at(item: &DragItem, hit: &Hit, after: bool) -> Option<DropTarget> {
    if let Hit::Root { heading } = hit {
        return Some(DropTarget::Root { heading: *heading });
    }
    match item.kind {
        DragKind::Folder => match hit {
            Hit::Folder(id) if *id != item.id => Some(DropTarget::Row {
                id: id.clone(),
                after,
            }),
            _ => None,
        },
        DragKind::Workspace => match hit {
            Hit::Workspace(id) if *id == item.id => None,
            Hit::Workspace(id) => Some(DropTarget::Row {
                id: id.clone(),
                after,
            }),
            Hit::Folder(id) | Hit::FolderBody(id) => Some(DropTarget::Into { folder: id.clone() }),
            Hit::Root { .. } => None,
        },
    }
}

/// The store action a release on `target` makes, if it moves anything
/// (`commit`).
pub fn commit(
    item: &DragItem,
    target: &DropTarget,
    workspaces: &[Workspace],
    folders: &[WorkspaceFolder],
) -> Option<StoreAction> {
    match item.kind {
        DragKind::Folder => {
            let id = item.id.clone();
            match target {
                DropTarget::Root { .. } => Some(StoreAction::MoveFolder { id, before: None }),
                DropTarget::Into { .. } => None,
                DropTarget::Row { id: beside, after } => {
                    let others: Vec<&WorkspaceFolder> =
                        folders.iter().filter(|folder| folder.id != id).collect();
                    let at = others.iter().position(|folder| folder.id == *beside)?;
                    let before = if *after {
                        others.get(at + 1).map(|folder| folder.id.clone())
                    } else {
                        Some(beside.clone())
                    };
                    Some(StoreAction::MoveFolder { id, before })
                }
            }
        }
        DragKind::Workspace => {
            let id: WsId = item.id.clone();
            match target {
                DropTarget::Root { .. } => Some(StoreAction::MoveWorkspace {
                    id,
                    folder: None,
                    before: None,
                }),
                DropTarget::Into { folder } => Some(StoreAction::MoveWorkspace {
                    id,
                    folder: Some(folder.clone()),
                    before: None,
                }),
                // Beside another workspace: into its folder, next to it.
                // "After" is "before the next one", or last in that folder
                // when it is the last.
                DropTarget::Row { id: beside, after } => {
                    let anchor = workspaces.iter().find(|ws| ws.id == *beside)?;
                    let group: Vec<&Workspace> = workspaces
                        .iter()
                        .filter(|ws| ws.id != id && ws.folder_id == anchor.folder_id)
                        .collect();
                    let at = group.iter().position(|ws| ws.id == anchor.id)?;
                    let before = if *after {
                        group.get(at + 1).map(|ws| ws.id.clone())
                    } else {
                        Some(anchor.id.clone())
                    };
                    Some(StoreAction::MoveWorkspace {
                        id,
                        folder: anchor.folder_id.clone(),
                        before,
                    })
                }
            }
        }
    }
}

/// The marker a row wears while a drag would land beside it: `Some(after)`.
pub fn marker(drag: Option<&Drag>, kind: DragKind, id: &str) -> Option<bool> {
    let drag = drag.filter(|drag| drag.active && drag.item.kind == kind)?;
    match &drag.target {
        Some(DropTarget::Row { id: beside, after }) if beside == id => Some(*after),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
