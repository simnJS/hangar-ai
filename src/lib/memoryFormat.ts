import { AGENTS } from "../types";

/**
 * How a memory row writes where an entry came from: who signed it, when, and
 * from which folder. Pure, so it can be tested without a DOM.
 */

/**
 * A path that fits a narrow column, keeping its end: the folder a fact came
 * from is told apart by its last segments, never by its drive or home.
 * "C:\Users\me\Documents\IaBench" becomes "…\Documents\IaBench".
 */
export function compactPath(path: string, max = 30): string {
  if (path.length <= max) return path;
  const sep = path.includes("\\") && !path.includes("/") ? "\\" : "/";
  const parts = path.split(/[\\/]/).filter(Boolean);
  let tail = parts.pop() ?? path;
  while (parts.length) {
    const next = `${parts[parts.length - 1]}${sep}${tail}`;
    if (next.length + 2 > max) break;
    tail = next;
    parts.pop();
  }
  return `…${sep}${tail}`;
}

/** Who wrote an entry, as a row says it; null when it was you. */
export function signerName(agent: string): string | null {
  const name = agent.trim();
  if (!name || name === "user") return null;
  return AGENTS.find((known) => known.id === name)?.label ?? name;
}

/**
 * A day, as short as it can be said: "21 oct." this year, "21 oct. 2025"
 * before, and `today` for anything since midnight, which the caller words.
 */
export function shortDay(ms: number, now: number, locale: string): string | "today" {
  const date = new Date(ms);
  const current = new Date(now);
  const sameDay =
    date.getFullYear() === current.getFullYear() &&
    date.getMonth() === current.getMonth() &&
    date.getDate() === current.getDate();
  if (sameDay) return "today";
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    ...(date.getFullYear() === current.getFullYear() ? {} : { year: "numeric" }),
  }).format(date);
}

/**
 * An entry counts as rewritten when it changed well after it was created: an
 * upsert a few seconds later is still the first writing.
 */
export const wasUpdated = (created: number, updated: number) => updated - created > 60_000;
