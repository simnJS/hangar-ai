import type { PaneActivity } from "./agentState";
import { READY_TO_LAND_LABEL, managerOf } from "./orchestration";
import { NEEDS_YOU, TIMING, oneLine, taskRef, type BoardTask } from "./orchestratorEvents";
import type { AppState, Workspace } from "../types";

/**
 * What waits for the person, in any workspace but the HQ, as the project
 * manager hears it (lib/projects). Four things and nothing else: an agent pane
 * waiting for a permission or a question, a task labelled needs-you, a
 * delivery ready to land, a board whose tasks are all done. Moves, comments,
 * worker starts and landings stay with the orchestrators (lib/orchestratorEvents).
 *
 * The batching is the orchestrators' own: a few seconds to gather what comes
 * together, a gap between two messages, held while the manager is in a turn,
 * read again just before it goes (a wait that ended is no longer news), capped.
 * The workspaces need no orchestrator. The manager's end of turn is what
 * raises the Windows notification (TerminalPane), so nothing is sent to the
 * desktop from here.
 *
 * The phone's push notifications (lib/remotePush) are a second sink of the
 * same detection: it runs while a manager runs or a phone is subscribed, and
 * each sink gets what it is owed whether the other exists or not — the push
 * takes every event as it is seen and keeps its own pace, the manager's batch
 * is not held for it, nor it for the manager's turn.
 */
export type ManagerEvent =
  | { kind: "paneWaiting"; workspaceId: string; workspace: string; paneId: string; pane: string; forMs: number }
  | { kind: "taskNeedsYou"; workspaceId: string; workspace: string; taskId: string; title: string; reason: string }
  | { kind: "readyToLand"; workspaceId: string; workspace: string; taskId: string; title: string }
  | { kind: "boardDone"; workspaceId: string; workspace: string; tasks: number };

export interface ManagerJournalEntry {
  at: number;
  event: ManagerEvent;
}

/** A sink beside the manager's pane: it hears every event while `active()` says it listens. */
export interface ManagerSink {
  active: () => boolean;
  post: (event: ManagerEvent) => void;
}

