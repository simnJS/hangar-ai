import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { boardComment, boardLoad, boardUpdate, type Task, type TaskPatch } from "./board";
import { resolvePath, type Caller, type ControlDeps } from "./control";
import { bridgeEnqueue } from "./ipc";
import { NEEDS_YOU_LABEL, orchestrationOf, READY_TO_LAND_LABEL, workersOf } from "./orchestration";
import { postOrchestratorEvent, type OrchestratorEvent } from "./orchestratorEvents";
import { resolveShell } from "./shells";
import type { Pane, ShellInfo, Workspace } from "../types";

/**
 * The delivery_* control operations: a worker handing in its branch, which
 * lands on its workspace's branch once checked and reviewed (delivery.rs).
 *
 * delivery.rs does the git, the check and the review, one delivery at a time
 * per repository, and says what happened. What follows is decided here, where
 * the board, the panes and the orchestrator are: a landed task is done and its
 * worker goes; a failed one goes back to its worker, as a message from Hangar,
 * with what to fix — until the third failure, which is the person's.
 *
 * One that passed but waits for the person stands for the branch as it was
 * tested: new commits on it replace it with a delivery of their own, through
 * the merge, the check and the review again, and it never lands past them.
 *
 * Every text below that a model reads is English and says what to do next.
 */

export type DeliveryOutcomeKind = Extract<OrchestratorEvent, { kind: "delivery" }>["outcome"];

/** Field names are camelCase to match the Rust serialization. */
export interface DeliveryOutcome {
  outcome: DeliveryOutcomeKind;
  /** What happened and what to do about it, for the worker. */
  detail: string;
  /** In conflict, or uncommitted in the way. */
  files: string[];
  /** For `dirty`: the person's checkout is in the way, not the worker's worktree. */
  inCheckout: boolean;
  changed: string[];
  outsideOwns: string[];
  check: {
    command: string;
    passed: boolean;
    exitCode: number | null;
    timedOut: boolean;
    seconds: number;
    tail: string;
  } | null;
  review: {
    approve: boolean;
    blocking: { file: string; line?: number | null; problem: string }[];
    notes: string;
  } | null;
  /** The merge it was tested as, and the head it was merged onto. */
  merge: string | null;
  base: string | null;
  /** The checkout fast-forwarded; null when only the branch's ref moved. */
  landedIn: string | null;
}

export interface DeliveryPlan {
  /** The main checkout of the repository. */
  repo: string;
  /** What deliveries of this repository wait in line on. */
  queue: string;
  branch: string;
  target: string;
  /** Commits the target does not have yet. */
  ahead: number;
  /** The task's delivery that waits for the person, when preflight was asked about a task. */
  tested?: TestedDelivery | null;
}

export interface TestedDelivery {
  /** The head of the target it was tested on. */
  base: string;
  /** The merge it was checked and reviewed as. */
  merge: string;
  /** The worker's branch has not moved since: what waits is its latest work. */
  current: boolean;
}

export interface DeliveryRequest {
  workspace: string;
  worktree: string;
  branch: string;
  target: string;
  task: { id: string; title: string; description: string; owns: string[] };
  summary: string | null;
  checkCommand: string | null;
  shell: ShellInfo | null;
  reviewerModel: string;
  land: boolean;
  /** Lands the merge tested earlier when the target has not moved since. */
  reuse: boolean;
  portBase: number | null;
}

export interface DeliveryCleanup {
  worktreeRemoved: boolean;
  branchDeleted: boolean;
  /** What was kept, and why. */
  kept: string[];
}

export interface RunningDelivery {
  taskId: string;
  queue: string;
  /** queued, merging, checking, reviewing or landing. */
  stage: string;
  since: number;
}

