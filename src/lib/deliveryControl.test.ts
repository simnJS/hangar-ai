import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Task, TaskPatch } from "./board";
import type { Caller, ControlDeps } from "./control";
import {
  deliveringIn,
  deliveryControl,
  deliveryState,
  landDelivery,
  resetDeliveries,
  type DeliveryDeps,
  type DeliveryOutcome,
  type DeliveryRequest,
} from "./deliveryControl";
import type { OrchestratorEvent } from "./orchestratorEvents";
import { DEFAULT_SETTINGS, type AppState, type Pane, type Workspace } from "../types";

const events: { workspaceId: string; event: OrchestratorEvent }[] = [];
vi.mock("./orchestratorEvents", () => ({
  postOrchestratorEvent: (workspaceId: string, event: OrchestratorEvent) => events.push({ workspaceId, event }),
}));

function pane(id: string, name: string, patch: Partial<Pane> = {}): Pane {
  return { id, name, agent: "claude", sessionId: null, cwd: null, shellId: null, title: null, ...patch };
}

function task(patch: Partial<Task> = {}): Task {
  return {
    id: "t1",
    title: "Add the export button",
    description: "A button that exports the board as CSV.",
    column: "doing",
    priority: 2,
    assignee: "Bo",
    labels: ["ui"],
    comments: [],
    depends_on: [],
    created_at: 0,
    updated_at: 0,
    order: 1,
    ...patch,
    ...({ owns: ["src/export"] } as object),
  };
}

function outcome(patch: Partial<DeliveryOutcome>): DeliveryOutcome {
  return {
    outcome: "landed",
    detail: "",
    files: [],
    inCheckout: false,
    changed: ["src/export/button.tsx"],
    outsideOwns: [],
    check: null,
    review: null,
    merge: "abcdef1234567890",
    base: "1234567abcdef",
    landedIn: "C:\\dev\\app",
    ...patch,
  };
}

const WORKER = pane("p2", "Bo", { role: "worker", taskId: "t1", cwd: "C:\\dev\\app-wt\\t1", portBase: 41020 });

let state: AppState;
let tasks: Task[];
let log: string[];
let runs: DeliveryRequest[];
let next: (request: DeliveryRequest) => Promise<DeliveryOutcome>;
let preflightError: string | null;
let deps: ControlDeps;

function workspace(): Workspace {
  return state.workspaces[0];
}

const as = (paneId: string | null): Caller => ({ paneId, paneName: null, cwd: "C:\\dev\\app" });

/** Until the delivery submitted in the background has had its follow-ups. */
async function settled(taskId = "t1") {
  await vi.waitFor(() => expect(deliveryState(taskId)?.state).not.toBe("running"));
}

