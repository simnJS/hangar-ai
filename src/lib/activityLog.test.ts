import { describe, expect, it } from "vitest";
import { createActivityLog, lasted, textOf, type ActivityDeps, type ActivityEntry } from "./activityLog";
import type { ManagerJournalEntry } from "./managerEvents";
import type { JournalEntry, OrchestratorEvent } from "./orchestratorEvents";
import type { SharedFile } from "./remotePush";
import { DEFAULT_SETTINGS, type AppState, type Pane, type Workspace } from "../types";

function pane(id: string, name: string): Pane {
  return { id, name, agent: "claude", sessionId: null, cwd: null, shellId: null, title: null };
}

function workspace(id: string, name: string, extra: Partial<Workspace> = {}): Workspace {
  return {
    id,
    name,
    cwd: `C:\\dev\\${id}`,
    extraRoots: [],
    panes: [pane(`${id}-p`, "Ava")],
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
    ...extra,
  };
}

interface Rig {
  deps: ActivityDeps;
  written: ActivityEntry[][];
  manager: ManagerJournalEntry[];
  orchestrators: Record<string, JournalEntry[]>;
  asleep: Set<string>;
  waiting: Set<string>;
  clock: { at: number };
  state: AppState;
}

function rig(workspaces: Workspace[], locale: "en" | "fr" = "en"): Rig {
  const state = {
    ...DEFAULT_SETTINGS,
    workspaces: [workspace("hq", "HQ", { hq: true }), ...workspaces],
    settings: { ...DEFAULT_SETTINGS, locale },
  } as unknown as AppState;
  const r: Rig = {
    written: [],
    manager: [],
    orchestrators: {},
    asleep: new Set(),
    waiting: new Set(),
    clock: { at: 10_000 },
    state,
    deps: null as unknown as ActivityDeps,
  };
  r.deps = {
    snapshot: () => state,
    append: async (entries) => void r.written.push(entries),
    managerJournal: () => r.manager,
    orchestratorJournal: (id) => r.orchestrators[id] ?? [],
    isAsleep: (id) => r.asleep.has(id),
    isWaiting: (id) => r.waiting.has(id),
    now: () => r.clock.at,
  };
  return r;
}

const delivery = (outcome: "landed" | "done" | "conflict", taskId = "t1", title = "Cart"): OrchestratorEvent => ({
  kind: "delivery",
  taskId,
  title,
  worker: "Ben",
  outcome,
  detail: "",
});
const journalEntry = (at: number, event: OrchestratorEvent): JournalEntry => ({ at, event, quiet: false });
const flat = (r: Rig) => r.written.flat();

