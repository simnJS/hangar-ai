import { useSyncExternalStore } from "react";
import type { ModelFormat } from "./modelFiles";
import type { FileKind } from "./preview";

/**
 * What is open in this window's previews, one preview per workspace: the
 * files each workspace has open — 3D models, images, videos, sounds, text,
 * Markdown, PDF, web pages — which is in front, whether its panel shows and
 * whether it fills the work area. Switching workspaces switches previews; a
 * file an agent shows in a workspace that is not on screen waits there.
 *
 * Written by the MCP control handlers (an agent showing a file), by the
 * panel itself (the user closing a tab, the 3D viewer reporting what it
 * loaded) and read by both. Outside React for the same reason as
 * lib/paneInsight: the writers are nowhere near the panel in the tree.
 *
 * Entries are ids across the whole window: the one 3D viewer the window has
 * (ModelPanel) holds the models of every workspace, whichever it belongs to.
 * Nothing here is saved; a reload starts every preview empty.
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

/** What every open file has, whatever its kind. */
interface OpenEntry {
  id: string;
  /** Canonical absolute path. */
  path: string;
  title: string;
  /** The workspace whose preview it is open in. */
  workspaceId: string;
  /** The pane name of the agent that opened it; null when the user did. */
  openedBy: string | null;
  openedAt: number;
  /** The file's mtime when last loaded; a newer one means reload. */
  modifiedMs: number;
  /** Bumped to make the viewer load the file again. */
  revision: number;
  /** A file that is not a model is ready as soon as it is open: its viewer reads it on screen. */
  status: "loading" | "ready" | "error";
  error: string | null;
}

export interface OpenModel extends OpenEntry {
  kind: "model";
  format: ModelFormat;
  info: ModelInfo | null;
}

export interface OpenFile extends OpenEntry {
  kind: FileKind;
  mime: string;
  /** In bytes, when it was shown. */
  size: number;
}

/** A tab of the preview. */
export type OpenPreview = OpenModel | OpenFile;

export const isModel = (entry: OpenPreview): entry is OpenModel => entry.kind === "model";

/** What opening a file takes, its id and workspace aside. */
export type PreviewToShow =
  | { kind: "model"; path: string; title?: string; format: ModelFormat; openedBy: string | null; modifiedMs: number }
  | {
      kind: FileKind;
      path: string;
      title?: string;
      mime: string;
      size: number;
      openedBy: string | null;
      modifiedMs: number;
    };

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
  /** Every tab, in order: models and the other kinds together. */
  models: OpenPreview[];
  activeId: string | null;
  panelOpen: boolean;
  /** The panel takes the whole work area instead of sitting beside it. */
  expanded: boolean;
  /** An agent opened or changed something here that has not been on screen since. */
  unseen: boolean;
}

interface State {
  /** By workspace id; a workspace with nothing open has no entry. */
  scopes: Readonly<Record<string, ModelScope>>;
  /** Everything open in the window, every workspace's, every kind. */
  entries: OpenPreview[];
  /** Every workspace's 3D models in one list: what this window's 3D viewer may hold. */
  all: OpenModel[];
}

const EMPTY: ModelScope = Object.freeze({
  models: [],
  activeId: null,
  panelOpen: false,
  expanded: false,
  unseen: false,
}) as ModelScope;

let state: State = { scopes: {}, entries: [], all: [] };
const listeners = new Set<() => void>();
let snapshotter: Snapshotter | null = null;
/** Callers waiting for a file to finish loading, by id. */
const settleWaiters = new Map<string, Set<(entry: OpenPreview) => void>>();

