import { useSyncExternalStore } from "react";
import type { ModelFormat } from "./modelFiles";

/**
 * The 3D models open in this window, one preview per workspace: which models
 * each workspace has open, which is in front, whether its panel shows and
 * whether it fills the work area. Switching workspaces switches previews; a
 * model an agent shows in a workspace that is not on screen waits there.
 *
 * Written by the MCP control handlers (an agent showing a model), by the
 * panel itself (the user closing a tab, the viewer reporting what it loaded)
 * and read by both. Outside React for the same reason as lib/paneInsight: the
 * writers are nowhere near the panel in the tree.
 *
 * Models are ids across the whole window: the one viewer the window has
 * (ModelPanel) holds them all, whichever workspace they belong to. Nothing
 * here is saved; a reload starts every preview empty.
 */

export interface ModelInfo {
  triangles: number;
  vertices: number;
  meshes: number;
  materials: number;
  /** Animation clip names, in file order. */
  animations: string[];
  /** Bounding box size and centre, in the file's units. */
  size: [number, number, number];
  center: [number, number, number];
}

export interface OpenModel {
  id: string;
  /** Canonical absolute path. */
  path: string;
  title: string;
  format: ModelFormat;
  /** The workspace whose preview it is open in. */
  workspaceId: string;
  /** The pane name of the agent that opened it; null when the user did. */
  openedBy: string | null;
  openedAt: number;
  /** The file's mtime when last loaded; a newer one means reload. */
  modifiedMs: number;
  /** Bumped to make the viewer load the file again. */
  revision: number;
  status: "loading" | "ready" | "error";
  info: ModelInfo | null;
  error: string | null;
}

export type SnapshotAngle = "front" | "back" | "left" | "right" | "top" | "bottom" | "iso";

export interface SnapshotOptions {
  angle: SnapshotAngle;
  width: number;
  height: number;
}

/** Renders a model off screen; base64 PNG without the data: prefix. */
export type Snapshotter = (modelId: string, options: SnapshotOptions) => Promise<string>;

/** One workspace's preview. */
export interface ModelScope {
  models: OpenModel[];
  activeId: string | null;
  panelOpen: boolean;
  /** The panel takes the whole work area instead of sitting beside it. */
  expanded: boolean;
  /** An agent opened or changed something here that has not been on screen since. */
  unseen: boolean;
}

interface State {
  /** By workspace id; a workspace with no model open has no entry. */
  scopes: Readonly<Record<string, ModelScope>>;
  /** Every workspace's models in one list: what this window's viewer may hold. */
  all: OpenModel[];
}

const EMPTY: ModelScope = Object.freeze({
  models: [],
  activeId: null,
  panelOpen: false,
  expanded: false,
  unseen: false,
}) as ModelScope;

let state: State = { scopes: {}, all: [] };
const listeners = new Set<() => void>();
let snapshotter: Snapshotter | null = null;
/** Callers waiting for a model to finish loading, by model id. */
const settleWaiters = new Map<string, Set<(model: OpenModel) => void>>();

