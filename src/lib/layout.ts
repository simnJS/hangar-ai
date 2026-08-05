import type { Pane, SplitNode } from "../types";

/**
 * Pure layout algebra for the pane grid.
 *
 * The split tree is the single source of truth for the arrangement; panes are
 * referenced by id, never by position, so a pane can be closed or moved without
 * disturbing the identity of the others (and therefore without killing PTYs).
 *
 * Rectangles are expressed as fractions of the grid, which lets the grid render
 * every pane as an absolutely positioned sibling: React never reparents a
 * terminal when the arrangement changes, it only rewrites four numbers.
 */

export type Zone = "left" | "right" | "top" | "bottom" | "center";

/** Trail of 'a'/'b' turns leading to a split node. */
export type Path = ("a" | "b")[];

/** Fractions of the grid, 0..1. */
export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Handle {
  key: string;
  path: Path;
  dir: "row" | "col";
  /** The boundary line: zero width for a row split, zero height for a column. */
  rect: Rect;
  /** Area owned by the split, used to turn a pointer position into a ratio. */
  bounds: Rect;
}

export interface GridLayout {
  rects: Record<string, Rect>;
  handles: Handle[];
  /** Pane ids in reading order — drives numbering and Ctrl+N. */
  order: string[];
}

/** A pane never shrinks below this, so it stays usable while dragging. */
export const MIN_PANE_PX = 150;

/** Beyond this the panes are too small to type in, whatever the screen. */
export const MAX_PANES = 16;

const FULL: Rect = { left: 0, top: 0, width: 1, height: 1 };

const leafOf = (id: string): SplitNode => ({ type: "pane", id });

export const pathKey = (path: Path) => path.join("") || "root";

export function leafIds(node: SplitNode, out: string[] = []): string[] {
  if (node.type === "pane") out.push(node.id);
  else {
    leafIds(node.a, out);
    leafIds(node.b, out);
  }
  return out;
}

/** Folds a list of nodes into a balanced binary split with even shares. */
function fold(dir: "row" | "col", nodes: SplitNode[]): SplitNode {
  if (nodes.length === 1) return nodes[0];
  const mid = Math.ceil(nodes.length / 2);
  return {
    type: "split",
    dir,
    ratio: mid / nodes.length,
    a: fold(dir, nodes.slice(0, mid)),
    b: fold(dir, nodes.slice(mid)),
  };
}

/**
 * Even grid for a given set of panes: one wide row up to two panes, then as
 * many columns as rows allow. Reproduces the familiar 1 / 2 / 2x2 / 4x2 shapes.
 */
export function presetTree(ids: string[]): SplitNode {
  if (ids.length <= 1) return leafOf(ids[0] ?? "");
  const rows = Math.max(1, Math.ceil(Math.sqrt(ids.length / 2)));
  const columns = Math.ceil(ids.length / rows);
  const bands: SplitNode[] = [];
  for (let i = 0; i < ids.length; i += columns) {
    bands.push(fold("row", ids.slice(i, i + columns).map(leafOf)));
  }
  return fold("col", bands);
}

/** Rebuilds the tree with one ratio replaced. */
export function withRatio(node: SplitNode, path: Path, ratio: number): SplitNode {
  if (node.type === "pane") return node;
  if (path.length === 0) return { ...node, ratio };
  const [head, ...rest] = path;
  return head === "a"
    ? { ...node, a: withRatio(node.a, rest, ratio) }
    : { ...node, b: withRatio(node.b, rest, ratio) };
}

export function ratioAt(node: SplitNode, path: Path): number {
  if (node.type === "pane") return 0.5;
  if (path.length === 0) return node.ratio;
  const [head, ...rest] = path;
  return ratioAt(head === "a" ? node.a : node.b, rest);
}

