import { accountName } from "../../lib/accountSwitch";
import type { AccountId, ClaudeAccount } from "../../lib/accounts";
import { useAccountsT } from "./strings";
import "./accounts.css";

/**
 * A discreet label naming the Claude account a workspace or a pane runs on,
 * for the sidebar and the pane headers. It shows the account's label, never
 * its email.
 *
 * Props:
 * - `accounts`: every account (the label is looked up here).
 * - `id`: the account in force (already resolved: pane > workspace > default).
 * - `className`: lets the host place it.
 *
 * With a single account there is nothing to tell apart, so nothing is drawn.
 * No callback: it is a label, not a control.
 */
export function AccountBadge({
  accounts,
  id,
  className,
}: {
  accounts: ClaudeAccount[];
  id: AccountId;
  className?: string;
}) {
  const t = useAccountsT();
  if (accounts.length < 2) return null;
  const name = accountName(accounts, id, t("picker.unknown"));
  return (
    <span className={["acc-badge", className].filter(Boolean).join(" ")} title={t("badge.title", { name })}>
      {name}
    </span>
  );
}
