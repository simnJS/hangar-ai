import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getPaneActivity } from "./agentState";
import { isAsleep } from "./hibernateStore";
import { managerJournal, type ManagerJournalEntry } from "./managerEvents";
import { orchestratorJournal, type JournalEntry } from "./orchestratorEvents";
import { workspaceOfShare, type SharedFile } from "./remotePush";
import { systemLocale, translate, type Locale, type TranslateKey, type TranslateParams } from "../i18n";
import type { RemoteHistoryEntry } from "../remote/types";
import type { AppState } from "../types";

/**
 * What happened in the projects, kept on disk (a file a day for 30 days, in
 * the app data dir) so the phone's "Since your last visit" still answers after
 * Hangar restarted, a week away included. This is the window's side: it
 * writes through `activity_append` and reads through `activity_read`
 * (src-tauri/src/remote/activity.rs, where the rotation, the pruning and the
 * cap live).
 *
 * It detects nothing. It reads what the app already knows: the project
 * manager's journal (a pane that waits, a task that needs the person, a
 * delivery ready, a board done), the orchestrators' journals (deliveries
 * landed or done, tasks moved to done), `remote:shared`, and the workspaces
 * falling asleep or waking. A pane that waited is written again when its wait
 * ends, with how long it lasted. Nothing from the HQ.
 *
 * Each line is {at, workspace, kind, text}, its text in the language of the
 * PC at the time it happened.
 */

export type ActivityEntry = RemoteHistoryEntry;

export const ACTIVITY_KINDS = [
  "landed",
  "delivered",
  "taskDone",
  "needsYou",
  "readyToLand",
  "boardDone",
  "paneWaiting",
  "paneWaited",
  "shared",
  "asleep",
  "awake",
  "appStarted",
  "appStopped",
] as const;
export type ActivityKind = (typeof ACTIVITY_KINDS)[number];

/** How often the journals are read. */
export const ACTIVITY_TICK_MS = 5_000;
/** A line the disk refused is kept this many and tried again. */
const MAX_UNWRITTEN = 200;
/** The same task asking for the person twice within this is one line. */
const SAME_ASK_MS = 10 * 60_000;

/** The words of a line, in the PC's language. */
export type Say = (key: TranslateKey, params?: TranslateParams) => string;

/** "45 s", "12 min", "3 h 05": the same in both languages. */
export function lasted(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")}`;
}

const KIND_WORDS: Partial<Record<ActivityKind, TranslateKey>> = {
  appStarted: "activity.appStarted",
  appStopped: "activity.appStopped",
};

/** A line written without words (Hangar stopping is written by the PC itself) gets them here. */
export function textOf(entry: ActivityEntry, say: Say): string {
  if (entry.text) return entry.text;
  const key = KIND_WORDS[entry.kind as ActivityKind];
  return key ? say(key) : entry.kind;
}

export interface ActivityDeps {
  snapshot: () => AppState;
  append: (entries: ActivityEntry[]) => Promise<unknown>;
  managerJournal?: () => ManagerJournalEntry[];
  orchestratorJournal?: (workspaceId: string) => JournalEntry[];
  isAsleep?: (workspaceId: string) => boolean;
  isWaiting?: (paneId: string) => boolean;
  now?: () => number;
}

export interface ActivityLog {
  /** Reads the journals once and writes what is new. */
  tick: () => Promise<void>;
  /** An agent shared a file with the phones. */
  shared: (share: SharedFile) => void;
  /** Writes what is waiting to be written. */
  flush: () => Promise<void>;
  stop: () => void;
}

const sayIn = (state: AppState): Say => {
  const locale: Locale = state.settings.locale ?? systemLocale();
  return (key, params) => translate(locale, key, params);
};

