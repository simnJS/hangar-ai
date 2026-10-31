import type { PaneActivity } from "./agentState";
import {
  kitMayCall,
  KIT_DENIED,
  KIT_EVENTS,
  KIT_METHODS,
  type KitEventName,
  type KitMethod,
} from "./kits";
import type { Pane, Workspace } from "../types";

/**
 * The window's side of a kit tab's message protocol (docs/kits.md): what a
 * kit's page, isolated in its frame (components/preview/KitTabView), may ask
 * Hangar for, and only that.
 *
 * A host is bound to one tab's frame. It answers only a message whose source
 * is that frame's window and whose origin is opaque — the frame is
 * sandboxed, and the scheme's policy sandboxes the page again — checks every
 * method against what the tab may call (its declared permissions that the
 * person granted the kit on the workspace, read afresh at each call), checks
 * every parameter, and reaches the app only through `KitHostDeps`, which a
 * test hands fakes.
 *
 * Messages that are not requests from the frame are dropped without a word:
 * answering them would tell a stranger something.
 */

/** Error codes, beside `denied` (docs/kits.md). */
export const KIT_ERRORS = {
  denied: KIT_DENIED,
  /** The params are not what the method takes. */
  invalid: "invalid",
  /** No such method. */
  unknown: "unknown",
  /** No such pane, or no such file. */
  notFound: "not_found",
  /** Too many calls of this kind lately, or at once. */
  rateLimited: "rate_limited",
  /** Hangar cannot do it now: the pane's agent cannot take a message, the workspace is gone. */
  unavailable: "unavailable",
  /** It was tried, and failed. */
  failed: "failed",
} as const;

export class KitError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/** The longest text `panes.send` carries, in characters. */
export const KIT_SEND_MAX_CHARS = 20_000;
/** At most this many `panes.send` in any `KIT_SEND_WINDOW_MS`. */
export const KIT_SEND_MAX = 10;
export const KIT_SEND_WINDOW_MS = 60_000;
/** At most this many `notify` in any `KIT_NOTIFY_WINDOW_MS`. */
export const KIT_NOTIFY_MAX = 5;
export const KIT_NOTIFY_WINDOW_MS = 60_000;
/** Calls of a tab still waiting for their answer, at most. */
export const KIT_MAX_PENDING = 16;
/** The longest path a tab may give. */
export const KIT_PATH_MAX_CHARS = 1024;
const TITLE_MAX = 120;
const BODY_MAX = 1000;
const ID_MAX = 128;

/** How a tab names who sent a message to an agent (the bridge labels it, never as the person). */
export const kitSender = (kitId: string) => `kit:${kitId}`;

/** The frame's window, as far as the host needs it. */
export interface KitFrame {
  postMessage(message: unknown, targetOrigin: string): void;
}

/** A `message` event, as far as the host reads it. */
export interface KitMessageEvent {
  source: unknown;
  origin: string;
  data: unknown;
}

/** Which tab the host speaks for, read afresh at each call. */
export interface KitHostContext {
  kitId: string;
  kitName: () => string;
  /** The workspace the tab is shown in, as it is now; null once it is gone. */
  workspace: () => Workspace | null;
  /** What the tab may call now (lib/kitTabs › effectivePermissions). */
  permissions: () => readonly string[];
}

/** What the API reaches of the app. */
export interface KitHostDeps {
  /** `kit_file_read`: a text file inside `root`, checked again in Rust. */
  readFile: (root: string, path: string) => Promise<string>;
  /** `kit_workspace_path`: the canonical path of a file inside `root`. */
  resolveFile: (root: string, path: string) => Promise<string>;
  /** Opens the file in the workspace's preview, as opened by `openedBy`. */
  showFile: (ws: Workspace, path: string, openedBy: string) => Promise<void>;
  /** The hangar-bridge queue: submitted once the agent is free. */
  enqueue: (paneId: string, text: string, from: string) => Promise<unknown>;
  activity: (paneId: string) => PaneActivity | undefined;
  notify: (title: string, body: string) => Promise<void>;
  now: () => number;
}

export interface KitHost {
  /** A `message` event of the window: answered when it is a request of this tab's frame. */
  receive: (event: KitMessageEvent) => void;
  /** Tells the tab about an event, when it may hear it. */
  emit: (name: KitEventName, data: unknown) => void;
  /** The tab closed, or left its page: nothing is answered any more. */
  close: () => void;
}

/** A pane as `panes.list` shows it. */
export interface KitPaneInfo {
  name: string;
  /** `claude`, `codex`, `gemini`, `opencode`, or `shell`. */
  agent: string;
  /** `orchestrator`, `worker` or `manager`; null for any other pane. */
  role: string | null;
  /** `working`, `waiting`, `yours` or `idle`; null when Hangar cannot tell. */
  state: string | null;
  /** Whether `panes.send` can reach it now. */
  canSend: boolean;
}

