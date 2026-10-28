import { describe, expect, it } from "vitest";
import type { AgentActivity } from "./agentState";
import { createManagerFeed, type ManagerEvent } from "./managerEvents";
import { TIMING, type BoardTask } from "./orchestratorEvents";
import {
  PUSH_TIMING,
  createRemotePush,
  pushMessage,
  settlePush,
  workspaceOfShare,
  type PushMessage,
  type SharedFile,
} from "./remotePush";
import { NARROW_NBSP } from "./typography";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

function pane(id: string, name: string, extra: Partial<Pane> & { role?: PaneRole } = {}): Pane {
  return { id, name, agent: "claude", sessionId: null, cwd: null, shellId: null, title: null, ...extra };
}

function workspace(id: string, name: string, cwd: string, panes: Pane[], hq = false): Workspace {
  return {
    id,
    name,
    cwd,
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

function state(): AppState {
  return {
    workspaces: [
      workspace("hq", "HQ", "C:\\Users\\me\\AppData\\Roaming\\hangar\\hq", [pane("m", "Max", { role: "manager" })], true),
      workspace("a", "Shop", "C:\\dev\\shop", [
        pane("o", "Jack", { role: "orchestrator" }),
        pane("w1", "Ada", { role: "worker", cwd: "C:\\dev\\shop-worktrees\\ada" }),
        pane("w2", "Ben", { role: "worker" }),
      ]),
      workspace("b", "Blog", "/home/me/blog", [pane("p", "Cora")]),
    ],
    folders: [],
    activeWorkspaceId: "a",
    settings: DEFAULT_SETTINGS,
  };
}

function fakeTime() {
  let t = 1_000_000;
  let next = 1;
  const timers = new Map<number, { at: number; run: () => void }>();
  const settle = async () => {
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
    pending: () => timers.size,
    async advance(ms: number) {
      const end = t + ms;
      for (;;) {
        const due = [...timers.entries()].filter(([, timer]) => timer.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        timers.delete(due[0]);
        t = Math.max(t, due[1].at);
        due[1].run();
        await settle();
      }
      t = end;
      await settle();
    },
  };
}

function rig(options: { subscribers?: number } = {}) {
  const time = fakeTime();
  const world = state();
  const waiting = new Set<string>();
  let subscribers = options.subscribers ?? 1;
  const sent: { at: number; message: PushMessage }[] = [];
  const push = createRemotePush({
    snapshot: () => world,
    send: async (message) => {
      sent.push({ at: time.now(), message });
    },
    subscribers: async () => subscribers,
    isWaiting: (paneId) => waiting.has(paneId),
    now: time.now,
    setTimer: time.setTimer,
    clearTimer: time.clearTimer,
  });
  return {
    push,
    time,
    world,
    waiting,
    sent,
    setSubscribers: (n: number) => {
      subscribers = n;
    },
  };
}

const waits = (paneId: string, name: string, workspaceId = "a", ws = "Shop"): ManagerEvent => ({
  kind: "paneWaiting",
  workspaceId,
  workspace: ws,
  paneId,
  pane: name,
  forMs: 120_000,
});
const question = (taskId: string, title: string, workspaceId = "a", ws = "Shop"): ManagerEvent => ({
  kind: "taskNeedsYou",
  workspaceId,
  workspace: ws,
  taskId,
  title,
  reason: "Jack: which db?",
});
const lands = (taskId: string, title: string): ManagerEvent => ({
  kind: "readyToLand",
  workspaceId: "a",
  workspace: "Shop",
  taskId,
  title,
});
const done = (tasks: number, workspaceId = "b", ws = "Blog"): ManagerEvent => ({
  kind: "boardDone",
  workspaceId,
  workspace: ws,
  tasks,
});

function share(id: string, patch: Partial<SharedFile> = {}): SharedFile {
  return {
    id,
    at: 1,
    workspace: "C:\\dev\\shop",
    pane: "Ada",
    name: `${id}.png`,
    mime: "image/png",
    size: 10,
    caption: null,
    ...patch,
  };
}

const fr = (text: string) => text.replace(/ ([:?!])/g, `${NARROW_NBSP}$1`);

describe("remote push, what triggers it", () => {
  it("a pane waiting opens that pane", async () => {
    const r = rig();
    await r.push.refresh();
    r.waiting.add("w1");
    r.push.sink.post(waits("w1", "Ada"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent).toHaveLength(1);
    expect(r.sent[0].message).toEqual({
      title: { en: "Ada waits for you", fr: "Ada vous attend" },
      body: { en: "Shop: a permission or a question", fr: fr("Shop : une permission ou une question") },
      url: "./#/pane/w1",
    });
  });

  it("needs-you and ready to land open For you", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.sink.post(question("t1", "Which database?"));
    r.push.sink.post(lands("t2", "Mobile settings"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent.map((s) => s.message)).toEqual([
      {
        title: { en: "A question for you", fr: "Une question pour vous" },
        // The task's own words are left as written.
        body: { en: "Shop: Which database?", fr: `Shop${NARROW_NBSP}: Which database?` },
        url: "./#/",
      },
      {
        title: { en: "Ready to merge", fr: "Prête à fusionner" },
        body: { en: "Shop: Mobile settings", fr: `Shop${NARROW_NBSP}: Mobile settings` },
        url: "./#/",
      },
    ]);
  });

  it("a board all done opens its project", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.sink.post(done(12));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent[0].message).toEqual({
      title: { en: "Every task is done", fr: "Toutes les tâches sont faites" },
      body: { en: "Blog: 12 tasks on the board", fr: fr("Blog : 12 tâches au tableau") },
      url: "./#/projects/b",
    });
  });

  it("a share opens the shares, with its caption or its name", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.shared(share("s1", { caption: "The For you screen" }));
    r.push.shared(share("s2", { workspace: "/home/me/blog", pane: "Cora", name: "notes.pdf", mime: "application/pdf" }));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent.map((s) => s.message)).toEqual([
      {
        title: { en: "Ada sent you an image", fr: "Ada vous a envoyé une image" },
        body: { en: "Shop: The For you screen", fr: `Shop${NARROW_NBSP}: The For you screen` },
        url: "./#/shares",
      },
      {
        title: { en: "Cora sent you a file", fr: "Cora vous a envoyé un fichier" },
        body: { en: "Blog: notes.pdf", fr: `Blog${NARROW_NBSP}: notes.pdf` },
        url: "./#/shares",
      },
    ]);
  });

  it("feeds on the manager feed, with no manager at all", async () => {
    const r = rig();
    await r.push.refresh();
    const activities = new Map<string, AgentActivity>();
    const boards = new Map<string, BoardTask[]>();
    const board = (labels: string[]): BoardTask[] => [
      {
        id: "t1",
        title: "Which database?",
        description: "",
        column: "todo",
        priority: 1,
        assignee: null,
        labels,
        comments: [],
        depends_on: [],
        created_at: 1,
        updated_at: 1,
        order: 1,
      },
    ];
    boards.set("C:\\dev\\shop", board([]));
    const enqueued: string[] = [];
    const feed = createManagerFeed({
      snapshot: () => ({ ...r.world, workspaces: r.world.workspaces.map((ws) => (ws.hq ? { ...ws, panes: [] } : ws)) }),
      enqueue: async (_pane, text) => {
        enqueued.push(text);
      },
      activity: (id) => {
        const activity = activities.get(id);
        return activity ? { workspaceId: "x", activity, bridged: true } : undefined;
      },
      loadBoard: async (cwd) => boards.get(cwd) ?? [],
      push: r.push.sink,
      now: r.time.now,
      setTimer: r.time.setTimer,
      clearTimer: r.time.clearTimer,
    });
    await feed.tick();
    boards.set("C:\\dev\\shop", board(["needs-you"]));
    await feed.tick();
    await r.time.advance(TIMING.maxWaitMs);
    expect(enqueued).toHaveLength(0);
    expect(r.sent).toHaveLength(1);
    expect(r.sent[0].message.title.en).toBe("A question for you");
    feed.stop();
  });
});