export function createActivityLog(deps: ActivityDeps): ActivityLog {
  const now = deps.now ?? Date.now;
  const managerEntries = deps.managerJournal ?? managerJournal;
  const orchestratorEntries = deps.orchestratorJournal ?? orchestratorJournal;
  const asleepNow = deps.isAsleep ?? isAsleep;
  const waitingNow = deps.isWaiting ?? ((paneId: string) => getPaneActivity(paneId)?.activity === "waiting");

  /** Written down already, among what the journals still hold. */
  let seen = new Set<string>();
  const asked = new Map<string, number>();
  const waits = new Map<string, { at: number; pane: string; workspace: string }>();
  const slept = new Map<string, boolean>();
  let baseline = true;
  let unwritten: ActivityEntry[] = [];
  let writing: Promise<void> = Promise.resolve();
  let stopped = false;

  const say = () => sayIn(deps.snapshot());
  const isHq = (workspaceId: string) => deps.snapshot().workspaces.some((ws) => ws.id === workspaceId && ws.hq === true);

  function add(entry: ActivityEntry) {
    unwritten.push(entry);
    if (unwritten.length > MAX_UNWRITTEN) unwritten.splice(0, unwritten.length - MAX_UNWRITTEN);
  }

  /** Once for a task asking twice, from the manager's journal and the orchestrator's. */
  function ask(kind: ActivityKind, workspaceId: string, taskId: string, at: number): boolean {
    const key = `${kind}\n${workspaceId}\n${taskId}`;
    const last = asked.get(key);
    if (last !== undefined && Math.abs(at - last) < SAME_ASK_MS) return false;
    asked.set(key, at);
    return true;
  }

  function fromManager(current: Set<string>, t: Say) {
    for (const { at, event } of managerEntries()) {
      const key = `m\n${at}\n${event.kind}\n${event.workspaceId}\n${"taskId" in event ? event.taskId : "paneId" in event ? event.paneId : ""}`;
      current.add(key);
      if (seen.has(key) || isHq(event.workspaceId)) continue;
      const base = { at, workspace: event.workspace };
      switch (event.kind) {
        case "paneWaiting":
          waits.set(event.paneId, { at, pane: event.pane, workspace: event.workspace });
          add({ ...base, kind: "paneWaiting", text: t("activity.paneWaiting", { pane: event.pane }) });
          break;
        case "taskNeedsYou":
          if (ask("needsYou", event.workspaceId, event.taskId, at)) {
            add({ ...base, kind: "needsYou", text: t("activity.needsYou", { title: event.title }) });
          }
          break;
        case "readyToLand":
          if (ask("readyToLand", event.workspaceId, event.taskId, at)) {
            add({ ...base, kind: "readyToLand", text: t("activity.readyToLand", { title: event.title }) });
          }
          break;
        case "boardDone":
          add({ ...base, kind: "boardDone", text: t("activity.boardDone", { n: event.tasks }) });
          break;
      }
    }
  }

  function fromOrchestrators(current: Set<string>, t: Say) {
    for (const ws of deps.snapshot().workspaces) {
      if (ws.hq) continue;
      const journal = orchestratorEntries(ws.id);
      const delivered = new Set(
        journal.flatMap(({ event }) =>
          event.kind === "delivery" && (event.outcome === "landed" || event.outcome === "done") ? [event.taskId] : [],
        ),
      );
      for (const { at, event } of journal) {
        const key = `o\n${ws.id}\n${at}\n${event.kind}\n${"taskId" in event ? event.taskId : ""}`;
        current.add(key);
        if (seen.has(key)) continue;
        const base = { at, workspace: ws.name };
        if (event.kind === "delivery" && (event.outcome === "landed" || event.outcome === "done")) {
          const landed = event.outcome === "landed";
          add({
            ...base,
            kind: landed ? "landed" : "delivered",
            text: t(landed ? "activity.landed" : "activity.delivered", { title: event.title }),
          });
        } else if (event.kind === "taskMoved" && event.to === "done" && !delivered.has(event.taskId)) {
          add({ ...base, kind: "taskDone", text: t("activity.taskDone", { title: event.title }) });
        } else if (event.kind === "taskNeedsYou" && ask("needsYou", ws.id, event.taskId, at)) {
          add({ ...base, kind: "needsYou", text: t("activity.needsYou", { title: event.title }) });
        }
      }
    }
  }

  function waitsEnded(t: Say) {
    for (const [paneId, wait] of waits) {
      if (waitingNow(paneId)) continue;
      waits.delete(paneId);
      const at = now();
      add({
        at,
        workspace: wait.workspace,
        kind: "paneWaited",
        text: t("activity.paneWaited", { pane: wait.pane, time: lasted(at - wait.at) }),
      });
    }
  }

  function sleeps(t: Say) {
    const workspaces = deps.snapshot().workspaces.filter((ws) => !ws.hq);
    const ids = new Set(workspaces.map((ws) => ws.id));
    for (const id of slept.keys()) if (!ids.has(id)) slept.delete(id);
    for (const ws of workspaces) {
      const asleep = asleepNow(ws.id);
      const was = slept.get(ws.id);
      slept.set(ws.id, asleep);
      // What is asleep when Hangar starts is not news; what changes after is.
      if (baseline || was === undefined || was === asleep) continue;
      add({
        at: now(),
        workspace: ws.name,
        kind: asleep ? "asleep" : "awake",
        text: t(asleep ? "activity.asleep" : "activity.awake", { name: ws.name }),
      });
    }
  }

  function write(): Promise<void> {
    // One write after another: lines reach the file in the order they came.
    writing = writing.then(async () => {
      if (unwritten.length === 0) return;
      const batch = unwritten;
      unwritten = [];
      try {
        await deps.append(batch);
      } catch {
        unwritten = [...batch, ...unwritten].slice(-MAX_UNWRITTEN);
      }
    });
    return writing;
  }

  return {
    async tick() {
      if (stopped) return;
      const t = say();
      const current = new Set<string>();
      fromManager(current, t);
      fromOrchestrators(current, t);
      seen = current;
      waitsEnded(t);
      sleeps(t);
      baseline = false;
      await write();
    },
    shared(share) {
      if (stopped) return;
      const state = deps.snapshot();
      const ws = workspaceOfShare(state, share);
      if (ws?.hq) return;
      const fallback = share.workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || share.workspace;
      add({
        at: share.at || now(),
        workspace: ws?.name ?? fallback,
        kind: "shared",
        text: sayIn(state)("activity.shared", { pane: share.pane, name: share.name }),
      });
      void write();
    },
    flush: write,
    stop() {
      stopped = true;
    },
  };
}