export interface DeliveryDeps {
  preflight: (args: {
    workspace: string;
    worktree: string;
    target: string | null;
    landing: boolean;
    /** Also says what of this task's delivery waits for the person. */
    task?: string;
  }) => Promise<DeliveryPlan>;
  /** Resolves once the delivery is over — minutes, with a check and a review. */
  run: (delivery: DeliveryRequest) => Promise<DeliveryOutcome>;
  cleanup: (args: {
    worktree: string;
    branch: string;
    target: string;
    taskId: string;
  }) => Promise<DeliveryCleanup>;
  running: () => Promise<RunningDelivery[]>;
  /** The shells detected on this machine, for the worker's own. */
  shells: () => ShellInfo[];
  board: {
    load: (cwd: string) => Promise<{ tasks: Task[] }>;
    update: (cwd: string, id: string, patch: TaskPatch) => Promise<Task>;
    comment: (cwd: string, id: string, author: string, text: string) => Promise<Task>;
  };
  /** Queues `text` for a pane as a message from Hangar, never as the person's. */
  tell: (paneId: string, text: string) => Promise<unknown>;
}

/** What the comments and the messages are signed with (bridge.rs `Queued.from`). */
export const HANGAR = "hangar";
/** A delivery that passed and waits for the person to land it. */
export const READY_LABEL = READY_TO_LAND_LABEL;
export { NEEDS_YOU_LABEL };
/** Failed deliveries of one task before it goes to the person. */
export const MAX_FAILED_DELIVERIES = 3;
/** How the board's comments count the failed deliveries: they outlive the app. */
const FAILED = "Delivery failed";

/** The app's own wiring: delivery.rs, the board, the hangar-bridge queue. */
export function deliveryDeps(shells: () => ShellInfo[]): DeliveryDeps {
  // The board view and the orchestrator's events reload on this, which
  // writes through the HTTP API emit by themselves and these do not.
  const changed = <T>(cwd: string, task: T) => {
    emit("board:changed", cwd).catch(() => undefined);
    return task;
  };
  return {
    preflight: (args) => invoke<DeliveryPlan>("delivery_preflight", args),
    run: (delivery) => invoke<DeliveryOutcome>("delivery_run", { delivery }),
    cleanup: (args) => invoke<DeliveryCleanup>("delivery_cleanup", args),
    running: () => invoke<RunningDelivery[]>("delivery_running"),
    shells,
    board: {
      load: boardLoad,
      update: (cwd, id, patch) => boardUpdate(cwd, id, patch).then((task) => changed(cwd, task)),
      comment: (cwd, id, author, text) =>
        boardComment(cwd, id, author, text).then((task) => changed(cwd, task)),
    },
    tell: (paneId, text) => bridgeEnqueue(paneId, text, HANGAR),
  };
}

/* ── State ─────────────────────────────────────────────────────── */

interface InFlight {
  workspaceId: string;
  worker: string;
  since: number;
  /** The person landing a delivery that waited, rather than a worker submitting. */
  landing: boolean;
}

/** By task id: one delivery at a time per task. */
const inFlight = new Map<string, InFlight>();
/** The last outcome of each task's delivery, while the app runs. */
const settled = new Map<string, { outcome: DeliveryOutcomeKind; detail: string; at: number }>();

export interface DeliveryState {
  /** `running` until it is over, then its outcome. */
  state: "running" | DeliveryOutcomeKind;
  since: number;
  detail?: string;
}

/** Where a task's delivery stands, for whoever lists the workers. */
export function deliveryState(taskId: string): DeliveryState | null {
  const running = inFlight.get(taskId);
  if (running) return { state: "running", since: running.since };
  const last = settled.get(taskId);
  return last ? { state: last.outcome, since: last.at, detail: last.detail } : null;
}

/**
 * Whether a delivery of the workspace's is being tested or landed — minutes,
 * during which its worker waits quietly and its workspace must not sleep.
 */
export const deliveringIn = (workspaceId: string) =>
  [...inFlight.values()].some((entry) => entry.workspaceId === workspaceId);

/** For the tests: forgets every delivery. */
export function resetDeliveries() {
  inFlight.clear();
  settled.clear();
}

/* ── Helpers ───────────────────────────────────────────────────── */

const messageOf = (err: unknown) => (err instanceof Error ? err.message : String(err));

const text = (value: unknown) =>
  typeof value === "string" && value.trim() !== "" ? value.trim() : null;

const short = (sha: string | null) => (sha ? sha.slice(0, 7) : "?");

const ownsOf = (task: Task) => (task as Task & { owns?: string[] }).owns ?? [];

const without = (labels: string[], ...gone: string[]) => labels.filter((label) => !gone.includes(label));

