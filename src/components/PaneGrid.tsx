import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import {
  computeLayout,
  MIN_PANE_PX,
  ratioAt,
  withRatio,
  zoneAt,
  zoneRect,
  type GridLayout,
  type Handle,
  type Rect,
  type Zone,
} from "../lib/layout";
import type { Pane, SplitNode } from "../types";

/**
 * Renders the split tree as flat, absolutely positioned slots.
 *
 * Nothing is ever reparented: a pane keeps the same DOM position in the tree
 * whatever the arrangement, which is what keeps its PTY alive while it is
 * dragged around. Only the four numbers of its rectangle change.
 */

interface DragTarget {
  paneId: string;
  zone: Zone;
}

interface DragValue {
  /** Starts a pane drag from a pointer event on its title bar. */
  begin: (paneId: string, event: React.PointerEvent) => void;
  draggingId: string | null;
}

const DragContext = createContext<DragValue>({ begin: () => {}, draggingId: null });

/** Lets a pane header act as the drag handle without knowing about the grid. */
export const usePaneDrag = () => useContext(DragContext);

const percent = (value: number) => `${value * 100}%`;

function slotStyle(rect: Rect): CSSProperties {
  return {
    left: percent(rect.left),
    top: percent(rect.top),
    width: percent(rect.width),
    height: percent(rect.height),
  };
}

function handleStyle(handle: Handle): CSSProperties {
  return handle.dir === "row"
    ? {
        left: percent(handle.rect.left),
        top: percent(handle.rect.top),
        height: percent(handle.rect.height),
      }
    : {
        left: percent(handle.rect.left),
        top: percent(handle.rect.top),
        width: percent(handle.rect.width),
      };
}

/**
 * Pushes a layout straight to the DOM. Routing every pointer move through
 * React would re-render every terminal header on each frame.
 */
function paintLayout(host: HTMLElement, layout: GridLayout) {
  for (const [id, rect] of Object.entries(layout.rects)) {
    const el = host.querySelector<HTMLElement>(`[data-slot="${CSS.escape(id)}"]`);
    if (el) Object.assign(el.style, slotStyle(rect));
  }
  for (const handle of layout.handles) {
    const el = host.querySelector<HTMLElement>(
      `[data-handle="${CSS.escape(handle.key)}"]`,
    );
    if (el) Object.assign(el.style, handleStyle(handle));
  }
}

interface Props {
  panes: Pane[];
  tree: SplitNode;
  renderPane: (pane: Pane) => ReactNode;
  onTreeChange: (tree: SplitNode) => void;
  onMove: (dragId: string, targetId: string, zone: Zone) => void;
  hidden?: boolean;
}

