import { describe, expect, it } from "vitest";
import { arrowStep, stepIndex } from "./menuNav";

describe("stepIndex", () => {
  it("has nowhere to go in an empty list", () => {
    expect(stepIndex(-1, 0, 1, true)).toBeNull();
    expect(stepIndex(0, 0, -1, false)).toBeNull();
  });

  it("enters from outside at the top going down, at the bottom going up", () => {
    expect(stepIndex(-1, 4, 1, true)).toBe(0);
    expect(stepIndex(-1, 4, -1, true)).toBe(3);
    expect(stepIndex(-1, 4, -1, false)).toBe(3);
  });

  it("treats a stale index past the end as outside", () => {
    expect(stepIndex(7, 3, 1, false)).toBe(0);
  });

  it("moves one entry at a time inside the list", () => {
    expect(stepIndex(1, 4, 1, false)).toBe(2);
    expect(stepIndex(2, 4, -1, false)).toBe(1);
  });

  it("wraps around the ends of a menu", () => {
    expect(stepIndex(3, 4, 1, true)).toBe(0);
    expect(stepIndex(0, 4, -1, true)).toBe(3);
  });

  it("stops at the ends of a list", () => {
    expect(stepIndex(3, 4, 1, false)).toBeNull();
    expect(stepIndex(0, 4, -1, false)).toBeNull();
  });

  it("stays put on a single entry that wraps", () => {
    expect(stepIndex(0, 1, 1, true)).toBe(0);
    expect(stepIndex(0, 1, -1, true)).toBe(0);
  });
});

describe("arrowStep", () => {
  it("reads Up and Down, and nothing else", () => {
    expect(arrowStep("ArrowDown")).toBe(1);
    expect(arrowStep("ArrowUp")).toBe(-1);
    expect(arrowStep("ArrowLeft")).toBeNull();
    expect(arrowStep("Tab")).toBeNull();
  });
});
