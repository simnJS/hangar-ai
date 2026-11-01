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
  leafIds,
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
import { useT } from "../i18n";
import type { Pane, SplitNode } from "../types";
import { PaneZoomBar } from "./PaneZoomBar";

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

/** How far an arrow key moves a boundary, in pixels; Shift goes five times as far. */
const KEY_STEP_PX = 24;

function slotStyle(rect: Rect): CSSProperties {
  return {
    left: percent(rect.left),
    top: percent(rect.top),
    width: percent(rect.width),
    height: percent(rect.height),
  };
}

/** The zoomed pane: everything under the zoom bar. */
const ZOOMED_SLOT: CSSProperties = {
  left: 0,
  top: "var(--zoombar-h)",
  width: "100%",
  height: "calc(100% - var(--zoombar-h))",
};

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
  /**
   * The pane drawn over the whole grid, or null for the arrangement itself.
   * Purely a way of looking at the tree: the others stay mounted, hidden, and
   * keep the size they had, so their PTYs are neither killed nor resized.
   */
  zoomedId?: string | null;
  /** Zooms onto a pane, or with null puts every pane back on screen. */
  onZoom?: (paneId: string | null) => void;
  /** The pane a file dragged from the desktop would land on, if dropped now. */
  fileTarget?: { paneId: string; count: number } | null;
  hidden?: boolean;
}

