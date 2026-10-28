import { describe, expect, it } from "vitest";
import type { Pane, Project, RemoteState, Share, Waiting } from "./api";
import {
  byDay,
  cleanScreen,
  destinationOf,
  digitsOf,
  fleetOf,
  forYouGroups,
  isImage,
  locatePane,
  mergeShares,
  screenLines,
  unseenShares,
  waitingKey,
} from "./model";

const pane = (id: string, name: string, role: string | null, activity: Pane["activity"]): Pane => ({
  id,
  name,
  agent: "claude",
  role,
  activity,
});

const project = (id: string, panes: Pane[], extra: Partial<Project> = {}): Project => ({
  id,
  name: id.toUpperCase(),
  open: true,
  asleep: false,
  git: true,
  orchestrator: null,
  panes,
  board: { todo: 0, doing: 0, review: 0, done: 0 },
  planStatus: null,
  needsYou: [],
  readyToLand: [],
  ...extra,
});

const state: RemoteState = {
  at: 0,
  manager: null,
  waiting: [],
  projects: [
    project(
      "a",
      [pane("p1", "Jack", "orchestrator", "working"), pane("p2", "Ada", "worker", "waiting"), pane("p3", "shell", null, null)],
      { orchestrator: { name: "Jack", activity: "working" } },
    ),
    project("b", [pane("p4", "Mia", null, "yours"), pane("p5", "Leo", null, "idle")], { orchestrator: { name: "Mia", activity: "yours" } }),
    project("c", []),
  ],
};

const waiting = (kind: Waiting["kind"], since: number, extra: Partial<Waiting> = {}): Waiting => ({
  kind,
  workspaceId: "a",
  workspace: "A",
  since,
  ...extra,
});

describe("what waits for the person", () => {
  it("groups panes, questions, then merges, the longest waiting first", () => {
    const groups = forYouGroups([
      waiting("readyToLand", 5, { taskId: "t1" }),
      waiting("pane", 30, { paneId: "p2" }),
      waiting("needsYou", 20, { taskId: "t2" }),
      waiting("pane", 10, { paneId: "p9" }),
    ]);
    expect(groups.map((group) => [group.kind, group.items.map((item) => item.since)])).toEqual([
      ["pane", [10, 30]],
      ["needsYou", [20]],
      ["readyToLand", [5]],
    ]);
  });

  it("leaves out the kinds with nothing", () => {
    expect(forYouGroups([])).toEqual([]);
  });

  it("keys each item the same from one poll to the next", () => {
    expect(waitingKey(waiting("pane", 1, { paneId: "p2" }))).toBe(waitingKey(waiting("pane", 2, { paneId: "p2" })));
    expect(waitingKey(waiting("needsYou", 1, { taskId: "t1" }))).not.toBe(waitingKey(waiting("needsYou", 1, { taskId: "t2" })));
  });
});

describe("where a card leads", () => {
  it("opens the waiting pane", () => {
    expect(destinationOf(waiting("pane", 0, { paneId: "p2", pane: "Ada" }), state)).toEqual({ to: "pane", pane: "p2" });
    expect(destinationOf(waiting("pane", 0, { pane: "Ada" }), state)).toEqual({ to: "pane", pane: "Ada" });
  });

  it("answers a question in the pane the PC names, else the orchestrator's", () => {
    expect(destinationOf(waiting("needsYou", 0, { taskId: "t", paneId: "p2" }), state)).toEqual({ to: "pane", pane: "p2" });
    expect(destinationOf(waiting("needsYou", 0, { taskId: "t" }), state)).toEqual({ to: "pane", pane: "p1" });
  });

  it("finds an orchestrator by name when no pane has its role", () => {
    const other = waiting("needsYou", 0, { workspaceId: "b", taskId: "t" });
    expect(destinationOf(other, state)).toEqual({ to: "pane", pane: "p4" });
  });

  it("opens the project when there is no pane to answer in, or a merge to do on the PC", () => {
    expect(destinationOf(waiting("needsYou", 0, { workspaceId: "c", taskId: "t" }), state)).toEqual({ to: "project", projectId: "c" });
    expect(destinationOf(waiting("readyToLand", 0, { taskId: "t", paneId: "p1" }), state)).toEqual({ to: "project", projectId: "a" });
  });
});

