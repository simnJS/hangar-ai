import { describe, expect, it } from "vitest";
import type { AgentActivity } from "./agentState";
import type { MemoryEntry } from "./memory";
import {
  ORCHESTRATION_DEFAULTS,
  hqOf,
  managerOf,
  orchestrationOf,
  orchestratorOf,
  workersOf,
} from "./orchestration";
import { PLAN_TAG, buildBriefing } from "./orchestratorBrief";
import type { BoardTask, JournalEntry } from "./orchestratorEvents";
import {
  loadProjects,
  planStatusOf,
  renderOverview,
  renderProject,
  type ProjectSummary,
  type ProjectsDeps,
} from "./projects";
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

const NOW = new Date(2026, 9, 28, 18, 30).getTime();

function plan(title: string, content: string, extra: Partial<MemoryEntry> = {}): MemoryEntry {
  return {
    id: `m-${title}`,
    title,
    content,
    tags: [PLAN_TAG],
    workspace: "C:\\dev\\shop",
    agent: "orch",
    created_at: NOW - 3_600_000,
    updated_at: NOW - 60_000,
    ...extra,
  };
}

const hq = () => workspace("hq", "HQ", [pane("m", "boss", "manager")], { hq: true });

/** Shop has an orchestration at work; Blog has agents and a board but no orchestrator; Wiki sleeps. */
function setup() {
  const shop = workspace("shop", "Shop", [
    pane("o", "orch", "orchestrator"),
    pane("w1", "worker-1", "worker", { taskId: "t1" }),
    pane("w2", "worker-2", "worker", { taskId: "t2" }),
    pane("p", "notes"),
    pane("sh", "term", undefined, { agent: "shell" }),
  ]);
  const blog = workspace("blog", "Blog", [pane("b1", "writer"), pane("b2", "cx", undefined, { agent: "codex" })]);
  const wiki = workspace("wiki", "Wiki", [pane("k", "wiki-agent")]);
  const state: AppState = {
    workspaces: [hq(), shop, blog, wiki],
    folders: [],
    activeWorkspaceId: "shop",
    settings: DEFAULT_SETTINGS,
  };
  const boards: Record<string, BoardTask[]> = {
    "C:\\dev\\shop": [
      task("t1", { title: "Login form", column: "doing", assignee: "worker-1", order: 1 }),
      task("t2", { title: "Payment page", column: "doing", assignee: "worker-2", priority: 2, order: 2 }),
      task("t3", { title: "Signup", column: "review", labels: ["ready-to-land"] }),
      task("t4", { title: "Pick a mailer", labels: ["needs-you"], order: 1 }),
      task("t5", { title: "Pick a payment provider", labels: ["needs-you"], priority: 3, order: 2 }),
      task("t6", { title: "Old question", column: "done", labels: ["needs-you", "ready-to-land"] }),
      task("t0", { title: "Scaffold", column: "done" }),
    ],
    "C:\\dev\\blog": [
      task("b-1", { title: "Write about Rust" }),
      task("b-2", { title: "Draft the intro", column: "doing", assignee: "writer" }),
      task("b-3", { title: "Pick a theme", column: "done" }),
    ],
    "C:\\dev\\wiki": [task("k-1", { title: "Import pages", column: "done" })],
  };
  const activities: Record<string, AgentActivity> = {
    o: "working",
    w1: "working",
    w2: "waiting",
    p: "waiting",
    sh: "waiting",
    b1: "idle",
    b2: "waiting",
    m: "waiting",
  };
  const journal: JournalEntry[] = Array.from({ length: 7 }, (_, n) => ({
    at: NOW - (7 - n) * 60_000,
    quiet: false,
    event: { kind: "workerStarted", worker: `worker-${n}`, taskId: "t1" },
  }));
  const reads: string[] = [];
  const deps: ProjectsDeps = {
    snapshot: () => state,
    loadBoard: async (cwd) => {
      reads.push(cwd);
      return boards[cwd] ?? [];
    },
    loadMemory: async () => [
      plan("Plan — Shop", "Objective: sell things.\nTasks:\n- t1 doing\nStatus: T1 and T2 running; T3 waits to land."),
    ],
    isRepo: async (cwd) => cwd !== "C:\\dev\\blog",
    activity: (id) => (activities[id] ? { workspaceId: "?", activity: activities[id], bridged: true } : undefined),
    asleep: (id) => id === "wiki",
    isOpen: (id) => id === "shop" || id === "blog",
    journal: (id) => (id === "shop" ? journal : []),
  };
  return { state, boards, deps, reads, shop, blog, wiki };
}