beforeEach(() => {
  resetDeliveries();
  events.length = 0;
  log = [];
  runs = [];
  preflightError = null;
  tasks = [task()];
  next = async () => outcome({ outcome: "landed", detail: "worker landed on main (abcdef1)." });
  state = {
    workspaces: [
      {
        id: "w1",
        name: "App",
        cwd: "C:\\dev\\app",
        extraRoots: [],
        panes: [pane("p1", "Ava", { role: "orchestrator" }), WORKER, pane("p3", "Cy")],
        themeId: null,
        shellId: "pwsh",
        tree: null,
        savedCommands: [],
        folderId: null,
        orchestration: {
          maxWorkers: 3,
          maxAttempts: 2,
          autoMerge: true,
          checkCommand: "pnpm test",
          orchestratorModel: "opus",
          workerModel: "sonnet",
          reviewerModel: "sonnet",
          branch: null,
          permissionMode: null,
        },
      },
    ],
    folders: [],
    activeWorkspaceId: "w1",
    settings: DEFAULT_SETTINGS,
  };
  const delivery: DeliveryDeps = {
    preflight: async (args) => {
      log.push(`preflight:${args.worktree}:${args.target}:${args.landing}`);
      if (preflightError) throw preflightError;
      return { repo: "C:\\dev\\app", queue: "c:/dev/app/.git", branch: "worker/t1", target: "main", ahead: 2 };
    },
    run: (request) => {
      runs.push(request);
      return next(request);
    },
    cleanup: async (args) => {
      log.push(`cleanup:${args.worktree}:${args.branch}:${args.target}`);
      return { worktreeRemoved: true, branchDeleted: true, kept: [] };
    },
    running: async () => [],
    shells: () => [{ id: "pwsh", label: "PowerShell 7", program: "pwsh.exe", args: [] }],
    board: {
      load: async () => ({ tasks: tasks.map((entry) => ({ ...entry })) }),
      update: async (_cwd, id, patch: TaskPatch) => {
        log.push(`update:${id}:${JSON.stringify(patch)}`);
        tasks = tasks.map((entry) => (entry.id === id ? { ...entry, ...patch } as Task : entry));
        return tasks.find((entry) => entry.id === id)!;
      },
      comment: async (_cwd, id, author, text) => {
        log.push(`comment:${id}:${author}:${text}`);
        tasks = tasks.map((entry) =>
          entry.id === id
            ? { ...entry, comments: [...entry.comments, { id: "c", author, text, created_at: 0 }] }
            : entry,
        );
        return tasks.find((entry) => entry.id === id)!;
      },
    },
    tell: async (paneId, text) => log.push(`tell:${paneId}:${text}`),
  };
  deps = {
    snapshot: () => state,
    closePane: (workspaceId: string, paneId: string) => {
      log.push(`close:${workspaceId}:${paneId}`);
      state = {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === workspaceId ? { ...ws, panes: ws.panes.filter((entry) => entry.id !== paneId) } : ws,
        ),
      };
    },
    delivery,
  } as unknown as ControlDeps;
});

const submit = (args: Record<string, unknown> = {}, paneId: string | null = "p2") =>
  deliveryControl("delivery_submit", workspace(), args, as(paneId), deps);

describe("delivery_submit", () => {
  it("is only for a worker pane on a task", async () => {
    await expect(submit({}, "p1")).rejects.toThrow(/for worker panes.*worker_spawn/);
    await expect(submit({}, "p3")).rejects.toThrow(/for worker panes/);
    await expect(submit({}, null)).rejects.toThrow(/for worker panes/);
    expect(runs).toEqual([]);
  });

  it("refuses a task that is not being worked on, or already passed", async () => {
    tasks = [task({ column: "done" })];
    await expect(submit()).rejects.toThrow(/already done/);
    tasks = [task({ column: "review", labels: ["ready-to-land"] })];
    await expect(submit()).rejects.toThrow(/waits for the person to land it/);
    tasks = [task({ column: "todo" })];
    await expect(submit()).rejects.toThrow(/is in todo, not doing/);
    tasks = [];
    await expect(submit()).rejects.toThrow(/no longer on the board/);
    expect(runs).toEqual([]);
  });

  it("hands the worker git's refusal as it is", async () => {
    preflightError = "Your worktree has uncommitted changes: a.ts. Only commits are delivered.";
    await expect(submit()).rejects.toThrow("Your worktree has uncommitted changes: a.ts.");
    expect(runs).toEqual([]);
    expect(deliveryState("t1")).toBeNull();
  });

  it("answers at once, and runs the delivery with the workspace's settings", async () => {
    let finish: (value: DeliveryOutcome) => void = () => undefined;
    next = () => new Promise((resolve) => (finish = resolve));

    const answer = (await submit({ summary: "  Added the button.  " })) as Record<string, unknown>;

    expect(answer).toMatchObject({ submitted: true, branch: "worker/t1", target: "main", commits: 2 });
    expect(answer.next).toMatch(/runs `pnpm test` there.*message from Hangar.*End your turn/s);
    expect(log[0]).toBe("preflight:C:\\dev\\app-wt\\t1:null:false");
    expect(runs[0]).toMatchObject({
      workspace: "C:\\dev\\app",
      worktree: "C:\\dev\\app-wt\\t1",
      branch: "worker/t1",
      target: "main",
      task: { id: "t1", title: "Add the export button", owns: ["src/export"] },
      summary: "Added the button.",
      checkCommand: "pnpm test",
      reviewerModel: "sonnet",
      land: true,
      reuse: false,
      portBase: 41020,
      shell: { id: "pwsh" },
    });
    expect(deliveryState("t1")?.state).toBe("running");
    expect(deliveringIn("w1")).toBe(true);
    await expect(submit()).rejects.toThrow(/already being merged, checked and reviewed/);
    expect(runs).toHaveLength(1);

    finish(outcome({ outcome: "landed", detail: "landed" }));
    await settled();
    expect(deliveryState("t1")?.state).toBe("landed");
    expect(deliveringIn("w1")).toBe(false);
  });

  it("leaves a landing to the person when automatic landing is off", async () => {
    state.workspaces[0].orchestration!.autoMerge = false;
    await submit();
    expect(runs[0].land).toBe(false);
  });
});

