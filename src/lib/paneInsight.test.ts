import { describe, expect, it } from "vitest";
import type { Translator } from "../i18n";
import {
  EMPTY_INSIGHT,
  MAX_EVENTS,
  clearPaneInsight,
  getPaneInsight,
  paneEventText,
  queuedBy,
  recordEditedFile,
  recordPaneEvent,
  updatePaneInsight,
  type PaneEventDetail,
} from "./paneInsight";

/** The key and its parameters, so a test reads which sentence was picked. */
const t: Translator = (key, params) =>
  params ? `${key}(${Object.values(params).join(",")})` : key;

describe("paneEventText", () => {
  const text = (event: PaneEventDetail) => paneEventText(event, t);

  it("words a session as new or resumed", () => {
    expect(text({ kind: "session", params: { id: "01J8Z4", resumed: false } })).toBe(
      "pane.event.newSession",
    );
    expect(text({ kind: "session", params: { id: "01J8Z4", resumed: true } })).toBe(
      "pane.event.resumed(01J8Z4)",
    );
  });

  it("shows a turn's answer, or how it ended when there is none", () => {
    expect(text({ kind: "turn-start", params: {} })).toBe("pane.event.turnStart");
    const end = (outcome: "answer" | "aborted" | "failed", answer: string | null) =>
      text({ kind: "turn-end", params: { outcome, answer } });
    expect(end("answer", "Tests pass")).toBe("Tests pass");
    expect(end("answer", null)).toBe("pane.event.turnEnd");
    expect(end("failed", null)).toBe("pane.event.failed");
    expect(end("failed", "Rate limited")).toBe("Rate limited");
    // An interrupted turn says so, whatever it had written by then.
    expect(end("aborted", "Half an answer")).toBe("pane.event.aborted");
  });

  it("names what a permission is for", () => {
    const ask = (tool: string | null) => text({ kind: "permission", params: { tool } });
    expect(ask("Edit")).toBe("pane.event.permission(Edit)");
    expect(ask(null)).toBe("pane.event.permissionAny");
  });

  it("says when an edit followed another pane's on the same file", () => {
    expect(text({ kind: "edit", params: { file: "NOTES.md", overlap: "Ava" } })).toBe(
      "pane.event.editShared(NOTES.md,Ava)",
    );
    expect(text({ kind: "edit", params: { file: "NOTES.md", overlap: null } })).toBe(
      "pane.event.edit(NOTES.md)",
    );
  });

  it("words the rest with their one parameter", () => {
    expect(text({ kind: "edit", params: { file: "i18n.ts" } })).toBe("pane.event.edit(i18n.ts)");
    expect(text({ kind: "queued", params: { text: "run the tests" } })).toBe(
      "pane.event.queued(run the tests)",
    );
    expect(text({ kind: "queued", params: { text: "task done", from: "hangar" } })).toBe(
      "pane.event.queuedBy(Hangar,task done)",
    );
    expect(text({ kind: "queued", params: { text: "review it", from: "Ava" } })).toBe(
      "pane.event.queuedBy(Ava,review it)",
    );
    expect(text({ kind: "dropped", params: { text: "run the tests" } })).toBe(
      "pane.event.dropped(run the tests)",
    );
    expect(text({ kind: "exit", params: { what: "agent" } })).toBe("pane.event.end");
    expect(text({ kind: "exit", params: { what: "shell" } })).toBe("pane.event.exit");
  });
});

describe("queuedBy", () => {
  it("names who queued a prompt, and nobody for the person's own", () => {
    expect(queuedBy(undefined)).toBeNull();
    expect(queuedBy(null)).toBeNull();
    expect(queuedBy("Ava")).toBe("Ava");
    expect(queuedBy("hangar")).toBe("Hangar");
  });
});

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
    for (let i = 0; i < MAX_EVENTS + 5; i++) {
      recordPaneEvent("b", { kind: "edit", params: { file: `e${i}` } }, i);
    }
    const { events } = getPaneInsight("b");
    expect(events).toHaveLength(MAX_EVENTS);
    expect(events[0]).toEqual({ kind: "edit", params: { file: `e${MAX_EVENTS + 4}` }, at: MAX_EVENTS + 4 });
    clearPaneInsight("b");
  });

  it("keeps what happened, not a sentence about it", () => {
    recordPaneEvent("d", { kind: "turn-end", params: { outcome: "answer", answer: "Done" } }, 1);
    const [event] = getPaneInsight("d").events;
    expect(event).not.toHaveProperty("text");
    expect(paneEventText(event, t)).toBe("Done");
    clearPaneInsight("d");
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
