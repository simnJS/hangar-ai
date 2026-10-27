import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PaneActivity } from "./agentState";
import type { Worktree } from "./git";
import { leafIds, MAX_PANES, normalizeTree } from "./layout";
import { MAX_MODEL_BYTES, MODEL_FORMATS, modelFormatOf, type ModelFileInfo } from "./modelFiles";
import type { ModelHost } from "./modelHost";
import type { OpenModel, SnapshotAngle } from "./modelStore";
import { AGENTS, type AgentId, type AppState, type Pane, type Workspace } from "../types";
import { isMainWindow, type PaneTerminal } from "./windows";
import { workerControl } from "./workerControl";
import { deliveryControl } from "./deliveryControl";

/**
 * Agents driving the panes through the MCP server (see src-tauri/src/control.rs).
 *
 * Rust relays each request as a `control:request` event; the main window,
 * which owns the store, carries it out here and answers with `control_reply`.
 * Everything that can be decided from the state is decided in `runControl`,
 * which only reaches the app through `ControlDeps` — a test hands it fakes.
 *
 * The messages thrown below are read by a model, not by the user, so they are
 * English and say what to do next.
 */

export interface ControlRequest {
  id: string;
  op: string;
  args: Record<string, unknown>;
}

/** Who is asking, from the environment of the pane the agent runs in. */
export interface Caller {
  paneId: string | null;
  paneName: string | null;
  cwd: string | null;
}

export type ControlEvent =
  | { kind: "created" | "closed" | "restarted" | "reset"; by: string | null; pane: string; workspace: string }
  | {
      /** "Elsewhere": in a workspace that is not the one on screen, which the line names. */
      kind: "modelShown" | "modelClosed" | "modelShownElsewhere" | "modelClosedElsewhere";
      by: string | null;
      file: string;
      workspace: string;
    };

export interface ControlDeps {
  snapshot: () => AppState;
  enabled: () => boolean;
  addPane: (
    workspaceId: string,
    opts: { agent?: AgentId; near?: string | null; dir?: "row" | "col"; cwd?: string | null; name?: string },
  ) => string | null;
  closePane: (workspaceId: string, paneId: string) => void;
  /** Restarts under a fresh id, which it returns. */
  respawnPane: (workspaceId: string, paneId: string, patch: Partial<Pane>) => string | null;
  /** Mounts a workspace's terminals without switching to it. */
  openWorkspace: (workspaceId: string) => void;
  isOpen: (workspaceId: string) => boolean;
  /** Put to sleep for being left alone (lib/hibernate); opening it wakes it. */
  asleep: (workspaceId: string) => boolean;
  /** An agent acting on a workspace is using it: its sleep waits. */
  touch: (workspaceId: string) => void;
  /** Switches the window to the workspace and focuses the pane. */
  activate: (workspaceId: string, paneId: string) => void;
  /** The pane's terminal, in this window or in the one that draws it. */
  terminal: (paneId: string) => Promise<PaneTerminal | null>;
  write: (paneId: string, data: string) => Promise<void>;
  alive: (paneId: string) => Promise<boolean>;
  activity: (paneId: string) => PaneActivity | undefined;
  /** The hangar-bridge queue: submitted once the Claude Code session is idle. */
  enqueue: (paneId: string, text: string) => Promise<unknown>;
  dirExists: (path: string) => Promise<boolean>;
  /** Whether workspaces can go to windows of their own at all. */
  canDetach: () => boolean;
  /** Moves a workspace to a window of its own, or back into the main one. */
  detach: (workspaceId: string) => Promise<void>;
  reattach: (workspaceId: string) => void;
  announce: (event: ControlEvent) => void;
  sleep: (ms: number) => Promise<void>;
  /**
   * Lets Rust serve models from these folders on behalf of this workspace,
   * keeping what the other workspaces were granted (lib/modelHost).
   */
  modelAllowRoots: (workspaceId: string, roots: string[]) => Promise<void>;
  /** Canonical path, size and mtime; rejects outside the allowed folders. */
  modelStat: (path: string) => Promise<ModelFileInfo>;
  /** Every worktree of the repository holding `path`; rejects outside one. */
  worktrees: (path: string) => Promise<Worktree[]>;
  /**
   * A workspace's 3D preview: in this window, or in the window of its own a
   * detached workspace is shown in; null when that window does not answer.
   */
  modelHost: (workspaceId: string, detached: boolean) => Promise<ModelHost | null>;
  // orchestration: workers deps
  // orchestration: delivery deps
  /** Merges, checks, reviews and lands workers' deliveries (lib/deliveryControl). */
  delivery?: import("./deliveryControl").DeliveryDeps;
}

