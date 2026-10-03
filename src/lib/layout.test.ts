import { describe, expect, it } from "vitest";
import type { Pane, SplitNode } from "../types";
import {
  computeLayout,
  insertLeaf,
  leafIds,
  moveLeaf,
  neighborOf,
  normalizeTree,
  presetTree,
  ratioAt,
  removeLeaf,
  renameLeaf,
  swapLeaves,
  withRatio,
  zoneAt,
  zoneRect,
} from "./layout";

const leaf = (id: string): SplitNode => ({ type: "pane", id });
const split = (dir: "row" | "col", a: SplitNode, b: SplitNode, ratio = 0.5): SplitNode => ({
  type: "split",
  dir,
  ratio,
  a,
  b,
});
const panes = (...ids: string[]) => ids.map((id) => ({ id }) as Pane);

describe("presetTree", () => {
  it("is a single leaf for one pane", () => {
    expect(presetTree(["a"])).toEqual(leaf("a"));
  });

  it("puts two panes side by side", () => {
    expect(presetTree(["a", "b"])).toEqual(split("row", leaf("a"), leaf("b")));
  });

  it("lays four panes out as an even 2x2", () => {
    const { rects, order } = computeLayout(presetTree(["a", "b", "c", "d"]));
    expect(order).toEqual(["a", "b", "c", "d"]);
    expect(rects.a).toEqual({ left: 0, top: 0, width: 0.5, height: 0.5 });
    expect(rects.d).toEqual({ left: 0.5, top: 0.5, width: 0.5, height: 0.5 });
  });

  it("lays eight panes out as 4x2", () => {
    const ids = ["1", "2", "3", "4", "5", "6", "7", "8"];
    const { rects } = computeLayout(presetTree(ids));
    for (const id of ids) {
      expect(rects[id].width).toBeCloseTo(0.25);
      expect(rects[id].height).toBeCloseTo(0.5);
    }
    expect(rects["5"].top).toBeCloseTo(0.5);
  });

  it("lets the odd pane out take the whole last row", () => {
    const { rects } = computeLayout(presetTree(["a", "b", "c"]));
    expect(rects.c).toEqual({ left: 0, top: 0.5, width: 1, height: 0.5 });
  });
});

describe("splitting and closing", () => {
  it("puts the new pane on the side it was dropped on", () => {
    expect(insertLeaf(leaf("a"), "a", "n", "right")).toEqual(split("row", leaf("a"), leaf("n")));
    expect(insertLeaf(leaf("a"), "a", "n", "left")).toEqual(split("row", leaf("n"), leaf("a")));
    expect(insertLeaf(leaf("a"), "a", "n", "top")).toEqual(split("col", leaf("n"), leaf("a")));
    expect(insertLeaf(leaf("a"), "a", "n", "bottom")).toEqual(split("col", leaf("a"), leaf("n")));
  });

  it("splits only the target, deep in the tree", () => {
    const tree = split("row", leaf("a"), split("col", leaf("b"), leaf("c")));
    const next = insertLeaf(tree, "c", "n", "right");
    expect(leafIds(next)).toEqual(["a", "b", "c", "n"]);
    // The untouched branch is shared, not copied.
    expect(next.type === "split" && tree.type === "split" && next.a === tree.a).toBe(true);
  });

  it("gives a closed pane's space to its sibling", () => {
    const tree = split("row", leaf("a"), split("col", leaf("b"), leaf("c"), 0.3), 0.7);
    expect(removeLeaf(tree, "b")).toEqual(split("row", leaf("a"), leaf("c"), 0.7));
    expect(removeLeaf(split("row", leaf("a"), leaf("b")), "a")).toEqual(leaf("b"));
  });

  it("returns null once the last pane is gone", () => {
    expect(removeLeaf(leaf("a"), "a")).toBeNull();
  });

  it("returns the same tree when the pane is not in it", () => {
    const tree = split("row", leaf("a"), leaf("b"));
    expect(removeLeaf(tree, "zz")).toBe(tree);
  });

  it("keeps the arrangement when a pane is renamed", () => {
    const tree = split("row", leaf("a"), split("col", leaf("b"), leaf("c"), 0.3));
    expect(renameLeaf(tree, "b", "b2")).toEqual(
      split("row", leaf("a"), split("col", leaf("b2"), leaf("c"), 0.3)),
    );
  });
});

describe("ratios", () => {
  const tree = split("row", leaf("a"), split("col", leaf("b"), leaf("c")));

  it("reads and writes the ratio at a path", () => {
    const next = withRatio(tree, ["b"], 0.25);
    expect(ratioAt(next, ["b"])).toBe(0.25);
    expect(ratioAt(next, [])).toBe(0.5);
    // The original is left alone.
    expect(ratioAt(tree, ["b"])).toBe(0.5);
  });

  it("follows the ratio in the rectangles", () => {
    const { rects } = computeLayout(withRatio(tree, [], 0.25));
    expect(rects.a.width).toBeCloseTo(0.25);
    expect(rects.b.left).toBeCloseTo(0.25);
    expect(rects.b.width).toBeCloseTo(0.75);
  });

  it("clamps a ratio that would crush a pane", () => {
    const { rects } = computeLayout(split("row", leaf("a"), leaf("b"), 0));
    expect(rects.a.width).toBeCloseTo(0.05);
    expect(computeLayout(split("row", leaf("a"), leaf("b"), 1)).rects.b.width).toBeCloseTo(0.05);
  });

  it("has one handle per split, keyed by its path", () => {
    const { handles } = computeLayout(tree);
    expect(handles.map((h) => h.key).sort()).toEqual(["b", "root"]);
    const root = handles.find((h) => h.key === "root")!;
    expect(root.dir).toBe("row");
    expect(root.rect).toEqual({ left: 0.5, top: 0, width: 0, height: 1 });
  });
});

