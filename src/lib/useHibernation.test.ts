import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, type AgentId, type AppState, type Pane, type Workspace } from "../types";

/** What the system was asked: the probes of the shells, and the kills. */
const system = vi.hoisted(() => ({
  probe: vi.fn(async (_ids: string[]): Promise<string[]> => []),
  kills: [] as { id: string; tree: boolean }[],
}));

vi.mock("./ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./ipc")>()),
  ptyHasChildren: (ids: string[]) => system.probe(ids),
  ptyKill: async (id: string, tree = false) => {
    system.kills.push({ id, tree });
  },
}));

const { forceSleep, sweep } = await import("./useHibernation");
const store = await import("./hibernateStore");

const MIN = 60_000;

/** Every test gets ids of its own: the stores below live as long as the module. */
let run = 0;
const ids = () => {
  run += 1;
  return (name: string) => `${name}-${run}`;
};

function pane(id: string, agent: AgentId, sessionId: string | null = `s-${id}`): Pane {
  return { id, name: id, agent, sessionId, cwd: null, shellId: null, title: null };
}

function workspace(id: string, panes: Pane[], patch: Partial<Workspace> = {}): Workspace {
  return {
    id,
    name: id,
    cwd: `/work/${id}`,
    extraRoots: [],
    panes,
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
    ...patch,
  };
}

/** A window that mounted every workspace given, with each pane seen just now. */
function mainWindow(workspaces: Workspace[], activeWorkspaceId: string, recent: string[] = []) {
  let state: AppState = { workspaces, folders: [], activeWorkspaceId, settings: DEFAULT_SETTINGS };
  let openedIds = workspaces.map((ws) => ws.id);
  const calls: string[] = [];
  for (const ws of workspaces) for (const p of ws.panes) store.notePaneMounted(p.id);
  const context = () => ({
    snapshot: () => state,
    openedIds,
    recent,
    delayMs: 10 * MIN,
    unmount: (id: string) => {
      calls.push(`unmount ${id}`);
      openedIds = openedIds.filter((entry) => entry !== id);
    },
    show: (id: string) => {
      calls.push(`show ${id}`);
      state = { ...state, activeWorkspaceId: id };
    },
  });
  return { context, calls, state: () => state };
}

/** What a sleep leaves behind, whoever decided it. */
function slept(ws: Workspace) {
  return {
    asleep: store.isAsleep(ws.id),
    killed: system.kills.filter((kill) => ws.panes.some((p) => p.id === kill.id)),
    owed: ws.panes.filter((p) => store.mustResume(p.id)).map((p) => p.id),
  };
}

beforeEach(() => {
  system.probe.mockReset();
  system.probe.mockImplementation(async () => []);
  system.kills.length = 0;
});

afterEach(() => {
  vi.useRealTimers();
});

describe("one way to sleep, two ways to decide", () => {
  it("puts a workspace to sleep by hand exactly as the timer does once it is due", async () => {
    vi.useFakeTimers({ now: 1_000 * MIN });
    const id = ids();
    const screen = workspace(id("screen"), [pane(id("gus"), "claude")]);
    const byHand = workspace(id("hand"), [pane(id("ava"), "claude"), pane(id("sam"), "shell", null)]);
    const byTimer = workspace(id("timer"), [pane(id("ben"), "claude"), pane(id("tom"), "shell", null)]);
    const app = mainWindow([screen, byHand, byTimer], screen.id);

    // Just mounted: the timer has nothing to do yet, a click does.
    expect(await sweep(app.context)).toEqual([]);
    const outcome = await forceSleep(byHand.id, app.context);
    expect(outcome).toEqual({ kind: "slept", workspace: byHand, agents: 1, shown: null });

    vi.setSystemTime(1_011 * MIN);
    const swept = await sweep(app.context);
    expect(swept).toEqual([{ workspace: byTimer, agents: 1 }]);

    // The same marks on both: every tree killed, the agent owed its
    // session, the workspace asleep and unmounted.
    expect(slept(byHand)).toEqual({
      asleep: true,
      killed: [
        { id: id("ava"), tree: true },
        { id: id("sam"), tree: true },
      ],
      owed: [id("ava")],
    });
    expect(slept(byTimer)).toEqual({
      asleep: true,
      killed: [
        { id: id("ben"), tree: true },
        { id: id("tom"), tree: true },
      ],
      owed: [id("ben")],
    });
    expect(app.calls).toEqual([`unmount ${byHand.id}`, `unmount ${byTimer.id}`]);
    // Both asked the system about the shell before deciding.
    expect(system.probe.mock.calls).toEqual([[[id("sam")]], [[id("tom")]]]);
  });

  it("never lets the timer put to sleep again a workspace a click just did", async () => {
    vi.useFakeTimers({ now: 2_000 * MIN });
    const id = ids();
    const screen = workspace(id("screen"), [pane(id("gus"), "claude")]);
    const other = workspace(id("other"), [pane(id("ava"), "claude")]);
    const app = mainWindow([screen, other], screen.id);
    await forceSleep(other.id, app.context);

    // The window has not drawn the unmount yet: it is still in openedIds.
    const stale = () => ({ ...app.context(), openedIds: [screen.id, other.id] });
    vi.setSystemTime(2_011 * MIN);
    expect(await sweep(stale)).toEqual([]);
    expect(system.kills).toHaveLength(1);
  });
});