describe("finding a pane", () => {
  it("by id, then by name, with its project", () => {
    expect(locatePane(state, "p5")?.project.id).toBe("b");
    expect(locatePane(state, "Ada")?.pane.id).toBe("p2");
    expect(locatePane(state, "nobody")).toBeNull();
  });
});

describe("the fleet", () => {
  it("draws every agent pane by project and counts them by state", () => {
    const fleet = fleetOf(state.projects);
    expect(fleet.projects.map((group) => [group.id, group.tiles.map((tile) => tile.activity)])).toEqual([
      ["a", ["working", "waiting"]],
      ["b", ["yours", "idle"]],
    ]);
    expect(fleet.counts).toEqual({ waiting: 1, yours: 1, working: 1, idle: 1 });
    expect(fleet.total).toBe(4);
  });
});

describe("shares", () => {
  const share = (id: string, at: number, mime = "image/png"): Share => ({
    id,
    at,
    workspace: "A",
    pane: "Ada",
    name: `${id}.png`,
    mime,
    size: 1,
    caption: null,
  });

  it("merge newest first, without duplicates", () => {
    const merged = mergeShares([share("a", 1), share("b", 3)], [share("c", 2), share("b", 3)]);
    expect(merged.map((item) => item.id)).toEqual(["b", "c", "a"]);
  });

  it("count the ones that came after the last look", () => {
    const shares = [share("b", 3), share("a", 1)];
    expect(unseenShares(shares, null)).toBe(2);
    expect(unseenShares(shares, 1)).toBe(1);
    expect(unseenShares(shares, 3)).toBe(0);
  });

  it("show raster images inline, never SVG or HTML", () => {
    expect(isImage(share("a", 1, "image/jpeg"))).toBe(true);
    expect(isImage(share("a", 1, "image/svg+xml"))).toBe(false);
    expect(isImage(share("a", 1, "text/html"))).toBe(false);
  });
});

describe("a pane's screen", () => {
  it("loses terminal escapes, trailing spaces and the blank lines at the bottom", () => {
    expect(cleanScreen("\u001b[32m✓\u001b[0m done   \r\n\u001b]0;title\u0007next\n\n\n")).toBe("✓ done\nnext");
  });

  it("unframes Claude Code's prompts so they fit a phone", () => {
    const text = [
      "⏺ Let me check.",
      "╭──────────────────────────────╮",
      "│ Bash command                 │",
      "│                              │",
      "│   pnpm vitest run            │",
      "│ ❯ 1. Yes                     │",
      "╰──────────────────────────────╯",
      "────────────────────────────────",
      "> ",
      "────────────────────────────────",
    ].join("\n");
    expect(screenLines(text)).toEqual([
      { kind: "text", text: "⏺ Let me check." },
      { kind: "rule" },
      { kind: "text", text: "Bash command" },
      { kind: "text", text: "" },
      { kind: "text", text: "  pnpm vitest run" },
      { kind: "text", text: "❯ 1. Yes" },
      { kind: "rule" },
      { kind: "text", text: ">" },
      { kind: "rule" },
    ]);
  });

  it("keeps a line that only mentions a box character", () => {
    expect(screenLines("a │ b\n--- not a rule")).toEqual([
      { kind: "text", text: "a │ b" },
      { kind: "text", text: "--- not a rule" },
    ]);
  });
});

describe("the history", () => {
  it("falls under the day each entry happened, newest first", () => {
    const now = new Date(2026, 9, 28, 20, 0).getTime();
    const entry = (at: number, text: string) => ({ at, workspace: "A", kind: "done", text });
    const days = byDay(
      [entry(new Date(2026, 9, 27, 9).getTime(), "old"), entry(now - 60_000, "new"), entry(now - 3_600_000, "mid")],
      now,
      "en",
    );
    expect(days.map((day) => [day.day, day.entries.map((item) => item.text)])).toEqual([
      ["Today", ["new", "mid"]],
      ["Yesterday", ["old"]],
    ]);
  });
});

describe("a pairing code", () => {
  it("keeps six digits of whatever was typed or pasted", () => {
    expect(digitsOf("123 456")).toBe("123456");
    expect(digitsOf("code: 98-76-54-32")).toBe("987654");
    expect(digitsOf("12a")).toBe("12");
  });
});