const withLabel = (labels: string[], label: string) => (labels.includes(label) ? labels : [...labels, label]);

/** The first sentence or line of `detail`, short enough for an event. */
function headline(detail: string): string {
  const line = detail.split("\n")[0].trim();
  const sentence = line.match(/^.*?[.:](\s|$)/)?.[0]?.trim() ?? line;
  return sentence.length > 240 ? `${sentence.slice(0, 237)}...` : sentence;
}

/**
 * An outcome in one line for the orchestrator: the fact it acts on — the
 * files in conflict, the line of the check that failed, the first blocking
 * item — not the instructions the worker gets.
 */
function eventDetail(outcome: DeliveryOutcome, target: string): string {
  const files = outcome.files.join(", ");
  const line = (() => {
    switch (outcome.outcome) {
      case "landed":
        return `landed on ${target} (${short(outcome.merge)})`;
      case "ready":
        return `ready to land on ${target}`;
      case "conflict":
        return files ? `conflict with ${target} in ${files}` : headline(outcome.detail);
      case "dirty":
        return outcome.inCheckout
          ? `passed, but ${target}'s checkout has local changes in the way${files ? ` (${files})` : ""}: ` +
              "waits for the person"
          : `uncommitted changes in the worktree: ${files}`;
      case "checkFailed": {
        const check = outcome.check;
        if (!check) return headline(outcome.detail);
        const lines = check.tail.split("\n").map((entry) => entry.trim()).filter(Boolean);
        const telling = lines.find((entry) => /error|fail|panic|assert/i.test(entry)) ?? lines[lines.length - 1];
        const how = check.timedOut ? `timed out after ${check.seconds} s` : `exited ${check.exitCode ?? "?"}`;
        return `\`${check.command}\` ${how}${telling ? `: ${telling}` : ""}`;
      }
      case "changesRequested": {
        const blocking = outcome.review?.blocking ?? [];
        const first = blocking[0];
        if (!first) return `review: ${outcome.review?.notes.trim() || "changes requested"}`;
        const more = blocking.length > 1 ? ` (+${blocking.length - 1} more)` : "";
        return `review: ${first.file}${first.line ? `:${first.line}` : ""}: ${first.problem.trim()}${more}`;
      }
      case "stale":
        return `the person asked to land it on ${target}; it was tested as ${short(outcome.merge)}`;
      default:
        return headline(outcome.detail);
    }
  })();
  return line.length > 360 ? `${line.slice(0, 357)}...` : line;
}

function checkLine(outcome: DeliveryOutcome, checkCommand: string | null): string {
  const check = outcome.check;
  if (!check) return checkCommand ? `Check: \`${checkCommand}\` not run.` : "Check: none set for this workspace.";
  if (check.passed) return `Check: \`${check.command}\` passed in ${check.seconds} s.`;
  if (check.timedOut) return `Check: \`${check.command}\` timed out after ${check.seconds} s.`;
  return `Check: \`${check.command}\` failed (exit ${check.exitCode ?? "?"}).`;
}

function reviewLine(outcome: DeliveryOutcome, model: string): string | null {
  const review = outcome.review;
  if (!review) return null;
  const notes = review.notes.trim();
  const verdict = review.approve && review.blocking.length === 0 ? "approved" : "changes requested";
  return `Review (${model}): ${verdict}.${notes ? ` Notes: ${notes}` : ""}`;
}

const callerPane = (ws: Workspace, caller: Caller): Pane | undefined =>
  caller.paneId ? ws.panes.find((pane) => pane.id === caller.paneId) : undefined;

function requireDeps(deps: ControlDeps): DeliveryDeps {
  if (!deps.delivery) throw new Error("Deliveries are not available in this Hangar window.");
  return deps.delivery;
}

/* ── Operations ────────────────────────────────────────────────── */

export async function deliveryControl(
  op: string,
  ws: Workspace,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
): Promise<unknown> {
  switch (op) {
    case "delivery_submit":
      return submit(ws, args, caller, deps, requireDeps(deps));
    case "delivery_status":
      return status(ws, caller, requireDeps(deps));
    default:
      throw new Error(`Unknown operation "${op}".`);
  }
}

