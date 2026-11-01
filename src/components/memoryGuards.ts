/**
 * Pure rules behind MemoryView's safeguards, kept free of React and Tauri so a
 * test can run them under Node.
 */
import type { MemoryEntry } from "../lib/memory";

/** Mirrors MAX_ENTRIES in memory.rs: past it, memory_write refuses. */
export const MEMORY_CAP = 1000;
/** From here MemoryView tells the person the memory is nearly full. */
export const MEMORY_WARN_AT = 900;

/** True when the memory is close enough to its cap to deserve a line. */
export function nearCap(count: number): boolean {
  return count >= MEMORY_WARN_AT;
}

/**
 * The same identity memory.rs upserts on: titles that differ only by case or
 * edge whitespace are one fact (`title_key`).
 */
export function titleKey(title: string): string {
  return title.trim().toLowerCase();
}

/**
 * The entry a new title would replace, if any. Creating from the composer is an
 * upsert on the backend, so a taken title silently overwrites that entry's
 * content — the view asks first, using the entries it already loaded.
 */
export function findTitleClash(
  entries: readonly MemoryEntry[],
  title: string,
): MemoryEntry | null {
  const key = titleKey(title);
  if (!key) return null;
  return entries.find((entry) => titleKey(entry.title) === key) ?? null;
}

export type MemorySort = "recent" | "oldest";

/** Newest update first, or oldest update first to find what to clean up. */
export function sortEntries(entries: readonly MemoryEntry[], sort: MemorySort): MemoryEntry[] {
  const direction = sort === "oldest" ? 1 : -1;
  return [...entries].sort((a, b) => direction * (a.updated_at - b.updated_at));
}
