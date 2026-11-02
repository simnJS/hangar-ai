import { useSyncExternalStore } from "react";
import type { ModelFormat } from "./modelFiles";
import type { FileKind } from "./preview";

/**
 * What is open in this window's previews, one preview per workspace: the
 * files each workspace has open — 3D models, images, videos, sounds, text,
 * Markdown, PDF, web pages — and the tabs of its kits (docs/kits.md), which
 * is in front, whether its panel shows and whether it fills the work area. Switching workspaces switches previews; a
 * file an agent shows in a workspace that is not on screen waits there.
 *
 * Written by the MCP control handlers (an agent showing a file), by the
 * panel itself (the user closing a tab, the 3D viewer reporting what it
 * loaded) and read by both. Outside React for the same reason as
 * lib/paneInsight: the writers are nowhere near the panel in the tree.
 *
 * Entries are ids across the whole window: the one 3D viewer the window has
 * (ModelPanel) holds the models of every workspace, whichever it belongs to.
 * Files are not saved, and a reload starts without them. A kit's tabs are, with
 * their params: they come back with their workspace (`restoreKitTabs`).
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

/**
 * A kit's tab: a page of the kit, framed and isolated
 * (components/preview/KitTabView). Which page, and what it may ask, is read
 * from the installed kit and the workspace each time it is drawn, never
 * kept here: a kit updated or taken off the workspace changes it at once.
 * `path` is `kit:<kit id>/<tab id>`, and the params that tell it apart from its
 * siblings (`kitTabPath`), which no file's path can be.
 */
export interface OpenKitTab extends OpenEntry {
  kind: "kit";
  kitId: string;
  tabId: string;
  /** What it was opened with, which its page reads (`tab.params`). */
  params: KitTabParams;
  /** Its state in the tab strip (`tab.setBadge`), until the page next loads. */
  badge: KitTabBadge | null;
  /** The page named it (`tab.setTitle`): asking for it again leaves that name. */
  titled: boolean;
  /**
   * Brought back at startup and not shown since: its page is not loaded yet
   * (`kitTabLoads`). Never saved; it ends when the tab is first shown, reloaded,
   * or asked for again by an agent or a hook.
   */
  restored: boolean;
}

/** What a caller passes a kit's tab, by the names its manifest declares. */
export type KitTabParams = Readonly<Record<string, string>>;

/** The pane of the agent or hook that last asked for a kit's tab, which its page can answer. */
export interface KitTabCaller {
  pane: string;
}

export const KIT_BADGE_STATES = ["running", "success", "failure", "neutral"] as const;
export type KitBadgeState = (typeof KIT_BADGE_STATES)[number];

export interface KitTabBadge {
  state: KitBadgeState;
  text: string | null;
}

/** A tab of the preview. */
export type OpenPreview = OpenModel | OpenFile | OpenKitTab;

export const isModel = (entry: OpenPreview): entry is OpenModel => entry.kind === "model";

export const isKitTab = (entry: OpenPreview): entry is OpenKitTab => entry.kind === "kit";

/** A file of the preview: neither a 3D model nor a kit's tab. */
export const isPreviewFile = (entry: OpenPreview): entry is OpenFile =>
  entry.kind !== "model" && entry.kind !== "kit";

/** A set of params, the same whichever order its names came in. */
export const kitParamsKey = (params: KitTabParams) =>
  JSON.stringify(Object.entries(params).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));

const hasParams = (params: KitTabParams) => Object.keys(params).length > 0;

/**
 * What stands for a kit's tab where a file has its path: one per tab, or, given
 * params, one per set of them. A singleton's stays the same whatever it holds.
 */
export const kitTabPath = (kitId: string, tabId: string, params: KitTabParams = {}) =>
  hasParams(params) ? `kit:${kitId}/${tabId}?${kitParamsKey(params)}` : `kit:${kitId}/${tabId}`;

/** What opening a kit's tab takes. */
export interface KitTabToOpen {
  workspaceId: string;
  kitId: string;
  tabId: string;
  title: string;
  params?: KitTabParams;
  /** One tab whatever its params: new ones replace the old. */
  singleton?: boolean;
  /** The pane of the agent or hook that asks; null when the person does. */
  openedBy?: string | null;
  /** Opens the panel, the tab in front. False adds it without either. Default true. */
  reveal?: boolean;
}

/** What opening a file takes, its id and workspace aside. */
type FileToShow =
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