describe("what follows an outcome", () => {
  it("a landed delivery finishes the task and lets the worker go", async () => {
    tasks = [task({ labels: ["ui", "needs-you"] })];
    next = async () =>
      outcome({
        outcome: "landed",
        detail: "worker/t1 landed on main (abcdef1).",
        check: { command: "pnpm test", passed: true, exitCode: 0, timedOut: false, seconds: 42, tail: "ok" },
        review: { approve: true, blocking: [], notes: "Clean." },
        outsideOwns: ["src/i18n.ts"],
      });

    await submit({ summary: "Added the button." });
    await settled();

    expect(tasks[0]).toMatchObject({ column: "done", labels: ["ui"] });
    expect(log).toContain("close:w1:p2");
    expect(log).toContain("cleanup:C:\\dev\\app-wt\\t1:worker/t1:main");
    const comment = log.find((line) => line.startsWith("comment:"))!;
    expect(comment).toContain("comment:t1:hangar:Landed on main (abcdef1), fast-forwarded in C:\\dev\\app.");
    expect(comment).toContain("Summary: Added the button.");
    expect(comment).toContain("Check: `pnpm test` passed in 42 s.");
    expect(comment).toContain("Review (sonnet): approved. Notes: Clean.");
    expect(comment).toContain("Changed outside the files it owns, accepted by the review: src/i18n.ts.");
    expect(log.some((line) => line.startsWith("tell:"))).toBe(false);
    expect(events).toEqual([
      {
        workspaceId: "w1",
        event: {
          kind: "delivery",
          taskId: "t1",
          title: "Add the export button",
          worker: "Bo",
          outcome: "landed",
          detail: "landed on main (abcdef1)",
        },
      },
    ]);
  });

  it("a failure goes back to the worker with what to do, and the task stays its own", async () => {
    next = async () =>
      outcome({
        outcome: "conflict",
        detail:
          "Merging worker/t1 onto the current main conflicts in src/a.ts, src/b.ts. Merge main into your branch " +
          "(`git merge main`), resolve the conflicts, commit, then call delivery_submit again.",
        files: ["src/a.ts", "src/b.ts"],
      });

    await submit();
    await settled();

    expect(tasks[0].column).toBe("doing");
    expect(log.some((line) => line.startsWith("close:"))).toBe(false);
    expect(log).toContain(
      "comment:t1:hangar:Delivery failed (conflict, 1 of 3): Merging worker/t1 onto the current main conflicts in " +
        "src/a.ts, src/b.ts. Merge main into your branch (`git merge main`), resolve the conflicts, commit, then " +
        "call delivery_submit again.",
    );
    const told = log.find((line) => line.startsWith("tell:p2:"))!;
    expect(told).toContain('Your delivery of "Add the export button" did not land.');
    expect(told).toContain("conflicts in src/a.ts, src/b.ts");
    expect(told).toContain("Failed deliveries so far: 1 of 3.");
    expect(events.map(({ event }) => event.kind)).toEqual(["delivery"]);
    expect(events[0].event).toMatchObject({
      outcome: "conflict",
      detail: "conflict with main in src/a.ts, src/b.ts (failed delivery 1 of 3)",
    });
  });

  it("the third failure is the person's", async () => {
    const earlier = (n: number) => ({ id: `c${n}`, author: "hangar", text: `Delivery failed (checkFailed, ${n} of 3): x`, created_at: 0 });
    tasks = [task({ comments: [earlier(1), earlier(2)] })];
    next = async () => outcome({ outcome: "checkFailed", detail: "The check `pnpm test` failed (exit 1) on worker/t1 merged onto main." });

    await submit();
    await settled();

    expect(tasks[0]).toMatchObject({ column: "doing", labels: ["ui", "needs-you"] });
    expect(log.find((line) => line.startsWith("tell:p2:"))).toContain("stop working on it now");
    expect(events.map(({ event }) => event.kind)).toEqual(["delivery", "taskNeedsYou"]);
    expect(events[1].event).toMatchObject({
      kind: "taskNeedsYou",
      taskId: "t1",
      reason: "3 deliveries failed, the last one: The check `pnpm test` failed (exit 1) on worker/t1 merged onto main.",
    });
  });

  it("tells the orchestrator the fact it acts on, not the worker's instructions", async () => {
    next = async () =>
      outcome({
        outcome: "checkFailed",
        detail: "The check `pnpm test` failed (exit 1) on worker/t1 merged onto main. It runs in a fresh checkout.",
        check: {
          command: "pnpm test",
          passed: false,
          exitCode: 1,
          timedOut: false,
          seconds: 12,
          tail:
            " RUN  v5\n FAIL  src/export.test.ts > exports a row per task\nAssertionError: expected 2 to be 3\n Tests  1 failed",
        },
      });
    await submit();
    await settled();
    expect(events[0].event).toMatchObject({
      detail: "`pnpm test` exited 1: FAIL  src/export.test.ts > exports a row per task (failed delivery 1 of 3)",
    });

    events.length = 0;
    next = async () =>
      outcome({
        outcome: "changesRequested",
        detail: "The reviewer asked for changes before worker/t1 lands:",
        review: {
          approve: false,
          blocking: [
            { file: "src/export/csv.ts", line: 14, problem: "Commas inside a title are not quoted." },
            { file: "src/export/button.tsx", problem: "Nothing happens on an empty board." },
          ],
          notes: "",
        },
      });
    await submit();
    await settled();
    expect(events[0].event).toMatchObject({
      detail: "review: src/export/csv.ts:14: Commas inside a title are not quoted. (+1 more) (failed delivery 2 of 3)",
    });
  });

  it("an error from the app itself is a failed delivery too", async () => {
    next = async () => {
      throw "the window went away";
    };

    await submit();
    await settled();

    expect(deliveryState("t1")?.state).toBe("error");
    expect(log.find((line) => line.startsWith("tell:p2:"))).toContain(
      "The delivery could not run: the window went away. Nothing was merged.",
    );
    expect(log.find((line) => line.startsWith("tell:p2:"))).toContain("If it fails again the same way, stop");
  });

  it("one that passed waits for the person, and lands when they say so", async () => {
    state.workspaces[0].orchestration!.autoMerge = false;
    next = async () =>
      outcome({
        outcome: "ready",
        detail: "worker/t1 passed its check and its review. Automatic landing is off in this workspace, so it waits.",
        landedIn: null,
      });

    await submit();
    await settled();

    expect(tasks[0]).toMatchObject({ column: "review", labels: ["ui", "ready-to-land"] });
    expect(log.find((line) => line.startsWith("tell:p2:"))).toContain("Automatic landing is off");
    expect(events[0].event).toMatchObject({ outcome: "ready", detail: "ready to land on main" });
    expect(log.some((line) => line.startsWith("close:"))).toBe(false);

    next = async () => outcome({ outcome: "landed", detail: "landed" });
    await landDelivery(workspace(), "t1", deps);

    expect(runs[1]).toMatchObject({ land: true, reuse: true });
    expect(log).toContain("preflight:C:\\dev\\app-wt\\t1:null:true");
    expect(tasks[0]).toMatchObject({ column: "done", labels: ["ui"] });
    expect(log).toContain("close:w1:p2");
    expect(tasks[0].comments[tasks[0].comments.length - 1].text).toContain(
      "Landed as it was checked and reviewed when it was submitted.",
    );
  });

  it("the person's local changes in the way make it wait for them, not the worker", async () => {
    next = async () =>
      outcome({
        outcome: "dirty",
        inCheckout: true,
        files: ["src/a.ts"],
        detail: "worker/t1 passed its review, but main is checked out in C:\\dev\\app, which has local changes.",
        landedIn: null,
      });

    await submit();
    await settled();

    expect(tasks[0]).toMatchObject({ column: "review", labels: ["ui", "ready-to-land"] });
    expect(log.find((line) => line.startsWith("comment:"))).toContain("local changes in the way (src/a.ts)");
    expect(log.some((line) => line.startsWith("comment:t1:hangar:Delivery failed"))).toBe(false);
  });

  it("landing what waited says why when it no longer can, and the worker gets it back", async () => {
    tasks = [task({ column: "review", labels: ["ui", "ready-to-land"] })];
    next = async () =>
      outcome({
        outcome: "conflict",
        files: ["src/a.ts"],
        detail: "Merging worker/t1 onto the current main conflicts in src/a.ts.",
      });

    await expect(landDelivery(workspace(), "t1", deps)).rejects.toThrow(
      "It no longer lands as it was tested (conflict with main in src/a.ts), so Bo was told what to fix.",
    );

    expect(tasks[0]).toMatchObject({ column: "doing", labels: ["ui"] });
    expect(log.find((line) => line.startsWith("tell:p2:"))).toContain("conflicts in src/a.ts");
  });

  it("landing tells the person when their own changes are in the way", async () => {
    tasks = [task({ column: "review", labels: ["ui", "ready-to-land"] })];
    next = async () =>
      outcome({ outcome: "dirty", inCheckout: true, files: ["src/a.ts"], detail: "passed, but…", landedIn: null });

    await expect(landDelivery(workspace(), "t1", deps)).rejects.toThrow(
      "Local changes in main's checkout are in the way (src/a.ts): commit them or set them aside, then land it " +
        "again. Nothing was touched.",
    );
    expect(tasks[0]).toMatchObject({ column: "review", labels: ["ui", "ready-to-land"] });
  });

  it("landing needs the worker that holds the branch", async () => {
    state.workspaces[0].panes = state.workspaces[0].panes.filter((entry) => entry.id !== "p2");
    await expect(landDelivery(workspace(), "t1", deps)).rejects.toThrow(/No worker holds/);
    expect(runs).toEqual([]);
  });
});

