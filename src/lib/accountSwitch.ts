import type { RateLimit } from "./bridge";
import { readAccountChoice, resolveAccount, type AccountId, type ClaudeAccount } from "./accounts";

/**
 * The pure logic behind « Comptes Claude »'s interface: which panes restart
 * after a switch, how much of each account's limits is used, and how an
 * account is named. No store, no invoke: the components and the later wiring
 * hand it plain data.
 */

/** What this logic needs to know of a pane. */
export interface SwitchPane {
  id: string;
  /** `claude` is the only agent an account applies to. */
  agent: string;
  /** The pane's own account (override); absent or null follows its workspace. */
  claudeAccount?: string | null;
  /** Only a running agent has anything to restart. */
  running?: boolean;
}

export interface SwitchWorkspace {
  id: string;
  /** The workspace's own account; absent or null follows the default. */
  claudeAccount?: string | null;
  panes: SwitchPane[];
}

const isClaude = (pane: SwitchPane) => pane.agent === "claude";
const hasOwn = (x: { claudeAccount?: unknown }) => readAccountChoice(x.claudeAccount) !== null;

/**
 * The Claude Code panes of `workspace` that follow it, so that change when its
 * account does. A pane with an account of its own is untouched.
 */
export function panesToRestartOnWorkspaceChange(workspace: SwitchWorkspace): SwitchPane[] {
  return workspace.panes.filter((pane) => isClaude(pane) && !hasOwn(pane));
}

/**
 * The Claude Code panes that follow the default when it changes: those of the
 * workspaces without an account of their own, minus the panes with theirs.
 */
export function panesToRestartOnDefaultChange(workspaces: SwitchWorkspace[]): SwitchPane[] {
  return workspaces.filter((w) => !hasOwn(w)).flatMap(panesToRestartOnWorkspaceChange);
}

/** How many of `panes` are running an agent right now. */
export function runningCount(panes: SwitchPane[]): number {
  return panes.filter((pane) => pane.running).length;
}

/** One report of the hangar-bridge: a pane's rate limits at a moment. */
export interface UsageMeasure {
  paneId: string;
  /** Epoch milliseconds. */
  at: number;
  rateLimits: RateLimit[];
}

export interface WindowUsage {
  percentUsed: number;
  resetsAt: string | null;
  at: number;
}

export interface AccountUsage {
  fiveHour?: WindowUsage;
  weekly?: WindowUsage;
}

const FIVE_HOUR = "five_hour";
const WEEKLY = "seven_day";

/**
 * The usage of each account, from the measures of the panes launched on it:
 * the latest one per account and per window. A window an account never
 * reported is absent, which the interface shows as « unknown ». A pane whose
 * account is not known (`accountOf` gives nothing) counts for nobody.
 */
export function usageByAccount(
  measures: UsageMeasure[],
  accountOf: (paneId: string) => AccountId | null | undefined,
): Record<AccountId, AccountUsage> {
  const result: Record<AccountId, AccountUsage> = {};
  for (const measure of measures) {
    const account = accountOf(measure.paneId);
    if (!account) continue;
    for (const limit of measure.rateLimits) {
      const slot = limit.kind === FIVE_HOUR ? "fiveHour" : limit.kind === WEEKLY ? "weekly" : null;
      if (!slot || !Number.isFinite(limit.percentUsed)) continue;
      const entry = (result[account] ??= {});
      const known = entry[slot];
      if (known && known.at > measure.at) continue;
      entry[slot] = { percentUsed: limit.percentUsed, resetsAt: limit.resetsAt, at: measure.at };
    }
  }
  return result;
}

/** A usage as a whole percent between 0 and 100, null when unknown. */
export function usagePercent(usage: WindowUsage | undefined): number | null {
  if (!usage) return null;
  return Math.min(100, Math.max(0, Math.round(usage.percentUsed)));
}

/** The account's label, or `unknown` when it does not exist (any more). */
export function accountName(
  accounts: ClaudeAccount[],
  id: AccountId | null | undefined,
  unknown: string,
): string {
  const found = accounts.find((a) => a.id === id);
  return found ? found.label : unknown;
}

/**
 * « Default (Perso) »: `template` holds `{name}`. The default is what the
 * settings name, main when they name nothing or an account that is gone.
 */
export function defaultLabel(
  accounts: ClaudeAccount[],
  settings: { claudeAccount?: string | null } | undefined,
  template: string,
  unknown: string,
): string {
  const { id } = resolveAccount(accounts, settings);
  return template.replace("{name}", accountName(accounts, id, unknown));
}

/** The workspaces whose own account is `id`. */
export function workspacesUsing(workspaces: SwitchWorkspace[], id: AccountId): SwitchWorkspace[] {
  return workspaces.filter((w) => readAccountChoice(w.claudeAccount) === id);
}

/**
 * `known` with `fresh` laid over it: per account and window, the more recent
 * measure wins. Neither argument is modified.
 */
export function mergeUsage(
  known: Record<AccountId, AccountUsage>,
  fresh: Record<AccountId, AccountUsage>,
): Record<AccountId, AccountUsage> {
  const result: Record<AccountId, AccountUsage> = { ...known };
  for (const [account, usage] of Object.entries(fresh)) {
    const merged: AccountUsage = { ...result[account] };
    for (const slot of ["fiveHour", "weekly"] as const) {
      const next = usage[slot];
      const old = merged[slot];
      if (next && (!old || next.at >= old.at)) merged[slot] = next;
    }
    result[account] = merged;
  }
  return result;
}

/** True when a hand-edited `claudeAccount` choice names account `id`. */
export function namesAccount(x: { claudeAccount?: unknown } | undefined, id: AccountId): boolean {
  return readAccountChoice(x?.claudeAccount) === id;
}