export function PaneGrid({
  panes,
  tree,
  renderPane,
  onTreeChange,
  onMove,
  zoomedId = null,
  onZoom,
  fileTarget = null,
  hidden,
}: Props) {
  const hostRef = useRef<HTMLDivElement>(null);
  const ghostRef = useRef<HTMLDivElement>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [target, setTarget] = useState<DragTarget | null>(null);
  const t = useT();

  const layout = useMemo(() => computeLayout(tree), [tree]);
  /** Reading order, which is the order the zoom bar lists the panes in. */
  const order = useMemo(() => leafIds(tree), [tree]);

  const zoomed = zoomedId && layout.rects[zoomedId] ? zoomedId : null;
  const zoomedRef = useRef(zoomed);
  zoomedRef.current = zoomed;

  /**
   * The header the last press landed on. A double-click is only a zoom when it
   * lands on that same bar: anywhere in the terminal it selects a word.
   */
  const pressedRef = useRef<{ paneId: string; bar: Element } | null>(null);

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
    // Keeps the hairline lit for the whole gesture, wherever the pointer goes.
    grip.classList.add("is-active");

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
      grip.classList.remove("is-active");
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

  /**
   * The keyboard's resize, for a boundary that has the focus: the arrows along
   * it move it, Home and End take it as far as the panes allow, Enter evens it
   * out like a double-click.
   */
  function resizeByKey(handle: Handle, event: React.KeyboardEvent) {
    const host = hostRef.current;
    if (!host) return;
    const horizontal = handle.dir === "row";
    const box = host.getBoundingClientRect();
    const span = Math.max(
      horizontal ? handle.bounds.width * box.width : handle.bounds.height * box.height,
      1,
    );
    // The same floor as a drag: no pane gets narrower than MIN_PANE_PX.
    const min = Math.min(0.45, MIN_PANE_PX / span);
    const current = ratioAt(treeRef.current, handle.path);
    const step = (event.shiftKey ? KEY_STEP_PX * 5 : KEY_STEP_PX) / span;
    let next: number;
    if (event.key === (horizontal ? "ArrowLeft" : "ArrowUp")) next = current - step;
    else if (event.key === (horizontal ? "ArrowRight" : "ArrowDown")) next = current + step;
    else if (event.key === "Home") next = min;
    else if (event.key === "End") next = 1 - min;
    else if (event.key === "Enter") next = 0.5;
    else return;
    event.preventDefault();
    onTreeChange(withRatio(treeRef.current, handle.path, Math.min(1 - min, Math.max(min, next))));
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

      pressedRef.current = { paneId, bar: event.currentTarget };
      // Nowhere to drop it: every other pane is out of sight.
      if (zoomedRef.current) return;

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

        // Hidden until placed: it mounts on the move that arms the drag, one
        // render before it can be told where the pointer is.
        const ghost = ghostRef.current;
        if (ghost) {
          ghost.style.transform = `translate(${ev.clientX}px, ${ev.clientY}px)`;
          ghost.style.visibility = "visible";
        }

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

  function toggleZoom(paneId: string, event: React.MouseEvent) {
    const pressed = pressedRef.current;
    const hit = event.target as Element;
    if (!onZoom || pressed?.paneId !== paneId || !pressed.bar.contains(hit)) return;
    if (hit.closest("button, select, input, a")) return;
    // The second press of a double-click has already selected the word under it.
    window.getSelection()?.removeAllRanges();
    onZoom(zoomed === paneId ? null : paneId);
  }

  const names = new Map(panes.map((pane) => [pane.id, pane.name]));

  return (
    <DragContext.Provider value={drag}>
      <div
        className={`grid ${zoomed ? "grid--zoomed" : ""}`}
        ref={hostRef}
        style={{ display: hidden ? "none" : "block" }}
      >
        {/* Stable order, keyed by pane id: React never remounts a terminal. */}
        {panes.map((pane) => {
          const rect = layout.rects[pane.id];
          if (!rect) return null;
          const behind = zoomed !== null && zoomed !== pane.id;
          // The 1px separators: a pane off the grid's left or top edge gives
          // its first column or row to one. A resize never moves a pane off
          // or onto an edge, so painting rectangles alone keeps these right.
          const inner = zoomed
            ? ""
            : `${rect.left > 1e-6 ? "is-inner-x" : ""} ${rect.top > 1e-6 ? "is-inner-y" : ""}`;
          return (
            <div
              key={pane.id}
              className={`grid__slot ${inner} ${dragId === pane.id ? "is-dragging" : ""} ${behind ? "is-behind" : ""}`}
              data-slot={pane.id}
              style={zoomed === pane.id ? ZOOMED_SLOT : slotStyle(rect)}
              onDoubleClick={(event) => toggleZoom(pane.id, event)}
            >
              {renderPane(pane)}
              {fileTarget?.paneId === pane.id && (
                <div className="grid__filedrop" aria-hidden>
                  <span className="grid__filedrop-label">
                    {t("drop.paste", { n: Math.max(1, fileTarget.count) })}
                  </span>
                </div>
              )}
            </div>
          );
        })}

        {zoomed && onZoom && (
          <PaneZoomBar zoomedId={zoomed} order={order} names={names} onZoom={onZoom} />
        )}

        {/* Focusable, and named after the panes on either side: the
            terminals keep Tab and the arrows for themselves, so a boundary
            is the keyboard's one way to resize them. */}
        {!zoomed && layout.handles.map((handle) => {
          const side = (ids: string[]) => ids.map((id) => names.get(id) ?? "").join(", ");
          const ratio = Math.min(0.95, Math.max(0.05, ratioAt(tree, handle.path)));
          return (
            <div
              key={handle.key}
              data-handle={handle.key}
              className={`splitter splitter--${handle.dir === "row" ? "col" : "row"}`}
              style={handleStyle(handle)}
              onPointerDown={(event) => startResize(handle, event)}
              onDoubleClick={() => onTreeChange(withRatio(tree, handle.path, 0.5))}
              onKeyDown={(event) => resizeByKey(handle, event)}
              role="separator"
              tabIndex={0}
              aria-orientation={handle.dir === "row" ? "vertical" : "horizontal"}
              aria-label={t(handle.dir === "row" ? "grid.resizeColumns" : "grid.resizeRows", {
                before: side(handle.before),
                after: side(handle.after),
              })}
              aria-valuenow={Math.round(ratio * 100)}
              aria-valuemin={0}
              aria-valuemax={100}
            />
          );
        })}

        {hint && (
          <div
            className={`drop-hint drop-hint--${target?.zone}`}
            style={slotStyle(hint)}
            aria-hidden
          />
        )}

        {/* Follows the pointer, carrying the name of the pane on the move. */}
        {dragId && (
          <div className="drag-ghost" ref={ghostRef} aria-hidden>
            {names.get(dragId)}
          </div>
        )}
      </div>
    </DragContext.Provider>
  );
}
