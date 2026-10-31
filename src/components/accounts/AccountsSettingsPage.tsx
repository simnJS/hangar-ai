import { useState } from "react";
import type { SwitchWorkspace } from "../../lib/accountSwitch";
import {
  resolveAccount,
  type AccountChoice,
  type AccountId,
  type AccountsSupport,
  type ClaudeAccount,
} from "../../lib/accounts";
import { Icon } from "../Icon";
import { useAccountsT, type AccountsT } from "./strings";
import "./accounts.css";

/**
 * The « Comptes Claude » page of the settings: the list of accounts and what
 * can be done with each. Props only: it never reads the store and never calls
 * the Tauri commands itself.
 *
 * Props:
 * - `accounts`: every account, main included. Each row shows its label, its
 *   email and plan when known, whether it is logged in, whether it is the
 *   default, and the workspaces that use it.
 * - `settings`: the global choice (`claudeAccount`), to know the default.
 * - `workspaces`: `{ id, name, claudeAccount?, panes }` — only the id, name and
 *   own account are read here, to say which workspaces use which account
 *   (one that follows the default counts for the default account).
 * - `support`: `accountsSupport()`'s answer, null until known. When
 *   `isolatedLogins` is false a plain warning is shown (with its `note`).
 * - `onAdd(label)`: add an account; the host then opens the login pane. May
 *   return a promise: the form waits for it and keeps the name if it rejects.
 * - `onRename(id, label)`, `onRemove(id)` (asked twice, never for main),
 *   `onSetDefault(id)`: the matching commands.
 * - `onLogin(id)`: optional; when given, a logged-out account offers « Log in ».
 * - `busy`: disables the controls while a command runs; `error`: the last
 *   failure, shown as text.
 */
export interface AccountsSettingsPageProps {
  accounts: ClaudeAccount[];
  settings: AccountChoice;
  workspaces: (SwitchWorkspace & { name: string })[];
  support: AccountsSupport | null;
  onAdd: (label: string) => void | Promise<void>;
  onRename: (id: AccountId, label: string) => void | Promise<void>;
  onRemove: (id: AccountId) => void | Promise<void>;
  onSetDefault: (id: AccountId) => void | Promise<void>;
  onLogin?: (id: AccountId) => void;
  busy?: boolean;
  error?: string | null;
}

/** Why `label` cannot name an account, or null. `except` is the one being renamed. */
export function labelProblem(
  accounts: ClaudeAccount[],
  label: string,
  except?: AccountId,
): "nameEmpty" | "nameTaken" | null {
  const wanted = label.trim().toLocaleLowerCase();
  if (wanted === "") return "nameEmpty";
  const taken = accounts.some((a) => a.id !== except && a.label.trim().toLocaleLowerCase() === wanted);
  return taken ? "nameTaken" : null;
}