const byName = (projects: ProjectSummary[], name: string) => {
  const found = projects.find((project) => project.name === name);
  if (!found) throw new Error(`no project ${name}`);
  return found;
};

describe("the manager role", () => {
  it("is neither an orchestrator nor a worker, and has no briefing yet", () => {
    const { state } = setup();
    const home = state.workspaces[0];
    expect(orchestratorOf(home)).toBeNull();
    expect(workersOf(home)).toEqual([]);
    expect(orchestrationOf(home)).toEqual(ORCHESTRATION_DEFAULTS);
    // A manager pane listed first does not hide a workspace's orchestrator.
    const mixed = workspace("x", "X", [pane("m2", "boss", "manager"), pane("o2", "orch", "orchestrator")]);
    expect(orchestratorOf(mixed)?.id).toBe("o2");
    expect(workersOf(mixed)).toEqual([]);
    expect(
      buildBriefing({ state, paneId: "m", tasks: [], memory: [], journal: [], activity: () => undefined, now: NOW }),
    ).toBeNull();
  });

  it("is found across the workspaces, beside the HQ", () => {
    const { state, shop } = setup();
    expect(managerOf(state)).toEqual({ ws: state.workspaces[0], pane: state.workspaces[0].panes[0] });
    expect(hqOf(state)?.id).toBe("hq");
    expect(managerOf({ workspaces: [shop] })).toBeNull();
    expect(hqOf({ workspaces: [shop] })).toBeNull();
  });
});

