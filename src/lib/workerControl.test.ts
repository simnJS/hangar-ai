import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "./board";
import { runControl, type ControlDeps } from "./control";
import type { ChangedFile, Worktree } from "./git";
import { MAX_PANES } from "./layout";
import type { OrchestratorEvent } from "./orchestratorEvents";
import {
  PORT_POOL_START,
  allocatePortBase,
  stopAllWorkers,
  taskSlug,
  workerBranch,
  workerBrief,
  type WorkerDeps,
} from "./workerControl";
import {
  DEFAULT_SETTINGS,
  type AgentId,
  type AppState,
  type OrchestrationSettings,
  type Pane,
  type Workspace,
} from "../types";

const events = vi.hoisted(() => [] as { workspaceId: string; event: OrchestratorEvent }[]);
vi.mock("./orchestratorEvents", () => ({
  postOrchestratorEvent: (workspaceId: string, event: OrchestratorEvent) =>
    events.push({ workspaceId, event }),
}));

const REPO = "C:\\dev\\shop";
const SIBLINGS = "C:\\dev\\shop-worktrees";

function pane(id: string, name: string, agent: AgentId, patch: Partial<Pane> = {}): Pane {
  return { id, name, agent, sessionId: null, cwd: null, shellId: null, title: null, ...patch };
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

function task(id: string, title: string, patch: Partial<Task> = {}): Task {
  return {
    id,
    title,
    description: `Do ${title}. Acceptance: it works.`,
    column: "todo",
    priority: 1,
    assignee: null,
    labels: [],
    comments: [],
    depends_on: [],
    created_at: 1,
    updated_at: 1,
    order: 1,
    owns: [],
    attempts: 0,
    ...patch,
  };
}

const ORCHESTRATOR = { paneId: "p1", paneName: "Ava", cwd: REPO };

/**
 * A Hangar with a workspace on a git repository, its orchestrator Ava and a
 * shell, and a second workspace whose pane already holds the first block of
 * ports. The board's claim follows board.rs's rules closely enough to refuse
 * what it refuses — the overlap itself is read as identical patterns, the
 * real rule being tested in Rust.
 */
function world(
  options: {
    tasks?: Task[];
    orchestration?: Partial<OrchestrationSettings>;
    panes?: Pane[];
    /** The workspace's folder: the repository's root unless said otherwise. */
    cwd?: string;
  } = {},
) {
  let state: AppState = {
    workspaces: [
      {
        ...workspace(
          "w1",
          "Shop",
          options.cwd ?? REPO,
          options.panes ?? [pane("p1", "Ava", "claude", { role: "orchestrator" }), pane("p2", "Max", "shell")],
        ),
        orchestration: options.orchestration
          ? ({ ...options.orchestration } as OrchestrationSettings)
          : undefined,
      },
      workspace("w2", "Blog", "C:\\dev\\blog", [pane("p9", "Leo", "claude", { portBase: PORT_POOL_START })]),
    ],
    folders: [],
    activeWorkspaceId: "w1",
    settings: DEFAULT_SETTINGS,
  };
  const log: string[] = [];
  const tasks = new Map((options.tasks ?? []).map((entry) => [entry.id, { ...entry }]));
  const branches = new Set(["main"]);
  const folders = new Set<string>([REPO]);
  const trees: Worktree[] = [
    {
      path: "C:/dev/shop",
      head: "abc1234",
      branch: "main",
      isMain: true,
      isCurrent: false,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
  ];
  /** Uncommitted files per worktree, and commits ahead per branch. */
  const dirty = new Map<string, ChangedFile[]>();
  const ahead = new Map<string, number>();
  const fail: { worktreeAdd?: string; addPane?: boolean; enqueue?: string; repo?: "none" | "detached" } = {};
  let next = 10;

  const mapWorkspace = (id: string, map: (ws: Workspace) => Workspace) => {
    state = { ...state, workspaces: state.workspaces.map((ws) => (ws.id === id ? map(ws) : ws)) };
  };

  const workers: WorkerDeps = {
    updatePane: (workspaceId, paneId, patch) => {
      log.push(`update:${paneId}:${JSON.stringify(patch)}`);
      mapWorkspace(workspaceId, (ws) => ({
        ...ws,
        panes: ws.panes.map((entry) => (entry.id === paneId ? { ...entry, ...patch } : entry)),
      }));
    },
    killPane: async (paneId) => {
      log.push(`kill:${paneId}`);
    },
    boardLoad: async () => ({ tasks: [...tasks.values()] }),
    claim: async (_cwd, id, worker, maxAttempts) => {
      log.push(`claim:${id}:${worker}:${maxAttempts}`);
      const entry = tasks.get(id);
      if (!entry) throw `No task ${id} on this board.`;
      if (entry.column !== "todo") throw `"${entry.title}" is in ${entry.column} already.`;
      const waiting = entry.depends_on.filter((dep) => tasks.get(dep)?.column !== "done");
      if (waiting.length) throw `"${entry.title}" depends on tasks that are not done: ${waiting.join(", ")}.`;
      if ((entry.attempts ?? 0) >= maxAttempts) {
        throw `"${entry.title}" has had ${entry.attempts} worker(s) already. Add the label needs-you.`;
      }
      const clash = [...tasks.values()].find(
        (other) =>
          other.id !== id &&
          other.column === "doing" &&
          (other.owns ?? []).some((path) => (entry.owns ?? []).includes(path)),
      );
      if (clash) throw `Cannot start "${entry.title}": it owns files that "${clash.title}" owns too.`;
      Object.assign(entry, { column: "doing", assignee: worker, attempts: (entry.attempts ?? 0) + 1 });
      return { ...entry };
    },
    release: async (_cwd, id, worker, undo) => {
      log.push(`release:${id}:${worker}:${undo}`);
      const entry = tasks.get(id)!;
      if (entry.assignee !== worker) return { ...entry };
      if (undo) entry.attempts = (entry.attempts ?? 1) - 1;
      if (entry.column === "todo" || entry.column === "doing") {
        Object.assign(entry, { column: "todo", assignee: null });
      }
      return { ...entry };
    },
    repoInfo: async () => ({
      installed: true,
      isRepo: fail.repo !== "none",
      root: fail.repo === "none" ? null : REPO,
      mainRoot: fail.repo === "none" ? null : REPO,
      isLinkedWorktree: false,
      branch: fail.repo === "detached" ? null : "main",
      head: "abc1234",
    }),
    worktreeAdd: async ({ worktreePath, branch, baseRef, createBranch }) => {
      log.push(`worktree:${worktreePath}:${branch}:${baseRef}:${createBranch}`);
      if (fail.worktreeAdd) throw fail.worktreeAdd;
      if (branches.has(branch)) throw `a branch named '${branch}' already exists`;
      branches.add(branch);
      folders.add(worktreePath);
      trees.push({ ...trees[0], path: worktreePath.replace(/\\/g, "/"), branch, isMain: false });
    },
    worktreeRemove: async (_path, worktreePath, force) => {
      log.push(`unworktree:${worktreePath}:${force}`);
      folders.delete(worktreePath);
      const at = trees.findIndex((tree) => tree.path === worktreePath.replace(/\\/g, "/"));
      if (at >= 0) trees.splice(at, 1);
      return { removed: true, needsForce: false, message: null };
    },
    branchDelete: async (_path, branch, force) => {
      log.push(`unbranch:${branch}:${force}`);
      branches.delete(branch);
      return { deleted: true, needsForce: false, message: null };
    },
    copyIgnored: async (from, to) => {
      log.push(`copy:${from}:${to}`);
      return { copied: 1, failed: 0 };
    },
    changes: async (path) => ({ root: path, base: "abc1234", files: dirty.get(path) ?? [], truncated: false }),
    commitsAhead: async (_path, base, head) => {
      log.push(`ahead:${base}..${head}`);
      return ahead.get(head) ?? 0;
    },
  };

  const deps: ControlDeps = {
    snapshot: () => state,
    enabled: () => true,
    addPane: (workspaceId, opts) => {
      const ws = state.workspaces.find((entry) => entry.id === workspaceId)!;
      if (fail.addPane || ws.panes.length >= MAX_PANES) return null;
      const id = `p${next++}`;
      log.push(`add:${JSON.stringify(opts)}`);
      mapWorkspace(workspaceId, (entry) => ({
        ...entry,
        panes: [...entry.panes, pane(id, opts.name ?? "Zed", opts.agent ?? "shell", { cwd: opts.cwd ?? null })],
      }));
      return id;
    },
    closePane: (workspaceId, paneId) => {
      log.push(`close:${paneId}`);
      mapWorkspace(workspaceId, (ws) => ({ ...ws, panes: ws.panes.filter((entry) => entry.id !== paneId) }));
    },
    respawnPane: () => null,
    openWorkspace: (id) => log.push(`open:${id}`),
    isOpen: () => true,
    asleep: () => false,
    touch: () => undefined,
    activate: () => undefined,
    terminal: async () => null,
    write: async () => undefined,
    alive: async () => true,
    activity: () => ({ workspaceId: "w1", activity: "working", bridged: true }),
    enqueue: async (paneId, text, from) => {
      if (fail.enqueue) throw fail.enqueue;
      log.push(`enqueue:${paneId}:${from}`);
      briefs.set(paneId, text);
    },
    dirExists: async (path) => folders.has(path),
    canDetach: () => false,
    detach: async () => undefined,
    reattach: () => undefined,
    announce: (event) => log.push(`announce:${event.kind}:${"pane" in event ? event.pane : ""}:${event.by}`),
    sleep: async () => undefined,
    modelAllowRoots: async () => undefined,
    modelStat: async () => {
      throw "no models here";
    },
    worktrees: async () => trees.map((tree) => ({ ...tree })),
    modelHost: async () => null,
    workers,
  };
  const briefs = new Map<string, string>();

  const call = (op: string, args: Record<string, unknown> = {}, caller: object = ORCHESTRATOR) =>
    runControl(op, { ...args, caller }, deps) as Promise<any>;
  const shop = () => state.workspaces.find((ws) => ws.id === "w1")!;
  const paneNamed = (name: string) => shop().panes.find((entry) => entry.name === name);
  return { deps, log, tasks, branches, folders, trees, dirty, ahead, fail, briefs, call, shop, paneNamed };
}

beforeEach(() => {
  events.length = 0;
});

describe("worker_spawn", () => {
  it("claims the task, checks out a worktree from the target branch, opens the worker and queues its brief", async () => {
    const app = world({
      tasks: [task("login-1", "Login form", { owns: ["src/login/**"] })],
      orchestration: { checkCommand: "pnpm test" },
    });

    const spawned = await app.call("worker_spawn", { task: "login-1" });

    const branch = "hangar/login-form-login1";
    const path = `${SIBLINGS}\\hangar-login-form-login1`;
    expect(spawned).toMatchObject({
      worker: "Mia",
      task: { id: "login-1", title: "Login form", attempt: 1, maxAttempts: 2 },
      model: "sonnet",
      branch,
      base: "main",
      worktree: path,
      ports: { base: PORT_POOL_START + 20, count: 20 },
    });
    const worker = app.paneNamed("Mia")!;
    expect(worker).toMatchObject({
      agent: "claude",
      cwd: path,
      role: "worker",
      taskId: "login-1",
      model: "sonnet",
      portBase: PORT_POOL_START + 20,
    });
    expect(app.tasks.get("login-1")).toMatchObject({ column: "doing", assignee: "Mia", attempts: 1 });
    // In that order: nothing is created before the claim holds.
    expect(app.log.slice(0, 7)).toEqual([
      "claim:login-1:Mia:2",
      `worktree:${path}:${branch}:main:true`,
      `copy:${REPO}:${path}`,
      `add:${JSON.stringify({ agent: "claude", cwd: path, name: "Mia", near: "p1" })}`,
      `update:${worker.id}:${JSON.stringify({ role: "worker", taskId: "login-1", model: "sonnet", portBase: PORT_POOL_START + 20 })}`,
      "open:w1",
      `enqueue:${worker.id}:Ava`,
    ]);
    expect(app.log).toContain("announce:created:Mia:Ava");
    expect(events).toEqual([
      { workspaceId: "w1", event: { kind: "workerStarted", worker: "Mia", taskId: "login-1" } },
    ]);

    const brief = app.briefs.get(worker.id)!;
    expect(brief).toContain("## Task login-1: Login form");
    expect(brief).toContain("Do Login form. Acceptance: it works.");
    expect(brief).toContain("`src/login/**`. Change nothing outside them.");
    expect(brief).toContain(`(${PORT_POOL_START + 20} to ${PORT_POOL_START + 39};`);
    expect(brief).toContain("`pnpm test` passes in your worktree");
    expect(brief).toContain(`on branch ${branch}, made from main`);
    expect(brief).toContain("delivery_submit");
    expect(brief).toContain("Never mock, stub or guess");
  });

  it("starts the worker with the model asked for, and refuses one that is not a model name", async () => {
    const app = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
      orchestration: { workerModel: "opus" },
    });

    expect((await app.call("worker_spawn", { task: "t1" })).model).toBe("opus");
    expect((await app.call("worker_spawn", { task: "t2", model: "haiku", name: "Quick" })).model).toBe("haiku");
    expect(app.paneNamed("Quick")?.model).toBe("haiku");

    await expect(app.call("worker_spawn", { task: "t2", model: "haiku; rm -rf /" })).rejects.toThrow(
      /not a model name/,
    );
  });

  it("gives each worker a block of ports no pane of any workspace has", async () => {
    const app = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
    });
    const first = await app.call("worker_spawn", { task: "t1" });
    const second = await app.call("worker_spawn", { task: "t2" });

    // 21000 is the Blog workspace's.
    expect([first.ports.base, second.ports.base]).toEqual([PORT_POOL_START + 20, PORT_POOL_START + 40]);
    expect(allocatePortBase(app.deps.snapshot().workspaces)).toBe(PORT_POOL_START + 60);
    // A block another pane overlaps is skipped too, aligned or not.
    expect(allocatePortBase([workspace("x", "x", "x", [pane("a", "a", "shell", { portBase: 41005 })])])).toBe(
      PORT_POOL_START + 40,
    );
  });

  it("refuses beyond maxWorkers before touching anything", async () => {
    const app = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
      orchestration: { maxWorkers: 1 },
    });
    await app.call("worker_spawn", { task: "t1" });
    app.log.length = 0;

    await expect(app.call("worker_spawn", { task: "t2" })).rejects.toThrow(
      /already runs 1 worker\(s\) \("Mia"\), its limit \(maxWorkers 1\).*worker_stop/,
    );
    expect(app.log).toEqual([]);
    expect(app.tasks.get("t2")!.column).toBe("todo");
  });

  it("counts a start still under way against maxWorkers, and never hands two the same name or ports", async () => {
    const limited = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
      orchestration: { maxWorkers: 1 },
    });
    const both = await Promise.allSettled([
      limited.call("worker_spawn", { task: "t1" }),
      limited.call("worker_spawn", { task: "t2" }),
    ]);
    expect(both.map((outcome) => outcome.status)).toEqual(["fulfilled", "rejected"]);

    const roomy = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
    });
    const [one, two] = await Promise.all([
      roomy.call("worker_spawn", { task: "t1" }),
      roomy.call("worker_spawn", { task: "t2" }),
    ]);
    expect(one.worker).not.toBe(two.worker);
    expect(one.ports.base).not.toBe(two.ports.base);
  });

  it("passes on every refusal of the claim, and leaves nothing behind", async () => {
    const app = world({
      tasks: [
        task("busy", "Busy", { column: "doing", assignee: "Leo", owns: ["src/shared/**"] }),
        task("later", "Later", { depends_on: ["busy"], owns: ["c/**"] }),
        task("tired", "Tired", { attempts: 2, owns: ["d/**"] }),
        task("clash", "Clash", { owns: ["src/shared/**"] }),
      ],
    });
    const cases: [string, RegExp][] = [
      ["nope", /No task nope/],
      ["busy", /in doing already/],
      ["later", /depends on tasks that are not done: busy/],
      ["tired", /2 worker\(s\) already.*needs-you/],
      ["clash", /owns files that "Busy" owns too/],
    ];
    for (const [id, refusal] of cases) {
      await expect(app.call("worker_spawn", { task: id })).rejects.toThrow(refusal);
    }
    expect(app.log.filter((line) => !line.startsWith("claim:"))).toEqual([]);
    expect(app.shop().panes).toHaveLength(2);
    expect(app.tasks.get("tired")!.attempts).toBe(2);
    expect(events).toEqual([]);
  });

  it("starts workers on tasks with disjoint files, and refuses the third that overlaps one of them", async () => {
    const app = world({
      tasks: [
        task("api", "API", { owns: ["src-tauri/src/board.rs"] }),
        task("ui", "UI", { owns: ["src/components/BoardView.tsx"] }),
        task("both", "Both", { owns: ["src/components/BoardView.tsx", "docs/board.md"] }),
      ],
    });
    await app.call("worker_spawn", { task: "api" });
    await app.call("worker_spawn", { task: "ui" });

    await expect(app.call("worker_spawn", { task: "both" })).rejects.toThrow(/"UI" owns too/);
    expect(app.tasks.get("both")).toMatchObject({ column: "todo", assignee: null, attempts: 0 });
  });

  it("tells a later attempt it is one, up to maxAttempts", async () => {
    const app = world({ tasks: [task("t1", "Again", { attempts: 1, owns: ["a/**"] })] });
    const spawned = await app.call("worker_spawn", { task: "t1" });

    expect(spawned.task).toMatchObject({ attempt: 2, maxAttempts: 2 });
    const brief = app.briefs.get(app.paneNamed(spawned.worker)!.id)!;
    expect(brief).toContain("This is attempt 2 of 2");
  });

  it("refuses a workspace with no room, no repository or no branch to start from", async () => {
    const crowded = Array.from({ length: MAX_PANES }, (_, at) =>
      pane(`c${at}`, `C${at}`, "shell", at === 0 ? { role: "orchestrator", agent: "claude" } : {}),
    );
    const full = world({ tasks: [task("t1", "One")], panes: crowded });
    await expect(full.call("worker_spawn", { task: "t1" }, { paneId: "c0" })).rejects.toThrow(/is full/);

    const bare = world({ tasks: [task("t1", "One")] });
    bare.fail.repo = "none";
    await expect(bare.call("worker_spawn", { task: "t1" })).rejects.toThrow(/not a git repository/);
    bare.fail.repo = "detached";
    await expect(bare.call("worker_spawn", { task: "t1" })).rejects.toThrow(/detached HEAD/);
    expect(bare.log).toEqual([]);
  });

  it("undoes the claim when the worktree cannot be made", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    app.fail.worktreeAdd = "fatal: invalid reference: develop";

    await expect(app.call("worker_spawn", { task: "t1" })).rejects.toThrow(
      /Could not create the worker's worktree.*invalid reference: develop.*Nothing was left behind/,
    );
    expect(app.tasks.get("t1")).toMatchObject({ column: "todo", assignee: null, attempts: 0 });
    expect(app.shop().panes).toHaveLength(2);
    expect(app.log).toContain("release:t1:Mia:true");
  });

  it("removes the worktree and its branch when the pane cannot be opened", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    app.fail.addPane = true;

    await expect(app.call("worker_spawn", { task: "t1" })).rejects.toThrow(/is full.*Nothing was left behind/);
    expect(app.log.slice(-3)).toEqual([
      `unworktree:${SIBLINGS}\\hangar-one-t1:true`,
      "unbranch:hangar/one-t1:true",
      "release:t1:Mia:true",
    ]);
    expect([...app.branches]).toEqual(["main"]);
    expect(app.tasks.get("t1")).toMatchObject({ column: "todo", attempts: 0 });
  });

  it("closes the pane, removes the worktree and gives the task back when the brief cannot be queued", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    app.fail.enqueue = "the bridge is gone";

    await expect(app.call("worker_spawn", { task: "t1" })).rejects.toThrow(/the bridge is gone/);
    expect(app.paneNamed("Mia")).toBeUndefined();
    expect(app.log.filter((line) => /^(kill|close|unworktree|unbranch|release):/.test(line))).toEqual([
      "kill:p10",
      "close:p10",
      `unworktree:${SIBLINGS}\\hangar-one-t1:true`,
      "unbranch:hangar/one-t1:true",
      "release:t1:Mia:true",
    ]);
    expect(app.tasks.get("t1")).toMatchObject({ column: "todo", assignee: null, attempts: 0 });
    expect(events).toEqual([]);

    // Whatever it held — the name, the place, the ports — is free again.
    app.fail.enqueue = undefined;
    const retried = await app.call("worker_spawn", { task: "t1" });
    expect(retried).toMatchObject({ worker: "Mia", ports: { base: PORT_POOL_START + 20 } });
  });

  it("takes the next branch name when an earlier attempt's branch is still there", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    app.branches.add("hangar/one-t1");

    const spawned = await app.call("worker_spawn", { task: "t1" });
    expect(spawned.branch).toBe("hangar/one-t1-2");
    expect(spawned.worktree).toBe(`${SIBLINGS}\\hangar-one-t1-2`);
  });

  it("starts the worker in the same subfolder of its worktree as the workspace is of the repository", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })], cwd: `${REPO}\\packages\\App` });
    const spawned = await app.call("worker_spawn", { task: "t1" });

    expect(spawned.worktree).toBe(`${SIBLINGS}\\hangar-one-t1`);
    expect(app.paneNamed("Mia")!.cwd).toBe(`${SIBLINGS}\\hangar-one-t1\\packages\\App`);
    expect((await app.call("worker_list")).workers[0].worktree).toBe("C:/dev/shop-worktrees/hangar-one-t1");

    const stopped = await app.call("worker_stop", { worker: "Mia" });
    expect(stopped.worktree).toMatchObject({ path: "C:/dev/shop-worktrees/hangar-one-t1", removed: true });
  });

  it("refuses a name another pane has anywhere in Hangar", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    await expect(app.call("worker_spawn", { task: "t1", name: "leo" })).rejects.toThrow(
      /already named "leo" \(in "Blog"\)/,
    );
    expect(app.log).toEqual([]);
  });

  it("refuses anyone but the workspace's orchestrator", async () => {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    await expect(
      app.call("worker_spawn", { task: "t1" }, { paneId: "p2", paneName: "Max", cwd: REPO }),
    ).rejects.toThrow(/orchestrator of "Shop", which is "Ava", not you/);
    await expect(app.call("worker_list", {}, { paneId: null, paneName: null, cwd: REPO })).rejects.toThrow(
      /which is "Ava", not you/,
    );

    const spawned = await app.call("worker_spawn", { task: "t1" });
    const worker = app.paneNamed(spawned.worker)!;
    await expect(
      app.call("worker_stop", { worker: "Mia" }, { paneId: worker.id, paneName: "Mia", cwd: worker.cwd }),
    ).rejects.toThrow(/you are one of its workers/);

    const unpinned = world({
      tasks: [task("t1", "One")],
      panes: [pane("p1", "Ava", "claude"), pane("p2", "Max", "shell")],
    });
    await expect(unpinned.call("worker_spawn", { task: "t1" })).rejects.toThrow(/"Shop" has none/);
    expect(unpinned.log).toEqual([]);
  });

  it("refuses to start a worker that could never receive its brief", async () => {
    const app = world({ tasks: [task("t1", "One")] });
    const state = app.deps.snapshot();
    app.deps.snapshot = () => ({ ...state, settings: { ...state.settings, claudeBridge: false } });
    await expect(app.call("worker_spawn", { task: "t1" })).rejects.toThrow(/bridge is turned off/);
  });
});

