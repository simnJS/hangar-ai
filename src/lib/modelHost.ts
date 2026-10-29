import { emit, listen } from "@tauri-apps/api/event";
import { modelAllowRoots } from "./modelFiles";
import {
  activateModel,
  adoptModelScope,
  closeModels,
  hasSnapshotter,
  modelScope,
  modelView,
  reloadModel,
  requestSnapshot,
  setModelPanel,
  showPreview,
  takeModelScope,
  whenSettled,
  type ModelScope,
  type OpenPreview,
  type PreviewToShow,
  type SnapshotOptions,
} from "./modelStore";

/**
 * A workspace's preview, as the MCP control handlers reach it: its 3D models
 * and every other file it has open.
 *
 * Every window keeps the previews of the workspaces it draws (lib/modelStore),
 * but only the main one carries out the agents' requests (lib/control). A
 * workspace shown in a window of its own has its preview drawn there, so a
 * request about it is forwarded to that window and answered from its store —
 * the same way lib/windows lends the main window a detached pane's terminal.
 *
 * A host is bound to one workspace: it lists, shows, renders and closes that
 * workspace's files and no other's, whichever workspace is on screen.
 *
 * Everything is asynchronous, even here, so the local preview and a remote
 * one are interchangeable behind `ModelHost`.
 */

/** The host decides the workspace. */
export type ModelToShow = PreviewToShow;

export interface ModelList {
  /** Every tab, of every kind. */
  models: OpenPreview[];
  activeId: string | null;
  /** The workspace is the one its window has on screen. */
  onScreen: boolean;
  /** And its preview panel is on screen with it: the file in front can be seen. */
  shown: boolean;
}

export interface ModelHost {
  list: () => Promise<ModelList>;
  /** Opens the file, or reloads it when it is open already; returns its id. */
  show: (file: ModelToShow) => Promise<string>;
  /** Puts the file in front, and the workspace's panel open. */
  reveal: (id: string) => Promise<void>;
  reload: (id: string, modifiedMs: number) => Promise<void>;
  /** The file once it has loaded or failed, or null after `ms`. */
  whenSettled: (id: string, ms: number) => Promise<OpenPreview | null>;
  /** Base64 PNG, without the data: prefix. */
  snapshot: (id: string, options: SnapshotOptions) => Promise<string>;
  close: (ids: string[]) => Promise<void>;
}

/** How long a viewer that is mounting gets to register its snapshotter: it
    loads three.js on demand the first time. */
const MOUNT_WAIT_MS = 4000;
const MOUNT_POLL_MS = 150;

const pause = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

const NOT_HERE = "That file is not open in this workspace.";

/** A workspace's preview, in the window this code runs in. */
export function localModelHost(workspaceId: string): ModelHost {
  const owns = (id: string) => modelScope(workspaceId).models.some((entry) => entry.id === id);
  return {
    list: async () => {
      const { models, activeId } = modelScope(workspaceId);
      const view = modelView();
      const onScreen = view.workspaceId === workspaceId;
      return { models, activeId, onScreen, shown: onScreen && view.panelShown };
    },
    show: async (file) => showPreview({ ...file, workspaceId }),
    reveal: async (id) => {
      if (!owns(id)) throw new Error(NOT_HERE);
      activateModel(id);
      setModelPanel(workspaceId, { open: true });
    },
    reload: async (id, modifiedMs) => {
      if (owns(id)) reloadModel(id, modifiedMs);
    },
    whenSettled: async (id, ms) => (owns(id) ? whenSettled(id, ms) : null),
    snapshot: async (id, options) => {
      // Never another workspace's model, whatever id it is asked for.
      if (!owns(id)) throw new Error(NOT_HERE);
      // The window's viewer may be mounting — for its first model, or after
      // the window changed what it shows: give it the time to register its
      // renderer instead of failing a request it can carry out.
      for (let waited = 0; !hasSnapshotter() && waited < MOUNT_WAIT_MS; waited += MOUNT_POLL_MS) {
        await pause(MOUNT_POLL_MS);
      }
      return requestSnapshot(id, options);
    },
    close: async (ids) => closeModels(ids.filter(owns)),
  };
}

/* ── another window's preview ─────────────────────────────────── */

type Method = keyof ModelHost | "probe";

interface Ask {
  reqId: string;
  workspaceId: string;
  method: Method;
  args: unknown[];
}

interface Answer {
  reqId: string;
  ok: boolean;
  value?: unknown;
  error?: string;
}

/** Whether a window answers at all: short, so a missing one fails fast. */
const PROBE_MS = 1500;
/** Anything that only touches the store. */
const CALL_MS = 5000;
/** A render at up to 2048 px, after the panel had to mount. */
const SNAPSHOT_MS = 30_000;

const NOT_ANSWERING = "The window that shows this workspace stopped answering.";

let asked = 0;

function askWindow(workspaceId: string, method: Method, args: unknown[], ms: number): Promise<unknown> {
  const reqId = `model:${Date.now().toString(36)}:${++asked}`;
  return new Promise((resolve, reject) => {
    let off: (() => void) | null = null;
    let settled = false;
    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      off?.();
      fn();
    };
    const timer = window.setTimeout(() => finish(() => reject(new Error(NOT_ANSWERING))), ms);
    listen<Answer>("model:answer", (event) => {
      const answer = event.payload;
      if (answer.reqId !== reqId) return;
      finish(() =>
        answer.ok ? resolve(answer.value) : reject(new Error(answer.error ?? "The preview failed.")),
      );
    })
      .then((stop) => {
        // Timed out while the listener was being registered.
        if (settled) {
          stop();
          return;
        }
        off = stop;
        return emit("model:ask", { reqId, workspaceId, method, args } satisfies Ask);
      })
      .catch((err) => finish(() => reject(err instanceof Error ? err : new Error(String(err)))));
  });
}