describe("forceSleep", () => {
  it("moves the main window to the workspace used last, then puts this one to sleep", async () => {
    const id = ids();
    const a = workspace(id("a"), [pane(id("ava"), "claude")]);
    const b = workspace(id("b"), [pane(id("ben"), "claude")]);
    const c = workspace(id("c"), [pane(id("cleo"), "claude")]);
    const app = mainWindow([a, b, c], a.id, [a.id, c.id, b.id]);

    const outcome = await forceSleep(a.id, app.context);
    expect(outcome).toMatchObject({ kind: "slept", shown: c.id });
    expect(app.calls).toEqual([`show ${c.id}`, `unmount ${a.id}`]);
    expect(app.state().activeWorkspaceId).toBe(c.id);
    expect(store.isAsleep(a.id)).toBe(true);
  });

  it("keeps the only workspace on screen, and awake", async () => {
    const id = ids();
    const only = workspace(id("only"), [pane(id("ava"), "claude")]);
    const app = mainWindow([only], only.id);
    expect(await forceSleep(only.id, app.context)).toEqual({
      kind: "refused",
      workspace: only,
      verdict: { kind: "awake", reason: "alone" },
    });
    expect(app.calls).toEqual([]);
    expect(system.kills).toEqual([]);
  });

  it("refuses at the click when a shell turns out to run a process, and moves nothing", async () => {
    const id = ids();
    const a = workspace(id("a"), [pane(id("ava"), "claude"), pane(id("leo"), "shell", null)]);
    const b = workspace(id("b"), [pane(id("ben"), "claude")]);
    const app = mainWindow([a, b], a.id, [a.id, b.id]);
    system.probe.mockImplementation(async () => [id("leo")]);

    expect(await forceSleep(a.id, app.context)).toEqual({
      kind: "refused",
      workspace: a,
      verdict: { kind: "awake", reason: "shellChildren", paneId: id("leo") },
    });
    // Still on screen, still running.
    expect(app.calls).toEqual([]);
    expect(app.state().activeWorkspaceId).toBe(a.id);
    expect(system.kills).toEqual([]);
    expect(store.isAsleep(a.id)).toBe(false);
  });

  it("reads everything again once the shells have answered", async () => {
    const id = ids();
    const screen = workspace(id("screen"), [pane(id("gus"), "claude")]);
    const ws = workspace(id("ws"), [pane(id("ava"), "claude"), pane(id("leo"), "shell", null)]);
    const app = mainWindow([screen, ws], screen.id);
    // Ava starts a turn while the system is being asked about Leo's shell.
    system.probe.mockImplementation(async () => {
      store.notePaneActivity(id("ava"), "working");
      return [];
    });

    const outcome = await forceSleep(ws.id, app.context);
    expect(outcome).toMatchObject({
      kind: "refused",
      verdict: { reason: "busy", paneId: id("ava") },
    });
    expect(system.kills).toEqual([]);
  });

  it("keeps a workspace awake when the system cannot say what its shells run", async () => {
    const id = ids();
    const screen = workspace(id("screen"), [pane(id("gus"), "claude")]);
    const ws = workspace(id("ws"), [pane(id("ava"), "claude"), pane(id("leo"), "shell", null)]);
    const app = mainWindow([screen, ws], screen.id);
    system.probe.mockImplementation(async () => {
      throw new Error("no process table");
    });
    expect(await forceSleep(ws.id, app.context)).toMatchObject({
      kind: "refused",
      verdict: { reason: "unknownProcesses" },
    });
  });

  it("says an agent at work keeps it awake, and asks the system nothing", async () => {
    const id = ids();
    const screen = workspace(id("screen"), [pane(id("gus"), "claude")]);
    const ws = workspace(id("ws"), [pane(id("ava"), "claude"), pane(id("leo"), "shell", null)]);
    const app = mainWindow([screen, ws], screen.id);
    store.notePaneActivity(id("ava"), "waiting");

    expect(await forceSleep(ws.id, app.context)).toMatchObject({
      kind: "refused",
      verdict: { reason: "busy", paneId: id("ava") },
    });
    expect(system.probe).not.toHaveBeenCalled();
  });

  it("answers asleep the second time, having done nothing more", async () => {
    const id = ids();
    const screen = workspace(id("screen"), [pane(id("gus"), "claude")]);
    const ws = workspace(id("ws"), [pane(id("ava"), "claude")]);
    const app = mainWindow([screen, ws], screen.id);
    await forceSleep(ws.id, app.context);
    expect(await forceSleep(ws.id, app.context)).toMatchObject({
      kind: "refused",
      verdict: { reason: "asleep" },
    });
    expect(system.kills).toHaveLength(1);
  });

  it("is nothing for a workspace deleted meanwhile", async () => {
    const id = ids();
    const app = mainWindow([workspace(id("screen"), [pane(id("gus"), "claude")])], id("screen"));
    expect(await forceSleep(id("gone"), app.context)).toBeNull();
  });
});
