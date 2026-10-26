import { useRef, type ComponentProps } from "react";
import { BoardView } from "./BoardView";
import { useT } from "../i18n";
import { clampDock, DOCK_MIN } from "../lib/sidePanels";
import { useDrawer } from "../lib/useDrawer";
import { DEFAULT_SETTINGS } from "../types";

/**
 * The board as a panel on the right of the terminal grid, so the agents and
 * their tasks can be watched at once.
 *
 * It sits beside the grids rather than over them: the terminals only get
 * narrower, and their resize observers refit them — nothing is remounted, so
 * no PTY restarts when the panel opens, closes or changes width. Where that
 * would leave the terminals too little room, App draws it as a drawer over
 * them instead, opened on demand (lib/sidePanels, lib/useDrawer).
 */

/** In interface points, like the width itself. */
const KEY_STEP = 24;

interface Props extends Omit<ComponentProps<typeof BoardView>, "variant"> {
  /** As saved, in interface points: drawn at `width × ui-scale`. */
  width: number;
  /** What the room leaves of it, in pixels, as lib/sidePanels placed it. */
  drawnWidth: number;
  /** The widest the room allows right now, in points. */
  maxWidth: number;
  scale: number;
  /** No room beside the terminals: drawn over them while it is asked for. */
  drawer: boolean;
  /** The focused pane, which takes the keyboard back when nothing else can. */
  paneId: string | null;
  onResize: (width: number) => void;
}

export function BoardDock({
  width,
  drawnWidth,
  maxWidth,
  scale,
  drawer,
  paneId,
  onResize,
  onClose,
  ...board
}: Props) {
  const asideRef = useRef<HTMLElement>(null);
  const t = useT();

  const closeDrawer = useDrawer({
    panelRef: asideRef,
    active: drawer,
    onClose: () => onClose?.(),
    paneId,
  });

  /** In points, bounded by the room actually available, which a window resize changes. */
  const clamp = (next: number) => Math.min(maxWidth, clampDock(next));
  const shown = Math.round(drawnWidth / scale);

  function beginResize(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const aside = asideRef.current;
    if (!aside) return;
    event.preventDefault();

    const grip = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startWidth = aside.getBoundingClientRect().width;
    let latest = shown;
    grip.setPointerCapture(pointerId);
    grip.classList.add("is-active");
    document.body.classList.add("is-resizing");

    // Painted straight onto the element while dragging; the store hears about
    // it once, on release, instead of re-rendering the board on every move.
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      latest = clamp((startWidth + (startX - ev.clientX)) / scale);
      aside.style.width = `${latest * scale}px`;
    };

    const end = (ev?: PointerEvent) => {
      if (ev && ev.pointerId !== pointerId) return;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("blur", lostFocus);
      if (grip.hasPointerCapture(pointerId)) grip.releasePointerCapture(pointerId);
      grip.classList.remove("is-active");
      document.body.classList.remove("is-resizing");
      if (latest !== width) onResize(latest);
    };
    const lostFocus = () => end();

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", lostFocus);
  }

  return (
    <aside
      className={`board-dock ${drawer ? "board-dock--drawer" : ""}`}
      ref={asideRef}
      style={{ width: drawnWidth }}
      aria-label={t("board.dock")}
      // Focused as a drawer, so Escape and the keys after it start inside.
      tabIndex={-1}
    >
      <div
        className="board-dock__grip"
        role="separator"
        aria-orientation="vertical"
        aria-label={t("board.dockResize")}
        aria-valuenow={shown}
        aria-valuemin={DOCK_MIN}
        aria-valuemax={Math.max(DOCK_MIN, Math.round(maxWidth))}
        title={t("board.dockResize")}
        tabIndex={0}
        onPointerDown={beginResize}
        onDoubleClick={() => onResize(DEFAULT_SETTINGS.boardDockWidth)}
        onKeyDown={(event) => {
          // The panel grows leftwards, into the terminals.
          if (event.key === "ArrowLeft") onResize(clamp(shown + KEY_STEP));
          else if (event.key === "ArrowRight") onResize(clamp(shown - KEY_STEP));
          else return;
          event.preventDefault();
        }}
      />
      <BoardView
        {...board}
        variant="dock"
        onClose={drawer ? () => closeDrawer("button") : onClose}
      />
    </aside>
  );
}
