import { describe, expect, it } from "vitest";
import type { AgentActivity } from "./agentState";
import {
  TIMING,
  createOrchestratorEngine,
  diffBoard,
  readyTasks,
  renderDelta,
  settle,
  snapBoard,
  type BoardTask,
  type JournalEntry,
  type OrchestratorEvent,
} from "./orchestratorEvents";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

function pane(id: string, name: string, role?: PaneRole, taskId?: string): Pane {
  return {
    id,
    name,
    agent: "claude",
    sessionId: null,
    cwd: null,
    shellId: null,
    title: null,
    ...(role ? { role } : {}),
    ...(taskId ? { taskId } : {}),
  };
}

function workspace(id: string, panes: Pane[]): Workspace {
  return {
    id,
    name: id === "ws" ? "Shop" : id,
    cwd: `C:\\dev\\${id}`,
    extraRoots: [],
    panes,
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
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

/** Timers and a clock the test moves by hand. */
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
        // A timer that keeps re-arming itself at once is a hot loop in the app.
        if (fired > 1_000) throw new Error("timers re-arm themselves without end");
        const due = [...timers.entries()]
          .filter(([, timer]) => timer.at <= end)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        timers.delete(due[0]);
        t = Math.max(t, due[1].at);
        due[1].run();
        await settleAll();
      }
      t = end;
      await settleAll();
    },
    settleAll,
  };
}

function rig(panes: Pane[] = [pane("o", "orch", "orchestrator"), pane("w1", "worker-1", "worker", "t1")]) {
  const time = fakeTime();
  const activities = new Map<string, AgentActivity>();
  const state: AppState = {
    workspaces: [workspace("ws", panes), workspace("plain", [pane("p", "solo")])],
    folders: [],
    activeWorkspaceId: "ws",
    settings: DEFAULT_SETTINGS,
  };
  let tasks: BoardTask[] = [task("t1", { column: "doing", assignee: "worker-1" }), task("t2")];
  const sent: { paneId: string; text: string; from: string }[] = [];
  const engine = createOrchestratorEngine({
    snapshot: () => state,
    enqueue: async (paneId, text, from) => {
      sent.push({ paneId, text, from });
    },
    activity: (id) => {
      const activity = activities.get(id);
      return activity ? { workspaceId: "ws", activity, bridged: true } : undefined;
    },
    loadBoard: async () => tasks,
    now: time.now,
    setTimer: time.setTimer,
    clearTimer: time.clearTimer,
  });
  return {
    engine,
    time,
    state,
    sent,
    activities,
    setTasks: (next: BoardTask[]) => {
      tasks = next;
    },
  };
}

const delivery = (outcome: "landed" | "checkFailed", taskId = "t1"): OrchestratorEvent => ({
  kind: "delivery",
  taskId,
  title: `Task ${taskId}`,
  worker: "worker-1",
  outcome,
  detail: outcome === "landed" ? "check passed" : "pnpm test exited 1",
});