describe("moving panes", () => {
  const tree = split("row", leaf("a"), split("col", leaf("b"), leaf("c"), 0.3));

  it("swaps two panes on a drop in the middle, keeping the shape", () => {
    expect(moveLeaf(tree, "a", "c", "center")).toEqual(
      split("row", leaf("c"), split("col", leaf("b"), leaf("a"), 0.3)),
    );
    expect(swapLeaves(tree, "a", "c")).toEqual(moveLeaf(tree, "a", "c", "center"));
  });

  it("re-splits the target on a drop on an edge", () => {
    const next = moveLeaf(tree, "a", "c", "bottom");
    expect(leafIds(next)).toEqual(["b", "c", "a"]);
    expect(computeLayout(next).rects.a.top).toBeGreaterThan(computeLayout(next).rects.c.top);
  });

  it("does nothing when a pane is dropped on itself", () => {
    expect(moveLeaf(tree, "b", "b", "left")).toBe(tree);
  });
});

describe("normalizeTree", () => {
  it("returns a tree that still matches untouched, ratios included", () => {
    const tree = split("row", leaf("a"), leaf("b"), 0.8);
    expect(normalizeTree(panes("a", "b"), tree)).toEqual(tree);
  });

  it("builds the preset when there is no tree", () => {
    expect(normalizeTree(panes("a", "b"), null)).toEqual(presetTree(["a", "b"]));
  });

  it("maps a legacy tree of pane indexes onto ids", () => {
    const legacy = {
      type: "split",
      dir: "col",
      ratio: 0.6,
      a: { type: "pane", index: 1 },
      b: { type: "pane", index: 0 },
    } as unknown as SplitNode;
    expect(normalizeTree(panes("a", "b"), legacy)).toEqual(split("col", leaf("b"), leaf("a"), 0.6));
  });

  it("drops panes that are gone and appends new ones, in the user's order", () => {
    const tree = split("row", leaf("c"), split("row", leaf("gone"), leaf("a")));
    expect(leafIds(normalizeTree(panes("a", "b", "c"), tree))).toEqual(["c", "a", "b"]);
  });

  it("never lists a pane twice", () => {
    const tree = split("row", leaf("a"), leaf("a"));
    expect(leafIds(normalizeTree(panes("a", "b"), tree))).toEqual(["a", "b"]);
  });

  it("is an empty leaf when there are no panes", () => {
    expect(normalizeTree([], null)).toEqual(leaf(""));
  });
});

describe("neighborOf", () => {
  const grid = presetTree(["a", "b", "c", "d"]);

  it("finds the pane on each side", () => {
    expect(neighborOf(grid, "a", "right")).toBe("b");
    expect(neighborOf(grid, "a", "down")).toBe("c");
    expect(neighborOf(grid, "d", "left")).toBe("c");
    expect(neighborOf(grid, "d", "up")).toBe("b");
  });

  it("is null at the edge of the grid", () => {
    expect(neighborOf(grid, "a", "left")).toBeNull();
    expect(neighborOf(grid, "a", "up")).toBeNull();
  });

  it("reads the screen, not the tree", () => {
    // b and c are not siblings of a, yet both sit to its right.
    const tree = split("row", leaf("a"), split("col", leaf("b"), leaf("c"), 0.3));
    expect(neighborOf(tree, "a", "right")).toBe("c");
    expect(neighborOf(tree, "c", "left")).toBe("a");
    expect(neighborOf(tree, "c", "up")).toBe("b");
  });

  it("is null for a pane that is not in the tree", () => {
    expect(neighborOf(grid, "zz", "left")).toBeNull();
  });
});

describe("drop zones", () => {
  const box = { left: 0, top: 0, width: 100, height: 100 } as DOMRect;

  it("swaps in the middle and splits towards the nearest edge", () => {
    expect(zoneAt(box, 50, 50)).toBe("center");
    expect(zoneAt(box, 5, 50)).toBe("left");
    expect(zoneAt(box, 95, 50)).toBe("right");
    expect(zoneAt(box, 50, 5)).toBe("top");
    expect(zoneAt(box, 50, 95)).toBe("bottom");
  });

  it("previews the half a dropped pane would take", () => {
    const rect = { left: 0, top: 0, width: 1, height: 1 };
    expect(zoneRect(rect, "right")).toEqual({ left: 0.5, top: 0, width: 0.5, height: 1 });
    expect(zoneRect(rect, "bottom")).toEqual({ left: 0, top: 0.5, width: 1, height: 0.5 });
    expect(zoneRect(rect, "center")).toBe(rect);
  });
});
