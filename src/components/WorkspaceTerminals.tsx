import { useEffect, useMemo, useRef, useState } from "react";
import { CommandDialog } from "./CommandDialog";
import { DiffView } from "./DiffView";
import { PaneGrid } from "./PaneGrid";
import { TerminalPane } from "./TerminalPane";
import { dropText, watchFileDrops, type Point } from "../lib/fileDrop";
import { leafIds, normalizeTree } from "../lib/layout";
import { runSavedCommand } from "../lib/savedCommands";
import { resolveShell } from "../lib/shells";
import { getTerminal } from "../lib/terminalRegistry";
import { useStore, type CommandScope } from "../store";
import { getTheme } from "../themes";
import type {
  AgentId,
  Pane,
  SavedCommand,
  Settings,
  ShellInfo,
  Workspace,
} from "../types";

/**
 * Every pane of one workspace.
 *
 * Split out so the app can keep several workspaces mounted at once: a
 * workspace the user leaves is only hidden, never unmounted, because
 * unmounting a pane kills its PTY and with it whatever agent was running.
 */

interface Props {
  workspace: Workspace;
  hidden: boolean;
  settings: Settings;
  availableAgents: string[];
  shells: ShellInfo[];
  focusedPaneId: string | null;
  /** The focused pane fills the grid. The zoom follows the focus around. */
  zoomed: boolean;
  onZoomChange: (zoomed: boolean) => void;
  onFocusPane: (paneId: string) => void;
  onSplitPane: (paneId: string) => void;
  /** Restart a pane under a fresh id, optionally patching it first. */
  onReplacePane: (paneId: string, patch: Partial<Pane>) => void;
  onOpenSessions: (paneId: string) => void;
}

