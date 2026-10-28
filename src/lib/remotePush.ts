import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getPaneActivity } from "./agentState";
import type { ManagerEvent, ManagerSink } from "./managerEvents";
import { typeset } from "./typography";
import type { AppState, Workspace } from "../types";

/**
 * Hangar Mobile's push notifications, the window's side: what waits for the
 * person in a project — the four things the project manager is told
 * (lib/managerEvents) — and each file an agent shares with the phone
 * (`remote:shared`), pushed to every subscribed phone by the PC itself
 * (remote_push, src-tauri/src/remote/push.rs), app closed or not.
 *
 * At most one notification per kind and workspace every 30 s: the first
 * waits a few seconds for what comes with it, the next ones wait out the 30 s
 * and go as one ("2 panes wait for you"). A wait that ended before its turn
 * is dropped. Nothing from the HQ. A few words in English and in French, each
 * phone getting its own language; a tap opens the screen it is about: the
 * pane, the shares, the project, For you.
 */

/** A file an agent shared with the phones, as `remote:shared` carries it. */
export interface SharedFile {
  id: string;
  at: number;
  /** The folder of the workspace the agent works in. */
  workspace: string;
  pane: string;
  name: string;
  mime: string;
  size: number;
  caption: string | null;
}

export type PushItem = ManagerEvent | { kind: "shared"; workspaceId: string; workspace: string; share: SharedFile };

export type PushKind = PushItem["kind"];

export interface Localized {
  en: string;
  fr: string;
}

/** What remote_push takes: each phone gets the words of its language. */
export interface PushMessage {
  title: Localized;
  body: Localized;
  /** A screen of the phone app. */
  url: string;
}

export const PUSH_TIMING = {
  /** The least time between two notifications of a kind in a workspace. */
  gapMs: 30_000,
  /** What comes together goes together. */
  gatherMs: 3_000,
};

/** What a notification lists by name before it says "…". */
const NAMED = 3;
const MAX_PENDING = 50;

type Vars = Record<string, string | number>;

/**
 * A notification's words in both languages: the templates typeset, then
 * filled — a task title or a file name is shown as it was written.
 */
function words(en: string, fr: string, vars: Vars = {}): Localized {
  const fill = (text: string) =>
    text.replace(/\{(\w+)\}/g, (all, name: string) => (name in vars ? String(vars[name]) : all));
  return { en: fill(typeset(en, "en")), fr: fill(typeset(fr, "fr")) };
}

/** The phone app's screens (src/mobile/route.ts), relative to where it is served. */
const screen = {
  forYou: "./#/",
  shares: "./#/shares",
  pane: (id: string) => `./#/pane/${encodeURIComponent(id)}`,
  project: (id: string) => `./#/projects/${encodeURIComponent(id)}`,
};

function names(list: string[]): string {
  const unique = [...new Set(list)];
  const shown = unique.slice(0, NAMED).join(", ");
  return unique.length > NAMED ? `${shown}…` : shown;
}

const isImage = (share: SharedFile) => /^image\//.test(share.mime);