async function submit(
  ws: Workspace,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
  delivery: DeliveryDeps,
): Promise<unknown> {
  const pane = callerPane(ws, caller);
  if (!pane || pane.role !== "worker" || !pane.taskId) {
    throw new Error(
      "delivery_submit is for worker panes, the ones an orchestrator starts on a board task with worker_spawn, " +
        "and this is not one. Commit your work and tell whoever asked for it instead.",
    );
  }
  const taskId = pane.taskId;
  const busy = inFlight.get(taskId);
  if (busy) {
    throw new Error(
      busy.landing
        ? "Your delivery is being landed right now. Wait for Hangar's message; do not submit again."
        : "Your delivery is already being merged, checked and reviewed. Wait for Hangar's message with the " +
            "outcome; do not submit again meanwhile.",
    );
  }
  // Taken before anything is awaited: the person landing it, or a second
  // submission, waits for this one.
  inFlight.set(taskId, { workspaceId: ws.id, worker: pane.name, since: Date.now(), landing: false });
  let started = false;
  try {
    const task = (await delivery.board.load(ws.cwd)).tasks.find((entry) => entry.id === taskId);
    if (!task) {
      throw new Error(
        `Your task (${taskId}) is no longer on the board, so there is nothing to deliver it for. Stop here, and ` +
          "say so in your last message.",
      );
    }
    if (task.column === "done") {
      throw new Error(`"${task.title}" is already done: there is nothing more to deliver. Stop here.`);
    }
    // Passed, and waiting for the person to land it.
    const waiting = task.labels.includes(READY_LABEL);
    if (task.column !== "doing" && !(waiting && task.column === "review")) {
      throw new Error(
        `"${task.title}" is in ${task.column}, not doing: nobody is meant to be working on it now. Stop here.`,
      );
    }

    const settings = orchestrationOf(ws);
    const worktree = resolvePath(ws.cwd, pane.cwd || ws.cwd);
    const plan = await delivery
      .preflight({ workspace: ws.cwd, worktree, target: settings.branch, landing: false, task: taskId })
      .catch((err) => {
        throw new Error(messageOf(err));
      });

    const tested = plan.tested ?? null;
    if (waiting && tested?.current) {
      return waitsAlready(ws, pane, task, plan, tested);
    }
    if (waiting) {
      // New commits since it passed: what waited no longer stands for the
      // branch. It goes, and this one takes the whole way again.
      await delivery.board.update(ws.cwd, taskId, { column: "doing", labels: without(task.labels, READY_LABEL) });
      await delivery.board.comment(
        ws.cwd,
        taskId,
        HANGAR,
        `Submitted again with new commits on ${plan.branch}: the delivery that waited for the person` +
          `${tested ? ` (tested as ${short(tested.merge)})` : ""} is withdrawn, and this one is merged, checked and ` +
          "reviewed before it can land.",
      );
    }
    const ahead = (await delivery.running().catch(() => [] as RunningDelivery[])).filter(
      (entry) => entry.queue === plan.queue,
    ).length;

    started = true;
    void deliver(ws, pane, task, plan, text(args.summary), deps, delivery, false);

    const check = settings.checkCommand ? `runs \`${settings.checkCommand}\` there, ` : "";
    return {
      submitted: true,
      task: task.title,
      branch: plan.branch,
      target: plan.target,
      commits: plan.ahead,
      ...(ahead ? { queuedBehind: ahead } : {}),
      ...(waiting ? { replaces: tested ? short(tested.merge) : "the delivery that waited for the person" } : {}),
      next:
        `Hangar now merges ${plan.branch} onto ${plan.target} in a separate worktree, ${check}and has a reviewer ` +
        "read the change. The outcome comes to you as a message from Hangar, usually within minutes. End your " +
        `turn now and wait; do not change ${plan.branch} meanwhile.`,
    };
  } finally {
    if (!started) inFlight.delete(taskId);
  }
}

/**
 * Submitted again without a new commit: what passed still waits for the
 * person, and the worker has nothing to do. Said to the orchestrator too, which
 * may be waiting on this submission.
 */
