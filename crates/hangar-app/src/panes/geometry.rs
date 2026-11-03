//! The part of `src/lib/layout.ts` the grid draws with: the tree turned into
//! rectangles and boundaries (`computeLayout`), the ratio at a boundary
//! (`withRatio`, `ratioAt`), the drop zones of a pane drag (`zoneAt`,
//! `zoneRect`), and the repair that makes a stored tree cover exactly the
//! panes it should (`normalizeTree`, `presetTree`), with their cases of
//! layout.test.ts.
//!
//! The rest of layout.ts (inserting, removing, swapping and moving leaves,
//! `neighborOf`, presets as store actions) is the store's, which the shell
//! ports in `state/layout.rs` (P0b). This module was written before that one
//! landed, so the grid could draw; once `state::layout` has the same
//! functions, these should become calls to it rather than a second copy.

use std::collections::HashSet;

use crate::state::layout::leaf_ids;
use crate::state::types::{DropZone, Pane, PaneId, SplitDir, SplitNode};

/// A turn in the tree on the way to a split: `a` is left or top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    A,
    B,
}

/// The trail of turns leading to a split node (`Path`).
pub type Path = Vec<Turn>;

/// The handle's key, as layout.ts wrote it: the turns joined, or `root`.
pub fn path_key(path: &[Turn]) -> String {
    if path.is_empty() {
        return "root".into();
    }
    path.iter()
        .map(|turn| match turn {
            Turn::A => 'a',
            Turn::B => 'b',
        })
        .collect()
}

/// Fractions of the grid, 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const FULL: Rect = Rect {
        left: 0.0,
        top: 0.0,
        width: 1.0,
        height: 1.0,
    };
}

/// A boundary between the two sides of a split.
#[derive(Debug, Clone, PartialEq)]
pub struct Handle {
    pub path: Path,
    pub dir: SplitDir,
    /// The boundary line: zero width for a row split, zero height for a
    /// column.
    pub rect: Rect,
    /// The area the split owns, to turn a pointer position into a ratio.
    pub bounds: Rect,
    /// The panes on either side, left or above, right or below, in reading
    /// order.
    pub before: Vec<PaneId>,
    pub after: Vec<PaneId>,
}

/// The tree as flat rectangles (`GridLayout`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayout {
    /// In reading order.
    pub rects: Vec<(PaneId, Rect)>,
    pub handles: Vec<Handle>,
}

impl GridLayout {
    pub fn rect(&self, id: &str) -> Option<Rect> {
        self.rects
            .iter()
            .find(|(pane, _)| pane == id)
            .map(|(_, rect)| *rect)
    }

    /// Pane ids in reading order: what drives the numbering and Ctrl+N.
    pub fn order(&self) -> impl Iterator<Item = &PaneId> {
        self.rects.iter().map(|(id, _)| id)
    }
}

fn leaf(id: &str) -> SplitNode {
    SplitNode::Pane { id: id.to_owned() }
}

/// A list of nodes folded into a balanced binary split with even shares.
fn fold(dir: SplitDir, mut nodes: Vec<SplitNode>) -> SplitNode {
    if nodes.len() == 1 {
        return nodes.pop().expect("one node");
    }
    let len = nodes.len();
    let mid = len.div_ceil(2);
    let b = nodes.split_off(mid);
    SplitNode::Split {
        dir,
        ratio: mid as f64 / len as f64,
        a: Box::new(fold(dir, nodes)),
        b: Box::new(fold(dir, b)),
    }
}

/// An even grid for a set of panes: one wide row up to two panes, then as
/// many columns as rows allow; the familiar 1, 2, 2×2, 4×2 shapes
/// (`presetTree`).
pub fn preset_tree(ids: &[PaneId]) -> SplitNode {
    if ids.len() <= 1 {
        return leaf(ids.first().map_or("", String::as_str));
    }
    let n = ids.len();
    let rows = ((n as f64 / 2.0).sqrt().ceil() as usize).max(1);
    let columns = n.div_ceil(rows);
    let bands = ids
        .chunks(columns)
        .map(|band| fold(SplitDir::Row, band.iter().map(|id| leaf(id)).collect()))
        .collect();
    fold(SplitDir::Col, bands)
}