describe("remote push, its pace", () => {
  it("gathers what comes together, then waits out 30 s per kind and workspace", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.sink.post(question("t1", "One"));
    r.push.sink.post(question("t2", "Two"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent).toHaveLength(1);
    expect(r.sent[0].message.title.en).toBe("2 questions for you");
    expect(r.sent[0].message.body.en).toBe("Shop: One, Two");
    const first = r.sent[0].at;

    // Within the 30 s: held, then sent as one.
    await r.time.advance(5_000);
    r.push.sink.post(question("t3", "Three"));
    await r.time.advance(10_000);
    r.push.sink.post(question("t4", "Four"));
    r.push.sink.post(question("t5", "Five"));
    r.push.sink.post(question("t6", "Six"));
    await r.time.advance(PUSH_TIMING.gapMs);
    expect(r.sent).toHaveLength(2);
    expect(r.sent[1].at - first).toBe(PUSH_TIMING.gapMs);
    expect(r.sent[1].message.title.en).toBe("4 questions for you");
    expect(r.sent[1].message.title.fr).toBe("4 questions pour vous");
    expect(r.sent[1].message.body.en).toBe("Shop: Three, Four, Five…");

    // Long after: the next goes after the gathering only.
    await r.time.advance(60_000);
    r.push.sink.post(question("t7", "Seven"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent).toHaveLength(3);
  });

  it("never sends one kind in one workspace twice within 30 s", async () => {
    const r = rig();
    await r.push.refresh();
    for (let i = 0; i < 40; i++) {
      r.push.shared(share(`s${i}`));
      await r.time.advance(2_000);
    }
    await r.time.advance(PUSH_TIMING.gapMs);
    const gaps = r.sent.slice(1).map((s, i) => s.at - r.sent[i].at);
    expect(r.sent.length).toBeGreaterThan(1);
    for (const gap of gaps) expect(gap).toBeGreaterThanOrEqual(PUSH_TIMING.gapMs);
    expect(r.sent[1].message.title.en).toMatch(/^\d+ files for you$/);
  });

  it("does not hold one kind or workspace for another", async () => {
    const r = rig();
    await r.push.refresh();
    r.waiting.add("w1").add("p");
    r.push.sink.post(waits("w1", "Ada"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    r.push.sink.post(question("t1", "One"));
    r.push.sink.post(waits("p", "Cora", "b", "Blog"));
    r.push.shared(share("s1"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent.map((s) => s.message.title.en)).toEqual([
      "Ada waits for you",
      "A question for you",
      "Cora waits for you",
      "Ada sent you an image",
    ]);
  });

  it("several panes waiting open For you, and a wait that ended is dropped", async () => {
    const r = rig();
    await r.push.refresh();
    r.waiting.add("w1").add("w2").add("o");
    r.push.sink.post(waits("w1", "Ada"));
    r.push.sink.post(waits("w2", "Ben"));
    r.push.sink.post(waits("o", "Jack"));
    r.waiting.delete("o");
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent).toHaveLength(1);
    expect(r.sent[0].message).toMatchObject({
      title: { en: "2 panes wait for you", fr: "2 panneaux vous attendent" },
      body: { en: "Shop: Ada, Ben" },
      url: "./#/",
    });

    // Every wait of the next batch ended: nothing goes, and the 30 s are not spent.
    r.push.sink.post(waits("w1", "Ada"));
    r.waiting.clear();
    await r.time.advance(PUSH_TIMING.gapMs);
    expect(r.sent).toHaveLength(1);
  });
});

describe("remote push, who it is for", () => {
  it("pushes nothing without a subscribed phone", async () => {
    const r = rig({ subscribers: 0 });
    await r.push.refresh();
    expect(r.push.sink.active()).toBe(false);
    r.waiting.add("w1");
    r.push.sink.post(waits("w1", "Ada"));
    r.push.shared(share("s1"));
    await r.time.advance(PUSH_TIMING.gapMs);
    expect(r.sent).toHaveLength(0);

    r.setSubscribers(1);
    await r.push.refresh();
    expect(r.push.sink.active()).toBe(true);
    r.push.shared(share("s2"));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent).toHaveLength(1);
  });

  it("drops what waited to go when the last phone unsubscribes", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.shared(share("s1"));
    r.setSubscribers(0);
    await r.push.refresh();
    await r.time.advance(PUSH_TIMING.gapMs);
    expect(r.sent).toHaveLength(0);
    expect(r.time.pending()).toBe(0);
  });

  it("is off when the PC cannot say how many phones listen", async () => {
    const r = rig();
    const push = createRemotePush({
      snapshot: () => r.world,
      send: async () => {
        throw new Error("never");
      },
      subscribers: async () => {
        throw new Error("Hangar Mobile is unavailable");
      },
      isWaiting: () => true,
    });
    await push.refresh();
    expect(push.sink.active()).toBe(false);
  });

  it("pushes nothing from the HQ", async () => {
    const r = rig();
    await r.push.refresh();
    r.waiting.add("m");
    r.push.sink.post(waits("m", "Max", "hq", "HQ"));
    r.push.sink.post(question("h1", "Anything?", "hq", "HQ"));
    r.push.sink.post(done(3, "hq", "HQ"));
    r.push.shared(share("s1", { workspace: "C:\\Users\\me\\AppData\\Roaming\\hangar\\hq", pane: "Max" }));
    await r.time.advance(PUSH_TIMING.gapMs);
    expect(r.sent).toHaveLength(0);
  });

  it("stops for good", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.shared(share("s1"));
    r.push.stop();
    await r.push.refresh();
    r.push.shared(share("s2"));
    await r.time.advance(PUSH_TIMING.gapMs);
    expect(r.sent).toHaveLength(0);
    expect(r.push.sink.active()).toBe(false);
  });
});

