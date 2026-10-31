import { resolveAccount } from "../../lib/accounts";
import { useStore } from "../../store";
import { AccountBadge } from "./AccountBadge";
import { useAccounts } from "./useAccounts";

/**
 * For a pane header: names the account of a pane whose agent runs on another
 * one than its workspace's (a pane-level override). Nothing otherwise. Label
 * only, never the email.
 */
export function PaneAccountBadge({ workspaceId, paneId }: { workspaceId: string; paneId: string }) {
  const { accounts } = useAccounts();
  const { state } = useStore();
  const workspace = state.workspaces.find((w) => w.id === workspaceId);
  const pane = workspace?.panes.find((p) => p.id === paneId);
  if (!accounts || !workspace || !pane || pane.agent !== "claude") return null;
  const own = resolveAccount(accounts, state.settings, workspace, pane);
  if (own.source !== "pane") return null;
  const base = resolveAccount(accounts, state.settings, workspace);
  if (own.id === base.id) return null;
  return <AccountBadge accounts={accounts} id={own.id} />;
}
