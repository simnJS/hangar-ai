import { describe, expect, it } from "vitest";
import type { AgentActivity } from "./agentState";
import { createManagerFeed, settleManager, type ManagerJournalEntry } from "./managerEvents";
import { TIMING, type BoardTask } from "./orchestratorEvents";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

function pane(id: string, name: string, role?: PaneRole): Pane {
  return { id, name, agent: "claude", sessionId: null, cwd: null, shellId: null, title: null, ...(role ? { role } : {}) };
}

function workspace(id: string, name: string, panes: Pane[], hq = false): Workspace {
  return {
    id,
    name,
    cwd: `C:\\dev\\${id}`,
    extraRoots: [],
    panes,
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
    ...(hq ? { hq: true } : {}),
  };
}

function task(id: string, patch: Partial<BoardTask> = {}): BoardTask {
  return {
    id,
    title: `Task ${id}`,
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
    ...patch,
  };
}

function fakeTime() {
  let t = 1_000_000;
  let next = 1;
  const timers = new Map<number, { at: number; run: () => void }>();
  const settleAll = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  };
  return {
    now: () => t,
    setTimer: (run: () => void, ms: number) => {
      const id = next++;
      timers.set(id, { at: t + ms, run });
      return id;
    },
    clearTimer: (id: unknown) => {
      timers.delete(id as number);
    },
    async advance(ms: number) {
      const end = t + ms;
      for (let fired = 0; ; fired++) {
        if (fired > 1_000) throw new Error("timers re-arm themselves without end");
        const due = [...timers.entries()].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        timers.delete(due[0]);
        t = Math.max(t, due[1].at);
        due[1].run();
        await settleAll();
      }
      t = end;
      await settleAll();
    },
  };
}

function rig(options: { manager?: boolean } = {}) {
  const time = fakeTime();
  const withManager = options.manager ?? true;
  const state: AppState = {
    workspaces: [
      workspace("hq", "HQ", [pane("m", "manager", withManager ? "manager" : undefined), pane("hw", "hq-agent")], true),
      workspace("a", "Shop", [pane("o", "orch", "orchestrator"), pane("w1", "worker-1", "worker")]),
      workspace("b", "Blog", [pane("p", "solo")]),
    ],
    folders: [],
    activeWorkspaceId: "a",
    settings: DEFAULT_SETTINGS,
  };
  const activities = new Map<string, AgentActivity>([["m", "idle"]]);
  const boards = new Map<string, BoardTask[]>([
    ["C:\\dev\\hq", [task("h1")]],
    ["C:\\dev\\a", [task("t1", { column: "doing" }), task("t2")]],
    ["C:\\dev\\b", [task("u1", { column: "doing" })]],
  ]);
  const sent: { paneId: string; text: string; from: string }[] = [];
  const feed = createManagerFeed({
    snapshot: () => state,
    enqueue: async (paneId, text, from) => {
      sent.push({ paneId, text, from });
    },
    activity: (id) => {
      const activity = activities.get(id);
      return activity ? { workspaceId: "x", activity, bridged: true } : undefined;
    },
    loadBoard: async (cwd) => boards.get(cwd) ?? [],
    now: time.now,
    setTimer: time.setTimer,
    clearTimer: time.clearTimer,
  });
  return { feed, time, state, sent, activities, boards };
}

/** The baseline read, then the batch window. */
async function primed(r: ReturnType<typeof rig>) {
  await r.feed.tick();
}

