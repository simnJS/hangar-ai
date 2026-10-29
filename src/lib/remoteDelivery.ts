import type { Task, TaskComment } from "./board";
import { resolvePath, type ControlDeps } from "./control";
import {
  deliveryState,
  HANGAR,
  landDelivery,
  READY_LABEL,
  type DeliveryDeps,
  type DeliveryPlan,
  type RunningDelivery,
} from "./deliveryControl";
import { beginDelivery } from "./deliveryRuns";
import { gitChanges, gitFileDiff, type ChangedFile, type Changes, type FileDiff } from "./git";
import { orchestrationOf, workersOf } from "./orchestration";
import type {
  RemoteDelivery,
  RemoteDeliveryBlocking,
  RemoteDeliveryCheck,
  RemoteDeliveryEarlier,
  RemoteDeliveryFile,
  RemoteDeliveryFileStatus,
  RemoteDeliveryPatch,
  RemoteDeliveryReview,
  RemoteDeliveryStage,
  RemoteLanded,
  RemoteReworked,
} from "../remote/types";
import type { Pane, Workspace } from "../types";

/**
 * What the window answers Hangar Mobile about a delivery that passed and
 * waits for the person (lib/deliveryControl): what it brings — the worker's
 * worktree against the branch it lands on, as the diff view reads it — what
 * the check and the review said, read back from the hangar comment the
 * delivery left on its task, and the two things the person can do about it
 * from the phone: land it, as the PC's « Fusionner » does, or send it back to
 * its worker with what to fix.
 *
 * The phone passes back the merge it was shown: once another delivery of the
 * task has taken its place, landing or sending back the one seen is refused.
 */

/** A patch past this is cut: the phone draws every line of it. */
export const PATCH_MAX_BYTES = 200 * 1024;
/**
 * remote_land waits this long for a landing, within the 10 s the mobile
 * server gives the window; past it, the phone asks again.
 */
export const LAND_WAIT_MS = 7_000;
/** A landing's outcome nobody came back for is forgotten after this. */
const LAND_KEPT_MS = 5 * 60_000;

/** Where the delivery ops read git from; the tests bring their own. */
export interface DeliverySources {
  changes: (path: string, base: string) => Promise<Changes>;
  fileDiff: (path: string, file: ChangedFile, base: string) => Promise<FileDiff>;
  wait: (ms: number) => Promise<void>;
}

export const deliverySources: DeliverySources = {
  changes: gitChanges,
  fileDiff: gitFileDiff,
  wait: (ms) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
};

/**
 * Puts the person's words to a worker's Claude Code, as their own: queued
 * for its session, its project woken for it (lib/remoteControl).
 */
export type TellWorker = (ws: Workspace, pane: Pane, text: string) => Promise<unknown>;

/* ── The hangar comments ───────────────────────────────────────── */

/** What the hangar comment of a delivery that passed says (deliveryControl's `settle`). */
export interface Passed {
  at: number;
  target: string;
  /** Short; null when the comment has none. */
  merge: string | null;
  /** Local changes in the target's checkout stood in the way. */
  inTheWay: string[] | null;
  summary: string | null;
  check: RemoteDeliveryCheck | null;
  review: RemoteDeliveryReview | null;
}

const READY_HEAD = /^Passed: waits for the person to land it on (.+) \(tested as ([0-9a-f]+|\?)\)\.$/;
const DIRTY_HEAD =
  /^Passed, but (.+?)'s checkout has local changes in the way(?: \((.*)\))?: waits for the person to land it once they are committed or set aside \(tested as ([0-9a-f]+|\?)\)\.$/;
