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
import { emit, listen } from "@tauri-apps/api/event";
import { loadState, saveState } from "./lib/ipc";
import { WINDOW_LABEL, isMainWindow } from "./lib/windows";
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
import { allPaneNames, paneNames, pickPaneName, uniquePaneNames } from "./lib/paneNames";
import {
  DEFAULT_SETTINGS,
  type AgentId,
  type AppState,
  type LayoutSize,
  type Pane,
  type SavedCommand,
  type Settings,
  type SplitNode,
  type Workspace,
  type WorkspaceFolder,
} from "./types";

const newId = () => crypto.randomUUID();

/** `taken` holds the names already in use, in every workspace. */
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
  /** Empty unless the workspace came from a multi-root `.code-workspace`. */
  extraRoots?: string[];
  layout: LayoutSize;
  /** One agent per pane, in grid order. */
  agents: AgentId[];
  shellId: string | null;
  /** The sidebar folder to file it under. Left out, it lands at the top level. */
  folderId?: string | null;
}

/** `taken` holds the pane names the other workspaces already use. */
export const makeWorkspace = (draft: WorkspaceDraft, taken: Iterable<string> = []): Workspace => {
  const others = [...taken];
  const panes: Pane[] = [];
  for (let i = 0; i < draft.layout; i++) {
    panes.push(makePane(draft.agents[i] ?? "shell", [...others, ...paneNames(panes)]));
  }
  return {
    id: newId(),
    name: draft.name,
    cwd: draft.cwd,
    extraRoots: draft.extraRoots ?? [],
    panes,
    themeId: null,
    shellId: draft.shellId,
    tree: presetTree(panes.map((pane) => pane.id)),
    savedCommands: [],
    folderId: draft.folderId ?? null,
  };
};

/**
 * Brings a stored workspace up to the shape the current version expects.
 * `folders` holds the folder ids the same file stores.
 */
const hydrateWorkspace = (ws: Workspace, folders: Set<string>): Workspace => ({
  ...ws,
  // A stored workspace without `panes` would take down everything that
  // counts them. Names are settled afterwards, across every workspace.
  panes: ws.panes ?? [],
  // Absent from everything saved before workspaces could hold extra roots.
  extraRoots: ws.extraRoots ?? [],
  // Same, for the workspaces stored before commands could be saved.
  savedCommands: ws.savedCommands ?? [],
  // Absent before folders existed. A folder the file no longer has would hide
  // the workspace from every list, so it falls back to the top level.
  folderId: ws.folderId && folders.has(ws.folderId) ? ws.folderId : null,
});

/** `item` moved — or added — just before `beforeId`, or last when there is no such entry. */
function placeBefore<T extends { id: string }>(
  list: T[],
  item: T,
  beforeId: string | null,
): T[] {
  const rest = list.filter((entry) => entry.id !== item.id);
  const at = beforeId === null ? -1 : rest.findIndex((entry) => entry.id === beforeId);
  return at < 0 ? [...rest, item] : [...rest.slice(0, at), item, ...rest.slice(at)];
}

/** Where a saved command lives: with its project, or with the settings. */
export type CommandScope = "workspace" | "global";

/** A saved command as the dialog hands it over — the id is the store's. */
export type SavedCommandDraft = Omit<SavedCommand, "id">;

const EMPTY: AppState = {
  workspaces: [],
  folders: [],
  activeWorkspaceId: null,
  settings: DEFAULT_SETTINGS,
};