describe("loadProjects", () => {
  it("leaves the HQ out, and reads nothing of it", async () => {
    const { deps, reads } = setup();
    const projects = await loadProjects(deps);
    expect(projects.map((project) => project.name)).toEqual(["Shop", "Blog", "Wiki"]);
    expect(reads).not.toContain("C:\\dev\\hq");
  });

  it("summarizes a workspace without an orchestrator from its panes and board", async () => {
    const { deps } = setup();
    const blog = byName(await loadProjects(deps), "Blog");
    expect(blog).toMatchObject({
      id: "blog",
      cwd: "C:\\dev\\blog",
      open: true,
      asleep: false,
      git: false,
      orchestrator: null,
      workers: [],
      waiting: [{ name: "cx", agent: "codex" }],
      board: { todo: 1, doing: 1, review: 0, done: 1 },
      boardError: null,
      doing: [{ id: "b-2", title: "Draft the intro", assignee: "writer" }],
      needsYou: [],
      readyToLand: [],
      allDone: false,
      planStatus: null,
      lastEvents: [],
    });
  });

  it("gives the orchestrator, the workers and what they are doing", async () => {
    const { deps } = setup();
    const shop = byName(await loadProjects(deps), "Shop");
    expect(shop.git).toBe(true);
    expect(shop.orchestrator).toEqual({ name: "orch", activity: "working" });
    expect(shop.workers).toEqual([
      { name: "worker-1", taskId: "t1", activity: "working" },
      { name: "worker-2", taskId: "t2", activity: "waiting" },
    ]);
    // Higher priority first.
    expect(shop.doing.map((entry) => entry.id)).toEqual(["t2", "t1"]);
    expect(shop.doing[0]).toEqual({ id: "t2", title: "Payment page", assignee: "worker-2" });
  });

  it("lists every agent pane waiting for the person, workers or not, and no shell", async () => {
    const { deps } = setup();
    const shop = byName(await loadProjects(deps), "Shop");
    expect(shop.waiting).toEqual([
      { name: "worker-2", agent: "claude" },
      { name: "notes", agent: "claude" },
    ]);
  });

  it("counts the board's columns", async () => {
    const { deps } = setup();
    const shop = byName(await loadProjects(deps), "Shop");
    expect(shop.board).toEqual({ todo: 2, doing: 2, review: 1, done: 2 });
  });

  it("lists the open tasks that need the person and those ready to land", async () => {
    const { deps } = setup();
    const shop = byName(await loadProjects(deps), "Shop");
    // The done one waits for nobody, whatever its labels say.
    expect(shop.needsYou).toEqual([
      { id: "t5", title: "Pick a payment provider" },
      { id: "t4", title: "Pick a mailer" },
    ]);
    expect(shop.readyToLand).toEqual([{ id: "t3", title: "Signup" }]);
  });

  it("is all done only for a board with tasks, every one of them done", async () => {
    const { deps, boards } = setup();
    expect(byName(await loadProjects(deps), "Wiki").allDone).toBe(true);
    expect(byName(await loadProjects(deps), "Shop").allDone).toBe(false);
    boards["C:\\dev\\wiki"] = [];
    expect(byName(await loadProjects(deps), "Wiki").allDone).toBe(false);
  });

  it("takes the status line of the orchestrator's plan", async () => {
    const { deps, state } = setup();
    expect(byName(await loadProjects(deps), "Shop").planStatus).toBe("T1 and T2 running; T3 waits to land.");
    // Renamed since: the plan is still found from its folder.
    state.workspaces[1] = { ...state.workspaces[1], name: "Store" };
    expect(byName(await loadProjects(deps), "Store").planStatus).toBe("T1 and T2 running; T3 waits to land.");
    // No status written: none, rather than something else of the plan.
    const bare = { ...deps, loadMemory: async () => [plan("Plan — Store", "Objective: sell things.")] };
    expect(byName(await loadProjects(bare), "Store").planStatus).toBeNull();
  });

  it("tells a workspace asleep, and one not open", async () => {
    const { deps } = setup();
    const wiki = byName(await loadProjects(deps), "Wiki");
    expect(wiki.asleep).toBe(true);
    expect(wiki.open).toBe(false);
    expect(renderOverview([wiki], NOW)).toContain("Wiki — C:\\dev\\wiki · asleep");
  });

  it("gives an unreadable board an empty part, and still summarizes everything else", async () => {
    const { deps } = setup();
    const broken: ProjectsDeps = {
      ...deps,
      loadBoard: (cwd) => {
        if (cwd === "C:\\dev\\wiki") throw new Error("locked");
        return cwd === "C:\\dev\\blog" ? Promise.reject(new Error("board.json is not JSON")) : deps.loadBoard(cwd);
      },
    };
    const projects = await loadProjects(broken);
    const blog = byName(projects, "Blog");
    expect(blog.boardError).toBe("board.json is not JSON");
    expect(blog.board).toEqual({ todo: 0, doing: 0, review: 0, done: 0 });
    expect(blog.doing).toEqual([]);
    expect(blog.allDone).toBe(false);
    expect(blog.waiting).toEqual([{ name: "cx", agent: "codex" }]);
    expect(byName(projects, "Wiki").boardError).toBe("locked");
    expect(byName(projects, "Shop").board.doing).toBe(2);
    expect(renderProject(blog)).toContain("Board: could not be read (board.json is not JSON).");
  });

  it("does without the memory and the repository when they cannot be read", async () => {
    const { deps } = setup();
    const projects = await loadProjects({
      ...deps,
      loadMemory: () => Promise.reject(new Error("memory locked")),
      isRepo: () => Promise.reject(new Error("no git")),
    });
    expect(projects).toHaveLength(3);
    expect(projects.every((project) => project.planStatus === null && project.git === null)).toBe(true);
  });

  it("reads a folder two workspaces share once", async () => {
    const { deps, state, reads } = setup();
    state.workspaces.push(workspace("shop2", "Shop (old)", [], { cwd: "c:/dev/shop/" }));
    const projects = await loadProjects(deps);
    expect(reads.filter((cwd) => cwd.toLowerCase().includes("shop"))).toHaveLength(1);
    expect(byName(projects, "Shop (old)").board).toEqual(byName(projects, "Shop").board);
  });

  it("keeps the latest few events, worded with the board's titles", async () => {
    const { deps } = setup();
    const shop = byName(await loadProjects(deps), "Shop");
    expect(shop.lastEvents).toHaveLength(5);
    expect(shop.lastEvents[0].text).toBe('worker-2 started on "Login form" [t1]');
    expect(shop.lastEvents[4].text).toBe('worker-6 started on "Login form" [t1]');
  });
});

describe("planStatusOf", () => {
  it("reads the status on its line, or under it", () => {
    expect(planStatusOf("Objective: x\nStatus: T2 running, T3 next.\n")).toBe("T2 running, T3 next.");
    expect(planStatusOf("  Status: indented as the role shows it")).toBe("indented as the role shows it");
    expect(planStatusOf("**Status:** all landed")).toBe("all landed");
    expect(planStatusOf("- Status: in a list")).toBe("in a list");
    expect(planStatusOf("Status:\n- T2 running\n- T3 next\n\nNotes")).toBe("- T2 running / - T3 next");
    expect(planStatusOf("## Status\nAll landed.\n## Notes")).toBe("All landed.");
    expect(planStatusOf("Status:\nTasks:\n- t1")).toBeNull();
    expect(planStatusOf("Objective: the status quo")).toBeNull();
    expect(planStatusOf(`Status: ${"x".repeat(400)}`)?.length).toBe(300);
  });
});

