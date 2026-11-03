//! The pane tree (`src/lib/layout.ts`, tests in layout.test.ts): `leafIds`,
//! `presetTree`, `withRatio`/`ratioAt`, `removeLeaf`, `renameLeaf`,
//! `insertLeaf`, `swapLeaves`, `moveLeaf`, `normalizeTree`, `readingOrder`,
//! `presetSize`, `closedByPreset`, `computeLayout`, `zoneRect`/`zoneAt`,
//! `neighborOf`, `preferredDir`.
//!
//! TODO(P0b): port the module with its tests. `leaf_ids` is here already
//! because it is the one every reader of a tree starts from.

use super::types::{PaneId, SplitNode};

/// Smallest a pane may be dragged to, in logical pixels (`MIN_PANE_PX`).
pub const MIN_PANE_PX: f32 = 150.0;

/// The pane ids of a tree, left to right and top to bottom (`leafIds`).
pub fn leaf_ids(node: &SplitNode) -> Vec<PaneId> {
    fn walk(node: &SplitNode, out: &mut Vec<PaneId>) {
        match node {
            SplitNode::Pane { id } => out.push(id.clone()),
            SplitNode::Split { a, b, .. } => {
                walk(a, out);
                walk(b, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(node, &mut out);
    out
}
