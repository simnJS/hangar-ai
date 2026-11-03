//! The pane tree (`src/lib/layout.ts`, tests in layout.test.ts).
//!
//! The split tree is the one source of truth for the arrangement. Panes are
//! named by id, never by position, so a pane can be closed or moved without
//! touching the identity of the others, and therefore without killing their
//! shells. Rectangles are fractions of the grid, so the grid can place every
//! pane from four numbers and never rebuild a terminal when the arrangement
//! changes.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::types::{DropZone, Pane, PaneId, SplitDir, SplitNode};

pub use super::types::MAX_PANES;

/// Smallest a pane may be dragged to, in logical pixels (`MIN_PANE_PX`), so it
/// stays usable while dragging.
pub const MIN_PANE_PX: f32 = 150.0;

/// One turn on the way from the root to a split: into its `a` or its `b`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    A,
    B,
}

/// The turns leading to a split node (`Path`).
pub type Path = Vec<Side>;

/// A path as the TS keyed it: `"ab"`, or `"root"` for the root.
pub fn path_key(path: &[Side]) -> String {
    if path.is_empty() {
        return "root".into();
    }
    path.iter()
        .map(|side| match side {
            Side::A => 'a',
            Side::B => 'b',
        })
        .collect()
}

/// Fractions of the grid, 0 to 1 (`Rect`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const FULL: Rect = Rect {
        left: 0.0,
        top: 0.0,
        width: 1.0,
        height: 1.0,
    };
}

/// The boundary between the two sides of a split, which a drag moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Handle {
    pub key: String,
    pub path: Path,
    pub dir: SplitDir,
    /// The line itself: no width for a row split, no height for a column.
    pub rect: Rect,
    /// What the split owns, to turn a pointer position into a ratio.
    pub bounds: Rect,
    /// The panes on either side, left or above, right or below, in reading
    /// order: what the handle's label names.
    pub before: Vec<PaneId>,
    pub after: Vec<PaneId>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayout {
    pub rects: HashMap<PaneId, Rect>,
    pub handles: Vec<Handle>,
    /// Pane ids in reading order: what the numbering and Ctrl+N follow.
    pub order: Vec<PaneId>,
}

pub fn leaf(id: impl Into<PaneId>) -> SplitNode {
    SplitNode::Pane { id: id.into() }
}

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

fn split(dir: SplitDir, ratio: f64, a: SplitNode, b: SplitNode) -> SplitNode {
    SplitNode::Split {
        dir,
        ratio,
        a: Box::new(a),
        b: Box::new(b),
    }
}

/// Folds nodes into a balanced binary split with even shares.
fn fold(dir: SplitDir, mut nodes: Vec<SplitNode>) -> SplitNode {
    if nodes.len() == 1 {
        return nodes.remove(0);
    }
    let mid = nodes.len().div_ceil(2);
    let ratio = mid as f64 / nodes.len() as f64;
    let rest = nodes.split_off(mid);
    split(dir, ratio, fold(dir, nodes), fold(dir, rest))
}

/// The even grid for a set of panes: one wide row up to two panes, then as
/// many columns as the rows allow, which gives the familiar 1, 2, 2×2 and 4×2
/// shapes (`presetTree`).
pub fn preset_tree(ids: &[PaneId]) -> SplitNode {
    if ids.len() <= 1 {
        return leaf(ids.first().cloned().unwrap_or_default());
    }
    let rows = ((ids.len() as f64 / 2.0).sqrt().ceil() as usize).max(1);
    let columns = ids.len().div_ceil(rows);
    let bands = ids
        .chunks(columns)
        .map(|band| fold(SplitDir::Row, band.iter().cloned().map(leaf).collect()))
        .collect();
    fold(SplitDir::Col, bands)
}

/// The share of the width the lead pane takes beside `others` panes: the most
/// of any pane whatever their number, less as they grow so theirs stay
/// usable.
fn lead_share(others: usize) -> f64 {
    match others {
        0 | 1 => 0.6,
        2 | 3 => 0.55,
        4..=7 => 0.5,
        _ => 0.4,
    }
}

/// The layout Hangar gives a workspace when an agent opens or closes a pane,
/// instead of halving one pane each time: the `lead` pane (the
/// orchestrator's) on the left, bigger than any other, and the rest in an
/// even grid beside it, in `order`. Without a lead, the even grid
/// ([`preset_tree`]).
///
/// The grid beside the lead has as many columns as keep its panes about as
/// wide for their height as the window is: one column up to four panes, two
/// from five, three from eleven.
pub fn arranged_tree(order: &[PaneId], lead: Option<&str>) -> SplitNode {
    let Some(lead) = lead.filter(|lead| order.iter().any(|id| id == lead)) else {
        return preset_tree(order);
    };
    let others: Vec<PaneId> = order.iter().filter(|id| *id != lead).cloned().collect();
    if others.is_empty() {
        return leaf(lead);
    }
    let share = lead_share(others.len());
    let columns = ((others.len() as f64 * (1.0 - share)).sqrt().round() as usize).max(1);
    // As many rows as the columns need, the panes dealt so that no row has
    // more than one pane over another: 13 in three columns are rows of 3, 3,
    // 3, 2 and 2, not four of 3 and one pane alone across the width.
    let rows = others.len().div_ceil(columns);
    let (base, extra) = (others.len() / rows, others.len() % rows);
    let mut rest = others.as_slice();
    let bands = (0..rows)
        .map(|row| {
            let (band, after) = rest.split_at(base + usize::from(row < extra));
            rest = after;
            fold(SplitDir::Row, band.iter().cloned().map(leaf).collect())
        })
        .collect();
    split(SplitDir::Row, share, leaf(lead), fold(SplitDir::Col, bands))
}