describe("batching", () => {
  it("sends the events of a few seconds as one message from Hangar", async () => {
    const { engine, time, sent } = rig();
    engine.post("ws", delivery("checkFailed"));
    await time.advance(2_000);
    engine.post("ws", { kind: "taskCommented", taskId: "t1", title: "Task t1", by: "worker-1", text: "Fixing the test" });
    await time.advance(2_000);
    expect(sent).toHaveLength(0);
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    expect(sent[0].paneId).toBe("o");
    expect(sent[0].from).toBe("hangar");
    const lines = sent[0].text.split("\n");
    expect(lines[0]).toBe('2 updates in workspace "Shop":');
    expect(lines[1]).toContain('Delivery of "Task t1" [t1] by worker-1 failed the check');
    expect(lines[1]).toContain("pnpm test exited 1");
    expect(lines[2]).toBe('- worker-1 commented on "Task t1" [t1]: "Fixing the test"');
  });

  it("does not let a steady stream postpone the first event for good", async () => {
    const { engine, time, sent } = rig();
    for (let i = 0; i < 12; i++) {
      engine.post("ws", { kind: "taskCommented", taskId: "t1", title: "Task t1", by: `w${i}`, text: `note ${i}` });
      await time.advance(3_000);
    }
    expect(sent.length).toBeGreaterThanOrEqual(1);
    expect(sent[0].text).toContain("note 0");
  });

  it("holds the batch while the orchestrator is in a turn, and folds what comes meanwhile into it", async () => {
    const { engine, time, sent, activities } = rig();
    activities.set("o", "working");
    engine.post("ws", delivery("checkFailed"));
    await time.advance(TIMING.debounceMs + 1);
    engine.post("ws", { kind: "workerExited", worker: "worker-1", taskId: "t1" });
    await time.advance(60_000);
    expect(sent).toHaveLength(0);
    activities.set("o", "yours");
    await time.advance(TIMING.holdMs + TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toMatch(/^2 updates/);
  });

  it("believes the mod over a pane still showing the 'working' of its startup", async () => {
    const { engine, time, sent, activities } = rig();
    activities.set("o", "working");
    engine.bridgeEvent("o", { kind: "hello", sessionId: "s", model: "sonnet", version: "2" });
    engine.post("ws", delivery("checkFailed"));
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);

    engine.bridgeEvent("o", { kind: "turn", phase: "start" });
    engine.post("ws", delivery("landed"));
    await time.advance(TIMING.minGapMs + TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    engine.bridgeEvent("o", { kind: "permission", waiting: true });
    engine.bridgeEvent("o", { kind: "turn", phase: "end", reason: "answer", durationMs: 1, answer: "" });
    await time.advance(TIMING.holdMs);
    expect(sent).toHaveLength(2);
  });

  it("keeps two messages apart", async () => {
    const { engine, time, sent } = rig();
    engine.post("ws", delivery("checkFailed"));
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    engine.post("ws", delivery("landed"));
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    await time.advance(TIMING.minGapMs);
    expect(sent).toHaveLength(2);
  });

  it("says nothing, and keeps nothing, for a workspace without an orchestrator", async () => {
    const { engine, time, sent } = rig([pane("w1", "worker-1", "worker", "t1")]);
    engine.post("ws", delivery("landed"));
    engine.post("plain", delivery("landed"));
    engine.post("nowhere", delivery("landed"));
    await time.advance(60_000);
    await engine.tick();
    expect(sent).toHaveLength(0);
    expect(engine.journal("ws")).toEqual([]);
    expect(engine.journal("plain")).toEqual([]);
  });

  it("drops what is pending when the orchestrator is unpinned before it goes", async () => {
    const { engine, time, sent, state } = rig();
    engine.post("ws", delivery("landed"));
    state.workspaces[0].panes[0] = { ...state.workspaces[0].panes[0], role: undefined };
    await time.advance(60_000);
    expect(sent).toHaveLength(0);
  });

  it("keeps what the orchestrator did itself out of its messages, but in its journal", async () => {
    const { engine, time, sent, activities } = rig();
    activities.set("o", "working");
    engine.post("ws", { kind: "workerStarted", worker: "worker-1", taskId: "t1" });
    activities.set("o", "yours");
    await time.advance(60_000);
    expect(sent).toHaveLength(0);
    expect(engine.journal("ws")).toMatchObject([{ quiet: true, event: { kind: "workerStarted" } }]);
  });

  it("keeps the last fifty events of a workspace", () => {
    const { engine } = rig();
    for (let i = 0; i < 70; i++) {
      engine.post("ws", { kind: "taskCommented", taskId: "t1", title: "Task t1", by: "worker-1", text: `n${i}` });
    }
    const journal = engine.journal("ws");
    expect(journal).toHaveLength(50);
    expect(journal[0].event).toMatchObject({ text: "n20" });
  });
});

describe("the board", () => {
  it("takes the first read as it is, then tells moves, comments and needs-you", async () => {
    const { engine, time, sent, setTasks } = rig();
    await engine.tick();
    await time.advance(60_000);
    expect(sent).toHaveLength(0);

    setTasks([
      task("t1", {
        column: "review",
        assignee: "worker-1",
        comments: [
          { id: "c1", author: "worker-1", text: "Done.\nRan pnpm test: 12 passed", created_at: 2 },
          { id: "c2", author: "orch", text: "Thanks", created_at: 3 },
          { id: "c3", author: "hangar", text: "Landed", created_at: 4 },
        ],
      }),
      task("t2", { labels: ["needs-you"], comments: [{ id: "c4", author: "user", text: "Which API?", created_at: 5 }] }),
      task("t3"),
    ]);
    engine.boardChanged();
    await time.advance(300);
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    const text = sent[0].text;
    // The person's question is told once, as what the task needs.
    expect(text).toMatch(/^3 updates/);
    expect(text).toContain('"Task t2" [t2] needs the person: user: Which API?');
    expect(text).toContain('worker-1 commented on "Task t1" [t1]: "Done. / Ran pnpm test: 12 passed"');
    expect(text).not.toContain("The person commented");
    expect(text).toContain('"Task t1" [t1] moved doing → review');
    expect(text).not.toContain("Thanks");
    expect(text).not.toContain("Landed");
    expect(text).not.toContain("t3");
  });

  it("takes a move made during the orchestrator's turn for its own", () => {
    const before = snapBoard([task("a"), task("b", { column: "doing" })]);
    const after = [task("a", { column: "doing" }), task("b", { column: "doing", labels: ["needs-you"] })];
    const own = diffBoard(before, after, { name: "orch", busy: true });
    expect(own).toEqual([
      { event: expect.objectContaining({ kind: "taskMoved", by: "orch" }), quiet: true },
      { event: expect.objectContaining({ kind: "taskNeedsYou", reason: "labelled needs-you" }), quiet: true },
    ]);
    const theirs = diffBoard(before, after, { name: "orch", busy: false });
    expect(theirs.map((change) => change.quiet)).toEqual([false, false]);
    expect(theirs[0].event).toMatchObject({ by: null });
  });
});

describe("settling a batch", () => {
  const entry = (event: OrchestratorEvent, at = 0, quiet = false): JournalEntry => ({ at, event, quiet });
  const live = (activity: Record<string, AgentActivity> = {}, columns: Record<string, string> = {}) => ({
    activity: (worker: string) => activity[worker],
    column: (taskId: string) => columns[taskId],
  });

  it("folds the moves of one task into one, and drops a round trip", () => {
    const moves = [
      entry({ kind: "taskMoved", taskId: "a", title: "A", from: "todo", to: "doing", by: null }),
      entry({ kind: "taskMoved", taskId: "a", title: "A", from: "doing", to: "review", by: null }),
      entry({ kind: "taskMoved", taskId: "b", title: "B", from: "todo", to: "doing", by: null }),
      entry({ kind: "taskMoved", taskId: "b", title: "B", from: "doing", to: "todo", by: null }),
    ];
    expect(settle(moves, moves, live())).toEqual([
      { kind: "taskMoved", taskId: "a", title: "A", from: "todo", to: "review", by: null },
    ]);
  });

  it("drops what a more telling event already says", () => {
    const started = entry({ kind: "workerStarted", worker: "w", taskId: "a" }, 0, true);
    const landed = entry(delivery("landed", "b"), 0);
    const stopped = entry({ kind: "workerStopped", worker: "x", taskId: "c" }, 0);
    const batch = [
      entry({ kind: "taskMoved", taskId: "a", title: "A", from: "todo", to: "doing", by: null }, 2_000),
      entry({ kind: "taskMoved", taskId: "b", title: "B", from: "doing", to: "done", by: null }, 3_000),
      entry({ kind: "workerExited", worker: "x", taskId: "c" }, 1_000),
    ];
    expect(settle(batch, [started, landed, stopped, ...batch], live())).toEqual([]);
    // …unless it is about something else, or long ago.
    const old = { ...started, at: -10 * TIMING.nearMs };
    expect(settle(batch.slice(0, 1), [old, ...batch], live())).toHaveLength(1);
  });

  it("drops a wait or an idle the moment has overtaken", () => {
    const batch = [
      entry({ kind: "workerWaiting", worker: "w1", taskId: "a", forMs: 150_000 }),
      entry({ kind: "workerWaiting", worker: "w2", taskId: "b", forMs: 150_000 }),
      entry({ kind: "workerIdle", worker: "w3", taskId: "c", forMs: 700_000 }),
      entry({ kind: "workerIdle", worker: "w4", taskId: "d", forMs: 700_000 }),
    ];
    const now = live({ w1: "waiting", w2: "working", w3: "yours", w4: "yours" }, { c: "doing", d: "done" });
    expect(settle(batch, batch, now).map((e) => ("worker" in e ? e.worker : null))).toEqual(["w1", "w3"]);
  });

  it("removes repeats, keeps a worker's last two comments, and puts deliveries first", () => {
    const comment = (text: string): OrchestratorEvent => ({
      kind: "taskCommented",
      taskId: "a",
      title: "A",
      by: "w1",
      text,
    });
    const batch = [
      entry({ kind: "workerStarted", worker: "w2", taskId: "b" }),
      entry(comment("one")),
      entry(comment("two")),
      entry(comment("two")),
      entry(comment("three")),
      entry(delivery("checkFailed", "c")),
    ];
    expect(settle(batch, batch, live())).toEqual([
      delivery("checkFailed", "c"),
      comment("two"),
      comment("three"),
      { kind: "workerStarted", worker: "w2", taskId: "b" },
    ]);
  });
});

describe("the message", () => {
  it("lists a dozen lines at most and counts the rest", () => {
    const events: OrchestratorEvent[] = Array.from({ length: 15 }, (_, i) => ({
      kind: "workerStarted",
      worker: `w${i}`,
      taskId: null,
    }));
    const lines = renderDelta(events, [], "Shop").split("\n");
    expect(lines).toHaveLength(14);
    expect(lines[13]).toBe("- …and 3 more: board_list_tasks and worker_list have the rest.");
  });

  it("names the tasks a landing made ready, or says the board is done", () => {
    const tasks = [
      task("a", { column: "done" }),
      task("b", { depends_on: ["a"], priority: 2 }),
      task("c", { depends_on: ["a", "z"] }),
      task("d", { labels: ["needs-you"] }),
      task("e", { assignee: "w1" }),
    ];
    expect(readyTasks(tasks).map((t) => t.id)).toEqual(["b"]);
    expect(renderDelta([delivery("landed", "a")], tasks, "Shop")).toContain('Ready to start: "Task b" [b].');
    expect(renderDelta([delivery("checkFailed", "a")], tasks, "Shop")).not.toContain("Ready to start");
    expect(renderDelta([delivery("landed", "a")], [task("a", { column: "done" })], "Shop")).toContain(
      "Every task on the board is done",
    );
  });

  it("names a worker's task by its title", () => {
    const text = renderDelta(
      [{ kind: "workerWaiting", worker: "w1", taskId: "a", forMs: 185_000 }],
      [task("a", { title: "Login form" })],
      "Shop",
    );
    expect(text).toContain('w1 has been waiting for the person for 3 min on "Login form" [a]');
  });
});

describe("the workers", () => {
  it("tells a wait for the person once it passes two minutes, once", async () => {
    const { engine, time, sent, activities } = rig();
    activities.set("w1", "working");
    await engine.tick();
    activities.set("w1", "waiting");
    await engine.tick();
    await time.advance(TIMING.waitingMs - 1_000);
    await engine.tick();
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(0);
    await time.advance(2_000);
    await engine.tick();
    await engine.tick();
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toContain("worker-1 has been waiting for the person for 2 min");
    await time.advance(5 * 60_000);
    await engine.tick();
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
  });

  it("tells a turn ended ten minutes ago without a delivery", async () => {
    const { engine, time, sent, activities } = rig();
    activities.set("w1", "working");
    await engine.tick();
    activities.set("w1", "yours");
    await engine.tick();
    await time.advance(TIMING.idleMs + 1_000);
    await engine.tick();
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toContain('worker-1 ended its turn 10 min ago without delivering on "Task t1" [t1]');
  });

  it("does not call a worker idle that delivered, or that never had a turn", async () => {
    const delivered = rig();
    delivered.activities.set("w1", "working");
    await delivered.engine.tick();
    delivered.activities.set("w1", "yours");
    await delivered.engine.tick();
    delivered.engine.post("ws", delivery("checkFailed"));
    await delivered.time.advance(TIMING.idleMs + 20_000);
    await delivered.engine.tick();
    await delivered.time.advance(TIMING.debounceMs);
    expect(delivered.sent.map((m) => m.text).join("\n")).not.toContain("without delivering");

    const fresh = rig();
    fresh.activities.set("w1", "idle");
    await fresh.engine.tick();
    await fresh.time.advance(TIMING.idleMs + 20_000);
    await fresh.engine.tick();
    await fresh.time.advance(TIMING.debounceMs);
    expect(fresh.sent).toHaveLength(0);
  });

  it("tells an exit once, whether Claude Code or its shell said it", async () => {
    const { engine, time, sent } = rig();
    await engine.tick();
    engine.bridgeEvent("w1", { kind: "end", reason: "exit" });
    engine.ptyExited("w1");
    engine.bridgeEvent("o", { kind: "end", reason: "exit" });
    await time.advance(TIMING.debounceMs);
    expect(sent).toHaveLength(1);
    expect(sent[0].text).toBe(
      '1 update in workspace "Shop":\n- worker-1\'s session ended on "Task t1" [t1]; the task is still assigned to it',
    );
  });

  it("tells a worker pane closed by hand, not one stopped or restarted", async () => {
    const closed = rig();
    await closed.engine.tick();
    closed.state.workspaces[0].panes.splice(1, 1);
    await closed.engine.tick();
    await closed.time.advance(TIMING.debounceMs);
    expect(closed.sent[0]?.text).toContain("worker-1's session ended");

    const stopped = rig();
    await stopped.engine.tick();
    stopped.engine.post("ws", { kind: "workerStopped", worker: "worker-1", taskId: "t1" });
    stopped.state.workspaces[0].panes.splice(1, 1);
    await stopped.engine.tick();
    await stopped.time.advance(TIMING.debounceMs);
    expect(stopped.sent).toHaveLength(1);
    expect(stopped.sent[0].text).not.toContain("session ended");

    const restarted = rig();
    await restarted.engine.tick();
    restarted.state.workspaces[0].panes[1] = { ...restarted.state.workspaces[0].panes[1], id: "w1b" };
    await restarted.engine.tick();
    await restarted.time.advance(TIMING.debounceMs);
    expect(restarted.sent).toHaveLength(0);
  });
});