describe("what is written", () => {
  it("a delivery landed or done, and a task moved to done once, not twice", async () => {
    const r = rig([workspace("shop", "Shop")]);
    r.orchestrators.shop = [
      journalEntry(100, delivery("landed")),
      journalEntry(101, { kind: "taskMoved", taskId: "t1", title: "Cart", from: "review", to: "done", by: null }),
      journalEntry(200, delivery("done", "t2", "Search")),
      journalEntry(300, delivery("conflict", "t3", "Pay")),
      journalEntry(400, { kind: "taskMoved", taskId: "t4", title: "Docs", from: "doing", to: "done", by: "Ava" }),
      journalEntry(500, { kind: "taskMoved", taskId: "t5", title: "Wip", from: "todo", to: "doing", by: null }),
    ];
    await createActivityLog(r.deps).tick();
    expect(flat(r)).toEqual([
      { at: 100, workspace: "Shop", kind: "landed", text: "Landed: Cart" },
      { at: 200, workspace: "Shop", kind: "delivered", text: "Delivered: Search" },
      { at: 400, workspace: "Shop", kind: "taskDone", text: "Done: Docs" },
    ]);
  });

  it("what the project manager is told: a task that needs the person, a delivery ready, a board done", async () => {
    const r = rig([workspace("shop", "Shop")]);
    r.manager = [
      { at: 100, event: { kind: "taskNeedsYou", workspaceId: "shop", workspace: "Shop", taskId: "t1", title: "Pay", reason: "?" } },
      { at: 200, event: { kind: "readyToLand", workspaceId: "shop", workspace: "Shop", taskId: "t2", title: "Cart" } },
      { at: 300, event: { kind: "boardDone", workspaceId: "shop", workspace: "Shop", tasks: 7 } },
    ];
    await createActivityLog(r.deps).tick();
    expect(flat(r).map((e) => [e.kind, e.text])).toEqual([
      ["needsYou", "Needs you: Pay"],
      ["readyToLand", "Ready to merge: Cart"],
      ["boardDone", "Every task is done (7)"],
    ]);
  });

  it("a task that needs the person is one line, whoever saw it", async () => {
    const r = rig([workspace("shop", "Shop")]);
    r.manager = [
      { at: 100, event: { kind: "taskNeedsYou", workspaceId: "shop", workspace: "Shop", taskId: "t1", title: "Pay", reason: "?" } },
    ];
    r.orchestrators.shop = [journalEntry(150, { kind: "taskNeedsYou", taskId: "t1", title: "Pay", reason: "?" })];
    await createActivityLog(r.deps).tick();
    expect(flat(r).filter((e) => e.kind === "needsYou")).toHaveLength(1);
  });

  it("a pane that waited, when it starts and when it ends, with how long", async () => {
    const r = rig([workspace("shop", "Shop")]);
    r.waiting.add("shop-p");
    r.manager = [
      { at: 10_000, event: { kind: "paneWaiting", workspaceId: "shop", workspace: "Shop", paneId: "shop-p", pane: "Ava", forMs: 0 } },
    ];
    const log = createActivityLog(r.deps);
    await log.tick();
    r.clock.at = 10_000 + 12 * 60_000;
    await log.tick();
    expect(flat(r).map((e) => e.kind)).toEqual(["paneWaiting"]);
    r.waiting.clear();
    await log.tick();
    expect(flat(r)[flat(r).length - 1]).toEqual({
      at: 10_000 + 12 * 60_000,
      workspace: "Shop",
      kind: "paneWaited",
      text: "Ava waited for you for 12 min",
    });
    await log.tick();
    expect(flat(r)).toHaveLength(2);
  });

  it("a workspace falling asleep or waking; what is asleep at the start is not news", async () => {
    const r = rig([workspace("shop", "Shop"), workspace("blog", "Blog")]);
    r.asleep.add("blog");
    const log = createActivityLog(r.deps);
    await log.tick();
    expect(flat(r)).toEqual([]);
    r.asleep.add("shop");
    r.asleep.delete("blog");
    r.clock.at = 20_000;
    await log.tick();
    expect(flat(r).map((e) => [e.workspace, e.kind, e.text])).toEqual([
      ["Shop", "asleep", "Shop fell asleep"],
      ["Blog", "awake", "Blog woke up"],
    ]);
  });

  it("a file shared with the phone, from the workspace it came from", async () => {
    const r = rig([workspace("shop", "Shop")]);
    const log = createActivityLog(r.deps);
    const share: SharedFile = {
      id: "s1",
      at: 777,
      workspace: "C:\\dev\\shop",
      pane: "Ava",
      name: "report.pdf",
      mime: "application/pdf",
      size: 10,
      caption: null,
    };
    log.shared(share);
    await log.flush();
    expect(flat(r)).toEqual([{ at: 777, workspace: "Shop", kind: "shared", text: "Ava shared report.pdf" }]);
  });

  it("nothing from the HQ", async () => {
    const r = rig([]);
    r.manager = [{ at: 1, event: { kind: "boardDone", workspaceId: "hq", workspace: "HQ", tasks: 1 } }];
    r.orchestrators.hq = [journalEntry(2, delivery("landed"))];
    const log = createActivityLog(r.deps);
    log.shared({ id: "s", at: 3, workspace: "C:\\dev\\hq", pane: "Pm", name: "x", mime: "text/plain", size: 1, caption: null });
    await log.tick();
    expect(flat(r)).toEqual([]);
  });

  it("writes in the language of the PC", async () => {
    const r = rig([workspace("shop", "Shop")], "fr");
    r.orchestrators.shop = [journalEntry(100, delivery("landed"))];
    await createActivityLog(r.deps).tick();
    expect(flat(r)[0].text).toBe("Intégrée : Cart");
  });
});

describe("the journals are read again and again", () => {
  it("a line is written once, however many times the journal is read", async () => {
    const r = rig([workspace("shop", "Shop")]);
    r.orchestrators.shop = [journalEntry(100, delivery("landed"))];
    const log = createActivityLog(r.deps);
    await log.tick();
    await log.tick();
    await log.tick();
    expect(flat(r)).toHaveLength(1);
    r.orchestrators.shop.push(journalEntry(200, delivery("done", "t2", "Search")));
    await log.tick();
    expect(flat(r).map((e) => e.at)).toEqual([100, 200]);
    // Gone from the journal and back is not new either.
    r.orchestrators.shop = [r.orchestrators.shop[1]];
    await log.tick();
    expect(flat(r)).toHaveLength(2);
  });

  it("a write the disk refused is tried again with the next, in order", async () => {
    const r = rig([workspace("shop", "Shop")]);
    let failing = true;
    r.deps.append = async (entries) => {
      if (failing) throw new Error("disk");
      r.written.push(entries);
    };
    r.orchestrators.shop = [journalEntry(100, delivery("landed"))];
    const log = createActivityLog(r.deps);
    await log.tick();
    expect(flat(r)).toEqual([]);
    failing = false;
    r.orchestrators.shop.push(journalEntry(200, delivery("done", "t2", "Search")));
    await log.tick();
    expect(flat(r).map((e) => e.at)).toEqual([100, 200]);
  });

  it("nothing is written once it is stopped", async () => {
    const r = rig([workspace("shop", "Shop")]);
    r.orchestrators.shop = [journalEntry(100, delivery("landed"))];
    const log = createActivityLog(r.deps);
    log.stop();
    await log.tick();
    expect(flat(r)).toEqual([]);
  });
});

describe("words", () => {
  it("says how long, the same in both languages", () => {
    expect(lasted(0)).toBe("0 s");
    expect(lasted(45_000)).toBe("45 s");
    expect(lasted(12 * 60_000)).toBe("12 min");
    expect(lasted(3 * 3_600_000 + 5 * 60_000)).toBe("3 h 05");
  });

  it("words a line the PC wrote without text", () => {
    const say = (key: string) => key;
    expect(textOf({ at: 1, workspace: "", kind: "appStopped", text: "" }, say)).toBe("activity.appStopped");
    expect(textOf({ at: 1, workspace: "", kind: "landed", text: "Landed: x" }, say)).toBe("Landed: x");
    expect(textOf({ at: 1, workspace: "", kind: "mystery", text: "" }, say)).toBe("mystery");
  });
});
