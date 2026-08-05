import { useEffect, useMemo, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { SettingsPanel } from "./components/SettingsPanel";
import { SessionPicker } from "./components/SessionPicker";
import { TerminalPane } from "./components/TerminalPane";
import { WorkspaceDialog } from "./components/WorkspaceDialog";
import { BoardView } from "./components/BoardView";
import { McpPanel } from "./components/McpPanel";
import { PaneGrid, normalizeTree } from "./components/PaneGrid";
import { UpdateBanner } from "./components/UpdateBanner";
import { detectAgents, detectShells, ptyWrite } from "./lib/ipc";
import { resolveShell } from "./lib/shells";
import { useStore } from "./store";
import { applyThemeToDocument, getTheme } from "./themes";
import { AGENTS, type AgentId, type LayoutSize, type ShellInfo } from "./types";

const LAYOUTS: LayoutSize[] = [1, 2, 4, 8];

export default function App() {
  const { state, hydrated, activeWorkspace, updatePane, setLayout, updateWorkspace } =
    useStore();
  const [showSettings, setShowSettings] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [showMcp, setShowMcp] = useState(false);
  const [view, setView] = useState<"terminals" | "board">("terminals");
  const [focusedPaneId, setFocusedPaneId] = useState<string | null>(null);
  const [pickerPaneId, setPickerPaneId] = useState<string | null>(null);
  const [availableAgents, setAvailableAgents] = useState<string[]>([]);
  const [shells, setShells] = useState<ShellInfo[]>([]);
  const [broadcast, setBroadcast] = useState("");

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

  const panes = activeWorkspace ? activeWorkspace.panes.slice(0, activeWorkspace.layout) : [];

  const paneTree = useMemo(
    () => normalizeTree(activeWorkspace?.layout ?? 1, activeWorkspace?.tree),
    [activeWorkspace?.layout, activeWorkspace?.tree],
  );

  // Keep focus on a pane that still exists after a layout change.
  useEffect(() => {
    if (panes.length && !panes.some((p) => p.id === focusedPaneId)) {
      setFocusedPaneId(panes[0].id);
    }
  }, [panes, focusedPaneId]);

  // Ctrl+1..8 jumps between panes; Ctrl+, toggles settings.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (!event.ctrlKey || event.altKey) return;
      if (event.key === ",") {
        event.preventDefault();
        setShowSettings((open) => !open);
        return;
      }
      const digit = Number(event.key);
      if (Number.isInteger(digit) && digit >= 1 && digit <= panes.length) {
        event.preventDefault();
        setFocusedPaneId(panes[digit - 1].id);
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [panes]);

  function replacePane(paneId: string, patch: Record<string, unknown>) {
    if (!activeWorkspace) return;
    // A fresh id remounts the terminal, which is how a pane restarts.
    updatePane(activeWorkspace.id, paneId, { ...patch, id: crypto.randomUUID() });
  }

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
    return <div className="boot">Chargement…</div>;
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

              <div className="layouts" role="group" aria-label="Vue">
                <button
                  className={`layouts__btn layouts__btn--wide ${view === "terminals" ? "is-active" : ""}`}
                  onClick={() => setView("terminals")}
                >
                  Terminaux
                </button>
                <button
                  className={`layouts__btn layouts__btn--wide ${view === "board" ? "is-active" : ""}`}
                  onClick={() => setView("board")}
                >
                  Tableau
                </button>
              </div>

              {view === "terminals" && (
                <>
                  <div className="layouts" role="group" aria-label="Disposition">
                    {LAYOUTS.map((size) => (
                      <button
                        key={size}
                        className={`layouts__btn ${activeWorkspace.layout === size ? "is-active" : ""}`}
                        onClick={() => setLayout(activeWorkspace.id, size)}
                        title={`${size} panneau${size > 1 ? "x" : ""}`}
                      >
                        {size}
                      </button>
                    ))}
                  </div>

                  <button
                    className="btn btn--ghost"
                    onClick={() => panes.forEach((p) => replacePane(p.id, {}))}
                    title="Relancer tous les panneaux"
                  >
                    ↻ Tout relancer
                  </button>
                </>
              )}
            </header>

            {/* Hidden, never unmounted: unmounting would kill every PTY and
                lose the running agents when switching to the board. */}
            <PaneGrid
              hidden={view !== "terminals"}
              tree={paneTree}
              onTreeChange={(tree) => updateWorkspace(activeWorkspace.id, { tree })}
              renderPane={(index) => {
                const pane = panes[index];
                if (!pane) return null;
                return (
                <TerminalPane
                  key={pane.id}
                  pane={pane}
                  index={index}
                  cwd={activeWorkspace.cwd}
                  settings={state.settings}
                  theme={theme}
                  focused={focusedPaneId === pane.id}
                  availableAgents={availableAgents}
                  shells={shells}
                  shell={resolveShell(
                    shells,
                    pane.shellId,
                    activeWorkspace.shellId,
                    state.settings.shellId,
                  )}
                  onFocus={() => setFocusedPaneId(pane.id)}
                  onAgentChange={(agent: AgentId) =>
                    replacePane(pane.id, { agent, sessionId: null })
                  }
                  onShellChange={(shellId) => replacePane(pane.id, { shellId })}
                  onSessionCaptured={(sessionId) =>
                    updatePane(activeWorkspace.id, pane.id, { sessionId })
                  }
                  onRestart={() => replacePane(pane.id, {})}
                  onOpenSessions={() => setPickerPaneId(pane.id)}
                />
                );
              }}
            />

            {view === "board" && (
              <BoardView cwd={activeWorkspace.cwd} onOpenMcp={() => setShowMcp(true)} />
            )}

            {view === "terminals" && (
            <footer className="broadcast">
              <span className="broadcast__icon">⇉</span>
              <input
                className="broadcast__input"
                placeholder={`Envoyer la même instruction aux ${panes.length} panneaux…`}
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
                Diffuser
              </button>
            </footer>
            )}
          </>
        ) : (
          <div className="placeholder">
            <h2>Aucun workspace ouvert</h2>
            <p>
              Crée un workspace depuis la barre latérale : choisis un dossier de projet, puis
              répartis tes agents (
              {AGENTS.filter((a) => a.id !== "shell")
                .map((a) => a.label)
                .join(", ")}
              ) dans 1, 2, 4 ou 8 panneaux.
            </p>
            <button className="btn btn--primary" onClick={() => setShowCreate(true)}>
              + Nouveau workspace
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
            replacePane(pickerPane.id, { sessionId });
            setPickerPaneId(null);
          }}
        />
      )}
    </div>
  );
}
