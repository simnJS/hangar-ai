import { defaultLabel } from "../../lib/accountSwitch";
import { readAccountChoice, type AccountChoice, type ClaudeAccount } from "../../lib/accounts";
import type { AccountsT } from "./strings";

/**
 * The entries of the sidebar's right-click menu that choose a workspace's
 * Claude account: « Default (Perso) » first, then one entry per account, the
 * workspace's current choice checked. Plain data, so the host menu renders it
 * the way it renders its own items.
 *
 * - `accounts`: every account; `settings`: the global choice, to name the default.
 * - `value`: the workspace's own account id, null/absent when it follows the default.
 * - `onSelect(id | null)`: null means « follow the default ».
 * - `t`: from `useAccountsT()`.
 *
 * Labels only, never emails.
 */
export interface AccountMenuItem {
  key: string;
  label: string;
  checked: boolean;
  onSelect: () => void;
}

export function accountMenuItems({
  accounts,
  settings,
  value,
  onSelect,
  t,
}: {
  accounts: ClaudeAccount[];
  settings?: AccountChoice;
  value?: string | null;
  onSelect: (id: string | null) => void;
  t: AccountsT;
}): AccountMenuItem[] {
  const own = readAccountChoice(value);
  const knownOwn = own !== null && accounts.some((a) => a.id === own);
  return [
    {
      key: "default",
      label: defaultLabel(accounts, settings, t("picker.default"), t("picker.unknown")),
      checked: !knownOwn,
      onSelect: () => onSelect(null),
    },
    ...accounts.map((account) => ({
      key: account.id,
      label: account.label,
      checked: knownOwn && own === account.id,
      onSelect: () => onSelect(account.id),
    })),
  ];
}