describe("delivery_status", () => {
  it("tells a worker where its own delivery stands", async () => {
    let finish: (value: DeliveryOutcome) => void = () => undefined;
    next = () => new Promise((resolve) => (finish = resolve));
    (deps.delivery as DeliveryDeps).running = async () => [
      { taskId: "t1", queue: "c:/dev/app/.git", stage: "reviewing", since: 1_700_000_000_000 },
    ];
    await submit();

    const status = (await deliveryControl("delivery_status", workspace(), {}, as("p2"), deps)) as {
      deliveries: Record<string, unknown>[];
    };
    expect(status.deliveries).toEqual([
      { worker: "Bo", task: "t1", state: "reviewing", since: new Date(1_700_000_000_000).toISOString() },
    ]);

    finish(outcome({ outcome: "changesRequested", detail: "The reviewer asked for changes before worker/t1 lands:" }));
    await settled();
    (deps.delivery as DeliveryDeps).running = async () => [];
    const after = (await deliveryControl("delivery_status", workspace(), {}, as("p1"), deps)) as {
      deliveries: Record<string, unknown>[];
    };
    expect(after.deliveries).toEqual([
      expect.objectContaining({ worker: "Bo", state: "changesRequested", detail: "The reviewer asked for changes before worker/t1 lands:" }),
    ]);
  });
});
