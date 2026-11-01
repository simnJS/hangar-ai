/**
 * Moving a card between the Board's columns, with pointer events.
 *
 * The HTML5 drag-and-drop the Board used before never reaches its drop on
 * Windows: with `dragDropEnabled` on (which dropping a file on a pane needs),
 * Tauri registers its own drop target on the webview, so the page gets
 * `dragstart` and `dragend` but no `dragenter`, `dragover` or `drop`. Pointer
 * events do not go through the operating system's drag loop, so they work the
 * same everywhere.
 */

export interface Point {
  x: number;
  y: number;
}

export interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** How far the pointer has to travel before a press becomes a drag; below it,
    the press is a click and opens the card. */
export const DRAG_THRESHOLD = 5;

/** True once the pointer has left `from` by at least `threshold` pixels. */
export function movedBeyond(from: Point, to: Point, threshold = DRAG_THRESHOLD): boolean {
  return Math.hypot(to.x - from.x, to.y - from.y) >= threshold;
}

/** The column whose box holds the point, or null when it is over none. A point
    on the edge shared by two boxes belongs to the second. */
export function columnAtPoint<C extends string>(
  point: Point,
  columns: ReadonlyArray<{ column: C; box: Box }>,
): C | null {
  let found: C | null = null;
  for (const { column, box } of columns) {
    if (point.x >= box.left && point.x < box.right && point.y >= box.top && point.y <= box.bottom) {
      found = column;
    }
  }
  return found;
}

/** Only the primary button drags, and never from a control inside the card (the
    assignee's focus button, a link): those keep their own click. */
export function startsDrag(button: number, target: EventTarget | null): boolean {
  if (button !== 0) return false;
  const el = target as { closest?: (selector: string) => unknown } | null;
  return !(el && typeof el.closest === "function" && el.closest("button, a, input, select, textarea"));
}
