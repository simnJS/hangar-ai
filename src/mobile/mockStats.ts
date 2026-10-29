import type { RateLimit } from "../lib/bridge";
import type { StatsAgent, StatsBucket, StatsSession, UsageStats } from "../lib/stats";
import { addDays, projectFor, startOfDay, summarize, type Measure, type Period, type ProjectRoot } from "../lib/statsModel";
import { remoteStatsOf } from "../remote/stats";
import type { RemoteStats } from "../remote/types";

/**
 * The fake PC's usage statistics: five months of a person's agents, hour by
 * hour, as stats.rs would read them from the transcripts, summed up by the
 * statistics page's own `summarize` and cut down by the same code as the
 * real window's answer (src/remote/stats). Drawn from a seeded generator, so
 * a screen looks the same from one load to the next.
 *
 * IaBench is the busy one, every weekday and more so lately; storefront was
 * finished ten days ago; billing-api has been asleep since yesterday;
 * recipes-app is Codex's, for three weeks; a few folders of no workspace
 * come and go. The plan's 5-hour window is well used, the week less so.
 */

const HOUR = 3_600_000;
const DAYS = 150;
const HOME = "C:\\Users\\simnJS\\Documents";

const ROOTS: ProjectRoot[] = [
  { workspaceId: "ws-iabench", name: "IaBench", path: "c:/users/simnjs/documents/iabench" },
  { workspaceId: "ws-iabench", name: "IaBench", path: "c:/users/simnjs/documents/iabench-worktrees" },
  { workspaceId: "ws-storefront", name: "storefront", path: "c:/users/simnjs/documents/storefront" },
  { workspaceId: "ws-billing", name: "billing-api", path: "c:/users/simnjs/documents/billing-api" },
  { workspaceId: "ws-recipes", name: "recipes-app", path: "c:/users/simnjs/documents/recipes-app" },
];

/** Dollars per million tokens: input, output, cache write, cache read. */
const MODELS: { id: string; price: [number, number, number, number] | null }[] = [
  { id: "claude-opus-5-5", price: [5, 25, 6.25, 0.5] },
  { id: "claude-sonnet-5-5", price: [3, 15, 3.75, 0.3] },
  { id: "claude-haiku-5-5", price: [1, 5, 1.25, 0.1] },
  { id: "gpt-5.6-sol", price: [1.25, 10, 0, 0.125] },
  // A local model Codex was pointed at once: no price, no cost.
  { id: "qwen3-coder-30b", price: null },
];

interface Work {
  cwd: string;
  agent: StatsAgent;
  model: number;
  /** Sessions on a working day, at most. */
  sessions: number;
  /** Days before today it runs on, both ends included. */
  from: number;
  to: number;
  /** A weekday's chance of any work. */
  odds: number;
  /** Weekends too. */
  weekends?: boolean;
  /** Cache reads per active hour, millions. */
  reads: number;
}

const WORK: Work[] = [
  { cwd: `${HOME}\\IaBench`, agent: "claude", model: 0, sessions: 2, from: DAYS, to: 0, odds: 0.92, reads: 2.4 },
  { cwd: `${HOME}\\IaBench-worktrees\\hangar-mobile-stats`, agent: "claude", model: 0, sessions: 2, from: 70, to: 0, odds: 0.9, weekends: true, reads: 1.6 },
  { cwd: `${HOME}\\storefront`, agent: "claude", model: 1, sessions: 2, from: 120, to: 10, odds: 0.7, reads: 1.6 },
  { cwd: `${HOME}\\billing-api`, agent: "claude", model: 0, sessions: 1, from: 90, to: 1, odds: 0.35, reads: 1.2 },
  { cwd: `${HOME}\\recipes-app`, agent: "codex", model: 3, sessions: 1, from: 21, to: 0, odds: 0.8, reads: 1.1 },
  { cwd: `${HOME}\\recipes-app`, agent: "codex", model: 4, sessions: 1, from: 16, to: 14, odds: 1, reads: 0.4 },
  { cwd: "C:\\Users\\simnJS\\Desktop\\scratch", agent: "claude", model: 2, sessions: 1, from: DAYS, to: 2, odds: 0.15, weekends: true, reads: 0.3 },
  { cwd: "C:\\Users\\simnJS\\dotfiles", agent: "claude", model: 1, sessions: 1, from: 140, to: 40, odds: 0.08, weekends: true, reads: 0.5 },
  { cwd: "C:\\Users\\simnJS\\blog", agent: "claude", model: 1, sessions: 1, from: 75, to: 30, odds: 0.1, weekends: true, reads: 0.6 },
  { cwd: "C:\\Users\\simnJS\\advent-of-code", agent: "codex", model: 3, sessions: 1, from: 60, to: 35, odds: 0.12, weekends: true, reads: 0.4 },
];