/** Drops a pane and lets its sibling take the whole space. */
export function removeLeaf(node: SplitNode, id: string): SplitNode | null {
  if (node.type === "pane") return node.id === id ? null : node;
  const a = removeLeaf(node.a, id);
  const b = removeLeaf(node.b, id);
  if (!a) return b;
  if (!b) return a;
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

/** Keeps the arrangement when a pane is respawned under a fresh id. */
export function renameLeaf(node: SplitNode, from: string, to: string): SplitNode {
  if (node.type === "pane") return node.id === from ? leafOf(to) : node;
  const a = renameLeaf(node.a, from, to);
  const b = renameLeaf(node.b, from, to);
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

function replaceLeaf(
  node: SplitNode,
  id: string,
  make: (leaf: SplitNode) => SplitNode,
): SplitNode {
  if (node.type === "pane") return node.id === id ? make(node) : node;
  const a = replaceLeaf(node.a, id, make);
  const b = replaceLeaf(node.b, id, make);
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

/** Splits `targetId` in two and puts `newId` on the requested side. */
export function insertLeaf(
  node: SplitNode,
  targetId: string,
  newId: string,
  zone: Exclude<Zone, "center">,
): SplitNode {
  const dir = zone === "left" || zone === "right" ? "row" : "col";
  const before = zone === "left" || zone === "top";
  return replaceLeaf(node, targetId, (target) => ({
    type: "split",
    dir,
    ratio: 0.5,
    a: before ? leafOf(newId) : target,
    b: before ? target : leafOf(newId),
  }));
}

/** Exchanges two panes without touching the shape of the tree. */
export function swapLeaves(node: SplitNode, first: string, second: string): SplitNode {
  if (node.type === "pane") {
    if (node.id === first) return leafOf(second);
    if (node.id === second) return leafOf(first);
    return node;
  }
  const a = swapLeaves(node.a, first, second);
  const b = swapLeaves(node.b, first, second);
  return a === node.a && b === node.b ? node : { ...node, a, b };
}

/** Drop on the middle of a pane swaps the two; drop on an edge re-splits it. */
export function moveLeaf(
  node: SplitNode,
  dragId: string,
  targetId: string,
  zone: Zone,
): SplitNode {
  if (dragId === targetId) return node;
  if (zone === "center") return swapLeaves(node, dragId, targetId);
  const without = removeLeaf(node, dragId);
  if (!without) return node;
  return insertLeaf(without, targetId, dragId, zone);
}

/** Legacy trees stored a pane index; map it back onto the current pane ids. */
function migrate(node: unknown, ids: string[]): SplitNode | null {
  if (!node || typeof node !== "object") return null;
  const raw = node as Record<string, unknown>;

  if (raw.type === "pane") {
    if (typeof raw.id === "string") return leafOf(raw.id);
    if (typeof raw.index === "number") {
      const id = ids[raw.index];
      return id ? leafOf(id) : null;
    }
    return null;
  }

  if (raw.type === "split") {
    const a = migrate(raw.a, ids);
    const b = migrate(raw.b, ids);
    if (!a) return b;
    if (!b) return a;
    const ratio =
      typeof raw.ratio === "number" && raw.ratio > 0 && raw.ratio < 1 ? raw.ratio : 0.5;
    return { type: "split", dir: raw.dir === "col" ? "col" : "row", ratio, a, b };
  }

  return null;
}

/**
 * Guarantees the tree covers exactly the current panes. A stored tree that
 * still matches is returned untouched, so ratios survive; anything else is
 * repaired, keeping the order the user had.
 */
export function normalizeTree(panes: Pane[], tree?: SplitNode | null): SplitNode {
  const ids = panes.map((pane) => pane.id);
  if (!ids.length) return leafOf("");

  const migrated = tree ? migrate(tree, ids) : null;
  if (!migrated) return presetTree(ids);

  const known = new Set(ids);
  const seen = new Set<string>();
  const leaves = leafIds(migrated);
  const kept: string[] = [];
  for (const id of leaves) {
    if (!known.has(id) || seen.has(id)) continue;
    seen.add(id);
    kept.push(id);
  }

  if (kept.length === leaves.length && kept.length === ids.length) return migrated;
  return presetTree([...kept, ...ids.filter((id) => !seen.has(id))]);
}

/** Turns the tree into flat rectangles plus the boundaries between them. */
export function computeLayout(tree: SplitNode): GridLayout {
  const rects: Record<string, Rect> = {};
  const handles: Handle[] = [];
  const order: string[] = [];

  const walk = (node: SplitNode, rect: Rect, path: Path) => {
    if (node.type === "pane") {
      rects[node.id] = rect;
      order.push(node.id);
      return;
    }

    const ratio = Math.min(0.95, Math.max(0.05, node.ratio));

    if (node.dir === "row") {
      const width = rect.width * ratio;
      walk(node.a, { ...rect, width }, [...path, "a"]);
      walk(node.b, { ...rect, left: rect.left + width, width: rect.width - width }, [
        ...path,
        "b",
      ]);
      handles.push({
        key: pathKey(path),
        path,
        dir: "row",
        rect: { left: rect.left + width, top: rect.top, width: 0, height: rect.height },
        bounds: rect,
      });
    } else {
      const height = rect.height * ratio;
      walk(node.a, { ...rect, height }, [...path, "a"]);
      walk(node.b, { ...rect, top: rect.top + height, height: rect.height - height }, [
        ...path,
        "b",
      ]);
      handles.push({
        key: pathKey(path),
        path,
        dir: "col",
        rect: { left: rect.left, top: rect.top + height, width: rect.width, height: 0 },
        bounds: rect,
      });
    }
  };

  walk(tree, FULL, []);
  return { rects, handles, order };
}

/** Half of a pane, as previewed while dragging another one onto it. */
export function zoneRect(rect: Rect, zone: Zone): Rect {
  switch (zone) {
    case "left":
      return { ...rect, width: rect.width / 2 };
    case "right":
      return { ...rect, left: rect.left + rect.width / 2, width: rect.width / 2 };
    case "top":
      return { ...rect, height: rect.height / 2 };
    case "bottom":
      return { ...rect, top: rect.top + rect.height / 2, height: rect.height / 2 };
    default:
      return rect;
  }
}

/** Which side of `rect` a pointer is closest to; the middle means "swap". */
export function zoneAt(rect: DOMRect, x: number, y: number): Zone {
  const dx = (x - rect.left) / Math.max(rect.width, 1);
  const dy = (y - rect.top) / Math.max(rect.height, 1);
  if (dx > 0.33 && dx < 0.67 && dy > 0.33 && dy < 0.67) return "center";

  let zone: Zone = "left";
  let best = dx;
  if (1 - dx < best) {
    best = 1 - dx;
    zone = "right";
  }
  if (dy < best) {
    best = dy;
    zone = "top";
  }
  if (1 - dy < best) zone = "bottom";
  return zone;
}

export type Direction = "left" | "right" | "up" | "down";

/**
 * The pane sitting next to `fromId` in a direction, or null at the edge of the
 * grid.
 *
 * Read off the rectangles rather than the tree: what "the pane on the left"
 * means is what is on screen, and two panes can be side by side without being
 * siblings in the tree. A candidate has to be entirely past our edge — a pane
 * we overlap is not next to us — and to actually face us, which is what the
 * overlap on the other axis measures. The closest one wins; when two are
 * equally close, the one facing us the most.
 */
export function neighborOf(
  tree: SplitNode,
  fromId: string,
  dir: Direction,
): string | null {
  const { rects } = computeLayout(tree);
  const from = rects[fromId];
  if (!from) return null;

  const epsilon = 1e-4;
  const horizontal = dir === "left" || dir === "right";
  const backwards = dir === "left" || dir === "up";

  const near = horizontal ? from.left : from.top;
  const far = near + (horizontal ? from.width : from.height);
  const sideStart = horizontal ? from.top : from.left;
  const sideEnd = sideStart + (horizontal ? from.height : from.width);

  let best: string | null = null;
  let bestGap = Infinity;
  let bestOverlap = 0;

  for (const [id, rect] of Object.entries(rects)) {
    if (id === fromId) continue;

    const start = horizontal ? rect.left : rect.top;
    const end = start + (horizontal ? rect.width : rect.height);
    const gap = backwards ? near - end : start - far;
    if (gap < -epsilon) continue;

    const otherStart = horizontal ? rect.top : rect.left;
    const otherEnd = otherStart + (horizontal ? rect.height : rect.width);
    const overlap = Math.min(sideEnd, otherEnd) - Math.max(sideStart, otherStart);
    if (overlap <= epsilon) continue;

    if (gap < bestGap - epsilon || (gap <= bestGap + epsilon && overlap > bestOverlap)) {
      best = id;
      bestGap = gap;
      bestOverlap = overlap;
    }
  }

  return best;
}

/** Splits along the longer side, so panes stay as square as they can. */
export function preferredDir(paneId: string | null): "row" | "col" {
  if (!paneId) return "row";
  const el = document.querySelector(`[data-slot="${CSS.escape(paneId)}"]`);
  if (!el) return "row";
  const rect = el.getBoundingClientRect();
  return rect.width >= rect.height * 1.15 ? "row" : "col";
}
