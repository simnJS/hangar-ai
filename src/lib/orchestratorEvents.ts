import type { AgentActivity, PaneActivity } from "./agentState";
import type { BridgeEvent } from "./bridge";
import type { Task } from "./board";
import { NEEDS_YOU_LABEL, orchestratorOf, workersOf } from "./orchestration";
import type { AppState, Pane, Workspace } from "../types";

/**
 * What happens in a workspace that its orchestrator should hear about, posted
 * by the modules that see it happen (workers, deliveries, the board) and
 * handed to the orchestrator pane in batches, as messages from Hangar rather
 * than from the person (lib/orchestration).
 */
export type OrchestratorEvent =
  | { kind: "workerStarted" | "workerStopped" | "workerExited"; worker: string; taskId: string | null }
  | { kind: "workerWaiting"; worker: string; taskId: string | null; forMs: number }
  | { kind: "workerIdle"; worker: string; taskId: string | null; forMs: number }
  | { kind: "taskMoved"; taskId: string; title: string; from: string; to: string; by: string | null }
  | { kind: "taskCommented"; taskId: string; title: string; by: string; text: string }
  | { kind: "taskNeedsYou"; taskId: string; title: string; reason: string }
  | {
      kind: "delivery";
      taskId: string;
      title: string;
      worker: string;
      outcome: "landed" | "ready" | "conflict" | "checkFailed" | "changesRequested" | "dirty" | "stale" | "error";
      detail: string;
    };

/**
 * How the orchestrator hears about its workspace without asking.
 *
 * Events come from two sides: the modules that cause them post them
 * (workerControl, deliveryControl), and this engine makes the rest out of
 * what it watches — the board of every workspace that has an orchestrator,
 * read again whenever an agent changes a board and every few seconds besides
 * (the window's own edits announce nothing), and the activity of its workers.
 *
 * Nothing is sent as it happens. A workspace's events wait a few seconds for
 * the ones that come with them, and for the orchestrator to be between turns;
 * then they go as one message, read again just before — a worker that stopped
 * waiting in the meantime is no longer news — deduplicated, ranked, capped.
 * What the orchestrator did itself, it is not told about. Everything, told or
 * not, is kept in a short journal its briefing hands back after a compaction.
 *
 * A workspace without an orchestrator is never looked at.
 */

/** A board task as orchestration reads it; board.rs leaves both out until set. */
export type BoardTask = Task & { owns?: string[]; attempts?: number };

/** The label of a task that waits for the person. */
export const NEEDS_YOU = NEEDS_YOU_LABEL;

/** Who signs the comments Hangar writes itself (delivery results, say). */
const HANGAR_AUTHOR = /^hangar\b/i;

export interface JournalEntry {
  at: number;
  event: OrchestratorEvent;
  /** Kept for the briefing, never woken for: the orchestrator caused it. */
  quiet: boolean;
}

export const TIMING = {
  /** Quiet this long after the last event, the batch goes. */
  debounceMs: 5_000,
  /** …unless events keep coming: the first one never waits longer. */
  maxWaitMs: 30_000,
  /** Between two messages to one orchestrator, at least. */
  minGapMs: 15_000,
  /** Looked at again this often while the orchestrator is in a turn. */
  holdMs: 3_000,
  /** A worker waiting for the person this long is worth saying. */
  waitingMs: 2 * 60_000,
  /** A worker that ended its turn without delivering, this long ago. */
  idleMs: 10 * 60_000,
  /** Two events about one task this close are one story. */
  nearMs: 60_000,
};

const JOURNAL_SIZE = 50;
const MAX_PENDING = 200;
/** Event lines in one message; the rest is counted, not listed. */
const MAX_LINES = 12;
const MAX_TEXT = 280;
const MAX_DETAIL = 400;