/// The tree with one ratio replaced (`withRatio`).
pub fn with_ratio(node: &SplitNode, path: &[Turn], ratio: f64) -> SplitNode {
    match node {
        SplitNode::Pane { .. } => node.clone(),
        SplitNode::Split {
            dir,
            ratio: old,
            a,
            b,
        } => match path.split_first() {
            None => SplitNode::Split {
                dir: *dir,
                ratio,
                a: a.clone(),
                b: b.clone(),
            },
            Some((Turn::A, rest)) => SplitNode::Split {
                dir: *dir,
                ratio: *old,
                a: Box::new(with_ratio(a, rest, ratio)),
                b: b.clone(),
            },
            Some((Turn::B, rest)) => SplitNode::Split {
                dir: *dir,
                ratio: *old,
                a: a.clone(),
                b: Box::new(with_ratio(b, rest, ratio)),
            },
        },
    }
}

/// The ratio at `path`; a leaf answers 0.5 (`ratioAt`).
pub fn ratio_at(node: &SplitNode, path: &[Turn]) -> f64 {
    match node {
        SplitNode::Pane { .. } => 0.5,
        SplitNode::Split { ratio, a, b, .. } => match path.split_first() {
            None => *ratio,
            Some((Turn::A, rest)) => ratio_at(a, rest),
            Some((Turn::B, rest)) => ratio_at(b, rest),
        },
    }
}