const CHECK_PASSED = /^Check: `(.*)` passed in (\d+(?:\.\d+)?) s\.$/;
const CHECK_TIMED_OUT = /^Check: `(.*)` timed out after (\d+(?:\.\d+)?) s\.$/;
const CHECK_FAILED = /^Check: `(.*)` failed \(exit .*\)\.$/;
const CHECK_NONE = /^Check: (?:`.*` not run|none set for this workspace)\.$/;
const REVIEW = /^Review \(.*?\): (approved|changes requested)\.(?: Notes: ([\s\S]*))?$/;
const OUTSIDE = "Changed outside the files it owns, accepted by the review: ";
/** A landing tried again lands the merge tested back then, and says so instead of a check and a review. */
const AS_TESTED = "Landed as it was checked and reviewed when it was submitted.";
const FAILED = /^Delivery failed \((\w+), \d+ of \d+\): ([\s\S]*)$/;
const BLOCKING = /^- (.+?)(?::(\d+))?: (.+)$/;

const isCheckLine = (line: string) =>
  CHECK_PASSED.test(line) || CHECK_TIMED_OUT.test(line) || CHECK_FAILED.test(line) || CHECK_NONE.test(line);

function checkOf(line: string): RemoteDeliveryCheck | null {
  const passed = CHECK_PASSED.exec(line);
  if (passed) return { command: passed[1], passed: true, seconds: Number(passed[2]) };
  const timedOut = CHECK_TIMED_OUT.exec(line);
  if (timedOut) return { command: timedOut[1], passed: false, seconds: Number(timedOut[2]) };
  const failed = CHECK_FAILED.exec(line);
  if (failed) return { command: failed[1], passed: false, seconds: 0 };
  return null;
}

const short = (sha: string) => (sha === "?" ? null : sha.slice(0, 7));

/**
 * One comment, if it is the one a delivery that passed left; `asTested` when
 * it is a landing that waits again, which says nothing of the check, the
 * review or the summary of the delivery it lands.
 */
function passedOne(comment: TaskComment): (Passed & { asTested: boolean }) | null {
  if (comment.author !== HANGAR) return null;
  const lines = comment.text.split("\n");
  const ready = READY_HEAD.exec(lines[0]);
  const dirty = ready ? null : DIRTY_HEAD.exec(lines[0]);
  if (!ready && !dirty) return null;
  const head = ready
    ? { target: ready[1], merge: short(ready[2]), inTheWay: null }
    : {
        target: dirty![1],
        merge: short(dirty![3]),
        inTheWay: dirty![2] ? dirty![2].split(", ").filter(Boolean) : [],
      };

  // The summary, then the check's line, the review's, and what it changed
  // outside its files: a summary and the notes can take several lines.
  const at = lines.findIndex((line, index) => index > 0 && isCheckLine(line));
  const checkAt = at === -1 ? lines.length : at;
  const said = lines.slice(1, checkAt).join("\n");
  const summary = said.startsWith("Summary: ") ? said.slice("Summary: ".length).trim() || null : null;
  const rest = lines.slice(checkAt + 1);
  if (rest.length && rest[rest.length - 1].startsWith(OUTSIDE)) rest.pop();
  const verdict = REVIEW.exec(rest.join("\n"));
  return {
    at: comment.created_at,
    ...head,
    asTested: at === -1 && lines.includes(AS_TESTED),
    summary,
    check: at === -1 ? null : checkOf(lines[at]),
    review: verdict
      ? { approved: verdict[1] === "approved", notes: (verdict[2] ?? "").trim(), blocking: [] }
      : null,
  };
}

/** The first sentence or line of what the PC told the worker. */
function headline(detail: string): string {
  const line = detail.split("\n")[0].trim();
  const sentence = line.match(/^.*?[.:](\s|$)/)?.[0]?.trim() ?? line;
  return sentence.length > 240 ? `${sentence.slice(0, 237)}...` : sentence;
}

/**
 * The delivery that waits, as its hangar comment said it passed, and the
 * deliveries of the task that failed before it: the latest pass wins, a
 * landing tried again writing one of its own.
 */