export interface EngineDeps {
  snapshot: () => AppState;
  /** bridgeEnqueue: the pane's mod submits it once the session is idle. */
  enqueue: (paneId: string, text: string, from: string) => Promise<unknown>;
  activity: (paneId: string) => PaneActivity | undefined;
  loadBoard: (cwd: string) => Promise<BoardTask[]>;
  now?: () => number;
  setTimer?: (run: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

/* ── Wording ───────────────────────────────────────────────────── */

const oneLine = (text: string, max = MAX_TEXT) => {
  const flat = text.replace(/\s*\n\s*/g, " / ").replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
};

const minutes = (ms: number) => `${Math.max(1, Math.round(ms / 60_000))} min`;

/** `"Title" [id]`: the title for the reader, the id for the next tool call. */
export const taskRef = (title: string | null | undefined, id: string) =>
  title ? `"${oneLine(title, 120)}" [${id}]` : `[${id}]`;

type Titles = (taskId: string) => string | null;

const onTask = (taskId: string | null, titles: Titles) =>
  taskId ? ` on ${taskRef(titles(taskId), taskId)}` : "";

const DELIVERY: Record<Extract<OrchestratorEvent, { kind: "delivery" }>["outcome"], string> = {
  landed: "landed: merged on the branch, task done",
  ready: "passed its check and review; it waits for the person to land it",
  conflict: "hit a merge conflict; Hangar sent the worker what to fix",
  checkFailed: "failed the check; Hangar sent the worker the output",
  changesRequested: "came back from review with changes requested; Hangar sent the worker the review",
  // The worker's worktree or the person's checkout: the detail says which.
  dirty: "was held back by uncommitted changes",
  stale:
    "was not landed: the worker committed after it was checked and reviewed, so Hangar asked it to submit again",
  error: "failed",
};

/** One event as a line a model reads: who, what, which task, what it means. */
export function describeEvent(event: OrchestratorEvent, titles: Titles): string {
  switch (event.kind) {
    case "delivery": {
      const detail = event.detail.trim() ? ` — ${oneLine(event.detail, MAX_DETAIL)}` : "";
      return `Delivery of ${taskRef(event.title, event.taskId)} by ${event.worker} ${DELIVERY[event.outcome]}${detail}`;
    }
    case "taskNeedsYou":
      return `${taskRef(event.title, event.taskId)} needs the person: ${oneLine(event.reason)}`;
    case "taskCommented": {
      const who = event.by === "user" ? "The person" : event.by;
      return `${who} commented${onTask(event.taskId, () => event.title)}: "${oneLine(event.text)}"`;
    }
    case "taskMoved":
      return `${taskRef(event.title, event.taskId)} moved ${event.from} → ${event.to}${event.by ? ` (by ${event.by})` : ""}`;
    case "workerStarted":
      return `${event.worker} started${onTask(event.taskId, titles)}`;
    case "workerStopped":
      return `${event.worker} was stopped${onTask(event.taskId, titles)}`;
    case "workerExited":
      return `${event.worker}'s session ended${onTask(event.taskId, titles)}; the task is still assigned to it`;
    case "workerWaiting":
      return (
        `${event.worker} has been waiting for the person for ${minutes(event.forMs)}${onTask(event.taskId, titles)}: ` +
        "a permission or a question in its pane, which only the person can answer"
      );
    case "workerIdle":
      return `${event.worker} ended its turn ${minutes(event.forMs)} ago without delivering${onTask(event.taskId, titles)}`;
  }
}

/* ── Batching ──────────────────────────────────────────────────── */

const RANK: Record<OrchestratorEvent["kind"], number> = {
  delivery: 0,
  taskNeedsYou: 1,
  taskCommented: 2,
  workerExited: 3,
  workerWaiting: 4,
  workerIdle: 5,
  taskMoved: 6,
  workerStarted: 7,
  workerStopped: 8,
};

const taskOf = (event: OrchestratorEvent): string | null => ("taskId" in event ? event.taskId : null);
const workerOf = (event: OrchestratorEvent): string | null =>
  "worker" in event ? event.worker : null;

/** What a batch is checked against just before it goes. */
export interface Live {
  /** A worker's activity now, by pane name; undefined once it is gone. */
  activity: (worker: string) => AgentActivity | undefined;
  /** A task's column now; undefined when the board does not have it. */
  column: (taskId: string) => string | undefined;
}

/**
 * The events of a batch worth sending, in the order they matter: the ones
 * the moment has overtaken left out, the ones a more telling event already
 * covers left out, repeats folded into one.
 *
 * `journal` is everything recent, sent or not, the batch included: a move to
 * `doing` is told by the worker it started, and the worker's own event may be
 * in an earlier batch.
 */
export function settle(batch: JournalEntry[], journal: JournalEntry[], live: Live): OrchestratorEvent[] {
  const near = (entry: JournalEntry, kind: OrchestratorEvent["kind"], match: (e: OrchestratorEvent) => boolean) =>
    journal.some(
      (other) =>
        other !== entry &&
        other.event.kind === kind &&
        Math.abs(other.at - entry.at) <= TIMING.nearMs &&
        match(other.event),
    );

  const kept = batch.filter((entry) => {
    const { event } = entry;
    switch (event.kind) {
      case "workerWaiting":
        return live.activity(event.worker) === "waiting";
      case "workerIdle": {
        const now = live.activity(event.worker);
        return now !== "working" && now !== "waiting" && (!event.taskId || live.column(event.taskId) === "doing");
      }
      case "workerExited": {
        // Stopped on purpose: worker_stop says so itself.
        if (near(entry, "workerStopped", (e) => workerOf(e) === event.worker)) return false;
        // Closed because its delivery landed, or off a task no longer in
        // doing: nothing is left hanging on it.
        const column = event.taskId ? live.column(event.taskId) : undefined;
        return column === undefined || column === "doing";
      }
      case "taskMoved": {
        const same = (e: OrchestratorEvent) => taskOf(e) === event.taskId;
        if (event.to === "doing" && near(entry, "workerStarted", same)) return false;
        // Landed, waiting for the person, or back with its worker: the
        // delivery says so, and why.
        if (event.to !== "todo" && near(entry, "delivery", same)) return false;
        if (event.to === "todo" && near(entry, "workerStopped", same)) return false;
        return true;
      }
      case "taskNeedsYou":
        // Posted by whoever labelled it, then seen again on the board.
        return !journal.some(
          (other) =>
            other.at < entry.at &&
            other.event.kind === "taskNeedsYou" &&
            other.event.taskId === event.taskId &&
            entry.at - other.at <= TIMING.nearMs,
        );
      default:
        return true;
    }
  });

  const out: OrchestratorEvent[] = [];
  const seen = new Set<string>();
  const comments = new Map<string, number>();
  for (const { event } of kept) {
    const key = JSON.stringify(event);
    if (seen.has(key)) continue;
    seen.add(key);
    if (event.kind === "taskMoved") {
      // todo → doing → review is one move, todo → review.
      const at = out.findIndex((e) => e.kind === "taskMoved" && e.taskId === event.taskId);
      if (at >= 0) {
        const first = out[at] as Extract<OrchestratorEvent, { kind: "taskMoved" }>;
        out[at] = { ...first, to: event.to, by: first.by === event.by ? first.by : null };
        continue;
      }
    }
    if (event.kind === "workerWaiting" || event.kind === "workerIdle") {
      const at = out.findIndex((e) => e.kind === event.kind && e.worker === event.worker);
      if (at >= 0) {
        out[at] = event;
        continue;
      }
    }
    if (event.kind === "taskCommented") {
      const key = `${event.taskId}\n${event.by}`;
      comments.set(key, (comments.get(key) ?? 0) + 1);
    }
    out.push(event);
  }
  // A worker writing five comments in a row: its last two say where it is.
  for (const [key, count] of comments) {
    let drop = count - 2;
    if (drop <= 0) continue;
    for (let i = 0; i < out.length && drop > 0; i++) {
      const e = out[i];
      if (e.kind === "taskCommented" && `${e.taskId}\n${e.by}` === key) {
        out.splice(i--, 1);
        drop--;
      }
    }
  }
  // "Needs the person: <its last comment>" already quotes that comment.
  const quoted = new Set(
    out.flatMap((e) => (e.kind === "taskNeedsYou" ? [`${e.taskId}\n${e.reason}`] : [])),
  );
  return out
    .filter((e) => !(e.kind === "taskMoved" && e.from === e.to))
    .filter((e) => !(e.kind === "taskCommented" && quoted.has(`${e.taskId}\n${e.by}: ${oneLine(e.text)}`)))
    .map((event, order) => ({ event, order }))
    .sort((a, b) => RANK[a.event.kind] - RANK[b.event.kind] || a.order - b.order)
    .map(({ event }) => event);
}

const isDone = (task: BoardTask | undefined) => task?.column === "done";

/** Tasks nobody holds, in todo, with every dependency done: the next spawns. */
export function readyTasks(tasks: BoardTask[]): BoardTask[] {
  const byId = new Map(tasks.map((task) => [task.id, task]));
  return tasks
    .filter(
      (task) =>
        task.column === "todo" &&
        !task.assignee &&
        !task.labels.includes(NEEDS_YOU) &&
        task.depends_on.every((id) => isDone(byId.get(id))),
    )
    .sort((a, b) => b.priority - a.priority || a.order - b.order);
}

/** The message a batch becomes. */
export function renderDelta(events: OrchestratorEvent[], tasks: BoardTask[], workspace: string): string {
  const titles: Titles = (id) => tasks.find((task) => task.id === id)?.title ?? null;
  const shown = events.slice(0, MAX_LINES);
  const lines = shown.map((event) => `- ${describeEvent(event, titles)}`);
  const hidden = events.length - shown.length;
  if (hidden > 0) lines.push(`- …and ${hidden} more: board_list_tasks and worker_list have the rest.`);
  const head =
    events.length === 1 ? `1 update in workspace "${workspace}":` : `${events.length} updates in workspace "${workspace}":`;
  const out = [head, ...lines];

  const finished = events.some(
    (e) => (e.kind === "delivery" && e.outcome === "landed") || (e.kind === "taskMoved" && e.to === "done"),
  );
  if (finished && tasks.length) {
    const ready = readyTasks(tasks);
    if (tasks.every((task) => task.column === "done")) {
      out.push("Every task on the board is done: check the plan's validation criteria, then report to the person.");
    } else if (ready.length) {
      out.push(`Ready to start: ${ready.slice(0, 6).map((task) => taskRef(task.title, task.id)).join(", ")}.`);
    }
  }
  return out.join("\n");
}

/* ── The board, compared with what it was ──────────────────────── */

export interface TaskSnap {
  column: string;
  labels: string[];
  comments: string[];
}

export function snapBoard(tasks: BoardTask[]): Map<string, TaskSnap> {
  return new Map(
    tasks.map((task) => [
      task.id,
      { column: task.column, labels: [...task.labels], comments: task.comments.map((c) => c.id) },
    ]),
  );
}

const sameName = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/**
 * What changed on a board between two reads, as events. The board does not
 * record who moved a task, so a move made while the orchestrator is in a turn
 * is taken for its own — it is the one moving tasks — and kept quiet; comments
 * are signed, and the orchestrator's and Hangar's own are no news. New and
 * deleted tasks are no event: the orchestrator creates them.
 */
export function diffBoard(
  before: Map<string, TaskSnap>,
  after: BoardTask[],
  orchestrator: { name: string; busy: boolean },
): { event: OrchestratorEvent; quiet: boolean }[] {
  const out: { event: OrchestratorEvent; quiet: boolean }[] = [];
  const self = orchestrator.busy;
  for (const task of after) {
    const was = before.get(task.id);
    if (!was) continue;
    if (was.column !== task.column) {
      out.push({
        event: {
          kind: "taskMoved",
          taskId: task.id,
          title: task.title,
          from: was.column,
          to: task.column,
          by: self ? orchestrator.name : null,
        },
        quiet: self,
      });
    }
    const known = new Set(was.comments);
    for (const comment of task.comments) {
      if (known.has(comment.id)) continue;
      if (sameName(comment.author, orchestrator.name) || HANGAR_AUTHOR.test(comment.author)) continue;
      out.push({
        event: { kind: "taskCommented", taskId: task.id, title: task.title, by: comment.author, text: comment.text },
        quiet: false,
      });
    }
    if (task.labels.includes(NEEDS_YOU) && !was.labels.includes(NEEDS_YOU)) {
      const last = task.comments[task.comments.length - 1];
      out.push({
        event: {
          kind: "taskNeedsYou",
          taskId: task.id,
          title: task.title,
          reason: last ? `${last.author}: ${oneLine(last.text)}` : `labelled ${NEEDS_YOU}`,
        },
        quiet: self,
      });
    }
  }
  return out;
}

/* ── The engine ────────────────────────────────────────────────── */

interface Space {
  journal: JournalEntry[];
  pending: JournalEntry[];
  firstPendingAt: number | null;
  lastSentAt: number;
  timer: unknown;
  /** The batch waits for the orchestrator's turn to end. */
  holding: boolean;
  /** The board as last read; null until the first read, which is the baseline. */
  board: Map<string, TaskSnap> | null;
  tasks: BoardTask[];
  reading: Promise<void> | null;
  readAgain: boolean;
}

interface WorkerWatch {
  workspaceId: string;
  name: string;
  taskId: string | null;
  activity: AgentActivity;
  since: number;
  /** The worker had a turn and handed back; not merely never started. */
  afterTurn: boolean;
  waitingSent: boolean;
  idleSent: boolean;
  exitSent: boolean;
}

export interface OrchestratorEngine {
  post: (workspaceId: string, event: OrchestratorEvent, quiet?: boolean) => void;
  /** An agent changed a board: read the watched ones again. */
  boardChanged: () => void;
  bridgeEvent: (paneId: string, event: BridgeEvent) => void;
  ptyExited: (paneId: string) => void;
  /** Reads the boards, looks at the workers: called every few seconds. */
  tick: () => Promise<void>;
  /** Sends what is pending now, waits and holds aside (tests). */
  flush: (workspaceId: string) => Promise<string | null>;
  journal: (workspaceId: string) => JournalEntry[];
  tasks: (workspaceId: string) => BoardTask[];
  stop: () => void;
}

export function createOrchestratorEngine(deps: EngineDeps): OrchestratorEngine {
  const now = deps.now ?? Date.now;
  const setTimer = deps.setTimer ?? ((run: () => void, ms: number) => setTimeout(run, ms));
  const clearTimer = deps.clearTimer ?? ((handle: unknown) => clearTimeout(handle as ReturnType<typeof setTimeout>));
  const spaces = new Map<string, Space>();
  const workers = new Map<string, WorkerWatch>();
  let boardTimer: unknown = null;
  let stopped = false;

  const space = (id: string): Space => {
    let found = spaces.get(id);
    if (!found) {
      found = {
        journal: [],
        pending: [],
        firstPendingAt: null,
        lastSentAt: 0,
        timer: null,
        holding: false,
        board: null,
        tasks: [],
        reading: null,
        readAgain: false,
      };
      spaces.set(id, found);
    }
    return found;
  };

  const workspace = (id: string): Workspace | undefined =>
    deps.snapshot().workspaces.find((ws) => ws.id === id);

  const orchestrated = (): { ws: Workspace; orchestrator: Pane }[] =>
    deps.snapshot().workspaces.flatMap((ws) => {
      const orchestrator = orchestratorOf(ws);
      return orchestrator ? [{ ws, orchestrator }] : [];
    });

  /**
   * Where each pane's session stands, from its mod's own reports. The pane's
   * activity is the fallback only: a session that has just started keeps the
   * "working" its startup output suggested until its first turn, and events
   * held for it would wait for the person to type something.
   */
  const turns = new Map<string, { inTurn: boolean; waiting: boolean }>();

  const inTurn = (pane: Pane) => {
    const turn = turns.get(pane.id);
    return turn ? turn.inTurn : deps.activity(pane.id)?.activity === "working";
  };

  const busy = (pane: Pane) => {
    const turn = turns.get(pane.id);
    if (turn) return turn.inTurn || turn.waiting;
    const activity = deps.activity(pane.id)?.activity;
    return activity === "working" || activity === "waiting";
  };

  function post(workspaceId: string, event: OrchestratorEvent, quiet = false) {
    if (stopped) return;
    const ws = workspace(workspaceId);
    const orchestrator = ws ? orchestratorOf(ws) : null;
    if (!ws || !orchestrator) return;
    // Starting and stopping workers is the orchestrator's own doing when it
    // happens during its turn: it already has the tool's answer.
    const own = (event.kind === "workerStarted" || event.kind === "workerStopped") && inTurn(orchestrator);
    const entry: JournalEntry = { at: now(), event, quiet: quiet || own };
    const s = space(workspaceId);
    s.journal.push(entry);
    if (s.journal.length > JOURNAL_SIZE) s.journal.splice(0, s.journal.length - JOURNAL_SIZE);
    if (entry.quiet) return;
    s.pending.push(entry);
    if (s.pending.length > MAX_PENDING) s.pending.splice(0, s.pending.length - MAX_PENDING);
    s.firstPendingAt ??= entry.at;
    // Already held for the orchestrator's turn: the hold looks again soon.
    if (!s.holding) schedule(workspaceId, false);
  }

  /**
   * The next look at a workspace's batch: once things quieten (never later
   * than the first event's deadline), or — `hold` — a few seconds on, while
   * the orchestrator is in a turn. Never closer than the gap to the last
   * message.
   */
  function schedule(workspaceId: string, hold: boolean) {
    const s = space(workspaceId);
    if (s.timer !== null) clearTimer(s.timer);
    s.holding = hold;
    const t = now();
    const gap = s.lastSentAt + TIMING.minGapMs - t;
    const deadline = (s.firstPendingAt ?? t) + TIMING.maxWaitMs - t;
    const delay = Math.max(hold ? TIMING.holdMs : Math.min(TIMING.debounceMs, deadline), gap, 0);
    s.timer = setTimer(() => {
      s.timer = null;
      s.holding = false;
      void fire(workspaceId);
    }, delay);
  }

  /** The batch goes, or waits for the orchestrator's turn to end. */
  async function fire(workspaceId: string) {
    if (stopped) return;
    const ws = workspace(workspaceId);
    const orchestrator = ws ? orchestratorOf(ws) : null;
    const s = space(workspaceId);
    if (!ws || !orchestrator) {
      s.pending = [];
      s.firstPendingAt = null;
      return;
    }
    // Queued now, it would sit in the pane's queue while the news aged; held
    // here, what comes meanwhile joins it and the whole is read again later.
    if (busy(orchestrator)) {
      schedule(workspaceId, true);
      return;
    }
    await send(ws, orchestrator);
  }

  async function send(ws: Workspace, orchestrator: Pane): Promise<string | null> {
    const s = space(ws.id);
    // The board as it is now: a landing moved its task, and the next spawns
    // are read off it.
    await read(ws).catch(() => undefined);
    const batch = s.pending;
    s.pending = [];
    s.firstPendingAt = null;
    if (!batch.length) return null;
    const live = liveOf(ws, s);
    const events = settle(batch, s.journal, live);
    if (!events.length) return null;
    const text = renderDelta(events, s.tasks, ws.name);
    s.lastSentAt = now();
    await deps.enqueue(orchestrator.id, text, "hangar").catch(() => undefined);
    return text;
  }

  function liveOf(ws: Workspace, s: Space): Live {
    return {
      activity: (worker) => {
        const pane = ws.panes.find((p) => p.role === "worker" && sameName(p.name, worker));
        if (!pane) return undefined;
        return deps.activity(pane.id)?.activity ?? "idle";
      },
      column: (taskId) => s.tasks.find((task) => task.id === taskId)?.column,
    };
  }

  /** One read of a board at a time; a change during it reads it once more. */
  function read(ws: Workspace): Promise<void> {
    const s = space(ws.id);
    if (s.reading) {
      s.readAgain = true;
      return s.reading;
    }
    s.reading = (async () => {
      do {
        s.readAgain = false;
        const tasks = await deps.loadBoard(ws.cwd);
        const current = workspace(ws.id);
        const orchestrator = current ? orchestratorOf(current) : null;
        if (!orchestrator) return;
        if (s.board) {
          const changes = diffBoard(s.board, tasks, { name: orchestrator.name, busy: inTurn(orchestrator) });
          s.board = snapBoard(tasks);
          s.tasks = tasks;
          for (const change of changes) post(ws.id, change.event, change.quiet);
        } else {
          s.board = snapBoard(tasks);
          s.tasks = tasks;
        }
      } while (s.readAgain);
    })().finally(() => {
      s.reading = null;
    });
    return s.reading;
  }

  async function readAll() {
    await Promise.all(orchestrated().map(({ ws }) => read(ws).catch(() => undefined)));
  }

  function watchWorkers() {
    const t = now();
    const present = new Set<string>();
    for (const { ws } of orchestrated()) {
      const s = space(ws.id);
      for (const pane of workersOf(ws)) {
        present.add(pane.id);
        const activity = deps.activity(pane.id)?.activity ?? "idle";
        const handedBack = activity === "yours" || activity === "idle";
        let w = workers.get(pane.id);
        if (!w) {
          w = {
            workspaceId: ws.id,
            name: pane.name,
            taskId: pane.taskId ?? null,
            activity,
            since: t,
            afterTurn: activity === "yours",
            waitingSent: false,
            idleSent: false,
            exitSent: false,
          };
          workers.set(pane.id, w);
        }
        w.name = pane.name;
        w.taskId = pane.taskId ?? null;
        if (activity !== w.activity) {
          const wasInTurn = w.activity === "working" || w.activity === "waiting";
          w.afterTurn = handedBack && (wasInTurn || w.afterTurn);
          w.activity = activity;
          w.since = t;
          w.waitingSent = false;
          w.idleSent = false;
        }
        const forMs = t - w.since;
        if (activity === "waiting" && !w.waitingSent && forMs >= TIMING.waitingMs) {
          w.waitingSent = true;
          post(ws.id, { kind: "workerWaiting", worker: pane.name, taskId: w.taskId, forMs });
        }
        if (handedBack && w.afterTurn && !w.idleSent && forMs >= TIMING.idleMs) {
          const task = w.taskId ? s.tasks.find((entry) => entry.id === w!.taskId) : undefined;
          const since = w.since;
          const delivered = s.journal.some(
            (entry) => entry.at >= since && entry.event.kind === "delivery" && entry.event.taskId === w!.taskId,
          );
          // A task that left `doing`, or waits for the person, is not idling.
          const open = !task || (task.column === "doing" && !task.labels.includes(NEEDS_YOU));
          if (!delivered && open) {
            w.idleSent = true;
            post(ws.id, { kind: "workerIdle", worker: pane.name, taskId: w.taskId, forMs });
          }
        }
      }
    }
    // A worker pane gone without worker_stop — closed by hand — exited. One
    // that restarted under a new id is still there by name.
    for (const [paneId, w] of workers) {
      if (present.has(paneId)) continue;
      workers.delete(paneId);
      const ws = workspace(w.workspaceId);
      if (!ws || !orchestratorOf(ws) || w.exitSent) continue;
      if (ws.panes.some((pane) => pane.role === "worker" && sameName(pane.name, w.name))) continue;
      post(ws.id, { kind: "workerExited", worker: w.name, taskId: w.taskId });
    }
  }

  function exited(paneId: string) {
    for (const { ws } of orchestrated()) {
      const pane = workersOf(ws).find((p) => p.id === paneId);
      if (!pane) continue;
      const w = workers.get(paneId);
      if (w?.exitSent) return;
      if (w) w.exitSent = true;
      else {
        workers.set(paneId, {
          workspaceId: ws.id,
          name: pane.name,
          taskId: pane.taskId ?? null,
          activity: "idle",
          since: now(),
          afterTurn: false,
          waitingSent: false,
          idleSent: false,
          exitSent: true,
        });
      }
      post(ws.id, { kind: "workerExited", worker: pane.name, taskId: pane.taskId ?? null });
      return;
    }
  }

  return {
    post,
    boardChanged() {
      if (stopped || boardTimer !== null) return;
      // A tool call often writes twice (claim, then comment): read once.
      boardTimer = setTimer(() => {
        boardTimer = null;
        void readAll();
      }, 250);
    },
    bridgeEvent(paneId, event) {
      const turn = turns.get(paneId);
      switch (event.kind) {
        case "hello": {
          turns.set(paneId, { inTurn: false, waiting: false });
          const w = workers.get(paneId);
          if (w) w.exitSent = false;
          break;
        }
        case "turn":
          turns.set(paneId, { inTurn: event.phase === "start", waiting: false });
          break;
        case "permission":
          if (turn) turn.waiting = event.waiting;
          break;
        case "end":
          turns.delete(paneId);
          exited(paneId);
          break;
      }
    },
    ptyExited(paneId) {
      turns.delete(paneId);
      exited(paneId);
    },
    async tick() {
      if (stopped) return;
      if (!orchestrated().length) return;
      await readAll();
      watchWorkers();
    },
    async flush(workspaceId) {
      const ws = workspace(workspaceId);
      const orchestrator = ws ? orchestratorOf(ws) : null;
      const s = space(workspaceId);
      if (s.timer !== null) {
        clearTimer(s.timer);
        s.timer = null;
      }
      if (!ws || !orchestrator) return null;
      return send(ws, orchestrator);
    },
    journal: (workspaceId) => [...(spaces.get(workspaceId)?.journal ?? [])],
    tasks: (workspaceId) => spaces.get(workspaceId)?.tasks ?? [],
    stop() {
      stopped = true;
      for (const s of spaces.values()) if (s.timer !== null) clearTimer(s.timer);
      if (boardTimer !== null) clearTimer(boardTimer);
      spaces.clear();
      workers.clear();
      turns.clear();
    },
  };
}

/* ── The window's one engine ───────────────────────────────────── */

let engine: OrchestratorEngine | null = null;

/** Started by the main window once its store is loaded (orchestratorBrief). */
export function startOrchestratorEvents(deps: EngineDeps): OrchestratorEngine {
  engine?.stop();
  engine = createOrchestratorEngine(deps);
  return engine;
}

export function stopOrchestratorEvents() {
  engine?.stop();
  engine = null;
}

/** Hands `event` to the workspace's orchestrator, if it has one. */
export function postOrchestratorEvent(workspaceId: string, event: OrchestratorEvent): void {
  engine?.post(workspaceId, event);
}

/** What happened lately in the workspace, oldest first, for its briefing. */
export function orchestratorJournal(workspaceId: string): JournalEntry[] {
  return engine?.journal(workspaceId) ?? [];
}
