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
  DEFAULT_SETTINGS,
  type AgentId,
  type AppState,
  type LayoutSize,
  type Pane,
  type Settings,
  type Workspace,
} from "./types";

const newId = () => crypto.randomUUID();

export const makePane = (agent: AgentId = "shell"): Pane => ({
  id: newId(),
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

export const makeWorkspace = (draft: WorkspaceDraft): Workspace => ({
  id: newId(),
  name: draft.name,
  cwd: draft.cwd,
  layout: draft.layout,
  panes: Array.from({ length: draft.layout }, (_, i) => makePane(draft.agents[i] ?? "shell")),
  themeId: null,
  shellId: draft.shellId,
  tree: null,
});

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
  setLayout: (workspaceId: string, layout: LayoutSize) => void;
  updatePane: (workspaceId: string, paneId: string, patch: Partial<Pane>) => void;
  resetPane: (workspaceId: string, paneId: string) => void;
  updateSettings: (patch: Partial<Settings>) => void;
}

const StoreContext = createContext<StoreValue | null>(null);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AppState>(EMPTY);
  const [hydrated, setHydrated] = useState(false);
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    loadState()
      .then((loaded) => {
        if (cancelled) return;
        if (loaded) {
          setState({
            workspaces: loaded.workspaces ?? [],
            activeWorkspaceId: loaded.activeWorkspaceId ?? null,
            // Merge so settings added in later versions get their defaults.
            settings: { ...DEFAULT_SETTINGS, ...(loaded.settings ?? {}) },
          });
        }
      })
      .catch(() => undefined)
      .finally(() => !cancelled && setHydrated(true));
    return () => {
      cancelled = true;
    };
  }, []);

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

  const mapWorkspace = useCallback(
    (id: string, fn: (ws: Workspace) => Workspace) =>
      setState((prev) => ({
        ...prev,
        workspaces: prev.workspaces.map((ws) => (ws.id === id ? fn(ws) : ws)),
      })),
    [],
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
        setState((prev) => ({
          ...prev,
          workspaces: [...prev.workspaces, ws],
          activeWorkspaceId: ws.id,
        }));
        return ws.id;
      },

      removeWorkspace(id) {
        setState((prev) => {
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
        setState((prev) => ({ ...prev, activeWorkspaceId: id }));
      },

      setLayout(workspaceId, layout) {
        mapWorkspace(workspaceId, (ws) => {
          const panes = ws.panes.slice(0, layout);
          while (panes.length < layout) panes.push(makePane());
          // The stored split tree describes the old shape; drop it.
          return { ...ws, layout, panes, tree: null };
        });
      },

      updatePane(workspaceId, paneId, patch) {
        mapWorkspace(workspaceId, (ws) => ({
          ...ws,
          panes: ws.panes.map((p) => (p.id === paneId ? { ...p, ...patch } : p)),
        }));
      },

      /** Gives the pane a brand new identity so its terminal remounts clean. */
      resetPane(workspaceId, paneId) {
        mapWorkspace(workspaceId, (ws) => ({
          ...ws,
          panes: ws.panes.map((p) =>
            p.id === paneId ? { ...makePane(p.agent), cwd: p.cwd, title: p.title } : p,
          ),
        }));
      },

      updateSettings(patch) {
        setState((prev) => ({ ...prev, settings: { ...prev.settings, ...patch } }));
      },
    };
  }, [state, hydrated, mapWorkspace]);

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}

export function useStore(): StoreValue {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore must be used inside StoreProvider");
  return ctx;
}
