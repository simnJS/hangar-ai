import { describe, expect, it } from "vitest";
import { columnAtPoint, DRAG_THRESHOLD, movedBeyond, startsDrag } from "./boardDrag";

const columns = [
  { column: "todo", box: { left: 0, right: 100, top: 0, bottom: 500 } },
  { column: "doing", box: { left: 100, right: 200, top: 0, bottom: 500 } },
  { column: "done", box: { left: 200, right: 300, top: 0, bottom: 500 } },
] as const;

describe("movedBeyond", () => {
  it("keeps a press that barely moves a click", () => {
    expect(movedBeyond({ x: 10, y: 10 }, { x: 12, y: 11 })).toBe(false);
  });
  it("starts the drag at the threshold, in any direction", () => {
    expect(movedBeyond({ x: 0, y: 0 }, { x: DRAG_THRESHOLD, y: 0 })).toBe(true);
    expect(movedBeyond({ x: 0, y: 0 }, { x: 0, y: -DRAG_THRESHOLD })).toBe(true);
    expect(movedBeyond({ x: 0, y: 0 }, { x: 3, y: 4 })).toBe(true);
  });
});

describe("columnAtPoint", () => {
  it("finds the column under the pointer", () => {
    expect(columnAtPoint({ x: 50, y: 200 }, columns)).toBe("todo");
    expect(columnAtPoint({ x: 250, y: 10 }, columns)).toBe("done");
  });
  it("gives the shared edge to one column only", () => {
    expect(columnAtPoint({ x: 100, y: 10 }, columns)).toBe("doing");
  });
  it("is null outside every column", () => {
    expect(columnAtPoint({ x: 350, y: 10 }, columns)).toBeNull();
    expect(columnAtPoint({ x: 50, y: 600 }, columns)).toBeNull();
    expect(columnAtPoint({ x: 50, y: 10 }, [])).toBeNull();
  });
});

describe("startsDrag", () => {
  const target = (inside: boolean) => ({ closest: () => (inside ? {} : null) }) as unknown as EventTarget;
  it("drags with the primary button only", () => {
    expect(startsDrag(0, target(false))).toBe(true);
    expect(startsDrag(2, target(false))).toBe(false);
    expect(startsDrag(1, target(false))).toBe(false);
  });
  it("leaves the controls inside a card alone", () => {
    expect(startsDrag(0, target(true))).toBe(false);
  });
});
