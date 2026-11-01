import { describe, expect, it } from "vitest";
import {
  beginLaunch,
  endLaunch,
  finishesLaunch,
  launchesPane,
  markHandover,
  visibleLaunchNotes,
} from "./windows";

describe("who launches a pane after a hand-over", () => {
  it("the window that started the launch finishes it when the pane moves meanwhile", () => {
    beginLaunch("p1");
    markHandover(["p1"]);
    expect(finishesLaunch("p1")).toBe(true);
    endLaunch("p1");
    expect(finishesLaunch("p1")).toBe(false);
  });

  it("an unmount without a hand-over stops the launch", () => {
    beginLaunch("p2");
    expect(finishesLaunch("p2")).toBe(false);
    endLaunch("p2");
  });

  it("a pane already launched is not launched again by the window it leaves", () => {
    markHandover(["p3"]);
    expect(finishesLaunch("p3")).toBe(false);
  });

  it("the adopting window never launches: exactly one launcher", () => {
    expect(launchesPane(true)).toBe(false);
    expect(launchesPane(false)).toBe(true);
    // Hand-over before launch: the old window launches, the adopter does not.
    beginLaunch("p4");
    markHandover(["p4"]);
    const launchers = [finishesLaunch("p4"), launchesPane(true)].filter(Boolean);
    expect(launchers).toHaveLength(1);
    endLaunch("p4");
  });
});

describe("launch notes shown in the bar", () => {
  it("drops blanks, duplicates and dismissed notes", () => {
    expect(visibleLaunchNotes([" a ", "", "a", "b", "c"], new Set(["c"]))).toEqual(["a", "b"]);
  });
  it("nothing to show without notes", () => {
    expect(visibleLaunchNotes([], new Set())).toEqual([]);
  });
});