/* ── Texts ─────────────────────────────────────────────────────── */

function summary(n: number, patch: Partial<ProjectSummary> = {}): ProjectSummary {
  return {
    id: `p${n}`,
    name: `Project ${n}`,
    cwd: `C:\\dev\\project-${n}`,
    open: true,
    asleep: false,
    git: true,
    orchestrator: null,
    workers: [],
    waiting: [],
    board: { todo: 0, doing: 0, review: 0, done: 0 },
    boardError: null,
    needsYou: [],
    readyToLand: [],
    doing: [],
    allDone: false,
    planStatus: null,
    lastEvents: [],
    ...patch,
  };
}

const uuid = (n: number, k: number) => `${String(n).padStart(8, "0")}-1ce9-461d-9521-${String(k).padStart(12, "0")}`;

/** A project with all an overview can say about it, at length. */
function busy(n: number): ProjectSummary {
  const tasks = (k: number, what: string) =>
    Array.from({ length: k }, (_, i) => ({ id: uuid(n, i), title: `${what} ${i} `.repeat(12) }));
  return summary(n, {
    orchestrator: { name: "orch", activity: "working" },
    workers: [
      { name: "worker-1", taskId: uuid(n, 0), activity: "working" },
      { name: "worker-2", taskId: uuid(n, 1), activity: "waiting" },
      { name: "worker-3", taskId: null, activity: null },
    ],
    waiting: Array.from({ length: 5 }, (_, i) => ({ name: `pane-with-a-long-name-${i}`, agent: "claude" as const })),
    board: { todo: 12, doing: 3, review: 2, done: 30 },
    needsYou: tasks(5, "Decide something"),
    readyToLand: tasks(4, "Land something"),
    doing: tasks(3, "Do something").map((entry) => ({ ...entry, assignee: "worker-1" })),
    planStatus: "Status words go on and on. ".repeat(20),
    lastEvents: [{ at: NOW, text: "Something happened. ".repeat(30) }],
  });
}

/** A project as most are: an orchestrator, one worker, one question, a status. */
function typical(n: number): ProjectSummary {
  return summary(n, {
    orchestrator: { name: "orch", activity: "idle" },
    workers: [{ name: "worker-1", taskId: uuid(n, 1), activity: "working" }],
    board: { todo: 3, doing: 1, review: 0, done: 5 },
    needsYou: [{ id: uuid(n, 2), title: "Choose between the two layouts for the settings page" }],
    doing: [{ id: uuid(n, 1), title: "Add CSV export to the orders page", assignee: "worker-1" }],
    planStatus: "T2 runs; T3 and T4 start once it lands, then the release notes.",
  });
}

