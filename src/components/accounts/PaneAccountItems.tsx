import "./accounts.css";
import { useAccountSwitching } from "./useAccountSwitching";
import { useAccounts } from "./useAccounts";
import { useAccountsT } from "./strings";
import { accountMenuItems } from "./accountMenuItems";
import { Icon } from "../Icon";

/**
 * The account choice at the end of a pane's "…" menu: « Default » (follow the
 * workspace) or one account for this pane alone. A running agent restarts on
 * the new account with its conversation. Nothing for a pane that does not run
 * Claude Code.
 */
export function PaneAccountItems({
  workspaceId,
  paneId,
  onDone,
}: {
  workspaceId: string;
  paneId: string;
  onDone: () => void;
}) {
  const t = useAccountsT();
  const { accounts } = useAccounts();
  const { state, setPaneAccount } = useAccountSwitching();
  const workspace = state.workspaces.find((w) => w.id === workspaceId);
  const pane = workspace?.panes.find((p) => p.id === paneId);
  if (!accounts || !workspace || !pane || pane.agent !== "claude" || pane.login) return null;

  const items = accountMenuItems({
    accounts,
    // « Default » here means the pane follows its workspace, so the label names
    // the account the workspace is on.
    settings: { claudeAccount: workspace.claudeAccount ?? state.settings.claudeAccount },
    value: pane.claudeAccount,
    onSelect: (id) => {
      setPaneAccount(workspaceId, paneId, id);
      onDone();
    },
    t,
  });

  return (
    <div role="group" aria-label={t("pane.menuHeading")}>
      <div className="pane-menu__sep" role="separator" />
      <p className="pane-menu__heading">{t("pane.menuHeading")}</p>
      {items.map((item) => (
        <div key={item.key} className="pane-menu__row" role="none">
          <button
            className="pane-menu__item"
            role="menuitemradio"
            aria-checked={item.checked}
            tabIndex={-1}
            onClick={item.onSelect}
            onMouseEnter={(event) => event.currentTarget.focus()}
          >
            {item.checked ? <Icon name="check" size={14} /> : <span className="acc-menu__check" />}
            <span className="pane-menu__label">{item.label}</span>
          </button>
        </div>
      ))}
    </div>
  );
}
