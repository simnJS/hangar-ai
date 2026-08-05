import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { loadState, saveState } from "./lib/ipc";
import {
  insertLeaf,
  leafIds,
  MAX_PANES,
  moveLeaf,
  normalizeTree,
  presetTree,
  removeLeaf,
  renameLeaf,
  type Zone,
} from "./lib/layout";
import { nameWorkspacePanes, paneNames, pickPaneName } from "./lib/paneNames";
import {
  DEFAULT_SETTINGS,
  type AgentId,
  type AppState,
  type LayoutSize,
  type Pane,
  type Settings,
  type SplitNode,
  type Workspace,
} from "./types";

const newId = () => crypto.randomUUID();

/** `taken` holds the names already used in the target workspace. */
export const makePane = (agent: AgentId = "shell", taken: Iterable<string> = []): Pane => ({
  id: newId(),
  name: pickPaneName(taken),
  agent,
  sessionId: null,
  cwd: null,
  shellId: null,
  title: null,
});

export interface WorkspaceDraft {
  name: string;
  cwd: string;
  layout: LayoutSize;
  /** One agent per pane, in grid order. */
  agents: AgentId[];
  shellId: string | null;
}

export const makeWorkspace = (draft: WorkspaceDraft): Workspace => {
  const panes: Pane[] = [];
  for (let i = 0; i < draft.layout; i++) {
    panes.push(makePane(draft.agents[i] ?? "shell", paneNames(panes)));
  }
  return {
    id: newId(),
    name: draft.name,
    cwd: draft.cwd,
    panes,
    themeId: null,
    shellId: draft.shellId,
    tree: presetTree(panes.map((pane) => pane.id)),
  };
};

const EMPTY: AppState = {
  workspaces: [],
  activeWorkspaceId: null,
  settings: DEFAULT_SETTINGS,
};

interface StoreValue {
  state: AppState;
  hydrated: boolean;
  activeWorkspace: Workspace | null;
  addWorkspace: (draft: WorkspaceDraft) => string;
  removeWorkspace: (id: string) => void;
  updateWorkspace: (id: string, patch: Partial<Workspace>) => void;
  setActiveWorkspace: (id: string | null) => void;
  /** Rebuilds an even arrangement with exactly `count` panes. */
  applyPreset: (workspaceId: string, count: number) => void;
  setTree: (workspaceId: string, tree: SplitNode) => void;
  /**
   * Splits `near` (or the last pane) in two and returns the new pane id, or
   * `null` when the workspace is unknown or already full.
   */
  addPane: (
    workspaceId: string,
    opts?: { agent?: AgentId; near?: string | null; dir?: "row" | "col" },
  ) => string | null;
  closePane: (workspaceId: string, paneId: string) => void;
  movePane: (
    workspaceId: string,
    dragId: string,
    targetId: string,
    zone: Zone,
  ) => void;
  updatePane: (workspaceId: string, paneId: string, patch: Partial<Pane>) => void;
  /**
   * Patches a pane and gives it a fresh id, which remounts its terminal.
   * Returns that id, or `null` when the pane is no longer there.
   */
  respawnPane: (
    workspaceId: string,
    paneId: string,
    patch?: Partial<Pane>,
  ) => string | null;
  updateSettings: (patch: Partial<Settings>) => void;
}

