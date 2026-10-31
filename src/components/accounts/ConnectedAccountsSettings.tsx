import { useState } from "react";
import { accountAdd, accountRemove, accountRename, MAIN_ACCOUNT, readAccountChoice } from "../../lib/accounts";
import { useStore } from "../../store";
import { AccountsSettingsPage } from "./AccountsSettingsPage";
import { useAccountsT } from "./strings";
import { useAccounts } from "./useAccounts";
import { namesAccount } from "../../lib/accountSwitch";

/**
 * Settings › Comptes, fed from the store and the Tauri commands. The login
 * itself is the person's, in the browser: a visible Claude Code pane opens on
 * the profile and runs the login there.
 */
export function ConnectedAccountsSettings() {
  const t = useAccountsT();
  const store = useStore();
  const { state, activeWorkspace, addPane, updatePane, updateWorkspace, updateSettings } = store;
  const { accounts, support, refresh } = useAccounts();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!accounts) return null;

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (err) {
      setError(t("settings.failed", { detail: err instanceof Error ? err.message : String(err) }));
      throw err;
    } finally {
      setBusy(false);
    }
  }

  function openLoginPane(id: string) {
    if (!activeWorkspace) return setError(t("settings.noPane"));
    const paneId = addPane(activeWorkspace.id, { agent: "claude" });
    if (!paneId) return setError(t("settings.noPane"));
    updatePane(activeWorkspace.id, paneId, { claudeAccount: id, login: true });
  }

  return (
    <AccountsSettingsPage
      accounts={accounts}
      settings={state.settings}
      workspaces={state.workspaces.map((w) => ({
        id: w.id,
        name: w.name,
        claudeAccount: w.claudeAccount,
        panes: [],
      }))}
      support={support}
      busy={busy}
      error={error}
      onAdd={(label) =>
        run(async () => {
          const added = await accountAdd(label);
          openLoginPane(added.account.id);
        })
      }
      onLogin={openLoginPane}
      onRename={(id, label) => run(async () => void (await accountRename(id, label)))}
      onRemove={(id) =>
        run(async () => {
          if (id === MAIN_ACCOUNT) return;
          await accountRemove(id);
          // Whatever named it falls back to the level above; say so in the file too.
          for (const w of state.workspaces) {
            if (namesAccount(w, id)) updateWorkspace(w.id, { claudeAccount: null });
            for (const p of w.panes) {
              if (namesAccount(p, id)) updatePane(w.id, p.id, { claudeAccount: null });
            }
          }
          if (readAccountChoice(state.settings.claudeAccount) === id) {
            updateSettings({ claudeAccount: MAIN_ACCOUNT });
          }
        })
      }
      onSetDefault={(id) => updateSettings({ claudeAccount: id })}
    />
  );
}
