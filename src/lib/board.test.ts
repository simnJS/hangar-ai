import { describe, expect, it } from "vitest";
import {
  blockPatch,
  blockedMoveRefusal,
  isBlocked,
  isoDay,
  recheckProblem,
  unblockPatch,
  type Task,
} from "./board";

const task = (extra: Partial<Task> = {}): Task => ({
  id: "a",
  title: "t",
  description: "",
  column: "todo",
  priority: 1,
  assignee: null,
  labels: [],
  comments: [],
  depends_on: [],
  created_at: 1,
  updated_at: 1,
  order: 1,
  ...extra,
});

const blocked = { reason: "npm release", by: "Eli", since: 1, humanOnly: false };

describe("isBlocked", () => {
  it("reads an old task as not blocked", () => {
    expect(isBlocked(task())).toBe(false);
    expect(isBlocked(task({ blocked: null }))).toBe(false);
    expect(isBlocked(task({ blocked }))).toBe(true);
  });
});

describe("isoDay", () => {
  it("is the local day, padded", () => {
    expect(isoDay(new Date(2026, 0, 5))).toBe("2026-01-05");
    expect(isoDay(new Date(2026, 10, 2, 23, 59))).toBe("2026-11-02");
  });
});

describe("recheckProblem", () => {
  it("accepts nothing, today and later", () => {
    expect(recheckProblem("", "2026-11-02")).toBeNull();
    expect(recheckProblem("2026-11-02", "2026-11-02")).toBeNull();
    expect(recheckProblem("2027-01-01", "2026-11-02")).toBeNull();
  });
  it("refuses the past and what is not a day", () => {
    expect(recheckProblem("2026-11-01", "2026-11-02")).toBe("past");
    expect(recheckProblem("next week", "2026-11-02")).toBe("format");
    expect(recheckProblem("2026-02-30", "2026-11-02")).toBe("format");
  });
});

describe("blockedMoveRefusal", () => {
  it("gives the reason when a blocked card leaves todo", () => {
    expect(blockedMoveRefusal(task({ blocked }), "doing")).toBe("npm release");
    expect(blockedMoveRefusal(task({ blocked }), "done")).toBe("npm release");
  });
  it("lets it stay, and lets free cards go", () => {
    expect(blockedMoveRefusal(task({ blocked }), "todo")).toBeNull();
    expect(blockedMoveRefusal(task(), "doing")).toBeNull();
  });
});

describe("the patches", () => {
  it("block as the person, trimmed, with the recheck only when given", () => {
    expect(blockPatch("  wait  ", " ", true)).toEqual({
      block: { reason: "wait", human_only: true, by: "user", person: true },
    });
    expect(blockPatch("wait", "2026-12-01", false).block?.recheck).toBe("2026-12-01");
  });
  it("unblock as the person, with the note only when given", () => {
    expect(unblockPatch(" ")).toEqual({ unblock: { by: "user", person: true } });
    expect(unblockPatch("out").unblock?.note).toBe("out");
  });
});