/** A pane `panes.send` may reach: a Claude Code agent whose session runs Hangar's bridge. */
const sendable = (pane: Pane, activity: PaneActivity | undefined) =>
  pane.agent === "claude" && Boolean(activity?.bridged);

/** The workspace's panes, in their order, as a tab sees them. */
export function kitPanes(
  ws: Workspace,
  activity: (paneId: string) => PaneActivity | undefined,
): KitPaneInfo[] {
  return ws.panes.map((pane) => {
    const now = activity(pane.id);
    return {
      name: pane.name,
      agent: pane.agent,
      role: pane.role ?? null,
      state: now?.activity ?? null,
      canSend: sendable(pane, now),
    };
  });
}

/**
 * A path a tab gave, as a path relative to the workspace folder: either
 * slash, no `..`, no drive or stream (`:`), no hidden file or folder — the
 * same rules `kit_file_read` holds on disk. Returns it with `/`.
 */
export function workspaceRelativePath(raw: unknown): string {
  const bad = (why: string) => new KitError(KIT_ERRORS.invalid, `path ${why}`);
  if (typeof raw !== "string" || !raw.trim()) throw bad("must be a non-empty string");
  if (raw.length > KIT_PATH_MAX_CHARS) throw bad(`must be at most ${KIT_PATH_MAX_CHARS} characters`);
  if (raw.includes("\0")) throw bad("must not hold a NUL character");
  if (/^[\\/]/.test(raw)) throw bad("must be relative to the workspace folder");
  const parts: string[] = [];
  for (const part of raw.split(/[\\/]/)) {
    if (part === "" || part === ".") continue;
    if (part === "..") throw bad("must not climb out with ..");
    if (part.includes(":")) throw bad("must be relative to the workspace folder");
    if (part.startsWith(".")) throw bad("must not name a hidden file or folder");
    if (/[. ]$/.test(part)) throw bad("parts must not end with a dot or a space");
    parts.push(part);
  }
  if (!parts.length) throw bad("must name a file inside the workspace folder");
  return parts.join("/");
}