describe("worker_list", () => {
  it("lists each worker with its task, branch, ports and delivery", async () => {
    const app = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
    });
    await app.call("worker_spawn", { task: "t1" });
    await app.call("worker_spawn", { task: "t2", model: "haiku" });
    app.tasks.get("t2")!.labels = ["ready-to-land"];

    const listed = await app.call("worker_list");
    expect(listed).toMatchObject({ workspace: "Shop", maxWorkers: 3, maxAttempts: 2 });
    expect(listed.workers).toEqual([
      {
        name: "Mia",
        task: { id: "t1", title: "One", column: "doing", attempts: 1 },
        activity: "working",
        running: true,
        branch: "hangar/one-t1",
        worktree: `${SIBLINGS}\\hangar-one-t1`,
        model: "sonnet",
        ports: { base: PORT_POOL_START + 20, count: 20 },
        delivery: null,
      },
      expect.objectContaining({ name: "Sam", model: "haiku", delivery: "ready" }),
    ]);
  });
});

describe("worker_stop", () => {
  async function started() {
    const app = world({ tasks: [task("t1", "One", { owns: ["a/**"] })] });
    const spawned = await app.call("worker_spawn", { task: "t1" });
    const worker = app.paneNamed(spawned.worker)!;
    app.log.length = 0;
    events.length = 0;
    return { app, worker, path: spawned.worktree as string };
  }

  it("ends its processes, closes it, gives the task back and removes a worktree that holds nothing", async () => {
    const { app, worker, path } = await started();

    const stopped = await app.call("worker_stop", { worker: "mia" });

    expect(stopped).toEqual({
      stopped: "Mia",
      task: { id: "t1", title: "One", column: "todo" },
      worktree: { path, branch: "hangar/one-t1", removed: true, branchDeleted: true },
    });
    expect(app.log).toEqual([
      `kill:${worker.id}`,
      `close:${worker.id}`,
      "release:t1:Mia:false",
      "ahead:main..hangar/one-t1",
      `unworktree:${path}:true`,
      "unbranch:hangar/one-t1:true",
      "announce:closed:Mia:Ava",
    ]);
    // The attempt still counts.
    expect(app.tasks.get("t1")).toMatchObject({ column: "todo", assignee: null, attempts: 1 });
    expect(events).toEqual([
      { workspaceId: "w1", event: { kind: "workerStopped", worker: "Mia", taskId: "t1" } },
    ]);
  });

  it("keeps a worktree with uncommitted changes, and says which", async () => {
    const { app, path } = await started();
    app.dirty.set(path, [
      { path: "a/x.ts", oldPath: null, status: "M", additions: 1, deletions: 0, binary: false },
    ]);

    const stopped = await app.call("worker_stop", { worker: "Mia" });
    expect(stopped.worktree).toMatchObject({ removed: false, branchDeleted: false });
    expect(stopped.worktree.kept).toMatch(/1 uncommitted change\(s\): a\/x\.ts/);
    expect(app.log.some((line) => line.startsWith("unworktree:"))).toBe(false);
  });

  it("keeps a branch with commits the target does not have", async () => {
    const { app } = await started();
    app.ahead.set("hangar/one-t1", 2);

    const stopped = await app.call("worker_stop", { worker: "Mia" });
    expect(stopped.worktree.kept).toMatch(/hangar\/one-t1 has 2 commit\(s\) that main does not/);
    expect(app.branches.has("hangar/one-t1")).toBe(true);
  });

  it("keeps the worktree when asked to", async () => {
    const { app } = await started();
    const stopped = await app.call("worker_stop", { worker: "Mia", keepWorktree: true });
    expect(stopped.worktree).toMatchObject({ removed: false, kept: "keepWorktree was asked for" });
  });

  it("only stops workers", async () => {
    const { app } = await started();
    await expect(app.call("worker_stop", { worker: "Max" })).rejects.toThrow(/"Max" is not a worker/);
    await expect(app.call("worker_stop", { worker: "Nobody" })).rejects.toThrow(/No worker "Nobody".*"Mia"/);
  });

  it("stops every worker of a workspace for the person", async () => {
    const app = world({
      tasks: [task("t1", "One", { owns: ["a/**"] }), task("t2", "Two", { owns: ["b/**"] })],
    });
    await app.call("worker_spawn", { task: "t1" });
    await app.call("worker_spawn", { task: "t2" });

    expect(await stopAllWorkers(app.shop(), app.deps)).toBe(2);
    expect(app.shop().panes.map((entry) => entry.name)).toEqual(["Ava", "Max"]);
    expect(app.log).toContain("announce:closed:Sam:null");
    expect([...app.tasks.values()].map((entry) => entry.column)).toEqual(["todo", "todo"]);
  });
});

describe("names", () => {
  it("makes a branch from the task's title and id", () => {
    expect(taskSlug("Écran de connexion : mot de passe oublié")).toBe("ecran-de-connexion-mot-de-passe");
    expect(taskSlug("ログイン")).toBe("task");
    expect(workerBranch({ id: "3f2a9c41-0000", title: "Fix the login form" })).toBe(
      "hangar/fix-the-login-form-3f2a9c",
    );
  });

  it("writes a brief that says what to do when a task owns no files", () => {
    const brief = workerBrief({
      task: { id: "t", title: "T", description: "", owns: [] },
      worker: "Sam",
      orchestrator: null,
      workspace: "Shop",
      branch: "hangar/t-t",
      target: "main",
      worktree: "/w",
      portBase: 41000,
      checkCommand: null,
      attempt: 1,
      maxAttempts: 2,
    });
    expect(brief).toContain("The person started you");
    expect(brief).toContain("No files were assigned to this task");
    expect(brief).toContain("(No description beyond the title.)");
    expect(brief).not.toContain("attempt 1");
  });
});