/* ── The window's one log ──────────────────────────────────────── */

let current: ActivityLog | null = null;

/**
 * Starts the log for the main window, writing "Hangar started", then reading
 * the journals every few seconds and `remote:shared` as it comes. Returns the
 * function that stops it.
 */
export function startActivityLog(snapshot: () => AppState): () => void {
  current?.stop();
  const log = createActivityLog({
    snapshot,
    append: (entries) => invoke("activity_append", { entries }),
  });
  current = log;
  const state = snapshot();
  void invoke("activity_append", {
    entries: [
      {
        at: Date.now(),
        workspace: "",
        kind: "appStarted",
        text: sayIn(state)("activity.appStarted"),
      },
    ],
  }).catch(() => undefined);
  void log.tick();
  const timer = window.setInterval(() => void log.tick(), ACTIVITY_TICK_MS);
  const off = listen<SharedFile>("remote:shared", (event) => log.shared(event.payload));
  return () => {
    window.clearInterval(timer);
    off.then((stop) => stop()).catch(() => undefined);
    log.stop();
    if (current === log) current = null;
  };
}

/** What remote_history reads: the disk's history, the latest lines included. */
export interface ActivityHistory {
  read: (since: number) => Promise<ActivityEntry[]>;
}

export const activityHistory: ActivityHistory = {
  async read(since) {
    await current?.tick().catch(() => undefined);
    return invoke<ActivityEntry[]>("activity_read", { since });
  },
};
