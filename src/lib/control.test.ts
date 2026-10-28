import { describe, expect, it } from "vitest";
import {
  DISABLED,
  callerWorkspace,
  modelRoots,
  resolvePath,
  runControl,
  type ControlDeps,
} from "./control";
import type { Worktree } from "./git";
import { createRootGrants, type ModelHost } from "./modelHost";
import type { ModelInfo, OpenModel } from "./modelStore";
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

const INFO: ModelInfo = {
  triangles: 1284,
  vertices: 702,
  meshes: 3,
  materials: 2,
  animations: ["Idle"],
  size: [2, 1, 3],
  center: [0, 0.5, 0],
};

function openModel(id: string, path: string, patch: Partial<OpenModel> = {}): OpenModel {
  return {
    id,
    path,
    title: path.split(/[\\/]/).pop()!,
    format: "glb",
    workspaceId: "w1",
    openedBy: "Ava",
    openedAt: 0,
    modifiedMs: 100,
    revision: 0,
    status: "ready",
    info: INFO,
    error: null,
    ...patch,
  };
}

/**
 * A window's previews, in memory, reached through the workspace the host was
 * asked for: it lists that workspace's models only. A load ends the way
 * `settle` says: loaded, failed, or still going when the caller stops waiting.
 */
function fakePreview(log: string[]) {
  const preview = {
    models: [] as OpenModel[],
    activeId: null as string | null,
    /** The workspace the last host was asked for. */
    workspace: "w1",
    /** Its workspace on screen, and its panel with it. */
    onScreen: true,
    shown: true,
    settle: "ready" as "ready" | "error" | "timeout",
    snapshotError: null as string | null,
    host: null as unknown as ModelHost,
  };
  let next = 1;
  preview.host = {
    list: async () => ({
      models: preview.models.filter((model) => model.workspaceId === preview.workspace),
      activeId: preview.activeId,
      onScreen: preview.onScreen,
      shown: preview.shown,
    }),
    show: async (model) => {
      log.push(`show:${model.path}:${model.title}:${model.openedBy}:${preview.workspace}`);
      const existing = preview.models.find(
        (entry) => entry.path === model.path && entry.workspaceId === preview.workspace,
      );
      const id = existing?.id ?? `m${next++}`;
      if (!existing) {
        preview.models.push(
          openModel(id, model.path, {
            workspaceId: preview.workspace,
            openedBy: model.openedBy,
            status: "loading",
            info: null,
          }),
        );
      }
      preview.activeId = id;
      return id;
    },
    reveal: async (id) => {
      log.push(`reveal:${id}`);
      preview.activeId = id;
    },
    reload: async (id, modifiedMs) => {
      log.push(`reload:${id}:${modifiedMs}`);
    },
    whenSettled: async (id, ms) => {
      log.push(`settle:${id}:${ms}`);
      const model = preview.models.find((entry) => entry.id === id)!;
      if (preview.settle === "timeout") return null;
      return preview.settle === "error"
        ? { ...model, status: "error", error: "not a glTF file" }
        : { ...model, status: "ready", info: INFO };
    },
    snapshot: async (id, options) => {
      log.push(`snapshot:${id}:${options.angle}:${options.width}x${options.height}`);
      if (preview.snapshotError) throw new Error(preview.snapshotError);
      return "iVBORw0KGgo=";
    },
    close: async (ids) => {
      log.push(`closeModels:${ids.join(",")}`);
      preview.models = preview.models.filter((model) => !ids.includes(model.id));
    },
  };
  return preview;
}

