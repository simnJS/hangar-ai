import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ACCOUNTS_CHANGED,
  accountsList,
  accountsSupport,
  type AccountsSupport,
  type ClaudeAccount,
} from "../../lib/accounts";

/**
 * The accounts as the interface sees them: read once, then again on every
 * `accounts:changed`. `accounts` is null until the backend has answered (and
 * stays null outside the desktop shell, where nothing answers).
 */
export function useAccounts(): {
  accounts: ClaudeAccount[] | null;
  support: AccountsSupport | null;
  refresh: () => Promise<void>;
} {
  const [accounts, setAccounts] = useState<ClaudeAccount[] | null>(null);
  const [support, setSupport] = useState<AccountsSupport | null>(null);

  const refresh = useCallback(async () => {
    try {
      setAccounts(await accountsList());
    } catch {
      // No backend (or it failed): the interface simply has no accounts to show.
    }
  }, []);

  useEffect(() => {
    void refresh();
    accountsSupport()
      .then(setSupport)
      .catch(() => undefined);
    const stop = listen(ACCOUNTS_CHANGED, () => void refresh()).catch(() => null);
    return () => void stop.then((off) => off?.());
  }, [refresh]);

  return { accounts, support, refresh };
}