export function deliveryHistory(comments: TaskComment[]): { passed: Passed | null; earlier: RemoteDeliveryEarlier[] } {
  let passed: Passed | null = null;
  let passedAt = -1;
  for (const [index, comment] of comments.entries()) {
    const one = passedOne(comment);
    if (!one) continue;
    const { asTested, ...said } = one;
    passed = asTested && passed ? { ...said, summary: passed.summary, check: passed.check, review: passed.review } : said;
    passedAt = index;
  }
  const earlier: RemoteDeliveryEarlier[] = [];
  for (const [index, comment] of comments.entries()) {
    if (comment.author !== HANGAR || (passedAt !== -1 && index > passedAt)) continue;
    const failed = FAILED.exec(comment.text);
    if (!failed) continue;
    const blocking: RemoteDeliveryBlocking[] = [];
    for (const line of failed[2].split("\n")) {
      const item = BLOCKING.exec(line.trim());
      if (item && failed[1] === "changesRequested") {
        blocking.push({ file: item[1], ...(item[2] ? { line: Number(item[2]) } : {}), problem: item[3] });
      }
    }
    earlier.push({
      at: comment.created_at,
      outcome: failed[1],
      text: headline(failed[2]),
      ...(blocking.length ? { blocking } : {}),
    });
  }
  return { passed, earlier };
}

/* ── Finding the delivery ──────────────────────────────────────── */

interface Found {
  ws: Workspace;
  pane: Pane;
  task: Task;
  delivery: DeliveryDeps;
  worktree: string;
}

const messageOf = (err: unknown) => (err instanceof Error ? err.message : String(err));

function requireDelivery(deps: ControlDeps): DeliveryDeps {
  if (!deps.delivery) throw new Error("Deliveries are not available in this Hangar window.");
  return deps.delivery;
}

function taskRef(args: Record<string, unknown>): string {
  const task = typeof args.task === "string" ? args.task.trim() : "";
  if (!task) throw new Error("Name the task: its id, from the board.");
  return task;
}

/**
 * A task's delivery that waits for the person: its worker pane still holds
 * it, with the worktree and the branch, and the board labels it ready to land.
 */
async function waiting(deps: ControlDeps, taskId: string): Promise<Found> {
  const delivery = requireDelivery(deps);
  let held: { ws: Workspace; pane: Pane } | null = null;
  for (const ws of deps.snapshot().workspaces) {
    const pane = workersOf(ws).find((entry) => entry.taskId === taskId);
    if (pane) {
      held = { ws, pane };
      break;
    }
  }
  if (!held) throw new Error("No worker holds this task any more, so nothing of it waits to land.");
  const { ws, pane } = held;
  const task = (await delivery.board.load(ws.cwd)).tasks.find((entry) => entry.id === taskId);
  if (!task) throw new Error(`No task ${taskId} on the board of "${ws.name}".`);
  if (!task.labels.includes(READY_LABEL)) {
    throw new Error(`Nothing of "${task.title}" waits to land any more: the board shows where it is now.`);
  }
  return { ws, pane, task, delivery, worktree: resolvePath(ws.cwd, pane.cwd || ws.cwd) };
}

/** The branch and its target, and what of the task was tested, as delivery.rs reads them. */
async function planOf(found: Found): Promise<DeliveryPlan> {
  return found.delivery
    .preflight({
      workspace: found.ws.cwd,
      worktree: found.worktree,
      target: orchestrationOf(found.ws).branch,
      landing: true,
      task: found.task.id,
    })
    .catch((err) => {
      throw new Error(messageOf(err));
    });
}

/** The merge it was tested as, short: what the phone passes back. */
function mergeOf(plan: DeliveryPlan, passed: Passed | null): string | null {
  return plan.tested ? plan.tested.merge.slice(0, 7) : (passed?.merge ?? null);
}

/** Refused once another delivery of the task has taken the place of the one the phone showed. */
function sameAs(args: Record<string, unknown>, found: Found, plan: DeliveryPlan) {
  if (typeof args.merge !== "string" || !args.merge.trim()) return;
  const seen = args.merge.trim().slice(0, 7);
  const now = mergeOf(plan, deliveryHistory(found.task.comments).passed);
  if (now !== null && now !== seen) {
    throw new Error(
      `"${found.task.title}" was delivered again since you opened it (now ${now}, you saw ${seen}): look at what ` +
        "waits now first. Nothing was done.",
    );
  }
}

const STAGES: readonly RemoteDeliveryStage[] = ["queued", "merging", "checking", "reviewing", "landing"];