/** The notification for a batch of one kind in one workspace. */
export function pushMessage(items: PushItem[]): PushMessage | null {
  const first = items[0];
  if (!first) return null;
  const where = first.workspace;
  const n = items.length;
  switch (first.kind) {
    case "paneWaiting": {
      if (n === 1) {
        return {
          title: words("{pane} waits for you", "{pane} vous attend", { pane: first.pane }),
          body: words("{where}: a permission or a question", "{where} : une permission ou une question", { where }),
          url: screen.pane(first.paneId),
        };
      }
      const list = names(items.map((item) => (item.kind === "paneWaiting" ? item.pane : "")));
      return {
        title: words("{n} panes wait for you", "{n} panneaux vous attendent", { n }),
        body: words("{where}: {list}", "{where} : {list}", { where, list }),
        url: screen.forYou,
      };
    }
    case "taskNeedsYou":
    case "readyToLand": {
      const list = names(items.map((item) => ("title" in item ? item.title : "")));
      const questions = first.kind === "taskNeedsYou";
      const title =
        n === 1
          ? questions
            ? words("A question for you", "Une question pour vous")
            : words("Ready to merge", "Prête à fusionner")
          : questions
            ? words("{n} questions for you", "{n} questions pour vous", { n })
            : words("{n} ready to merge", "{n} prêtes à fusionner", { n });
      return { title, body: words("{where}: {list}", "{where} : {list}", { where, list }), url: screen.forYou };
    }
    case "boardDone": {
      // The latest count is the board's.
      const last = items[items.length - 1];
      const tasks = last.kind === "boardDone" ? last.tasks : 0;
      return {
        title: words("Every task is done", "Toutes les tâches sont faites"),
        body:
          tasks === 1
            ? words("{where}: 1 task on the board", "{where} : 1 tâche au tableau", { where })
            : words("{where}: {n} tasks on the board", "{where} : {n} tâches au tableau", { where, n: tasks }),
        url: screen.project(first.workspaceId),
      };
    }
    case "shared": {
      const shares = items.flatMap((item) => (item.kind === "shared" ? [item.share] : []));
      if (n === 1) {
        const share = shares[0];
        const vars = { pane: share.pane, where, what: share.caption?.trim() || share.name };
        return {
          title: isImage(share)
            ? words("{pane} sent you an image", "{pane} vous a envoyé une image", vars)
            : words("{pane} sent you a file", "{pane} vous a envoyé un fichier", vars),
          body: words("{where}: {what}", "{where} : {what}", vars),
          url: screen.shares,
        };
      }
      const list = names(shares.map((share) => share.name));
      return {
        title: words("{n} files for you", "{n} fichiers pour vous", { n }),
        body: words("{where}: {list}", "{where} : {list}", { where, list }),
        url: screen.shares,
      };
    }
  }
}

const itemKey = (item: PushItem) => {
  switch (item.kind) {
    case "paneWaiting":
      return item.paneId;
    case "taskNeedsYou":
    case "readyToLand":
      return item.taskId;
    case "boardDone":
      return "";
    case "shared":
      return item.share.id;
  }
};

/** A batch as it goes: repeats folded, waits that ended dropped. */
export function settlePush(batch: PushItem[], isWaiting: (paneId: string) => boolean): PushItem[] {
  const seen = new Map<string, PushItem>();
  for (const item of batch) {
    if (item.kind === "paneWaiting" && !isWaiting(item.paneId)) continue;
    const key = itemKey(item);
    // A board done twice says its latest count.
    if (seen.has(key)) seen.delete(key);
    seen.set(key, item);
  }
  return [...seen.values()];
}

