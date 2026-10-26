import { emit, listen } from "@tauri-apps/api/event";
import { modelAllowRoots } from "./modelFiles";
import {
  activateModel,
  closeAllModels,
  closeModel,
  getModels,
  hasSnapshotter,
  reloadModel,
  requestSnapshot,
  setModelPanel,
  showModel,
  whenSettled,
  type OpenModel,
  type SnapshotOptions,
} from "./modelStore";

/**
 * The 3D preview of a window, as the MCP control handlers reach it.
 *
 * Every window keeps its own models (lib/modelStore), but only the main one
 * carries out the agents' requests (lib/control). A workspace shown in a
 * window of its own has its preview drawn there, so a request about it is
 * forwarded to that window and answered from its store — the same way
 * lib/windows lends the main window a detached pane's terminal.
 *
 * Everything is asynchronous, even here, so the local preview and a remote
 * one are interchangeable behind `ModelHost`.
 */

export type ModelToShow = Parameters<typeof showModel>[0];

export interface ModelList {
  models: OpenModel[];
  activeId: string | null;
}

export interface ModelHost {
  list: () => Promise<ModelList>;
  /** Opens the model, or reloads it when it is open already; returns its id. */
  show: (model: ModelToShow) => Promise<string>;
  /** Puts the model in front and the panel on screen. */
  reveal: (id: string) => Promise<void>;
  reload: (id: string, modifiedMs: number) => Promise<void>;
  /** The model once it has loaded or failed, or null after `ms`. */
  whenSettled: (id: string, ms: number) => Promise<OpenModel | null>;
  /** Base64 PNG, without the data: prefix. */
  snapshot: (id: string, options: SnapshotOptions) => Promise<string>;
  close: (ids: string[]) => Promise<void>;
}

/** How long a panel that was just revealed gets to mount and register its
    snapshotter: the viewer loads three.js on demand the first time. */
const MOUNT_WAIT_MS = 4000;
const MOUNT_POLL_MS = 150;

const pause = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

/** The preview of the window this code runs in. */
export const localModelHost: ModelHost = {
  list: async () => {
    const { models, activeId } = getModels();
    return { models, activeId };
  },
  show: async (model) => showModel(model),
  reveal: async (id) => {
    activateModel(id);
    setModelPanel({ open: true });
  },
  reload: async (id, modifiedMs) => reloadModel(id, modifiedMs),
  whenSettled: (id, ms) => whenSettled(id, ms),
  snapshot: async (id, options) => {
    // A panel the user had hidden is mounting again: give it the time to
    // register its renderer instead of failing a request it can carry out.
    for (let waited = 0; !hasSnapshotter() && waited < MOUNT_WAIT_MS; waited += MOUNT_POLL_MS) {
      await pause(MOUNT_POLL_MS);
    }
    return requestSnapshot(id, options);
  },
  close: async (ids) => {
    const open = getModels().models;
    if (ids.length >= open.length && open.every((model) => ids.includes(model.id))) {
      closeAllModels();
      return;
    }
    for (const id of ids) closeModel(id);
  },
};

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
 * The preview of the window that shows `workspaceId`, or null when no window
 * answers for it — not open yet, or gone.
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
    show: (model) => call("show", [model]),
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

/**
 * Workspace window: answers the main window's requests about the models of
 * the workspace it shows. Returns the unsubscribe.
 */
export function serveModelHost(workspaceId: string): () => void {
  let off: (() => void) | null = null;
  let stopped = false;
  listen<Ask>("model:ask", (event) => {
    const { reqId, method, args } = event.payload;
    // Not ours: the window of another workspace answers, or nobody does.
    if (event.payload.workspaceId !== workspaceId || !METHODS.has(method)) return;
    const run =
      method === "probe"
        ? Promise.resolve(true)
        : (localModelHost[method] as (...values: unknown[]) => Promise<unknown>)(...args);
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
  })
    .then((stop) => {
      if (stopped) stop();
      else off = stop;
    })
    .catch(() => undefined);
  return () => {
    stopped = true;
    off?.();
  };
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