export interface ManagerFeedDeps {
  snapshot: () => AppState;
  enqueue: (paneId: string, text: string, from: string) => Promise<unknown>;
  activity: (paneId: string) => PaneActivity | undefined;
  loadBoard: (cwd: string) => Promise<BoardTask[]>;
  /** The phone's notifications (lib/remotePush). */
  push?: ManagerSink;
  now?: () => number;
  setTimer?: (run: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

const JOURNAL_SIZE = 50;
const MAX_PENDING = 200;
const MAX_LINES = 12;

export function describeManagerEvent(event: ManagerEvent): string {
  const where = `Workspace "${event.workspace}"`;
  switch (event.kind) {
    case "paneWaiting":
      return (
        `${where}: pane "${event.pane}" has been waiting for the person for ` +
        `${Math.max(1, Math.round(event.forMs / 60_000))} min (a permission or a question in its pane)`
      );
    case "taskNeedsYou":
      return `${where}: ${taskRef(event.title, event.taskId)} needs the person: ${event.reason}`;
    case "readyToLand":
      return `${where}: the delivery of ${taskRef(event.title, event.taskId)} is ready to land, it waits for the person`;
    case "boardDone":
      return `${where}: every task on the board is done (${event.tasks})`;
  }
}

const RANK: Record<ManagerEvent["kind"], number> = {
  paneWaiting: 0,
  taskNeedsYou: 1,
  readyToLand: 2,
  boardDone: 3,
};

const keyOf = (event: ManagerEvent) =>
  `${event.kind}\n${event.workspaceId}\n${"paneId" in event ? event.paneId : "taskId" in event ? event.taskId : ""}`;

/** The events of a batch worth sending: repeats folded, waits that ended dropped, most pressing first. */
export function settleManager(batch: ManagerJournalEntry[], isWaiting: (paneId: string) => boolean): ManagerEvent[] {
  const seen = new Set<string>();
  const out: ManagerEvent[] = [];
  for (const { event } of batch) {
    if (event.kind === "paneWaiting" && !isWaiting(event.paneId)) continue;
    const key = keyOf(event);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(event);
  }
  return out
    .map((event, order) => ({ event, order }))
    .sort((a, b) => RANK[a.event.kind] - RANK[b.event.kind] || a.order - b.order)
    .map(({ event }) => event);
}

export function renderManagerDelta(events: ManagerEvent[]): string {
  const shown = events.slice(0, MAX_LINES);
  const lines = shown.map((event) => `- ${oneLine(describeManagerEvent(event), 400)}`);
  const hidden = events.length - shown.length;
  if (hidden > 0) lines.push(`- …and ${hidden} more: the HQ overview has the rest.`);
  const head = events.length === 1 ? "1 thing waits for the person:" : `${events.length} things wait for the person:`;
  return [head, ...lines].join("\n");
}

interface BoardWatch {
  /** Labels of each task as last read. */
  labels: Map<string, string[]>;
  allDone: boolean;
}

interface WaitWatch {
  since: number;
  sent: boolean;
}

export interface ManagerFeed {
  /** Reads the boards, looks at the panes: called every few seconds. */
  tick: () => Promise<void>;
  /** An agent changed a board: read them again. */
  boardChanged: () => void;
  /** Sends what is pending now, waits and holds aside (tests). */
  flush: () => Promise<string | null>;
  journal: () => ManagerJournalEntry[];
  stop: () => void;
}

export function createManagerFeed(deps: ManagerFeedDeps): ManagerFeed {
  const now = deps.now ?? Date.now;
  const setTimer = deps.setTimer ?? ((run: () => void, ms: number) => setTimeout(run, ms));
  const clearTimer = deps.clearTimer ?? ((handle: unknown) => clearTimeout(handle as ReturnType<typeof setTimeout>));
  const boards = new Map<string, BoardWatch>();
  const waits = new Map<string, WaitWatch>();
  const journal: ManagerJournalEntry[] = [];
  let pending: ManagerJournalEntry[] = [];
  let firstPendingAt: number | null = null;
  let lastSentAt = 0;
  let timer: unknown = null;
  let holding = false;
  let boardTimer: unknown = null;
  let reading: Promise<void> | null = null;
  let readAgain = false;
  let stopped = false;

  /** The manager, when it exists and its session runs. */
  const manager = () => {
    const found = managerOf(deps.snapshot());
    return found && deps.activity(found.pane.id) !== undefined ? found : null;
  };

  const pushing = () => deps.push?.active() === true;
  /** Someone to tell: the detection runs only then. */
  const listened = () => pushing() || manager() !== null;

  const watched = (): Workspace[] => deps.snapshot().workspaces.filter((ws) => ws.hq !== true);

  function post(event: ManagerEvent) {
    if (stopped) return;
    const entry = { at: now(), event };
    journal.push(entry);
    if (journal.length > JOURNAL_SIZE) journal.splice(0, journal.length - JOURNAL_SIZE);
    if (pushing()) {
      try {
        deps.push?.post(event);
      } catch {
        // The push's trouble is not the manager's.
      }
    }
    if (!manager()) return;
    pending.push(entry);
    if (pending.length > MAX_PENDING) pending.splice(0, pending.length - MAX_PENDING);
    firstPendingAt ??= entry.at;
    if (!holding) schedule(false);
  }

  function schedule(hold: boolean) {
    if (timer !== null) clearTimer(timer);
    holding = hold;
    const t = now();
    const gap = lastSentAt + TIMING.minGapMs - t;
    const deadline = (firstPendingAt ?? t) + TIMING.maxWaitMs - t;
    const delay = Math.max(hold ? TIMING.holdMs : Math.min(TIMING.debounceMs, deadline), gap, 0);
    timer = setTimer(() => {
      timer = null;
      holding = false;
      void fire();
    }, delay);
  }

  async function fire() {
    if (stopped) return;
    const found = manager();
    if (!found) {
      pending = [];
      firstPendingAt = null;
      return;
    }
    const activity = deps.activity(found.pane.id)?.activity;
    if (activity === "working" || activity === "waiting") {
      schedule(true);
      return;
    }
    await send();
  }

  async function send(): Promise<string | null> {
    const found = manager();
    const batch = pending;
    pending = [];
    firstPendingAt = null;
    if (!found || !batch.length) return null;
    const events = settleManager(batch, (paneId) => deps.activity(paneId)?.activity === "waiting");
    if (!events.length) return null;
    const text = renderManagerDelta(events);
    lastSentAt = now();
    await deps.enqueue(found.pane.id, text, "hangar").catch(() => undefined);
    return text;
  }

  async function readBoard(ws: Workspace) {
    const tasks = await deps.loadBoard(ws.cwd);
    const before = boards.get(ws.id);
    const next: BoardWatch = {
      labels: new Map(tasks.map((task) => [task.id, [...task.labels]])),
      allDone: tasks.length > 0 && tasks.every((task) => task.column === "done"),
    };
    boards.set(ws.id, next);
    // The first read is the baseline: what already waited is not news.
    if (!before) return;
    for (const task of tasks) {
      const was = before.labels.get(task.id);
      if (!was) continue;
      if (task.labels.includes(NEEDS_YOU) && !was.includes(NEEDS_YOU)) {
        const last = task.comments[task.comments.length - 1];
        post({
          kind: "taskNeedsYou",
          workspaceId: ws.id,
          workspace: ws.name,
          taskId: task.id,
          title: task.title,
          reason: last ? `${last.author}: ${oneLine(last.text)}` : `labelled ${NEEDS_YOU}`,
        });
      }
      if (task.labels.includes(READY_TO_LAND_LABEL) && !was.includes(READY_TO_LAND_LABEL)) {
        post({ kind: "readyToLand", workspaceId: ws.id, workspace: ws.name, taskId: task.id, title: task.title });
      }
    }
    if (next.allDone && !before.allDone) {
      post({ kind: "boardDone", workspaceId: ws.id, workspace: ws.name, tasks: tasks.length });
    }
  }

  /** One read of the boards at a time; a change during it reads them once more. */
  function readAll(): Promise<void> {
    if (reading) {
      readAgain = true;
      return reading;
    }
    reading = (async () => {
      do {
        readAgain = false;
        const spaces = watched();
        const ids = new Set(spaces.map((ws) => ws.id));
        for (const id of boards.keys()) if (!ids.has(id)) boards.delete(id);
        await Promise.all(spaces.map((ws) => readBoard(ws).catch(() => undefined)));
      } while (readAgain && !stopped);
    })().finally(() => {
      reading = null;
    });
    return reading;
  }

  function watchPanes() {
    const t = now();
    const present = new Set<string>();
    for (const ws of watched()) {
      for (const pane of ws.panes) {
        if (pane.role === "manager") continue;
        if (deps.activity(pane.id)?.activity !== "waiting") continue;
        present.add(pane.id);
        let w = waits.get(pane.id);
        if (!w) {
          w = { since: t, sent: false };
          waits.set(pane.id, w);
        }
        if (!w.sent && t - w.since >= TIMING.waitingMs) {
          w.sent = true;
          post({
            kind: "paneWaiting",
            workspaceId: ws.id,
            workspace: ws.name,
            paneId: pane.id,
            pane: pane.name,
            forMs: t - w.since,
          });
        }
      }
    }
    for (const id of waits.keys()) if (!present.has(id)) waits.delete(id);
  }

  return {
    async tick() {
      if (stopped) return;
      if (!listened()) {
        // Nobody to tell: forget what was gathered, and start over when there is.
        pending = [];
        firstPendingAt = null;
        boards.clear();
        waits.clear();
        return;
      }
      await readAll();
      watchPanes();
    },
    boardChanged() {
      if (stopped || boardTimer !== null || !listened()) return;
      boardTimer = setTimer(() => {
        boardTimer = null;
        void readAll();
      }, 250);
    },
    async flush() {
      if (timer !== null) {
        clearTimer(timer);
        timer = null;
      }
      return send();
    },
    journal: () => [...journal],
    stop() {
      stopped = true;
      if (timer !== null) clearTimer(timer);
      if (boardTimer !== null) clearTimer(boardTimer);
      boards.clear();
      waits.clear();
    },
  };
}

/* ── The window's one feed ─────────────────────────────────────── */

let feed: ManagerFeed | null = null;

export function startManagerEvents(deps: ManagerFeedDeps): ManagerFeed {
  feed?.stop();
  feed = createManagerFeed(deps);
  return feed;
}

export function stopManagerEvents() {
  feed?.stop();
  feed = null;
}

/** What waited for the person lately, oldest first, for the manager's briefing. */
export function managerJournal(): ManagerJournalEntry[] {
  return feed?.journal() ?? [];
}