/** Replaces the scopes named in `patch`; null drops one. */
function set(patch: Record<string, ModelScope | null>) {
  const scopes = { ...state.scopes };
  for (const [workspaceId, scope] of Object.entries(patch)) {
    if (scope && scope.models.length) scopes[workspaceId] = scope;
    else delete scopes[workspaceId];
  }
  state = { scopes, all: Object.values(scopes).flatMap((scope) => scope.models) };
  for (const listener of listeners) listener();
  for (const model of state.all) {
    if (model.status === "loading") continue;
    const waiters = settleWaiters.get(model.id);
    if (!waiters) continue;
    settleWaiters.delete(model.id);
    for (const resolve of waiters) resolve(model);
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

const newId = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `m-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

const find = (id: string) => state.all.find((model) => model.id === id);

/** The workspace's preview; an empty one when it has nothing open. */
export function modelScope(workspaceId: string | null): ModelScope {
  return (workspaceId && state.scopes[workspaceId]) || EMPTY;
}

/** One model of a scope rewritten. */
const withModel = (scope: ModelScope, id: string, change: (model: OpenModel) => OpenModel) => ({
  ...scope,
  models: scope.models.map((model) => (model.id === id ? change(model) : model)),
});

/**
 * Opens a model in its workspace's preview, or brings it forward and reloads
 * it when that workspace has it open already — an agent showing the same file
 * again has usually just rewritten it. Opens that workspace's panel, and only
 * that one. Returns the model's id.
 */
export function showModel(model: {
  path: string;
  title?: string;
  format: ModelFormat;
  workspaceId: string;
  openedBy: string | null;
  modifiedMs: number;
}): string {
  const scope = modelScope(model.workspaceId);
  const existing = scope.models.find((m) => m.path === model.path);
  const title = model.title?.trim() || model.path.replace(/\\/g, "/").split("/").pop() || model.path;
  if (existing) {
    set({
      [model.workspaceId]: {
        ...withModel(scope, existing.id, (m) => ({
          ...m,
          title,
          openedBy: model.openedBy ?? m.openedBy,
          modifiedMs: model.modifiedMs,
          revision: m.revision + 1,
          status: "loading",
          error: null,
        })),
        activeId: existing.id,
        panelOpen: true,
        unseen: true,
      },
    });
    return existing.id;
  }
  const id = newId();
  const entry: OpenModel = {
    id,
    path: model.path,
    title,
    format: model.format,
    workspaceId: model.workspaceId,
    openedBy: model.openedBy,
    openedAt: Date.now(),
    modifiedMs: model.modifiedMs,
    revision: 0,
    status: "loading",
    info: null,
    error: null,
  };
  set({
    [model.workspaceId]: {
      ...scope,
      models: [...scope.models, entry],
      activeId: id,
      panelOpen: true,
      unseen: true,
    },
  });
  return id;
}

/**
 * Closes models, wherever they are open. A workspace left with none has its
 * panel closed; one that still has some keeps its panel as it was, the last
 * of them in front if the one in front went.
 */
export function closeModels(ids: Iterable<string>) {
  const closing = new Set(ids);
  const patch: Record<string, ModelScope | null> = {};
  for (const [workspaceId, scope] of Object.entries(state.scopes)) {
    if (!scope.models.some((model) => closing.has(model.id))) continue;
    const models = scope.models.filter((model) => !closing.has(model.id));
    const activeId =
      scope.activeId && closing.has(scope.activeId)
        ? (models[models.length - 1]?.id ?? null)
        : scope.activeId;
    patch[workspaceId] = models.length ? { ...scope, models, activeId } : null;
  }
  if (Object.keys(patch).length) set(patch);
}

export function closeModel(id: string) {
  closeModels([id]);
}

/** Every model of one workspace: its preview goes away. */
export function closeWorkspaceModels(workspaceId: string) {
  if (state.scopes[workspaceId]) set({ [workspaceId]: null });
}

/** Every model of every workspace. */
export function closeAllModels() {
  if (!state.all.length) return;
  set(Object.fromEntries(Object.keys(state.scopes).map((id) => [id, null])));
}

/** Forgets the previews of the workspaces that are not in `live`: deleted since. */
export function pruneModels(live: Iterable<string>) {
  const keep = new Set(live);
  const gone = Object.keys(state.scopes).filter((id) => !keep.has(id));
  if (gone.length) set(Object.fromEntries(gone.map((id) => [id, null])));
}

/** Puts a model in front in its workspace's preview. */
export function activateModel(id: string) {
  const model = find(id);
  if (!model) return;
  const scope = modelScope(model.workspaceId);
  if (scope.activeId === id) return;
  set({ [model.workspaceId]: { ...scope, activeId: id } });
}

/** The viewer's report on a load: what it found, or why it failed. */
export function settleModel(id: string, result: { info: ModelInfo } | { error: string }) {
  const model = find(id);
  if (!model) return;
  set({
    [model.workspaceId]: withModel(modelScope(model.workspaceId), id, (m) =>
      "info" in result
        ? { ...m, status: "ready", info: result.info, error: null }
        : { ...m, status: "error", error: result.error },
    ),
  });
}

/** A rewritten file: load it again, keeping the camera where the user left it. */
export function reloadModel(id: string, modifiedMs: number) {
  const model = find(id);
  if (!model) return;
  set({
    [model.workspaceId]: {
      ...withModel(modelScope(model.workspaceId), id, (m) => ({
        ...m,
        modifiedMs,
        revision: m.revision + 1,
        status: "loading",
      })),
      unseen: true,
    },
  });
}

/** Opens, closes, expands or collapses a workspace's panel. Nothing to show, nothing to open. */
export function setModelPanel(workspaceId: string, patch: { open?: boolean; expanded?: boolean }) {
  const scope = state.scopes[workspaceId];
  if (!scope) return;
  const panelOpen = patch.open ?? scope.panelOpen;
  const expanded = panelOpen ? (patch.expanded ?? scope.expanded) : false;
  if (panelOpen === scope.panelOpen && expanded === scope.expanded) return;
  set({ [workspaceId]: { ...scope, panelOpen, expanded } });
}

/** The workspace's preview has been on screen since its last change. */
export function markModelsSeen(workspaceId: string) {
  const scope = state.scopes[workspaceId];
  if (scope?.unseen) set({ [workspaceId]: { ...scope, unseen: false } });
}

/**
 * Removes a workspace's preview from this window and hands it over, for the
 * window the workspace moves to (lib/modelHost). Null when it has none.
 */
export function takeModelScope(workspaceId: string): ModelScope | null {
  const scope = state.scopes[workspaceId];
  if (!scope) return null;
  set({ [workspaceId]: null });
  return scope;
}

/**
 * Takes in a preview handed over by another window. What this window already
 * has open for the workspace stays, in front; the same file is not opened twice.
 */
export function adoptModelScope(workspaceId: string, scope: ModelScope) {
  const here = state.scopes[workspaceId];
  const known = new Set(state.all.map((model) => model.id));
  const paths = new Set(here?.models.map((model) => model.path));
  const arriving = scope.models
    .filter((model) => !known.has(model.id) && !paths.has(model.path))
    .map((model) => ({ ...model, workspaceId }));
  if (!arriving.length) return;
  set({
    [workspaceId]: here
      ? { ...here, models: [...here.models, ...arriving], unseen: here.unseen || scope.unseen }
      : {
          ...scope,
          models: arriving,
          activeId: arriving.some((model) => model.id === scope.activeId)
            ? scope.activeId
            : arriving[arriving.length - 1].id,
        },
  });
}

/** Resolves once the model has loaded or failed, or with null after `ms`. */
export function whenSettled(id: string, ms: number): Promise<OpenModel | null> {
  const now = find(id);
  if (!now) return Promise.resolve(null);
  if (now.status !== "loading") return Promise.resolve(now);
  return new Promise((resolve) => {
    const waiters = settleWaiters.get(id) ?? new Set();
    const done = (model: OpenModel | null) => {
      window.clearTimeout(timer);
      waiters.delete(done);
      resolve(model);
    };
    const timer = window.setTimeout(() => done(null), ms);
    waiters.add(done);
    settleWaiters.set(id, waiters);
  });
}

/* ── what the window shows ────────────────────────────────────── */

/**
 * The workspace this window has on screen, and whether its preview panel is
 * on screen with it — docked, an open drawer or expanded. Kept by App for the
 * agents' answers: a model shown elsewhere is not one the user can see.
 */
let view: { workspaceId: string | null; panelShown: boolean } = {
  workspaceId: null,
  panelShown: false,
};

export function noteModelView(workspaceId: string | null, panelShown: boolean) {
  view = { workspaceId, panelShown };
}

export function modelView(): Readonly<typeof view> {
  return view;
}

/* ── the viewer ───────────────────────────────────────────────── */

/** The viewer registers how it renders a snapshot; null when it unmounts. */
export function registerSnapshotter(fn: Snapshotter | null) {
  snapshotter = fn;
}

/** Whether a viewer is mounted in this window to render snapshots. */
export function hasSnapshotter(): boolean {
  return snapshotter !== null;
}

export function requestSnapshot(id: string, options: SnapshotOptions): Promise<string> {
  if (!snapshotter) return Promise.reject(new Error("the 3D preview is not mounted"));
  return snapshotter(id, options);
}

export function getModels(): State {
  return state;
}

/** One workspace's preview, kept current; an empty one for null. */
export function useModels(workspaceId: string | null): ModelScope {
  return useSyncExternalStore(subscribe, () => modelScope(workspaceId));
}

/** Every model open in this window, whichever workspace has it. */
export function useAllModels(): OpenModel[] {
  return useSyncExternalStore(subscribe, () => state.all);
}

/** Whether any workspace of this window has a model open; a load landing does not change it. */
export function useAnyModels(): boolean {
  return useSyncExternalStore(subscribe, () => state.all.length > 0);
}
