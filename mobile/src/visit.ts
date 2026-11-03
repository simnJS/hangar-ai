/**
 * "Since your last visit": the phone keeps when the app was last in use, and
 * a visit begins when it comes back after a while away. The history then
 * shows what happened since the end of the previous visit.
 */

/** Away this long, coming back is a new visit. */
export const VISIT_GAP_MS = 30 * 60_000;

export interface Visit {
  /** When the app was last seen in use. */
  lastSeen: number | null;
  /** Where the current visit's history starts; null on a first visit. */
  since: number | null;
}

/** The app is in use at `now`: the same visit, or a new one since `lastSeen`. */
export function touchVisit(visit: Visit, now: number, gapMs = VISIT_GAP_MS): Visit {
  const isNew = visit.lastSeen === null || now - visit.lastSeen >= gapMs;
  return { lastSeen: now, since: isNew ? visit.lastSeen : visit.since };
}

export function parseVisit(raw: string | null): Visit {
  try {
    const value = JSON.parse(raw ?? "null");
    const time = (field: unknown) => (typeof field === "number" && Number.isFinite(field) ? field : null);
    return { lastSeen: time(value?.lastSeen), since: time(value?.since) };
  } catch {
    return { lastSeen: null, since: null };
  }
}