/// The tree with the ratio at `path` replaced (`withRatio`).
pub fn with_ratio(node: &SplitNode, path: &[Side], ratio: f64) -> SplitNode {
    match node {
        SplitNode::Pane { .. } => node.clone(),
        SplitNode::Split {
            dir,
            ratio: own,
            a,
            b,
        } => match path.split_first() {
            None => split(*dir, ratio, (**a).clone(), (**b).clone()),
            Some((Side::A, rest)) => split(*dir, *own, with_ratio(a, rest, ratio), (**b).clone()),
            Some((Side::B, rest)) => split(*dir, *own, (**a).clone(), with_ratio(b, rest, ratio)),
        },
    }
}

/// The ratio at `path`; a leaf answers an even half (`ratioAt`).
pub fn ratio_at(node: &SplitNode, path: &[Side]) -> f64 {
    match node {
        SplitNode::Pane { .. } => 0.5,
        SplitNode::Split { ratio, a, b, .. } => match path.split_first() {
            None => *ratio,
            Some((Side::A, rest)) => ratio_at(a, rest),
            Some((Side::B, rest)) => ratio_at(b, rest),
        },
    }
}

/// The tree without pane `id`, its sibling taking the whole space; `None`
/// once the last pane is gone (`removeLeaf`).
pub fn remove_leaf(node: &SplitNode, id: &str) -> Option<SplitNode> {
    match node {
        SplitNode::Pane { id: own } => (own != id).then(|| node.clone()),
        SplitNode::Split { dir, ratio, a, b } => match (remove_leaf(a, id), remove_leaf(b, id)) {
            (None, rest) | (rest, None) => rest,
            (Some(a), Some(b)) => Some(split(*dir, *ratio, a, b)),
        },
    }
}

/// The same arrangement with pane `from` now called `to`: a respawned pane
/// keeps its place (`renameLeaf`).
pub fn rename_leaf(node: &SplitNode, from: &str, to: &str) -> SplitNode {
    replace_leaf(node, from, &|_| leaf(to))
}

fn replace_leaf(node: &SplitNode, id: &str, make: &dyn Fn(&SplitNode) -> SplitNode) -> SplitNode {
    match node {
        SplitNode::Pane { id: own } if own == id => make(node),
        SplitNode::Pane { .. } => node.clone(),
        SplitNode::Split { dir, ratio, a, b } => split(
            *dir,
            *ratio,
            replace_leaf(a, id, make),
            replace_leaf(b, id, make),
        ),
    }
}

/// Splits `target` in two and puts `new_id` on the side `zone` names
/// (`insertLeaf`). `Center` is not a side; it is read as `Right`.
pub fn insert_leaf(node: &SplitNode, target: &str, new_id: &str, zone: DropZone) -> SplitNode {
    let dir = match zone {
        DropZone::Left | DropZone::Right | DropZone::Center => SplitDir::Row,
        DropZone::Top | DropZone::Bottom => SplitDir::Col,
    };
    let before = matches!(zone, DropZone::Left | DropZone::Top);
    replace_leaf(node, target, &|target| {
        let (a, b) = if before {
            (leaf(new_id), target.clone())
        } else {
            (target.clone(), leaf(new_id))
        };
        split(dir, 0.5, a, b)
    })
}

/// Exchanges two panes without changing the shape of the tree
/// (`swapLeaves`).
pub fn swap_leaves(node: &SplitNode, first: &str, second: &str) -> SplitNode {
    match node {
        SplitNode::Pane { id } if id == first => leaf(second),
        SplitNode::Pane { id } if id == second => leaf(first),
        SplitNode::Pane { .. } => node.clone(),
        SplitNode::Split { dir, ratio, a, b } => split(
            *dir,
            *ratio,
            swap_leaves(a, first, second),
            swap_leaves(b, first, second),
        ),
    }
}

/// A drop in the middle of a pane swaps the two; a drop on an edge splits
/// the target there (`moveLeaf`).
pub fn move_leaf(node: &SplitNode, drag: &str, target: &str, zone: DropZone) -> SplitNode {
    if drag == target {
        return node.clone();
    }
    if zone == DropZone::Center {
        return swap_leaves(node, drag, target);
    }
    match remove_leaf(node, drag) {
        Some(without) => insert_leaf(&without, target, drag, zone),
        None => node.clone(),
    }
}

