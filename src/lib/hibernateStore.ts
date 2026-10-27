import { useSyncExternalStore } from "react";
import type { AgentActivity } from "./agentState";
import type { PaneRest } from "./hibernate";

/**
 * What the decision in lib/hibernate is made from, and what it decided.
 *
 * The panes write here as things happen — a key pressed, a chunk printed, a
 * turn over — so these are plain maps rather than React state: a pane printing
 * a build log would otherwise re-render the app for every line. Nothing reads
 * them but the window's timer, twice a minute, and a workspace's menu when it
 * opens on « Mettre en veille ». The workspaces asleep are the one thing on
 * screen, and only they notify.
 *
 * Everything is per window and in memory: a reload forgets which workspaces
 * were asleep, and they simply count as never opened.
 */

/** When each workspace was last used, by workspace id. */
const touched = new Map<string, number>();

/** Someone typed into the workspace, clicked it, or sent it something. */
export function touchWorkspace(workspaceId: string, at = Date.now()) {
  touched.set(workspaceId, at);
}

export function lastTouch(workspaceId: string): number | undefined {
  return touched.get(workspaceId);
}

type Rest = Omit<PaneRest, "queued" | "hasChildren">;

/** By pane id, from the moment its terminal mounts until it unmounts. */
const rest = new Map<string, Rest>();

/** A pane that just mounted has done nothing yet, which counts from now. */
export function notePaneMounted(paneId: string) {
  const now = Date.now();
  rest.set(paneId, { lastOutputAt: now, busy: false, restingSince: now, agentExited: false });
}

/** Called for every chunk a pane prints, so it does no more than a write. */
export function notePaneOutput(paneId: string) {
  const entry = rest.get(paneId);
  if (entry) entry.lastOutputAt = Date.now();
}

/** The resting clock starts when the agent stops working or waiting. */
export function notePaneActivity(paneId: string, activity: AgentActivity) {
  const entry = rest.get(paneId);
  if (!entry) return;
  const busy = activity === "working" || activity === "waiting";
  if (busy === entry.busy) return;
  entry.busy = busy;
  if (!busy) entry.restingSince = Date.now();
}

/** The agent quit back to its shell, or a new one said hello in it. */
export function noteAgentExited(paneId: string, exited: boolean) {
  const entry = rest.get(paneId);
  if (entry) entry.agentExited = exited;
}

export function forgetPane(paneId: string) {
  rest.delete(paneId);
}

export function paneRest(paneId: string): Rest | undefined {
  return rest.get(paneId);
}

/**
 * Panes put to sleep that have not started again yet. Each resumes its
 * session on its next launch whatever the auto-resume setting says: a user
 * who turned it off still never lost a conversation to a workspace going to
 * sleep, behind their back or at their word. Taken once, by the launch it was
 * meant for.
 *
 * A workspace window opened on a sleeping workspace is handed the ids in its
 * address (lib/windows), and starts with them here.
 */
const resumeOnce = new Set<string>(
  (() => {
    try {
      return new URLSearchParams(window.location.search).get("resume")?.split(",") ?? [];
    } catch {
      return [];
    }
  })().filter(Boolean),
);

export function resumeOnWake(paneIds: string[]) {
  for (const id of paneIds) resumeOnce.add(id);
}

/** Whether the pane's next launch must resume; `take` uses the promise up. */
export function mustResume(paneId: string, take = false): boolean {
  const owed = resumeOnce.has(paneId);
  if (take) resumeOnce.delete(paneId);
  return owed;
}

/** A pane restarted under a new id keeps what its old id was owed. */
export function carryResume(fromId: string, toId: string) {
  if (mustResume(fromId, true)) resumeOnce.add(toId);
}

/** A workspace asleep: since when, and how many agents it stopped. */
export interface Asleep {
  at: number;
  agents: number;
}

let asleep: ReadonlyMap<string, Asleep> = new Map();
const listeners = new Set<() => void>();

function publish(next: Map<string, Asleep>) {
  asleep = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function markAsleep(workspaceId: string, info: Asleep) {
  publish(new Map(asleep).set(workspaceId, info));
}

/** Awake again — opened by hand, by an agent, or sent to a window. */
export function markAwake(workspaceId: string) {
  if (!asleep.has(workspaceId)) return;
  const next = new Map(asleep);
  next.delete(workspaceId);
  publish(next);
}

export function isAsleep(workspaceId: string): boolean {
  return asleep.has(workspaceId);
}

/** Forgets the workspaces asleep that are not in `existing`: deleted since. */
export function pruneAsleep(existing: Set<string>) {
  const gone = [...asleep.keys()].filter((id) => !existing.has(id));
  if (!gone.length) return;
  const next = new Map(asleep);
  for (const id of gone) next.delete(id);
  publish(next);
}

/** Every workspace asleep, by id. The same map until one falls asleep or wakes. */
export function useAsleep(): ReadonlyMap<string, Asleep> {
  return useSyncExternalStore(subscribe, () => asleep);
}