/** What runs for the task now, if anything: delivery.rs's stage, or queued before it starts. */
async function stageOf(delivery: DeliveryDeps, taskId: string): Promise<RemoteDeliveryStage | null> {
  const running = await delivery.running().catch(() => [] as RunningDelivery[]);
  const stage = running.find((entry) => entry.taskId === taskId)?.stage;
  if (stage) return STAGES.includes(stage as RemoteDeliveryStage) ? (stage as RemoteDeliveryStage) : "queued";
  return deliveryState(taskId)?.state === "running" || landings.get(taskId)?.outcome === null ? "queued" : null;
}

/* ── The ops ───────────────────────────────────────────────────── */

const STATUS: Record<string, RemoteDeliveryFileStatus> = {
  A: "added",
  C: "added",
  D: "deleted",
  R: "renamed",
};

function fileOf(file: ChangedFile): RemoteDeliveryFile {
  return {
    path: file.path,
    ...(file.oldPath ? { oldPath: file.oldPath } : {}),
    status: STATUS[file.status] ?? "modified",
    additions: file.additions ?? 0,
    deletions: file.deletions ?? 0,
    ...(file.binary ? { binary: true as const } : {}),
  };
}

/** What a delivery brings: its commits; an untracked file was never part of one. */
const delivered = (changes: Changes) => changes.files.filter((file) => file.status !== "?");

async function remoteDelivery(deps: ControlDeps, args: Record<string, unknown>, sources: DeliverySources): Promise<RemoteDelivery> {
  const found = await waiting(deps, taskRef(args));
  const { ws, pane, task, delivery } = found;
  const plan = await planOf(found);
  const [changes, stage] = await Promise.all([sources.changes(found.worktree, plan.target), stageOf(delivery, task.id)]);
  const { passed, earlier } = deliveryHistory(task.comments);
  return {
    taskId: task.id,
    title: task.title,
    workspace: ws.name,
    workspaceId: ws.id,
    worker: pane.name,
    branch: plan.branch,
    target: plan.target,
    summary: passed?.summary ?? null,
    check: passed?.check ?? null,
    review: passed?.review ?? null,
    files: delivered(changes).map(fileOf),
    ...(changes.truncated ? { moreFiles: true as const } : {}),
    waiting: stage === null,
    stage,
    merge: mergeOf(plan, passed),
    passedAt: passed?.at ?? null,
    moved: plan.tested ? !plan.tested.current : false,
    inTheWay: passed?.inTheWay ?? null,
    earlier,
  };
}

/**
 * A patch cut at a line boundary past `max` bytes of UTF-8, or within the
 * line when the first one is longer than that, never inside a character.
 */
export function capPatch(text: string, max = PATCH_MAX_BYTES): { text: string; cut: boolean } {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= max) return { text, cut: false };
  let end = bytes.lastIndexOf(0x0a, max - 1) + 1;
  if (end === 0) {
    end = max;
    while (end > 0 && (bytes[end] & 0xc0) === 0x80) end--;
  }
  return { text: new TextDecoder().decode(bytes.subarray(0, end)), cut: true };
}

async function remoteDeliveryFile(
  deps: ControlDeps,
  args: Record<string, unknown>,
  sources: DeliverySources,
): Promise<RemoteDeliveryPatch> {
  const wanted = typeof args.file === "string" ? args.file.trim() : "";
  if (!wanted) throw new Error("Name the file: its path, from the delivery's files.");
  const found = await waiting(deps, taskRef(args));
  const plan = await planOf(found);
  const file = delivered(await sources.changes(found.worktree, plan.target)).find((entry) => entry.path === wanted);
  if (!file) throw new Error(`"${wanted}" is not one of the files "${found.task.title}" changes.`);
  const diff = await sources.fileDiff(found.worktree, file, plan.target);
  const { text, cut } = capPatch(diff.binary ? "" : diff.text);
  return { path: file.path, patch: text, binary: diff.binary || file.binary, truncated: diff.truncated || cut };
}

