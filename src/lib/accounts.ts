import { invoke } from "@tauri-apps/api/core";

/**
 * « Comptes Claude »: several Claude subscriptions in Hangar, one profile per
 * account through Claude Code's CLAUDE_CONFIG_DIR. The account is chosen per
 * workspace first, with a global default and a per-pane override. Switching is
 * always the person's click, never an automatic rotation.
 *
 * This is the front contract: the Rust side implements these commands as
 * written. The pure parts (reading a choice, resolving the account) are tested.
 */

/** `"main"` is the default Claude Code config: always present, never removed. */
export type AccountId = string;

export const MAIN_ACCOUNT: AccountId = "main";

export interface ClaudeAccount {
  id: AccountId;
  label: string;
  isMain: boolean;
  loggedIn: boolean;
  email?: string;
  plan?: string;
}

export interface AccountAdded {
  account: ClaudeAccount;
  /** The command a visible pane runs so the person logs in in the browser. */
  login: { command: string };
}

export interface AccountsSupport {
  /** False on a macOS whose Keychain cannot separate logins. */
  isolatedLogins: boolean;
  note?: string;
}

/** Emitted by the backend after every change to the list of accounts. */
export const ACCOUNTS_CHANGED = "accounts:changed";

export const accountsList = () => invoke<ClaudeAccount[]>("accounts_list");
export const accountAdd = (label: string) => invoke<AccountAdded>("account_add", { label });
export const accountRename = (id: AccountId, label: string) =>
  invoke<ClaudeAccount>("account_rename", { id, label });
export const accountRemove = (id: AccountId) => invoke<void>("account_remove", { id });
export const accountsSupport = () => invoke<AccountsSupport>("accounts_support");

/** The field the Settings, the Workspace and the Pane each carry. */
export interface AccountChoice {
  claudeAccount?: string | null;
}

/**
 * state.json is hand-edited: anything that is not a non-empty string is no
 * choice at all.
 */
export function readAccountChoice(x: unknown): string | null {
  if (typeof x !== "string") return null;
  const id = x.trim();
  return id === "" ? null : id;
}

export type AccountSource = "pane" | "workspace" | "default" | "main";

export interface ResolvedAccount {
  id: AccountId;
  source: AccountSource;
  /** The id that was asked for but does not exist (any more); the fallback applies. */
  missing?: string;
}

/**
 * Pane > workspace > settings > main. A choice naming an unknown account is
 * skipped for the next level, and reported in `missing` (the first one met).
 */
export function resolveAccount(
  accounts: ClaudeAccount[],
  settings?: AccountChoice | null,
  workspace?: AccountChoice | null,
  pane?: AccountChoice | null,
): ResolvedAccount {
  const levels: [AccountSource, unknown][] = [
    ["pane", pane?.claudeAccount],
    ["workspace", workspace?.claudeAccount],
    ["default", settings?.claudeAccount],
  ];
  let missing: string | undefined;
  for (const [source, raw] of levels) {
    const id = readAccountChoice(raw);
    if (id === null) continue;
    if (accounts.some((a) => a.id === id)) {
      return missing ? { id, source, missing } : { id, source };
    }
    missing ??= id;
  }
  return missing ? { id: MAIN_ACCOUNT, source: "main", missing } : { id: MAIN_ACCOUNT, source: "main" };
}