export const DISABLED =
  "Pane control is turned off in Hangar.AI (Settings › General › Agents). Ask the user to turn it on.";

/** Said wherever a sleeping workspace is the reason nothing runs. */
const ASLEEP =
  "Hangar put it to sleep to free memory, as nobody had used it for a while: its agents were stopped, " +
  "and resume their sessions as soon as it opens. pane_send, pane_create and pane_restart open it.";

const AGENT_IDS = AGENTS.map((agent) => agent.id) as string[];

const KEYS: Record<string, string> = {
  enter: "\r",
  escape: "\x1b",
  tab: "\t",
  "shift+tab": "\x1b[Z",
  backspace: "\x7f",
  "ctrl+c": "\x03",
  "ctrl+d": "\x04",
};

/** Arrows as the terminal itself would send them: an application that turned
    on cursor-key mode expects ESC O, everything else ESC [. */
const ARROWS: Record<string, string> = { up: "A", down: "B", right: "C", left: "D" };

const READ_DEFAULT = 60;
const READ_MAX = 1000;

/** Same folder, the way Windows compares them: either separator, any case. */
const normalize = (path: string) =>
  path.replace(/[\\/]+$/, "").replace(/\\/g, "/").toLowerCase();

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

const text = (value: unknown) =>
  typeof value === "string" && value.trim() !== "" ? value.trim() : null;

function resolveFolder(root: string, path: string): string {
  if (/^([a-zA-Z]:[\\/]|[\\/])/.test(path)) return path;
  const separator = root.includes("\\") ? "\\" : "/";
  const relative = path.replace(/^\.[\\/]/, "").replace(/[\\/]+/g, separator);
  return `${root.replace(/[\\/]+$/, "")}${separator}${relative}`;
}

function readCaller(args: Record<string, unknown>): Caller {
  const raw = (args.caller ?? {}) as Record<string, unknown>;
  return {
    paneId: text(raw.paneId),
    paneName: text(raw.paneName),
    cwd: text(raw.cwd) ?? text(args.callerCwd),
  };
}

const effectiveCwd = (ws: Workspace, pane: Pane) => pane.cwd || ws.cwd;

/** Panes in reading order, which is what "the last one" means on screen. */
const ordered = (ws: Workspace): Pane[] => {
  const byId = new Map(ws.panes.map((pane) => [pane.id, pane]));
  return leafIds(normalizeTree(ws.panes, ws.tree))
    .map((id) => byId.get(id))
    .filter((pane): pane is Pane => Boolean(pane));
};

/** The workspace the agent is working in: its pane if Hangar launched it,
    otherwise the folder it runs in. */
export function callerWorkspace(state: AppState, caller: Caller): Workspace | null {
  if (caller.paneId) {
    const owner = state.workspaces.find((ws) => ws.panes.some((p) => p.id === caller.paneId));
    if (owner) return owner;
  }
  if (!caller.cwd) return null;
  const here = normalize(caller.cwd);
  const exact = state.workspaces.filter(
    (ws) =>
      normalize(ws.cwd) === here || ws.panes.some((p) => p.cwd && normalize(p.cwd) === here),
  );
  if (exact.length > 1) {
    // Two workspaces on one folder: the pane name tells them apart, then
    // whichever the user is looking at.
    const named =
      caller.paneName &&
      exact.find((ws) => ws.panes.some((p) => same(p.name, caller.paneName as string)));
    return named || exact.find((ws) => ws.id === state.activeWorkspaceId) || exact[0];
  }
  if (exact.length === 1) return exact[0];
  const inside = state.workspaces
    .filter((ws) => here.startsWith(`${normalize(ws.cwd)}/`))
    .sort((a, b) => b.cwd.length - a.cwd.length);
  return inside[0] ?? null;
}

function workspaceFor(state: AppState, args: Record<string, unknown>, caller: Caller): Workspace {
  const names = state.workspaces.map((ws) => `"${ws.name}"`).join(", ") || "none";
  const wanted = text(args.workspace);
  if (wanted) {
    const found =
      state.workspaces.find((ws) => ws.id === wanted) ??
      state.workspaces.find((ws) => same(ws.name, wanted)) ??
      state.workspaces.find((ws) => normalize(ws.cwd) === normalize(wanted));
    if (!found) throw new Error(`No workspace "${wanted}". Workspaces: ${names}.`);
    return found;
  }
  const mine = callerWorkspace(state, caller);
  // Pane names are unique across workspaces, so a pane named without its
  // workspace is found wherever it is — the caller's own workspace first.
  const pane = text(args.pane);
  if (pane && !(mine && hasPane(mine, pane))) {
    const holders = state.workspaces.filter((ws) => hasPane(ws, pane));
    if (holders.length === 1) return holders[0];
  }
  if (!mine) {
    throw new Error(
      `Could not tell which workspace you are in. Pass "workspace", one of: ${names}.`,
    );
  }
  return mine;
}

