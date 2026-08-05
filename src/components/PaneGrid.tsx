import { useRef, type ReactNode } from "react";
import type { LayoutSize, SplitNode } from "../types";

/** A pane never shrinks below this, so it stays usable while dragging. */
const MIN_PANE_PX = 140;

const pane = (index: number): SplitNode => ({ type: "pane", index });

const split = (dir: "row" | "col", a: SplitNode, b: SplitNode): SplitNode => ({
  type: "split",
  dir,
  ratio: 0.5,
  a,
  b,
});

/** Reproduces the familiar 1 / 2 / 2x2 / 4x2 arrangements as a split tree. */
export function defaultTree(layout: LayoutSize): SplitNode {
  switch (layout) {
    case 1:
      return pane(0);
    case 2:
      return split("row", pane(0), pane(1));
    case 4:
      return split("col", split("row", pane(0), pane(1)), split("row", pane(2), pane(3)));
    case 8:
    default:
      return split(
        "col",
        split("row", split("row", pane(0), pane(1)), split("row", pane(2), pane(3))),
        split("row", split("row", pane(4), pane(5)), split("row", pane(6), pane(7))),
      );
  }
}

function leaves(node: SplitNode, out: number[] = []): number[] {
  if (node.type === "pane") out.push(node.index);
  else {
    leaves(node.a, out);
    leaves(node.b, out);
  }
  return out;
}

/** Rejects a stored tree that no longer covers exactly the current panes. */
export function normalizeTree(layout: LayoutSize, tree?: SplitNode | null): SplitNode {
  if (!tree) return defaultTree(layout);
  try {
    const found = leaves(tree).sort((a, b) => a - b);
    const matches =
      found.length === layout && found.every((value, index) => value === index);
    return matches ? tree : defaultTree(layout);
  } catch {
    return defaultTree(layout);
  }
}

/** Rebuilds the tree with one ratio replaced. `path` is a trail of 'a'/'b'. */
function withRatio(node: SplitNode, path: string[], ratio: number): SplitNode {
  if (node.type === "pane") return node;
  if (path.length === 0) return { ...node, ratio };
  const [head, ...rest] = path;
  return head === "a"
    ? { ...node, a: withRatio(node.a, rest, ratio) }
    : { ...node, b: withRatio(node.b, rest, ratio) };
}

interface BranchProps {
  node: SplitNode;
  path: string[];
  renderPane: (index: number) => ReactNode;
  onRatio: (path: string[], ratio: number) => void;
}

function Branch({ node, path, renderPane, onRatio }: BranchProps) {
  const hostRef = useRef<HTMLDivElement>(null);

  if (node.type === "pane") {
    return <>{renderPane(node.index)}</>;
  }

  const horizontal = node.dir === "row";

  function startDrag(event: React.PointerEvent) {
    const host = hostRef.current;
    if (!host) return;
    event.preventDefault();
    (event.target as Element).setPointerCapture(event.pointerId);

    const rect = host.getBoundingClientRect();
    const total = horizontal ? rect.width : rect.height;
    const origin = horizontal ? event.clientX : event.clientY;
    const startRatio = (node as { ratio: number }).ratio;
    const min = Math.min(0.45, MIN_PANE_PX / Math.max(total, 1));
    let latest = startRatio;

    const sides = host.children;
    const first = sides[0] as HTMLElement | undefined;
    const second = sides[2] as HTMLElement | undefined;

    function move(ev: PointerEvent) {
      const delta = (horizontal ? ev.clientX : ev.clientY) - origin;
      latest = Math.min(1 - min, Math.max(min, startRatio + delta / Math.max(total, 1)));
      // Applied straight to the DOM: routing every pointer move through React
      // would re-render every terminal in this branch on each frame.
      if (first) first.style.flexGrow = String(latest);
      if (second) second.style.flexGrow = String(1 - latest);
    }

    function end() {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      onRatio(path, latest);
    }

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
  }

  return (
    <div
      ref={hostRef}
      className="split"
      style={{ flexDirection: horizontal ? "row" : "column" }}
    >
      <div className="split__side" style={{ flexGrow: node.ratio }}>
        <Branch node={node.a} path={[...path, "a"]} renderPane={renderPane} onRatio={onRatio} />
      </div>

      <div
        className={`splitter splitter--${horizontal ? "col" : "row"}`}
        onPointerDown={startDrag}
        onDoubleClick={() => onRatio(path, 0.5)}
        role="separator"
        aria-orientation={horizontal ? "vertical" : "horizontal"}
      />

      <div className="split__side" style={{ flexGrow: 1 - node.ratio }}>
        <Branch node={node.b} path={[...path, "b"]} renderPane={renderPane} onRatio={onRatio} />
      </div>
    </div>
  );
}

interface Props {
  tree: SplitNode;
  onTreeChange: (tree: SplitNode) => void;
  renderPane: (index: number) => ReactNode;
  hidden?: boolean;
}

export function PaneGrid({ tree, onTreeChange, renderPane, hidden }: Props) {
  return (
    <div className="grid" style={{ display: hidden ? "none" : "flex" }}>
      <Branch
        node={tree}
        path={[]}
        renderPane={renderPane}
        onRatio={(path, ratio) => onTreeChange(withRatio(tree, path, ratio))}
      />
    </div>
  );
}