/// A tree as `state.json` holds it, which may be a legacy one whose leaves
/// stored a pane's index rather than its id: mapped onto the panes' ids
/// (`migrate`). A split with one side unreadable is its other side; a ratio
/// outside (0, 1) is an even half.
pub fn migrate(raw: &Value, ids: &[PaneId]) -> Option<SplitNode> {
    let raw = raw.as_object()?;
    match raw.get("type").and_then(Value::as_str) {
        Some("pane") => {
            if let Some(id) = raw.get("id").and_then(Value::as_str) {
                return Some(leaf(id));
            }
            let index = raw.get("index")?.as_f64()?;
            if index < 0.0 || index.fract() != 0.0 {
                return None;
            }
            ids.get(index as usize).map(|id| leaf(id.clone()))
        }
        Some("split") => {
            let a = raw.get("a").and_then(|a| migrate(a, ids));
            let b = raw.get("b").and_then(|b| migrate(b, ids));
            let (a, b) = match (a, b) {
                (None, rest) | (rest, None) => return rest,
                (Some(a), Some(b)) => (a, b),
            };
            let ratio = raw
                .get("ratio")
                .and_then(Value::as_f64)
                .filter(|ratio| *ratio > 0.0 && *ratio < 1.0)
                .unwrap_or(0.5);
            let dir = match raw.get("dir").and_then(Value::as_str) {
                Some("col") => SplitDir::Col,
                _ => SplitDir::Row,
            };
            Some(split(dir, ratio, a, b))
        }
        _ => None,
    }
}

/// `migrate` for a tree already typed: the ratios the TS would not have kept.
fn sanitized(node: &SplitNode) -> SplitNode {
    match node {
        SplitNode::Pane { .. } => node.clone(),
        SplitNode::Split { dir, ratio, a, b } => {
            let ratio = if *ratio > 0.0 && *ratio < 1.0 {
                *ratio
            } else {
                0.5
            };
            split(*dir, ratio, sanitized(a), sanitized(b))
        }
    }
}

/// The tree, made to cover exactly the current panes (`normalizeTree`). A
/// stored tree that still matches comes back as it was, ratios included;
/// anything else is rebuilt as the even grid, in the order the person had.
pub fn normalize_tree(panes: &[Pane], tree: Option<&SplitNode>) -> SplitNode {
    let ids: Vec<PaneId> = panes.iter().map(|pane| pane.id.clone()).collect();
    if ids.is_empty() {
        return leaf("");
    }
    let Some(tree) = tree.map(sanitized) else {
        return preset_tree(&ids);
    };
    let known: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    let leaves = leaf_ids(&tree);
    let mut kept = Vec::new();
    for id in &leaves {
        if !known.contains(id.as_str()) || !seen.insert(id.clone()) {
            continue;
        }
        kept.push(id.clone());
    }
    if kept.len() == leaves.len() && kept.len() == ids.len() {
        return tree;
    }
    kept.extend(ids.into_iter().filter(|id| !seen.contains(id)));
    preset_tree(&kept)
}

/// The panes in the order they are read on screen (`readingOrder`): the order
/// a preset keeps them in, so the panes seen first stay.
pub fn reading_order<'a>(panes: &'a [Pane], tree: Option<&SplitNode>) -> Vec<&'a Pane> {
    let by_id: HashMap<&str, &Pane> = panes.iter().map(|pane| (pane.id.as_str(), pane)).collect();
    leaf_ids(&normalize_tree(panes, tree))
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect()
}

/// How many panes a preset asking for `count` ends up with (`presetSize`).
pub fn preset_size(count: usize) -> usize {
    count.clamp(1, MAX_PANES)
}

/// The panes a preset of `count` closes, in reading order (`closedByPreset`).
pub fn closed_by_preset<'a>(
    panes: &'a [Pane],
    tree: Option<&SplitNode>,
    count: usize,
) -> Vec<&'a Pane> {
    reading_order(panes, tree)
        .into_iter()
        .skip(preset_size(count))
        .collect()
}