const pathKey = (path: string) =>
  path
    .replace(/\\/g, "/")
    .replace(/^\/\/\?\//, "")
    .replace(/\/+$/, "")
    .toLowerCase();

/**
 * The workspace a share came from. `remote:shared` names its folder, which
 * is a workspace's own, or a pane's (a worktree); failing both, the pane's
 * name, when one workspace only has a pane by that name.
 */
export function workspaceOfShare(state: AppState, share: SharedFile): Workspace | null {
  const folder = pathKey(share.workspace);
  const byFolder =
    state.workspaces.find((ws) => pathKey(ws.cwd) === folder) ??
    state.workspaces.find((ws) => ws.panes.some((pane) => pane.cwd && pathKey(pane.cwd) === folder));
  if (byFolder) return byFolder;
  const byPane = state.workspaces.filter((ws) => ws.panes.some((pane) => pane.name === share.pane));
  return byPane.length === 1 ? byPane[0] : null;
}

export interface RemotePushDeps {
  snapshot: () => AppState;
  send: (message: PushMessage) => Promise<unknown>;
  /** How many phones take notifications. */
  subscribers: () => Promise<number>;
  isWaiting: (paneId: string) => boolean;
  now?: () => number;
  setTimer?: (run: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

export interface RemotePush {
  /** The manager feed's second sink. */
  sink: ManagerSink;
  /** An agent shared a file with the phones. */
  shared: (share: SharedFile) => void;
  /** Asks the PC again how many phones listen. */
  refresh: () => Promise<void>;
  stop: () => void;
}

interface Slot {
  lastSentAt: number;
  pending: PushItem[];
  timer: unknown;
}

export function createRemotePush(deps: RemotePushDeps): RemotePush {
  const now = deps.now ?? Date.now;
  const setTimer = deps.setTimer ?? ((run: () => void, ms: number) => setTimeout(run, ms));
  const clearTimer = deps.clearTimer ?? ((handle: unknown) => clearTimeout(handle as ReturnType<typeof setTimeout>));
  const slots = new Map<string, Slot>();
  let subscribers = 0;
  let stopped = false;

  const active = () => !stopped && subscribers > 0;
  const isHq = (workspaceId: string) => deps.snapshot().workspaces.some((ws) => ws.id === workspaceId && ws.hq === true);

  function add(item: PushItem) {
    if (!active() || isHq(item.workspaceId)) return;
    const key = `${item.kind}\n${item.workspaceId}`;
    let slot = slots.get(key);
    if (!slot) {
      slot = { lastSentAt: -Infinity, pending: [], timer: null };
      slots.set(key, slot);
    }
    slot.pending.push(item);
    if (slot.pending.length > MAX_PENDING) slot.pending.splice(0, slot.pending.length - MAX_PENDING);
    if (slot.timer !== null) return;
    const delay = Math.max(PUSH_TIMING.gatherMs, slot.lastSentAt + PUSH_TIMING.gapMs - now());
    const owner = slot;
    owner.timer = setTimer(() => {
      owner.timer = null;
      void fire(owner);
    }, delay);
  }

  async function fire(slot: Slot) {
    const batch = slot.pending;
    slot.pending = [];
    if (!active()) return;
    const message = pushMessage(settlePush(batch, deps.isWaiting));
    if (!message) return;
    slot.lastSentAt = now();
    await deps.send(message).catch(() => undefined);
  }

  function clear() {
    for (const slot of slots.values()) if (slot.timer !== null) clearTimer(slot.timer);
    slots.clear();
  }

  return {
    sink: { active, post: add },
    shared(share) {
      if (!active()) return;
      const ws = workspaceOfShare(deps.snapshot(), share);
      if (ws?.hq) return;
      // A folder no workspace has: named after its last part.
      const fallback = share.workspace.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || share.workspace;
      add({ kind: "shared", workspaceId: ws?.id ?? pathKey(share.workspace), workspace: ws?.name ?? fallback, share });
    },
    async refresh() {
      const count = await deps.subscribers().catch(() => 0);
      if (stopped) return;
      subscribers = count;
      // Nobody listens any more: what waited to go is dropped.
      if (subscribers === 0) clear();
    },
    stop() {
      stopped = true;
      clear();
    },
  };
}

/* ── The window's one push ─────────────────────────────────────── */

let current: RemotePush | null = null;

/**
 * Starts the push for the main window: it hears `remote:shared`, and asks
 * again how many phones listen whenever one subscribes, unsubscribes or is
 * revoked (`remote:changed`) and at each `refresh()` the window's watchers
 * make. Its `sink` goes to the manager feed.
 */
export function startRemotePush(snapshot: () => AppState): RemotePush {
  current?.stop();
  const push = createRemotePush({
    snapshot,
    send: (message) => invoke("remote_push", { title: message.title, body: message.body, url: message.url }),
    subscribers: () => invoke<number>("remote_push_subscribers"),
    isWaiting: (paneId) => getPaneActivity(paneId)?.activity === "waiting",
  });
  const stops = [
    listen<SharedFile>("remote:shared", (event) => push.shared(event.payload)),
    listen("remote:changed", () => void push.refresh()),
  ];
  const started: RemotePush = {
    ...push,
    stop() {
      push.stop();
      for (const stop of stops) stop.then((off) => off()).catch(() => undefined);
      if (current === started) current = null;
    },
  };
  current = started;
  return started;
}
