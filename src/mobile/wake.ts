import type { Project } from "./api";

/**
 * Waking a project asleep from the phone. The person taps « Réveiller », or
 * writes to one of its agents; the PC opens it (remote_wake), and its
 * agents resume their sessions. The phone says « Réveil en cours… » from
 * then until the PC's state, polled every 2 s, shows it awake — and gives
 * the button back, saying why, when the call failed or the PC still shows
 * it asleep after WAKE_TIMEOUT_MS.
 *
 * Kept by project id, for the list, the project and its panes to agree.
 */

/** Far past the moment the PC opens a project: by then, it did not. */
export const WAKE_TIMEOUT_MS = 30_000;

/** One try at waking a project. */
export interface WakeTry {
  /** When the person asked, or sent what wakes it. */
  at: number;
  /** The call failed: what it threw, for the screen to word. */
  error?: unknown;
}

export type Wakes = Readonly<Record<string, WakeTry>>;

export type Wake =
  | { state: "awake" }
  | { state: "waking" }
  /** `failed`: the last try's call failed, or the PC never showed it awake. */
  | { state: "asleep"; failed: "call" | "timeout" | null };

/** Where a project stands: awake as soon as the PC says so, waking while a try is under way. */
export function wakeOf(project: Pick<Project, "asleep">, attempt: WakeTry | undefined, now: number): Wake {
  if (!project.asleep) return { state: "awake" };
  if (!attempt) return { state: "asleep", failed: null };
  if (attempt.error !== undefined) return { state: "asleep", failed: "call" };
  return now - attempt.at < WAKE_TIMEOUT_MS ? { state: "waking" } : { state: "asleep", failed: "timeout" };
}

/** A try starts: the one under way is kept, so its clock does not start over. */
export function startWake(wakes: Wakes, projectId: string, now: number): Wakes {
  const current = wakes[projectId];
  if (current && current.error === undefined && now - current.at < WAKE_TIMEOUT_MS) return wakes;
  return { ...wakes, [projectId]: { at: now } };
}

export function failWake(wakes: Wakes, projectId: string, error: unknown, now: number): Wakes {
  return { ...wakes, [projectId]: { at: now, error } };
}

/**
 * What the PC's state settles: a project it shows awake, or no longer
 * lists, has nothing left to wake. The same object when nothing changes.
 */
export function settleWakes(wakes: Wakes, projects: Pick<Project, "id" | "asleep">[]): Wakes {
  const asleep = new Set(projects.filter((project) => project.asleep).map((project) => project.id));
  const kept = Object.entries(wakes).filter(([id]) => asleep.has(id));
  return kept.length === Object.keys(wakes).length ? wakes : Object.fromEntries(kept);
}