export function WorkspaceTerminals({
  workspace,
  hidden,
  settings,
  availableAgents,
  shells,
  focusedPaneId,
  zoomed,
  onZoomChange,
  onFocusPane,
  onSplitPane,
  onReplacePane,
  onOpenSessions,
}: Props) {
  const {
    setTree,
    movePane,
    closePane,
    updatePane,
    addSavedCommand,
    updateSavedCommand,
    removeSavedCommand,
    moveSavedCommand,
  } = useStore();

  /**
   * The command editor, hosted here rather than in each pane: it is a modal
   * over the whole window, and one per pane would mean eight of them listening
   * for the same Escape.
   */
  const [editor, setEditor] = useState<{
    /** null while a command is being written for the first time. */
    command: SavedCommand | null;
    scope: CommandScope;
  } | null>(null);

  /** The pane whose folder the diff view is showing, hosted here for the
      same reason as the editor. */
  const [diffPane, setDiffPane] = useState<Pane | null>(null);

  // Dropped rather than merely hidden when the workspace leaves the screen:
  // kept, it would come back with the grid — an editor the user walked away
  // from, holding whatever they had typed before switching.
  useEffect(() => {
    if (hidden) {
      setEditor(null);
      setDiffPane(null);
    }
  }, [hidden]);

  const panes = workspace.panes;

  const tree = useMemo(
    () => normalizeTree(panes, workspace.tree),
    [panes, workspace.tree],
  );

  /** Pane ids in reading order — drives the numbering shown on each pane. */
  const order = useMemo(() => leafIds(tree), [tree]);

  const zoomedId = zoomed && panes.length > 1 ? focusedPaneId : null;

  // A split, a close or a preset ends the zoom, as it does in tmux: the user
  // asked for a change to the arrangement and expects to see it.
  const paneCount = panes.length;
  const countRef = useRef(paneCount);
  useEffect(() => {
    if (countRef.current === paneCount) return;
    countRef.current = paneCount;
    if (zoomed) onZoomChange(false);
  }, [paneCount, zoomed, onZoomChange]);

  function zoomTo(paneId: string | null) {
    if (paneId) onFocusPane(paneId);
    onZoomChange(paneId !== null);
    // A mouse gesture asked for it, and a click on the header leaves the
    // keyboard nowhere. Next frame, once the pane is displayed again: a
    // hidden textarea cannot take the focus.
    const target = paneId ?? focusedPaneId;
    requestAnimationFrame(() => getTerminal(target)?.focus());
  }

  /** The pane a file drag is over, and how many files it carries. */
  const [fileTarget, setFileTarget] = useState<{ paneId: string; count: number } | null>(
    null,
  );

  // The listeners are registered once per visibility change; the drop itself
  // has to see the panes, shells and settings as they are when it lands.
  const panesRef = useRef(panes);
  panesRef.current = panes;
  const dropRef = useRef<(paneId: string, paths: string[]) => void>(() => {});
  dropRef.current = (paneId, paths) => {
    const pane = panes.find((p) => p.id === paneId);
    if (!pane) return;
    const shell = resolveShell(shells, pane.shellId, workspace.shellId, settings.shellId);
    onFocusPane(paneId);
    const term = getTerminal(paneId);
    // Through xterm, as a paste: an agent that brackets pastes sees one, which
    // is how Claude Code tells a dropped image path from typed text.
    term?.paste(dropText(paths, pane.agent, shell));
    term?.focus();
  };

  // Only the workspace on screen listens: the drop lands on whatever pane is
  // under the pointer, and a hidden grid has none.
  useEffect(() => {
    if (hidden) return;
    const paneAt = ({ x, y }: Point) => {
      const id = document
        .elementFromPoint(x, y)
        ?.closest<HTMLElement>("[data-slot]")?.dataset.slot;
      return id && panesRef.current.some((p) => p.id === id) ? id : null;
    };
    const stop = watchFileDrops({
      over: (point, count) => {
        const paneId = point && paneAt(point);
        setFileTarget((current) =>
          !paneId
            ? null
            : current?.paneId === paneId && current.count === count
              ? current
              : { paneId, count },
        );
      },
      drop: (paths, point) => {
        const paneId = paneAt(point);
        if (paneId) dropRef.current(paneId, paths);
      },
    });
    return () => {
      stop();
      setFileTarget(null);
    };
  }, [hidden]);

  // A workspace can override the global theme, and hidden workspaces keep
  // their own: their terminals stay styled the way their owner set them.
  const theme = useMemo(
    () => getTheme(workspace.themeId ?? settings.themeId),
    [workspace.themeId, settings.themeId],
  );

  return (
    <>
      <PaneGrid
        hidden={hidden}
        panes={panes}
        tree={tree}
        onTreeChange={(next) => setTree(workspace.id, next)}
        onMove={(dragId, targetId, zone) =>
          movePane(workspace.id, dragId, targetId, zone)
        }
        zoomedId={zoomedId}
        onZoom={zoomTo}
        fileTarget={fileTarget}
        renderPane={(pane) => (
          <TerminalPane
            pane={pane}
            workspaceId={workspace.id}
            workspaceName={workspace.name}
            index={Math.max(0, order.indexOf(pane.id))}
            cwd={workspace.cwd}
            extraRoots={workspace.extraRoots}
            settings={settings}
            theme={theme}
            focused={focusedPaneId === pane.id}
            // Focus is per workspace, so a pane can be the focused one of a
            // workspace nobody is looking at. Each pane is told whether its
            // grid is on screen, which is what decides who takes the keyboard
            // and whose hand-back is worth a notification. A pane hidden
            // behind a zoomed one is off screen just the same.
            visible={!hidden && (!zoomedId || zoomedId === pane.id)}
            availableAgents={availableAgents}
            shells={shells}
            shell={resolveShell(
              shells,
              pane.shellId,
              workspace.shellId,
              settings.shellId,
            )}
            canClose={panes.length > 1}
            workspaceCommands={workspace.savedCommands}
            globalCommands={settings.savedCommands}
            onFocus={() => onFocusPane(pane.id)}
            onAgentChange={(agent: AgentId) =>
              onReplacePane(pane.id, { agent, sessionId: null })
            }
            onShellChange={(shellId) => onReplacePane(pane.id, { shellId })}
            onSessionCaptured={(sessionId) =>
              updatePane(workspace.id, pane.id, { sessionId })
            }
            onRestart={() => onReplacePane(pane.id, {})}
            onSplit={() => onSplitPane(pane.id)}
            onClose={() => closePane(workspace.id, pane.id)}
            onOpenSessions={() => onOpenSessions(pane.id)}
            // A broadcast command goes to the whole workspace, which is the
            // broadcast field's target spelled once and kept.
            onRunCommand={(command) =>
              runSavedCommand(
                command,
                command.broadcast ? panes.map((p) => p.id) : [pane.id],
              )
            }
            onEditCommand={(command, scope) => setEditor({ command, scope })}
            onRemoveCommand={(command, scope) =>
              removeSavedCommand(scope, workspace.id, command.id)
            }
            onAddCommand={() => setEditor({ command: null, scope: "workspace" })}
            onShowDiff={() => setDiffPane(pane)}
          />
        )}
      />

      {diffPane && !hidden && (
        <DiffView
          cwd={diffPane.cwd || workspace.cwd}
          label={diffPane.name}
          onClose={() => setDiffPane(null)}
        />
      )}

      {/* Not rendered while the workspace is off screen: a modal nobody can
          see would still be answering Escape for the workspace in front. */}
      {editor && !hidden && (
        <CommandDialog
          editing={editor.command ? { command: editor.command, scope: editor.scope } : null}
          onClose={() => setEditor(null)}
          onSubmit={(draft, scope) => {
            const editing = editor.command;
            if (!editing) {
              addSavedCommand(scope, workspace.id, draft);
            } else {
              // Patched where it currently is, then moved if the scope changed
              // — both writes land against the same tick, so the second one
              // carries the text the first one just wrote.
              updateSavedCommand(editor.scope, workspace.id, editing.id, draft);
              if (scope !== editor.scope) {
                moveSavedCommand(editor.scope, scope, workspace.id, editing.id);
              }
            }
            setEditor(null);
          }}
        />
      )}
    </>
  );
}
