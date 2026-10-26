import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * The usage statistics, as src-tauri/src/stats.rs reads them out of the
 * agents' own transcripts. Field names follow the Rust serialization.
 */

export type StatsAgent = "claude" | "codex";

export interface StatsSession {
  agent: StatsAgent;
  id: string;
  /** The folder the session was launched in. */
  cwd: string;
  /** What the user named it, what Claude Code called it, or its first prompt. */
  title: string;
  firstAt: number;
  lastAt: number;
}

export interface StatsModel {
  id: string;
  /** The price table knows it: its tokens are in the cost. */
  priced: boolean;
}

/**
 * One session's hour, for one model. Turns and active time are the
 * session's, not a model's, and sit in rows whose `model` is -1.
 */
export interface StatsBucket {
  /** Index into `sessions`. */
  session: number;
  /** Start of the local hour, ms. */
  at: number;
  /** Index into `models`, or -1. */
  model: number;
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
  /** API-equivalent, US dollars: the total, then its share per kind. */
  cost: number;
  costInput: number;
  costOutput: number;
  costCacheWrite: number;
  costCacheRead: number;
  calls: number;
  turns: number;
  activeMs: number;
}

export interface StatsSource {
  agent: StatsAgent;
  root: string;
  found: boolean;
  files: number;
}

export interface StatsScan {
  files: number;
  parsed: number;
  reused: number;
  failed: number;
  bytes: number;
  ms: number;
  /** Answered from the cache alone, without looking at the disk. */
  cachedOnly: boolean;
}

export interface UsageStats {
  generatedAt: number;
  scan: StatsScan;
  sources: StatsSource[];
  /** The longest a silence counts for in active time. */
  activeGapCapMs: number;
  sessions: StatsSession[];
  models: StatsModel[];
  buckets: StatsBucket[];
}

export const usageStats = (args: { from?: number; to?: number; cachedOnly?: boolean } = {}) =>
  invoke<UsageStats>("usage_stats", args);

/**
 * What the page shows, kept for the life of the window: coming back to the
 * statistics draws the last answer at once, and a look at the disk follows.
 */
interface StatsState {
  data: UsageStats | null;
  /** Reading the transcripts: the page shows it without hiding what it has. */
  refreshing: boolean;
  error: string | null;
  /** When the last full scan finished, ms. */
  checkedAt: number | null;
}

let state: StatsState = { data: null, refreshing: false, error: null, checkedAt: null };
const listeners = new Set<() => void>();

function set(patch: Partial<StatsState>) {
  state = { ...state, ...patch };
  listeners.forEach((listener) => listener());
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

let inflight: Promise<void> | null = null;

/**
 * The cache first — instant, even right after a launch, from the copy the
 * last run left on disk — then a scan for whatever changed since. A second
 * call while one runs joins it.
 */
export function refreshStats(): Promise<void> {
  if (inflight) return inflight;
  set({ refreshing: true });
  const run = async () => {
    if (!state.data) {
      try {
        const cached = await usageStats({ cachedOnly: true });
        // An empty cache is no answer yet: the scan's is the first one.
        if (cached.buckets.length) set({ data: cached });
      } catch {
        // The full scan below reports what went wrong.
      }
    }
    try {
      const fresh = await usageStats();
      set({ data: fresh, error: null, checkedAt: Date.now() });
    } catch (err) {
      set({ error: String(err) });
    } finally {
      set({ refreshing: false });
      inflight = null;
    }
  };
  inflight = run();
  return inflight;
}

/** A scan this recent is still the truth: switching views does not redo it. */
const FRESH_FOR_MS = 60_000;

/** The statistics, refreshed when the page mounts if they are not fresh. */
export function useUsageStats(): StatsState & { refresh: () => void } {
  const snapshot = useSyncExternalStore(subscribe, () => state);
  useEffect(() => {
    if (!state.checkedAt || Date.now() - state.checkedAt > FRESH_FOR_MS) void refreshStats();
  }, []);
  return { ...snapshot, refresh: () => void refreshStats() };
}