/// The tree as flat rectangles and the boundaries between them
/// (`computeLayout`). Ratios are clamped to 5–95 % so no pane is crushed.
pub fn compute_layout(tree: &SplitNode) -> GridLayout {
    fn walk(node: &SplitNode, rect: Rect, path: &mut Path, out: &mut GridLayout) {
        let SplitNode::Split { dir, ratio, a, b } = node else {
            if let SplitNode::Pane { id } = node {
                out.rects.insert(id.clone(), rect);
                out.order.push(id.clone());
            }
            return;
        };
        let ratio = ratio.clamp(0.05, 0.95);
        let (first, second, line) = match dir {
            SplitDir::Row => {
                let width = rect.width * ratio;
                (
                    Rect { width, ..rect },
                    Rect {
                        left: rect.left + width,
                        width: rect.width - width,
                        ..rect
                    },
                    Rect {
                        left: rect.left + width,
                        top: rect.top,
                        width: 0.0,
                        height: rect.height,
                    },
                )
            }
            SplitDir::Col => {
                let height = rect.height * ratio;
                (
                    Rect { height, ..rect },
                    Rect {
                        top: rect.top + height,
                        height: rect.height - height,
                        ..rect
                    },
                    Rect {
                        left: rect.left,
                        top: rect.top + height,
                        width: rect.width,
                        height: 0.0,
                    },
                )
            }
        };
        path.push(Side::A);
        walk(a, first, path, out);
        path.pop();
        path.push(Side::B);
        walk(b, second, path, out);
        path.pop();
        out.handles.push(Handle {
            key: path_key(path),
            path: path.clone(),
            dir: *dir,
            rect: line,
            bounds: rect,
            before: leaf_ids(a),
            after: leaf_ids(b),
        });
    }
    let mut out = GridLayout::default();
    walk(tree, Rect::FULL, &mut Vec::new(), &mut out);
    out
}

/// The half of a pane a dropped pane would take, previewed while dragging
/// (`zoneRect`); the middle is the whole pane.
pub fn zone_rect(rect: Rect, zone: DropZone) -> Rect {
    match zone {
        DropZone::Left => Rect {
            width: rect.width / 2.0,
            ..rect
        },
        DropZone::Right => Rect {
            left: rect.left + rect.width / 2.0,
            width: rect.width / 2.0,
            ..rect
        },
        DropZone::Top => Rect {
            height: rect.height / 2.0,
            ..rect
        },
        DropZone::Bottom => Rect {
            top: rect.top + rect.height / 2.0,
            height: rect.height / 2.0,
            ..rect
        },
        DropZone::Center => rect,
    }
}

