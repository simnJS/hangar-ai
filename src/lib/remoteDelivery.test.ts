import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Task, TaskComment, TaskPatch } from "./board";
import type { ControlDeps } from "./control";
import {
  deliveryState,
  resetDeliveries,
  type DeliveryDeps,
  type DeliveryOutcome,
  type RunningDelivery,
  type TestedDelivery,
} from "./deliveryControl";
import { isDelivering } from "./deliveryRuns";
import type { ChangedFile, Changes, FileDiff } from "./git";
import { runRemote, statsSources } from "./remoteControl";
import {
  capPatch,
  deliveryHistory,
  PATCH_MAX_BYTES,
  resetLandings,
  runDelivery,
  type DeliverySources,
  type TellWorker,
} from "./remoteDelivery";
import { activityHistory } from "./activityLog";
import type { OrchestratorEvent } from "./orchestratorEvents";
import type { RemoteDelivery, RemoteDeliveryPatch, RemoteLanded } from "../remote/types";
import { DEFAULT_SETTINGS, type AppState, type Pane } from "../types";

vi.mock("./orchestratorEvents", () => ({
  postOrchestratorEvent: (_workspaceId: string, _event: OrchestratorEvent) => undefined,
}));

const comment = (id: string, author: string, text: string, created_at = 1): TaskComment => ({ id, author, text, created_at });

/** What deliveryControl writes on the board as a delivery fails, then passes. */
const FAILED_CHECK = comment(
  "c1",
  "hangar",
  "Delivery failed (checkFailed, 1 of 3): The check `pnpm test` failed (exit 1) on hangar/qr merged onto main. It " +
    "runs in a fresh checkout of that merge.\n```\nFAIL src/qr.test.ts\n```\nFix it on your branch, then call " +
    "delivery_submit again.",
  1_000,
);
const CHANGES_REQUESTED = comment(
  "c2",
  "hangar",
  "Delivery failed (changesRequested, 2 of 3): The reviewer asked for changes before hangar/qr lands:\n" +
    "- src/qr.ts:12: the code is drawn before it is checked\n- src/i18n.ts: a French string is missing\n" +
    "Notes: Close.\nFix these on your branch, commit, then call delivery_submit again.",
  2_000,
);
const PASSED = comment(
  "c3",
  "hangar",
  "Passed: waits for the person to land it on main (tested as abc1234).\n" +
    "Summary: Settings › Mobile pairs a phone by QR code.\nThe code is single use.\n" +
    "Check: `pnpm test` passed in 84 s.\n" +
    "Review (sonnet): approved. Notes: Clean split.\nMinor: memoize the canvas.\n" +
    "Changed outside the files it owns, accepted by the review: src/i18n.ts.",
  3_000,
);

function task(patch: Partial<Task> = {}): Task {
  return {
    id: "t1",
    title: "Pair a phone by QR code",
    description: "",
    column: "review",
    priority: 1,
    assignee: "Dana",
    labels: ["ui", "ready-to-land"],
    comments: [FAILED_CHECK, comment("c-you", "user", "Looks close."), CHANGES_REQUESTED, PASSED],
    depends_on: [],
    created_at: 0,
    updated_at: 0,
    order: 1,
    ...patch,
  };
}

function outcome(patch: Partial<DeliveryOutcome>): DeliveryOutcome {
  return {
    outcome: "landed",
    detail: "",
    files: [],
    inCheckout: false,
    changed: [],
    outsideOwns: [],
    check: null,
    review: null,
    merge: "abc1234def567",
    base: "0123456789ab",
    landedIn: "C:\\dev\\app",
    ...patch,
  };
}

function changed(path: string, status: string, additions: number | null, deletions: number | null, patch: Partial<ChangedFile> = {}): ChangedFile {
  return { path, oldPath: null, status, additions, deletions, binary: additions === null, ...patch };
}

const WORKER: Pane = {
  id: "p-dana",
  name: "Dana",
  agent: "claude",
  sessionId: "s1",
  cwd: "C:\\dev\\app-wt\\qr",
  shellId: null,
  title: null,
  role: "worker",
  taskId: "t1",
};