export function AccountsSettingsPage(props: AccountsSettingsPageProps) {
  const { accounts, settings, workspaces, support, busy, error } = props;
  const t = useAccountsT();
  const [renaming, setRenaming] = useState<{ id: AccountId; value: string } | null>(null);
  const [removing, setRemoving] = useState<AccountId | null>(null);
  const [name, setName] = useState("");
  const [touched, setTouched] = useState(false);

  const defaultId = resolveAccount(accounts, settings).id;

  /** The workspaces on this account, whether chosen or by following the default. */
  function usersOf(account: ClaudeAccount): string[] {
    return workspaces.filter((w) => resolveAccount(accounts, settings, w).id === account.id).map((w) => w.name);
  }

  const addProblem = labelProblem(accounts, name);

  async function submitAdd() {
    setTouched(true);
    if (addProblem) return;
    try {
      await props.onAdd(name.trim());
      setName("");
      setTouched(false);
    } catch {
      // The host reports the failure through `error`; the name stays to retry.
    }
  }

  async function submitRename() {
    if (!renaming || labelProblem(accounts, renaming.value, renaming.id)) return;
    try {
      await props.onRename(renaming.id, renaming.value.trim());
      setRenaming(null);
    } catch {
      // Same: `error` tells, the field stays open.
    }
  }

  return (
    <section className="set-section acc" aria-labelledby="acc-title">
      <header className="set-section__head">
        <h3 className="set-section__title" id="acc-title">
          {t("settings.title")}
        </h3>
        <p className="set-section__hint">{t("settings.intro")}</p>
      </header>

      {support && !support.isolatedLogins && (
        <div className="acc-warning" role="note">
          <Icon name="alert" size={14} />
          <div>
            <strong>{t("settings.unsupportedTitle")}</strong>
            <p>{t("settings.unsupportedBody")}</p>
            {support.note && <p>{support.note}</p>}
          </div>
        </div>
      )}

      {error && (
        <p className="acc-error" role="alert">
          {error}
        </p>
      )}

      <ul className="set-group acc-list" aria-label={t("settings.listLabel")}>
        {accounts.map((account) => {
          const isDefault = account.id === defaultId;
          const users = usersOf(account);
          const isRenaming = renaming?.id === account.id;
          const renameProblem = isRenaming ? labelProblem(accounts, renaming.value, account.id) : null;
          return (
            <li className="set-row set-row--block acc-row" key={account.id}>
              <div className="acc-row__head">
                <span className="acc-row__name">{account.label}</span>
                {isDefault && <span className="acc-tag">{t("settings.default")}</span>}
                <span className={`acc-tag ${account.loggedIn ? "acc-tag--ok" : "acc-tag--off"}`}>
                  {account.loggedIn ? t("settings.loggedIn") : t("settings.loggedOut")}
                </span>
              </div>
              {(account.email || account.plan) && (
                <p className="acc-row__meta">{[account.email, account.plan].filter(Boolean).join(" · ")}</p>
              )}
              <p className="acc-row__meta">
                {users.length ? t("settings.usedBy", { names: users.join(", ") }) : t("settings.usedByNone")}
              </p>

              {isRenaming ? (
                <form
                  className="acc-form"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void submitRename();
                  }}
                >
                  <input
                    className="acc-input"
                    autoFocus
                    value={renaming.value}
                    aria-label={t("settings.renameLabel", { name: account.label })}
                    aria-invalid={renameProblem !== null}
                    onChange={(event) => setRenaming({ id: account.id, value: event.target.value })}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") {
                        event.stopPropagation();
                        setRenaming(null);
                      }
                    }}
                  />
                  <button type="submit" className="btn btn--primary" disabled={busy || renameProblem !== null}>
                    {t("settings.save")}
                  </button>
                  <button type="button" className="btn btn--ghost" onClick={() => setRenaming(null)}>
                    {t("settings.cancel")}
                  </button>
                  {renameProblem && <p className="acc-note acc-note--bad">{problemText(t, renameProblem)}</p>}
                </form>
              ) : removing === account.id ? (
                <div className="acc-confirm" role="alertdialog" aria-labelledby={`acc-rm-${account.id}`}>
                  <p className="acc-confirm__title" id={`acc-rm-${account.id}`}>
                    {t("settings.removeAsk", { name: account.label })}
                  </p>
                  <p className="acc-note">{t("settings.removeBody")}</p>
                  <div className="acc-actions">
                    <button
                      type="button"
                      className="btn btn--danger"
                      autoFocus
                      disabled={busy}
                      onClick={async () => {
                        try {
                          await props.onRemove(account.id);
                          setRemoving(null);
                        } catch {
                          // `error` tells; the question stays.
                        }
                      }}
                    >
                      {t("settings.removeConfirm")}
                    </button>
                    <button type="button" className="btn btn--ghost" onClick={() => setRemoving(null)}>
                      {t("settings.cancel")}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="acc-actions">
                  {!account.loggedIn && props.onLogin && (
                    <button type="button" className="btn btn--primary" disabled={busy} onClick={() => props.onLogin?.(account.id)}>
                      {t("settings.logIn")}
                    </button>
                  )}
                  {!isDefault && (
                    <button type="button" className="btn" disabled={busy} onClick={() => void props.onSetDefault(account.id)}>
                      {t("settings.setDefault")}
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn btn--ghost"
                    disabled={busy}
                    onClick={() => setRenaming({ id: account.id, value: account.label })}
                  >
                    {t("settings.rename")}
                  </button>
                  {account.isMain ? (
                    <span className="acc-note">{t("settings.mainFixed")}</span>
                  ) : (
                    <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => setRemoving(account.id)}>
                      {t("settings.remove")}
                    </button>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>

      <form
        className="set-group acc-add"
        onSubmit={(event) => {
          event.preventDefault();
          void submitAdd();
        }}
      >
        <div className="set-row set-row--block">
          <label className="acc-add__title" htmlFor="acc-add-name">
            {t("settings.addTitle")}
          </label>
          <div className="acc-form">
            <input
              id="acc-add-name"
              className="acc-input"
              value={name}
              placeholder={t("settings.addPlaceholder")}
              aria-label={t("settings.addName")}
              aria-invalid={touched && addProblem !== null}
              onChange={(event) => setName(event.target.value)}
            />
            <button type="submit" className="btn btn--primary" disabled={busy}>
              {t("settings.add")}
            </button>
          </div>
          {touched && addProblem ? (
            <p className="acc-note acc-note--bad" role="alert">
              {problemText(t, addProblem)}
            </p>
          ) : (
            <p className="acc-note">{t("settings.addHint")}</p>
          )}
        </div>
      </form>
    </section>
  );
}

function problemText(t: AccountsT, problem: "nameEmpty" | "nameTaken"): string {
  return problem === "nameEmpty" ? t("settings.nameEmpty") : t("settings.nameTaken");
}
