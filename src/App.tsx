import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Sidebar } from "./components/Sidebar";
import { SettingsPage } from "./components/settings/SettingsPage";
import { SessionPicker } from "./components/SessionPicker";
import { WorkspaceDialog } from "./components/WorkspaceDialog";
import { WorkspaceTerminals } from "./components/WorkspaceTerminals";
import { BoardView } from "./components/BoardView";
import { BoardDock } from "./components/BoardDock";
import { ModelPanel } from "./components/ModelPanel";
import { MemoryView } from "./components/MemoryView";
import { StatsView } from "./components/StatsView";
import { McpPanel } from "./components/McpPanel";
import { McpServersPanel } from "./components/McpServersPanel";
import { UpdateBanner } from "./components/UpdateBanner";
import { VoiceHud } from "./components/VoiceHud";
import { Toolbar, type View } from "./components/Toolbar";
import { StatusBar } from "./components/StatusBar";
import { Inspector } from "./components/Inspector";
import { CommandPalette } from "./components/CommandPalette";
import { NeedsYou, type LandItem } from "./components/NeedsYou";
import { OrchestrationPanel } from "./components/OrchestrationPanel";
import {
  bridgeEnqueue,
  detectAgents,
  detectShells,
  dirExists,
  ptyAlive,
  ptyKill,
  ptyWrite,
} from "./lib/ipc";
import { usePaneControl, type ControlDeps } from "./lib/control";
import { useOrchestrator } from "./lib/orchestratorBrief";
import { deliveryDeps, landDelivery } from "./lib/deliveryControl";
import { stopAllWorkers } from "./lib/workerControl";
import { beginDelivery } from "./lib/deliveryRuns";
import { isOrchestrated, useNeedsYou, useNeedsYouNotices } from "./lib/needsYou";
import { orchestratorOf, workersOf } from "./lib/orchestration";
import {
  canCreateOrchestrator,
  pinRefusal,
  useOrchestrationActions,
} from "./lib/orchestrationActions";
import {
  carryResume,
  isAsleep,
  markAwake,
  mustResume,
  resumeOnWake,
  touchWorkspace,
} from "./lib/hibernateStore";
import { useHibernation } from "./lib/useHibernation";
import { sleepRefusal } from "./lib/hibernate";
import { gitWorktreeList } from "./lib/git";
import { modelStat } from "./lib/modelFiles";
import {
  grantModelRoots,
  handBackModels,
  localModelHost,
  remoteModelHost,
  serveModelHandovers,
  serveModelHost,
} from "./lib/modelHost";
import {
  canDetach,
  closeWorkspaceWindow,
  focusWorkspaceWindow,
  isMainWindow,
  localTerminal,
  markHandover,
  openWorkspaceWindow,
  remoteTerminal,
  serveTerminals,
  windowWorkspaceId,
  WINDOW_LABEL,
} from "./lib/windows";
import { emit } from "@tauri-apps/api/event";
import { applyRemoteActivity, relayActivity, type PaneActivity } from "./lib/agentState";
import { getPaneActivity, isWaiting, useBridgedCount } from "./lib/agentState";
import {
  leafIds,
  neighborOf,
  normalizeTree,
  preferredDir,
  type Direction,
} from "./lib/layout";
import { buildPresence, useDiscordPresence } from "./lib/discord";
import { sidebarOrder } from "./lib/folders";
import { findAgentPane } from "./lib/agentPane";
import { mergeCommands, runSavedCommand } from "./lib/savedCommands";
import { useVoice } from "./lib/voice";
import { formatChord } from "./lib/keys";
import { getTerminal, openFinder } from "./lib/terminalRegistry";
import { placeSidePanels } from "./lib/sidePanels";
import {
  closeWorkspaceModels,
  markModelsSeen,
  noteModelView,
  pruneModels,
  setModelPanel,
  useAnyModels,
  useModels,
} from "./lib/modelStore";
import { focusRegion, nextRegion, regionOf, type Region } from "./lib/regions";
import type { CommandId } from "./lib/shortcuts";
import {
  useKeymap,
  useShortcuts,
  type ShortcutHandlers,
} from "./lib/useShortcuts";
import { liveWorkerDeps } from "./lib/workerControl";
import { useT } from "./i18n";
import { useStore } from "./store";
import { applyThemeToDocument, getTheme } from "./themes";
import {
  AGENTS,
  DEFAULT_SETTINGS,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
  type Pane,
  type ShellInfo,
} from "./types";

/** Same bounds as the font size slider in the settings. */
const FONT_MIN = 9;
const FONT_MAX = 24;

/**
 * The width of an element, kept current. Set while the commit that mounted it
 * is still being laid out, so the first paint already knows it.
 */