let state: AppState;
let tasks: Task[];
let log: string[];
let next: () => Promise<DeliveryOutcome>;
let tested: TestedDelivery | null;
let running: RunningDelivery[];
let changes: Changes;
let diffs: Record<string, FileDiff>;
let deps: ControlDeps;
let sources: DeliverySources;
let told: string[];
const tell: TellWorker = async (ws, pane, text) => void told.push(`${ws.id}:${pane.id}:${text}`);

beforeEach(() => {
  resetDeliveries();
  resetLandings();
  log = [];
  told = [];
  tasks = [task()];
  tested = { base: "0123456789ab", merge: "abc1234def567", current: true };
  running = [];
  next = async () => outcome({ outcome: "landed" });
  changes = {
    root: "C:\\dev\\app-wt\\qr",
    base: "0123456",
    truncated: false,
    files: [
      changed("assets/qr-frame.png", "A", null, null),
      changed("notes.txt", "?", 3, 0),
      changed("src/i18n.ts", "M", 24, 3),
      changed("src/mobile/Settings.tsx", "R", 4, 1, { oldPath: "src/mobile/Pairing.tsx" }),
      changed("src/old.ts", "D", 0, 40),
      changed("src/qr.ts", "A", 88, 0),
    ],
  };
  diffs = {
    "src/qr.ts": { text: "diff --git a/src/qr.ts b/src/qr.ts\n@@ -0,0 +1,2 @@\n+export const qr = 1;\n+\n", binary: false, truncated: false },
    "assets/qr-frame.png": { text: "Binary files /dev/null and b/assets/qr-frame.png differ\n", binary: true, truncated: false },
  };
  state = {
    workspaces: [
      {
        id: "w1",
        name: "App",
        cwd: "C:\\dev\\app",
        extraRoots: [],
        panes: [{ ...WORKER, id: "p-ava", name: "Ava", role: "orchestrator", taskId: undefined }, WORKER],
        themeId: null,
        shellId: null,
        tree: null,
        savedCommands: [],
        folderId: null,
        orchestration: {
          maxWorkers: 3,
          maxAttempts: 2,
          autoMerge: false,
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
    settings: { ...DEFAULT_SETTINGS },
  } as AppState;
  const delivery: DeliveryDeps = {
    preflight: async (args) => {
      log.push(`preflight:${args.worktree}:${args.landing}:${args.task ?? ""}`);
      return {
        repo: "C:\\dev\\app",
        queue: "c:/dev/app/.git",
        branch: "hangar/qr",
        target: "main",
        ahead: 3,
        tested: args.task ? tested : null,
      };
    },
    run: (request) => {
      log.push(`run:${request.land}:${request.reuse}`);
      return next();
    },
    cleanup: async () => ({ worktreeRemoved: true, branchDeleted: true, kept: [] }),
    running: async () => running,
    shells: () => [],
    board: {
      load: async () => ({ tasks: tasks.map((entry) => ({ ...entry })) }),
      update: async (_cwd, id, patch: TaskPatch) => {
        tasks = tasks.map((entry) => (entry.id === id ? ({ ...entry, ...patch } as Task) : entry));
        return tasks.find((entry) => entry.id === id)!;
      },
      comment: async (_cwd, id, author, text) => {
        log.push(`comment:${author}:${text}`);
        tasks = tasks.map((entry) =>
          entry.id === id ? { ...entry, comments: [...entry.comments, comment(`c${entry.comments.length + 1}`, author, text, 9_000)] } : entry,
        );
        return tasks.find((entry) => entry.id === id)!;
      },
    },
    tell: async (paneId, text) => void log.push(`tell:${paneId}:${text}`),
  };
  deps = {
    snapshot: () => state,
    closePane: (workspaceId: string, paneId: string) => {
      log.push(`close:${workspaceId}:${paneId}`);
      state = {
        ...state,
        workspaces: state.workspaces.map((ws) => (ws.id === workspaceId ? { ...ws, panes: ws.panes.filter((p) => p.id !== paneId) } : ws)),
      };
    },
    sleep: async () => undefined,
    delivery,
  } as unknown as ControlDeps;
  sources = {
    changes: async (path, base) => {
      log.push(`changes:${path}:${base}`);
      return changes;
    },
    fileDiff: async (path, file, base) => {
      log.push(`diff:${path}:${file.path}:${base}`);
      const diff = diffs[file.path];
      if (!diff) throw new Error(`no diff for ${file.path}`);
      return diff;
    },
    // A landing gets as long as it takes, unless a test says otherwise.
    wait: () => new Promise<void>(() => undefined),
  };
});

const call = <T>(op: string, args: Record<string, unknown>) => runDelivery(op, args, deps, tell, sources) as Promise<T>;

describe("the hangar comments", () => {
  it("say what the check and the review of the delivery that waits found", () => {
    const { passed, earlier } = deliveryHistory(task().comments);
    expect(passed).toEqual({
      at: 3_000,
      target: "main",
      merge: "abc1234",
      inTheWay: null,
      summary: "Settings › Mobile pairs a phone by QR code.\nThe code is single use.",
      check: { command: "pnpm test", passed: true, seconds: 84 },
      review: { approved: true, notes: "Clean split.\nMinor: memoize the canvas.", blocking: [] },
    });
    expect(earlier).toEqual([
      {
        at: 1_000,
        outcome: "checkFailed",
        text: "The check `pnpm test` failed (exit 1) on hangar/qr merged onto main.",
      },
      {
        at: 2_000,
        outcome: "changesRequested",
        text: "The reviewer asked for changes before hangar/qr lands:",
        blocking: [
          { file: "src/qr.ts", line: 12, problem: "the code is drawn before it is checked" },
          { file: "src/i18n.ts", problem: "a French string is missing" },
        ],
      },
    ]);
  });

  it("read a delivery without a summary, a check or a review", () => {
    const bare = comment("c", "hangar", "Passed: waits for the person to land it on dev (tested as 1234567).\nCheck: none set for this workspace.");
    expect(deliveryHistory([bare]).passed).toMatchObject({ target: "dev", summary: null, check: null, review: null });
    // Someone else writing the same words is not Hangar.
    expect(deliveryHistory([{ ...bare, author: "user" }]).passed).toBeNull();
  });

  it("know the person's local changes stood in the way, and keep what a landing tried again did not repeat", () => {
    const blocked = comment(
      "c4",
      "hangar",
      "Passed, but main's checkout has local changes in the way (src/a.ts, src/b.ts): waits for the person to land " +
        "it once they are committed or set aside (tested as abc1234).\nLanded as it was checked and reviewed when it was submitted.",
      4_000,
    );
    const { passed, earlier } = deliveryHistory([...task().comments, blocked]);
    expect(passed).toMatchObject({
      at: 4_000,
      inTheWay: ["src/a.ts", "src/b.ts"],
      summary: "Settings › Mobile pairs a phone by QR code.\nThe code is single use.",
      check: { command: "pnpm test", passed: true, seconds: 84 },
      review: { approved: true },
    });
    expect(earlier).toHaveLength(2);
  });
});

describe("remote_delivery", () => {
  it("says what the delivery brings, what the check and the review said, and where it lands", async () => {
    const answer = await call<RemoteDelivery>("remote_delivery", { task: "t1" });
    expect(answer).toEqual({
      taskId: "t1",
      title: "Pair a phone by QR code",
      workspace: "App",
      workspaceId: "w1",
      worker: "Dana",
      branch: "hangar/qr",
      target: "main",
      summary: "Settings › Mobile pairs a phone by QR code.\nThe code is single use.",
      check: { command: "pnpm test", passed: true, seconds: 84 },
      review: { approved: true, notes: "Clean split.\nMinor: memoize the canvas.", blocking: [] },
      // The untracked notes.txt was never delivered.
      files: [
        { path: "assets/qr-frame.png", status: "added", additions: 0, deletions: 0, binary: true },
        { path: "src/i18n.ts", status: "modified", additions: 24, deletions: 3 },
        { path: "src/mobile/Settings.tsx", oldPath: "src/mobile/Pairing.tsx", status: "renamed", additions: 4, deletions: 1 },
        { path: "src/old.ts", status: "deleted", additions: 0, deletions: 40 },
        { path: "src/qr.ts", status: "added", additions: 88, deletions: 0 },
      ],
      waiting: true,
      stage: null,
      merge: "abc1234",
      passedAt: 3_000,
      moved: false,
      inTheWay: null,
      earlier: expect.any(Array),
    });
    // The worktree against the branch it lands on, as delivery.rs names them.
    expect(log).toEqual([`preflight:${WORKER.cwd}:true:t1`, `changes:${WORKER.cwd}:main`]);
  });

  it("says when the worker committed since, and when a landing runs for it", async () => {
    tested = { ...tested!, current: false };
    running = [{ taskId: "t1", queue: "q", stage: "checking", since: 1 }];
    changes = { ...changes, truncated: true };
    const answer = await call<RemoteDelivery>("remote_delivery", { task: "t1" });
    expect(answer).toMatchObject({ moved: true, waiting: false, stage: "checking", moreFiles: true });
  });

  it("refuses a task that does not wait to land, or that no worker holds", async () => {
    tasks = [task({ column: "doing", labels: ["ui"] })];
    for (const op of ["remote_delivery", "remote_delivery_file", "remote_land", "remote_rework"]) {
      await expect(call(op, { task: "t1", file: "src/qr.ts", text: "Fix it" })).rejects.toThrow(
        'Nothing of "Pair a phone by QR code" waits to land any more',
      );
    }
    state.workspaces[0].panes = state.workspaces[0].panes.filter((pane) => pane.id !== WORKER.id);
    await expect(call("remote_delivery", { task: "t1" })).rejects.toThrow(/No worker holds this task/);
    await expect(call("remote_delivery", {})).rejects.toThrow(/Name the task/);
    expect(log.some((line) => line.startsWith("run:"))).toBe(false);
    expect(told).toEqual([]);
  });
});

describe("remote_delivery_file", () => {
  it("gives one file's patch against the branch it lands on", async () => {
    expect(await call<RemoteDeliveryPatch>("remote_delivery_file", { task: "t1", file: "src/qr.ts" })).toEqual({
      path: "src/qr.ts",
      patch: diffs["src/qr.ts"].text,
      binary: false,
      truncated: false,
    });
    expect(log).toContain(`diff:${WORKER.cwd}:src/qr.ts:main`);
    expect(await call<RemoteDeliveryPatch>("remote_delivery_file", { task: "t1", file: "assets/qr-frame.png" })).toEqual({
      path: "assets/qr-frame.png",
      patch: "",
      binary: true,
      truncated: false,
    });
  });

  it("cuts a big patch at 200 KB, and says so", async () => {
    diffs["src/i18n.ts"] = { text: `@@ -1,1 +1,9000 @@\n${"+ a line of the dictionary\n".repeat(9_000)}`, binary: false, truncated: false };
    const big = await call<RemoteDeliveryPatch>("remote_delivery_file", { task: "t1", file: "src/i18n.ts" });
    expect(big.truncated).toBe(true);
    expect(new TextEncoder().encode(big.patch).length).toBeLessThanOrEqual(PATCH_MAX_BYTES);
    expect(big.patch.endsWith("+ a line of the dictionary\n")).toBe(true);
  });

  it("refuses a file the delivery does not change", async () => {
    await expect(call("remote_delivery_file", { task: "t1", file: "notes.txt" })).rejects.toThrow(
      '"notes.txt" is not one of the files',
    );
    await expect(call("remote_delivery_file", { task: "t1" })).rejects.toThrow(/Name the file/);
  });

  it("never cuts inside a line, nor inside a character", () => {
    expect(capPatch("ab\ncd\n", 5)).toEqual({ text: "ab\n", cut: true });
    expect(capPatch("ab\ncd\n", 6)).toEqual({ text: "ab\ncd\n", cut: false });
    // One line longer than the cut: it stops before the "é" it would split.
    expect(capPatch("abé", 3)).toEqual({ text: "ab", cut: true });
  });
});

describe("remote_land", () => {
  it("lands what waits, as the PC's « Fusionner » does", async () => {
    let shown = false;
    next = async () => {
      shown = isDelivering("w1");
      return outcome({ outcome: "landed" });
    };
    expect(await call<RemoteLanded>("remote_land", { task: "t1", merge: "abc1234" })).toEqual({ landed: true, target: "main" });
    // Tested back then, landed as it was.
    expect(log).toContain("run:true:true");
    expect(shown).toBe(true);
    expect(isDelivering("w1")).toBe(false);
    expect(tasks[0]).toMatchObject({ column: "done", labels: ["ui"] });
    expect(log).toContain("close:w1:p-dana");
  });

  it("relays why it did not land, in landDelivery's words", async () => {
    next = async () => outcome({ outcome: "dirty", inCheckout: true, files: ["src/a.ts"], detail: "passed, but…", landedIn: null });
    await expect(call("remote_land", { task: "t1" })).rejects.toThrow(
      "Local changes in main's checkout are in the way (src/a.ts): commit them or set them aside, then land it again. " +
        "Nothing was touched.",
    );
    // Told once: the next « Fusionner » is a landing of its own.
    next = async () => outcome({ outcome: "landed" });
    expect(await call<RemoteLanded>("remote_land", { task: "t1" })).toEqual({ landed: true, target: "main" });
    expect(log.filter((line) => line.startsWith("run:"))).toHaveLength(2);
  });

  it("refuses to land a delivery other than the one the phone showed", async () => {
    tested = { ...tested!, merge: "fedcba9876543" };
    await expect(call("remote_land", { task: "t1", merge: "abc1234" })).rejects.toThrow(
      /delivered again since you opened it \(now fedcba9, you saw abc1234\).*Nothing was done/,
    );
    expect(log.some((line) => line.startsWith("run:"))).toBe(false);
  });

  it("answers while a long landing runs, and the same call waits for that landing", async () => {
    let finish!: (value: DeliveryOutcome) => void;
    next = () => new Promise((resolve) => (finish = resolve));
    sources.wait = async () => undefined;
    running = [{ taskId: "t1", queue: "q", stage: "checking", since: 1 }];

    expect(await call<RemoteLanded>("remote_land", { task: "t1", merge: "abc1234" })).toEqual({ landed: false, stage: "checking" });
    expect(await call<RemoteDelivery>("remote_delivery", { task: "t1" })).toMatchObject({ waiting: false, stage: "checking" });
    await expect(call("remote_rework", { task: "t1", text: "Fix it" })).rejects.toThrow(/being landed or tested right now/);
    running = [];
    expect(await call<RemoteLanded>("remote_land", { task: "t1" })).toEqual({ landed: false, stage: "queued" });

    finish(outcome({ outcome: "landed" }));
    await vi.waitFor(() => expect(deliveryState("t1")?.state).toBe("landed"));
    expect(await call<RemoteLanded>("remote_land", { task: "t1" })).toEqual({ landed: true, target: "main" });
    expect(log.filter((line) => line.startsWith("run:"))).toEqual(["run:true:true"]);
  });
});

describe("remote_rework", () => {
  it("sends the person's words to the worker, and records them on the task", async () => {
    expect(await call("remote_rework", { task: "t1", text: "  The QR code is too small on an iPhone SE.  ", merge: "abc1234" })).toEqual({
      sent: true,
    });
    expect(told).toEqual(["w1:p-dana:The QR code is too small on an iPhone SE."]);
    expect(log).toContain(
      "comment:user:Sent back to Dana from the phone, to fix before landing: The QR code is too small on an iPhone SE.",
    );
    // What waits stays until the worker's next delivery replaces it.
    expect(tasks[0]).toMatchObject({ column: "review", labels: ["ui", "ready-to-land"] });
  });

  it("refuses an empty instruction, or one for a delivery since replaced", async () => {
    await expect(call("remote_rework", { task: "t1", text: "  " })).rejects.toThrow(/Say what to fix/);
    tested = { ...tested!, merge: "fedcba9876543" };
    await expect(call("remote_rework", { task: "t1", text: "Fix it", merge: "abc1234" })).rejects.toThrow(/delivered again/);
    expect(told).toEqual([]);
  });

  it("reaches the worker through the queue, as the person, its project woken for it", async () => {
    const queued: string[] = [];
    const opened: string[] = [];
    const answer = await runRemote(
      "remote_rework",
      { task: "t1", text: "Use the accent colour" },
      {
        ...deps,
        enqueue: async (id: string, text: string, from?: string) => void queued.push(`${id}:${text}:${from ?? "person"}`),
        openWorkspace: (id: string) => void opened.push(id),
        touch: () => undefined,
        asleep: () => true,
        isOpen: () => false,
        terminal: async () => null,
      } as ControlDeps,
      activityHistory,
      undefined,
      statsSources,
      sources,
    );
    expect(answer).toEqual({ sent: true });
    expect(queued).toEqual(["p-dana:Use the accent colour:person"]);
    expect(opened).toEqual(["w1"]);
  });
});