interface StoreValue {
  state: AppState;
  /**
   * The state as every write so far left it, including the ones of this very
   * tick that React has not rendered yet. For code that writes and then needs
   * to read back what it wrote, like an agent creating a pane and asking for
   * its name.
   */
  snapshot: () => AppState;
  hydrated: boolean;
  activeWorkspace: Workspace | null;
  addWorkspace: (draft: WorkspaceDraft) => string;
  removeWorkspace: (id: string) => void;
  updateWorkspace: (id: string, patch: Partial<Workspace>) => void;
  setActiveWorkspace: (id: string | null) => void;
  /** Appends a folder, expanded, and returns its id. */
  addFolder: (name: string) => string;
  updateFolder: (id: string, patch: Partial<Omit<WorkspaceFolder, "id">>) => void;
  /** Deletes a folder. Its workspaces go back to the top level, untouched. */
  removeFolder: (id: string) => void;
  /** Places a folder just before `beforeId`, or last when that is null. */
  moveFolder: (id: string, beforeId: string | null) => void;
  /**
   * Files a workspace under `folderId` — null for the top level — just before
   * `beforeId`, or last in that folder when that is null.
   */
  moveWorkspace: (id: string, folderId: string | null, beforeId: string | null) => void;
  /**
   * Rebuilds an even arrangement with exactly `count` panes. Any pane it has to
   * create copies the agent of `modelPaneId` — the one you are working in —
   * because a preset is how you ask for more of what you already have.
   */
  applyPreset: (workspaceId: string, count: number, modelPaneId?: string | null) => void;
  setTree: (workspaceId: string, tree: SplitNode) => void;
  /**
   * Splits `near` (or the last pane) in two and returns the new pane id, or
   * `null` when the workspace is unknown or already full. A `cwd` opens the
   * new pane somewhere other than the workspace root.
   */
  addPane: (
    workspaceId: string,
    /** Without an `agent`, the new pane copies the one it was split off. */
    opts?: {
      agent?: AgentId;
      near?: string | null;
      dir?: "row" | "col";
      cwd?: string | null;
      /** Used when no other pane of the workspace has it already. */
      name?: string;
    },
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
  /**
   * Saved commands. Every one of these takes the scope it acts on plus the
   * workspace it was launched from — `workspaceId` is only read for the
   * workspace scope, and the caller always has one to give.
   */
  addSavedCommand: (
    scope: CommandScope,
    workspaceId: string,
    draft: SavedCommandDraft,
  ) => string;
  updateSavedCommand: (
    scope: CommandScope,
    workspaceId: string,
    id: string,
    patch: Partial<SavedCommandDraft>,
  ) => void;
  removeSavedCommand: (scope: CommandScope, workspaceId: string, id: string) => void;
  /**
   * Sends a command over to the other scope, keeping its id and landing it at
   * the end of the list it arrives in. Changing where a command is saved is an
   * edit like any other, which is why it is a move rather than a delete and a
   * create: the shortcut numbering is the only thing that shifts.
   */
  moveSavedCommand: (
    from: CommandScope,
    to: CommandScope,
    workspaceId: string,
    id: string,
  ) => void;
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
  const hydratedRef = useRef(false);
  hydratedRef.current = hydrated;

  /**
   * Every window holds a copy of the state (lib/windows). A write goes out to
   * the others once per tick, whole: the state is a few kilobytes, and a
   * whole state cannot be applied out of order the way a patch could.
   */
  const broadcastQueued = useRef(false);
  const broadcast = useCallback(() => {
    if (broadcastQueued.current) return;
    broadcastQueued.current = true;
    queueMicrotask(() => {
      broadcastQueued.current = false;
      emit("store:state", { from: WINDOW_LABEL, state: latest.current }).catch(() => undefined);
    });
  }, []);

  const update = useCallback(
    (fn: (prev: AppState) => AppState) => {
      latest.current = fn(latest.current);
      setState(latest.current);
      broadcast();
    },
    [broadcast],
  );

  const load = useCallback(
    (loaded: AppState | null) => {
      if (!loaded) return;
      const folders = loaded.folders ?? [];
      const folderIds = new Set(folders.map((folder) => folder.id));
      update(() => ({
        // Panes stored without a name, or under one another workspace had
        // first — names used to be unique per workspace only — get one here.
        workspaces: uniquePaneNames(
          (loaded.workspaces ?? []).map((ws) => hydrateWorkspace(ws, folderIds)),
        ),
        folders,
        activeWorkspaceId: loaded.activeWorkspaceId ?? null,
        // Merge so settings added in later versions get their defaults.
        settings: { ...DEFAULT_SETTINGS, ...(loaded.settings ?? {}) },
      }));
    },
    [update],
  );

  /**
   * The main window is the one source of truth: it adopts what another window
   * changed and sends it back out, so every window ends on the main one's
   * version even when two of them wrote in the same instant. A workspace
   * window only ever adopts what comes from the main one.
   */
  useEffect(() => {
    let cancelled = false;
    const stops: (() => void)[] = [];
    const keep = (stop: () => void) => (cancelled ? stop() : stops.push(stop));

    listen<{ from: string; state: AppState }>("store:state", (event) => {
      const { from, state: incoming } = event.payload;
      if (from === WINDOW_LABEL) return;
      if (!isMainWindow && from !== "main") return;
      latest.current = incoming;
      setState(incoming);
      if (isMainWindow) broadcast();
      else setHydrated(true);
    })
      .then((stop) => {
        keep(stop);
        // Asked only once listening, or the answer could come back before
        // anything is there to hear it.
        if (!isMainWindow && !cancelled) {
          emit("store:hello", { from: WINDOW_LABEL }).catch(() => undefined);
        }
      })
      .catch(() => undefined);

    if (isMainWindow) {
      // A window that just opened asks for the state rather than reading the
      // file, which can be up to a debounce behind.
      listen("store:hello", () => {
        if (hydratedRef.current) broadcast();
      })
        .then(keep)
        .catch(() => undefined);
    }

    return () => {
      cancelled = true;
      stops.forEach((stop) => stop());
    };
  }, [broadcast]);

  useEffect(() => {
    let cancelled = false;
    if (isMainWindow) {
      loadState()
        .then((loaded) => !cancelled && load(loaded))
        .catch(() => undefined)
        .finally(() => !cancelled && setHydrated(true));
      return () => {
        cancelled = true;
      };
    }
    // A workspace window: the main window answers its hello with the live
    // state. The file is only the fallback for a main window that never does.
    const fallback = window.setTimeout(() => {
      if (cancelled || hydratedRef.current) return;
      loadState()
        .then((loaded) => !cancelled && !hydratedRef.current && load(loaded))
        .catch(() => undefined)
        .finally(() => !cancelled && setHydrated(true));
    }, 2500);
    return () => {
      cancelled = true;
      window.clearTimeout(fallback);
    };
  }, [load]);

  // Debounced persistence, by the main window alone; never writes before the
  // initial load has landed.
  useEffect(() => {
    if (!hydrated || !isMainWindow) return;
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

  /** Rewrites one of the two saved-command lists, whichever `scope` names. */
  const mapCommands = useCallback(
    (
      scope: CommandScope,
      workspaceId: string,
      fn: (commands: SavedCommand[]) => SavedCommand[],
    ) => {
      if (scope === "global") {
        update((prev) => ({
          ...prev,
          settings: { ...prev.settings, savedCommands: fn(prev.settings.savedCommands) },
        }));
        return;
      }
      mapWorkspace(workspaceId, (ws) => ({
        ...ws,
        savedCommands: fn(ws.savedCommands),
      }));
    },
    [mapWorkspace, update],
  );

  const value = useMemo<StoreValue>(() => {
    const activeWorkspace =
      state.workspaces.find((ws) => ws.id === state.activeWorkspaceId) ?? null;

    return {
      state,
      snapshot: () => latest.current,
      hydrated,
      activeWorkspace,

      addWorkspace(draft) {
        const ws = makeWorkspace(draft, allPaneNames(latest.current.workspaces));
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

      addFolder(name) {
        const folder: WorkspaceFolder = { id: newId(), name, collapsed: false };
        update((prev) => ({ ...prev, folders: [...prev.folders, folder] }));
        return folder.id;
      },

      updateFolder(id, patch) {
        update((prev) => ({
          ...prev,
          folders: prev.folders.map((folder) =>
            folder.id === id ? { ...folder, ...patch } : folder,
          ),
        }));
      },

      removeFolder(id) {
        update((prev) => ({
          ...prev,
          folders: prev.folders.filter((folder) => folder.id !== id),
          workspaces: prev.workspaces.map((ws) =>
            ws.folderId === id ? { ...ws, folderId: null } : ws,
          ),
        }));
      },

      moveFolder(id, beforeId) {
        update((prev) => {
          const folder = prev.folders.find((entry) => entry.id === id);
          if (!folder || beforeId === id) return prev;
          return { ...prev, folders: placeBefore(prev.folders, folder, beforeId) };
        });
      },

      moveWorkspace(id, folderId, beforeId) {
        update((prev) => {
          const ws = prev.workspaces.find((entry) => entry.id === id);
          if (!ws || beforeId === id) return prev;
          // A folder deleted while the workspace was on its way leaves it at
          // the top level rather than filed under nothing.
          const target =
            folderId !== null && prev.folders.some((folder) => folder.id === folderId)
              ? folderId
              : null;
          return {
            ...prev,
            workspaces: placeBefore(prev.workspaces, { ...ws, folderId: target }, beforeId),
          };
        });
      },

      applyPreset(workspaceId, count, modelPaneId) {
        const size = Math.min(MAX_PANES, Math.max(1, Math.round(count)));
        // Read now: `mapWorkspace` applies its function synchronously, against
        // the very state this is taken from.
        const others = allPaneNames(
          latest.current.workspaces.filter((entry) => entry.id !== workspaceId),
        );
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
          // The pane you were in leads. Falling back to the last one kept means
          // a grid grown from a single agent comes up as that agent throughout,
          // which is the whole point of asking for four terminals.
          const model =
            arranged.find((pane) => pane.id === modelPaneId) ?? panes[panes.length - 1];
          const agent = model?.agent ?? "shell";
          while (panes.length < size) {
            panes.push(makePane(agent, [...others, ...paneNames(panes)]));
          }
          return { ...ws, panes, tree: presetTree(panes.map((pane) => pane.id)) };
        });
      },

      setTree(workspaceId, tree) {
        mapWorkspace(workspaceId, (ws) => ({ ...ws, tree }));
      },

      addPane(workspaceId, opts = {}) {
        // Built outside the updater so the caller can focus it right away. Its
        // agent is settled inside, where the pane being split can be read.
        const pane = makePane(opts.agent ?? "shell");
        const zone = (opts.dir ?? "row") === "row" ? "right" : "bottom";
        // The updater is the only place that sees the workspace as it stands,
        // so it is also the only one that knows whether the pane made it in:
        // two adds in the same tick can leave the second one out at MAX_PANES.
        let inserted = false;
        const others = allPaneNames(
          latest.current.workspaces.filter((entry) => entry.id !== workspaceId),
        );

        mapWorkspace(workspaceId, (ws) => {
          if (ws.panes.length >= MAX_PANES) return ws;
          if (ws.panes.some((p) => p.id === pane.id)) return ws;
          const tree = normalizeTree(ws.panes, ws.tree);
          const ids = leafIds(tree);
          const near =
            opts.near && ids.includes(opts.near) ? opts.near : ids[ids.length - 1];
          // Named against the workspace as it stands now, so two adds in the
          // same tick cannot land on the same name.
          // Splitting a Claude Code pane asks for another one, not for a bare
          // prompt. An explicit agent from the caller still wins.
          const inherited = ws.panes.find((p) => p.id === near)?.agent;
          const wanted = opts.name?.trim();
          const taken = [...others, ...paneNames(ws.panes)];
          const named = {
            ...pane,
            name: wanted && !taken.includes(wanted) ? wanted : pickPaneName(taken),
            agent: opts.agent ?? inherited ?? "shell",
            // Left alone by default: a pane follows the workspace root.
            cwd: opts.cwd ?? pane.cwd,
          };
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

      addSavedCommand(scope, workspaceId, draft) {
        const command: SavedCommand = { ...draft, id: newId() };
        mapCommands(scope, workspaceId, (commands) => [...commands, command]);
        return command.id;
      },

      updateSavedCommand(scope, workspaceId, id, patch) {
        mapCommands(scope, workspaceId, (commands) =>
          commands.map((command) =>
            command.id === id ? { ...command, ...patch } : command,
          ),
        );
      },

      removeSavedCommand(scope, workspaceId, id) {
        mapCommands(scope, workspaceId, (commands) =>
          commands.filter((command) => command.id !== id),
        );
      },

      moveSavedCommand(from, to, workspaceId, id) {
        if (from === to) return;
        // One updater rather than a remove and an add: both lists are rewritten
        // against the same snapshot, so the command is never in both at once
        // and never in neither.
        update((prev) => {
          const workspace = prev.workspaces.find((ws) => ws.id === workspaceId) ?? null;
          // A workspace closed since the menu was opened has nowhere to take it.
          if (to === "workspace" && !workspace) return prev;
          const source =
            from === "global" ? prev.settings.savedCommands : (workspace?.savedCommands ?? []);
          const moved = source.find((command) => command.id === id);
          if (!moved) return prev;

          const drop = (commands: SavedCommand[]) =>
            commands.filter((command) => command.id !== id);
          const append = (commands: SavedCommand[]) => [...drop(commands), moved];

          return {
            ...prev,
            settings: {
              ...prev.settings,
              savedCommands:
                to === "global"
                  ? append(prev.settings.savedCommands)
                  : drop(prev.settings.savedCommands),
            },
            workspaces: prev.workspaces.map((ws) =>
              ws.id === workspaceId
                ? {
                    ...ws,
                    savedCommands:
                      to === "workspace" ? append(ws.savedCommands) : drop(ws.savedCommands),
                  }
                : ws,
            ),
          };
        });
      },
    };
  }, [state, hydrated, mapCommands, mapWorkspace, update]);

  return <StoreContext.Provider value={value}>{children}</StoreContext.Provider>;
}

export function useStore(): StoreValue {
  const ctx = useContext(StoreContext);
  if (!ctx) throw new Error("useStore must be used inside StoreProvider");
  return ctx;
}