function useWidth(): [(element: HTMLElement | null) => (() => void) | undefined, number | null] {
  const [width, setWidth] = useState<number | null>(null);
  const ref = useCallback((element: HTMLElement | null) => {
    if (!element) return;
    setWidth(element.getBoundingClientRect().width);
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width];
}

export default function App() {
  const {
    state,
    snapshot,
    hydrated,
    activeWorkspace: storeActiveWorkspace,
    updateWorkspace,
    respawnPane,
    applyPreset,
    tidyLayout,
    addPane,
    closePane,
    movePane,
    updatePane,
    setActiveWorkspace,
    updateSettings,
  } = useStore();
  const [showSettings, setShowSettings] = useState(false);
  /** Category a shortcut asked the settings to open on. */
  const [settingsCategory, setSettingsCategory] = useState("general");
  const [showCreate, setShowCreate] = useState(false);
  const [showMcp, setShowMcp] = useState(false);
  /** The workspace whose MCP servers are being chosen (McpServersPanel). */
  const [mcpServersFor, setMcpServersFor] = useState<string | null>(null);
  /** The workspace whose orchestration settings are open (OrchestrationPanel). */
  const [orchestrationFor, setOrchestrationFor] = useState<string | null>(null);
  /** « Besoin de toi », open under its toolbar button. */
  const [needsYouOpen, setNeedsYouOpen] = useState(false);
  const [view, setView] = useState<View>("terminals");
  const [paletteOpen, setPaletteOpen] = useState(false);
  /**
   * The side panel open as a drawer over the terminals, for want of room
   * beside them. Never saved, and never the saved "open" setting either: a
   * window that narrows starts with its drawers closed, whatever was docked.
   */
  const [drawer, setDrawer] = useState<"inspector" | "dock" | "model" | null>(null);
  /** The row the terminals share with the side panels. */
  const [bodyRef, bodyWidth] = useWidth();

  /**
   * The workspace this window draws. A workspace window draws the one it was
   * opened for; the main window draws the active one — unless that one is
   * shown in a window of its own, in which case the main window says so
   * (`elsewhere`) instead of drawing its terminals a second time.
   */
  const ownWorkspace = windowWorkspaceId
    ? (state.workspaces.find((ws) => ws.id === windowWorkspaceId) ?? null)
    : null;
  const elsewhere = isMainWindow && storeActiveWorkspace?.detached ? storeActiveWorkspace : null;
  const activeWorkspace = windowWorkspaceId
    ? ownWorkspace
    : elsewhere
      ? null
      : storeActiveWorkspace;
  /** One focused pane per workspace: leaving and coming back lands you back. */
  const [focusByWorkspace, setFocusByWorkspace] = useState<Record<string, string>>({});
  /** Workspaces whose focused pane fills the grid. A view, never saved. */
  const [zoomByWorkspace, setZoomByWorkspace] = useState<Record<string, boolean>>({});
  const [pickerPaneId, setPickerPaneId] = useState<string | null>(null);
  /** The reset button is asking "are you sure?" instead of acting. */
  const [confirmReset, setConfirmReset] = useState(false);
  /** So is « Arrêter les workers », in the same menu. */
  const [confirmStopWorkers, setConfirmStopWorkers] = useState(false);
  const [availableAgents, setAvailableAgents] = useState<string[]>([]);
  const [shells, setShells] = useState<ShellInfo[]>([]);
  const [broadcast, setBroadcast] = useState("");
  const [openedIds, setOpenedIds] = useState<string[]>([]);
  /**
   * Transient line at the bottom of the window. A plain string is a problem
   * (dictation, a window that would not open) and comes with the warning
   * sign; a workspace going to sleep or an agent acting on the layout is
   * news, and comes with its own quieter mark.
   */
  const [notice, setNotice] = useState<
    string | { text: string; kind: "asleep" | "agent" } | null
  >(null);
  const broadcastRef = useRef<HTMLInputElement>(null);
  const t = useT();

  const theme = useMemo(
    () => getTheme(activeWorkspace?.themeId ?? state.settings.themeId),
    [activeWorkspace?.themeId, state.settings.themeId],
  );

  useEffect(() => {
    applyThemeToDocument(theme);
  }, [theme]);

  // Every interface font size in styles.css is multiplied by this; the
  // terminals are sized by `fontSize` alone.
  const uiScale =
    Math.min(UI_SCALE_MAX, Math.max(UI_SCALE_MIN, state.settings.uiScale || 100)) / 100;
  useEffect(() => {
    document.documentElement.style.setProperty("--ui-scale", String(uiScale));
  }, [uiScale]);

  // The interface's monospace — session ids, paths, counts — is the
  // terminals' own font, so the chrome and the panes read as one thing.
  useEffect(() => {
    document.documentElement.style.setProperty("--mono", state.settings.fontFamily);
  }, [state.settings.fontFamily]);

  useEffect(() => {
    detectAgents()
      .then(setAvailableAgents)
      .catch(() => setAvailableAgents([]));
    detectShells()
      .then(setShells)
      .catch(() => setShells([]));
  }, []);

  const panes = useMemo(() => activeWorkspace?.panes ?? [], [activeWorkspace?.panes]);

  /** The arrangement as it stands, which is what directional moves read. */
  const tree = useMemo(
    () => (activeWorkspace ? normalizeTree(panes, activeWorkspace.tree) : null),
    [panes, activeWorkspace],
  );

  /** Pane ids in reading order: Ctrl+N follows the arrangement on screen. */
  const order = useMemo(() => (tree ? leafIds(tree) : []), [tree]);

  const focusedPaneId = activeWorkspace
    ? (focusByWorkspace[activeWorkspace.id] ?? null)
    : null;

  const focusPane = useCallback((workspaceId: string, paneId: string) => {
    setFocusByWorkspace((current) => ({ ...current, [workspaceId]: paneId }));
  }, []);

  const setZoom = useCallback((workspaceId: string, on: boolean) => {
    setZoomByWorkspace((current) =>
      Boolean(current[workspaceId]) === on ? current : { ...current, [workspaceId]: on },
    );
  }, []);

  /**
   * Where a transcript lands.
   *
   * The broadcast box is checked first and by identity, not by tag name: it is
   * a controlled input, so writing to the DOM node would put text on screen
   * that React does not know about and drops on the next render. Everything
   * else goes to the focused pane, unsubmitted — an agent acting on a sentence
   * a microphone guessed at is worth one deliberate Enter.
   */
  const insertTranscript = useCallback(
    (text: string) => {
      if (document.activeElement === broadcastRef.current) {
        setBroadcast((current) => (current ? `${current} ${text}` : text));
        return;
      }
      if (!focusedPaneId) {
        setNotice(t("voice.noPane"));
        return;
      }
      const payload = state.settings.voiceSubmit ? `${text}\r` : text;
      ptyWrite(focusedPaneId, payload).catch(() => undefined);
    },
    [focusedPaneId, state.settings.voiceSubmit, t],
  );

  const voice = useVoice({
    settings: state.settings,
    onText: insertTranscript,
    onError: (message) => setNotice(t("voice.failed", { error: message })),
  });

  // Notices are informational and never actionable, so they clear themselves
  // rather than asking for a dismiss button nobody wants to find.
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 6000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  // Discord shows the open workspace and what is running in it. Rebuilt here
  // and published only when it differs — see lib/discord.
  useDiscordPresence(
    useMemo(
      () =>
        buildPresence({
          settings: state.settings,
          workspace: activeWorkspace,
          focusedPaneId,
          t,
        }),
      [state.settings, activeWorkspace, focusedPaneId, t],
    ),
    isMainWindow,
  );

  /**
   * Workspaces stay mounted once visited. Switching away only hides them, so
   * their agents keep running — unmounting a pane kills its PTY. The list is
   * built lazily: nothing spawns for a workspace the user never opened.
   */
  useEffect(() => {
    const id = state.activeWorkspaceId;
    if (id) setOpenedIds((current) => (current.includes(id) ? current : [...current, id]));
  }, [state.activeWorkspaceId]);

  // A workspace window mounts its own workspace and nothing else; the main
  // window mounts every workspace visited, except the ones in a window of
  // their own — two terminals on one PTY would both write to it.
  const openWorkspaces = useMemo(
    () =>
      windowWorkspaceId
        ? state.workspaces.filter((ws) => ws.id === windowWorkspaceId)
        : state.workspaces.filter((ws) => openedIds.includes(ws.id) && !ws.detached),
    [state.workspaces, openedIds],
  );

  // Workspaces left alone go to sleep, their agents stopped to free memory
  // and resumed when they open again (lib/useHibernation); the navigator's
  // menu and the palette put one to sleep at once. The main window's job
  // alone: it is the one that mounts them.
  const forcedSleep = useHibernation({
    enabled: isMainWindow && hydrated && state.settings.hibernateIdle,
    // At least a minute, whatever a hand-edited state.json says.
    delayMs: Math.max(1, state.settings.hibernateAfterMin || 0) * 60_000,
    activeId: state.activeWorkspaceId,
    snapshot,
    openedIds,
    unmount: (id) => setOpenedIds((current) => current.filter((entry) => entry !== id)),
    show: setActiveWorkspace,
    announce: (workspaces, agents) =>
      setNotice({
        kind: "asleep",
        text:
          workspaces.length > 1
            ? t("hibernate.noticeMany", { count: workspaces.length, n: agents })
            : agents
              ? t("hibernate.notice", { name: workspaces[0].name, n: agents })
              : t("hibernate.noticeNoAgent", { name: workspaces[0].name }),
      }),
    // What changed between the menu and the click, or what the palette,
    // which has no greyed-out entries, could not show beforehand.
    refuse: (workspace, verdict) =>
      setNotice(
        t("sleep.refused", {
          name: workspace.name,
          reason: sleepRefusal(t, verdict, workspace.panes, isWaiting),
        }),
      ),
  });

  // Keep focus on a pane that still exists after a layout change.
  useEffect(() => {
    if (!activeWorkspace) return;
    if (panes.length && !panes.some((p) => p.id === focusedPaneId)) {
      focusPane(activeWorkspace.id, panes[0].id);
    }
  }, [activeWorkspace, panes, focusedPaneId, focusPane]);

  /**
   * Landing on the pane whose notification was clicked. The window is already
   * in front by the time this runs — the Rust side raises it — so all that is
   * left is showing what the toast was about.
   *
   * Rebuilt on every render behind a ref, because the listener below is
   * installed once and would otherwise keep answering with the workspaces the
   * app had on mount.
   */
  const activateRef = useRef<(workspaceId: string, paneId: string) => void>(() => {});
  activateRef.current = (workspaceId, paneId) => {
    // The snapshot, not the render's state: an agent that just created the
    // pane asks for it before React has drawn it.
    const workspace = snapshot().workspaces.find((ws) => ws.id === workspaceId);
    // Deleted since the toast went out: the raised window is all it gets.
    if (!workspace) return;
    // A workspace window only ever shows its own workspace, and never moves
    // the main window's selection.
    if (windowWorkspaceId) {
      if (workspaceId !== windowWorkspaceId) return;
      setView("terminals");
      if (workspace.panes.some((p) => p.id === paneId)) focusPane(workspaceId, paneId);
      return;
    }
    // Shown in a window of its own: that window is the one to bring forward.
    if (workspace.detached) {
      focusWorkspaceWindow(workspaceId).catch(() => undefined);
      emit("window:focus-pane", { workspaceId, paneId }).catch(() => undefined);
      return;
    }
    // Anything drawn over the terminals goes: the settings page is a
    // fullscreen layer, and a click that promised a pane must not land on it.
    setShowSettings(false);
    setShowMcp(false);
    setMcpServersFor(null);
    setOrchestrationFor(null);
    setNeedsYouOpen(false);
    setShowCreate(false);
    setPickerPaneId(null);
    setActiveWorkspace(workspaceId);
    setView("terminals");
    // A pane that is gone leaves the focus alone — the effect above moves it
    // onto a pane that still exists.
    if (workspace.panes.some((p) => p.id === paneId)) focusPane(workspaceId, paneId);
  };

  useEffect(() => {
    let cancelled = false;
    let off: (() => void) | null = null;
    listen<{ workspaceId: string; paneId: string }>("notification-activated", (event) =>
      activateRef.current(event.payload.workspaceId, event.payload.paneId),
    )
      .then((stop) => {
        // Unmounted while the listener was being registered; keeping it would
        // leak it for good.
        if (cancelled) stop();
        else off = stop;
      })
      // No event bus outside the Tauri shell, and nothing to say about it.
      .catch(() => undefined);
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);

  /** The board's way to an assignee's pane: same landing as a notification. */
  const findAgent = useCallback(
    (assignee: string) => {
      const target = findAgentPane(assignee, state.workspaces, state.activeWorkspaceId);
      return target ? () => activateRef.current(target.workspaceId, target.paneId) : null;
    },
    [state.workspaces, state.activeWorkspaceId],
  );

  /**
   * The 3D preview of the workspace on screen: its models, the one in front,
   * its panel. Every workspace has its own; the window's one viewer holds the
   * models of them all.
   */
  const modelsOf = activeWorkspace?.id ?? null;
  const models = useModels(modelsOf);
  const modelsOpen = models.models.length > 0;
  /** Any workspace's, in this window: the viewer stays up for their snapshots. */
  const windowModels = useAnyModels();
  /** Over the whole work area: only ever in the terminals view. */
  const modelExpanded = modelsOpen && models.expanded && view === "terminals";

  /**
   * Beside the terminals or over them (lib/sidePanels). Until the row has been
   * measured — within the commit that mounts it — everything counts as fitting.
   */
  const sides = placeSidePanels({
    room: bodyWidth ?? Number.POSITIVE_INFINITY,
    scale: uiScale,
    inspector: { open: state.settings.inspectorOpen, width: state.settings.inspectorWidth },
    // The board panel only ever sits beside the terminals.
    dock:
      view === "terminals"
        ? { open: state.settings.boardDockOpen, width: state.settings.boardDockWidth }
        : null,
    // Beside the terminals too, while a model is open; expanded, it covers
    // the work area instead of taking room in it.
    model:
      view === "terminals" && modelsOpen && !modelExpanded
        ? { open: models.panelOpen, width: state.settings.modelPanelWidth }
        : null,
  });
  const inspectorDrawer = !sides.inspector.fits;
  const dockDrawer = !sides.dock.fits;
  const modelDrawer = !sides.model.fits;

  // A drawer the window has grown wide enough to dock, or a board drawer whose
  // terminals left the screen, is over: narrowing again must not bring it back.
  useEffect(() => {
    if (drawer === "inspector" && !inspectorDrawer) setDrawer(null);
    if (drawer === "dock" && (!dockDrawer || view !== "terminals")) setDrawer(null);
    if (drawer === "model" && (!modelDrawer || view !== "terminals" || !modelsOpen)) setDrawer(null);
  }, [drawer, inspectorDrawer, dockDrawer, modelDrawer, modelsOpen, view]);

  const dockShown =
    view === "terminals" && (dockDrawer ? drawer === "dock" : state.settings.boardDockOpen);
  /** Without room, the button opens the drawer and leaves the setting alone. */
  const toggleDock = () => {
    if (view !== "terminals") {
      setView("terminals");
      updateSettings({ boardDockOpen: true });
      if (dockDrawer) setDrawer("dock");
      return;
    }
    if (dockDrawer) setDrawer((open) => (open === "dock" ? null : "dock"));
    else updateSettings({ boardDockOpen: !state.settings.boardDockOpen });
  };
  const closeDock = () =>
    dockDrawer ? setDrawer(null) : updateSettings({ boardDockOpen: false });

  /**
   * The 3D preview: docked when there is room and it is open, a drawer the
   * user opens otherwise — an agent showing a model never throws a drawer
   * over the terminals someone is typing in; the toolbar button says there
   * is something to see instead.
   */
  const modelShown =
    view === "terminals" &&
    modelsOpen &&
    (modelExpanded || (modelDrawer ? drawer === "model" : models.panelOpen));
  /** The workspace on screen's panel, which is all the user can reach. */
  const setModelsPanel = (patch: { open?: boolean; expanded?: boolean }) => {
    if (modelsOf) setModelPanel(modelsOf, patch);
  };
  const toggleModels = () => {
    if (view !== "terminals") {
      setView("terminals");
      setModelsPanel({ open: true });
      if (modelDrawer) setDrawer("model");
      return;
    }
    if (modelExpanded) return setModelsPanel({ open: false });
    if (modelDrawer) setDrawer((open) => (open === "model" ? null : "model"));
    else setModelsPanel({ open: !models.panelOpen });
  };
  const closeModelPanel = () => {
    if (modelExpanded || !modelDrawer) setModelsPanel({ open: false });
    else setDrawer(null);
  };
  const toggleModelExpand = () => setModelsPanel({ open: true, expanded: !models.expanded });

  /**
   * Something an agent opened or changed is waiting behind the toolbar button
   * — in this workspace: what it showed in another waits in that one. Seen as
   * soon as the panel is on screen, before the paint that would light it.
   */
  const modelsUnseen = models.unseen && !modelShown;
  useLayoutEffect(() => {
    if (modelsOf && modelShown && models.unseen) markModelsSeen(modelsOf);
  }, [modelsOf, modelShown, models.unseen]);
  // A drawer opened on one workspace's preview does not come over the next
  // workspace's terminals: that one's preview is its own.
  useEffect(() => {
    setDrawer((open) => (open === "model" ? null : open));
  }, [modelsOf]);
  // For the agents' answers: whether the model they showed is in sight.
  useEffect(() => {
    noteModelView(modelsOf, modelShown);
  }, [modelsOf, modelShown]);
  // A workspace deleted takes its models with it.
  const workspaceIds = state.workspaces.map((ws) => ws.id).join(" ");
  useEffect(() => {
    pruneModels(workspaceIds ? workspaceIds.split(" ") : []);
  }, [workspaceIds]);

  const toggleNavigator = () =>
    updateSettings({ navigatorOpen: !state.settings.navigatorOpen });
  const toggleInspector = () => {
    if (inspectorDrawer) setDrawer((open) => (open === "inspector" ? null : "inspector"));
    else updateSettings({ inspectorOpen: !state.settings.inspectorOpen });
  };
  const closeInspector = () =>
    inspectorDrawer ? setDrawer(null) : updateSettings({ inspectorOpen: false });
  const inspectorShown = inspectorDrawer ? drawer === "inspector" : state.settings.inspectorOpen;
  /** The main window's alone, and only when there is room asked for it. */
  const navigatorShown = isMainWindow && state.settings.navigatorOpen;

  // ---------------------------------------------------------------------------
  // The keyboard's way around the window (lib/regions): a terminal keeps Tab
  // for the shell, so F6 and the "focus" commands are how it gets out of a
  // pane and into the panels around it.
  // ---------------------------------------------------------------------------

  /** A region to put the keyboard in once the render that shows it is done. */
  const pendingRegion = useRef<Region | null>(null);
  useEffect(() => {
    const region = pendingRegion.current;
    if (!region) return;
    pendingRegion.current = null;
    focusRegion(region);
  });

  /** In the terminals view, the panes' region is the focused pane's terminal. */
  const enterRegion = (region: Region): boolean => {
    const term = view === "terminals" && region === "main" ? getTerminal(focusedPaneId) : null;
    if (!term) return focusRegion(region);
    term.focus();
    return true;
  };

  const cycleRegion = (step: 1 | -1) => {
    const at = document.activeElement;
    // A dialog keeps the keyboard until it is closed.
    if (at?.closest('[aria-modal="true"], .modal-backdrop')) return;
    const shown: Region[] = [];
    if (navigatorShown) shown.push("navigator");
    if (activeWorkspace) shown.push("toolbar", "main", "status");
    if (activeWorkspace && modelShown) shown.push("model");
    if (activeWorkspace && dockShown) shown.push("dock");
    if (activeWorkspace && inspectorShown) shown.push("inspector");
    // A region with nothing to focus — the status bar over the board, which
    // has no field — is stepped over.
    let current = regionOf(at);
    for (let tries = 0; tries < shown.length; tries++) {
      const next = nextRegion(shown, current, step);
      if (!next || enterRegion(next)) return;
      current = next;
    }
  };

  /** Shown first when it is hidden: asking for its keys is asking to see it. */
  const focusNavigator = () => {
    if (state.settings.navigatorOpen) return void focusRegion("navigator");
    pendingRegion.current = "navigator";
    updateSettings({ navigatorOpen: true });
  };

  const focusInspector = () => {
    // A drawer takes the keyboard by itself as it opens.
    if (inspectorDrawer && drawer !== "inspector") return setDrawer("inspector");
    if (inspectorShown) return void focusRegion("inspector");
    pendingRegion.current = "inspector";
    updateSettings({ inspectorOpen: true });
  };

  /** `dir` is only passed by the shortcuts that name a side; otherwise the
      longer side of the pane decides. */
  const splitPane = useCallback(
    (workspaceId: string, paneId: string | null, dir?: "row" | "col") => {
      const near = paneId ?? focusByWorkspace[workspaceId] ?? null;
      const created = addPane(workspaceId, { near, dir: dir ?? preferredDir(near) });
      if (created) focusPane(workspaceId, created);
    },
    [addPane, focusByWorkspace, focusPane],
  );

  const replacePane = useCallback(
    (workspaceId: string, paneId: string, patch: Partial<Pane>) => {
      // A fresh id remounts the terminal, which is how a pane restarts.
      const nextId = respawnPane(workspaceId, paneId, patch);
      if (nextId && focusByWorkspace[workspaceId] === paneId) {
        focusPane(workspaceId, nextId);
      }
    },
    [respawnPane, focusByWorkspace, focusPane],
  );

  // Agents driving the panes over MCP (lib/control). Everything is read when a
  // request lands, hence the refs.
  const focusRef = useRef(focusByWorkspace);
  focusRef.current = focusByWorkspace;
  const openedRef = useRef(openedIds);
  openedRef.current = openedIds;
  // Defined further down, with the rest of the window handling.
  const detachRef = useRef<(workspaceId: string) => Promise<void>>(async () => undefined);
  const reattachRef = useRef<(workspaceId: string) => void>(() => undefined);
  const controlDeps: ControlDeps | null =
    hydrated
      ? {
          snapshot,
          enabled: () => snapshot().settings.agentPaneControl !== false,
          addPane,
          closePane,
          respawnPane: (workspaceId, paneId, patch) => {
            const nextId = respawnPane(workspaceId, paneId, patch);
            if (nextId && focusRef.current[workspaceId] === paneId) focusPane(workspaceId, nextId);
            // Restarted while its workspace slept: still owed its session,
            // unless the restart asked for a fresh one.
            if (nextId && patch.sessionId !== null) carryResume(paneId, nextId);
            return nextId;
          },
          openWorkspace: (id) =>
            setOpenedIds((current) => (current.includes(id) ? current : [...current, id])),
          isOpen: (id) =>
            openedRef.current.includes(id) ||
            Boolean(snapshot().workspaces.find((ws) => ws.id === id)?.detached),
          asleep: isAsleep,
          touch: touchWorkspace,
          activate: (workspaceId, paneId) => activateRef.current(workspaceId, paneId),
          // A detached workspace's terminals are drawn by its own window.
          terminal: async (paneId) => {
            const term = getTerminal(paneId);
            return term ? localTerminal(term) : remoteTerminal(paneId);
          },
          write: ptyWrite,
          alive: ptyAlive,
          activity: getPaneActivity,
          enqueue: bridgeEnqueue,
          dirExists,
          canDetach: () => canDetach,
          detach: (id) => detachRef.current(id),
          reattach: (id) => reattachRef.current(id),
          // Said on screen: panes appearing and vanishing on their own would
          // otherwise look like a bug.
          announce: (event) =>
            setNotice({
              kind: "agent",
              text: t(`control.${event.kind}`, { ...event, by: event.by ?? t("control.someone") }),
            }),
          sleep: (ms) => new Promise((resolve) => window.setTimeout(resolve, ms)),
          modelAllowRoots: (workspaceId, roots) =>
            grantModelRoots(workspaceId, roots, snapshot().workspaces.map((ws) => ws.id)),
          modelStat,
          worktrees: gitWorktreeList,
          // A detached workspace's preview is drawn by its own window.
          modelHost: async (workspaceId, detached) =>
            detached ? remoteModelHost(workspaceId) : localModelHost(workspaceId),
          // orchestration: workers deps
          workers: liveWorkerDeps(updatePane),
          // orchestration: delivery deps
          delivery: deliveryDeps(() => shells),
        }
      : null;
  usePaneControl(controlDeps);

  // ---------------------------------------------------------------------------
  // Orchestration (lib/orchestration): pinning a workspace's orchestrator, its
  // settings, stopping its workers, and « Besoin de toi » — what waits for
  // the person across the workspaces. None of it shows until a workspace has
  // an orchestrator, but the entries that start one.
  // ---------------------------------------------------------------------------

  const orchestration = useOrchestrationActions();

  /** Pins or unpins a pane as its workspace's orchestrator, and says so. */
  const pinOrchestrator = (workspaceId: string, paneId: string, pinned: boolean) => {
    const ws = snapshot().workspaces.find((entry) => entry.id === workspaceId);
    const pane = ws?.panes.find((entry) => entry.id === paneId);
    if (!ws || !pane) return;
    if (pinned && !orchestration.pin(workspaceId, paneId)) return;
    if (!pinned) orchestration.unpin(workspaceId, paneId);
    setNotice({
      kind: "agent",
      text: t(pinned ? "orch.pinned" : "orch.unpinned", { name: pane.name, workspace: ws.name }),
    });
  };

  /** A new Claude Code pane, pinned from the start, brought forward. */
  const createOrchestrator = (workspaceId: string) => {
    const ws = snapshot().workspaces.find((entry) => entry.id === workspaceId);
    if (!ws || !canCreateOrchestrator(ws)) return;
    const id = orchestration.create(workspaceId, focusByWorkspace[workspaceId] ?? null);
    if (!id) return setNotice(t("orch.noRoom", { workspace: ws.name }));
    const pane = snapshot().workspaces.find((entry) => entry.id === workspaceId)?.panes.find((entry) => entry.id === id);
    activateRef.current(workspaceId, id);
    setNotice({
      kind: "agent",
      text: t("orch.pinned", { name: pane?.name ?? "", workspace: ws.name }),
    });
  };

  /** « Arrêter les workers », once confirmed. */
  const stopWorkers = (workspaceId: string) => {
    setConfirmStopWorkers(false);
    const ws = snapshot().workspaces.find((entry) => entry.id === workspaceId);
    if (!ws || !controlDeps) return;
    stopAllWorkers(ws, controlDeps)
      .then((n) =>
        setNotice({
          kind: "agent",
          text: n
            ? t("orch.stopped", { n, workspace: ws.name })
            : t("orch.stoppedNone", { workspace: ws.name }),
        }),
      )
      .catch((err) => setNotice(t("orch.stopFailed", { error: String(err) })));
  };

  /** « Fusionner »: the delivery runs for as long as it takes, and says so. */
  const landItem = async (item: LandItem) => {
    const ws = snapshot().workspaces.find((entry) => entry.id === item.workspaceId);
    if (!ws || !controlDeps) return;
    const end = beginDelivery(ws.id, item.task.id);
    try {
      await landDelivery(ws, item.task.id, controlDeps);
      setNotice({ kind: "agent", text: t("needsYou.landed", { title: item.task.title }) });
    } catch (err) {
      // Said here too, not only on its row: one its worker moved past
      // leaves the list as it is refused, and its row with it.
      setNotice(t("needsYou.landFailed", { error: err instanceof Error ? err.message : String(err) }));
      throw err;
    } finally {
      end();
    }
  };

  /** Lands on a pane from « Besoin de toi », keyboard included. */
  const goToPane = (workspaceId: string, paneId: string) => {
    activateRef.current(workspaceId, paneId);
    requestAnimationFrame(() => getTerminal(paneId)?.focus());
  };

  const openBoardOf = (workspaceId: string) => {
    if (!windowWorkspaceId) setActiveWorkspace(workspaceId);
    setView("board");
  };

  // Every workspace the window knows, in the navigator's order: a workspace
  // window only knows its own.
  const needsYouScope = useMemo(
    () => (windowWorkspaceId ? openWorkspaces : sidebarOrder(state.workspaces, state.folders)),
    [openWorkspaces, state.workspaces, state.folders],
  );
  const needsYou = useNeedsYou(needsYouScope);
  useNeedsYouNotices({
    enabled: isMainWindow && hydrated,
    items: needsYou.items,
    boards: needsYou.boards,
    workspaces: state.workspaces,
    settings: state.settings,
    t,
  });
  // Nothing left in it, it has nothing left to show: its button is gone too.
  useEffect(() => {
    if (!needsYou.items.length) setNeedsYouOpen(false);
  }, [needsYou.items.length]);

  // Orchestrators: the events that wake them, the briefing their sessions ask for.
  useOrchestrator(hydrated ? snapshot : null);

  // ---------------------------------------------------------------------------
  // Workspaces in windows of their own (lib/windows). Moving one never
  // restarts its panes: the window it leaves marks them as handed over, so
  // unmounting does not kill them, and the window it lands in takes the
  // running PTYs over.
  // ---------------------------------------------------------------------------

  /** Main window: sends a workspace to a window of its own. */
  const detach = useCallback(
    async (workspaceId: string) => {
      const ws = snapshot().workspaces.find((entry) => entry.id === workspaceId);
      if (!ws || ws.detached) return;
      markHandover(ws.panes.map((pane) => pane.id));
      // Asleep, it wakes in its new window: the sessions its panes are owed
      // go with it, since this window will not be the one starting them.
      const resume = isAsleep(ws.id)
        ? ws.panes.filter((pane) => mustResume(pane.id, true)).map((pane) => pane.id)
        : [];
      markAwake(ws.id);
      updateWorkspace(ws.id, { detached: true });
      // The main window moves on to a workspace it can still show.
      const next = sidebarOrder(snapshot().workspaces, snapshot().folders).find(
        (entry) => entry.id !== ws.id && !entry.detached,
      );
      if (next) setActiveWorkspace(next.id);
      try {
        await openWorkspaceWindow(ws.id, ws.name, ws.windowBounds, resume);
      } catch (err) {
        // No window: it comes straight back, its panes still running — or,
        // asleep, still owed their sessions.
        resumeOnWake(resume);
        updateWorkspace(ws.id, { detached: false });
        setActiveWorkspace(ws.id);
        setNotice(t("window.openFailed", { error: String(err) }));
      }
    },
    [snapshot, updateWorkspace, setActiveWorkspace, t],
  );

  /** Either window: brings a workspace back into the main window. */
  const reattach = useCallback(
    (workspaceId: string) => {
      const ws = snapshot().workspaces.find((entry) => entry.id === workspaceId);
      if (ws) {
        markHandover(ws.panes.map((pane) => pane.id));
        updateWorkspace(ws.id, { detached: false });
        setActiveWorkspace(ws.id);
      }
      // From its own window, give the change a moment to reach the main window
      // before this one goes.
      window.setTimeout(() => closeWorkspaceWindow(workspaceId).catch(() => undefined), 150);
    },
    [snapshot, updateWorkspace, setActiveWorkspace],
  );
  detachRef.current = detach;
  reattachRef.current = reattach;

  // Main window, once loaded: the workspaces that were in windows of their own
  // when the app last closed get those windows back. The browser demo has only
  // the one window, so it takes them in.
  const reopened = useRef(false);
  useEffect(() => {
    if (!isMainWindow || !hydrated || reopened.current) return;
    reopened.current = true;
    for (const ws of snapshot().workspaces.filter((entry) => entry.detached)) {
      if (!canDetach) {
        updateWorkspace(ws.id, { detached: false });
        continue;
      }
      openWorkspaceWindow(ws.id, ws.name, ws.windowBounds).catch(() =>
        updateWorkspace(ws.id, { detached: false }),
      );
    }
  }, [hydrated, snapshot, updateWorkspace]);

  // Main window: picking a detached workspace in the sidebar brings its
  // window forward.
  useEffect(() => {
    if (elsewhere) focusWorkspaceWindow(elsewhere.id).catch(() => undefined);
  }, [elsewhere?.id]);

  // Activity is worked out by whichever window draws the pane; the main
  // window's sidebar counts detached workspaces too.
  useEffect(() => {
    if (isMainWindow) {
      const stop = listen<{ from: string; paneId: string; value: PaneActivity | null }>(
        "agent:activity",
        (event) => {
          if (event.payload.from !== WINDOW_LABEL) {
            applyRemoteActivity(event.payload.paneId, event.payload.value);
          }
        },
      );
      // A workspace's models follow it into its window and back.
      const stopModels = serveModelHandovers();
      return () => {
        stop.then((off) => off()).catch(() => undefined);
        stopModels();
      };
    }
    relayActivity((paneId, value) => {
      emit("agent:activity", { from: WINDOW_LABEL, paneId, value }).catch(() => undefined);
    });
    // The main window carries out the agents' requests; this one lends it the
    // terminals it draws, and the 3D preview of its workspace.
    const stopServing = serveTerminals(getTerminal);
    const stopModels = windowWorkspaceId ? serveModelHost(windowWorkspaceId) : null;
    return () => {
      relayActivity(null);
      stopServing();
      stopModels?.();
    };
  }, []);

  // Workspace window: its workspace going back into the main window takes its
  // 3D preview along, before this window closes.
  const ownDetached = ownWorkspace ? Boolean(ownWorkspace.detached) : null;
  useEffect(() => {
    if (windowWorkspaceId && ownDetached === false) handBackModels(windowWorkspaceId);
  }, [ownDetached]);

  // Workspace window: everything that is about its own window.
  const lastPanes = useRef<string[]>([]);
  if (ownWorkspace) lastPanes.current = ownWorkspace.panes.map((pane) => pane.id);
  useEffect(() => {
    if (!windowWorkspaceId) return;
    const id = windowWorkspaceId;
    const win = getCurrentWindow();
    const stops: Promise<() => void>[] = [];

    // Its close button hands the workspace back rather than ending anything.
    stops.push(listen("window:close-requested", () => reattach(id)));
    // The main window pointing at one of its panes (a notification, an agent).
    stops.push(
      listen<{ workspaceId: string; paneId: string }>("window:focus-pane", (event) => {
        if (event.payload.workspaceId === id) activateRef.current(id, event.payload.paneId);
      }),
    );

    // Where it stands, so it reopens there.
    let timer = 0;
    const remember = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(async () => {
        try {
          const [position, size, scale] = await Promise.all([
            win.outerPosition(),
            win.innerSize(),
            win.scaleFactor(),
          ]);
          const at = position.toLogical(scale);
          const extent = size.toLogical(scale);
          updateWorkspace(id, {
            windowBounds: { x: at.x, y: at.y, width: extent.width, height: extent.height },
          });
        } catch {
          /* the window is going away */
        }
      }, 700);
    };
    stops.push(win.onMoved(remember));
    stops.push(win.onResized(remember));

    return () => {
      window.clearTimeout(timer);
      stops.forEach((stop) => stop.then((off) => off()).catch(() => undefined));
    };
  }, [reattach, updateWorkspace]);

  // Workspace window whose workspace was deleted from the main window: its
  // panes die with it — nobody else draws them — and the window closes.
  useEffect(() => {
    if (!windowWorkspaceId || !hydrated || ownWorkspace) return;
    for (const paneId of lastPanes.current) ptyKill(paneId).catch(() => undefined);
    closeWorkspaceWindow(windowWorkspaceId).catch(() => undefined);
  }, [hydrated, ownWorkspace]);

  // Workspace window: its title follows a rename.
  useEffect(() => {
    if (!ownWorkspace) return;
    getCurrentWindow()
      .setTitle(`${ownWorkspace.name} — Hangar.AI`)
      .catch(() => undefined);
  }, [ownWorkspace?.name]);

  /** Panes running an agent — the ones a reset has a conversation to drop. */
  const agentPanes = useMemo(() => panes.filter((p) => p.agent !== "shell"), [panes]);

  /**
   * Every agent of the workspace on a brand new conversation. The same respawn
   * as a restart, minus the session id, so the launch has nothing to resume —
   * the transcripts stay on disk and the session picker can still reopen them.
   */
  const resetAgents = () => {
    if (!activeWorkspace) return;
    for (const pane of agentPanes) {
      replacePane(activeWorkspace.id, pane.id, { sessionId: null });
    }
    setConfirmReset(false);
  };

  /** The confirmation had the keyboard; hand it back to the terminal. */
  const cancelReset = () => {
    setConfirmReset(false);
    if (focusedPaneId) getTerminal(focusedPaneId)?.focus();
  };

  // An armed confirmation is about the workspace it was armed on.
  useEffect(() => {
    setConfirmReset(false);
    setConfirmStopWorkers(false);
  }, [activeWorkspace?.id, view]);

  const cancelStopWorkers = () => {
    setConfirmStopWorkers(false);
    if (focusedPaneId) getTerminal(focusedPaneId)?.focus();
  };

  /** Opening always names a category, so a shortcut lands where it promised. */
  const openSettings = useCallback((category = "general") => {
    setSettingsCategory(category);
    setShowSettings(true);
  }, []);

  const keymap = useKeymap();

  /**
   * What every shortcut does, for the app as it stands right now.
   *
   * A command left out of the table is not a shortcut at all at this moment —
   * the dispatcher lets its keystroke through to the terminal instead of
   * swallowing it for nothing. That is why the pane commands are only added
   * when there is a pane to act on: Ctrl+Shift+X with no workspace open should
   * do nothing, not be eaten.
   *
   * Rebuilt on every render rather than memoised: the dispatcher reads it
   * through a ref when a key is pressed, so it is never a dependency of
   * anything and a stale entry would be a bug rather than a saved allocation.
   */
  function buildHandlers(): ShortcutHandlers {
    const zoom = (size: number) =>
      updateSettings({ fontSize: Math.min(FONT_MAX, Math.max(FONT_MIN, size)) });

    const map: ShortcutHandlers = {
      "view.settings": () => (showSettings ? setShowSettings(false) : openSettings()),
      "view.shortcuts": () => openSettings("shortcuts"),
      "workspace.new": () => setShowCreate(true),
      // The same key closes it again, as in any editor.
      "view.palette": () => setPaletteOpen((open) => !open),
      "view.inspector": toggleInspector,
      "view.nextRegion": () => cycleRegion(1),
      "view.prevRegion": () => cycleRegion(-1),
      "app.zoomIn": () => zoom(state.settings.fontSize + 1),
      "app.zoomOut": () => zoom(state.settings.fontSize - 1),
      "app.zoomReset": () => zoom(DEFAULT_SETTINGS.fontSize),
      "app.fullscreen": () => {
        const win = getCurrentWindow();
        win
          .isFullscreen()
          .then((on) => win.setFullscreen(!on))
          .catch(() => undefined);
      },
    };

    // Left out entirely while dictation is off, so the shortcut falls through
    // to the terminal instead of being swallowed by a feature nobody enabled.
    if (state.settings.voiceEnabled) map["voice.dictate"] = voice.press;

    // A workspace window shows one workspace and no settings: those keys are
    // the main window's.
    if (isMainWindow) {
      map["view.navigator"] = toggleNavigator;
      map["view.focusNavigator"] = focusNavigator;
    }
    if (windowWorkspaceId) {
      delete map["view.settings"];
      delete map["view.shortcuts"];
      delete map["workspace.new"];
    }

    const workspaces = windowWorkspaceId ? [] : sidebarOrder(state.workspaces, state.folders);
    for (let i = 0; i < Math.min(9, workspaces.length); i++) {
      const { id } = workspaces[i];
      map[`workspace.go${i + 1}` as CommandId] = () => setActiveWorkspace(id);
    }
    if (workspaces.length > 1 && activeWorkspace) {
      const at = Math.max(0, workspaces.findIndex((ws) => ws.id === activeWorkspace.id));
      const step = (delta: number) =>
        setActiveWorkspace(
          workspaces[(at + delta + workspaces.length) % workspaces.length].id,
        );
      map["workspace.next"] = () => step(1);
      map["workspace.prev"] = () => step(-1);
      // The navigator's « Mettre en veille », for the workspace on screen:
      // the main window moves on to another first, so it needs one to go to.
      // Whatever else keeps it awake is said when it runs.
      const shown = activeWorkspace.id;
      map["workspace.sleep"] = () => void forcedSleep.run(shown);
    }

    if (!activeWorkspace) return map;
    const wsId = activeWorkspace.id;

    map["view.focusInspector"] = focusInspector;
    map["view.terminals"] = () => setView("terminals");
    map["view.board"] = () => setView("board");
    map["view.boardDock"] = toggleDock;
    // Only while there is a model to show or close: otherwise the keys fall
    // through, and the palette does not offer them.
    if (modelsOpen) {
      map["view.modelPreview"] = toggleModels;
      map["view.closeModels"] = () => closeWorkspaceModels(wsId);
    }
    map["view.memory"] = () => setView("memory");
    map["view.stats"] = () => setView("stats");
    map["view.mcp"] = () => setShowMcp(true);
    map["workspace.mcpServers"] = () => setMcpServersFor(wsId);
    // Orchestration: its settings are a menu entry anyone may find; the rest
    // is only there where it applies.
    map["workspace.orchestration"] = () => setOrchestrationFor(wsId);
    if (canCreateOrchestrator(activeWorkspace)) {
      map["workspace.newOrchestrator"] = () => createOrchestrator(wsId);
    }
    if (workersOf(activeWorkspace).length) {
      // Only arms the confirmation, like the reset.
      map["workspace.stopWorkers"] = () => {
        setConfirmReset(false);
        setConfirmStopWorkers(true);
      };
    }
    if (needsYou.items.length) map["view.needsYou"] = () => setNeedsYouOpen(true);

    // Everything below acts on panes, which are only on screen — and are only
    // what a keystroke can be about — in the terminals view.
    if (view !== "terminals" || !tree) return map;

    map["view.broadcast"] = () => broadcastRef.current?.focus();
    map["pane.split"] = () => splitPane(wsId, null);
    map["pane.splitRight"] = () => splitPane(wsId, null, "row");
    map["pane.splitDown"] = () => splitPane(wsId, null, "col");
    map["pane.restartAll"] = () => panes.forEach((p) => replacePane(wsId, p.id, {}));
    // Only arms the confirmation: the key never throws conversations away by
    // itself.
    if (agentPanes.length) map["pane.resetAll"] = () => setConfirmReset(true);
    if (panes.length > 1) map["pane.tidy"] = () => tidyLayout(wsId);

    for (let i = 0; i < Math.min(9, order.length); i++) {
      const id = order[i];
      map[`pane.focus${i + 1}` as CommandId] = () => focusPane(wsId, id);
    }
    if (order.length > 1) {
      const at = Math.max(0, order.indexOf(focusedPaneId ?? ""));
      const step = (delta: number) =>
        focusPane(wsId, order[(at + delta + order.length) % order.length]);
      map["pane.next"] = () => step(1);
      map["pane.prev"] = () => step(-1);
    }

    const focused = focusedPaneId;
    if (!focused) return map;

    map["pane.restart"] = () => replacePane(wsId, focused, {});
    if (panes.length > 1) map["pane.close"] = () => closePane(wsId, focused);
    if (panes.length > 1) map["pane.zoom"] = () => setZoom(wsId, !zoomByWorkspace[wsId]);

    const pane = panes.find((p) => p.id === focused);
    if (pane && AGENTS.find((agent) => agent.id === pane.agent)?.resumable) {
      map["pane.sessions"] = () => setPickerPaneId(focused);
    }
    if (pinRefusal(activeWorkspace, focused) === null) {
      map["pane.pinOrchestrator"] = () => pinOrchestrator(wsId, focused, true);
    }
    if (orchestratorOf(activeWorkspace)?.id === focused) {
      map["pane.unpinOrchestrator"] = () => pinOrchestrator(wsId, focused, false);
    }

    // Counted through in the order the pane menu lists them — this workspace's
    // commands, then the global ones — so the third entry and the third key
    // are always the same command. Nothing is bound to them by default.
    const saved = mergeCommands(activeWorkspace, state.settings);
    for (let i = 0; i < Math.min(9, saved.length); i++) {
      const command = saved[i];
      map[`command.run${i + 1}` as CommandId] = () => {
        touchWorkspace(wsId);
        runSavedCommand(command, command.broadcast ? panes.map((p) => p.id) : [focused]);
      };
    }

    const toward = (dir: Direction, act: (neighbour: string) => void) => () => {
      const neighbour = neighborOf(tree, focused, dir);
      if (neighbour) act(neighbour);
    };
    const go = (dir: Direction) => toward(dir, (id) => focusPane(wsId, id));
    // Swapping is the drop-in-the-middle gesture, spelled with the keyboard.
    const swap = (dir: Direction) =>
      toward(dir, (id) => movePane(wsId, focused, id, "center"));

    map["pane.focusLeft"] = go("left");
    map["pane.focusRight"] = go("right");
    map["pane.focusUp"] = go("up");
    map["pane.focusDown"] = go("down");
    map["pane.swapLeft"] = swap("left");
    map["pane.swapRight"] = swap("right");
    map["pane.swapUp"] = swap("up");
    map["pane.swapDown"] = swap("down");

    // Resolved when the key is pressed: a pane that restarted since this
    // render has a different terminal behind the same focus.
    const term = () => getTerminal(focused);
    map["terminal.clear"] = () => term()?.clear();
    map["terminal.selectAll"] = () => term()?.selectAll();
    map["terminal.scrollTop"] = () => term()?.scrollToTop();
    map["terminal.scrollBottom"] = () => term()?.scrollToBottom();
    map["terminal.find"] = () => openFinder(focused);
    map["terminal.copy"] = () => {
      const selection = term()?.getSelection();
      if (selection) navigator.clipboard.writeText(selection).catch(() => undefined);
    };
    map["terminal.paste"] = () => {
      navigator.clipboard
        .readText()
        // Through xterm rather than straight to the PTY: it is what brackets
        // the paste, so a multi-line paste lands as text instead of being run
        // line by line.
        .then((text) => text && term()?.paste(text))
        .catch(() => undefined);
    };

    return map;
  }

  const handlers = buildHandlers();
  // While the palette is open the keyboard is its own: a shortcut acting on
  // the window behind it would change what the list is about mid-search. Only
  // its own key stays, to close it.
  const pendingChords = useShortcuts({
    keymap,
    handlers: paletteOpen ? { "view.palette": () => setPaletteOpen(false) } : handlers,
  });
  /** What the palette offers: every command a key would run, but itself. */
  const paletteHandlers = { ...handlers };
  delete paletteHandlers["view.palette"];

  /**
   * `whenFree` hands the line to each pane's hangar-bridge mod, which submits
   * it once its Claude Code has finished what it is doing — instead of typing
   * it into a permission dialog or over a half-written prompt. A pane without
   * the mod has nobody to hold the line for it, and gets it typed as usual.
   */
  function sendBroadcast(whenFree = false) {
    const text = broadcast.trim();
    if (!text) return;
    if (activeWorkspace) touchWorkspace(activeWorkspace.id);
    for (const pane of panes) {
      if (whenFree && getPaneActivity(pane.id)?.bridged) {
        bridgeEnqueue(pane.id, text).catch(() => undefined);
      } else {
        ptyWrite(pane.id, `${text}\r`).catch(() => undefined);
      }
    }
    setBroadcast("");
  }

  const bridgedPanes = useBridgedCount(panes.map((p) => p.id));

  const pickerPane = panes.find((p) => p.id === pickerPaneId) ?? null;
  const mcpServersWs = state.workspaces.find((ws) => ws.id === mcpServersFor) ?? null;
  const orchestrationWs = state.workspaces.find((ws) => ws.id === orchestrationFor) ?? null;

  if (!hydrated) {
    return <div className="boot">{t("app.loading")}</div>;
  }

  return (
    <div
      className={[
        "app",
        windowWorkspaceId ? "app--window" : "",
        navigatorShown ? "" : "app--no-nav",
      ].join(" ")}
    >
      {/* One update banner and one sidebar, both the main window's. */}
      {isMainWindow && <UpdateBanner />}
      {navigatorShown && (
        <Sidebar
          onOpenSettings={() => openSettings()}
          onNewWorkspace={() => setShowCreate(true)}
          focusByWorkspace={focusByWorkspace}
          onFocusPane={(workspaceId, paneId) => activateRef.current(workspaceId, paneId)}
          onOpenMcpServers={setMcpServersFor}
          onOpenOrchestration={setOrchestrationFor}
          sleep={forcedSleep}
        />
      )}

      <main className="main">
        {activeWorkspace ? (
          <>
            <Toolbar
              workspace={activeWorkspace}
              view={view}
              onView={setView}
              panes={panes}
              focusedPaneId={focusedPaneId}
              onFocusPane={(paneId) => focusPane(activeWorkspace.id, paneId)}
              dockShown={dockShown}
              onToggleDock={toggleDock}
              modelsOpen={modelsOpen}
              modelShown={modelShown}
              modelsUnseen={modelsUnseen}
              onToggleModels={toggleModels}
              canDetach={canDetach}
              onDetach={() => void detach(activeWorkspace.id)}
              inOwnWindow={Boolean(windowWorkspaceId)}
              onReattach={() => reattach(activeWorkspace.id)}
              // The focused pane is the model for whatever the preset has to
              // create.
              onPreset={(size) =>
                applyPreset(activeWorkspace.id, size, focusByWorkspace[activeWorkspace.id] ?? null)
              }
              onTidy={() => tidyLayout(activeWorkspace.id)}
              onSplit={() => splitPane(activeWorkspace.id, null)}
              onRestartAll={() => panes.forEach((p) => replacePane(activeWorkspace.id, p.id, {}))}
              agentPaneCount={agentPanes.length}
              confirmReset={confirmReset}
              onArmReset={() => setConfirmReset(true)}
              onCancelReset={cancelReset}
              onReset={resetAgents}
              hasNavigator={isMainWindow}
              navigatorOpen={state.settings.navigatorOpen}
              onToggleNavigator={toggleNavigator}
              inspectorOpen={inspectorShown}
              onToggleInspector={toggleInspector}
              onOpenPalette={() => setPaletteOpen(true)}
              needsYou={
                <NeedsYou
                  items={needsYou.items}
                  workspaces={state.workspaces}
                  open={needsYouOpen}
                  onOpenChange={setNeedsYouOpen}
                  onGoToPane={goToPane}
                  onOpenBoard={openBoardOf}
                  onLand={landItem}
                />
              }
              orchestrated={isOrchestrated(activeWorkspace)}
              workerCount={workersOf(activeWorkspace).length}
              onOpenOrchestration={() => setOrchestrationFor(activeWorkspace.id)}
              confirmStopWorkers={confirmStopWorkers}
              onArmStopWorkers={() => {
                setConfirmReset(false);
                setConfirmStopWorkers(true);
              }}
              onCancelStopWorkers={cancelStopWorkers}
              onStopWorkers={() => stopWorkers(activeWorkspace.id)}
            />

            {/* Always rendered, whatever the view: the grids below must keep
                the same parent for good, and the board panel sits beside
                them in this row. */}
            <div className="main__body" ref={bodyRef}>
              <div className="workarea">
                {/* One grid per workspace ever opened, all but one hidden. They
                    are never unmounted: that would kill every PTY and lose the
                    running agents — on a workspace switch as much as on the
                    switch to the board. */}
                {openWorkspaces.map((ws) => (
                  <WorkspaceTerminals
                    key={ws.id}
                    workspace={ws}
                    hidden={ws.id !== activeWorkspace.id || view !== "terminals"}
                    settings={state.settings}
                    availableAgents={availableAgents}
                    shells={shells}
                    focusedPaneId={focusByWorkspace[ws.id] ?? null}
                    zoomed={Boolean(zoomByWorkspace[ws.id])}
                    onZoomChange={(on) => setZoom(ws.id, on)}
                    onFocusPane={(paneId) => focusPane(ws.id, paneId)}
                    onSplitPane={(paneId, dir) => splitPane(ws.id, paneId, dir)}
                    onReplacePane={(paneId, patch) => replacePane(ws.id, paneId, patch)}
                    onOpenSessions={setPickerPaneId}
                    onPinOrchestrator={(paneId, pinned) => pinOrchestrator(ws.id, paneId, pinned)}
                    onOpenOrchestration={() => setOrchestrationFor(ws.id)}
                  />
                ))}

                {view === "board" && (
                  <BoardView
                    cwd={activeWorkspace.cwd}
                    onOpenMcp={() => setShowMcp(true)}
                    findAgent={findAgent}
                  />
                )}

                {view === "memory" && <MemoryView cwd={activeWorkspace.cwd} />}

                {/* Every project's statistics, not this workspace's: the
                    page has a project filter of its own. */}
                {view === "stats" && <StatsView workspaces={state.workspaces} />}

                {/* Mounted while any workspace of the window has a model
                    open, shown or not: the models loaded stay on the GPU, and
                    agents' snapshots render whatever the user is looking at.
                    It draws the preview of the workspace on screen. */}
                {windowModels && (
                  <ModelPanel
                    workspaceId={activeWorkspace.id}
                    shown={modelShown}
                    drawer={modelDrawer}
                    expanded={modelExpanded}
                    width={state.settings.modelPanelWidth}
                    drawnWidth={sides.model.width}
                    maxWidth={sides.model.max}
                    scale={uiScale}
                    paneId={focusedPaneId}
                    onResize={(modelPanelWidth) => updateSettings({ modelPanelWidth })}
                    onClose={closeModelPanel}
                    onToggleExpand={toggleModelExpand}
                  />
                )}

                {dockShown && (
                  <BoardDock
                    cwd={activeWorkspace.cwd}
                    onOpenMcp={() => setShowMcp(true)}
                    findAgent={findAgent}
                    width={state.settings.boardDockWidth}
                    drawnWidth={sides.dock.width}
                    maxWidth={sides.dock.max}
                    scale={uiScale}
                    drawer={dockDrawer}
                    paneId={focusedPaneId}
                    onResize={(boardDockWidth) => updateSettings({ boardDockWidth })}
                    onClose={closeDock}
                    onExpand={() => setView("board")}
                  />
                )}
              </div>

              {inspectorShown && (
                <Inspector
                  workspace={activeWorkspace}
                  workspaces={state.workspaces}
                  activeWorkspaceId={activeWorkspace.id}
                  pane={panes.find((p) => p.id === focusedPaneId) ?? null}
                  paneIndex={focusedPaneId ? order.indexOf(focusedPaneId) + 1 || null : null}
                  shells={shells}
                  width={state.settings.inspectorWidth}
                  maxWidth={sides.inspector.max}
                  drawer={inspectorDrawer}
                  onResize={(inspectorWidth) => updateSettings({ inspectorWidth })}
                  collapsed={state.settings.inspectorCollapsed}
                  onToggleSection={(id) => {
                    // Read from the store as it stands, not this render: two
                    // folds before a re-render would otherwise lose one.
                    const folded = snapshot().settings.inspectorCollapsed;
                    updateSettings({
                      inspectorCollapsed: folded.includes(id)
                        ? folded.filter((entry) => entry !== id)
                        : [...folded, id],
                    });
                  }}
                  onClose={closeInspector}
                  onOpenBoard={() => setView("board")}
                  onOpenSessions={setPickerPaneId}
                  onOpenMcpServers={() => setMcpServersFor(activeWorkspace.id)}
                />
              )}
            </div>

            <StatusBar
              workspace={activeWorkspace}
              showBroadcast={view === "terminals"}
              value={broadcast}
              onChange={setBroadcast}
              onSend={sendBroadcast}
              inputRef={broadcastRef}
              paneCount={panes.length}
              bridgedCount={bridgedPanes}
            />
          </>
        ) : elsewhere ? (
          <div className="placeholder">
            <h2>{t("window.elsewhereTitle", { name: elsewhere.name })}</h2>
            <p>{t("window.elsewhereBody")}</p>
            <div className="placeholder__actions">
              <button
                className="btn btn--primary"
                onClick={() => focusWorkspaceWindow(elsewhere.id).catch(() => undefined)}
              >
                {t("window.show")}
              </button>
              <button className="btn" onClick={() => reattach(elsewhere.id)}>
                {t("window.bringBack")}
              </button>
            </div>
            {/* Out of sight: the other workspaces' snapshots still render. */}
            {windowModels && (
              <ModelPanel
                workspaceId={null}
                shown={false}
                drawer={false}
                expanded={false}
                width={state.settings.modelPanelWidth}
                drawnWidth={0}
                maxWidth={state.settings.modelPanelWidth}
                scale={uiScale}
                paneId={null}
                onResize={() => undefined}
                onClose={() => undefined}
                onToggleExpand={() => undefined}
              />
            )}
          </div>
        ) : windowWorkspaceId ? (
          <div className="placeholder">
            <p>{t("app.loading")}</p>
          </div>
        ) : (
          <div className="placeholder">
            <h2>{t("placeholder.title")}</h2>
            <p>
              {t("placeholder.body", {
                agents: AGENTS.filter((a) => a.id !== "shell")
                  .map((a) => a.label)
                  .join(", "),
              })}
            </p>
            <button className="btn btn--primary" onClick={() => setShowCreate(true)}>
              {t("sidebar.new")}
            </button>
          </div>
        )}
      </main>

      {/* A microphone is silent to look at, so whether it is listening — and
          whether it can hear you — has to be somewhere on screen. */}
      {voice.phase !== "idle" && <VoiceHud phase={voice.phase} level={voice.level} />}

      {notice && (
        <div
          className={[
            "voice-pill voice-pill--notice",
            typeof notice !== "string" && `voice-pill--${notice.kind}`,
          ]
            .filter(Boolean)
            .join(" ")}
          role="status"
        >
          <span className="voice-pill__text">
            {typeof notice === "string" ? notice : notice.text}
          </span>
        </div>
      )}

      {/* A half-typed sequence is held for a moment; saying so is the only way
          the user can tell it from a keystroke that did nothing. */}
      {pendingChords.length > 0 && (
        <div className="chord-hint" role="status">
          <span className="chord-hint__keys">
            {pendingChords.map(formatChord).join(" ")}
          </span>
          {t("keymap.pending")}
        </div>
      )}

      {paletteOpen && (
        <CommandPalette
          onClose={() => setPaletteOpen(false)}
          handlers={paletteHandlers}
          keymap={keymap}
          workspaces={windowWorkspaceId ? openWorkspaces : sidebarOrder(state.workspaces, state.folders)}
          activeWorkspaceId={activeWorkspace?.id ?? null}
          focusedPaneId={focusedPaneId}
          onFocusPane={(workspaceId, paneId) => activateRef.current(workspaceId, paneId)}
          onActivateWorkspace={(id) => {
            setActiveWorkspace(id);
            setView("terminals");
          }}
        />
      )}

      {showSettings && (
        <SettingsPage
          shells={shells}
          category={settingsCategory}
          onClose={() => setShowSettings(false)}
        />
      )}

      {showCreate && (
        <WorkspaceDialog
          availableAgents={availableAgents}
          shells={shells}
          onClose={() => setShowCreate(false)}
        />
      )}

      {showMcp && activeWorkspace && (
        <McpPanel cwd={activeWorkspace.cwd} onClose={() => setShowMcp(false)} />
      )}

      {mcpServersWs && (
        <McpServersPanel
          workspace={mcpServersWs}
          // The same respawn as a restart: each pane resumes its session.
          onRestart={(paneIds) =>
            paneIds.forEach((paneId) => replacePane(mcpServersWs.id, paneId, {}))
          }
          onClose={() => setMcpServersFor(null)}
        />
      )}

      {orchestrationWs && (
        <OrchestrationPanel
          // A panel opened on one workspace's settings starts over on another's.
          key={orchestrationWs.id}
          workspace={orchestrationWs}
          onCreateOrchestrator={() => {
            setOrchestrationFor(null);
            createOrchestrator(orchestrationWs.id);
          }}
          onSaved={() =>
            setNotice({ kind: "agent", text: t("orch.saved", { workspace: orchestrationWs.name }) })
          }
          onClose={() => setOrchestrationFor(null)}
        />
      )}

      {pickerPane && activeWorkspace && (
        <SessionPicker
          agent={pickerPane.agent}
          cwd={pickerPane.cwd || activeWorkspace.cwd}
          currentId={pickerPane.sessionId}
          onClose={() => setPickerPaneId(null)}
          onPick={(sessionId) => {
            replacePane(activeWorkspace.id, pickerPane.id, { sessionId });
            setPickerPaneId(null);
          }}
        />
      )}
    </div>
  );
}