export function PaneGrid({
  panes,
  tree,
  renderPane,
  onTreeChange,
  onMove,
  hidden,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const ghostRef = useRef<HTMLDivElement>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [target, setTarget] = useState<DragTarget | null>(null);

  const layout = useMemo(() => computeLayout(tree), [tree]);

  // Read through a ref so a drag started earlier always sees the live tree.
  const treeRef = useRef(tree);
  treeRef.current = tree;

  function startResize(handle: Handle, event: React.PointerEvent) {
    const host = hostRef.current;
    if (!host || event.button !== 0) return;
    event.preventDefault();

    // Capturing on the grip keeps the move/up stream aimed at it even once the
    // pointer has left the window, so a button released over another app still
    // ends the gesture here instead of leaving it armed forever.
    const grip = event.currentTarget;
    const pointerId = event.pointerId;
    grip.setPointerCapture(pointerId);

    const box = host.getBoundingClientRect();
    const horizontal = handle.dir === "row";
    const total = Math.max(horizontal ? box.width : box.height, 1);
    const span = (horizontal ? handle.bounds.width : handle.bounds.height) * total;
    const origin = horizontal ? handle.bounds.left : handle.bounds.top;
    const extent = Math.max(horizontal ? handle.bounds.width : handle.bounds.height, 1e-6);
    const min = Math.min(0.45, MIN_PANE_PX / Math.max(span, 1));
    const base = treeRef.current;
    let latest = ratioAt(base, handle.path);

    document.body.classList.add("is-resizing");

    function move(ev: PointerEvent) {
      // A second pointer (touch, pen) must not steer a drag it did not start.
      if (ev.pointerId !== pointerId) return;
      const position =
        ((horizontal ? ev.clientX - box.left : ev.clientY - box.top) / total - origin) /
        extent;
      latest = Math.min(1 - min, Math.max(min, position));
      paintLayout(host!, computeLayout(withRatio(base, handle.path, latest)));
    }

    /**
     * The single exit, whatever ended the gesture: release, cancel, or the
     * window losing focus mid-drag.
     *
     * A cancelled resize commits `latest` exactly like a release would. The
     * panes are already painted at that size, so restoring the old ratio would
     * be a jump nobody asked for, and skipping the commit would leave the DOM
     * ahead of the store until some unrelated render happens to repaint it.
     */
    function end(ev?: PointerEvent) {
      if (ev && ev.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("blur", lostFocus);
      if (grip.hasPointerCapture(pointerId)) grip.releasePointerCapture(pointerId);
      document.body.classList.remove("is-resizing");
      onTreeChange(withRatio(treeRef.current, handle.path, latest));
    }

    // Last resort: a webview can lose the pointer without ever reporting it.
    const lostFocus = () => end();

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", lostFocus);
  }

  const hitTest = useCallback((x: number, y: number, moving: string): DragTarget | null => {
    const el = document
      .elementFromPoint(x, y)
      ?.closest<HTMLElement>("[data-slot]");
    const paneId = el?.dataset.slot;
    if (!el || !paneId || paneId === moving) return null;
    return { paneId, zone: zoneAt(el.getBoundingClientRect(), x, y) };
  }, []);

  const begin = useCallback(
    (paneId: string, event: React.PointerEvent) => {
      if (event.button !== 0) return;
      // The bar also holds selects and buttons; leave those alone. This has to
      // stay ahead of the capture below, which would otherwise steal the
      // pointer from those controls.
      if ((event.target as Element).closest("button, select, input, a")) return;

      // Same reasoning as the splitter: captured, the bar keeps receiving the
      // pointer outside the window, so the drag cannot survive its own release.
      const bar = event.currentTarget;
      const pointerId = event.pointerId;
      bar.setPointerCapture(pointerId);

      const startX = event.clientX;
      const startY = event.clientY;
      let armed = false;
      let last: DragTarget | null = null;

      const move = (ev: PointerEvent) => {
        if (ev.pointerId !== pointerId) return;
        if (!armed) {
          if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 6) return;
          armed = true;
          setDragId(paneId);
          document.body.classList.add("is-dragging-pane");
        }

        const ghost = ghostRef.current;
        if (ghost) ghost.style.transform = `translate(${ev.clientX}px, ${ev.clientY}px)`;

        const next = hitTest(ev.clientX, ev.clientY, paneId);
        if (next?.paneId !== last?.paneId || next?.zone !== last?.zone) {
          last = next;
          setTarget(next);
        }
      };

      /**
       * `release` is the event that ended the drag, or null when the gesture
       * was taken away instead of finished.
       *
       * A cancelled drag rearranges nothing: unlike a resize the drop is a
       * discrete, destructive edit of the tree, and the user never confirmed
       * this one by lifting the button. Everything else is torn down the same
       * way in both cases.
       */
      const stop = (release: PointerEvent | null) => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", end);
        window.removeEventListener("pointercancel", cancel);
        window.removeEventListener("blur", lostFocus);
        if (bar.hasPointerCapture(pointerId)) bar.releasePointerCapture(pointerId);
        document.body.classList.remove("is-dragging-pane");
        setDragId(null);
        setTarget(null);
        if (!armed || !release) return;
        const drop = hitTest(release.clientX, release.clientY, paneId);
        if (drop) onMove(paneId, drop.paneId, drop.zone);
      };

      const end = (ev: PointerEvent) => {
        if (ev.pointerId === pointerId) stop(ev);
      };

      const cancel = (ev: PointerEvent) => {
        if (ev.pointerId === pointerId) stop(null);
      };

      // Last resort: a webview can lose the pointer without ever reporting it.
      const lostFocus = () => stop(null);

      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", end);
      window.addEventListener("pointercancel", cancel);
      window.addEventListener("blur", lostFocus);
    },
    [hitTest, onMove],
  );

  const drag = useMemo<DragValue>(() => ({ begin, draggingId: dragId }), [begin, dragId]);

  const hint =
    target && layout.rects[target.paneId]
      ? zoneRect(layout.rects[target.paneId], target.zone)
      : null;

  return (
    <DragContext.Provider value={drag}>
      <div className="grid" ref={hostRef} style={{ display: hidden ? "none" : "block" }}>
        {/* Stable order, keyed by pane id: React never remounts a terminal. */}
        {panes.map((pane) => {
          const rect = layout.rects[pane.id];
          if (!rect) return null;
          return (
            <div
              key={pane.id}
              className={`grid__slot ${dragId === pane.id ? "is-dragging" : ""}`}
              data-slot={pane.id}
              style={slotStyle(rect)}
            >
              {renderPane(pane)}
            </div>
          );
        })}

        {layout.handles.map((handle) => (
          <div
            key={handle.key}
            data-handle={handle.key}
            className={`splitter splitter--${handle.dir === "row" ? "col" : "row"}`}
            style={handleStyle(handle)}
            onPointerDown={(event) => startResize(handle, event)}
            onDoubleClick={() => onTreeChange(withRatio(tree, handle.path, 0.5))}
            role="separator"
            aria-orientation={handle.dir === "row" ? "vertical" : "horizontal"}
          />
        ))}

        {hint && (
          <div
            className={`drop-hint drop-hint--${target?.zone}`}
            style={slotStyle(hint)}
            aria-hidden
          />
        )}

        {dragId && <div className="drag-ghost" ref={ghostRef} aria-hidden />}
      </div>
    </DragContext.Provider>
  );
}