/// Which side of `rect` (in pixels) the point is closest to; the middle
/// third means a swap (`zoneAt`).
pub fn zone_at(rect: Rect, x: f64, y: f64) -> DropZone {
    let dx = (x - rect.left) / rect.width.max(1.0);
    let dy = (y - rect.top) / rect.height.max(1.0);
    if dx > 0.33 && dx < 0.67 && dy > 0.33 && dy < 0.67 {
        return DropZone::Center;
    }
    let mut zone = DropZone::Left;
    let mut best = dx;
    if 1.0 - dx < best {
        best = 1.0 - dx;
        zone = DropZone::Right;
    }
    if dy < best {
        best = dy;
        zone = DropZone::Top;
    }
    if 1.0 - dy < best {
        zone = DropZone::Bottom;
    }
    zone
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The pane next to `from` in direction `dir`, or `None` at the edge of the
/// grid (`neighborOf`).
///
/// Read off the rectangles rather than the tree: "the pane on the left" is
/// what is on screen, and two panes can sit side by side without being
/// siblings. A candidate must lie entirely past our edge and face us (overlap
/// on the other axis); the closest wins, then the one facing us the most,
/// then the first in reading order.
pub fn neighbor_of(tree: &SplitNode, from: &str, dir: Direction) -> Option<PaneId> {
    const EPSILON: f64 = 1e-4;
    let layout = compute_layout(tree);
    let own = *layout.rects.get(from)?;
    let horizontal = matches!(dir, Direction::Left | Direction::Right);
    let backwards = matches!(dir, Direction::Left | Direction::Up);

    let near = if horizontal { own.left } else { own.top };
    let far = near + if horizontal { own.width } else { own.height };
    let side_start = if horizontal { own.top } else { own.left };
    let side_end = side_start + if horizontal { own.height } else { own.width };

    let mut best: Option<PaneId> = None;
    let mut best_gap = f64::INFINITY;
    let mut best_overlap = 0.0;
    for id in &layout.order {
        if id == from {
            continue;
        }
        let rect = layout.rects[id];
        let start = if horizontal { rect.left } else { rect.top };
        let end = start + if horizontal { rect.width } else { rect.height };
        let gap = if backwards { near - end } else { start - far };
        if gap < -EPSILON {
            continue;
        }
        let other_start = if horizontal { rect.top } else { rect.left };
        let other_end = other_start + if horizontal { rect.height } else { rect.width };
        let overlap = side_end.min(other_end) - side_start.max(other_start);
        if overlap <= EPSILON {
            continue;
        }
        if gap < best_gap - EPSILON || (gap <= best_gap + EPSILON && overlap > best_overlap) {
            best = Some(id.clone());
            best_gap = gap;
            best_overlap = overlap;
        }
    }
    best
}

/// The direction to split a pane of this size in: along its longer side, so
/// panes stay as square as they can (`preferredDir`). Without a size, a row.
pub fn preferred_dir(size: Option<(f32, f32)>) -> SplitDir {
    match size {
        Some((width, height)) if width < height * 1.15 => SplitDir::Col,
        _ => SplitDir::Row,
    }
}

#[cfg(test)]
mod tests {
    //! layout.test.ts, case by case.

    use super::*;
    use serde_json::json;

    fn split_of(dir: SplitDir, a: SplitNode, b: SplitNode, ratio: f64) -> SplitNode {
        split(dir, ratio, a, b)
    }
    fn row(a: SplitNode, b: SplitNode) -> SplitNode {
        split(SplitDir::Row, 0.5, a, b)
    }
    fn col(a: SplitNode, b: SplitNode) -> SplitNode {
        split(SplitDir::Col, 0.5, a, b)
    }
    fn panes(ids: &[&str]) -> Vec<Pane> {
        ids.iter()
            .map(|id| Pane {
                id: (*id).into(),
                ..Pane::default()
            })
            .collect()
    }
    fn ids(list: &[&str]) -> Vec<PaneId> {
        list.iter().map(|id| (*id).to_string()).collect()
    }
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn of<'a>(panes: &[&'a Pane]) -> Vec<&'a str> {
        panes.iter().map(|pane| pane.id.as_str()).collect()
    }

    // presetTree

    #[test]
    fn is_a_single_leaf_for_one_pane() {
        assert_eq!(preset_tree(&ids(&["a"])), leaf("a"));
    }

    #[test]
    fn puts_two_panes_side_by_side() {
        assert_eq!(preset_tree(&ids(&["a", "b"])), row(leaf("a"), leaf("b")));
    }

    #[test]
    fn lays_four_panes_out_as_an_even_2x2() {
        let layout = compute_layout(&preset_tree(&ids(&["a", "b", "c", "d"])));
        assert_eq!(layout.order, ids(&["a", "b", "c", "d"]));
        assert_eq!(
            layout.rects["a"],
            Rect {
                left: 0.0,
                top: 0.0,
                width: 0.5,
                height: 0.5
            }
        );
        assert_eq!(
            layout.rects["d"],
            Rect {
                left: 0.5,
                top: 0.5,
                width: 0.5,
                height: 0.5
            }
        );
    }

    #[test]
    fn lays_eight_panes_out_as_4x2() {
        let list = ids(&["1", "2", "3", "4", "5", "6", "7", "8"]);
        let layout = compute_layout(&preset_tree(&list));
        for id in &list {
            assert!(close(layout.rects[id].width, 0.25));
            assert!(close(layout.rects[id].height, 0.5));
        }
        assert!(close(layout.rects["5"].top, 0.5));
    }

    #[test]
    fn lets_the_odd_pane_out_take_the_whole_last_row() {
        let layout = compute_layout(&preset_tree(&ids(&["a", "b", "c"])));
        assert_eq!(
            layout.rects["c"],
            Rect {
                left: 0.0,
                top: 0.5,
                width: 1.0,
                height: 0.5
            }
        );
    }

    #[test]
    fn tidies_a_lopsided_tree_into_an_even_grid_in_reading_order() {
        let lopsided = split_of(
            SplitDir::Row,
            leaf("a"),
            split_of(
                SplitDir::Col,
                leaf("b"),
                split_of(
                    SplitDir::Row,
                    leaf("c"),
                    split_of(SplitDir::Col, leaf("d"), leaf("e"), 0.2),
                    0.8,
                ),
                0.3,
            ),
            0.15,
        );
        let tidy = preset_tree(&leaf_ids(&normalize_tree(
            &panes(&["a", "b", "c", "d", "e"]),
            Some(&lopsided),
        )));
        assert_eq!(leaf_ids(&tidy), ids(&["a", "b", "c", "d", "e"]));
        let layout = compute_layout(&tidy);
        for id in ["a", "b", "c"] {
            assert!(close(layout.rects[id].width, 1.0 / 3.0));
            assert!(close(layout.rects[id].height, 0.5));
        }
        for id in ["d", "e"] {
            assert!(close(layout.rects[id].width, 0.5));
            assert!(close(layout.rects[id].top, 0.5));
        }
    }

    // splitting and closing

    #[test]
    fn puts_the_new_pane_on_the_side_it_was_dropped_on() {
        let a = leaf("a");
        assert_eq!(
            insert_leaf(&a, "a", "n", DropZone::Right),
            row(leaf("a"), leaf("n"))
        );
        assert_eq!(
            insert_leaf(&a, "a", "n", DropZone::Left),
            row(leaf("n"), leaf("a"))
        );
        assert_eq!(
            insert_leaf(&a, "a", "n", DropZone::Top),
            col(leaf("n"), leaf("a"))
        );
        assert_eq!(
            insert_leaf(&a, "a", "n", DropZone::Bottom),
            col(leaf("a"), leaf("n"))
        );
    }

    #[test]
    fn splits_only_the_target_deep_in_the_tree() {
        let tree = row(leaf("a"), col(leaf("b"), leaf("c")));
        let next = insert_leaf(&tree, "c", "n", DropZone::Right);
        assert_eq!(leaf_ids(&next), ids(&["a", "b", "c", "n"]));
        // The untouched branch is the same (the TS shared it; here it is an
        // equal copy).
        let (SplitNode::Split { a: next_a, .. }, SplitNode::Split { a: tree_a, .. }) =
            (&next, &tree)
        else {
            panic!("splits expected");
        };
        assert_eq!(next_a, tree_a);
    }

    #[test]
    fn gives_a_closed_pane_s_space_to_its_sibling() {
        let tree = split_of(
            SplitDir::Row,
            leaf("a"),
            split_of(SplitDir::Col, leaf("b"), leaf("c"), 0.3),
            0.7,
        );
        assert_eq!(
            remove_leaf(&tree, "b"),
            Some(split_of(SplitDir::Row, leaf("a"), leaf("c"), 0.7))
        );
        assert_eq!(
            remove_leaf(&row(leaf("a"), leaf("b")), "a"),
            Some(leaf("b"))
        );
    }

    #[test]
    fn returns_none_once_the_last_pane_is_gone() {
        assert_eq!(remove_leaf(&leaf("a"), "a"), None);
    }

    #[test]
    fn returns_the_same_tree_when_the_pane_is_not_in_it() {
        let tree = row(leaf("a"), leaf("b"));
        assert_eq!(remove_leaf(&tree, "zz"), Some(tree));
    }

    #[test]
    fn keeps_the_arrangement_when_a_pane_is_renamed() {
        let tree = row(
            leaf("a"),
            split_of(SplitDir::Col, leaf("b"), leaf("c"), 0.3),
        );
        assert_eq!(
            rename_leaf(&tree, "b", "b2"),
            row(
                leaf("a"),
                split_of(SplitDir::Col, leaf("b2"), leaf("c"), 0.3)
            )
        );
    }

    // ratios

    fn ratio_tree() -> SplitNode {
        row(leaf("a"), col(leaf("b"), leaf("c")))
    }

    #[test]
    fn reads_and_writes_the_ratio_at_a_path() {
        let tree = ratio_tree();
        let next = with_ratio(&tree, &[Side::B], 0.25);
        assert_eq!(ratio_at(&next, &[Side::B]), 0.25);
        assert_eq!(ratio_at(&next, &[]), 0.5);
        assert_eq!(ratio_at(&tree, &[Side::B]), 0.5);
    }

    #[test]
    fn follows_the_ratio_in_the_rectangles() {
        let layout = compute_layout(&with_ratio(&ratio_tree(), &[], 0.25));
        assert!(close(layout.rects["a"].width, 0.25));
        assert!(close(layout.rects["b"].left, 0.25));
        assert!(close(layout.rects["b"].width, 0.75));
    }

    #[test]
    fn clamps_a_ratio_that_would_crush_a_pane() {
        let layout = compute_layout(&split_of(SplitDir::Row, leaf("a"), leaf("b"), 0.0));
        assert!(close(layout.rects["a"].width, 0.05));
        let layout = compute_layout(&split_of(SplitDir::Row, leaf("a"), leaf("b"), 1.0));
        assert!(close(layout.rects["b"].width, 0.05));
    }

    #[test]
    fn has_one_handle_per_split_keyed_by_its_path() {
        let layout = compute_layout(&ratio_tree());
        let mut keys: Vec<&str> = layout.handles.iter().map(|h| h.key.as_str()).collect();
        keys.sort();
        assert_eq!(keys, ["b", "root"]);
        let root = layout.handles.iter().find(|h| h.key == "root").unwrap();
        assert_eq!(root.dir, SplitDir::Row);
        assert_eq!(
            root.rect,
            Rect {
                left: 0.5,
                top: 0.0,
                width: 0.0,
                height: 1.0
            }
        );
    }

    #[test]
    fn names_the_panes_on_either_side_of_each_handle_for_its_label() {
        let layout = compute_layout(&ratio_tree());
        let by_key: HashMap<&str, (Vec<PaneId>, Vec<PaneId>)> = layout
            .handles
            .iter()
            .map(|h| (h.key.as_str(), (h.before.clone(), h.after.clone())))
            .collect();
        assert_eq!(by_key["root"], (ids(&["a"]), ids(&["b", "c"])));
        assert_eq!(by_key["b"], (ids(&["b"]), ids(&["c"])));
        assert_eq!(by_key.len(), 2);
    }

    // moving panes

    fn moving_tree() -> SplitNode {
        row(
            leaf("a"),
            split_of(SplitDir::Col, leaf("b"), leaf("c"), 0.3),
        )
    }

    #[test]
    fn swaps_two_panes_on_a_drop_in_the_middle_keeping_the_shape() {
        let tree = moving_tree();
        assert_eq!(
            move_leaf(&tree, "a", "c", DropZone::Center),
            row(
                leaf("c"),
                split_of(SplitDir::Col, leaf("b"), leaf("a"), 0.3)
            )
        );
        assert_eq!(
            swap_leaves(&tree, "a", "c"),
            move_leaf(&tree, "a", "c", DropZone::Center)
        );
    }

    #[test]
    fn re_splits_the_target_on_a_drop_on_an_edge() {
        let next = move_leaf(&moving_tree(), "a", "c", DropZone::Bottom);
        assert_eq!(leaf_ids(&next), ids(&["b", "c", "a"]));
        let layout = compute_layout(&next);
        assert!(layout.rects["a"].top > layout.rects["c"].top);
    }

    #[test]
    fn does_nothing_when_a_pane_is_dropped_on_itself() {
        let tree = moving_tree();
        assert_eq!(move_leaf(&tree, "b", "b", DropZone::Left), tree);
    }

    // normalizeTree

    #[test]
    fn returns_a_tree_that_still_matches_untouched_ratios_included() {
        let tree = split_of(SplitDir::Row, leaf("a"), leaf("b"), 0.8);
        assert_eq!(normalize_tree(&panes(&["a", "b"]), Some(&tree)), tree);
    }

    #[test]
    fn builds_the_preset_when_there_is_no_tree() {
        assert_eq!(
            normalize_tree(&panes(&["a", "b"]), None),
            preset_tree(&ids(&["a", "b"]))
        );
    }

    #[test]
    fn maps_a_legacy_tree_of_pane_indexes_onto_ids() {
        let legacy = json!({
            "type": "split", "dir": "col", "ratio": 0.6,
            "a": {"type": "pane", "index": 1},
            "b": {"type": "pane", "index": 0}
        });
        let list = panes(&["a", "b"]);
        let ids: Vec<PaneId> = list.iter().map(|p| p.id.clone()).collect();
        let migrated = migrate(&legacy, &ids);
        assert_eq!(
            normalize_tree(&list, migrated.as_ref()),
            split_of(SplitDir::Col, leaf("b"), leaf("a"), 0.6)
        );
    }

    #[test]
    fn drops_panes_that_are_gone_and_appends_new_ones_in_the_user_s_order() {
        let tree = row(leaf("c"), row(leaf("gone"), leaf("a")));
        assert_eq!(
            leaf_ids(&normalize_tree(&panes(&["a", "b", "c"]), Some(&tree))),
            ids(&["c", "a", "b"])
        );
    }

    #[test]
    fn never_lists_a_pane_twice() {
        let tree = row(leaf("a"), leaf("a"));
        assert_eq!(
            leaf_ids(&normalize_tree(&panes(&["a", "b"]), Some(&tree))),
            ids(&["a", "b"])
        );
    }

    #[test]
    fn is_an_empty_leaf_when_there_are_no_panes() {
        assert_eq!(normalize_tree(&[], None), leaf(""));
    }

    #[test]
    fn evens_out_a_ratio_the_ts_would_not_have_kept() {
        let tree = split_of(SplitDir::Row, leaf("a"), leaf("b"), 1.5);
        assert_eq!(
            normalize_tree(&panes(&["a", "b"]), Some(&tree)),
            row(leaf("a"), leaf("b"))
        );
    }

    // neighborOf

    fn grid() -> SplitNode {
        preset_tree(&ids(&["a", "b", "c", "d"]))
    }

    #[test]
    fn finds_the_pane_on_each_side() {
        let grid = grid();
        assert_eq!(
            neighbor_of(&grid, "a", Direction::Right).as_deref(),
            Some("b")
        );
        assert_eq!(
            neighbor_of(&grid, "a", Direction::Down).as_deref(),
            Some("c")
        );
        assert_eq!(
            neighbor_of(&grid, "d", Direction::Left).as_deref(),
            Some("c")
        );
        assert_eq!(neighbor_of(&grid, "d", Direction::Up).as_deref(), Some("b"));
    }

    #[test]
    fn is_none_at_the_edge_of_the_grid() {
        assert_eq!(neighbor_of(&grid(), "a", Direction::Left), None);
        assert_eq!(neighbor_of(&grid(), "a", Direction::Up), None);
    }

    #[test]
    fn reads_the_screen_not_the_tree() {
        let tree = row(
            leaf("a"),
            split_of(SplitDir::Col, leaf("b"), leaf("c"), 0.3),
        );
        assert_eq!(
            neighbor_of(&tree, "a", Direction::Right).as_deref(),
            Some("c")
        );
        assert_eq!(
            neighbor_of(&tree, "c", Direction::Left).as_deref(),
            Some("a")
        );
        assert_eq!(neighbor_of(&tree, "c", Direction::Up).as_deref(), Some("b"));
    }

    #[test]
    fn is_none_for_a_pane_that_is_not_in_the_tree() {
        assert_eq!(neighbor_of(&grid(), "zz", Direction::Left), None);
    }

    // drop zones

    #[test]
    fn swaps_in_the_middle_and_splits_towards_the_nearest_edge() {
        let bx = Rect {
            left: 0.0,
            top: 0.0,
            width: 100.0,
            height: 100.0,
        };
        assert_eq!(zone_at(bx, 50.0, 50.0), DropZone::Center);
        assert_eq!(zone_at(bx, 5.0, 50.0), DropZone::Left);
        assert_eq!(zone_at(bx, 95.0, 50.0), DropZone::Right);
        assert_eq!(zone_at(bx, 50.0, 5.0), DropZone::Top);
        assert_eq!(zone_at(bx, 50.0, 95.0), DropZone::Bottom);
    }

    #[test]
    fn previews_the_half_a_dropped_pane_would_take() {
        let rect = Rect::FULL;
        assert_eq!(
            zone_rect(rect, DropZone::Right),
            Rect {
                left: 0.5,
                top: 0.0,
                width: 0.5,
                height: 1.0
            }
        );
        assert_eq!(
            zone_rect(rect, DropZone::Bottom),
            Rect {
                left: 0.0,
                top: 0.5,
                width: 1.0,
                height: 0.5
            }
        );
        assert_eq!(zone_rect(rect, DropZone::Center), rect);
    }

    // presets

    fn four() -> Vec<Pane> {
        panes(&["a", "b", "c", "d"])
    }
    fn moved() -> SplitNode {
        row(col(leaf("d"), leaf("a")), col(leaf("b"), leaf("c")))
    }

    #[test]
    fn reads_the_panes_in_their_order_on_screen_not_their_order_of_creation() {
        let four = four();
        assert_eq!(
            of(&reading_order(&four, Some(&moved()))),
            ["d", "a", "b", "c"]
        );
    }

    #[test]
    fn closes_the_panes_read_last_which_are_the_ones_it_names_before_doing_it() {
        let four = four();
        assert_eq!(of(&closed_by_preset(&four, Some(&moved()), 2)), ["b", "c"]);
        assert_eq!(
            of(&closed_by_preset(&four, Some(&moved()), 1)),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn closes_nothing_when_it_keeps_or_grows_the_grid() {
        let four = four();
        assert!(closed_by_preset(&four, Some(&moved()), 4).is_empty());
        assert!(closed_by_preset(&four, Some(&moved()), 8).is_empty());
    }

    #[test]
    fn keeps_at_least_one_pane_and_at_most_the_grid_s_maximum() {
        assert_eq!(preset_size(0), 1);
        assert_eq!(preset_size(99), 16);
        let four = four();
        assert_eq!(of(&closed_by_preset(&four, None, 0)), ["b", "c", "d"]);
    }

    #[test]
    fn splits_along_the_longer_side() {
        assert_eq!(preferred_dir(None), SplitDir::Row);
        assert_eq!(preferred_dir(Some((800.0, 400.0))), SplitDir::Row);
        assert_eq!(preferred_dir(Some((400.0, 400.0))), SplitDir::Col);
    }

    // ---- Hangar's own layout for agents' panes

    fn numbered(n: usize) -> Vec<PaneId> {
        (0..n).map(|i| format!("p{i}")).collect()
    }

    fn area(rect: &Rect) -> f64 {
        rect.width * rect.height
    }

    #[test]
    fn the_lead_is_the_biggest_pane_whatever_the_number_of_others() {
        for n in 2..=MAX_PANES {
            let order = numbered(n);
            let tree = arranged_tree(&order, Some("p0"));
            let mut placed = leaf_ids(&tree);
            placed.sort();
            let mut wanted = order.clone();
            wanted.sort();
            assert_eq!(placed, wanted, "{n} panes, each once");
            let grid = compute_layout(&tree);
            let lead = area(&grid.rects["p0"]);
            for id in order.iter().skip(1) {
                assert!(
                    lead > area(&grid.rects[id]),
                    "{n} panes: p0 is not bigger than {id}"
                );
            }
            // On the left, the whole height.
            let rect = grid.rects["p0"];
            assert_eq!((rect.left, rect.top, rect.height), (0.0, 0.0, 1.0));
        }
    }

    #[test]
    fn the_others_share_what_is_left_evenly() {
        for n in 2..=MAX_PANES {
            let order = numbered(n);
            let grid = compute_layout(&arranged_tree(&order, Some("p0")));
            let areas: Vec<f64> = order
                .iter()
                .skip(1)
                .map(|id| area(&grid.rects[id]))
                .collect();
            let (min, max) = areas
                .iter()
                .fold((f64::MAX, 0.0_f64), |(lo, hi), a| (lo.min(*a), hi.max(*a)));
            // Rows differ by one pane at most: two panes beside one at worst.
            assert!(max <= min * 2.0 + 1e-9, "{n} panes: {areas:?}");
        }
    }

    #[test]
    fn the_lead_moves_to_the_left_and_the_others_keep_their_order() {
        let order = numbered(4);
        let tree = arranged_tree(&order, Some("p2"));
        assert_eq!(leaf_ids(&tree), ["p2", "p0", "p1", "p3"]);
    }

    #[test]
    fn without_a_lead_it_is_the_even_grid() {
        let order = numbered(5);
        assert_eq!(arranged_tree(&order, None), preset_tree(&order));
        assert_eq!(arranged_tree(&order, Some("gone")), preset_tree(&order));
        assert_eq!(arranged_tree(&numbered(1), Some("p0")), leaf("p0"));
    }
}