const hasPane = (ws: Workspace, ref: string) =>
  ws.panes.some((pane) => pane.id === ref || same(pane.name, ref));

function paneIn(ws: Workspace, ref: unknown): Pane {
  const wanted = text(ref);
  const names = ordered(ws).map((pane) => `"${pane.name}"`).join(", ");
  if (!wanted) throw new Error(`Name the pane. Panes of "${ws.name}": ${names}.`);
  const found =
    ws.panes.find((pane) => pane.id === wanted) ?? ws.panes.find((pane) => same(pane.name, wanted));
  if (!found) throw new Error(`No pane "${wanted}" in "${ws.name}". Panes: ${names}.`);
  return found;
}

function agentArg(value: unknown): AgentId | undefined {
  const wanted = text(value);
  if (!wanted) return undefined;
  if (!AGENT_IDS.includes(wanted)) {
    throw new Error(`Unknown agent "${wanted}". One of: ${AGENT_IDS.join(", ")}.`);
  }
  return wanted as AgentId;
}

function keySequence(name: string, applicationCursor: boolean): string {
  const arrow = ARROWS[name];
  if (arrow) return `\x1b${applicationCursor ? "O" : "["}${arrow}`;
  const key = KEYS[name];
  if (!key) {
    throw new Error(
      `Unknown key "${name}". One of: ${[...Object.keys(KEYS), ...Object.keys(ARROWS)].join(", ")}.`,
    );
  }
  return key;
}

/* ── 3D models ─────────────────────────────────────────────────── */

/** How long model_show waits for the viewer before answering "loading". The
    MCP side gives a model_* call 60 s in all. */
const SETTLE_MS = 45_000;
/** Shorter for a snapshot, which still has to render once the load is done. */
const SNAPSHOT_SETTLE_MS = 40_000;

const ANGLES: SnapshotAngle[] = ["front", "back", "left", "right", "top", "bottom", "iso"];
const SNAPSHOT_WIDTH = 1024;
const SNAPSHOT_HEIGHT = 768;
const SNAPSHOT_MIN = 64;
const SNAPSHOT_MAX = 2048;

const messageOf = (err: unknown) => (err instanceof Error ? err.message : String(err));

/**
 * `path` read from `base`, with `.` and `..` folded away so that a check on
 * the result cannot be walked out of. A Windows path keeps backslashes.
 */
export function resolvePath(base: string, path: string): string {
  const full = /^([a-zA-Z]:[\\/]|[\\/])/.test(path) ? path : `${base}/${path}`;
  const unc = /^[\\/]{2}[^\\/]/.test(full);
  const [head, ...rest] = full.split(/[\\/]+/);
  const separator = unc || /^[a-zA-Z]:$/.test(head) ? "\\" : "/";
  const parts: string[] = [];
  for (const part of rest) {
    if (part === "" || part === ".") continue;
    if (part === "..") parts.pop();
    else parts.push(part);
  }
  return `${unc ? separator : ""}${[head, ...parts].join(separator)}`;
}

/** For comparing: Rust's canonical paths carry Windows' `\\?\` prefix. */
const comparable = (path: string) =>
  normalize(path.replace(/^\\\\\?\\(UNC\\)?/i, (_, unc) => (unc ? "\\\\" : "")));

const inside = (path: string, roots: string[]) => {
  const target = comparable(path);
  return roots.some((root) => {
    const base = comparable(root);
    return target === base || target.startsWith(`${base}/`);
  });
};

const basename = (path: string) => path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;

/**
 * The folders an agent may show models from: its workspace's main folder,
 * extra roots and pane folders, plus every worktree of their repositories —
 * an agent working in a worktree must not be refused its own files. A folder
 * that is not a repository, or a machine without git, just adds none.
 */
