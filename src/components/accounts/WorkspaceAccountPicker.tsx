import { defaultLabel } from "../../lib/accountSwitch";
import { readAccountChoice, type AccountChoice, type ClaudeAccount } from "../../lib/accounts";
import { useAccountsT } from "./strings";
import "./accounts.css";

/**
 * The account choice of the workspace dialog: « Default (Perso) » or one of the
 * accounts. Shows the account's label only, never its email.
 *
 * Props:
 * - `accounts`: every account.
 * - `settings`: the global choice (`claudeAccount`), to name the default.
 * - `value`: the workspace's own account id, null/absent to follow the default.
 * - `onChange(id | null)`: the person picked an account, or null for the default.
 * - `id`: the select's DOM id, so the dialog's label can point to it.
 * - `disabled`.
 *
 * When `value` names an account that no longer exists, a plain note says the
 * default applies; picking anything replaces the dead id.
 */
export function WorkspaceAccountPicker({
  accounts,
  settings,
  value,
  onChange,
  id,
  disabled,
}: {
  accounts: ClaudeAccount[];
  settings?: AccountChoice;
  value?: string | null;
  onChange: (id: string | null) => void;
  id?: string;
  disabled?: boolean;
}) {
  const t = useAccountsT();
  const chosen = readAccountChoice(value);
  const missing = chosen !== null && !accounts.some((a) => a.id === chosen) ? chosen : null;
  return (
    <div className="acc-picker">
      <select
        id={id}
        className="acc-select"
        aria-label={id ? undefined : t("picker.label")}
        disabled={disabled}
        value={missing === null ? (chosen ?? "") : ""}
        onChange={(event) => onChange(event.target.value === "" ? null : event.target.value)}
      >
        <option value="">
          {defaultLabel(accounts, settings, t("picker.default"), t("picker.unknown"))}
        </option>
        {accounts.map((account) => (
          <option key={account.id} value={account.id}>
            {account.label}
          </option>
        ))}
      </select>
      {missing !== null && (
        <p className="acc-note" role="status">
          {t("picker.missing", { id: missing })}
        </p>
      )}
    </div>
  );
}
