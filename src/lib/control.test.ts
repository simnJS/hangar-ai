import { describe, expect, it } from "vitest";
import { DISABLED, callerWorkspace, runControl, type ControlDeps } from "./control";
import { localTerminal, readLines } from "./windows";
import { DEFAULT_SETTINGS, type AgentId, type AppState, type Pane, type Workspace } from "../types";

function pane(id: string, name: string, agent: AgentId, cwd: string | null = null): Pane {
  return { id, name, agent, sessionId: `s-${id}`, cwd, shellId: null, title: null };
}

function workspace(id: string, name: string, cwd: string, panes: Pane[]): Workspace {
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
  };
}

/** A fake terminal whose buffer holds `rows`, the wrapped ones flagged. */
function terminal(rows: [string, boolean?][], typed: string[]) {
  return {
    paste: (data: string) => typed.push(`paste:${data}`),
    modes: { applicationCursorKeysMode: false },
    buffer: {
      active: {
        length: rows.length,
        getLine: (row: number) =>
          rows[row] && {
            isWrapped: Boolean(rows[row][1]),
            translateToString: () => rows[row][0],
          },
      },
    },
  } as unknown as Parameters<typeof localTerminal>[0];
}

/** An app with two workspaces, the first one open, and a log of what was done. */
function setup(overrides: Partial<ControlDeps> = {}) {
  let state: AppState = {
    workspaces: [
      workspace("w1", "Hangar", "C:\\dev\\hangar", [
        pane("p1", "Ava", "claude"),
        pane("p2", "Max", "shell"),
      ]),
      workspace("w2", "Shop", "C:\\dev\\shop", [pane("p3", "Ava", "codex")]),
    ],
    folders: [],
    activeWorkspaceId: "w1",
    settings: DEFAULT_SETTINGS,
  };
  const log: string[] = [];
  const typed: string[] = [];
  const opened = new Set(["w1"]);
  let next = 10;

  const deps: ControlDeps = {
    snapshot: () => state,
    enabled: () => true,
    addPane: (workspaceId, opts) => {
      const id = `p${next++}`;
      state = {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === workspaceId
            ? { ...ws, panes: [...ws.panes, pane(id, opts.name ?? "Leo", opts.agent ?? "shell", opts.cwd ?? null)] }
            : ws,
        ),
      };
      log.push(`add:${workspaceId}:${JSON.stringify(opts)}`);
      return id;
    },
    closePane: (workspaceId, paneId) => log.push(`close:${workspaceId}:${paneId}`),
    respawnPane: (workspaceId, paneId, patch) => {
      log.push(`respawn:${workspaceId}:${paneId}:${JSON.stringify(patch)}`);
      return "fresh-id";
    },
    openWorkspace: (id) => {
      opened.add(id);
      log.push(`open:${id}`);
    },
    isOpen: (id) => opened.has(id),
    activate: (workspaceId, paneId) => log.push(`activate:${workspaceId}:${paneId}`),
    terminal: async (paneId) =>
      paneId === "p1" || paneId === "p2" ? localTerminal(terminal([["$ ls"], ["a.txt"]], typed)) : null,
    write: async (paneId, data) => {
      typed.push(`write:${paneId}:${JSON.stringify(data)}`);
    },
    alive: async () => true,
    activity: (paneId) =>
      paneId === "p1" ? { workspaceId: "w1", activity: "working", bridged: true } : undefined,
    enqueue: async (paneId, text) => log.push(`enqueue:${paneId}:${text}`),
    dirExists: async (path) => !path.includes("missing"),
    canDetach: () => true,
    detach: async (id) => {
      log.push(`detach:${id}`);
    },
    reattach: (id) => log.push(`reattach:${id}`),
    announce: (event) => log.push(`announce:${event.kind}:${event.pane}:${event.by}`),
    sleep: async () => undefined,
    ...overrides,
  };
  return { deps, log, typed };
}

const fromAva = { caller: { paneId: "p1", paneName: "Ava", cwd: "C:\\dev\\hangar" } };

describe("which workspace a caller is in", () => {
  const { deps } = setup();
  const state = deps.snapshot();

  it("follows the caller's pane first", () => {
    expect(callerWorkspace(state, { paneId: "p3", paneName: null, cwd: "C:/dev/hangar" })?.id).toBe("w2");
  });

  it("falls back on the folder, in any spelling, and on a folder inside it", () => {
    expect(callerWorkspace(state, { paneId: null, paneName: null, cwd: "c:/DEV/shop/" })?.id).toBe("w2");
    expect(callerWorkspace(state, { paneId: null, paneName: null, cwd: "C:\\dev\\hangar\\src" })?.id).toBe("w1");
    expect(callerWorkspace(state, { paneId: null, paneName: null, cwd: "D:\\elsewhere" })).toBeNull();
  });
});

