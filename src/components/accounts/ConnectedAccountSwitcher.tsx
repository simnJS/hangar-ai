import { useMemo } from "react";
import { resolveAccount } from "../../lib/accounts";
import { runningCount, type SwitchPane } from "../../lib/accountSwitch";
import { AccountSwitcher } from "./AccountSwitcher";
import { useAccounts } from "./useAccounts";
import { useAccountSwitching } from "./useAccountSwitching";
import { useAccountUsage } from "./usageStore";

/**
 * The toolbar's account switcher, fed from the store: the account of the
 * workspace on screen, usage from the bridge, and the two scopes of a click
 * (this workspace — the main action — or the default for all).
 */
export function ConnectedAccountSwitcher({ workspaceId }: { workspaceId: string }) {
  const { accounts } = useAccounts();
  const usage = useAccountUsage();
  const switching = useAccountSwitching();
  const { state } = switching;
  const workspace = state.workspaces.find((w) => w.id === workspaceId);

  const resolved = useMemo(
    () => (accounts && workspace ? resolveAccount(accounts, state.settings, workspace) : null),
    [accounts, state.settings, workspace],
  );
  if (!accounts || !workspace || !resolved) return null;
  const defaultId = resolveAccount(accounts, state.settings).id;
  const count = (panes: SwitchPane[]) => runningCount(panes);

  return (
    <AccountSwitcher
      accounts={accounts}
      currentId={resolved.id}
      defaultId={defaultId}
      usage={usage}
      runningHere={count(switching.runningInWorkspace(workspace))}
      runningDefault={count(switching.runningOnDefault())}
      onSelect={(id, scope, restart) =>
        scope === "workspace"
          ? switching.setWorkspaceAccount(workspace, id, restart)
          : switching.setDefaultAccount(id, restart)
      }
    />
  );
}