/** At most `max` takes in any `windowMs`. */
export function rateLimiter(max: number, windowMs: number, now: () => number) {
  const taken: number[] = [];
  return () => {
    const at = now();
    while (taken.length && at - taken[0] >= windowMs) taken.shift();
    if (taken.length >= max) return false;
    taken.push(at);
    return true;
  };
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** The params of a method that takes some: an object. */
function paramsOf(params: unknown): Record<string, unknown> {
  if (!isRecord(params)) throw new KitError(KIT_ERRORS.invalid, "params must be an object");
  return params;
}

function stringParam(params: Record<string, unknown>, key: string, max: number, empty = false): string {
  const value = params[key];
  if (typeof value !== "string") throw new KitError(KIT_ERRORS.invalid, `${key} must be a string`);
  if (!empty && !value.trim()) throw new KitError(KIT_ERRORS.invalid, `${key} must not be empty`);
  if (value.length > max) throw new KitError(KIT_ERRORS.invalid, `${key} must be at most ${max} characters`);
  return value;
}

const validId = (id: unknown): id is string | number =>
  (typeof id === "string" && id.length > 0 && id.length <= ID_MAX) ||
  (typeof id === "number" && Number.isFinite(id));

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function createKitHost(
  frame: () => KitFrame | null,
  context: KitHostContext,
  deps: KitHostDeps,
): KitHost {
  let closed = false;
  let pending = 0;
  const mayCall = (method: string) => kitMayCall(method, context.permissions());
  const sendAllowed = rateLimiter(KIT_SEND_MAX, KIT_SEND_WINDOW_MS, deps.now);
  const notifyAllowed = rateLimiter(KIT_NOTIFY_MAX, KIT_NOTIFY_WINDOW_MS, deps.now);

  const post = (target: KitFrame, message: unknown) => {
    if (closed) return;
    try {
      // The frame's origin is opaque: there is no other to name. The window
      // is that frame's, not anyone's who asks.
      target.postMessage(message, "*");
    } catch {
      /* the frame went away */
    }
  };

  /** The workspace, still there. */
  const workspaceNow = () => {
    const ws = context.workspace();
    if (!ws) throw new KitError(KIT_ERRORS.unavailable, "the workspace this tab was shown in is gone");
    return ws;
  };

  const handlers: Record<KitMethod, (params: unknown) => Promise<unknown> | unknown> = {
    "workspace.info": () => {
      const ws = workspaceNow();
      return { id: ws.id, name: ws.name, folder: ws.cwd };
    },

    "panes.list": () => kitPanes(workspaceNow(), deps.activity),

    "panes.send": async (raw) => {
      const params = paramsOf(raw);
      const name = stringParam(params, "pane", 200).trim();
      const text = stringParam(params, "text", KIT_SEND_MAX_CHARS);
      const ws = workspaceNow();
      const pane = ws.panes.find((entry) => entry.name.toLowerCase() === name.toLowerCase());
      if (!pane) throw new KitError(KIT_ERRORS.notFound, `no pane named ${JSON.stringify(name)} in this workspace`);
      if (pane.agent !== "claude") {
        throw new KitError(
          KIT_ERRORS.denied,
          `panes.send only reaches Claude Code agents, and ${pane.name} runs ${pane.agent}`,
        );
      }
      if (!sendable(pane, deps.activity(pane.id))) {
        throw new KitError(
          KIT_ERRORS.unavailable,
          `${pane.name}'s Claude Code is not running with Hangar's bridge, so nothing can wait for it to be free`,
        );
      }
      if (!sendAllowed()) {
        throw new KitError(
          KIT_ERRORS.rateLimited,
          `at most ${KIT_SEND_MAX} panes.send a minute; try again later`,
        );
      }
      await deps.enqueue(pane.id, text, kitSender(context.kitId));
      return { queued: true, pane: pane.name };
    },

    "files.read": async (raw) => {
      const path = workspaceRelativePath(paramsOf(raw).path);
      const ws = workspaceNow();
      try {
        return await deps.readFile(ws.cwd, path);
      } catch (error) {
        throw new KitError(KIT_ERRORS.failed, messageOf(error));
      }
    },

    "preview.show": async (raw) => {
      const path = workspaceRelativePath(paramsOf(raw).path);
      const ws = workspaceNow();
      try {
        const real = await deps.resolveFile(ws.cwd, path);
        await deps.showFile(ws, real, context.kitName());
      } catch (error) {
        throw new KitError(KIT_ERRORS.failed, messageOf(error));
      }
      return { opened: true, path };
    },

    notify: async (raw) => {
      const params = paramsOf(raw);
      const title = stringParam(params, "title", TITLE_MAX).trim();
      const body = params.body === undefined ? "" : stringParam(params, "body", BODY_MAX, true);
      if (!notifyAllowed()) {
        throw new KitError(KIT_ERRORS.rateLimited, `at most ${KIT_NOTIFY_MAX} notify a minute; try again later`);
      }
      // Always under the kit's name: a kit does not speak as Hangar.
      await deps.notify(`${context.kitName()} · ${title}`, body);
      return null;
    },
  };

  async function run(method: string, params: unknown): Promise<unknown> {
    if (!Object.prototype.hasOwnProperty.call(KIT_METHODS, method)) {
      throw new KitError(KIT_ERRORS.unknown, `no method ${JSON.stringify(method)}`);
    }
    if (!mayCall(method)) {
      throw new KitError(
        KIT_ERRORS.denied,
        `this tab was not granted ${KIT_METHODS[method as KitMethod]}`,
      );
    }
    return handlers[method as KitMethod](params);
  }

  return {
    receive(event) {
      if (closed) return;
      const target = frame();
      if (!target || event.source !== target) return;
      if (event.origin !== "null") return;
      const data = event.data;
      if (!isRecord(data) || data.hangar !== 1 || !validId(data.id)) return;
      const id = data.id;
      const answer = (body: { result: unknown } | { error: { code: string; message: string } }) =>
        post(target, { hangar: 1, id, ...body });
      if (typeof data.method !== "string") {
        answer({ error: { code: KIT_ERRORS.invalid, message: "method must be a string" } });
        return;
      }
      if (pending >= KIT_MAX_PENDING) {
        answer({
          error: { code: KIT_ERRORS.rateLimited, message: `at most ${KIT_MAX_PENDING} calls at once` },
        });
        return;
      }
      pending += 1;
      Promise.resolve()
        .then(() => run(data.method as string, data.params))
        .then(
          (result) => answer({ result: result === undefined ? null : result }),
          (error: unknown) =>
            answer({
              error:
                error instanceof KitError
                  ? { code: error.code, message: error.message }
                  : { code: KIT_ERRORS.failed, message: messageOf(error) },
            }),
        )
        .finally(() => {
          pending -= 1;
        });
    },

    emit(name, data) {
      if (closed) return;
      if (!kitMayCallEvent(name, context.permissions())) return;
      const target = frame();
      if (target) post(target, { hangar: 1, event: name, data });
    },

    close() {
      closed = true;
    },
  };
}

/** Whether a tab granted `granted` may hear `event`. */
export function kitMayCallEvent(event: string, granted: readonly string[]): boolean {
  if (!Object.prototype.hasOwnProperty.call(KIT_EVENTS, event)) return false;
  return granted.includes(KIT_EVENTS[event as KitEventName]);
}
