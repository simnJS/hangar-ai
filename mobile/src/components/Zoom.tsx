import { useEffect, useRef, useState, type PointerEvent } from "react";
import { createPortal } from "react-dom";
import { t } from "../i18n";
import { Icon } from "./Icon";

interface View {
  scale: number;
  /** Where the picture's top left corner sits, in the stage's pixels. */
  x: number;
  y: number;
}

interface Point {
  x: number;
  y: number;
}

const MAX_SCALE = 6;
const DOUBLE_TAP_SCALE = 2.5;
const DOUBLE_TAP_MS = 300;
/** A finger that moved less than this tapped. */
const TAP_SLOP = 10;

const FIT: View = { scale: 1, x: 0, y: 0 };

const distance = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
const middle = (a: Point, b: Point) => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });

/**
 * A picture over the whole screen, to zoom into: two fingers pinch, one
 * moves it about once zoomed, a double tap zooms in on that spot or back
 * out. The gestures are the page's, not the browser's: the browser would
 * zoom the whole app.
 */
export function Zoom({ src, alt, onClose }: { src: string; alt: string; onClose: () => void }) {
  const stage = useRef<HTMLDivElement>(null);
  const close = useRef<HTMLButtonElement>(null);
  const [view, setView] = useState<View>(FIT);
  const [easing, setEasing] = useState(false);
  const current = useRef(view);
  current.current = view;
  const pointers = useRef(new Map<number, Point>());
  /** The view and the fingers when the gesture, or its last change of fingers, began. */
  const start = useRef<{ view: View; points: Point[]; moved: boolean } | null>(null);
  const lastTap = useRef<{ at: number; point: Point } | null>(null);

  useEffect(() => {
    close.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [onClose]);

  /** Never smaller than the screen, never off it. */
  const within = (next: View): View => {
    const box = stage.current?.getBoundingClientRect();
    if (!box) return FIT;
    const scale = Math.min(MAX_SCALE, Math.max(1, next.scale));
    return {
      scale,
      x: Math.min(0, Math.max(box.width - box.width * scale, next.x)),
      y: Math.min(0, Math.max(box.height - box.height * scale, next.y)),
    };
  };

  const pointOf = (event: PointerEvent): Point => {
    const box = stage.current!.getBoundingClientRect();
    return { x: event.clientX - box.left, y: event.clientY - box.top };
  };

  const begin = (moved: boolean) => {
    start.current = { view: current.current, points: [...pointers.current.values()], moved };
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, pointOf(event));
    setEasing(false);
    // A second finger is a pinch, never a tap.
    begin(pointers.current.size > 1);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!pointers.current.has(event.pointerId) || !start.current) return;
    pointers.current.set(event.pointerId, pointOf(event));
    const from = start.current;
    const points = [...pointers.current.values()];
    if (points.length >= 2 && from.points.length >= 2) {
      const [a, b] = points;
      const scale = (from.view.scale * distance(a, b)) / Math.max(1, distance(from.points[0], from.points[1]));
      // The spot under the fingers when they came down stays under them.
      const was = middle(from.points[0], from.points[1]);
      const now = middle(a, b);
      setView(
        within({
          scale,
          x: now.x - ((was.x - from.view.x) / from.view.scale) * scale,
          y: now.y - ((was.y - from.view.y) / from.view.scale) * scale,
        }),
      );
      from.moved = true;
    } else if (points.length === 1) {
      const dx = points[0].x - from.points[0].x;
      const dy = points[0].y - from.points[0].y;
      if (Math.hypot(dx, dy) > TAP_SLOP) from.moved = true;
      if (from.view.scale > 1) setView(within({ ...from.view, x: from.view.x + dx, y: from.view.y + dy }));
    }
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (!pointers.current.delete(event.pointerId)) return;
    const from = start.current;
    if (pointers.current.size > 0) {
      begin(true);
      return;
    }
    start.current = null;
    if (!from || from.moved || event.type === "pointercancel") return;
    const point = pointOf(event);
    const previous = lastTap.current;
    if (previous && event.timeStamp - previous.at < DOUBLE_TAP_MS && distance(previous.point, point) < 3 * TAP_SLOP) {
      lastTap.current = null;
      setEasing(true);
      const view = current.current;
      setView(
        view.scale > 1
          ? FIT
          : within({
              scale: DOUBLE_TAP_SCALE,
              x: point.x - ((point.x - view.x) / view.scale) * DOUBLE_TAP_SCALE,
              y: point.y - ((point.y - view.y) / view.scale) * DOUBLE_TAP_SCALE,
            }),
      );
    } else {
      lastTap.current = { at: event.timeStamp, point };
    }
  };

  return createPortal(
    <div className="zoom" role="dialog" aria-modal="true" aria-label={alt}>
      <div
        ref={stage}
        className="zoom__stage"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        <img
          className={easing ? "zoom__img zoom__img--ease" : "zoom__img"}
          src={src}
          alt={alt}
          draggable={false}
          style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}
        />
      </div>
      <button ref={close} type="button" className="zoom__close" onClick={onClose} aria-label={t("shares.close")}>
        <Icon name="close" size={22} />
      </button>
      {view.scale === 1 && (
        <p className="zoom__hint" aria-hidden="true">
          {t("shares.zoomHint")}
        </p>
      )}
    </div>,
    document.body,
  );
}