describe("runControl", () => {
  it("refuses everything when the setting is off", async () => {
    const { deps } = setup({ enabled: () => false });
    await expect(runControl("pane_list", fromAva, deps)).rejects.toThrow(DISABLED);
  });

  it("lists the caller's panes, marking the caller and its activity", async () => {
    const { deps } = setup();
    const result = (await runControl("pane_list", fromAva, deps)) as any;
    expect(result.workspace.name).toBe("Hangar");
    expect(result.panes.map((p: any) => [p.name, p.you, p.activity, p.running])).toEqual([
      ["Ava", true, "working", true],
      ["Max", false, null, true],
    ]);
  });

  it("names a workspace that is not open and says nothing runs there", async () => {
    const { deps } = setup();
    const result = (await runControl("pane_list", { ...fromAva, workspace: "shop" }, deps)) as any;
    expect(result.workspace.open).toBe(false);
    expect(result.note).toMatch(/not open/);
    expect(result.panes[0].running).toBe(false);
  });

  it("creates a Claude pane with a queued prompt, opening its workspace", async () => {
    const { deps, log } = setup();
    const result = (await runControl(
      "pane_create",
      { ...fromAva, workspace: "Shop", agent: "claude", name: "Reviewer", cwd: "packages/api", prompt: "Review the API" },
      deps,
    )) as any;
    expect(result.pane.name).toBe("Reviewer");
    expect(result.pane.cwd).toBe("C:\\dev\\shop\\packages\\api");
    expect(log).toContain("open:w2");
    expect(log).toContain("enqueue:p10:Review the API");
    expect(log).toContain("announce:created:Reviewer:Ava");
  });

  it("refuses a prompt for an agent the queue cannot reach, a taken name and a missing folder", async () => {
    const { deps, log } = setup();
    await expect(
      runControl("pane_create", { ...fromAva, agent: "codex", prompt: "hi" }, deps),
    ).rejects.toThrow(/Claude Code/);
    await expect(runControl("pane_create", { ...fromAva, name: "max" }, deps)).rejects.toThrow(/already has/);
    await expect(runControl("pane_create", { ...fromAva, cwd: "missing" }, deps)).rejects.toThrow(/No such folder/);
    await expect(runControl("pane_create", { ...fromAva, agent: "vim" }, deps)).rejects.toThrow(/Unknown agent/);
    expect(log.some((entry) => entry.startsWith("add:"))).toBe(false);
  });

  it("closes a pane by name but never the last one of a workspace", async () => {
    const { deps, log } = setup();
    await runControl("pane_close", { ...fromAva, pane: "max" }, deps);
    expect(log).toContain("close:w1:p2");
    await expect(
      runControl("pane_close", { ...fromAva, workspace: "Shop", pane: "Ava" }, deps),
    ).rejects.toThrow(/last pane/);
    await expect(runControl("pane_close", { ...fromAva, pane: "Zed" }, deps)).rejects.toThrow(
      /No pane "Zed".*"Ava", "Max"/,
    );
  });

  it("restarts, resets and switches agents", async () => {
    const { deps, log } = setup();
    await runControl("pane_restart", { ...fromAva, pane: "Ava" }, deps);
    await runControl("pane_restart", { ...fromAva, pane: "Ava", fresh: true }, deps);
    await runControl("pane_restart", { ...fromAva, pane: "Max", agent: "claude" }, deps);
    expect(log.filter((entry) => entry.startsWith("respawn:"))).toEqual([
      "respawn:w1:p1:{}",
      'respawn:w1:p1:{"sessionId":null}',
      'respawn:w1:p2:{"agent":"claude","sessionId":null}',
    ]);
    expect(log).toContain("announce:reset:Ava:Ava");
  });

  it("types text, then Enter, then the keys in order", async () => {
    const { deps, typed } = setup();
    await runControl("pane_send", { ...fromAva, pane: "Max", text: "npm test", keys: ["up", "ctrl+c"] }, deps);
    expect(typed).toEqual([
      "paste:npm test",
      'write:p2:"\\r"',
      'write:p2:"\\u001b[A"',
      'write:p2:"\\u0003"',
    ]);
  });

  it("sends nothing at all when a key is unknown", async () => {
    const { deps, typed } = setup();
    await expect(
      runControl("pane_send", { ...fromAva, pane: "Max", text: "x", keys: ["f13"] }, deps),
    ).rejects.toThrow(/Unknown key/);
    expect(typed).toEqual([]);
  });

  it("queues for an idle Claude Code session only", async () => {
    const { deps, log } = setup();
    await runControl("pane_send", { ...fromAva, pane: "Ava", text: "next task", when_idle: true }, deps);
    expect(log).toContain("enqueue:p1:next task");
    await expect(
      runControl("pane_send", { ...fromAva, pane: "Max", text: "x", when_idle: true }, deps),
    ).rejects.toThrow(/Claude Code/);
  });

  it("opens a closed workspace when sent to, and asks to retry", async () => {
    const { deps, log } = setup();
    await expect(
      runControl("pane_send", { ...fromAva, workspace: "Shop", pane: "Ava", text: "hi" }, deps),
    ).rejects.toThrow(/starting now/);
    expect(log).toContain("open:w2");
  });

  it("reads the bottom of a terminal", async () => {
    const { deps } = setup();
    const result = (await runControl("pane_read", { ...fromAva, pane: "Max", lines: 1 }, deps)) as any;
    expect(result.text).toBe("a.txt");
  });

  it("moves a workspace to its own window and back, once", async () => {
    const { deps, log } = setup();
    const out = (await runControl("workspace_window", { ...fromAva, workspace: "Shop" }, deps)) as any;
    expect(out).toEqual({ workspace: "Shop", detached: true, changed: true });
    await runControl("workspace_window", { ...fromAva, detach: false }, deps);
    expect(log.filter((e) => /detach|reattach/.test(e))).toEqual(["detach:w2"]);
  });
});

describe("readLines", () => {
  it("joins wrapped rows and drops the blank bottom", () => {
    const typed: string[] = [];
    const buffer = terminal(
      [["first"], ["a very long li"], ["ne that wrapped", true], [""], ["  "]],
      typed,
    ).buffer.active;
    expect(readLines(buffer, 10)).toEqual(["first", "a very long line that wrapped"]);
    expect(readLines(buffer, 1)).toEqual(["a very long line that wrapped"]);
  });
});
