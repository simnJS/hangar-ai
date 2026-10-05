import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Sidebar } from "./components/Sidebar";
import { SettingsPage } from "./components/settings/SettingsPage";
import { SessionPicker } from "./components/SessionPicker";
import { WorkspaceDialog } from "./components/WorkspaceDialog";
import { WorkspaceTerminals } from "./components/WorkspaceTerminals";
import { BoardView } from "./components/BoardView";
import { BoardDock } from "./components/BoardDock";
import { MemoryView } from "./components/MemoryView";
import { McpPanel } from "./components/McpPanel";
import { UpdateBanner } from "./components/UpdateBanner";
import { VoiceHud } from "./components/VoiceHud";
import {
  bridgeEnqueue,
  detectAgents,
  detectShells,
  dirExists,
  ptyAlive,
  ptyKill,
  ptyWrite,
} from "./lib/ipc";
import { usePaneControl } from "./lib/control";
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
import { getPaneActivity, useBridgedCount } from "./lib/agentState";
import { Icon } from "./components/Icon";
import {
  leafIds,
  MAX_PANES,
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
import type { CommandId } from "./lib/shortcuts";
import {
  useKeymap,
  useShortcuts,
  useShortcutTitle,
  type ShortcutHandlers,
} from "./lib/useShortcuts";
import { useT } from "./i18n";
import { useStore } from "./store";
import { applyThemeToDocument, getTheme } from "./themes";
import {
  AGENTS,
  DEFAULT_SETTINGS,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
  type LayoutSize,
  type Pane,
  type ShellInfo,
} from "./types";

const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

/** Same bounds as the font size slider in the settings. */
const FONT_MIN = 9;
const FONT_MAX = 24;

export default function App() {
  const {
    state,
    snapshot,
    hydrated,
    activeWorkspace: storeActiveWorkspace,
    updateWorkspace,
    respawnPane,
    applyPreset,
    addPane,
    closePane,
    movePane,
    setActiveWorkspace,
    updateSettings,
  } = useStore();
  const [showSettings, setShowSettings] = useState(false);
  /** Category a shortcut asked the settings to open on. */
  const [settingsCategory, setSettingsCategory] = useState("general");
  const [showCreate, setShowCreate] = useState(false);
  const [showMcp, setShowMcp] = useState(false);
  const [view, setView] = useState<"terminals" | "board" | "memory">("terminals");

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
  const [availableAgents, setAvailableAgents] = useState<string[]>([]);
  const [shells, setShells] = useState<ShellInfo[]>([]);
  const [broadcast, setBroadcast] = useState("");
  const [openedIds, setOpenedIds] = useState<string[]>([]);
  /** Transient line at the bottom of the window — dictation errors, for now. */
  const [notice, setNotice] = useState<string | null>(null);
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
  useEffect(() => {
    const percent = Math.min(UI_SCALE_MAX, Math.max(UI_SCALE_MIN, state.settings.uiScale || 100));
    document.documentElement.style.setProperty("--ui-scale", String(percent / 100));
  }, [state.settings.uiScale]);

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

  /** The board panel only ever sits beside the terminals. */
  const dockShown = view === "terminals" && state.settings.boardDockOpen;
  const toggleDock = () => {
    if (view === "terminals") {
      updateSettings({ boardDockOpen: !state.settings.boardDockOpen });
      return;
    }
    setView("terminals");
    updateSettings({ boardDockOpen: true });
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
  usePaneControl(
    hydrated
      ? {
          snapshot,
          enabled: () => snapshot().settings.agentPaneControl !== false,
          addPane,
          closePane,
          respawnPane: (workspaceId, paneId, patch) => {
            const nextId = respawnPane(workspaceId, paneId, patch);
            if (nextId && focusRef.current[workspaceId] === paneId) focusPane(workspaceId, nextId);
            return nextId;
          },
          openWorkspace: (id) =>
            setOpenedIds((current) => (current.includes(id) ? current : [...current, id])),
          isOpen: (id) =>
            openedRef.current.includes(id) ||
            Boolean(snapshot().workspaces.find((ws) => ws.id === id)?.detached),
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
            setNotice(
              t(`control.${event.kind}`, {
                by: event.by ?? t("control.someone"),
                pane: event.pane,
                workspace: event.workspace,
              }),
            ),
          sleep: (ms) => new Promise((resolve) => window.setTimeout(resolve, ms)),
        }
      : null,
  );

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
      updateWorkspace(ws.id, { detached: true });
      // The main window moves on to a workspace it can still show.
      const next = sidebarOrder(snapshot().workspaces, snapshot().folders).find(
        (entry) => entry.id !== ws.id && !entry.detached,
      );
      if (next) setActiveWorkspace(next.id);
      try {
        await openWorkspaceWindow(ws.id, ws.name, ws.windowBounds);
      } catch (err) {
        // No window: it comes straight back, its panes still running.
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
      return () => {
        stop.then((off) => off()).catch(() => undefined);
      };
    }
    relayActivity((paneId, value) => {
      emit("agent:activity", { from: WINDOW_LABEL, paneId, value }).catch(() => undefined);
    });
    // The main window carries out the agents' requests; this one lends it the
    // terminals it draws.
    const stopServing = serveTerminals(getTerminal);
    return () => {
      relayActivity(null);
      stopServing();
    };
  }, []);

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

  /** The confirm button had the keyboard; hand it back to the terminal. */
  const cancelReset = () => {
    setConfirmReset(false);
    if (focusedPaneId) getTerminal(focusedPaneId)?.focus();
  };

  // An armed confirmation is about the workspace it was armed on.
  useEffect(() => {
    setConfirmReset(false);
  }, [activeWorkspace?.id, view]);

  /** Opening always names a category, so a shortcut lands where it promised. */
  const openSettings = useCallback((category = "general") => {
    setSettingsCategory(category);
    setShowSettings(true);
  }, []);

  const keymap = useKeymap();
  const withKeys = useShortcutTitle();

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
    }

    if (!activeWorkspace) return map;
    const wsId = activeWorkspace.id;

    map["view.terminals"] = () => setView("terminals");
    map["view.board"] = () => setView("board");
    map["view.boardDock"] = toggleDock;
    map["view.memory"] = () => setView("memory");
    map["view.mcp"] = () => setShowMcp(true);

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

    // Counted through in the order the pane menu lists them — this workspace's
    // commands, then the global ones — so the third entry and the third key
    // are always the same command. Nothing is bound to them by default.
    const saved = mergeCommands(activeWorkspace, state.settings);
    for (let i = 0; i < Math.min(9, saved.length); i++) {
      const command = saved[i];
      map[`command.run${i + 1}` as CommandId] = () =>
        runSavedCommand(command, command.broadcast ? panes.map((p) => p.id) : [focused]);
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

  const pendingChords = useShortcuts({ keymap, handlers: buildHandlers() });

  /**
   * `whenFree` hands the line to each pane's hangar-bridge mod, which submits
   * it once its Claude Code has finished what it is doing — instead of typing
   * it into a permission dialog or over a half-written prompt. A pane without
   * the mod has nobody to hold the line for it, and gets it typed as usual.
   */
  function sendBroadcast(whenFree = false) {
    const text = broadcast.trim();
    if (!text) return;
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

  if (!hydrated) {
    return <div className="boot">{t("app.loading")}</div>;
  }

  return (
    <div className={`app ${windowWorkspaceId ? "app--window" : ""}`}>
      {/* One update banner and one sidebar, both the main window's. */}
      {isMainWindow && <UpdateBanner />}
      {isMainWindow && (
        <Sidebar
          onOpenSettings={() => openSettings()}
          onNewWorkspace={() => setShowCreate(true)}
        />
      )}

      <main className="main">
        {activeWorkspace ? (
          <>
            <header className="topbar">
              <div className="topbar__id">
                <h1 className="topbar__name">{activeWorkspace.name}</h1>
                <span className="topbar__path" title={activeWorkspace.cwd}>
                  {activeWorkspace.cwd}
                </span>
              </div>

              <div className="layouts" role="group" aria-label={t("view.group")}>
                <button
                  className={`layouts__btn layouts__btn--wide ${view === "terminals" ? "is-active" : ""}`}
                  onClick={() => setView("terminals")}
                >
                  {t("view.terminals")}
                </button>
                <button
                  className={`layouts__btn layouts__btn--wide ${view === "board" ? "is-active" : ""}`}
                  onClick={() => setView("board")}
                >
                  {t("view.board")}
                </button>
                <button
                  className={`layouts__btn layouts__btn--wide ${view === "memory" ? "is-active" : ""}`}
                  onClick={() => setView("memory")}
                >
                  {t("view.memory")}
                </button>
              </div>

              <div className="layouts">
                <button
                  className={`layouts__btn layouts__btn--wide ${dockShown ? "is-active" : ""}`}
                  onClick={toggleDock}
                  aria-pressed={dockShown}
                  title={withKeys(t("board.dockHint"), "view.boardDock")}
                >
                  ◧ {t("board.dock")}
                </button>
                {canDetach && (
                  <button
                    className="layouts__btn layouts__btn--wide"
                    onClick={() => void detach(activeWorkspace.id)}
                    title={t("window.detachHint")}
                  >
                    ⧉ {t("window.detach")}
                  </button>
                )}
                {windowWorkspaceId && (
                  <button
                    className="layouts__btn layouts__btn--wide"
                    onClick={() => reattach(activeWorkspace.id)}
                    title={t("window.reattachHint")}
                  >
                    ⤓ {t("window.reattach")}
                  </button>
                )}
              </div>

              {view === "terminals" && (
                <>
                  <div className="layouts" role="group" aria-label={t("topbar.layout")}>
                    {LAYOUTS.map((size) => (
                      <button
                        key={size}
                        className={`layouts__btn ${panes.length === size ? "is-active" : ""}`}
                        onClick={() =>
                          // The focused pane is the model for whatever the
                          // preset has to create.
                          applyPreset(
                            activeWorkspace.id,
                            size,
                            focusByWorkspace[activeWorkspace.id] ?? null,
                          )
                        }
                        title={t("topbar.presetHint", { n: size })}
                      >
                        {size}
                      </button>
                    ))}
                  </div>

                  <button
                    className="btn btn--ghost"
                    onClick={() => splitPane(activeWorkspace.id, null)}
                    disabled={panes.length >= MAX_PANES}
                    title={withKeys(t("topbar.addPaneHint"), "pane.split")}
                  >
                    <Icon name="plus" size={13} />
                    {t("topbar.addPane")}
                  </button>

                  <button
                    className="btn btn--ghost"
                    onClick={() =>
                      panes.forEach((p) => replacePane(activeWorkspace.id, p.id, {}))
                    }
                    title={withKeys(t("topbar.restartAllHint"), "pane.restartAll")}
                  >
                    <Icon name="restart" size={13} />
                    {t("topbar.restartAll")}
                  </button>

                  {confirmReset ? (
                    <>
                      <button className="btn btn--ghost" onClick={cancelReset}>
                        {t("create.cancel")}
                      </button>
                      <button
                        className="btn btn--danger"
                        onClick={resetAgents}
                        onKeyDown={(event) => event.key === "Escape" && cancelReset()}
                        // Armed from the keyboard as often as from a click, so
                        // the answer is one Enter away either way.
                        autoFocus
                      >
                        {t("topbar.resetAllConfirm", { n: agentPanes.length })}
                      </button>
                    </>
                  ) : (
                    <button
                      className="btn btn--ghost"
                      onClick={() => setConfirmReset(true)}
                      disabled={!agentPanes.length}
                      title={
                        agentPanes.length
                          ? withKeys(t("topbar.resetAllHint"), "pane.resetAll")
                          : t("topbar.resetAllNone")
                      }
                    >
                      <Icon name="reset" size={13} />
                      {t("topbar.resetAll")}
                    </button>
                  )}
                </>
              )}
            </header>

            {/* Always rendered, whatever the view: the grids below must keep
                the same parent for good, and the board panel sits beside
                them in this row. */}
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
                  onSplitPane={(paneId) => splitPane(ws.id, paneId)}
                  onReplacePane={(paneId, patch) => replacePane(ws.id, paneId, patch)}
                  onOpenSessions={setPickerPaneId}
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

              {dockShown && (
                <BoardDock
                  cwd={activeWorkspace.cwd}
                  onOpenMcp={() => setShowMcp(true)}
                  findAgent={findAgent}
                  width={state.settings.boardDockWidth}
                  onResize={(boardDockWidth) => updateSettings({ boardDockWidth })}
                  onClose={() => updateSettings({ boardDockOpen: false })}
                  onExpand={() => setView("board")}
                />
              )}
            </div>

            {view === "terminals" && (
            <footer className="broadcast">
              <span className="broadcast__icon">⇉</span>
              <input
                className="broadcast__input"
                ref={broadcastRef}
                placeholder={t("broadcast.placeholder", { n: panes.length })}
                value={broadcast}
                onChange={(e) => setBroadcast(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") sendBroadcast(e.altKey);
                }}
              />
              <button
                className="btn btn--ghost"
                onClick={() => sendBroadcast(true)}
                disabled={!broadcast.trim()}
                title={t("broadcast.whenFreeHint", { n: bridgedPanes })}
              >
                <Icon name="hourglass" size={12} />
                {t("broadcast.whenFree")}
              </button>
              <button
                className="btn btn--primary"
                onClick={() => sendBroadcast()}
                disabled={!broadcast.trim()}
              >
                {t("broadcast.send")}
              </button>
            </footer>
            )}
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
        <div className="voice-pill voice-pill--notice" role="status">
          <span className="voice-pill__text">{notice}</span>
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
