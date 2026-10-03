import { useRef, type ComponentProps } from "react";
import { BoardView } from "./BoardView";
import { useT } from "../i18n";
import { DEFAULT_SETTINGS } from "../types";

/**
 * The board as a panel on the right of the terminal grid, so the agents and
 * their tasks can be watched at once.
 *
 * It sits beside the grids rather than over them: the terminals only get
 * narrower, and their resize observers refit them — nothing is remounted, so
 * no PTY restarts when the panel opens, closes or changes width.
 */

const MIN_WIDTH = 300;
/** What the terminals always keep, however wide the panel is dragged. */
const TERMINALS_MIN = 320;
const KEY_STEP = 24;

interface Props extends Omit<ComponentProps<typeof BoardView>, "variant"> {
  width: number;
  onResize: (width: number) => void;
}

export function BoardDock({ width, onResize, ...board }: Props) {
  const asideRef = useRef<HTMLElement>(null);
  const t = useT();

  /** Bounded by the room actually available, which a window resize changes. */
  function clamp(next: number): number {
    const area = asideRef.current?.parentElement?.getBoundingClientRect().width ?? Infinity;
    const max = Math.max(MIN_WIDTH, area - TERMINALS_MIN);
    return Math.round(Math.min(max, Math.max(MIN_WIDTH, next)));
  }

  function beginResize(event: React.PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    const aside = asideRef.current;
    if (!aside) return;
    event.preventDefault();

    const grip = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startWidth = aside.getBoundingClientRect().width;
    let latest = startWidth;
    grip.setPointerCapture(pointerId);
    grip.classList.add("is-active");
    document.body.classList.add("is-resizing");

    // Painted straight onto the element while dragging; the store hears about
    // it once, on release, instead of re-rendering the board on every move.
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      latest = clamp(startWidth + (startX - ev.clientX));
      aside.style.width = `${latest}px`;
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
      onResize(latest);
    };
    const lostFocus = () => end();

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("blur", lostFocus);
  }

  return (
    <aside className="board-dock" ref={asideRef} style={{ width }}>
      <div
        className="board-dock__grip"
        role="separator"
        aria-orientation="vertical"
        aria-label={t("board.dockResize")}
        aria-valuenow={width}
        aria-valuemin={MIN_WIDTH}
        title={t("board.dockResize")}
        tabIndex={0}
        onPointerDown={beginResize}
        onDoubleClick={() => onResize(DEFAULT_SETTINGS.boardDockWidth)}
        onKeyDown={(event) => {
          // The panel grows leftwards, into the terminals.
          if (event.key === "ArrowLeft") onResize(clamp(width + KEY_STEP));
          else if (event.key === "ArrowRight") onResize(clamp(width - KEY_STEP));
          else return;
          event.preventDefault();
        }}
      />
      <BoardView {...board} variant="dock" />
    </aside>
  );
}
