/**
 * The usage statistics, as crates/hangar-core/src/stats.rs reads them out of
 * the agents' own transcripts. Field names follow the Rust serialization.
 *
 * The phone's fake PC (src/mockStats.ts) builds them to sum up the way the
 * desktop does; the desktop's src/lib/stats.ts also fetched and kept them
 * (tag v1.12.1), which the phone never does: the PC answers remote_stats
 * already summed up.
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