/**
 * The preview of `workspaceId` in the window that shows it, or null when no
 * window answers for it — not open yet, or gone.
 */
export async function remoteModelHost(workspaceId: string): Promise<ModelHost | null> {
  try {
    await askWindow(workspaceId, "probe", [], PROBE_MS);
  } catch {
    return null;
  }
  const call = <T>(method: Method, args: unknown[], ms = CALL_MS) =>
    askWindow(workspaceId, method, args, ms) as Promise<T>;
  return {
    list: () => call("list", []),
    show: (file) => call("show", [file]),
    reveal: (id) => call("reveal", [id]),
    reload: (id, modifiedMs) => call("reload", [id, modifiedMs]),
    // The other window does the waiting; this one only needs to outlast it.
    whenSettled: (id, ms) => call("whenSettled", [id, ms], ms + CALL_MS),
    snapshot: (id, options) => call("snapshot", [id, options], SNAPSHOT_MS),
    close: (ids) => call("close", [ids]),
  };
}

const METHODS = new Set<Method>([
  "probe",
  "list",
  "show",
  "reveal",
  "reload",
  "whenSettled",
  "snapshot",
  "close",
]);

/* ── a workspace moving between windows ───────────────────────── */

/** A workspace's preview, sent to the window that draws it from now on. */
interface Handover {
  /** "main", or the workspace whose own window takes it. */
  to: string;
  workspaceId: string;
  scope: ModelScope;
}

const MAIN = "main";

function listenFor<T>(name: string, handler: (payload: T) => void, ready?: () => void): () => void {
  let off: (() => void) | null = null;
  let stopped = false;
  listen<T>(name, (event) => handler(event.payload))
    .then((stop) => {
      if (stopped) return stop();
      off = stop;
      ready?.();
    })
    .catch(() => undefined);
  return () => {
    stopped = true;
    off?.();
  };
}

const handOver = (handover: Handover) => emit("model:handover", handover).catch(() => undefined);

/**
 * Workspace window: answers the main window's requests about the preview of
 * the workspace it shows, and asks for the files the main window had open
 * for it before it moved here. Returns the unsubscribe.
 */
export function serveModelHost(workspaceId: string): () => void {
  const host = localModelHost(workspaceId);
  const stopAsks = listenFor<Ask>("model:ask", ({ reqId, method, args, ...ask }) => {
    // Not ours: the window of another workspace answers, or nobody does.
    if (ask.workspaceId !== workspaceId || !METHODS.has(method)) return;
    const run =
      method === "probe"
        ? Promise.resolve(true)
        : (host[method] as (...values: unknown[]) => Promise<unknown>)(...args);
    run
      .then((value) => emit("model:answer", { reqId, ok: true, value } satisfies Answer))
      .catch((err) =>
        emit("model:answer", {
          reqId,
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        } satisfies Answer),
      )
      .catch(() => undefined);
  });
  // Asked once listening, or the answer could come back before.
  const stopHandovers = listenFor<Handover>(
    "model:handover",
    (handover) => {
      if (handover.to === workspaceId) adoptModelScope(workspaceId, handover.scope);
    },
    () => void emit("model:claim", { workspaceId }).catch(() => undefined),
  );
  return () => {
    stopAsks();
    stopHandovers();
  };
}

/**
 * Main window: gives a workspace's preview to the window it moved to, when
 * that window asks for it, and takes it back when the workspace returns —
 * its files follow the workspace rather than staying where nobody sees them.
 */
export function serveModelHandovers(): () => void {
  const stopClaims = listenFor<{ workspaceId: string }>("model:claim", ({ workspaceId }) => {
    const scope = takeModelScope(workspaceId);
    if (scope) void handOver({ to: workspaceId, workspaceId, scope });
  });
  const stopReturns = listenFor<Handover>("model:handover", (handover) => {
    if (handover.to === MAIN) adoptModelScope(handover.workspaceId, handover.scope);
  });
  return () => {
    stopClaims();
    stopReturns();
  };
}

/** Workspace window, its workspace going back to the main window: its files go too. */
export function handBackModels(workspaceId: string) {
  const scope = takeModelScope(workspaceId);
  if (scope) void handOver({ to: MAIN, workspaceId, scope });
}

/* ── the folders models are served from ───────────────────────── */

/**
 * Remembers the folders granted to each workspace and sends Rust their union.
 *
 * `model_allow_roots` replaces the whole list, and a model stays open after
 * the call that showed it: the viewer keeps fetching its textures, and polls
 * the file to reload it when it changes. Replacing the list with only the
 * latest caller's folders would cut every other workspace's models off.
 * Workspaces that no longer exist drop out.
 */
export function createRootGrants(send: (roots: string[]) => Promise<void>) {
  const granted = new Map<string, string[]>();
  return (workspaceId: string, roots: string[], live: Iterable<string>): Promise<void> => {
    const keep = new Set(live);
    for (const id of [...granted.keys()]) if (!keep.has(id)) granted.delete(id);
    granted.set(workspaceId, roots);
    const seen = new Set<string>();
    const union: string[] = [];
    for (const root of [...granted.values()].flat()) {
      const key = root.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      union.push(root);
    }
    return send(union);
  };
}

export const grantModelRoots = createRootGrants(modelAllowRoots);
