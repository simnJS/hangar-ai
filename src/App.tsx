import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Sidebar } from "./components/Sidebar";
import { SettingsPage } from "./components/settings/SettingsPage";
import { SessionPicker } from "./components/SessionPicker";
import { WorkspaceDialog } from "./components/WorkspaceDialog";
import { WorkspaceTerminals } from "./components/WorkspaceTerminals";
import { BoardView } from "./components/BoardView";
import { McpPanel } from "./components/McpPanel";
import { UpdateBanner } from "./components/UpdateBanner";
import { VoiceHud } from "./components/VoiceHud";
import { detectAgents, detectShells, ptyWrite } from "./lib/ipc";
import {
  leafIds,
  MAX_PANES,
  neighborOf,
  normalizeTree,
  preferredDir,
  type Direction,
} from "./lib/layout";
import { buildPresence, useDiscordPresence } from "./lib/discord";
import { useVoice } from "./lib/voice";
import { formatChord } from "./lib/keys";
import { getTerminal } from "./lib/terminalRegistry";
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
    hydrated,
    activeWorkspace,
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
  const [view, setView] = useState<"terminals" | "board">("terminals");
  /** One focused pane per workspace: leaving and coming back lands you back. */
  const [focusByWorkspace, setFocusByWorkspace] = useState<Record<string, string>>({});
  const [pickerPaneId, setPickerPaneId] = useState<string | null>(null);
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

  const openWorkspaces = useMemo(
    () => state.workspaces.filter((ws) => openedIds.includes(ws.id)),
    [state.workspaces, openedIds],
  );

  // Keep focus on a pane that still exists after a layout change.
  useEffect(() => {
    if (!activeWorkspace) return;
    if (panes.length && !panes.some((p) => p.id === focusedPaneId)) {
      focusPane(activeWorkspace.id, panes[0].id);
    }
  }, [activeWorkspace, panes, focusedPaneId, focusPane]);

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

    const workspaces = state.workspaces;
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
    map["view.mcp"] = () => setShowMcp(true);

    // Everything below acts on panes, which are only on screen — and are only
    // what a keystroke can be about — in the terminals view.
    if (view !== "terminals" || !tree) return map;

    map["view.broadcast"] = () => broadcastRef.current?.focus();
    map["pane.split"] = () => splitPane(wsId, null);
    map["pane.splitRight"] = () => splitPane(wsId, null, "row");
    map["pane.splitDown"] = () => splitPane(wsId, null, "col");
    map["pane.restartAll"] = () => panes.forEach((p) => replacePane(wsId, p.id, {}));

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

    const pane = panes.find((p) => p.id === focused);
    if (pane && AGENTS.find((agent) => agent.id === pane.agent)?.resumable) {
      map["pane.sessions"] = () => setPickerPaneId(focused);
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

  function sendBroadcast() {
    const text = broadcast.trim();
    if (!text) return;
    for (const pane of panes) {
      ptyWrite(pane.id, `${text}\r`).catch(() => undefined);
    }
    setBroadcast("");
  }

  const pickerPane = panes.find((p) => p.id === pickerPaneId) ?? null;

  if (!hydrated) {
    return <div className="boot">{t("app.loading")}</div>;
  }

  return (
    <div className="app">
      <UpdateBanner />
      <Sidebar
        onOpenSettings={() => openSettings()}
        onNewWorkspace={() => setShowCreate(true)}
      />

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
                    {t("topbar.addPane")}
                  </button>

                  <button
                    className="btn btn--ghost"
                    onClick={() =>
                      panes.forEach((p) => replacePane(activeWorkspace.id, p.id, {}))
                    }
                    title={withKeys(t("topbar.restartAllHint"), "pane.restartAll")}
                  >
                    {t("topbar.restartAll")}
                  </button>
                </>
              )}
            </header>

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
                onFocusPane={(paneId) => focusPane(ws.id, paneId)}
                onSplitPane={(paneId) => splitPane(ws.id, paneId)}
                onReplacePane={(paneId, patch) => replacePane(ws.id, paneId, patch)}
                onOpenSessions={setPickerPaneId}
              />
            ))}

            {view === "board" && (
              <BoardView cwd={activeWorkspace.cwd} onOpenMcp={() => setShowMcp(true)} />
            )}

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
                  if (e.key === "Enter") sendBroadcast();
                }}
              />
              <button
                className="btn btn--primary"
                onClick={sendBroadcast}
                disabled={!broadcast.trim()}
              >
                {t("broadcast.send")}
              </button>
            </footer>
            )}
          </>
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