/** A landing the phone started: it asks again until it has the outcome. */
interface Landing {
  target: string;
  done: Promise<void>;
  /** null while it runs; the reason it did not land, in words for the person. */
  outcome: { landed: true } | { refused: string } | null;
  endedAt: number;
}

/** By task id. */
const landings = new Map<string, Landing>();

/** For the tests: forgets every landing. */
export function resetLandings() {
  landings.clear();
}

function startLanding(deps: ControlDeps, found: Found, plan: DeliveryPlan): Landing {
  const { ws, task } = found;
  // The PC shows it landing, and its workspace stays awake for it, as with « Fusionner ».
  const end = beginDelivery(ws.id, task.id);
  const landing: Landing = { target: plan.target, done: Promise.resolve(), outcome: null, endedAt: 0 };
  landing.done = landDelivery(ws, task.id, deps)
    .then(
      () => {
        landing.outcome = { landed: true };
      },
      (err) => {
        landing.outcome = { refused: messageOf(err) };
      },
    )
    .finally(() => {
      landing.endedAt = Date.now();
      end();
    });
  landings.set(task.id, landing);
  return landing;
}

async function remoteLand(deps: ControlDeps, args: Record<string, unknown>, sources: DeliverySources): Promise<RemoteLanded> {
  const taskId = taskRef(args);
  let landing = landings.get(taskId);
  if (landing?.outcome && Date.now() - landing.endedAt > LAND_KEPT_MS) {
    landings.delete(taskId);
    landing = undefined;
  }
  if (!landing) {
    const found = await waiting(deps, taskId);
    const plan = await planOf(found);
    sameAs(args, found, plan);
    landing = startLanding(deps, found, plan);
  }
  await Promise.race([landing.done, sources.wait(LAND_WAIT_MS)]);
  const outcome = landing.outcome;
  if (!outcome) return { landed: false, stage: (await stageOf(requireDelivery(deps), taskId)) ?? "queued" };
  // Said once: the next « Fusionner » is a landing of its own.
  landings.delete(taskId);
  if ("refused" in outcome) throw new Error(outcome.refused);
  return { landed: true, target: landing.target };
}

async function remoteRework(deps: ControlDeps, args: Record<string, unknown>, tell: TellWorker): Promise<RemoteReworked> {
  const text = typeof args.text === "string" ? args.text.trim() : "";
  if (!text) throw new Error("Say what to fix: give text.");
  const found = await waiting(deps, taskRef(args));
  const { ws, pane, task, delivery } = found;
  if (typeof args.merge === "string") sameAs(args, found, await planOf(found));
  if (await stageOf(delivery, task.id)) {
    throw new Error(`"${task.title}" is being landed or tested right now: wait for its outcome. Nothing was sent.`);
  }
  await tell(ws, pane, text);
  try {
    await delivery.board.comment(ws.cwd, task.id, "user", `Sent back to ${pane.name} from the phone, to fix before landing: ${text}`);
  } catch (err) {
    throw new Error(`Sent to ${pane.name}, but the comment on the task could not be written: ${messageOf(err)}`);
  }
  return { sent: true };
}

/** The delivery ops of remote_*; any other is not this module's. */
export const DELIVERY_OPS = ["remote_delivery", "remote_delivery_file", "remote_land", "remote_rework"] as const;

export const isDeliveryOp = (op: string) => (DELIVERY_OPS as readonly string[]).includes(op);

/** `deps` is the person's: they ask from their phone (lib/remoteControl). */
export async function runDelivery(
  op: string,
  args: Record<string, unknown>,
  deps: ControlDeps,
  tell: TellWorker,
  sources: DeliverySources = deliverySources,
): Promise<unknown> {
  switch (op) {
    case "remote_delivery":
      return remoteDelivery(deps, args, sources);
    case "remote_delivery_file":
      return remoteDeliveryFile(deps, args, sources);
    case "remote_land":
      return remoteLand(deps, args, sources);
    case "remote_rework":
      return remoteRework(deps, args, tell);
    default:
      throw new Error(`Unknown remote request "${op}".`);
  }
}
