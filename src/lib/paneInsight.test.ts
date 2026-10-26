import { describe, expect, it } from "vitest";
import {
  EMPTY_INSIGHT,
  MAX_EVENTS,
  clearPaneInsight,
  getPaneInsight,
  recordEditedFile,
  recordPaneEvent,
  updatePaneInsight,
} from "./paneInsight";

describe("paneInsight", () => {
  it("is empty for a pane that never reported", () => {
    expect(getPaneInsight("nobody")).toBe(EMPTY_INSIGHT);
  });

  it("keeps the same object when a patch changes nothing", () => {
    updatePaneInsight("a", { model: "claude-opus-5", costUsd: 1.2 });
    const before = getPaneInsight("a");
    updatePaneInsight("a", { model: "claude-opus-5", costUsd: 1.2 });
    expect(getPaneInsight("a")).toBe(before);
    updatePaneInsight("a", {
      context: { pct: 42, usedTokens: 84_000, window: 200_000, model: null },
    });
    const withGauge = getPaneInsight("a");
    updatePaneInsight("a", {
      context: { pct: 42, usedTokens: 84_000, window: 200_000, model: null },
    });
    expect(getPaneInsight("a")).toBe(withGauge);
    clearPaneInsight("a");
  });

  it("lists events newest first and caps the history", () => {
    for (let i = 0; i < MAX_EVENTS + 5; i++) recordPaneEvent("b", "edit", `e${i}`, i);
    const { events } = getPaneInsight("b");
    expect(events).toHaveLength(MAX_EVENTS);
    expect(events[0].text).toBe(`e${MAX_EVENTS + 4}`);
    clearPaneInsight("b");
  });

  it("lists an edited file once, at the top", () => {
    recordEditedFile("c", "src/a.ts");
    recordEditedFile("c", "src/b.ts");
    recordEditedFile("c", "src/a.ts");
    expect(getPaneInsight("c").editedFiles).toEqual(["src/a.ts", "src/b.ts"]);
    clearPaneInsight("c");
    expect(getPaneInsight("c")).toBe(EMPTY_INSIGHT);
  });
});
