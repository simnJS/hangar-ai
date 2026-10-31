import { invoke } from "@tauri-apps/api/core";

/**
 * « Comptes Claude »: several Claude subscriptions in Hangar, one profile per
 * account through Claude Code's CLAUDE_CONFIG_DIR. The account is chosen per
 * workspace first, with a global default and a per-pane override. Switching is
 * always the person's click, never an automatic rotation.
 *
 * This is the front contract: the Rust side (src-tauri/src/accounts.rs, whose
 * module doc says how a profile shares the main account's setup and keeps its
 * own login) implements these commands as written. The pure parts (reading a
 * choice, resolving the account, the accounts panes were launched on) are
 * tested.
 *
 * Only Claude Code panes run on an account: TerminalPane resolves it at each
 * launch (`launchAccount`) and hands it to `ptySpawn`, which sets
 * CLAUDE_CONFIG_DIR for a profile. Workers follow their workspace, like any
 * pane without an account of its own.
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
  /**
   * The command a visible pane runs so the person logs in in the browser:
   * `LOGIN_COMMAND`. A pane `{ agent: "claude", claudeAccount: <id>, login:
   * true }` runs it on the account's profile (TerminalPane).
   */
  login: { command: string };
}

/**
 * Logs the profile a pane runs on in, in the browser (Claude Code 2.1.316;
 * `claude` then `/login` does the same). Typed by a `Pane.login` pane.
 */
export const LOGIN_COMMAND = "claude auth login";

/** Something an account's profile could not take from the main account. */
export interface AccountIssue {
  /** The folder or file: `projects`, `settings.json`, `.claude.json`… */
  item: string;
  reason: string;
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
 * Main is always known, listed or not: it is never removed.
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
    if (id === MAIN_ACCOUNT || accounts.some((a) => a.id === id)) {
      return missing ? { id, source, missing } : { id, source };
    }
    missing ??= id;
  }
  return missing ? { id: MAIN_ACCOUNT, source: "main", missing } : { id: MAIN_ACCOUNT, source: "main" };
}

/**
 * The account a Claude Code pane starts on, as `resolveAccount` decides —
 * asking the backend for the accounts only when some level names one other
 * than main, so a machine with a single account never asks. When the list
 * cannot be had, every other account counts as gone: the pane starts on
 * main, and `missing` says which was asked for.
 */
export async function launchAccount(
  settings: AccountChoice | null | undefined,
  workspace: AccountChoice | null | undefined,
  pane: AccountChoice | null | undefined,
  list: () => Promise<ClaudeAccount[]> = accountsList,
): Promise<ResolvedAccount> {
  const named = [pane, workspace, settings].some((level) => {
    const id = readAccountChoice(level?.claudeAccount);
    return id !== null && id !== MAIN_ACCOUNT;
  });
  const accounts = named ? await list().catch(() => [] as ClaudeAccount[]) : [];
  return resolveAccount(accounts, settings, workspace, pane);
}

/**
 * The account each running Claude Code pane was launched on, by pane id. A
 * pane keeps the account it started on until it restarts, whatever is chosen
 * meanwhile: this is what its usage counts against (lib/accountSwitch
 * `usageByAccount`). Kept per window, filled by TerminalPane.
 */
const launched = new Map<string, AccountId>();
const launchedListeners = new Set<() => void>();

/** `account` null: the pane no longer runs Claude Code on any account. */
export function noteLaunchedAccount(paneId: string, account: AccountId | null): void {
  if ((launched.get(paneId) ?? null) === account) return;
  if (account === null) launched.delete(paneId);
  else launched.set(paneId, account);
  for (const listener of launchedListeners) listener();
}

export function launchedAccount(paneId: string): AccountId | null {
  return launched.get(paneId) ?? null;
}

/** Called after every change; returns the unsubscribe. */
export function subscribeLaunchedAccounts(listener: () => void): () => void {
  launchedListeners.add(listener);
  return () => {
    launchedListeners.delete(listener);
  };
}
