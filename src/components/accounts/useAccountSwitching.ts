import { useCallback, useSyncExternalStore } from "react";
import { launchedAccount, readAccountChoice, subscribeLaunchedAccounts, type AccountId } from "../../lib/accounts";
import {
  panesToRestartOnDefaultChange,
  panesToRestartOnWorkspaceChange,
  type SwitchPane,
  type SwitchWorkspace,
} from "../../lib/accountSwitch";
import { useStore } from "../../store";
import type { Workspace } from "../../types";

/** A workspace as the switching logic reads it; `running` = a Claude Code is up in the pane. */
export function toSwitchWorkspace(workspace: Workspace): SwitchWorkspace {
  return {
    id: workspace.id,
    claudeAccount: workspace.claudeAccount,
    panes: workspace.panes.map(
      (pane): SwitchPane => ({
        id: pane.id,
        // A pane opened to log in runs no agent: nothing to restart.
        agent: pane.login ? "login" : pane.agent,
        claudeAccount: pane.claudeAccount,
        running: launchedAccount(pane.id) !== null,
      }),
    ),
  };
}

/**
 * What the interface does with a choice of account: set it on a workspace, on
 * the defaults, or on one pane, and restart the running agents it concerns
 * with their conversation (a respawn with no patch keeps the session id —
 * the path the Kits activation uses). Always the person's call.
 */
export function useAccountSwitching() {
  const { state, updateWorkspace, updateSettings, updatePane, respawnPane } = useStore();
  // The panes that run an agent change without the store knowing.
  useSyncExternalStore(subscribeLaunchedAccounts, () => JSON.stringify(state.workspaces.map((w) => w.panes.map((p) => launchedAccount(p.id)))));

  const running = useCallback(
    (panes: SwitchPane[]) => panes.filter((pane) => pane.running),
    [],
  );

  /** Restarting the running ones only: a stopped pane has no conversation to resume. */
  const restart = useCallback(
    (workspaceId: string, panes: SwitchPane[]) => {
      for (const pane of running(panes)) respawnPane(workspaceId, pane.id, {});
    },
    [respawnPane, running],
  );

  const runningInWorkspace = (workspace: Workspace) =>
    running(panesToRestartOnWorkspaceChange(toSwitchWorkspace(workspace)));
  const runningOnDefault = () =>
    running(panesToRestartOnDefaultChange(state.workspaces.map(toSwitchWorkspace)));

  function setWorkspaceAccount(workspace: Workspace, id: AccountId | null, restartNow: boolean) {
    // Which panes follow is decided before the change; ids survive it, and the
    // respawn re-resolves the account at launch.
    const panes = panesToRestartOnWorkspaceChange(toSwitchWorkspace(workspace));
    updateWorkspace(workspace.id, { claudeAccount: id });
    if (restartNow) restart(workspace.id, panes);
  }

  function setDefaultAccount(id: AccountId, restartNow: boolean) {
    const workspaces = state.workspaces.map(toSwitchWorkspace);
    const affected = workspaces.filter((w) => readAccountChoice(w.claudeAccount) === null);
    updateSettings({ claudeAccount: id });
    if (!restartNow) return;
    for (const workspace of affected) restart(workspace.id, panesToRestartOnWorkspaceChange(workspace));
  }

  /** The person's pick in the pane's menu: a running agent restarts on it, conversation kept. */
  function setPaneAccount(workspaceId: string, paneId: string, id: AccountId | null) {
    const isRunning = launchedAccount(paneId) !== null;
    updatePane(workspaceId, paneId, { claudeAccount: id });
    if (isRunning) respawnPane(workspaceId, paneId, {});
  }

  return {
    state,
    runningInWorkspace,
    runningOnDefault,
    setWorkspaceAccount,
    setDefaultAccount,
    setPaneAccount,
  };
}
