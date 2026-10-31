import { useRef, useState } from "react";
import { accountName, usagePercent, type AccountUsage } from "../../lib/accountSwitch";
import type { AccountId, ClaudeAccount } from "../../lib/accounts";
import { Icon } from "../Icon";
import { ToolbarMenu, type MenuCloseReason } from "../ToolbarMenu";
import { useAccountsT, type AccountsT } from "./strings";
import "./accounts.css";

/**
 * The toolbar button of « Comptes Claude »: it shows the account of the
 * workspace on screen, and its menu switches it in one click.
 *
 * Props:
 * - `accounts`: every account (the email of the one on screen is in the
 *   tooltip of its row; it appears nowhere else in this component).
 * - `currentId`: the account in force in the workspace on screen.
 * - `defaultId`: the global default, marked in the list.
 * - `usage`: 5-hour and weekly usage by account (`usageByAccount`); an account
 *   or a window without a measure reads « unknown ».
 * - `runningHere`: agents running in this workspace that a switch would
 *   restart; `runningDefault`: the same for every workspace following the default.
 * - `onSelect(id, scope, restart)`: the person's choice. `scope` is
 *   `"workspace"` (the main action, one click on an account) or `"default"`
 *   (the secondary « For every workspace » entry). `restart` is true for
 *   « Restart … now (conversations kept) », false for « On next launch » — and
 *   false when nothing is running, in which case no question is asked.
 *
 * Switching is always this click: nothing here rotates accounts by itself.
 */
export type SwitchScope = "workspace" | "default";

export interface AccountSwitcherProps {
  accounts: ClaudeAccount[];
  currentId: AccountId;
  defaultId: AccountId;
  usage: Record<AccountId, AccountUsage>;
  runningHere: number;
  runningDefault: number;
  onSelect: (id: AccountId, scope: SwitchScope, restart: boolean) => void;
  disabled?: boolean;
}

function usageLine(t: AccountsT, usage: AccountUsage | undefined): string {
  const show = (value: ReturnType<typeof usagePercent>) =>
    value === null ? t("switcher.unknown") : t("switcher.percent", { n: value });
  return t("switcher.usage", {
    five: t("switcher.five"),
    fiveValue: show(usagePercent(usage?.fiveHour)),
    week: t("switcher.week"),
    weekValue: show(usagePercent(usage?.weekly)),
  });
}

export function AccountSwitcher({
  accounts,
  currentId,
  defaultId,
  usage,
  runningHere,
  runningDefault,
  onSelect,
  disabled,
}: AccountSwitcherProps) {
  const t = useAccountsT();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<{ id: AccountId; scope: SwitchScope } | null>(null);

  const unknown = t("picker.unknown");
  const currentName = accountName(accounts, currentId, unknown);

  function close(reason?: MenuCloseReason) {
    setOpen(false);
    setPending(null);
    // Back to the button for Escape and Tab; wherever the person clicked otherwise.
    if (reason === "escape" || reason === "tab") buttonRef.current?.focus();
  }

  function choose(id: AccountId, scope: SwitchScope) {
    const running = scope === "workspace" ? runningHere : runningDefault;
    if (running === 0) {
      onSelect(id, scope, false);
      close("escape");
      return;
    }
    setPending({ id, scope });
  }

  function decide(restart: boolean) {
    if (!pending) return;
    onSelect(pending.id, pending.scope, restart);
    close("escape");
  }

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className={`acc-switch ${open ? "is-open" : ""}`}
        aria-haspopup="menu"
        aria-expanded={open}
        title={t("switcher.button", { name: currentName })}
        aria-label={t("switcher.button", { name: currentName })}
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
      >
        <Icon name="bolt" size={14} />
        <span className="acc-switch__label">{currentName}</span>
        <Icon name="chevronDown" size={12} />
      </button>
      {open && !pending && (
        <ToolbarMenu anchorRef={buttonRef} label={t("switcher.menu")} className="acc-menu" onClose={close}>
          <p className="acc-menu__heading">{t("switcher.heading")}</p>
          {accounts.map((account) => {
            const isCurrent = account.id === currentId;
            return (
              <div className="acc-menu__row" key={account.id}>
                <button
                  type="button"
                  role="menuitem"
                  className="tmenu__item acc-menu__main"
                  aria-current={isCurrent}
                  title={account.email}
                  onClick={() => choose(account.id, "workspace")}
                >
                  <span className="acc-menu__check">{isCurrent && <Icon name="check" size={14} />}</span>
                  <span className="acc-menu__text">
                    <span className="tmenu__label">
                      {account.label}
                      {!account.loggedIn && (
                        <span className="acc-menu__warn"> · {t("switcher.notLoggedIn")}</span>
                      )}
                    </span>
                    <span className="acc-menu__usage">{usageLine(t, usage[account.id])}</span>
                  </span>
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="tmenu__item acc-menu__all"
                  aria-label={t("switcher.useEverywhereLabel", { name: account.label })}
                  aria-current={account.id === defaultId}
                  title={t("switcher.useEverywhereLabel", { name: account.label })}
                  onClick={() => choose(account.id, "default")}
                >
                  {t("switcher.useEverywhere")}
                </button>
              </div>
            );
          })}
        </ToolbarMenu>
      )}
      {open && pending && (
        <ToolbarMenu
          anchorRef={buttonRef}
          role="alertdialog"
          labelledBy="acc-ask-title"
          describedBy="acc-ask-body"
          onClose={close}
        >
          <p className="tmenu__title" id="acc-ask-title">
            {pending.scope === "workspace"
              ? t("switcher.askHere", { name: accountName(accounts, pending.id, unknown) })
              : t("switcher.askEverywhere", { name: accountName(accounts, pending.id, unknown) })}
          </p>
          <p className="tmenu__body" id="acc-ask-body">
            {t("switcher.running", { n: pending.scope === "workspace" ? runningHere : runningDefault })}
          </p>
          <div className="acc-ask__choices">
            <button type="button" className="btn btn--primary" autoFocus onClick={() => decide(true)}>
              {pending.scope === "workspace" ? t("switcher.restartHere") : t("switcher.restartEverywhere")}
            </button>
            <button type="button" className="btn" onClick={() => decide(false)}>
              {t("switcher.nextLaunch")}
            </button>
            <button type="button" className="btn btn--ghost" onClick={() => setPending(null)}>
              {t("switcher.back")}
            </button>
          </div>
        </ToolbarMenu>
      )}
    </>
  );
}