function waitsAlready(ws: Workspace, pane: Pane, task: Task, plan: DeliveryPlan, tested: TestedDelivery) {
  const testedAs = short(tested.merge);
  postOrchestratorEvent(ws.id, {
    kind: "delivery",
    taskId: task.id,
    title: task.title,
    worker: pane.name,
    outcome: "ready",
    detail: `submitted again without a new commit: still waits to land on ${plan.target} as tested (${testedAs})`,
  });
  return {
    submitted: false,
    waiting: true,
    task: task.title,
    branch: plan.branch,
    target: plan.target,
    testedAs,
    next:
      `This delivery already passed its check and its review, as ${testedAs}, and waits for the person to land it on ` +
      `${plan.target}: ${plan.branch} has no commit since. There is nothing more for you to do: stop here. If you ` +
      `were asked to change something, commit the change on ${plan.branch}, then call delivery_submit again: the ` +
      "new delivery replaces the one waiting.",
  };
}

async function status(ws: Workspace, caller: Caller, delivery: DeliveryDeps): Promise<unknown> {
  const running = await delivery.running().catch(() => [] as RunningDelivery[]);
  const mine = callerPane(ws, caller);
  const panes =
    mine?.role === "worker" && mine.taskId ? [mine] : workersOf(ws).filter((pane) => pane.taskId);
  return {
    deliveries: panes.map((pane) => {
      const taskId = pane.taskId as string;
      const stage = running.find((entry) => entry.taskId === taskId);
      const state = deliveryState(taskId);
      return {
        worker: pane.name,
        task: taskId,
        state: stage?.stage ?? state?.state ?? "none",
        ...(state ? { since: new Date(stage?.since ?? state.since).toISOString() } : {}),
        ...(state?.detail ? { detail: state.detail } : {}),
      };
    }),
  };
}

/** Runs one delivery to its end, and does what its outcome calls for. */
async function deliver(
  ws: Workspace,
  pane: Pane,
  task: Task,
  plan: DeliveryPlan,
  summary: string | null,
  deps: ControlDeps,
  delivery: DeliveryDeps,
  landing: boolean,
): Promise<DeliveryOutcome> {
  const settings = orchestrationOf(ws);
  const state = deps.snapshot();
  const request: DeliveryRequest = {
    workspace: ws.cwd,
    worktree: resolvePath(ws.cwd, pane.cwd || ws.cwd),
    branch: plan.branch,
    target: plan.target,
    task: { id: task.id, title: task.title, description: task.description, owns: ownsOf(task) },
    summary,
    checkCommand: settings.checkCommand,
    shell: resolveShell(delivery.shells(), pane.shellId, ws.shellId, state.settings.shellId),
    reviewerModel: settings.reviewerModel,
    land: landing || settings.autoMerge,
    reuse: landing,
    portBase: pane.portBase ?? null,
  };
  let outcome: DeliveryOutcome;
  try {
    outcome = await delivery.run(request);
  } catch (err) {
    outcome = {
      outcome: "error",
      detail: `The delivery could not run: ${messageOf(err)}. Nothing was merged.`,
      files: [],
      inCheckout: false,
      changed: [],
      outsideOwns: [],
      check: null,
      review: null,
      merge: null,
      base: null,
      landedIn: null,
    };
  }
  try {
    await settle(ws, pane, task, request, outcome, deps, delivery);
  } catch {
    // The outcome stands whatever its follow-ups managed; the next delivery
    // or the person sees the board as it is.
  } finally {
    settled.set(task.id, { outcome: outcome.outcome, detail: headline(outcome.detail), at: Date.now() });
    inFlight.delete(task.id);
  }
  return outcome;
}

