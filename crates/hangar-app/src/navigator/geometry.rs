//! Where each row of the list is, so the list can scroll to the one the
//! keyboard moves to (a focused element scrolls into view on its own in a web
//! page; an iced scrollable only moves when told to).
//!
//! Rows have fixed heights, so their places follow from the tree alone, with
//! the same constants the view lays them out with.

use super::model::{NavTree, RowKind, row_key};

/// A workspace's row, and a folder's heading.
pub const ROW_HEIGHT: f32 = 28.0;
/// The space between two children of a column of the list.
pub const GAP: f32 = 1.0;
/// The space above a folder, which with its heading is what groups it.
pub const FOLDER_GAP: f32 = 10.0;
/// An empty folder's placeholder, and the space under it.
pub const PLACEHOLDER_HEIGHT: f32 = 26.0;
pub const PLACEHOLDER_GAP: f32 = 4.0;
const PLACEHOLDER: f32 = PLACEHOLDER_HEIGHT + PLACEHOLDER_GAP;
/// The padding under a folder's rows.
pub const BODY_PAD: f32 = 2.0;
/// The padding above the list.
pub const LIST_PAD_TOP: f32 = 2.0;
/// What the list leaves visible around a row it scrolls to.
const MARGIN: f32 = 6.0;

/// Where a row is, in the coordinates of the scrolled content.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub key: String,
    pub top: f32,
    pub bottom: f32,
}

/// Every row of the tree that the keyboard can land on, top to bottom.
pub fn spans(tree: &NavTree<'_>) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut y = LIST_PAD_TOP;
    let mut children = 0;
    let mut next = |y: &mut f32| {
        if children > 0 {
            *y += GAP;
        }
        children += 1;
    };
    for ws in &tree.loose {
        next(&mut y);
        spans.push(Span {
            key: row_key(RowKind::Ws, &ws.id),
            top: y,
            bottom: y + ROW_HEIGHT,
        });
        y += ROW_HEIGHT;
    }
    for entry in &tree.folders {
        next(&mut y);
        // The space above, then the heading, then what it holds.
        y += FOLDER_GAP + GAP;
        spans.push(Span {
            key: row_key(RowKind::Folder, &entry.folder.id),
            top: y,
            bottom: y + ROW_HEIGHT,
        });
        y += ROW_HEIGHT;
        if !entry.expanded {
            continue;
        }
        y += GAP;
        for (at, ws) in entry.workspaces.iter().enumerate() {
            if at > 0 {
                y += GAP;
            }
            spans.push(Span {
                key: row_key(RowKind::Ws, &ws.id),
                top: y,
                bottom: y + ROW_HEIGHT,
            });
            y += ROW_HEIGHT;
        }
        if entry.size == 0 {
            y += PLACEHOLDER;
        }
        y += BODY_PAD;
    }
    spans
}

/// The offset the list has to scroll to for `span` to be in view, if it is not
/// already, given where the list is scrolled to and how tall it is.
pub fn reveal(span: &Span, offset: f32, viewport: f32) -> Option<f32> {
    if viewport <= 0.0 {
        return None;
    }
    if span.top < offset {
        return Some((span.top - MARGIN).max(0.0));
    }
    if span.bottom > offset + viewport {
        return Some(span.bottom + MARGIN - viewport);
    }
    None
}

#[cfg(test)]
mod tests;