/// A tree that covers exactly the current panes. One that still matches is
/// returned untouched, so its ratios survive; anything else is rebuilt as a
/// preset, keeping the order the person had (`normalizeTree`).
///
/// The migration of legacy trees of pane indexes is the reader's
/// (`state::read_state`): by the time a tree is a `SplitNode`, its leaves are
/// ids.
pub fn normalize_tree(panes: &[Pane], tree: Option<&SplitNode>) -> SplitNode {
    let ids: Vec<PaneId> = panes.iter().map(|pane| pane.id.clone()).collect();
    if ids.is_empty() {
        return leaf("");
    }
    let Some(tree) = tree.map(sanitize) else {
        return preset_tree(&ids);
    };
    let known: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let leaves = leaf_ids(&tree);
    let mut seen = HashSet::new();
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

/// A ratio outside (0, 1) reads as 0.5, as `migrate` made it.
fn sanitize(node: &SplitNode) -> SplitNode {
    match node {
        SplitNode::Pane { .. } => node.clone(),
        SplitNode::Split { dir, ratio, a, b } => SplitNode::Split {
            dir: *dir,
            ratio: if ratio.is_finite() && *ratio > 0.0 && *ratio < 1.0 {
                *ratio
            } else {
                0.5
            },
            a: Box::new(sanitize(a)),
            b: Box::new(sanitize(b)),
        },
    }
}

/// The tree as flat rectangles plus the boundaries between them
/// (`computeLayout`). Ratios are clamped to 5–95 %.
pub fn compute_layout(tree: &SplitNode) -> GridLayout {
    let mut out = GridLayout::default();
    walk(tree, Rect::FULL, &mut Vec::new(), &mut out);
    out
}

fn walk(node: &SplitNode, rect: Rect, path: &mut Path, out: &mut GridLayout) {
    let SplitNode::Split { dir, ratio, a, b } = node else {
        if let SplitNode::Pane { id } = node {
            out.rects.push((id.clone(), rect));
        }
        return;
    };
    let ratio = ratio.clamp(0.05, 0.95) as f32;
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
    path.push(Turn::A);
    walk(a, first, path, out);
    path.pop();
    path.push(Turn::B);
    walk(b, second, path, out);
    path.pop();
    out.handles.push(Handle {
        path: path.clone(),
        dir: *dir,
        rect: line,
        bounds: rect,
        before: leaf_ids(a),
        after: leaf_ids(b),
    });
}

/// Half of a pane, as previewed while another one is dragged onto it
/// (`zoneRect`).
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

/// Which side of a pane at `bounds` (in pixels) a pointer at `(x, y)` is
/// closest to; the middle third means "swap" (`zoneAt`). Ties go left,
/// right, top, bottom, in that order.
pub fn zone_at(bounds: iced::Rectangle, x: f32, y: f32) -> DropZone {
    let dx = (x - bounds.x) / bounds.width.max(1.0);
    let dy = (y - bounds.y) / bounds.height.max(1.0);
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

/// Splits along the longer side, so panes stay as square as they can
/// (`preferredDir`, read from the pane's size on screen).
pub fn preferred_dir(width: f32, height: f32) -> SplitDir {
    if width >= height * 1.15 {
        SplitDir::Row
    } else {
        SplitDir::Col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(dir: SplitDir, a: SplitNode, b: SplitNode, ratio: f64) -> SplitNode {
        SplitNode::Split {
            dir,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    fn row(a: SplitNode, b: SplitNode) -> SplitNode {
        split(SplitDir::Row, a, b, 0.5)
    }

    fn col(a: SplitNode, b: SplitNode) -> SplitNode {
        split(SplitDir::Col, a, b, 0.5)
    }

    fn ids(names: &[&str]) -> Vec<PaneId> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn panes(names: &[&str]) -> Vec<Pane> {
        names
            .iter()
            .map(|id| Pane {
                id: id.to_string(),
                ..Pane::default()
            })
            .collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn a_preset_of_one_pane_is_a_single_leaf() {
        assert_eq!(preset_tree(&ids(&["a"])), leaf("a"));
    }

    #[test]
    fn a_preset_puts_two_panes_side_by_side() {
        assert_eq!(preset_tree(&ids(&["a", "b"])), row(leaf("a"), leaf("b")));
    }

    #[test]
    fn a_preset_lays_four_panes_out_as_an_even_2x2() {
        let layout = compute_layout(&preset_tree(&ids(&["a", "b", "c", "d"])));
        assert_eq!(
            layout.order().cloned().collect::<Vec<_>>(),
            ids(&["a", "b", "c", "d"])
        );
        assert_eq!(
            layout.rect("a").unwrap(),
            Rect {
                left: 0.0,
                top: 0.0,
                width: 0.5,
                height: 0.5
            }
        );
        assert_eq!(
            layout.rect("d").unwrap(),
            Rect {
                left: 0.5,
                top: 0.5,
                width: 0.5,
                height: 0.5
            }
        );
    }

    #[test]
    fn a_preset_lays_eight_panes_out_as_4x2() {
        let names = ["1", "2", "3", "4", "5", "6", "7", "8"];
        let layout = compute_layout(&preset_tree(&ids(&names)));
        for id in names {
            let rect = layout.rect(id).unwrap();
            assert!(close(rect.width, 0.25) && close(rect.height, 0.5));
        }
        assert!(close(layout.rect("5").unwrap().top, 0.5));
    }

    #[test]
    fn the_odd_pane_out_takes_the_whole_last_row() {
        let layout = compute_layout(&preset_tree(&ids(&["a", "b", "c"])));
        assert_eq!(
            layout.rect("c").unwrap(),
            Rect {
                left: 0.0,
                top: 0.5,
                width: 1.0,
                height: 0.5
            }
        );
    }

    #[test]
    fn a_lopsided_tree_tidies_into_an_even_grid_in_reading_order() {
        let lopsided = split(
            SplitDir::Row,
            leaf("a"),
            split(
                SplitDir::Col,
                leaf("b"),
                split(
                    SplitDir::Row,
                    leaf("c"),
                    split(SplitDir::Col, leaf("d"), leaf("e"), 0.2),
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
            let rect = layout.rect(id).unwrap();
            assert!(close(rect.width, 1.0 / 3.0) && close(rect.height, 0.5));
        }
        for id in ["d", "e"] {
            let rect = layout.rect(id).unwrap();
            assert!(close(rect.width, 0.5) && close(rect.top, 0.5));
        }
    }

    #[test]
    fn reads_and_writes_the_ratio_at_a_path() {
        let tree = row(leaf("a"), col(leaf("b"), leaf("c")));
        assert_eq!(ratio_at(&tree, &[]), 0.5);
        let next = with_ratio(&tree, &[Turn::B], 0.25);
        assert_eq!(ratio_at(&next, &[Turn::B]), 0.25);
        assert_eq!(ratio_at(&next, &[]), 0.5);
        // A leaf has no ratio of its own.
        assert_eq!(ratio_at(&tree, &[Turn::A]), 0.5);
    }

    #[test]
    fn the_rectangles_follow_the_ratio() {
        let tree = with_ratio(&row(leaf("a"), col(leaf("b"), leaf("c"))), &[], 0.25);
        let layout = compute_layout(&tree);
        assert!(close(layout.rect("a").unwrap().width, 0.25));
        let b = layout.rect("b").unwrap();
        assert!(close(b.left, 0.25) && close(b.width, 0.75) && close(b.height, 0.5));
    }

    #[test]
    fn a_ratio_that_would_crush_a_pane_is_clamped() {
        let layout = compute_layout(&split(SplitDir::Row, leaf("a"), leaf("b"), 0.0));
        assert!(close(layout.rect("a").unwrap().width, 0.05));
        let layout = compute_layout(&split(SplitDir::Row, leaf("a"), leaf("b"), 1.0));
        assert!(close(layout.rect("b").unwrap().width, 0.05));
    }

    #[test]
    fn there_is_one_handle_per_split_keyed_by_its_path() {
        let tree = row(leaf("a"), col(leaf("b"), leaf("c")));
        let layout = compute_layout(&tree);
        let mut keys: Vec<String> = layout.handles.iter().map(|h| path_key(&h.path)).collect();
        keys.sort();
        assert_eq!(keys, ["b", "root"]);
        let root = layout.handles.iter().find(|h| h.path.is_empty()).unwrap();
        assert_eq!(root.dir, SplitDir::Row);
        assert!(close(root.rect.left, 0.5) && close(root.rect.width, 0.0));
    }

    #[test]
    fn a_handle_names_the_panes_on_either_side() {
        let tree = row(leaf("a"), col(leaf("b"), leaf("c")));
        let layout = compute_layout(&tree);
        let root = layout.handles.iter().find(|h| h.path.is_empty()).unwrap();
        assert_eq!(root.before, ids(&["a"]));
        assert_eq!(root.after, ids(&["b", "c"]));
        let inner = layout.handles.iter().find(|h| h.path == [Turn::B]).unwrap();
        assert_eq!(inner.before, ids(&["b"]));
        assert_eq!(inner.after, ids(&["c"]));
    }

    #[test]
    fn a_tree_that_still_matches_is_returned_untouched() {
        let tree = split(SplitDir::Row, leaf("a"), leaf("b"), 0.8);
        assert_eq!(normalize_tree(&panes(&["a", "b"]), Some(&tree)), tree);
    }

    #[test]
    fn without_a_tree_the_preset_is_built() {
        assert_eq!(
            normalize_tree(&panes(&["a", "b"]), None),
            row(leaf("a"), leaf("b"))
        );
    }

    #[test]
    fn panes_gone_are_dropped_and_new_ones_appended_in_the_persons_order() {
        let tree = row(leaf("c"), row(leaf("gone"), leaf("a")));
        assert_eq!(
            normalize_tree(&panes(&["a", "b", "c"]), Some(&tree)),
            preset_tree(&ids(&["c", "a", "b"]))
        );
    }

    #[test]
    fn a_pane_is_never_listed_twice() {
        let tree = row(leaf("a"), leaf("a"));
        assert_eq!(
            normalize_tree(&panes(&["a", "b"]), Some(&tree)),
            preset_tree(&ids(&["a", "b"]))
        );
    }

    #[test]
    fn no_panes_is_an_empty_leaf() {
        assert_eq!(normalize_tree(&[], None), leaf(""));
    }

    #[test]
    fn a_ratio_out_of_range_reads_as_even() {
        let tree = split(SplitDir::Row, leaf("a"), leaf("b"), 7.0);
        assert_eq!(
            normalize_tree(&panes(&["a", "b"]), Some(&tree)),
            row(leaf("a"), leaf("b"))
        );
    }

    #[test]
    fn the_middle_third_swaps_and_the_edges_split() {
        let bounds = iced::Rectangle::new(
            iced::Point::new(100.0, 100.0),
            iced::Size::new(300.0, 300.0),
        );
        assert_eq!(zone_at(bounds, 250.0, 250.0), DropZone::Center);
        assert_eq!(zone_at(bounds, 110.0, 250.0), DropZone::Left);
        assert_eq!(zone_at(bounds, 390.0, 250.0), DropZone::Right);
        assert_eq!(zone_at(bounds, 250.0, 110.0), DropZone::Top);
        assert_eq!(zone_at(bounds, 250.0, 390.0), DropZone::Bottom);
        // The corner equally near two edges goes to the first in the order.
        assert_eq!(zone_at(bounds, 100.0, 100.0), DropZone::Left);
    }

    #[test]
    fn a_zone_previews_half_of_the_pane() {
        let rect = Rect {
            left: 0.5,
            top: 0.0,
            width: 0.5,
            height: 1.0,
        };
        assert_eq!(
            zone_rect(rect, DropZone::Right),
            Rect {
                left: 0.75,
                top: 0.0,
                width: 0.25,
                height: 1.0
            }
        );
        assert_eq!(
            zone_rect(rect, DropZone::Bottom),
            Rect {
                left: 0.5,
                top: 0.5,
                width: 0.5,
                height: 0.5
            }
        );
        assert_eq!(zone_rect(rect, DropZone::Center), rect);
    }

    #[test]
    fn a_split_goes_along_the_longer_side() {
        assert_eq!(preferred_dir(800.0, 400.0), SplitDir::Row);
        assert_eq!(preferred_dir(400.0, 800.0), SplitDir::Col);
        assert_eq!(preferred_dir(114.0, 100.0), SplitDir::Col);
    }
}