/** And a kit's tab, so the preview of a workspace in another window can open one too. */
export type KitTabToShow = Omit<KitTabToOpen, "workspaceId" | "openedBy"> & {
  kind: "kit";
  path: string;
  openedBy: string | null;
};

export type PreviewToShow = FileToShow | KitTabToShow;

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
  /** Every workspace's kit tabs: the pages this window keeps running. */
  kits: OpenKitTab[];
}

const EMPTY: ModelScope = Object.freeze({
  models: [],
  activeId: null,
  panelOpen: false,
  expanded: false,
  unseen: false,
}) as ModelScope;

let state: State = { scopes: {}, entries: [], all: [], kits: [] };
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
  // The same for the kit tabs: their frames are not woken by a file opening.
  const tabs = entries.filter(isKitTab);
  const kits =
    tabs.length === state.kits.length && tabs.every((tab, i) => tab === state.kits[i])
      ? state.kits
      : tabs;
  state = { scopes, entries, all, kits };
  for (const workspaceId of Object.keys(patch)) saveKitTabs(workspaceId);
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
const kindFields = (file: FileToShow) =>
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
  if (file.kind === "kit") return openKitTab(file);
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
 * The open tab a request lands on: a singleton's one tab whatever it holds, or
 * the tab holding this very set of params.
 */
export function findKitTab(
  models: readonly OpenPreview[],
  tab: Pick<KitTabToOpen, "kitId" | "tabId" | "params" | "singleton">,
): OpenKitTab | undefined {
  const key = kitParamsKey(tab.params ?? {});
  return models
    .filter(isKitTab)
    .find(
      (entry) =>
        entry.kitId === tab.kitId &&
        entry.tabId === tab.tabId &&
        (tab.singleton || kitParamsKey(entry.params) === key),
    );
}

/**
 * Opens a kit's tab in its workspace's preview, or brings it forward when it
 * is open already — as it is: a page someone is working in is not reloaded
 * for being asked for again. A singleton given other params takes them; none given
 * leaves what it holds.
 *
 * An agent or a hook asking for an open tab becomes who its page answers
 * (`kitTabCaller`), and the page hears of it (`watchKitTabParams`), as it does of
 * new params. The person bringing it forward changes neither: the agent that asked
 * is still the one waiting.
 *
 * With `reveal`, the default, it opens that workspace's panel with the tab in
 * front; without, it is only added, the panel and what is in front as they were.
 * Neither ever moves the keyboard. What an agent or a hook opens is news to the
 * person, which the toolbar's button says; what they open themselves is not.
 * Returns the entry's id.
 */
export function openKitTab(tab: KitTabToOpen): string {
  restoreKitTabs(tab.workspaceId);
  const scope = modelScope(tab.workspaceId);
  const params = tab.params ?? {};
  const openedBy = tab.openedBy ?? null;
  const reveal = tab.reveal !== false;
  const title = tab.title.trim() || tab.tabId;
  const existing = findKitTab(scope.models, tab);
  if (existing) {
    const replaced = hasParams(params) && kitParamsKey(params) !== kitParamsKey(existing.params);
    set({
      [tab.workspaceId]: {
        ...withEntry(scope, existing.id, (entry) => ({
          ...entry,
          ...(existing.titled ? {} : { title }),
          openedBy: openedBy ?? entry.openedBy,
          // Asked for by an agent or a hook: it may be followed, so it loads.
          ...(openedBy !== null && isKitTab(entry) ? { restored: false } : {}),
          ...(replaced ? { params } : {}),
        })),
        ...(reveal ? { activeId: existing.id, panelOpen: true } : {}),
        unseen: scope.unseen || (openedBy !== null && (reveal || replaced)),
      },
    });
    if (replaced || openedBy !== null) {
      for (const watcher of paramWatchers.get(existing.id) ?? []) {
        watcher(kitTabParams(existing.id), kitTabCaller(existing.id));
      }
    }
    return existing.id;
  }
  const id = newId();
  const entry = kitTabEntry({
    id,
    workspaceId: tab.workspaceId,
    kitId: tab.kitId,
    tabId: tab.tabId,
    path: kitTabPath(tab.kitId, tab.tabId, tab.singleton ? {} : params),
    title,
    titled: false,
    params,
    openedBy,
  });
  set({
    [tab.workspaceId]: {
      ...scope,
      models: [...scope.models, entry],
      activeId: reveal || !scope.activeId ? id : scope.activeId,
      panelOpen: reveal || scope.panelOpen,
      unseen: scope.unseen || openedBy !== null,
    },
  });
  return id;
}