describe("renderOverview", () => {
  it("says each project in a few lines, the ones with something for the person first", () => {
    const text = renderOverview(
      [
        summary(1, { name: "Quiet", git: false, open: false }),
        summary(2, {
          name: "Shop",
          orchestrator: { name: "orch", activity: "working" },
          workers: [
            { name: "worker-1", taskId: "t1", activity: "working" },
            { name: "worker-2", taskId: "t2", activity: "waiting" },
          ],
          waiting: [{ name: "worker-2", agent: "claude" }],
          board: { todo: 2, doing: 2, review: 1, done: 2 },
          needsYou: [{ id: "t5", title: "Pick a payment provider" }],
          readyToLand: [{ id: "t3", title: "Signup" }],
          planStatus: "T1 and T2 running.",
        }),
        summary(3, { name: "Done", board: { todo: 0, doing: 0, review: 0, done: 4 }, allDone: true }),
      ],
      NOW,
    );
    expect(text).toBe(
      [
        "# Hangar projects at 18:30",
        "3 projects: 1 with something for the person (Shop), 0 at work, 2 quiet.",
        "",
        "Shop — C:\\dev\\project-2 · open · git repository",
        '  Orchestrator "orch" working · 2 workers: 1 working, 1 waiting for the person · board: 2 todo, 2 doing, 1 review, 2 done.',
        '  Needs you: "Pick a payment provider" [t5].',
        '  Ready to land: "Signup" [t3].',
        '  Waiting for the person: "worker-2" (claude).',
        "  Plan status: T1 and T2 running.",
        "",
        "Quiet — C:\\dev\\project-1 · not open · no git repository · no orchestrator · board: empty.",
        "",
        "Done — C:\\dev\\project-3 · open · git repository · no orchestrator · board: all 4 tasks done.",
      ].join("\n"),
    );
  });

  it("fits twenty typical projects whole", () => {
    const text = renderOverview(Array.from({ length: 20 }, (_, n) => typical(n)), NOW);
    expect(text.length).toBeLessThanOrEqual(6_000);
    expect(text).not.toContain("not shown");
    for (let n = 0; n < 20; n += 1) expect(text).toContain(`Project ${n} — `);
  });

  it("stays under its cap with many busy projects, and says how many it left out", () => {
    const projects = [...Array.from({ length: 40 }, (_, n) => busy(n)), summary(99, { name: "Idle one" })];
    const text = renderOverview(projects, NOW);
    expect(text.length).toBeLessThanOrEqual(6_000);
    expect(text).toContain("Shortest form, for room: tasks counted rather than named, no plan status.");
    expect(text).toContain("For the person: 5 tasks needing you, 4 tasks ready to land, 5 panes waiting.");
    expect(text).not.toContain("Plan status");
    const left = /…and (\d+) more projects not shown, (\d+) of them with something for the person: (.*)\.$/.exec(text);
    expect(left).not.toBeNull();
    const shown = (text.match(/^Project \d+ — /gm) ?? []).length;
    expect(shown).toBeGreaterThan(0);
    expect(shown + Number(left![1])).toBe(projects.length);
    // The quiet one is left out before any busy one.
    expect(Number(left![2])).toBe(Number(left![1]) - 1);
    expect(text).not.toContain("Idle one — ");
    expect(left![3]).toMatch(/^Project \d+, /);
  });

  it("names fewer tasks before it leaves a project out", () => {
    const full = renderOverview(Array.from({ length: 12 }, (_, n) => typical(n)), NOW);
    expect(full).toContain("Plan status: T2 runs");
    expect(full).not.toContain("form, for room");
    const short = renderOverview(Array.from({ length: 20 }, (_, n) => typical(n)), NOW);
    expect(short.length).toBeLessThanOrEqual(6_000);
    expect(short).toContain("Short form, for room");
    expect(short).not.toContain("not shown");
    const counted = renderOverview(Array.from({ length: 28 }, (_, n) => typical(n)), NOW);
    expect(counted.length).toBeLessThanOrEqual(6_000);
    expect(counted).toContain("For the person: 1 task needing you.");
    expect(counted).not.toContain("not shown");
  });

  it("says so when there is no project", () => {
    expect(renderOverview([], NOW)).toBe("# Hangar projects at 18:30\nNo project: Hangar has no workspace besides the HQ.");
  });
});

describe("renderProject", () => {
  it("gives one project in detail", async () => {
    const { deps } = setup();
    const shop = byName(await loadProjects(deps), "Shop");
    const text = renderProject(shop);
    for (const needed of [
      "Shop — C:\\dev\\shop · open · git repository",
      'Orchestrator "orch" working.',
      'Workers (2):\n- "worker-1" on "Login form" [t1]: working\n- "worker-2" on "Payment page" [t2]: waiting for the person',
      "Board: 2 todo, 2 doing, 1 review, 2 done.",
      'Doing:\n- "Payment page" [t2] · worker-2\n- "Login form" [t1] · worker-1',
      'Needs you:\n- "Pick a payment provider" [t5]\n- "Pick a mailer" [t4]',
      'Ready to land:\n- "Signup" [t3]',
      'Waiting for the person: "worker-2" (claude), "notes" (claude).',
      "Plan status: T1 and T2 running; T3 waits to land.",
      'Latest events, oldest first:\n- 18:25 worker-2 started on "Login form" [t1]',
    ]) {
      expect(text).toContain(needed);
    }
  });

  it("says what a quiet project lacks", async () => {
    const { deps } = setup();
    const text = renderProject(byName(await loadProjects(deps), "Wiki"));
    expect(text).toContain("No orchestrator.");
    expect(text).toContain("Workers: none running.");
    expect(text).toContain("Board: all 1 task done.");
    expect(text).toContain("No pane waits for the person.");
    expect(text).toContain("Plan status: none found.");
    expect(text).not.toContain("Latest events");
  });
});
