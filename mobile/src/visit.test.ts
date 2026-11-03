import { describe, expect, it } from "vitest";
import { VISIT_GAP_MS, parseVisit, touchVisit } from "./visit";

const MIN = 60_000;

describe("visits", () => {
  it("starts the first visit with nothing before it", () => {
    expect(touchVisit({ lastSeen: null, since: null }, 1_000)).toEqual({ lastSeen: 1_000, since: null });
  });

  it("stays the same visit while the app is in use", () => {
    const first = touchVisit({ lastSeen: null, since: null }, 0);
    const later = touchVisit(touchVisit(first, 10 * MIN), 25 * MIN);
    expect(later).toEqual({ lastSeen: 25 * MIN, since: null });
  });

  it("begins a new visit after a while away, since the end of the last", () => {
    const visit = { lastSeen: 100 * MIN, since: 40 * MIN };
    expect(touchVisit(visit, 100 * MIN + VISIT_GAP_MS - 1)).toEqual({ lastSeen: 100 * MIN + VISIT_GAP_MS - 1, since: 40 * MIN });
    expect(touchVisit(visit, 100 * MIN + VISIT_GAP_MS)).toEqual({ lastSeen: 100 * MIN + VISIT_GAP_MS, since: 100 * MIN });
  });

  it("keeps the new visit's start through the visit", () => {
    const back = touchVisit({ lastSeen: 0, since: null }, 7 * 24 * 60 * MIN);
    expect(touchVisit(back, back.lastSeen! + 5 * MIN).since).toBe(0);
  });

  it("reads what the phone kept, whatever state it is in", () => {
    expect(parseVisit(JSON.stringify({ lastSeen: 5, since: 2 }))).toEqual({ lastSeen: 5, since: 2 });
    expect(parseVisit(null)).toEqual({ lastSeen: null, since: null });
    expect(parseVisit("{not json")).toEqual({ lastSeen: null, since: null });
    expect(parseVisit(JSON.stringify({ lastSeen: "yesterday", since: null }))).toEqual({ lastSeen: null, since: null });
  });
});