const kitTabEntry = (
  tab: Pick<OpenKitTab, "id" | "workspaceId" | "kitId" | "tabId" | "path" | "title" | "titled" | "params" | "openedBy">,
): OpenKitTab => ({
  ...tab,
  kind: "kit",
  badge: null,
  restored: false,
  openedAt: Date.now(),
  modifiedMs: 0,
  revision: 0,
  status: "ready",
  error: null,
});

/** Loads a kit tab's page afresh: its reload button. What the last page said of its state goes with it. */
export function reloadKitTab(id: string) {
  const entry = find(id);
  if (!entry || !isKitTab(entry)) return;
  set({
    [entry.workspaceId]: withEntry(modelScope(entry.workspaceId), id, (e) => ({
      ...e,
      revision: e.revision + 1,
      badge: null,
      restored: false,
    })),
  });
}

/** A restored kit tab has been shown: from now on its page is loaded like any other. */
export function markKitTabShown(id: string) {
  const entry = find(id);
  if (!entry || !isKitTab(entry) || !entry.restored) return;
  set({ [entry.workspaceId]: withEntry(modelScope(entry.workspaceId), id, (e) => ({ ...e, restored: false })) });
}

/** What a kit's tab holds now; none once it is closed. */
export function kitTabParams(id: string): KitTabParams {
  const entry = find(id);
  return entry && isKitTab(entry) ? entry.params : {};
}

/**
 * Who a kit's tab answers: the pane of the agent or hook that last asked for it,
 * from Hangar's own record of the request. Null when only the person did, and once
 * it is closed.
 */
export function kitTabCaller(id: string): KitTabCaller | null {
  const entry = find(id);
  return entry && isKitTab(entry) && entry.openedBy ? { pane: entry.openedBy } : null;
}

type KitTabWatcher = (params: KitTabParams, caller: KitTabCaller | null) => void;

const paramWatchers = new Map<string, Set<KitTabWatcher>>();

/** Hears a tab taking new params, or an agent or a hook asking for it again. Returns the unsubscribe. */
export function watchKitTabParams(id: string, watcher: KitTabWatcher): () => void {
  const watchers = paramWatchers.get(id) ?? new Set();
  watchers.add(watcher);
  paramWatchers.set(id, watchers);
  return () => {
    watchers.delete(watcher);
    if (!watchers.size) paramWatchers.delete(id);
  };
}

/** A kit tab's own name for itself (`tab.setTitle`), checked by lib/kitApi. */
export function setKitTabTitle(id: string, title: string) {
  const entry = find(id);
  if (!entry || !isKitTab(entry) || (entry.titled && entry.title === title)) return;
  set({
    [entry.workspaceId]: withEntry(modelScope(entry.workspaceId), id, (e) => ({ ...e, title, titled: true })),
  });
}

/** A kit tab's state in the tab strip (`tab.setBadge`), checked by lib/kitApi; null takes it away. */
export function setKitTabBadge(id: string, badge: KitTabBadge | null) {
  const entry = find(id);
  if (!entry || !isKitTab(entry)) return;
  if (entry.badge?.state === badge?.state && entry.badge?.text === badge?.text) return;
  set({
    [entry.workspaceId]: withEntry(modelScope(entry.workspaceId), id, (e) => ({ ...e, badge })),
  });
}

/* ── a kit's tabs, across a restart ───────────────────────────── */

/**
 * One key per workspace, in the window's storage, written by the window that
 * draws it: a workspace in a window of its own is that window's alone, and two
 * windows cannot overwrite each other's. Only what a page cannot say again is
 * kept: its badge starts over with the page.
 */
const SAVED_PREFIX = "hangar.kitTabs.";

type SavedKitTab = Pick<OpenKitTab, "kitId" | "tabId" | "path" | "title" | "titled" | "params" | "openedBy">;