describe("workspaceOfShare", () => {
  const world = state();

  it("finds a workspace by its folder, however it is spelled", () => {
    expect(workspaceOfShare(world, share("s", { workspace: "c:/dev/Shop/" }))?.id).toBe("a");
    expect(workspaceOfShare(world, share("s", { workspace: "\\\\?\\C:\\dev\\shop" }))?.id).toBe("a");
    expect(workspaceOfShare(world, share("s", { workspace: "/home/me/blog", pane: "?" }))?.id).toBe("b");
  });

  it("by a pane's folder, then by a pane's name", () => {
    expect(workspaceOfShare(world, share("s", { workspace: "C:\\dev\\shop-worktrees\\ada", pane: "?" }))?.id).toBe("a");
    expect(workspaceOfShare(world, share("s", { workspace: "D:\\elsewhere", pane: "Cora" }))?.id).toBe("b");
    expect(workspaceOfShare(world, share("s", { workspace: "D:\\elsewhere", pane: "Nobody" }))).toBeNull();
  });

  it("names a folder no workspace has after its last part", async () => {
    const r = rig();
    await r.push.refresh();
    r.push.shared(share("s", { workspace: "D:\\elsewhere\\tools\\", pane: "Nobody" }));
    await r.time.advance(PUSH_TIMING.gatherMs);
    expect(r.sent[0].message.body.en).toBe("tools: s.png");
  });
});

describe("settlePush and pushMessage", () => {
  it("folds repeats, the latest board count winning", () => {
    const out = settlePush([done(3), question("t1", "One"), question("t1", "One"), done(4)], () => true);
    expect(out.map((item) => item.kind)).toEqual(["taskNeedsYou", "boardDone"]);
    expect(out[1]).toMatchObject({ tasks: 4 });
  });

  it("says one task in the singular", () => {
    expect(pushMessage([done(1)])?.body).toEqual({
      en: "Blog: 1 task on the board",
      fr: fr("Blog : 1 tâche au tableau"),
    });
  });

  it("lists several shares by name", () => {
    const message = pushMessage(
      ["a", "b", "c", "d"].map((id) => ({ kind: "shared" as const, workspaceId: "a", workspace: "Shop", share: share(id) })),
    );
    expect(message).toEqual({
      title: { en: "4 files for you", fr: "4 fichiers pour vous" },
      body: { en: "Shop: a.png, b.png, c.png…", fr: `Shop${NARROW_NBSP}: a.png, b.png, c.png…` },
      url: "./#/shares",
    });
    expect(pushMessage([])).toBeNull();
  });
});