/** mulberry32: a small generator, seeded. */
function seeded(seed: number) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function usage(now: number): UsageStats {
  const random = seeded(20261029);
  const between = (low: number, high: number) => low + random() * (high - low);
  const sessions: StatsSession[] = [];
  const buckets: StatsBucket[] = [];
  const today = startOfDay(now);

  for (let back = DAYS; back >= 0; back--) {
    const day = addDays(today, -back);
    const weekend = new Date(day).getDay() % 6 === 0;
    for (const work of WORK) {
      if (back > work.from || back < work.to) continue;
      if (weekend && !work.weekends && random() > 0.15) continue;
      if (random() > work.odds) continue;
      // The last weeks ran busier: more sessions, longer ones.
      const recent = back < 30 ? 1.15 : 1;
      const count = Math.max(1, Math.round(between(0.6, work.sessions + 0.4)));
      for (let s = 0; s < count; s++) {
        const start = Math.floor(between(8, 19));
        const hours = Math.max(1, Math.round(between(1, 4) * recent));
        const index = sessions.length;
        sessions.push({
          agent: work.agent,
          id: `mock-${index}`,
          cwd: work.cwd,
          title: "",
          firstAt: day + start * HOUR,
          lastAt: day + (start + hours) * HOUR,
        });
        for (let h = 0; h < hours && start + h < 24; h++) {
          const at = day + (start + h) * HOUR;
          // Today stops at the hour it is.
          if (at > now) break;
          const reads = work.reads * between(0.5, 1.5) * recent * 1e6;
          const row = token(index, at, work.model, {
            input: Math.round(between(6_000, 30_000) * (work.agent === "codex" ? 12 : 1)),
            output: Math.round(between(8_000, 40_000)),
            cacheWrite: work.agent === "codex" ? 0 : Math.round(reads * between(0.04, 0.09)),
            cacheRead: Math.round(reads),
          });
          buckets.push(row);
          // Claude Code hands the small work to Haiku.
          if (work.agent === "claude" && work.model !== 2 && random() < 0.5) {
            buckets.push(token(index, at, 2, {
              input: Math.round(between(2_000, 9_000)),
              output: Math.round(between(1_000, 5_000)),
              cacheWrite: Math.round(between(10_000, 40_000)),
              cacheRead: Math.round(between(60_000, 300_000)),
            }));
          }
          buckets.push({
            ...token(index, at, -1, { input: 0, output: 0, cacheWrite: 0, cacheRead: 0 }),
            turns: Math.round(between(1, 6)),
            activeMs: Math.round(between(18, 55) * 60_000),
          });
        }
      }
    }
  }

  return {
    generatedAt: now - 2 * 60_000,
    scan: { files: sessions.length, parsed: 3, reused: sessions.length - 3, failed: 0, bytes: 0, ms: 180, cachedOnly: false },
    sources: [],
    activeGapCapMs: 5 * 60_000,
    sessions,
    models: MODELS.map((model) => ({ id: model.id, priced: model.price !== null })),
    buckets,
  };
}

/** One hour of one model's tokens, priced as the table says. */
function token(
  session: number,
  at: number,
  model: number,
  tokens: { input: number; output: number; cacheWrite: number; cacheRead: number },
): StatsBucket {
  const price = model >= 0 ? MODELS[model].price : null;
  const [input, output, cacheWrite, cacheRead] = (price ?? [0, 0, 0, 0]).map((perMillion) => perMillion / 1e6);
  const costs = {
    costInput: tokens.input * input,
    costOutput: tokens.output * output,
    costCacheWrite: tokens.cacheWrite * cacheWrite,
    costCacheRead: tokens.cacheRead * cacheRead,
  };
  return {
    session,
    at,
    model,
    ...tokens,
    ...costs,
    cost: costs.costInput + costs.costOutput + costs.costCacheWrite + costs.costCacheRead,
    calls: model >= 0 ? Math.round(tokens.output / 900) : 0,
    turns: 0,
    activeMs: 0,
  };
}

/** The plan as a session reported it three minutes ago: the 5-hour window well used, the week less. */
function plan(now: number): { limits: RateLimit[]; at: number } {
  const saturday = addDays(startOfDay(now), ((6 - new Date(now).getDay() + 7) % 7) || 7) + 9 * HOUR;
  return {
    at: now - 3 * 60_000,
    limits: [
      { kind: "five_hour", percentUsed: 72.4, resetsAt: new Date(now + 107 * 60_000).toISOString() },
      { kind: "seven_day", percentUsed: 38.1, resetsAt: new Date(saturday).toISOString() },
    ],
  };
}

export interface MockStats {
  /** remote_stats, as the PC answers it; nothing at all with `none`. */
  answer: (period: Period, measure: Measure, now?: number) => RemoteStats;
}

export function mockStats(created: number, { none = false }: { none?: boolean } = {}): MockStats {
  const data = none ? { ...usage(created), sessions: [], buckets: [] } : usage(created);
  const projects = data.sessions.map((session) => projectFor(session.cwd, ROOTS));
  const agentLabel = (agent: StatsAgent) => (agent === "codex" ? "Codex" : "Claude Code");
  return {
    answer: (period, measure, now = Date.now()) =>
      remoteStatsOf(summarize(data, { period, project: null, measure, now, projects, agentLabel, topSessions: 0 }), {
        period,
        measure,
        readAt: data.generatedAt,
        cachedOnly: false,
        plan: none ? null : plan(created),
      }),
  };
}
