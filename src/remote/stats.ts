import type { RateLimit } from "../lib/bridge";
import { planUsage } from "../lib/inspectorFormat";
import { measureOf, type Measure, type Period, type Ranked, type Summary, type Totals } from "../lib/statsModel";
import type { RemoteStats, RemoteStatsRanking, RemoteStatsTotals, RemoteStatsWindow } from "./types";

/**
 * What remote_stats answers, from the statistics page's own summary
 * (lib/statsModel `summarize`): its numbers as they are, cut down to what a
 * phone draws. Pure, and shared by the window (lib/remoteControl) and the
 * phone's fake PC (src/mobile/mock), so both answer the same shapes.
 */

/** Rows of a ranking the phone gets; past them, the last one stands for the rest. */
export const RANKED_ROWS = 6;

const totalsOf = (totals: Totals): RemoteStatsTotals => ({
  tokens: totals.tokens,
  cacheRead: totals.cacheRead,
  cost: totals.cost,
  activeMs: totals.activeMs,
  sessions: totals.sessions,
  turns: totals.turns,
});

/** The first rows as they are; past RANKED_ROWS, the rest folded into one. */
export function ranking(rows: Ranked[]): RemoteStatsRanking {
  const kept = rows.length > RANKED_ROWS ? rows.slice(0, RANKED_ROWS - 1) : rows;
  const rest = rows.slice(kept.length);
  const sum = (pick: (row: Ranked) => number) => rest.reduce((total, row) => total + pick(row), 0);
  return {
    rows: kept.map((row) => ({
      key: row.key,
      label: row.label,
      tokens: row.tokens,
      cost: row.cost,
      activeMs: row.activeMs,
      sessions: row.sessions,
      share: row.share,
      ...(row.unpriced ? { unpriced: true as const } : {}),
    })),
    other: rest.length
      ? {
          count: rest.length,
          tokens: sum((row) => row.tokens),
          cost: sum((row) => row.cost),
          activeMs: sum((row) => row.activeMs),
          share: sum((row) => row.share),
        }
      : null,
  };
}

/** The plan's windows, whole percents (as planUsage reads them) and reset times in ms. */
export function planWindows(limits: RateLimit[]): RemoteStatsWindow[] {
  return limits.map((limit) => {
    const resets = limit.resetsAt ? Date.parse(limit.resetsAt) : NaN;
    return {
      kind: limit.kind,
      percentUsed: planUsage([limit], limit.kind)!.percentUsed,
      resetsAt: Number.isNaN(resets) ? null : resets,
    };
  });
}

export function remoteStatsOf(
  summary: Summary,
  options: {
    period: Period;
    measure: Measure;
    readAt: number;
    cachedOnly: boolean;
    plan: { limits: RateLimit[]; at: number } | null;
  },
): RemoteStats {
  const { period, measure, readAt, cachedOnly, plan } = options;
  return {
    period,
    measure,
    readAt,
    cachedOnly,
    range: { from: summary.range.from, to: summary.range.to, days: summary.range.days },
    granularity: summary.granularity,
    firstAt: summary.firstAt,
    totals: totalsOf(summary.totals),
    previous: summary.previous ? totalsOf(summary.previous) : null,
    activeDays: summary.activeDays,
    series: summary.series.map((point) => ({ at: point.at, value: measureOf(point, measure) })),
    projects: ranking(summary.projects),
    agents: ranking(summary.agents),
    models: ranking(summary.models),
    unpricedTokens: summary.unpricedTokens,
    plan: plan ? { at: plan.at, windows: planWindows(plan.limits) } : null,
  };
}
