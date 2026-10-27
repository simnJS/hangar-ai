import { useSyncExternalStore } from "react";
import type { RateLimit } from "./bridge";

/**
 * Where each pane's agent stands, as the pane itself works it out — from the
 * hangar-bridge mod when its Claude Code loaded it, from the shape of its
 * output otherwise. Kept outside React state because the panes write it and
 * the sidebar, which is nowhere near them in the tree, reads it.
 *
 * - `working`: a turn is running.
 * - `waiting`: a permission dialog (or a question) is waiting for an answer.
 * - `yours`: the agent handed control back and nobody has looked since.
 * - `idle`: nothing to report.
 */
export type AgentActivity = "working" | "waiting" | "yours" | "idle";

export interface PaneActivity {
  workspaceId: string;
  activity: AgentActivity;
  /** The state comes from the mod rather than from a guess. */
  bridged: boolean;
}

export interface WorkspaceActivity {
  working: number;
  waiting: number;
  yours: number;
}

const panes = new Map<string, PaneActivity>();
/** The plan's rate-limit windows, from whichever session reported last. */
let planLimits: { limits: RateLimit[]; at: number } | null = null;
const listeners = new Set<() => void>();
/** Bumped on every change, so snapshots can be cached between them. */
let version = 0;

function changed() {
  version += 1;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Told about every change the panes of this window make, so it can pass them
 * on to the other windows: a workspace in a window of its own still counts in
 * the main window's sidebar. Changes coming from another window go through
 * `applyRemoteActivity`, which does not pass them on again.
 */
let relay: ((paneId: string, value: PaneActivity | null) => void) | null = null;

export function relayActivity(fn: typeof relay) {
  relay = fn;
}

function apply(paneId: string, value: PaneActivity | null): boolean {
  if (value === null) return panes.delete(paneId);
  const current = panes.get(paneId);
  if (
    current &&
    current.activity === value.activity &&
    current.bridged === value.bridged &&
    current.workspaceId === value.workspaceId
  ) {
    return false;
  }
  panes.set(paneId, value);
  return true;
}

export function setPaneActivity(paneId: string, value: PaneActivity) {
  if (!apply(paneId, value)) return;
  changed();
  relay?.(paneId, value);
}

export function clearPaneActivity(paneId: string) {
  if (!apply(paneId, null)) return;
  changed();
  relay?.(paneId, null);
}

export function applyRemoteActivity(paneId: string, value: PaneActivity | null) {
  if (apply(paneId, value)) changed();
}

export function getPaneActivity(paneId: string): PaneActivity | undefined {
  return panes.get(paneId);
}

/** The pane's agent is waiting for an answer, rather than at work or done. */
export const isWaiting = (paneId: string) => panes.get(paneId)?.activity === "waiting";

/** One pane's state, re-rendering only when that pane's entry changes. */
export function usePaneActivity(paneId: string | null): PaneActivity | undefined {
  return useSyncExternalStore(subscribe, () => (paneId ? panes.get(paneId) : undefined));
}

/**
 * Panes whose file another pane just edited too. Keyed by the pane that had
 * edited it first, valued with the name of the pane that followed. Nothing
 * was stopped — both edits went through — so the mark only says it, and
 * clears itself.
 */
const conflicts = new Map<string, string>();
const conflictTimers = new Map<string, number>();
/** Long enough to be noticed on a glance at the grid, short of becoming noise. */
const OVERLAP_SHOWN_MS = 2 * 60 * 1000;

function setConflict(paneId: string, by: string | null) {
  if (by === null ? !conflicts.delete(paneId) : conflicts.get(paneId) === by) return;
  if (by !== null) conflicts.set(paneId, by);
  changed();
}

/** Marks `paneId` as overlapped by `by` for a couple of minutes. */
export function flagOverlap(paneId: string, by: string) {
  setConflict(paneId, by);
  window.clearTimeout(conflictTimers.get(paneId));
  conflictTimers.set(
    paneId,
    window.setTimeout(() => {
      conflictTimers.delete(paneId);
      setConflict(paneId, null);
    }, OVERLAP_SHOWN_MS),
  );
}

/** The name of the pane that just edited a file this one had edited, if any. */
export function useConflict(paneId: string): string | null {
  return useSyncExternalStore(subscribe, () => conflicts.get(paneId) ?? null);
}

export function setPlanLimits(limits: RateLimit[]) {
  if (!limits.length) return;
  planLimits = { limits, at: Date.now() };
  changed();
}

const summaries = new Map<string, { version: number; value: WorkspaceActivity }>();

function summarize(workspaceId: string): WorkspaceActivity {
  const cached = summaries.get(workspaceId);
  if (cached && cached.version === version) return cached.value;
  const value: WorkspaceActivity = { working: 0, waiting: 0, yours: 0 };
  for (const pane of panes.values()) {
    if (pane.workspaceId !== workspaceId) continue;
    if (pane.activity === "working") value.working += 1;
    else if (pane.activity === "waiting") value.waiting += 1;
    else if (pane.activity === "yours") value.yours += 1;
  }
  // Same object while nothing it counts changed, or every pane event would
  // re-render every sidebar row.
  if (
    cached &&
    cached.value.working === value.working &&
    cached.value.waiting === value.waiting &&
    cached.value.yours === value.yours
  ) {
    summaries.set(workspaceId, { version, value: cached.value });
    return cached.value;
  }
  summaries.set(workspaceId, { version, value });
  return value;
}

/** How many of a workspace's agents are working, waiting, or done. */
export function useWorkspaceActivity(workspaceId: string): WorkspaceActivity {
  return useSyncExternalStore(subscribe, () => summarize(workspaceId));
}

/** The panes, by id, that are working, waiting, or done, in no particular order. */
export interface AgentRoster {
  working: string[];
  waiting: string[];
  yours: string[];
}

const EMPTY_ROSTER: AgentRoster = { working: [], waiting: [], yours: [] };
const rosters = new Map<string, { version: number; value: AgentRoster }>();

const sameIds = (a: string[], b: string[]) =>
  a.length === b.length && a.every((id, i) => id === b[i]);

function rosterOf(workspaceIds: readonly string[]): AgentRoster {
  if (!workspaceIds.length) return EMPTY_ROSTER;
  const key = workspaceIds.join("\n");
  const cached = rosters.get(key);
  if (cached && cached.version === version) return cached.value;
  const wanted = new Set(workspaceIds);
  const value: AgentRoster = { working: [], waiting: [], yours: [] };
  for (const [paneId, pane] of panes) {
    if (!wanted.has(pane.workspaceId) || pane.activity === "idle") continue;
    value[pane.activity].push(paneId);
  }
  // Same object while the same panes are in the same states, as above.
  const kept =
    cached &&
    sameIds(cached.value.working, value.working) &&
    sameIds(cached.value.waiting, value.waiting) &&
    sameIds(cached.value.yours, value.yours)
      ? cached.value
      : value;
  rosters.set(key, { version, value: kept });
  return kept;
}

/**
 * Which panes of these workspaces are working, waiting, or done: the
 * navigator's summary of a workspace, or of a folded folder, names them.
 * Re-renders only when one of them changes state.
 */
export function useAgentsIn(workspaceIds: readonly string[]): AgentRoster {
  return useSyncExternalStore(subscribe, () => rosterOf(workspaceIds));
}

/** How many of these panes have a Claude Code that loaded the mod. */
export function useBridgedCount(paneIds: string[]): number {
  return useSyncExternalStore(
    subscribe,
    () => paneIds.filter((id) => panes.get(id)?.bridged).length,
  );
}

export function usePlanLimits(): { limits: RateLimit[]; at: number } | null {
  return useSyncExternalStore(subscribe, () => planLimits);
}
