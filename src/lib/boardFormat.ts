import type { TranslateKey, Translator } from "../i18n";

/**
 * How the board writes a task's numbers in words: its priority, its id, how
 * long it has been in progress. Pure, so the cards, the dock, the task dialog
 * and the inspector all say the same thing, and so it can be tested without
 * a DOM.
 */

export type PriorityLevel = 1 | 2 | 3;

/**
 * The three steps a card shows for a task's numeric priority.
 *
 * Tasks are created at 1 (src-tauri/src/board.rs), so 1 is the ordinary,
 * medium case; 0 is work that can wait, and anything above 1 is what an agent
 * or a person flagged as urgent — the board used to single out exactly the
 * tasks above 1 with a "P2" chip. The scale is open-ended upwards, and every
 * value above 1 reads as high: the exact number stays in the tooltip.
 */
export function priorityLevel(priority: number): PriorityLevel {
  if (!Number.isFinite(priority) || priority <= 0) return 1;
  if (priority === 1) return 2;
  return 3;
}

const PRIORITY_KEYS: Record<PriorityLevel, TranslateKey> = {
  1: "board.priority.low",
  2: "board.priority.medium",
  3: "board.priority.high",
};

export const priorityKey = (priority: number): TranslateKey =>
  PRIORITY_KEYS[priorityLevel(priority)];

/**
 * The short handle a card shows for its task. Ids are UUIDs, whose first
 * characters are as good as any to tell a dozen tasks apart at a glance; the
 * whole id stays in the tooltip for when an agent quotes it.
 */
export function shortTaskId(id: string): string {
  const compact = id.replace(/-/g, "");
  return (compact || id).slice(0, 4);
}

/**
 * How long something has been going on — a card in `doing`, the session a
 * cost adds up over — at a glance rather than to the second.
 */
export function elapsedLabel(ms: number, t: Translator): string {
  const minutes = Math.floor(Math.max(0, ms) / 60_000);
  if (minutes < 1) return t("board.elapsedNow");
  if (minutes < 60) return t("board.elapsedMinutes", { m: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return t("board.elapsedHours", { h: hours, m: String(minutes % 60).padStart(2, "0") });
  }
  return t("board.elapsedDays", { d: Math.floor(hours / 24), h: hours % 24 });
}