describe("manager feed", () => {
  it("tells a pane waiting for the person once, with workspace and pane", async () => {
    const r = rig();
    await primed(r);
    r.activities.set("w1", "waiting");
    await r.feed.tick();
    await r.time.advance(TIMING.waitingMs);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
    expect(r.sent[0]).toMatchObject({ paneId: "m", from: "hangar" });
    expect(r.sent[0].text).toContain('Workspace "Shop"');
    expect(r.sent[0].text).toContain('"worker-1"');
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
  });

  it("covers an orchestrator and a workspace that has none", async () => {
    const r = rig();
    await primed(r);
    r.activities.set("o", "waiting");
    r.activities.set("p", "waiting");
    await r.feed.tick();
    await r.time.advance(TIMING.waitingMs);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
    expect(r.sent[0].text).toContain('"orch"');
    expect(r.sent[0].text).toContain('Workspace "Blog": pane "solo"');
  });

  it("does not say a wait shorter than the threshold", async () => {
    const r = rig();
    await primed(r);
    r.activities.set("w1", "waiting");
    await r.feed.tick();
    await r.time.advance(TIMING.waitingMs - 1_000);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(0);
  });

  it("drops a wait that ended before the batch goes", async () => {
    const r = rig();
    await primed(r);
    r.activities.set("w1", "waiting");
    await r.feed.tick();
    await r.time.advance(TIMING.waitingMs);
    await r.feed.tick();
    r.activities.set("w1", "working");
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(0);
  });

  it("tells a task that gets needs-you, a delivery ready to land and a board that is done", async () => {
    const r = rig();
    await primed(r);
    r.boards.set("C:\\dev\\a", [
      task("t1", { column: "review", labels: ["ready-to-land"] }),
      task("t2", {
        labels: ["needs-you"],
        comments: [{ id: "c1", author: "orch", text: "Which db?", created_at: 1 } as BoardTask["comments"][number]],
      }),
    ]);
    r.boards.set("C:\\dev\\b", [task("u1", { column: "done" })]);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
    const text = r.sent[0].text;
    expect(text).toContain("3 things wait for the person");
    expect(text).toContain('Workspace "Shop": "Task t2" [t2] needs the person: orch: Which db?');
    expect(text).toContain('Workspace "Shop": the delivery of "Task t1" [t1] is ready to land');
    expect(text).toContain('Workspace "Blog": every task on the board is done (1)');
    // Once: the same labels read again are no news.
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
  });

  it("says nothing of what the boards already held, nor an empty board", async () => {
    const r = rig();
    r.boards.set("C:\\dev\\a", [task("t1", { labels: ["needs-you"] })]);
    r.boards.set("C:\\dev\\b", []);
    await primed(r);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(0);
  });

  it("says nothing of the HQ", async () => {
    const r = rig();
    await primed(r);
    r.boards.set("C:\\dev\\hq", [task("h1", { column: "done", labels: ["needs-you", "ready-to-land"] })]);
    r.activities.set("hw", "waiting");
    await r.feed.tick();
    await r.time.advance(TIMING.waitingMs);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(0);
  });

  it("says nothing else: moves, comments, worker starts", async () => {
    const r = rig();
    await primed(r);
    r.boards.set("C:\\dev\\a", [
      task("t1", { column: "review", assignee: "worker-1" }),
      task("t2", {
        column: "doing",
        comments: [{ id: "c1", author: "worker-1", text: "done", created_at: 1 } as BoardTask["comments"][number]],
      }),
      task("t3"),
    ]);
    r.activities.set("w1", "working");
    r.activities.set("o", "yours");
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(0);
    expect(r.feed.journal()).toHaveLength(0);
  });

  it("sends nothing without a manager, or one that does not run", async () => {
    const none = rig({ manager: false });
    await primed(none);
    none.boards.set("C:\\dev\\a", [task("t1", { labels: ["needs-you"] }), task("t2")]);
    await none.feed.tick();
    await none.time.advance(TIMING.maxWaitMs);
    expect(none.sent).toHaveLength(0);
    expect(none.feed.journal()).toHaveLength(0);

    const asleep = rig();
    await primed(asleep);
    asleep.activities.delete("m");
    asleep.boards.set("C:\\dev\\a", [task("t1", { labels: ["needs-you"] }), task("t2")]);
    await asleep.feed.tick();
    await asleep.time.advance(TIMING.maxWaitMs);
    expect(asleep.sent).toHaveLength(0);
  });

  it("holds the batch while the manager is in a turn", async () => {
    const r = rig();
    await primed(r);
    r.activities.set("m", "working");
    r.boards.set("C:\\dev\\a", [task("t1", { labels: ["needs-you"] }), task("t2")]);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(0);
    r.activities.set("m", "yours");
    await r.time.advance(TIMING.holdMs + TIMING.minGapMs);
    expect(r.sent).toHaveLength(1);
  });

  it("batches what comes together, keeps a journal", async () => {
    const r = rig();
    await primed(r);
    r.boards.set("C:\\dev\\a", [task("t1", { labels: ["needs-you"] }), task("t2", { labels: ["ready-to-land"] })]);
    await r.feed.tick();
    await r.time.advance(TIMING.debounceMs - 1_000);
    r.boards.set("C:\\dev\\b", [task("u1", { column: "done" })]);
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
    expect(r.feed.journal().map((e) => e.event.kind)).toEqual(["taskNeedsYou", "readyToLand", "boardDone"]);
  });

  it("caps the lines of a message and counts the rest", async () => {
    const r = rig();
    const many = Array.from({ length: 20 }, (_, i) => task(`n${i}`));
    r.boards.set("C:\\dev\\a", many);
    await primed(r);
    r.boards.set("C:\\dev\\a", many.map((t) => ({ ...t, labels: ["needs-you"] })));
    await r.feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(r.sent).toHaveLength(1);
    const lines = r.sent[0].text.split("\n");
    expect(lines[0]).toBe("20 things wait for the person:");
    expect(lines).toHaveLength(1 + 12 + 1);
    expect(lines[lines.length - 1]).toContain("…and 8 more");
  });
});

describe("settleManager", () => {
  const entry = (event: ManagerJournalEntry["event"]): ManagerJournalEntry => ({ at: 1, event });
  const waiting = (paneId: string) =>
    entry({ kind: "paneWaiting", workspaceId: "a", workspace: "Shop", paneId, pane: paneId, forMs: 120_000 });

  it("folds repeats and ranks the waits first", () => {
    const out = settleManager(
      [
        entry({ kind: "boardDone", workspaceId: "a", workspace: "Shop", tasks: 2 }),
        waiting("w1"),
        waiting("w1"),
      ],
      () => true,
    );
    expect(out.map((e) => e.kind)).toEqual(["paneWaiting", "boardDone"]);
  });

  it("drops a wait that ended", () => {
    expect(settleManager([waiting("w1")], () => false)).toEqual([]);
  });
});