/** Replaces the scopes named in `patch`; null drops one. */
function set(patch: Record<string, ModelScope | null>) {
  const scopes = { ...state.scopes };
  for (const [workspaceId, scope] of Object.entries(patch)) {
    if (scope && scope.models.length) scopes[workspaceId] = scope;
    else delete scopes[workspaceId];
  }
  const entries = Object.values(scopes).flatMap((scope) => scope.models);
  // The 3D list keeps its identity while its models do, so a viewer
  // watching it is not woken by an image opening.
  const models = entries.filter(isModel);
  const all =
    models.length === state.all.length && models.every((model, i) => model === state.all[i])
      ? state.all
      : models;
  state = { scopes, entries, all };
  for (const listener of listeners) listener();
  for (const entry of state.entries) {
    if (entry.status === "loading") continue;
    const waiters = settleWaiters.get(entry.id);
    if (!waiters) continue;
    settleWaiters.delete(entry.id);
    for (const resolve of waiters) resolve(entry);
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

const find = (id: string) => state.entries.find((entry) => entry.id === id);

/** The workspace's preview; an empty one when it has nothing open. */
export function modelScope(workspaceId: string | null): ModelScope {
  return (workspaceId && state.scopes[workspaceId]) || EMPTY;
}

/** One entry of a scope rewritten. */
const withEntry = (scope: ModelScope, id: string, change: (entry: OpenPreview) => OpenPreview) => ({
  ...scope,
  models: scope.models.map((entry) => (entry.id === id ? change(entry) : entry)),
});

/**
 * The fields that depend on the kind, as the file is now. A model waits for
 * the 3D viewer; any other kind is ready, its viewer reading it on screen.
 */
const kindFields = (file: PreviewToShow) =>
  file.kind === "model"
    ? ({ kind: "model", format: file.format, modifiedMs: file.modifiedMs, info: null, status: "loading" } as const)
    : ({ kind: file.kind, mime: file.mime, size: file.size, modifiedMs: file.modifiedMs, status: "ready" } as const);

/**
 * Opens a file in its workspace's preview, or brings it forward and reloads
 * it when that workspace has it open already — an agent showing the same file
 * again has usually just rewritten it. Opens that workspace's panel, and only
 * that one. Returns the entry's id.
 */
export function showPreview(file: PreviewToShow & { workspaceId: string }): string {
  const scope = modelScope(file.workspaceId);
  const existing = scope.models.find((entry) => entry.path === file.path);
  const title = file.title?.trim() || file.path.replace(/\\/g, "/").split("/").pop() || file.path;
  if (existing) {
    set({
      [file.workspaceId]: {
        ...withEntry(
          scope,
          existing.id,
          (entry) =>
            ({
              id: entry.id,
              path: entry.path,
              workspaceId: entry.workspaceId,
              openedAt: entry.openedAt,
              title,
              openedBy: file.openedBy ?? entry.openedBy,
              revision: entry.revision + 1,
              error: null,
              ...kindFields(file),
              // A model keeps what it last showed until the new version lands.
              ...(entry.kind === "model" && file.kind === "model" ? { info: entry.info } : {}),
            }) as OpenPreview,
        ),
        activeId: existing.id,
        panelOpen: true,
        unseen: true,
      },
    });
    return existing.id;
  }
  const id = newId();
  const entry = {
    id,
    path: file.path,
    title,
    workspaceId: file.workspaceId,
    openedBy: file.openedBy,
    openedAt: Date.now(),
    revision: 0,
    error: null,
    ...kindFields(file),
  } as OpenPreview;
  set({
    [file.workspaceId]: {
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
 * Closes entries, wherever they are open. A workspace left with none has its
 * panel closed; one that still has some keeps its panel as it was, the last
 * of them in front if the one in front went.
 */
export function closeModels(ids: Iterable<string>) {
  const closing = new Set(ids);
  const patch: Record<string, ModelScope | null> = {};
  for (const [workspaceId, scope] of Object.entries(state.scopes)) {
    if (!scope.models.some((entry) => closing.has(entry.id))) continue;
    const models = scope.models.filter((entry) => !closing.has(entry.id));
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

/** Everything one workspace has open: its preview goes away. */
export function closeWorkspaceModels(workspaceId: string) {
  if (state.scopes[workspaceId]) set({ [workspaceId]: null });
}

/** Everything every workspace has open. */
export function closeAllModels() {
  if (!state.entries.length) return;
  set(Object.fromEntries(Object.keys(state.scopes).map((id) => [id, null])));
}

/** Forgets the previews of the workspaces that are not in `live`: deleted since. */
export function pruneModels(live: Iterable<string>) {
  const keep = new Set(live);
  const gone = Object.keys(state.scopes).filter((id) => !keep.has(id));
  if (gone.length) set(Object.fromEntries(gone.map((id) => [id, null])));
}

/** Puts an entry in front in its workspace's preview. */
export function activateModel(id: string) {
  const entry = find(id);
  if (!entry) return;
  const scope = modelScope(entry.workspaceId);
  if (scope.activeId === id) return;
  set({ [entry.workspaceId]: { ...scope, activeId: id } });
}

/** The 3D viewer's report on a load: what it found, or why it failed. */
export function settleModel(id: string, result: { info: ModelInfo } | { error: string }) {
  const model = find(id);
  if (!model || !isModel(model)) return;
  set({
    [model.workspaceId]: withEntry(modelScope(model.workspaceId), id, (m) =>
      "info" in result
        ? { ...(m as OpenModel), status: "ready", info: result.info, error: null }
        : { ...m, status: "error", error: result.error },
    ),
  });
}

/**
 * A rewritten file: load it again — a model keeping the camera where the user
 * left it, and waiting for the 3D viewer; any other kind read afresh by its
 * viewer, its new size noted when it is known.
 */
export function reloadModel(id: string, modifiedMs: number, size?: number) {
  const entry = find(id);
  if (!entry) return;
  set({
    [entry.workspaceId]: {
      ...withEntry(modelScope(entry.workspaceId), id, (e) =>
        isModel(e)
          ? { ...e, modifiedMs, revision: e.revision + 1, status: "loading" }
          : { ...e, modifiedMs, revision: e.revision + 1, size: size ?? e.size },
      ),
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
  const known = new Set(state.entries.map((entry) => entry.id));
  const paths = new Set(here?.models.map((entry) => entry.path));
  const arriving = scope.models
    .filter((entry) => !known.has(entry.id) && !paths.has(entry.path))
    .map((entry) => ({ ...entry, workspaceId }));
  if (!arriving.length) return;
  set({
    [workspaceId]: here
      ? { ...here, models: [...here.models, ...arriving], unseen: here.unseen || scope.unseen }
      : {
          ...scope,
          models: arriving,
          activeId: arriving.some((entry) => entry.id === scope.activeId)
            ? scope.activeId
            : arriving[arriving.length - 1].id,
        },
  });
}

/** Resolves once the entry has loaded or failed, or with null after `ms`. */
export function whenSettled(id: string, ms: number): Promise<OpenPreview | null> {
  const now = find(id);
  if (!now) return Promise.resolve(null);
  if (now.status !== "loading") return Promise.resolve(now);
  return new Promise((resolve) => {
    const waiters = settleWaiters.get(id) ?? new Set();
    const done = (entry: OpenPreview | null) => {
      window.clearTimeout(timer);
      waiters.delete(done);
      resolve(entry);
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
 * agents' answers: a file shown elsewhere is not one the user can see.
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

/* ── the 3D viewer ────────────────────────────────────────────── */

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

/** Every 3D model open in this window, whichever workspace has it. */
export function useAllModels(): OpenModel[] {
  return useSyncExternalStore(subscribe, () => state.all);
}

/** Whether any workspace of this window has anything open in its preview; a load landing does not change it. */
export function useAnyModels(): boolean {
  return useSyncExternalStore(subscribe, () => state.entries.length > 0);
}