/** What `git worktree list` would say for the fake repository. */
function worktree(path: string, patch: Partial<Worktree> = {}): Worktree {
  return {
    path,
    head: "abc1234",
    branch: "main",
    isMain: false,
    isCurrent: false,
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
    ...patch,
  };
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
  /** Who each queued prompt is from, as the session will read it. */
  const senders: string[] = [];
  const opened = new Set(["w1"]);
  /** Put to sleep for being left alone. */
  const sleeping = new Set<string>();
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
    asleep: (id) => sleeping.has(id),
    touch: (id) => log.push(`touch:${id}`),
    activate: (workspaceId, paneId) => log.push(`activate:${workspaceId}:${paneId}`),
    terminal: async (paneId) =>
      paneId === "p1" || paneId === "p2" ? localTerminal(terminal([["$ ls"], ["a.txt"]], typed)) : null,
    write: async (paneId, data) => {
      typed.push(`write:${paneId}:${JSON.stringify(data)}`);
    },
    alive: async () => true,
    activity: (paneId) =>
      paneId === "p1" ? { workspaceId: "w1", activity: "working", bridged: true } : undefined,
    enqueue: async (paneId, text, from) => {
      senders.push(`${text} <- ${from ?? "the person"}`);
      log.push(`enqueue:${paneId}:${text}`);
    },
    dirExists: async (path) => !path.includes("missing"),
    canDetach: () => true,
    detach: async (id) => {
      log.push(`detach:${id}`);
    },
    reattach: (id) => log.push(`reattach:${id}`),
    announce: (event) =>
      log.push(`announce:${event.kind}:${"pane" in event ? event.pane : event.file}:${event.by}`),
    sleep: async () => undefined,
    modelAllowRoots: async (workspaceId, roots) => {
      allowed = roots;
      log.push(`allow:${workspaceId}:${roots.join("|")}`);
    },
    // Rust's rules, roughly: inside an allowed folder, present, any size.
    modelStat: async (path) => {
      log.push(`stat:${path}`);
      const lower = path.toLowerCase();
      if (!allowed.some((root) => lower.startsWith(root.toLowerCase()))) {
        throw `${path} is outside every folder the preview may read`;
      }
      if (lower.includes("missing")) throw `No such file: ${path}`;
      return {
        path,
        size: lower.includes("huge") ? 300 * 1024 * 1024 : 48_000,
        modifiedMs: mtimes[path] ?? 100,
      };
    },
    worktrees: async (path) => {
      if (path !== "C:\\dev\\hangar") throw "not a git repository";
      return [
        worktree("C:\\dev\\hangar", { isMain: true }),
        worktree("C:\\dev\\hangar-wt\\feature", { branch: "feature" }),
        worktree("C:\\dev\\hangar-wt\\old", { prunable: "gitdir file points to non-existent location" }),
      ];
    },
    modelHost: async (workspaceId, detached) => {
      log.push(`host:${workspaceId}:${detached ? "own window" : "here"}`);
      preview.workspace = workspaceId;
      return preview.host;
    },
    ...overrides,
  };
  let allowed: string[] = [];
  const mtimes: Record<string, number> = {};
  const preview = fakePreview(log);
  const patch = (workspaceId: string, change: Partial<Workspace>) => {
    state = {
      ...state,
      workspaces: state.workspaces.map((ws) => (ws.id === workspaceId ? { ...ws, ...change } : ws)),
    };
  };
  return { deps, log, typed, preview, patch, mtimes, sleeping, senders };
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

  it("hands project_* to the project manager's module, which refuses an ordinary agent", async () => {
    const { deps } = setup();
    await expect(runControl("project_list", fromAva, deps)).rejects.toThrow(/is for the project manager/);
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
    const { deps, log, senders } = setup();
    const result = (await runControl(
      "pane_create",
      { ...fromAva, workspace: "Shop", agent: "claude", name: "Reviewer", cwd: "packages/api", prompt: "Review the API" },
      deps,
    )) as any;
    expect(result.pane.name).toBe("Reviewer");
    expect(result.pane.cwd).toBe("C:\\dev\\shop\\packages\\api");
    expect(log).toContain("open:w2");
    expect(log).toContain("enqueue:p10:Review the API");
    // The agent that asked for the pane wrote the prompt, not the person.
    expect(senders).toEqual(["Review the API <- Ava"]);
    expect(log).toContain("announce:created:Reviewer:Ava");
  });

  it("refuses a prompt for an agent the queue cannot reach, a taken name and a missing folder", async () => {
    const { deps, log } = setup();
    await expect(
      runControl("pane_create", { ...fromAva, agent: "codex", prompt: "hi" }, deps),
    ).rejects.toThrow(/Claude Code/);
    await expect(runControl("pane_create", { ...fromAva, name: "max" }, deps)).rejects.toThrow(/already named/);
    // Taken in another workspace counts too: names are unique across Hangar.
    await expect(
      runControl("pane_create", { ...fromAva, workspace: "Shop", name: "Max" }, deps),
    ).rejects.toThrow(/in "Hangar"/);
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

  it("finds a pane by name in another workspace when the caller's has none", async () => {
    const { deps, log } = setup();
    const fromShop = { caller: { paneId: "p3", paneName: "Ava", cwd: "C:\dev\shop" } };
    await runControl("pane_close", { ...fromShop, pane: "Max" }, deps);
    expect(log).toContain("close:w1:p2");
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
    const { deps, log, senders } = setup();
    await runControl("pane_send", { ...fromAva, pane: "Ava", text: "next task", when_idle: true }, deps);
    expect(log).toContain("enqueue:p1:next task");
    expect(senders).toEqual(["next task <- Ava"]);
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

  it("says a sleeping workspace is asleep and how it wakes, without waking it", async () => {
    const { deps, log, sleeping } = setup();
    sleeping.add("w2");
    const list = (await runControl("workspace_list", fromAva, deps)) as any;
    expect(list.workspaces.map((ws: any) => [ws.name, ws.asleep])).toEqual([
      ["Hangar", false],
      ["Shop", true],
    ]);
    const panes = (await runControl("pane_list", { ...fromAva, workspace: "Shop" }, deps)) as any;
    expect(panes.workspace).toMatchObject({ open: false, asleep: true });
    expect(panes.note).toMatch(/asleep.*resume their sessions.*pane_send/);
    await expect(
      runControl("pane_read", { ...fromAva, workspace: "Shop", pane: "Ava" }, deps),
    ).rejects.toThrow(/is asleep/);
    // Looking is not using: neither wakes it, and neither counts as use.
    expect(log.filter((entry) => /^(open|touch):/.test(entry))).toEqual([]);
  });

  it("wakes a sleeping workspace when sent to, and counts that as use", async () => {
    const { deps, log, sleeping } = setup();
    sleeping.add("w2");
    await expect(
      runControl("pane_send", { ...fromAva, workspace: "Shop", pane: "Ava", text: "hi" }, deps),
    ).rejects.toThrow(/resuming their sessions/);
    expect(log).toEqual(expect.arrayContaining(["touch:w2", "open:w2"]));
  });

  it("counts creating and restarting panes as use of their workspace", async () => {
    const { deps, log } = setup();
    await runControl("pane_restart", { ...fromAva, pane: "Max" }, deps);
    await runControl("pane_create", { ...fromAva, workspace: "Shop" }, deps);
    expect(log.filter((entry) => entry.startsWith("touch:"))).toEqual(["touch:w1", "touch:w2"]);
  });

  it("starts a closed workspace for a prompt queued until its Claude Code is free", async () => {
    const { deps, log, patch } = setup();
    patch("w2", { panes: [pane("p3", "Ava", "claude")] });
    const out = await runControl(
      "pane_send",
      { ...fromAva, workspace: "Shop", pane: "Ava", text: "go", when_idle: true },
      deps,
    );
    expect(out).toEqual({ queued: true, pane: "Ava", starting: true });
    expect(log).toEqual(expect.arrayContaining(["enqueue:p3:go", "open:w2"]));
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

describe("model paths", () => {
  it("reads a relative path from a base and folds . and ..", () => {
    expect(resolvePath("C:\\dev\\hangar", "assets/chair.glb")).toBe("C:\\dev\\hangar\\assets\\chair.glb");
    expect(resolvePath("C:\\dev\\hangar", ".\\a\\..\\b.glb")).toBe("C:\\dev\\hangar\\b.glb");
    expect(resolvePath("C:\\dev\\hangar", "..\\..\\..\\..\\x.glb")).toBe("C:\\x.glb");
    expect(resolvePath("/home/me/game", "../art/ship.fbx")).toBe("/home/me/art/ship.fbx");
  });

  it("takes an absolute path as it is, a UNC share included", () => {
    expect(resolvePath("C:\\dev\\hangar", "D:/art/ship.glb")).toBe("D:\\art\\ship.glb");
    expect(resolvePath("C:\\dev\\hangar", "\\\\nas\\art\\ship.glb")).toBe("\\\\nas\\art\\ship.glb");
    expect(resolvePath("/home/me", "/srv/art/ship.glb")).toBe("/srv/art/ship.glb");
  });

  it("allows the workspace, its extra roots, its panes' folders and its live worktrees", async () => {
    const ws = workspace("w1", "Hangar", "C:\\dev\\hangar", [
      pane("p1", "Ava", "claude"),
      pane("p2", "Max", "shell", "C:\\dev\\hangar-wt\\feature"),
      pane("p3", "Leo", "shell", "tools"),
    ]);
    ws.extraRoots = ["D:\\art"];
    const listed: string[] = [];
    const roots = await modelRoots(ws, {
      worktrees: async (path) => {
        listed.push(path);
        if (path === "D:\\art") throw "not a git repository";
        return [
          worktree("C:\\dev\\hangar", { isMain: true }),
          worktree("C:\\dev\\hangar-wt\\feature"),
          worktree("C:\\dev\\hangar-wt\\review"),
          worktree("C:\\dev\\hangar-wt\\old", { prunable: "gone" }),
          worktree("C:\\dev\\hangar.git", { bare: true }),
        ];
      },
    });
    expect(listed).toEqual(["C:\\dev\\hangar", "D:\\art"]);
    expect(roots).toEqual([
      "C:\\dev\\hangar",
      "D:\\art",
      "C:\\dev\\hangar-wt\\feature",
      "C:\\dev\\hangar\\tools",
      "C:\\dev\\hangar-wt\\review",
    ]);
  });

  it("does without worktrees when git cannot list them", async () => {
    const ws = workspace("w1", "Hangar", "C:\\dev\\hangar", [pane("p1", "Ava", "claude")]);
    const roots = await modelRoots(ws, {
      worktrees: () => {
        throw new Error("git is missing");
      },
    });
    expect(roots).toEqual(["C:\\dev\\hangar"]);
  });

  it("sends Rust every granted workspace's folders, once each, and forgets deleted ones", async () => {
    const sent: string[][] = [];
    const grant = createRootGrants(async (roots) => {
      sent.push(roots);
    });
    await grant("w1", ["C:\\dev\\hangar", "C:\\dev\\hangar-wt\\a"], ["w1", "w2"]);
    await grant("w2", ["C:\\dev\\shop", "c:/dev/hangar/"], ["w1", "w2"]);
    await grant("w2", ["C:\\dev\\shop"], ["w2"]);
    expect(sent).toEqual([
      ["C:\\dev\\hangar", "C:\\dev\\hangar-wt\\a"],
      ["C:\\dev\\hangar", "C:\\dev\\hangar-wt\\a", "C:\\dev\\shop"],
      ["C:\\dev\\shop"],
    ]);
  });
});

describe("model_show", () => {
  it("opens a model from the caller's folder, announces it and answers with what the viewer found", async () => {
    const { deps, log } = setup();
    const result = await runControl("model_show", { ...fromAva, path: "assets\\chair.glb" }, deps);
    expect(result).toEqual({
      opened: true,
      path: "C:\\dev\\hangar\\assets\\chair.glb",
      format: "glb",
      title: "chair.glb",
      info: INFO,
      workspace: "Hangar",
      visible: true,
    });
    expect(log).toContain("host:w1:here");
    expect(log).toContain("show:C:\\dev\\hangar\\assets\\chair.glb:chair.glb:Ava:w1");
    expect(log).toContain("announce:modelShown:chair.glb:Ava");
    expect(log).toContain("settle:m1:45000");
    // Showing a model there is using the workspace.
    expect(log).toContain("touch:w1");
    expect(log).not.toContain("open:w1");
  });

  it("opens it in the caller's own workspace, and says so when that one is not on screen", async () => {
    const { deps, log, preview } = setup();
    preview.onScreen = false;
    preview.shown = false;
    const fromShop = { caller: { paneId: "p3", paneName: "Ava", cwd: "C:\\dev\\shop" } };
    const result = (await runControl("model_show", { ...fromShop, path: "car.glb" }, deps)) as any;
    expect(result).toMatchObject({ opened: true, workspace: "Shop", visible: false, info: INFO });
    expect(result.note).toMatch(/open in the 3D preview of "Shop", which is not the workspace on screen/);
    expect(log).toContain("host:w2:here");
    expect(log).toContain("show:C:\\dev\\shop\\car.glb:car.glb:Ava:w2");
    // Hangar is the one on screen: the line at the bottom names Shop.
    expect(log).toContain("announce:modelShownElsewhere:car.glb:Ava");
  });

  it("says so when the workspace is on screen but its preview is not", async () => {
    const { deps, preview } = setup();
    preview.shown = false;
    const result = (await runControl("model_show", { ...fromAva, path: "chair.glb" }, deps)) as any;
    expect(result).toMatchObject({ workspace: "Hangar", visible: false });
    expect(result.note).toMatch(/"Hangar" is on screen, but its 3D preview is not.*toolbar button/);
  });

  it("wakes a sleeping workspace it shows a model in, as pane_send does", async () => {
    const { deps, log, sleeping } = setup();
    sleeping.add("w2");
    const result = (await runControl(
      "model_show",
      { ...fromAva, workspace: "Shop", path: "C:\\dev\\shop\\car.glb" },
      deps,
    )) as any;
    expect(log).toContain("open:w2");
    expect(log).toContain("touch:w2");
    expect(result).toMatchObject({ workspace: "Shop", woke: true });
    expect(result.note).toMatch(/"Shop" was asleep: it is waking up now/);
  });

  it("reads a relative path from the caller's pane folder, not from the root its server reports", async () => {
    const { deps, log, patch } = setup();
    patch("w1", {
      panes: [pane("p1", "Ava", "claude"), pane("p2", "Max", "shell", "C:\\dev\\hangar-wt\\feature")],
    });
    const fromMax = { caller: { paneId: "p2", paneName: "Max", cwd: "C:\\dev\\hangar" } };
    const result = (await runControl("model_show", { ...fromMax, path: "out/ship.gltf", title: "Ship v2" }, deps)) as any;
    expect(result.path).toBe("C:\\dev\\hangar-wt\\feature\\out\\ship.gltf");
    expect(result.format).toBe("gltf");
    expect(log).toContain("show:C:\\dev\\hangar-wt\\feature\\out\\ship.gltf:Ship v2:Max:w1");
  });

  it("takes an absolute path in any spelling, and lets Rust in on the workspace and its worktrees", async () => {
    const { deps, log } = setup();
    const result = (await runControl("model_show", { ...fromAva, path: "C:/dev/hangar-wt/feature/a.stl" }, deps)) as any;
    expect(result.path).toBe("C:\\dev\\hangar-wt\\feature\\a.stl");
    expect(log).toContain("allow:w1:C:\\dev\\hangar|C:\\dev\\hangar-wt\\feature");
  });

  it("refuses what is not a model before asking anything", async () => {
    const { deps, log } = setup();
    await expect(runControl("model_show", { ...fromAva, path: "README.md" }, deps)).rejects.toThrow(
      /not a 3D model.*glb, gltf, obj/,
    );
    await expect(runControl("model_show", fromAva, deps)).rejects.toThrow(/Give the path/);
    expect(log.some((entry) => /^(allow|stat|show):/.test(entry))).toBe(false);
  });

  it("refuses a model outside the workspace, however the path gets there", async () => {
    const { deps, log } = setup();
    await expect(
      runControl("model_show", { ...fromAva, path: "D:\\downloads\\chair.glb" }, deps),
    ).rejects.toThrow(/outside your workspace.*C:\\dev\\hangar, C:\\dev\\hangar-wt\\feature/);
    await expect(
      runControl("model_show", { ...fromAva, path: "..\\shop\\car.glb" }, deps),
    ).rejects.toThrow(/C:\\dev\\shop\\car\.glb is outside your workspace/);
    expect(log.some((entry) => entry.startsWith("show:"))).toBe(false);
  });

  it("refuses another workspace's model that Rust would serve", async () => {
    // Shop showed a model earlier, so its folder is allowed too; Ava's
    // workspace is still the one that counts for Ava.
    const { deps } = setup({
      modelStat: async (path) => ({ path, size: 10, modifiedMs: 1 }),
    });
    await expect(
      runControl("model_show", { ...fromAva, path: "C:\\dev\\shop\\car.glb" }, deps),
    ).rejects.toThrow(/outside your workspace/);
  });

  it("passes on why Rust refused a file inside the workspace, and refuses one too big", async () => {
    const { deps, log } = setup();
    await expect(runControl("model_show", { ...fromAva, path: "missing.glb" }, deps)).rejects.toThrow(
      "No such file: C:\\dev\\hangar\\missing.glb",
    );
    await expect(runControl("model_show", { ...fromAva, path: "huge.fbx" }, deps)).rejects.toThrow(
      /huge\.fbx is 300 MB.*up to 256 MB/,
    );
    expect(log.some((entry) => entry.startsWith("show:"))).toBe(false);
  });

  it("answers that the model is still loading when the viewer takes too long", async () => {
    const { deps, preview } = setup();
    preview.settle = "timeout";
    const result = (await runControl("model_show", { ...fromAva, path: "big.glb" }, deps)) as any;
    expect(result).toMatchObject({ opened: true, status: "loading", path: "C:\\dev\\hangar\\big.glb" });
    expect(result.note).toMatch(/model_snapshot/);
  });

  it("hands back the viewer's error", async () => {
    const { deps, preview } = setup();
    preview.settle = "error";
    await expect(runControl("model_show", { ...fromAva, path: "chair.glb" }, deps)).rejects.toThrow(
      "The 3D preview could not load chair.glb: not a glTF file",
    );
  });

  it("shows a detached workspace's model in that workspace's window", async () => {
    const { deps, log, patch } = setup();
    patch("w1", { detached: true });
    await runControl("model_show", { ...fromAva, path: "chair.glb" }, deps);
    expect(log).toContain("host:w1:own window");
    expect(log).not.toContain("host:w1:here");
  });

  it("says so when that window does not answer", async () => {
    const { deps, patch, log } = setup({ modelHost: async () => null });
    patch("w1", { detached: true });
    await expect(runControl("model_show", { ...fromAva, path: "chair.glb" }, deps)).rejects.toThrow(
      /"Hangar" is shown in a window of its own, and that window is not answering/,
    );
    expect(log.some((entry) => entry.startsWith("announce:"))).toBe(false);
  });

  it("is refused with every other operation when pane control is off", async () => {
    const { deps, log } = setup({ enabled: () => false });
    await expect(runControl("model_show", { ...fromAva, path: "chair.glb" }, deps)).rejects.toThrow(DISABLED);
    await expect(runControl("model_snapshot", fromAva, deps)).rejects.toThrow(DISABLED);
    await expect(runControl("model_close", fromAva, deps)).rejects.toThrow(DISABLED);
    expect(log).toEqual([]);
  });
});

describe("model_snapshot", () => {
  it("says to show a model first when none is open", async () => {
    const { deps } = setup();
    await expect(runControl("model_snapshot", fromAva, deps)).rejects.toThrow(/No 3D model is open.*model_show/);
  });

  it("renders the model in front at the default size, and answers with the image and its stats", async () => {
    const { deps, log, preview } = setup();
    preview.models = [openModel("m1", "C:\\dev\\hangar\\a.glb"), openModel("m2", "C:\\dev\\hangar\\b.glb")];
    preview.activeId = "m1";
    const result = await runControl("model_snapshot", fromAva, deps);
    expect(result).toEqual({
      png: "iVBORw0KGgo=",
      mimeType: "image/png",
      path: "C:\\dev\\hangar\\a.glb",
      angle: "iso",
      width: 1024,
      height: 768,
      info: INFO,
    });
    expect(log).toContain("reveal:m1");
    // Loaded and unchanged on disk: nothing to wait for.
    expect(log.some((entry) => entry.startsWith("settle:"))).toBe(false);
  });

  it("picks a model by path or by file name, and clamps the size", async () => {
    const { deps, log, preview } = setup();
    preview.models = [openModel("m1", "C:\\dev\\hangar\\a.glb"), openModel("m2", "C:\\dev\\hangar\\art\\b.glb")];
    preview.activeId = "m1";
    await runControl("model_snapshot", { ...fromAva, path: "art/b.glb", angle: "Top", width: 5000, height: 3 }, deps);
    await runControl("model_snapshot", { ...fromAva, path: "b.glb", width: "800", height: 600.4 }, deps);
    expect(log.filter((entry) => entry.startsWith("snapshot:"))).toEqual([
      "snapshot:m2:top:2048x64",
      "snapshot:m2:iso:800x600",
    ]);
    await expect(runControl("model_snapshot", { ...fromAva, path: "c.glb" }, deps)).rejects.toThrow(
      /No open model matches "c\.glb"/,
    );
    await expect(runControl("model_snapshot", { ...fromAva, angle: "under" }, deps)).rejects.toThrow(
      /Unknown angle "under".*iso/,
    );
  });

  it("loads a file rewritten since it was shown before rendering it", async () => {
    const { deps, log, preview, mtimes } = setup();
    await deps.modelAllowRoots("w1", ["C:\\dev\\hangar"]);
    preview.models = [openModel("m1", "C:\\dev\\hangar\\a.glb", { modifiedMs: 100 })];
    mtimes["C:\\dev\\hangar\\a.glb"] = 250;
    await runControl("model_snapshot", fromAva, deps);
    const order = log.filter((entry) => /^(reload|settle|snapshot):/.test(entry));
    expect(order).toEqual(["reload:m1:250", "settle:m1:40000", "snapshot:m1:iso:1024x768"]);
  });

  it("waits for a model still loading, and gives up with a clear word", async () => {
    const { deps, preview } = setup();
    preview.models = [openModel("m1", "C:\\dev\\hangar\\a.glb", { status: "loading", info: null })];
    preview.settle = "timeout";
    await expect(runControl("model_snapshot", fromAva, deps)).rejects.toThrow(/still loading after 40 s/);
    preview.settle = "error";
    await expect(runControl("model_snapshot", fromAva, deps)).rejects.toThrow(/a\.glb failed to load: not a glTF file/);
  });

  it("explains a preview that did not come up", async () => {
    const { deps, preview } = setup();
    preview.models = [openModel("m1", "C:\\dev\\hangar\\a.glb")];
    preview.snapshotError = "the 3D preview is not mounted";
    await expect(runControl("model_snapshot", fromAva, deps)).rejects.toThrow(
      /did not come up in the window that shows "Hangar"/,
    );
  });

  it("renders the caller's workspace's model, never another workspace's", async () => {
    const { deps, log, preview } = setup();
    preview.models = [
      openModel("m1", "C:\\dev\\hangar\\a.glb"),
      openModel("m2", "C:\\dev\\shop\\car.glb", { workspaceId: "w2" }),
    ];
    preview.activeId = "m2";
    const fromShop = { caller: { paneId: "p3", paneName: "Ava", cwd: "C:\\dev\\shop" } };
    const result = (await runControl("model_snapshot", fromShop, deps)) as any;
    expect(result.path).toBe("C:\\dev\\shop\\car.glb");
    expect(log).toContain("host:w2:here");
    expect(log).toContain("snapshot:m2:iso:1024x768");
    await expect(
      runControl("model_snapshot", { ...fromShop, path: "C:\\dev\\hangar\\a.glb" }, deps),
    ).rejects.toThrow(/No open model matches/);
    expect(log.filter((entry) => entry.startsWith("snapshot:"))).toEqual(["snapshot:m2:iso:1024x768"]);
  });
});

describe("model_close", () => {
  it("closes one model by path, and announces it", async () => {
    const { deps, log, preview } = setup();
    preview.models = [openModel("m1", "C:\\dev\\hangar\\a.glb"), openModel("m2", "C:\\dev\\hangar\\b.glb")];
    const result = await runControl("model_close", { ...fromAva, path: ".\\b.glb" }, deps);
    expect(result).toEqual({ closed: ["C:\\dev\\hangar\\b.glb"], open: 1 });
    expect(log).toContain("closeModels:m2");
    expect(log).toContain("announce:modelClosed:b.glb:Ava");
  });

  it("finds a model however the agent spells its path, through Rust's canonical form", async () => {
    const { deps, log, preview } = setup();
    // Two files of the same name: only the canonical path can tell them apart.
    preview.models = [
      openModel("m1", "C:\dev\hangar\a\m.glb"),
      openModel("m2", "C:\dev\hangar\b\m.glb"),
    ];
    const stat = deps.modelStat;
    deps.modelStat = async (path) =>
      path.includes("/c/dev/hangar/b/m.glb")
        ? { path: "C:\dev\hangar\b\m.glb", size: 1, modifiedMs: 1 }
        : stat(path);
    const result = await runControl("model_close", { ...fromAva, path: "/c/dev/hangar/b/m.glb" }, deps);
    expect(result).toEqual({ closed: ["C:\dev\hangar\b\m.glb"], open: 1 });
    expect(log).toContain("closeModels:m2");
  });

  it("closes all of the caller's workspace's models, and leaves the others'", async () => {
    const { deps, log, preview } = setup();
    preview.models = [
      openModel("m1", "C:\\dev\\hangar\\a.glb"),
      openModel("m2", "C:\\dev\\shop\\car.glb", { workspaceId: "w2" }),
      openModel("m3", "C:\\dev\\hangar\\b.glb"),
    ];
    const result = await runControl("model_close", fromAva, deps);
    expect(result).toEqual({ closed: ["C:\\dev\\hangar\\a.glb", "C:\\dev\\hangar\\b.glb"], open: 0 });
    expect(log).toContain("closeModels:m1,m3");
    expect(log).toContain("announce:modelClosed:a.glb, b.glb:Ava");
    expect(preview.models.map((model) => model.id)).toEqual(["m2"]);
  });

  it("closes in the caller's workspace only, and names it when it is not on screen", async () => {
    const { deps, log, preview } = setup();
    preview.models = [
      openModel("m1", "C:\\dev\\hangar\\car.glb"),
      openModel("m2", "C:\\dev\\shop\\car.glb", { workspaceId: "w2" }),
    ];
    const fromShop = { caller: { paneId: "p3", paneName: "Ava", cwd: "C:\\dev\\shop" } };
    const result = await runControl("model_close", { ...fromShop, path: "car.glb" }, deps);
    expect(result).toEqual({ closed: ["C:\\dev\\shop\\car.glb"], open: 0 });
    expect(log).toContain("closeModels:m2");
    expect(log).toContain("announce:modelClosedElsewhere:car.glb:Ava");
    expect(preview.models.map((model) => model.id)).toEqual(["m1"]);
  });

  it("has nothing to do when nothing is open, but refuses a path that is not", async () => {
    const { deps, log } = setup();
    expect(await runControl("model_close", fromAva, deps)).toEqual({ closed: [], open: 0 });
    await expect(runControl("model_close", { ...fromAva, path: "a.glb" }, deps)).rejects.toThrow(
      /No 3D model is open/,
    );
    expect(log.some((entry) => entry.startsWith("announce:"))).toBe(false);
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