/** The storage, or null where there is none (under test, or refused). */
function savedTabs(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** The workspaces whose saved tabs this window has read: until then, saving would lose them. */
const restored = new Set<string>();

function saveKitTabs(workspaceId: string) {
  if (!restored.has(workspaceId)) return;
  const tabs = (state.scopes[workspaceId]?.models ?? []).filter(isKitTab);
  try {
    if (tabs.length) {
      const saved = tabs.map(
        ({ kitId, tabId, path, title, titled, params, openedBy }): SavedKitTab => ({
          kitId,
          tabId,
          path,
          title,
          titled,
          params,
          openedBy,
        }),
      );
      savedTabs()?.setItem(SAVED_PREFIX + workspaceId, JSON.stringify(saved));
    } else {
      savedTabs()?.removeItem(SAVED_PREFIX + workspaceId);
    }
  } catch {
    /* Storage full or refused: the tabs stay open, and only a restart loses them. */
  }
}

/** A saved tab, read with care: the storage can hold anything. */
function readSavedTab(raw: unknown): SavedKitTab | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { kitId, tabId, path, title, titled, params, openedBy } = raw as Record<string, unknown>;
  if (typeof kitId !== "string" || typeof tabId !== "string") return null;
  if (typeof path !== "string" || !path.startsWith(kitTabPath(kitId, tabId))) return null;
  if (typeof title !== "string" || typeof params !== "object" || params === null || Array.isArray(params)) return null;
  const values = Object.entries(params);
  if (!values.every(([, value]) => typeof value === "string")) return null;
  return {
    kitId,
    tabId,
    path,
    title,
    titled: titled === true,
    params: Object.fromEntries(values) as KitTabParams,
    openedBy: typeof openedBy === "string" ? openedBy : null,
  };
}

function readSavedTabs(workspaceId: string): SavedKitTab[] {
  try {
    const raw: unknown = JSON.parse(savedTabs()?.getItem(SAVED_PREFIX + workspaceId) ?? "[]");
    return Array.isArray(raw) ? raw.map(readSavedTab).filter((tab): tab is SavedKitTab => tab !== null) : [];
  } catch {
    return [];
  }
}

/**
 * Brings back the kit tabs a workspace had open when the app last closed, with
 * their params, once for this window. Called when the window first mounts the
 * workspace, so no page runs for a workspace nobody opened. They come back behind
 * a closed panel: a restart does not put anything in front of anyone, and is not
 * news. What is open already stays, and is not opened twice.
 */
export function restoreKitTabs(workspaceId: string) {
  if (restored.has(workspaceId)) return;
  restored.add(workspaceId);
  const scope = modelScope(workspaceId);
  const paths = new Set(scope.models.map((entry) => entry.path));
  const arriving = readSavedTabs(workspaceId)
    .filter((tab) => !paths.has(tab.path))
    .map((tab) => ({ ...kitTabEntry({ ...tab, id: newId(), workspaceId }), restored: true }));
  if (!arriving.length) return saveKitTabs(workspaceId);
  set({
    [workspaceId]: {
      ...scope,
      models: [...scope.models, ...arriving],
      activeId: scope.activeId ?? arriving[arriving.length - 1].id,
    },
  });
}

/** Forgets what the workspaces that are not in `live` saved: deleted since. */
export function forgetSavedKitTabs(live: Iterable<string>) {
  const storage = savedTabs();
  if (!storage) return;
  const keep = new Set([...live].map((id) => SAVED_PREFIX + id));
  try {
    const gone = Array.from({ length: storage.length }, (_, at) => storage.key(at)).filter(
      (key): key is string => key !== null && key.startsWith(SAVED_PREFIX) && !keep.has(key),
    );
    for (const key of gone) storage.removeItem(key);
  } catch {
    /* Nothing depends on it: a stale key only takes a little room. */
  }
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
  // A kit's tab has no file to watch: its own button reloads it.
  if (!entry || isKitTab(entry)) return;
  set({
    [entry.workspaceId]: {
      ...withEntry(modelScope(entry.workspaceId), id, (e) =>
        isModel(e)
          ? { ...e, modifiedMs, revision: e.revision + 1, status: "loading" }
          : isPreviewFile(e)
            ? { ...e, modifiedMs, revision: e.revision + 1, size: size ?? e.size }
            : e,
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
  // Nothing new, as this window restored them itself: the window they came from
  // has just forgotten them, though, so they are saved again from here.
  if (!arriving.length) return saveKitTabs(workspaceId);
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

/** Every kit tab open in this window, whichever workspace has it. */
export function useWindowKitTabs(): OpenKitTab[] {
  return useSyncExternalStore(subscribe, () => state.kits);
}

/** Whether any workspace of this window has anything open in its preview; a load landing does not change it. */
export function useAnyModels(): boolean {
  return useSyncExternalStore(subscribe, () => state.entries.length > 0);
}