export async function modelRoots(
  ws: Workspace,
  deps: Pick<ControlDeps, "worktrees">,
): Promise<string[]> {
  const repos = [ws.cwd, ...ws.extraRoots];
  const panes = ws.panes
    .map((pane) => pane.cwd)
    .filter((cwd): cwd is string => Boolean(cwd))
    .map((cwd) => resolvePath(ws.cwd, cwd));
  const listed = await Promise.all(
    repos.map((repo) =>
      Promise.resolve()
        .then(() => deps.worktrees(repo))
        .catch(() => [] as Worktree[]),
    ),
  );
  const trees = listed
    .flat()
    .filter((tree) => !tree.bare && !tree.prunable)
    .map((tree) => tree.path);
  const seen = new Set<string>();
  return [...repos, ...panes, ...trees].filter((root) => {
    const key = comparable(root);
    if (!root.trim() || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/**
 * Where a relative path is read from: the caller's pane folder — where Hangar
 * launched the agent, a worktree for instance — rather than the folder its
 * MCP server reports, which a project-wide config pins to the workspace root.
 */
function callerFolder(ws: Workspace, caller: Caller): string {
  const pane = caller.paneId ? ws.panes.find((entry) => entry.id === caller.paneId) : undefined;
  return pane ? effectiveCwd(ws, pane) : (caller.cwd ?? ws.cwd);
}

/**
 * The workspace's own preview — never the one on screen, nor another
 * workspace's — drawn by its window of its own when it has one.
 */
async function previewOf(ws: Workspace, deps: ControlDeps): Promise<ModelHost> {
  const host = await deps.modelHost(ws.id, Boolean(ws.detached));
  if (host) return host;
  throw new Error(
    `"${ws.name}" is shown in a window of its own, and that window is not answering, so the 3D ` +
      "preview cannot open there. Try again in a few seconds.",
  );
}

/**
 * The form `model_show` stored a path in: Rust's canonical one, whatever
 * casing, link or Git Bash spelling (/c/…) the agent used this time. Falls
 * back to the plain resolution when the file is gone.
 */
async function canonicalPath(deps: ControlDeps, base: string, given: string): Promise<string> {
  const resolved = resolvePath(base, given);
  const file = await deps.modelStat(resolved).catch(() => null);
  return file?.path ?? resolved;
}

/** An open model by path (already resolved, see canonicalPath), or by its
    file name alone when only one open model has it. */
function findModel(models: OpenModel[], given: string, resolved: string): OpenModel {
  const target = comparable(resolved);
  const exact = models.find((model) => comparable(model.path) === target);
  if (exact) return exact;
  // The same file reached through a link, or named without its folder.
  const name = basename(given).toLowerCase();
  const named = models.filter(
    (model) => basename(model.path).toLowerCase() === name || same(model.title, given),
  );
  if (named.length === 1) return named[0];
  throw new Error(
    models.length
      ? `No open model matches "${given}". Open: ${models.map((model) => model.path).join(", ")}.`
      : `No 3D model is open, so none matches "${given}". Show it with model_show first.`,
  );
}

function sizeArg(value: unknown, fallback: number): number {
  const wanted = typeof value === "number" ? value : typeof value === "string" ? Number(value) : NaN;
  if (!Number.isFinite(wanted)) return fallback;
  return Math.min(SNAPSHOT_MAX, Math.max(SNAPSHOT_MIN, Math.round(wanted)));
}

function angleArg(value: unknown): SnapshotAngle {
  const wanted = text(value)?.toLowerCase();
  if (!wanted) return "iso";
  if (!(ANGLES as string[]).includes(wanted)) {
    throw new Error(`Unknown angle "${wanted}". One of: ${ANGLES.join(", ")}.`);
  }
  return wanted as SnapshotAngle;
}

async function modelShow(
  ws: Workspace,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
): Promise<unknown> {
  const given = text(args.path);
  if (!given) {
    throw new Error("Give the path of the model to show, absolute or relative to your folder.");
  }
  const path = resolvePath(callerFolder(ws, caller), given);
  if (!modelFormatOf(path)) {
    throw new Error(
      `"${given}" is not a 3D model the preview can open. Formats: ${MODEL_FORMATS.join(", ")}.`,
    );
  }

  const roots = await modelRoots(ws, deps);
  const outside = () =>
    new Error(
      `${path} is outside your workspace. The preview only opens models from ${roots.join(", ")} ` +
        "(and their subfolders). Move or copy the file into one of them, then show it from there.",
    );
  await deps.modelAllowRoots(ws.id, roots);
  // Rust is the real gate, on the canonical path. The folders it allows are
  // every workspace's that showed a model, though, and this one is checked
  // against the caller's own: the path as given, or where its links lead.
  const givenInside = inside(path, roots);
  let file: ModelFileInfo;
  try {
    file = await deps.modelStat(path);
  } catch (err) {
    // Refused for being out of bounds, or missing out of bounds: either way,
    // the workspace is the thing worth naming.
    if (!givenInside) throw outside();
    throw new Error(messageOf(err));
  }
  if (!givenInside && !inside(file.path, roots)) throw outside();
  const title = text(args.title) ?? basename(file.path);
  if (file.size > MAX_MODEL_BYTES) {
    const mb = (bytes: number) => Math.round(bytes / (1024 * 1024));
    throw new Error(
      `${title} is ${mb(file.size)} MB, and the preview opens models up to ${mb(MAX_MODEL_BYTES)} MB. ` +
        "Export a lighter version (decimated, or compressed with Draco or meshopt) and show that.",
    );
  }
  const format = modelFormatOf(file.path) ?? modelFormatOf(path)!;

  const preview = await previewOf(ws, deps);
  // Showing a model in a workspace is using it, as sending to its panes is.
  // Asleep, it wakes the way pane_send wakes it: its window does not load
  // models for a workspace whose memory it gave back.
  const woke = deps.asleep(ws.id);
  if (woke) deps.openWorkspace(ws.id);
  deps.touch(ws.id);
  const id = await preview.show({
    path: file.path,
    title,
    format,
    openedBy: caller.paneName,
    modifiedMs: file.modifiedMs,
  });
  deps.announce({
    kind: onScreen(deps.snapshot(), ws) ? "modelShown" : "modelShownElsewhere",
    by: caller.paneName,
    file: title,
    workspace: ws.name,
  });

  const settled = await preview.whenSettled(id, SETTLE_MS);
  if (settled?.status === "error") {
    throw new Error(`The 3D preview could not load ${title}: ${settled.error ?? "unknown error"}`);
  }
  // Asked once the load is over, when the window has drawn what it opened.
  const seen = await sight(ws, preview, woke);
  if (!settled) {
    return {
      opened: true,
      status: "loading",
      path: file.path,
      format,
      title,
      ...seen,
      note: [
        `The preview is still loading it after ${SETTLE_MS / 1000} s; a big file takes a while. ` +
          "model_snapshot waits for the load, so call it when you want to look.",
        seen.note,
      ]
        .filter(Boolean)
        .join(" "),
    };
  }
  return { opened: true, path: file.path, format, title: settled.title, info: settled.info, ...seen };
}

/** The workspace is the one its window shows: the main window's active one, or one of its own. */
const onScreen = (state: AppState, ws: Workspace) =>
  Boolean(ws.detached) || state.activeWorkspaceId === ws.id;

/**
 * Whether the user can see the model just shown, told to the agent straight:
 * a model shown in a workspace that is not on screen waits there, and an
 * agent saying "look at the preview" would send the user looking for nothing.
 */
async function sight(
  ws: Workspace,
  preview: ModelHost,
  woke: boolean,
): Promise<{ workspace: string; visible: boolean; woke?: true; note?: string }> {
  const where = await preview.list().catch(() => null);
  const visible = Boolean(where?.shown);
  const notes = [
    woke
      ? `"${ws.name}" was asleep: it is waking up now, its agents resuming their sessions.`
      : null,
    visible
      ? null
      : where?.onScreen
        ? `"${ws.name}" is on screen, but its 3D preview is not: another view is open, or there is ` +
          "no room beside the terminals. The preview's toolbar button tells the user there is a model to see."
        : `It is open in the 3D preview of "${ws.name}", which is not the workspace on screen: the user ` +
          "sees it when they go to that workspace. Tell them it is there if they should look now.",
  ].filter(Boolean);
  return {
    workspace: ws.name,
    visible,
    ...(woke ? { woke: true as const } : {}),
    ...(notes.length ? { note: notes.join(" ") } : {}),
  };
}

async function modelSnapshot(
  ws: Workspace,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
): Promise<unknown> {
  const angle = angleArg(args.angle);
  const width = sizeArg(args.width, SNAPSHOT_WIDTH);
  const height = sizeArg(args.height, SNAPSHOT_HEIGHT);
  const preview = await previewOf(ws, deps);
  const { models, activeId } = await preview.list();
  const given = text(args.path);
  if (!models.length) {
    throw new Error(
      `No 3D model is open in "${ws.name}". Show one with model_show, then take the snapshot.`,
    );
  }
  const model = given
    ? findModel(models, given, await canonicalPath(deps, callerFolder(ws, caller), given))
    : (models.find((entry) => entry.id === activeId) ?? models[models.length - 1]);

  // In front in its workspace's preview, so the user sees what the agent is
  // looking at — there, without the preview coming over another workspace.
  await preview.reveal(model.id);
  // An agent iterating on a file snapshots right after writing it, before the
  // panel's own polling has noticed: load the new version first.
  const fresh = await deps.modelStat(model.path).catch(() => null);
  const rewritten = Boolean(fresh && fresh.modifiedMs > model.modifiedMs);
  if (fresh && rewritten) await preview.reload(model.id, fresh.modifiedMs);
  const ready =
    rewritten || model.status === "loading"
      ? await preview.whenSettled(model.id, SNAPSHOT_SETTLE_MS)
      : model;
  if (!ready) {
    throw new Error(
      `${model.title} is still loading after ${SNAPSHOT_SETTLE_MS / 1000} s. ` +
        "Try model_snapshot again in a moment.",
    );
  }
  if (ready.status === "error") {
    throw new Error(
      `${model.title} failed to load: ${ready.error ?? "unknown error"}. ` +
        "Fix the file, then call model_show again.",
    );
  }

  let png: string;
  try {
    png = await preview.snapshot(model.id, { angle, width, height });
  } catch (err) {
    const message = messageOf(err);
    if (/not mounted/i.test(message)) {
      throw new Error(
        `The 3D preview did not come up in the window that shows "${ws.name}", so ${model.title} ` +
          "cannot be rendered. Try model_snapshot again in a moment.",
      );
    }
    throw new Error(`Could not render ${model.title}: ${message}`);
  }
  return { png, mimeType: "image/png", path: model.path, angle, width, height, info: ready.info };
}

async function modelClose(
  ws: Workspace,
  args: Record<string, unknown>,
  caller: Caller,
  deps: ControlDeps,
): Promise<unknown> {
  const preview = await previewOf(ws, deps);
  const { models } = await preview.list();
  const given = text(args.path);
  // The caller's own workspace's preview only, the one listed: without a
  // path, everything in it.
  const closing = given
    ? [findModel(models, given, await canonicalPath(deps, callerFolder(ws, caller), given))]
    : models;
  if (!closing.length) return { closed: [], open: models.length };
  await preview.close(closing.map((model) => model.id));
  deps.announce({
    kind: onScreen(deps.snapshot(), ws) ? "modelClosed" : "modelClosedElsewhere",
    by: caller.paneName,
    file: closing.map((model) => model.title).join(", "),
    workspace: ws.name,
  });
  return { closed: closing.map((model) => model.path), open: models.length - closing.length };
}

export async function runControl(
  op: string,
  args: Record<string, unknown>,
  deps: ControlDeps,
): Promise<unknown> {
  if (!deps.enabled()) throw new Error(DISABLED);
  const caller = readCaller(args);
  const state = deps.snapshot();
  const by = caller.paneName;

  if (op === "workspace_list") {
    const mine = callerWorkspace(state, caller);
    return {
      workspaces: state.workspaces.map((ws) => ({
        name: ws.name,
        cwd: ws.cwd,
        panes: ws.panes.length,
        active: ws.id === state.activeWorkspaceId,
        open: deps.isOpen(ws.id),
        asleep: deps.asleep(ws.id),
        yours: ws.id === mine?.id,
        id: ws.id,
      })),
    };
  }

  const ws = workspaceFor(state, args, caller);
  // What an agent does to a workspace's panes counts as using it, like typing
  // into them would — reading them does not.
  if (op === "pane_send" || op === "pane_create" || op === "pane_restart") deps.touch(ws.id);

  // Orchestration has modules of its own: lib/workerControl, lib/deliveryControl.
  if (op.startsWith("worker_")) return workerControl(op, ws, args, caller, deps);
  if (op.startsWith("delivery_")) return deliveryControl(op, ws, args, caller, deps);

  switch (op) {
    case "pane_list": {
      const open = deps.isOpen(ws.id);
      const panes = await Promise.all(
        ordered(ws).map(async (pane) => {
          const activity = deps.activity(pane.id);
          return {
            name: pane.name,
            agent: pane.agent,
            cwd: effectiveCwd(ws, pane),
            running: open ? await deps.alive(pane.id).catch(() => false) : false,
            activity: activity?.activity ?? null,
            you: pane.id === caller.paneId,
            id: pane.id,
          };
        }),
      );
      const asleep = !open && deps.asleep(ws.id);
      return {
        workspace: { name: ws.name, cwd: ws.cwd, open, ...(asleep ? { asleep } : {}) },
        // Not mounted yet: its terminals start the first time it is shown, or
        // as soon as an agent creates, sends to or restarts one of its panes.
        ...(open
          ? {}
          : {
              note: asleep
                ? `This workspace is asleep, so none of its panes is running. ${ASLEEP}`
                : "This workspace is not open yet, so none of its panes is running.",
            }),
        panes,
      };
    }

    case "pane_create": {
      if (ws.panes.length >= MAX_PANES) {
        throw new Error(`"${ws.name}" is full: a workspace holds at most ${MAX_PANES} panes.`);
      }
      const agent = agentArg(args.agent);
      const name = text(args.name) ?? undefined;
      // Unique across every workspace: other sessions reach a pane by name.
      const holder = name ? state.workspaces.find((entry) => hasPane(entry, name)) : undefined;
      if (holder) {
        throw new Error(`A pane is already named "${name}" (in "${holder.name}"); pane names are unique across Hangar.`);
      }
      let cwd: string | null = null;
      const folder = text(args.cwd);
      if (folder) {
        cwd = resolveFolder(ws.cwd, folder);
        if (!(await deps.dirExists(cwd))) throw new Error(`No such folder: ${cwd}`);
      }
      const near = text(args.near) ? paneIn(ws, args.near) : null;
      const panes = ordered(ws);
      const model = near ?? panes[panes.length - 1] ?? null;
      const finalAgent = agent ?? model?.agent ?? "shell";
      const prompt = typeof args.prompt === "string" && args.prompt.trim() ? args.prompt : null;
      if (prompt && finalAgent !== "claude") {
        throw new Error(
          "A prompt can only be handed to a Claude Code pane at creation. Create the pane without it, " +
            "then send it with pane_send once pane_read shows the agent has started.",
        );
      }

      const id = deps.addPane(ws.id, {
        agent,
        near: near?.id ?? null,
        dir: args.direction === "down" ? "col" : "row",
        cwd,
        name,
      });
      if (!id) throw new Error(`"${ws.name}" is full: a workspace holds at most ${MAX_PANES} panes.`);
      const created = deps
        .snapshot()
        .workspaces.find((entry) => entry.id === ws.id)
        ?.panes.find((pane) => pane.id === id);
      deps.openWorkspace(ws.id);
      if (prompt) await deps.enqueue(id, prompt);
      if (args.focus === true) deps.activate(ws.id, id);
      const paneName = created?.name ?? name ?? id;
      deps.announce({ kind: "created", by, pane: paneName, workspace: ws.name });
      return {
        workspace: ws.name,
        pane: {
          name: paneName,
          agent: created?.agent ?? finalAgent,
          cwd: created ? effectiveCwd(ws, created) : (cwd ?? ws.cwd),
          id,
        },
        ...(prompt
          ? { prompt: "queued: submitted as soon as the Claude Code session is ready" }
          : {}),
      };
    }

    case "pane_close": {
      const pane = paneIn(ws, args.pane);
      if (ws.panes.length <= 1) {
        throw new Error(`"${pane.name}" is the last pane of "${ws.name}" and cannot be closed.`);
      }
      deps.closePane(ws.id, pane.id);
      deps.announce({ kind: "closed", by, pane: pane.name, workspace: ws.name });
      return { closed: pane.name, workspace: ws.name };
    }

    case "pane_restart": {
      const pane = paneIn(ws, args.pane);
      const agent = agentArg(args.agent);
      const fresh = args.fresh === true || agent !== undefined;
      const patch: Partial<Pane> = agent
        ? { agent, sessionId: null }
        : fresh
          ? { sessionId: null }
          : {};
      const id = deps.respawnPane(ws.id, pane.id, patch);
      if (!id) throw new Error(`"${pane.name}" was closed in the meantime.`);
      deps.openWorkspace(ws.id);
      deps.announce({ kind: fresh ? "reset" : "restarted", by, pane: pane.name, workspace: ws.name });
      return { pane: { name: pane.name, agent: agent ?? pane.agent, id }, fresh };
    }

    case "pane_send": {
      const pane = paneIn(ws, args.pane);
      const body = typeof args.text === "string" ? args.text : "";
      const keys = Array.isArray(args.keys) ? args.keys.map((key) => String(key).toLowerCase()) : [];
      if (!body && keys.length === 0) throw new Error("Nothing to send: give text, keys, or both.");

      if (args.when_idle === true) {
        if (pane.agent !== "claude") {
          throw new Error(`when_idle only works with Claude Code panes, and "${pane.name}" runs ${pane.agent}.`);
        }
        if (keys.length > 0) throw new Error("Keys cannot be queued: send them without when_idle.");
        await deps.enqueue(pane.id, body);
        // Held for a Claude Code that is not running, it would wait for good.
        const starting = !deps.isOpen(ws.id);
        if (starting) deps.openWorkspace(ws.id);
        return { queued: true, pane: pane.name, ...(starting ? { starting } : {}) };
      }

      const term = await deps.terminal(pane.id);
      if (!term) {
        if (!deps.isOpen(ws.id)) {
          const asleep = deps.asleep(ws.id);
          deps.openWorkspace(ws.id);
          throw new Error(
            asleep
              ? `"${ws.name}" was asleep: its agents are resuming their sessions now. Send again in a few seconds.`
              : `"${ws.name}" was not open: its panes are starting now. Send again in a few seconds.`,
          );
        }
        throw new Error(`"${pane.name}" has no terminal yet. Send again in a moment.`);
      }
      // Checked before anything is typed, so a bad key name sends nothing.
      const sequences = keys.map((key) => keySequence(key, term.applicationCursor));
      // Through xterm rather than straight to the PTY: it brackets the paste,
      // so several lines land as one message instead of one Enter each.
      if (body) await term.paste(body);
      // The paste reaches the PTY on its own path; an Enter written directly
      // could overtake it.
      if (body && (args.submit !== false || sequences.length > 0)) await deps.sleep(120);
      if (body && args.submit !== false) await deps.write(pane.id, "\r");
      for (const sequence of sequences) {
        await deps.write(pane.id, sequence);
        await deps.sleep(40);
      }
      return { sent: true, pane: pane.name };
    }

    case "pane_read": {
      const pane = paneIn(ws, args.pane);
      const term = await deps.terminal(pane.id);
      if (!term) {
        throw new Error(
          deps.isOpen(ws.id)
            ? `"${pane.name}" has no terminal yet.`
            : deps.asleep(ws.id)
              ? `"${ws.name}" is asleep, so "${pane.name}" shows nothing. ${ASLEEP}`
              : `"${ws.name}" is not open, so "${pane.name}" has shown nothing yet.`,
        );
      }
      const wanted = typeof args.lines === "number" ? Math.floor(args.lines) : READ_DEFAULT;
      const lines = await term.read(Math.min(READ_MAX, Math.max(1, wanted)));
      return { pane: pane.name, lines: lines.length, text: lines.join("\n") };
    }

    case "workspace_window": {
      const detach = args.detach !== false;
      if (detach === Boolean(ws.detached)) {
        return { workspace: ws.name, detached: detach, changed: false };
      }
      if (detach) {
        if (!deps.canDetach()) throw new Error("This Hangar cannot open other windows.");
        await deps.detach(ws.id);
      } else {
        deps.reattach(ws.id);
      }
      return { workspace: ws.name, detached: detach, changed: true };
    }

    case "pane_focus": {
      const pane = paneIn(ws, args.pane);
      deps.activate(ws.id, pane.id);
      return { focused: pane.name, workspace: ws.name };
    }

    case "model_show":
      return modelShow(ws, args, caller, deps);

    case "model_snapshot":
      return modelSnapshot(ws, args, caller, deps);

    case "model_close":
      return modelClose(ws, args, caller, deps);

    default:
      throw new Error(`Unknown operation "${op}".`);
  }
}

const controlReply = (id: string, ok: boolean, value: unknown, error: string | null) =>
  invoke<void>("control_reply", { id, ok, value: value ?? null, error });

/**
 * Answers the control requests for as long as the app is mounted. `deps` is
 * read when a request lands, so it can be rebuilt on every render; null means
 * the store has not loaded yet, and requests are told to wait.
 */
export function usePaneControl(deps: ControlDeps | null) {
  const depsRef = useRef(deps);
  depsRef.current = deps;

  useEffect(() => {
    // Only the window that owns the store carries requests out.
    if (!isMainWindow) return;
    let cancelled = false;
    let off: (() => void) | null = null;
    listen<ControlRequest>("control:request", (event) => {
      const { id, op, args } = event.payload;
      const current = depsRef.current;
      const run = current
        ? runControl(op, args ?? {}, current)
        : Promise.reject(new Error("Hangar.AI is still starting. Try again in a moment."));
      run
        .then((value) => controlReply(id, true, value, null))
        .catch((err) =>
          controlReply(id, false, null, err instanceof Error ? err.message : String(err)),
        )
        .catch(() => undefined);
    })
      .then((stop) => {
        if (cancelled) stop();
        else off = stop;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);
}
