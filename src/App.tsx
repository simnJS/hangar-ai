import { useCallback, useEffect, useMemo, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { SettingsPanel } from "./components/SettingsPanel";
import { SessionPicker } from "./components/SessionPicker";
import { WorkspaceDialog } from "./components/WorkspaceDialog";
import { WorkspaceTerminals } from "./components/WorkspaceTerminals";
import { BoardView } from "./components/BoardView";
import { McpPanel } from "./components/McpPanel";
import { UpdateBanner } from "./components/UpdateBanner";
import { detectAgents, detectShells, ptyWrite } from "./lib/ipc";
import { leafIds, MAX_PANES, normalizeTree, preferredDir } from "./lib/layout";
import { useT } from "./i18n";
import { useStore } from "./store";
import { applyThemeToDocument, getTheme } from "./themes";
import { AGENTS, type LayoutSize, type Pane, type ShellInfo } from "./types";

const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

export default function App() {
  const { state, hydrated, activeWorkspace, respawnPane, applyPreset, addPane, closePane } =
    useStore();
  const [showSettings, setShowSettings] = useState(false);
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

  /** Pane ids in reading order: Ctrl+N follows the arrangement on screen. */
  const order = useMemo(
    () => (activeWorkspace ? leafIds(normalizeTree(panes, activeWorkspace.tree)) : []),
    [panes, activeWorkspace],
  );

  const focusedPaneId = activeWorkspace
    ? (focusByWorkspace[activeWorkspace.id] ?? null)
    : null;

  const focusPane = useCallback((workspaceId: string, paneId: string) => {
    setFocusByWorkspace((current) => ({ ...current, [workspaceId]: paneId }));
  }, []);

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

  const splitPane = useCallback(
    (workspaceId: string, paneId: string | null) => {
      const near = paneId ?? focusByWorkspace[workspaceId] ?? null;
      const created = addPane(workspaceId, { near, dir: preferredDir(near) });
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

  // Ctrl+1..9 jumps between panes, Ctrl+, toggles settings,
  // Ctrl+Shift+Enter splits the focused pane and Ctrl+Shift+X closes it.
  useEffect(() => {
    /**
     * A shortcut we act on goes no further. This listener runs on the capture
     * phase of `window`, above everything: left to propagate, Ctrl+Shift+Enter
     * would also reach the broadcast input — which sends on Enter — and the
     * focused terminal, so one keystroke would do two things. Stopping here
     * rather than filtering on the target is what keeps the shortcuts working
     * from inside a terminal, whose xterm surface is a <textarea>.
     */
    function consume(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
    }

    function onKey(event: KeyboardEvent) {
      if (!event.ctrlKey || event.altKey) return;

      if (event.key === "," && !event.shiftKey) {
        consume(event);
        setShowSettings((open) => !open);
        return;
      }

      // Everything below acts on panes, which only exist on screen — and are
      // only what the keystroke can be about — in the terminals view.
      if (!activeWorkspace || view !== "terminals") return;

      if (event.shiftKey && event.key === "Enter") {
        consume(event);
        splitPane(activeWorkspace.id, null);
        return;
      }

      if (event.shiftKey && (event.key === "X" || event.key === "x")) {
        consume(event);
        if (focusedPaneId && panes.length > 1) {
          closePane(activeWorkspace.id, focusedPaneId);
        }
        return;
      }

      const digit = Number(event.key);
      if (Number.isInteger(digit) && digit >= 1 && digit <= order.length) {
        consume(event);
        focusPane(activeWorkspace.id, order[digit - 1]);
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [
    order,
    panes.length,
    focusedPaneId,
    activeWorkspace,
    view,
    closePane,
    splitPane,
    focusPane,
  ]);

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
        onOpenSettings={() => setShowSettings(true)}
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
                        onClick={() => applyPreset(activeWorkspace.id, size)}
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
                    title={t("topbar.addPaneHint")}
                  >
                    {t("topbar.addPane")}
                  </button>

                  <button
                    className="btn btn--ghost"
                    onClick={() =>
                      panes.forEach((p) => replacePane(activeWorkspace.id, p.id, {}))
                    }
                    title={t("topbar.restartAllHint")}
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

      {showSettings && (
        <SettingsPanel shells={shells} onClose={() => setShowSettings(false)} />
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