async function settle(
  submitted: Workspace,
  pane: Pane,
  task: Task,
  request: DeliveryRequest,
  outcome: DeliveryOutcome,
  deps: ControlDeps,
  delivery: DeliveryDeps,
): Promise<void> {
  // As things are now, minutes later: the pane may have been closed, the
  // task moved or renamed.
  const ws = deps.snapshot().workspaces.find((entry) => entry.id === submitted.id) ?? submitted;
  const worker = ws.panes.find((entry) => entry.id === pane.id) ?? null;
  const cwd = ws.cwd;
  const current = (await delivery.board.load(cwd)).tasks.find((entry) => entry.id === task.id) ?? task;
  const { target, branch, checkCommand, reviewerModel } = request;
  const post = (detail: string) =>
    postOrchestratorEvent(ws.id, {
      kind: "delivery",
      taskId: task.id,
      title: current.title,
      worker: pane.name,
      outcome: outcome.outcome,
      detail,
    });
  // Landing what waited for the person lands the merge tested back then.
  const asTested = request.reuse && !outcome.check && !outcome.review;
  const details = [
    request.summary ? `Summary: ${request.summary}` : null,
    asTested ? "Landed as it was checked and reviewed when it was submitted." : checkLine(outcome, checkCommand),
    asTested ? null : reviewLine(outcome, reviewerModel),
    outcome.outsideOwns.length
      ? `Changed outside the files it owns, accepted by the review: ${outcome.outsideOwns.join(", ")}.`
      : null,
  ];

  if (outcome.outcome === "landed") {
    await delivery.board.update(cwd, task.id, {
      column: "done",
      labels: without(current.labels, READY_LABEL, NEEDS_YOU_LABEL),
    });
    // Its work is on the branch now: the worker, its worktree and its branch go.
    if (worker) deps.closePane(ws.id, worker.id);
    const cleanup = await delivery
      .cleanup({ worktree: request.worktree, branch, target, taskId: task.id })
      .catch((err): DeliveryCleanup => ({ worktreeRemoved: false, branchDeleted: false, kept: [messageOf(err)] }));
    const where = outcome.landedIn ? `, fast-forwarded in ${outcome.landedIn}` : " (only its ref moved)";
    await delivery.board.comment(
      cwd,
      task.id,
      HANGAR,
      [
        `Landed on ${target} (${short(outcome.merge)})${where}.`,
        ...details,
        cleanup.kept.length ? `Kept: ${cleanup.kept.join("; ")}.` : null,
      ]
        .filter(Boolean)
        .join("\n"),
    );
    post(eventDetail(outcome, target));
    return;
  }

  if (outcome.outcome === "ready" || (outcome.outcome === "dirty" && outcome.inCheckout)) {
    await delivery.board.update(cwd, task.id, {
      column: "review",
      labels: withLabel(current.labels, READY_LABEL),
    });
    const waits =
      outcome.outcome === "ready"
        ? `Passed: waits for the person to land it on ${target} (tested as ${short(outcome.merge)}).`
        : `Passed, but ${target}'s checkout has local changes in the way` +
          (outcome.files.length ? ` (${outcome.files.join(", ")})` : "") +
          `: waits for the person to land it once they are committed or set aside (tested as ${short(outcome.merge)}).`;
    await delivery.board.comment(cwd, task.id, HANGAR, [waits, ...details].filter(Boolean).join("\n"));
    if (worker) await delivery.tell(worker.id, outcome.detail);
    post(eventDetail(outcome, target));
    return;
  }

  if (outcome.outcome === "stale") {
    // Nothing landed and nothing failed: the worker moved on past what
    // waited, which goes; its next submission is the delivery.
    await delivery.board.update(cwd, task.id, { column: "doing", labels: without(current.labels, READY_LABEL) });
    await delivery.board.comment(
      cwd,
      task.id,
      HANGAR,
      `Not landed: ${branch} has commits that were not in the delivery that waited (tested as ` +
        `${short(outcome.merge)}). It is withdrawn; ${pane.name} was asked to submit again.`,
    );
    if (worker) await delivery.tell(worker.id, `"${current.title}" was not landed.\n\n${outcome.detail}`);
    post(eventDetail(outcome, target));
    return;
  }

  // A failure: back to the worker, with what to fix.
  const failures =
    current.comments.filter((comment) => comment.author === HANGAR && comment.text.startsWith(FAILED)).length + 1;
  const last = failures >= MAX_FAILED_DELIVERIES;
  await delivery.board.comment(
    cwd,
    task.id,
    HANGAR,
    `${FAILED} (${outcome.outcome}, ${failures} of ${MAX_FAILED_DELIVERIES}): ${outcome.detail}`,
  );
  // One that was waiting to land and no longer can is worked on again.
  const labels = last
    ? withLabel(without(current.labels, READY_LABEL), NEEDS_YOU_LABEL)
    : without(current.labels, READY_LABEL);
  if (current.column !== "doing" || labels.length !== current.labels.length || last) {
    await delivery.board.update(cwd, task.id, { column: "doing", labels });
  }
  if (worker) {
    const next = last
      ? `This is failed delivery ${failures} of ${MAX_FAILED_DELIVERIES} for this task: stop working on it now. ` +
        "The task is marked needs-you; the orchestrator and the person have been told."
      : outcome.outcome === "error"
        ? "If it fails again the same way, stop and say so in a comment on the task."
        : `Failed deliveries so far: ${failures} of ${MAX_FAILED_DELIVERIES}.`;
    await delivery.tell(worker.id, `Your delivery of "${current.title}" did not land.\n\n${outcome.detail}\n\n${next}`);
  }
  post(`${eventDetail(outcome, target)} (failed delivery ${failures} of ${MAX_FAILED_DELIVERIES})`);
  if (last) {
    postOrchestratorEvent(ws.id, {
      kind: "taskNeedsYou",
      taskId: task.id,
      title: current.title,
      reason: `${MAX_FAILED_DELIVERIES} deliveries failed, the last one: ${eventDetail(outcome, target)}`,
    });
  }
}

