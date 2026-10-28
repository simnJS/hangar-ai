import { describe, expect, it } from "vitest";
import {
  PLAN_TAG,
  briefingFor,
  buildBriefing,
  findPlan,
  planTitle,
  releaseMessage,
  roleMessage,
  type BriefingInputs,
} from "./orchestratorBrief";
import { loadProjects } from "./projects";
import type { MemoryEntry } from "./memory";
import type { BoardTask, JournalEntry } from "./orchestratorEvents";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

function pane(id: string, name: string, role?: PaneRole, extra: Partial<Pane> = {}): Pane {
  return {
    id,
    name,
    agent: "claude",
    sessionId: null,
    cwd: null,
    shellId: null,
    title: null,
    ...(role ? { role } : {}),
    ...extra,
  };
}

function workspace(id: string, name: string, panes: Pane[], extra: Partial<Workspace> = {}): Workspace {
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
    ...extra,
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

const NOW = new Date(2026, 9, 27, 14, 5).getTime();

function memory(title: string, content: string, extra: Partial<MemoryEntry> = {}): MemoryEntry {
  return {
    id: `m-${title.length}`,
    title,
    content,
    tags: [PLAN_TAG],
    workspace: "C:\\dev\\shop",
    agent: "orch",
    created_at: NOW - 3_600_000,
    updated_at: NOW - 12 * 60_000,
    ...extra,
  };
}

function setup(overrides: Partial<BriefingInputs> = {}) {
  const shop = workspace("shop", "Shop", [
    pane("o", "orch", "orchestrator"),
    pane("w1", "worker-1", "worker", { taskId: "t1", portBase: 41000 }),
    pane("p", "plain"),
    pane("s", "shell-ish", "worker", { agent: "shell" }),
  ], { orchestration: { ...DEFAULT_ORCH, checkCommand: "pnpm test", maxWorkers: 2 } });
  const state: AppState = {
    workspaces: [shop, workspace("blog", "Blog", [pane("b", "blogger")])],
    folders: [],
    activeWorkspaceId: "shop",
    settings: DEFAULT_SETTINGS,
  };
  const tasks = [
    task("t1", {
      title: "Login form",
      column: "doing",
      assignee: "worker-1",
      attempts: 1,
      owns: ["src/login/**", "src/login.test.ts"],
      description: "Goal: a login form.\nAcceptance:\n- submits\nCheck: pnpm test",
      comments: [{ id: "c1", author: "worker-1", text: "Form done,\ntests next", created_at: NOW - 60_000 }],
    }),
    task("t2", { title: "Settings page", depends_on: ["t1"], owns: ["src/settings/**"] }),
    task("t3", { title: "Docs", priority: 3 }),
    task("t0", { title: "Scaffold", column: "done" }),
  ];
  const journal: JournalEntry[] = [
    { at: NOW - 5 * 60_000, quiet: true, event: { kind: "workerStarted", worker: "worker-1", taskId: "t1" } },
    {
      at: NOW - 60_000,
      quiet: false,
      event: {
        kind: "delivery",
        taskId: "t1",
        title: "Login form",
        worker: "worker-1",
        outcome: "checkFailed",
        detail: "2 tests failing",
      },
    },
  ];
  const inputs: BriefingInputs = {
    state,
    paneId: "o",
    tasks,
    memory: [memory("Plan — Shop", "Objective: let people log in.\nValidation criteria:\n1. a user logs in")],
    journal,
    activity: (id) => (id === "w1" ? { workspaceId: "shop", activity: "working", bridged: true } : undefined),
    now: NOW,
    ...overrides,
  };
  return inputs;
}

const DEFAULT_ORCH = {
  maxWorkers: 3,
  maxAttempts: 2,
  autoMerge: true,
  checkCommand: null,
  orchestratorModel: "opus",
  workerModel: "sonnet",
  reviewerModel: "sonnet",
  branch: null,
  permissionMode: null,
};

describe("who gets a briefing", () => {
  it("is only a Claude Code pane with a role", () => {
    expect(buildBriefing(setup({ paneId: "p" }))).toBeNull();
    expect(buildBriefing(setup({ paneId: "s" }))).toBeNull();
    expect(buildBriefing(setup({ paneId: "b" }))).toBeNull();
    expect(buildBriefing(setup({ paneId: "gone" }))).toBeNull();
    expect(buildBriefing(setup({ paneId: "o" }))?.role).toBe("orchestrator");
    expect(buildBriefing(setup({ paneId: "w1" }))?.role).toBe("worker");
  });

  it("reads no board and no memory for a pane without a role", async () => {
    const reads: string[] = [];
    const inputs = setup();
    const deps = {
      snapshot: () => inputs.state,
      loadBoard: async (cwd: string) => {
        reads.push(`board ${cwd}`);
        return inputs.tasks;
      },
      loadMemory: async () => {
        reads.push("memory");
        return inputs.memory;
      },
      journal: () => inputs.journal,
      activity: inputs.activity,
      now: () => NOW,
    };
    expect(await briefingFor("p", deps)).toBeNull();
    expect(reads).toEqual([]);
    expect((await briefingFor("w1", deps))?.role).toBe("worker");
    expect(reads).toEqual(["board C:\\dev\\shop"]);
    expect((await briefingFor("o", deps))?.context).toContain("let people log in");
    // An unreadable memory is no plan, not a failed briefing.
    const broken = { ...deps, loadMemory: async () => Promise.reject(new Error("locked")) };
    expect((await briefingFor("o", broken))?.context).toContain("Plan: none yet");
  });
});

describe("the orchestrator's briefing", () => {
  it("gives it the rules it is held to and the tools it has", () => {
    const { system } = buildBriefing(setup())!;
    for (const needed of [
      'orchestrator of the Hangar workspace "Shop"',
      "Do not write product code",
      "Never push",
      '"[Hangar]"',
      "Do not poll",
      "AskUserQuestion",
      "worker_spawn",
      "worker_stop",
      // A model reads the list as all the tool takes.
      "board_update_task {id, title, description, column,",
      "delivery landed",
      "owns",
      "depends_on",
      '"needs-you"',
      'titled exactly "Plan — Shop", tagged "orchestration"',
      "Validation criteria",
    ]) {
      expect(system).toContain(needed);
    }
  });

  it("hands back the board, its plan and the latest events", () => {
    const { context } = buildBriefing(setup())!;
    expect(context).toContain('# Hangar state at 14:05, workspace "Shop" (C:\\dev\\shop)');
    expect(context).toContain("at most 2 workers at once, 2 attempts per task, check `pnpm test`");
    expect(context).toContain('- worker-1 on "Login form" [t1]: working');
    expect(context).toContain("Board: 2 todo, 1 doing, 0 review, 1 done.");
    expect(context).toContain(
      '- "Login form" [t1] · worker-1 · attempt 1/2 · owns src/login/**, src/login.test.ts',
    );
    expect(context).toContain('  last comment, worker-1: "Form done, / tests next"');
    expect(context).toContain('- "Settings page" [t2] · owns src/settings/** · after "Login form" [t1]');
    // Priority first within a column.
    expect(context.indexOf('"Docs" [t3]')).toBeLessThan(context.indexOf('"Settings page" [t2]'));
    expect(context).toContain('- "Docs" [t3] · priority 3 · owns nothing yet · ready');
    expect(context).toContain('Done: "Scaffold".');
    expect(context).toContain('Plan (memory "Plan — Shop" [m-11], updated 12 min ago):\nObjective: let people log in.');
    expect(context).toContain("Latest events, oldest first:");
    expect(context).toContain(
      '- 14:04 Delivery of "Login form" [t1] by worker-1 failed the check; Hangar sent the worker the output — 2 tests failing',
    );
  });

  it("says where the plan goes when there is none yet", () => {
    const { context } = buildBriefing(setup({ memory: [] }))!;
    expect(context).toContain(
      'Plan: none yet. Write it with memory_write, title "Plan — Shop", tag "orchestration", before creating tasks.',
    );
  });

  it("stays compact on a big board with a long plan", () => {
    const tasks = Array.from({ length: 120 }, (_, i) =>
      task(`t${i}`, {
        title: `A task with a fairly long title number ${i}`,
        column: i % 3 === 0 ? "done" : "todo",
        owns: [`src/area${i}/**`],
        description: "x".repeat(2_000),
        comments: [{ id: `c${i}`, author: "w", text: "y".repeat(2_000), created_at: 1 }],
      }),
    );
    const journal: JournalEntry[] = Array.from({ length: 50 }, (_, i) => ({
      at: NOW - i,
      quiet: false,
      event: { kind: "taskCommented", taskId: "t1", title: "T", by: "w", text: "z".repeat(1_000) },
    }));
    const { context } = buildBriefing(
      setup({ tasks, journal, memory: [memory("Plan — Shop", "p".repeat(20_000))] }),
    )!;
    expect(context.length).toBeLessThan(16_000);
    expect(context).toContain("…and 55 more open tasks: board_list_tasks.");
    expect(context).toContain("(cut here: memory_read m-11 for the rest, and shorten it)");
    expect(context.match(/^- \d\d:\d\d /gm)).toHaveLength(20);
  });
});

describe("a workspace that is not a git repository", () => {
  it("tells the orchestrator its workers share the folder, rather than to ask for a repository", () => {
    const { system, context } = buildBriefing(setup({ git: false }))!;
    expect(system).toContain("in the workspace's folder, beside the other workers");
    expect(system).toContain("do not ask the person to make it one");
    expect(system).toContain("delivery is in");
    expect(system).toContain("worker_stop {worker}");
    expect(system).not.toMatch(/git worktree|git log|delivery landed|merge branches|keepWorktree/);
    expect(context).toContain("Not a git repository: workers share the workspace's folder");
    expect(context).toContain("check `pnpm test`, run by the workers");
  });

  it("asks only for the orchestrator, and takes a folder it cannot tell for a repository", async () => {
    const inputs = setup();
    const asked: string[] = [];
    const deps = {
      snapshot: () => inputs.state,
      loadBoard: async () => inputs.tasks,
      loadMemory: async () => inputs.memory,
      journal: () => inputs.journal,
      activity: inputs.activity,
      now: () => NOW,
      isRepo: async (cwd: string) => {
        asked.push(cwd);
        return false;
      },
    };
    expect((await briefingFor("o", deps))?.system).toContain("## No git here");
    expect((await briefingFor("w1", deps))?.system).toContain("in a git worktree");
    expect(asked).toEqual(["C:\\dev\\shop"]);
    const unknown = { ...deps, isRepo: async () => null };
    expect((await briefingFor("o", unknown))?.system).not.toContain("## No git here");
  });

  it("gives a worker in the shared folder rules that keep it off the others' files", () => {
    const inputs = setup({ paneId: "w1" });
    const shop = inputs.state.workspaces[0];
    const panes = shop.panes.map((entry) => (entry.id === "w1" ? { ...entry, sharedFolder: true } : entry));
    const state = { ...inputs.state, workspaces: [{ ...shop, panes }, ...inputs.state.workspaces.slice(1)] };
    const { system } = buildBriefing({ ...inputs, state })!;
    expect(system).toContain("The workspace is not a git repository: you work in its folder itself");
    expect(system).toContain("Never revert, move, reformat or delete a file that is not yours");
    expect(system).toContain("Nothing is merged or reviewed here");
    expect(system).not.toMatch(/worktree|branch|Commit/);
  });
});

describe("a worker's briefing", () => {
  it("gives it its task, its files, its check and its ports", () => {
    const { system, context } = buildBriefing(setup({ paneId: "w1" }))!;
    expect(system).toContain('to do one board task, "Login form" [t1]');
    expect(system).toContain('orchestrator (pane "orch")');
    expect(system).toContain("delivery_submit");
    expect(system).toContain("Never push");
    expect(system).toContain("HANGAR_PORT_BASE");
    expect(context).toContain('Your task "Login form" [t1]: in doing, attempt 1 of 2.');
    expect(context).toContain("Owns: src/login/**, src/login.test.ts.");
    expect(context).toContain('Orchestrator: pane "orch". Check: `pnpm test`. Ports: 41000 to 41019 (HANGAR_PORT_BASE).');
    expect(context).toContain("Description:\nGoal: a login form.");
    expect(context).toContain('worker-1: "Form done, / tests next"');
    expect(context).toContain("2 tests failing");
    expect(context).not.toContain("workerStarted");
    expect(context).not.toContain("Plan");
  });
});

describe("the plan document", () => {
  it("is named after the workspace, and its folder when another has that name", () => {
    const a = workspace("a", "Shop", []);
    const b = workspace("b", "shop", []);
    expect(planTitle(a, [a])).toBe("Plan — Shop");
    expect(planTitle(a, [a, b])).toBe("Plan — Shop (C:\\dev\\a)");
  });

  it("is found by its title, or by the folder it was written from", () => {
    const ws = workspace("shop", "Shop", []);
    const renamed = memory("Plan — Old name", "old", { workspace: "c:/dev/shop/" });
    const other = memory("Plan — Elsewhere", "x", { workspace: "C:\\dev\\blog" });
    const untagged = memory("Plan — Notes", "n", { tags: [] });
    expect(findPlan([renamed, other], "Plan — Shop", ws)?.content).toBe("old");
    expect(findPlan([other, untagged], "Plan — Shop", ws)).toBeNull();
    const exact = memory("Plan — Shop", "exact");
    expect(findPlan([renamed, exact], "Plan — Shop", ws)?.content).toBe("exact");
  });
});

describe("a role given mid-session", () => {
  it("carries the role and the state in one message", () => {
    const briefing = buildBriefing(setup())!;
    const text = roleMessage(briefing, "Shop");
    expect(text.startsWith('Hangar has just pinned you as the orchestrator of workspace "Shop".')).toBe(true);
    expect(text).toContain(briefing.system);
    expect(text).toContain(briefing.context);
    expect(releaseMessage("orchestrator", "Shop")).toContain("no longer the orchestrator");
  });
});

describe("the manager's briefing", () => {
  function managerSetup() {
    const inputs = setup();
    const hq = workspace("hq", "HQ", [pane("m", "manager", "manager")], { hq: true });
    const state: AppState = { ...inputs.state, workspaces: [hq, ...inputs.state.workspaces] };
    return { ...inputs, state, paneId: "m" };
  }

  const deps = (inputs: BriefingInputs, memoryEntries = inputs.memory) => ({
    snapshot: () => inputs.state,
    loadBoard: async () => inputs.tasks,
    loadMemory: async () => memoryEntries,
    journal: () => inputs.journal,
    activity: inputs.activity,
    now: () => NOW,
    loadProjects: () =>
      loadProjects({
        snapshot: () => inputs.state,
        loadBoard: async () => inputs.tasks,
        loadMemory: async () => memoryEntries,
        isRepo: async () => true,
        activity: inputs.activity,
        asleep: () => false,
        isOpen: () => true,
        journal: () => inputs.journal,
      }),
  });

  it("gives it its role, its rules and its tools", () => {
    const briefing = buildBriefing(managerSetup())!;
    expect(briefing.role).toBe("manager");
    expect(briefing.system).toContain("project manager");
    expect(briefing.system).toContain("Never write code");
    expect(briefing.system).toContain("project_send");
    expect(briefing.system).toContain("project_list");
    expect(briefing.system).toContain('"Projects — tracker"');
  });

  it("hands back the overview without the HQ, and says where the tracker goes when there is none", async () => {
    const inputs = managerSetup();
    const briefing = (await briefingFor("m", deps(inputs)))!;
    expect(briefing.role).toBe("manager");
    expect(briefing.context).toContain("# Hangar projects at 14:05");
    expect(briefing.context).toContain("Shop");
    expect(briefing.context).toContain("Blog");
    expect(briefing.context).not.toContain("HQ —");
    expect(briefing.context).toContain('Tracker: none yet. Write it with memory_write, title "Projects — tracker", tag "manager"');
  });

  it("hands back the tracker the manager wrote", async () => {
    const inputs = managerSetup();
    const tracker = memory("Projects — tracker", "Shop: the person wants login first.", { tags: ["manager"] });
    const briefing = (await briefingFor("m", deps(inputs, [...inputs.memory, tracker])))!;
    expect(briefing.context).toContain('Tracker (memory "Projects — tracker"');
    expect(briefing.context).toContain("the person wants login first");
    expect(briefing.context).not.toContain("Tracker: none yet");
  });

  it("reads no board for itself, and leaves the orchestrator and the worker alone", async () => {
    const inputs = managerSetup();
    const boards: string[] = [];
    const watched = { ...deps(inputs), loadBoard: async (cwd: string) => (boards.push(cwd), inputs.tasks) };
    await briefingFor("m", watched);
    expect(boards).not.toContain("C:\dev\hq");
    expect((await briefingFor("o", watched))?.context).toContain("let people log in");
    expect((await briefingFor("w1", watched))?.role).toBe("worker");
    expect(buildBriefing(setup())?.system).toBe(buildBriefing({ ...setup(), projects: [] })?.system);
  });

  it("is told its role mid-session, and when it loses it", async () => {
    const briefing = (await briefingFor("m", deps(managerSetup())))!;
    const text = roleMessage(briefing, "HQ");
    expect(text.startsWith("Hangar has just pinned you as the person's project manager")).toBe(true);
    expect(text).toContain(briefing.context);
    expect(releaseMessage("manager", "HQ")).toContain("no longer the person's project manager");
  });
});
