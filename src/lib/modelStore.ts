import { useSyncExternalStore } from "react";
import type { ModelFormat } from "./modelFiles";

/**
 * The 3D models open in this window's preview panel: which ones, which is in
 * front, what the viewer found in each, and whether the panel shows.
 *
 * Written by the MCP control handlers (an agent showing a model), by the
 * panel itself (the user closing a tab, the viewer reporting what it loaded)
 * and read by both. Outside React for the same reason as lib/paneInsight: the
 * writers are nowhere near the panel in the tree.
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
  workspaceId: string | null;
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

interface State {
  models: OpenModel[];
  activeId: string | null;
  panelOpen: boolean;
  /** The panel takes the whole work area instead of sitting beside it. */
  expanded: boolean;
}

let state: State = { models: [], activeId: null, panelOpen: false, expanded: false };
const listeners = new Set<() => void>();
let snapshotter: Snapshotter | null = null;
/** Callers waiting for a model to finish loading, by model id. */
const settleWaiters = new Map<string, Set<(model: OpenModel) => void>>();

function set(next: State) {
  state = next;
  for (const listener of listeners) listener();
  for (const model of state.models) {
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

/**
 * Opens a model, or brings it forward and reloads it when it is already
 * open — an agent showing the same file again has usually just rewritten it.
 * Opens the panel. Returns the model's id.
 */
export function showModel(model: {
  path: string;
  title?: string;
  format: ModelFormat;
  workspaceId: string | null;
  openedBy: string | null;
  modifiedMs: number;
}): string {
  const existing = state.models.find((m) => m.path === model.path);
  const title = model.title?.trim() || model.path.replace(/\\/g, "/").split("/").pop() || model.path;
  if (existing) {
    set({
      ...state,
      activeId: existing.id,
      panelOpen: true,
      models: state.models.map((m) =>
        m.id === existing.id
          ? {
              ...m,
              title,
              openedBy: model.openedBy ?? m.openedBy,
              modifiedMs: model.modifiedMs,
              revision: m.revision + 1,
              status: "loading",
              error: null,
            }
          : m,
      ),
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
  set({ ...state, models: [...state.models, entry], activeId: id, panelOpen: true });
  return id;
}

/** Closes one model; closing the last one closes the panel. */
export function closeModel(id: string) {
  const models = state.models.filter((m) => m.id !== id);
  if (models.length === state.models.length) return;
  const activeId =
    state.activeId === id ? (models[models.length - 1]?.id ?? null) : state.activeId;
  set({ ...state, models, activeId, panelOpen: models.length > 0 && state.panelOpen });
}

export function closeAllModels() {
  if (!state.models.length) return;
  set({ ...state, models: [], activeId: null, panelOpen: false, expanded: false });
}

export function activateModel(id: string) {
  if (state.activeId === id || !state.models.some((m) => m.id === id)) return;
  set({ ...state, activeId: id });
}

/** The viewer's report on a load: what it found, or why it failed. */
export function settleModel(id: string, result: { info: ModelInfo } | { error: string }) {
  if (!state.models.some((m) => m.id === id)) return;
  set({
    ...state,
    models: state.models.map((m) =>
      m.id !== id
        ? m
        : "info" in result
          ? { ...m, status: "ready", info: result.info, error: null }
          : { ...m, status: "error", error: result.error },
    ),
  });
}

/** A rewritten file: load it again, keeping the camera where the user left it. */
export function reloadModel(id: string, modifiedMs: number) {
  if (!state.models.some((m) => m.id === id)) return;
  set({
    ...state,
    models: state.models.map((m) =>
      m.id === id ? { ...m, modifiedMs, revision: m.revision + 1, status: "loading" } : m,
    ),
  });
}

export function setModelPanel(patch: { open?: boolean; expanded?: boolean }) {
  const panelOpen = patch.open ?? state.panelOpen;
  const expanded = panelOpen ? (patch.expanded ?? state.expanded) : false;
  if (panelOpen === state.panelOpen && expanded === state.expanded) return;
  set({ ...state, panelOpen, expanded });
}

/** Resolves once the model has loaded or failed, or with null after `ms`. */
export function whenSettled(id: string, ms: number): Promise<OpenModel | null> {
  const now = state.models.find((m) => m.id === id);
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

export function useModels(): State {
  return useSyncExternalStore(subscribe, () => state);
}