/**
 * Lands a delivery that passed its check and its review but waits for the
 * person, as it does with automatic landing off. Lands the merge that was
 * tested when the target branch has not moved since, and tests it again first
 * when it has; never once the worker's branch has moved past it. Rejects with
 * what stood in the way when it does not land, in words for the person who
 * asked.
 */
export async function landDelivery(ws: Workspace, taskId: string, deps: ControlDeps): Promise<void> {
  const delivery = requireDeps(deps);
  if (inFlight.has(taskId)) throw new Error("This delivery is being tested or landed already.");
  const pane = workersOf(ws).find((entry) => entry.taskId === taskId);
  // Taken before anything is awaited, like a submission.
  inFlight.set(taskId, { workspaceId: ws.id, worker: pane?.name ?? "", since: Date.now(), landing: true });
  let started = false;
  let plan: DeliveryPlan;
  let outcome: DeliveryOutcome;
  try {
    const task = (await delivery.board.load(ws.cwd)).tasks.find((entry) => entry.id === taskId);
    if (!task) throw new Error(`No task ${taskId} on the board of "${ws.name}".`);
    if (!pane) {
      throw new Error(`No worker holds "${task.title}" any more, so there is no branch to land.`);
    }
    // Submitted again since, or sent back: what is there now was not tested.
    if (!task.labels.includes(READY_LABEL)) {
      throw new Error(`Nothing of "${task.title}" waits to land any more: the board shows where it is now.`);
    }
    plan = await delivery
      .preflight({
        workspace: ws.cwd,
        worktree: resolvePath(ws.cwd, pane.cwd || ws.cwd),
        target: orchestrationOf(ws).branch,
        landing: true,
      })
      .catch((err) => {
        throw new Error(messageOf(err));
      });
    started = true;
    outcome = await deliver(ws, pane, task, plan, null, deps, delivery, true);
  } finally {
    if (!started) inFlight.delete(taskId);
  }
  if (outcome.outcome === "landed") return;
  if (outcome.outcome === "stale") {
    throw new Error(
      `${pane.name} committed on ${plan.branch} after it was checked and reviewed, so it would not land what is ` +
        `there now. Nothing was landed: ${pane.name} was asked to submit it again, and it comes back here once ` +
        "checked and reviewed.",
    );
  }
  if (outcome.outcome === "dirty" && outcome.inCheckout) {
    const files = outcome.files.length ? ` (${outcome.files.join(", ")})` : "";
    throw new Error(
      `Local changes in ${plan.target}'s checkout are in the way${files}: commit them or set them aside, then ` +
        "land it again. Nothing was touched.",
    );
  }
  if (outcome.outcome === "error") throw new Error(`It could not land: ${headline(outcome.detail)}`);
  throw new Error(
    `It no longer lands as it was tested (${eventDetail(outcome, plan.target)}), so ${pane.name} was told what ` +
      "to fix.",
  );
}
