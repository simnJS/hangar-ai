import { useMemo } from "react";
import { PaneGrid } from "./PaneGrid";
import { TerminalPane } from "./TerminalPane";
import { leafIds, normalizeTree } from "../lib/layout";
import { resolveShell } from "../lib/shells";
import { useStore } from "../store";
import { getTheme } from "../themes";
import type { AgentId, Pane, Settings, ShellInfo, Workspace } from "../types";

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
  onFocusPane,
  onSplitPane,
  onReplacePane,
  onOpenSessions,
}: Props) {
  const { setTree, movePane, closePane, updatePane } = useStore();

  const panes = workspace.panes;

  const tree = useMemo(
    () => normalizeTree(panes, workspace.tree),
    [panes, workspace.tree],
  );

  /** Pane ids in reading order — drives the numbering shown on each pane. */
  const order = useMemo(() => leafIds(tree), [tree]);

  // A workspace can override the global theme, and hidden workspaces keep
  // their own: their terminals stay styled the way their owner set them.
  const theme = useMemo(
    () => getTheme(workspace.themeId ?? settings.themeId),
    [workspace.themeId, settings.themeId],
  );

  return (
    <PaneGrid
      hidden={hidden}
      panes={panes}
      tree={tree}
      onTreeChange={(next) => setTree(workspace.id, next)}
      onMove={(dragId, targetId, zone) =>
        movePane(workspace.id, dragId, targetId, zone)
      }
      renderPane={(pane) => (
        <TerminalPane
          pane={pane}
          workspaceId={workspace.id}
          index={Math.max(0, order.indexOf(pane.id))}
          cwd={workspace.cwd}
          extraRoots={workspace.extraRoots}
          settings={settings}
          theme={theme}
          focused={focusedPaneId === pane.id}
          // Focus is per workspace, so a pane can be the focused one of a
          // workspace nobody is looking at. Each pane is told whether its
          // grid is on screen, which is what decides who takes the keyboard
          // and whose hand-back is worth a notification.
          visible={!hidden}
          availableAgents={availableAgents}
          shells={shells}
          shell={resolveShell(
            shells,
            pane.shellId,
            workspace.shellId,
            settings.shellId,
          )}
          canClose={panes.length > 1}
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
        />
      )}
    />
  );
}