const StoreContext = createContext<StoreValue | null>(null);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AppState>(EMPTY);
  const [hydrated, setHydrated] = useState(false);
  const saveTimer = useRef<number | null>(null);

  /**
   * The state as the writes already queued in this tick will leave it. React
   * only runs a `setState` updater at render time, so two writes in the same
   * tick would both decide against the same stale snapshot — and neither
   * could report what it ended up doing. Everything writes through `update`,
   * which applies its function to the ref immediately.
   */
  const latest = useRef<AppState>(EMPTY);

  const update = useCallback((fn: (prev: AppState) => AppState) => {
    latest.current = fn(latest.current);
    setState(latest.current);
  }, []);

  useEffect(() => {
    let cancelled = false;
    loadState()
      .then((loaded) => {
        if (cancelled) return;
        if (loaded) {
          update(() => ({
            // Workspaces stored before panes had names get them here.
            workspaces: (loaded.workspaces ?? []).map(nameWorkspacePanes),
            activeWorkspaceId: loaded.activeWorkspaceId ?? null,
            // Merge so settings added in later versions get their defaults.
            settings: { ...DEFAULT_SETTINGS, ...(loaded.settings ?? {}) },
          }));
        }
      })
      .catch(() => undefined)
      .finally(() => !cancelled && setHydrated(true));
    return () => {
      cancelled = true;
    };
  }, [update]);

  // Debounced persistence; never writes before the initial load has landed.
  useEffect(() => {
    if (!hydrated) return;
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      saveState(state).catch(() => undefined);
    }, 400);
    return () => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
    };
  }, [state, hydrated]);

  /** `fn` runs synchronously, against the workspace as the last write left it. */
  const mapWorkspace = useCallback(
    (id: string, fn: (ws: Workspace) => Workspace) =>
      update((prev) => ({
        ...prev,
        workspaces: prev.workspaces.map((ws) => (ws.id === id ? fn(ws) : ws)),
      })),
    [update],
  );

  const value = useMemo<StoreValue>(() => {
    const activeWorkspace =
      state.workspaces.find((ws) => ws.id === state.activeWorkspaceId) ?? null;

    return {
      state,
      hydrated,
      activeWorkspace,

      addWorkspace(draft) {
        const ws = makeWorkspace(draft);
        update((prev) => ({
          ...prev,
          workspaces: [...prev.workspaces, ws],
          activeWorkspaceId: ws.id,
        }));
        return ws.id;
      },

      removeWorkspace(id) {
        update((prev) => {
          const workspaces = prev.workspaces.filter((ws) => ws.id !== id);
          return {
            ...prev,
            workspaces,
            activeWorkspaceId:
              prev.activeWorkspaceId === id
                ? (workspaces[0]?.id ?? null)
                : prev.activeWorkspaceId,
          };
        });
      },

      updateWorkspace(id, patch) {
        mapWorkspace(id, (ws) => ({ ...ws, ...patch }));
      },

      setActiveWorkspace(id) {
        update((prev) => ({ ...prev, activeWorkspaceId: id }));
      },

      applyPreset(workspaceId, count) {
        const size = Math.min(MAX_PANES, Math.max(1, Math.round(count)));
        mapWorkspace(workspaceId, (ws) => {
          // Reading order, not creation order: the panes you see first are the
          // ones a smaller preset keeps. Sorting by id order instead would drop
          // whichever terminals happened to be spawned last, which after a few
          // moves has nothing to do with what is on screen.
          const order = leafIds(normalizeTree(ws.panes, ws.tree));
          const byId = new Map(ws.panes.map((pane) => [pane.id, pane]));
          const arranged = order
            .map((id) => byId.get(id))
            .filter((pane): pane is Pane => Boolean(pane));

          // Same pane objects, same ids: the survivors keep their PTY and only
          // get resized. Only a shrinking preset ever closes anything.
          const panes = arranged.slice(0, size);
          while (panes.length < size) panes.push(makePane("shell", paneNames(panes)));
          return { ...ws, panes, tree: presetTree(panes.map((pane) => pane.id)) };
        });
      },

      setTree(workspaceId, tree) {
        mapWorkspace(workspaceId, (ws) => ({ ...ws, tree }));
      },

      addPane(workspaceId, opts = {}) {
        // Built outside the updater so the caller can focus it right away.
        const pane = makePane(opts.agent ?? "shell");
        const zone = (opts.dir ?? "row") === "row" ? "right" : "bottom";
        // The updater is the only place that sees the workspace as it stands,
        // so it is also the only one that knows whether the pane made it in:
        // two adds in the same tick can leave the second one out at MAX_PANES.
        let inserted = false;

        mapWorkspace(workspaceId, (ws) => {
          if (ws.panes.length >= MAX_PANES) return ws;
          if (ws.panes.some((p) => p.id === pane.id)) return ws;
          const tree = normalizeTree(ws.panes, ws.tree);
          const ids = leafIds(tree);
          const near =
            opts.near && ids.includes(opts.near) ? opts.near : ids[ids.length - 1];
          // Named against the workspace as it stands now, so two adds in the
          // same tick cannot land on the same name.
          const named = { ...pane, name: pickPaneName(paneNames(ws.panes)) };
          inserted = true;
          return {
            ...ws,
            panes: [...ws.panes, named],
            tree: insertLeaf(tree, near, pane.id, zone),
          };
        });

        // No insertion, no id: the caller would focus a pane that never was.
        return inserted ? pane.id : null;
      },

      closePane(workspaceId, paneId) {
        mapWorkspace(workspaceId, (ws) => {
          // The last pane stays: an empty workspace has nothing to act on.
          if (ws.panes.length <= 1) return ws;
          const panes = ws.panes.filter((pane) => pane.id !== paneId);
          if (panes.length === ws.panes.length) return ws;
          const tree = removeLeaf(normalizeTree(ws.panes, ws.tree), paneId);
          return { ...ws, panes, tree: tree ?? presetTree(panes.map((p) => p.id)) };
        });
      },

      movePane(workspaceId, dragId, targetId, zone) {
        if (dragId === targetId) return;
        mapWorkspace(workspaceId, (ws) => ({
          ...ws,
          tree: moveLeaf(normalizeTree(ws.panes, ws.tree), dragId, targetId, zone),
        }));
      },

      updatePane(workspaceId, paneId, patch) {
        mapWorkspace(workspaceId, (ws) => ({
          ...ws,
          panes: ws.panes.map((p) => (p.id === paneId ? { ...p, ...patch } : p)),
        }));
      },

      respawnPane(workspaceId, paneId, patch = {}) {
        const nextId = newId();
        // The pane may have been closed — or already respawned, which changed
        // its id — since the caller last looked.
        let respawned = false;
        mapWorkspace(workspaceId, (ws) => {
          if (!ws.panes.some((pane) => pane.id === paneId)) return ws;
          respawned = true;
          return {
            ...ws,
            panes: ws.panes.map((pane) =>
              pane.id === paneId ? { ...pane, ...patch, id: nextId } : pane,
            ),
            // The arrangement is keyed by pane id: follow the rename.
            tree: renameLeaf(normalizeTree(ws.panes, ws.tree), paneId, nextId),
          };
        });
        return respawned ? nextId : null;
      },

      updateSettings(patch) {
        update((prev) => ({ ...prev, settings: { ...prev.settings, ...patch } }));
      },
    };
  }, [state, hydrated, mapWorkspace, update]);

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}

export function useStore(): StoreValue {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore must be used inside StoreProvider");
  return ctx;
}
