import { usageTone, type Tone } from "../lib/inspectorFormat";
import {
  change,
  formatChange,
  formatCompact,
  formatDuration,
  formatShare,
  formatTick,
  formatUsd,
} from "../lib/statsModel";
import type {
  RemoteStats,
  RemoteStatsMeasure,
  RemoteStatsPeriod,
  RemoteStatsRanking,
  RemoteStatsWindow,
} from "../remote/types";
import { ago, clock, dayHeading, daysBefore, moment } from "./format";
import type { Translator } from "./i18n";

/**
 * The Stats tab, from what remote_stats answers to what each part of the
 * screen draws: the plan's windows, the totals, the chart and the rankings.
 * The PC has summed everything up the statistics page's way; this only says
 * it in the phone's words, with the page's own number formats
 * (lib/statsModel). Pure: the clock and the language come in as arguments.
 */

export const PERIODS: RemoteStatsPeriod[] = ["7d", "30d", "90d", "all"];
export const MEASURES: RemoteStatsMeasure[] = ["tokens", "cost"];

/** The measure's value, as a total or a column says it: "4.5M tokens", "$12". */
export function measureText(value: number, measure: RemoteStatsMeasure, { t, locale }: Translator): string {
  return measure === "cost" ? formatUsd(value, locale) : t("stats.tokens", { value: formatCompact(value, locale) });
}

/** "45 min", "3 h 12": the page's own rule (formatDuration), in the phone's words. */
export function activeTime(ms: number, { t }: Translator): string {
  const words: Record<string, Parameters<Translator["t"]>[0]> = {
    "stats.durationUnderMinute": "stats.duration.underMinute",
    "stats.durationMinutes": "stats.duration.minutes",
    "stats.durationHours": "stats.duration.hours",
    "stats.durationHoursOnly": "stats.duration.hoursOnly",
  };
  return formatDuration(ms, (key, params) => t(words[key], params as Record<string, string | number>));
}

/**
 * A moment just past, as the end of a sentence: "just now", "12 min ago",
 * "at 14:32" today, "yesterday 14:32" before.
 */
export function when(at: number, now: number, tr: Translator): string {
  const relative = ago(at, now, tr.locale);
  if (relative !== moment(at, now, tr.locale)) return relative;
  return daysBefore(at, now) === 0 ? tr.t("stats.at", { time: clock(at, tr.locale) }) : relative;
}

/* ── The plan's windows ──────────────────────────────────────────── */

export interface PlanBar {
  kind: string;
  label: string;
  /** 0–100; 0 once the window has reset since. */
  percent: number;
  tone: Tone;
  /** "34 % used"; null once the window has reset since the report. */
  used: string | null;
  /** "66 % left", the number the person looks for. */
  left: string | null;
  /** "resets at 17:00", or that it has since; null when no reset time is known. */
  reset: string | null;
  /** The window has freed up since the session reported it: its figure is gone. */
  past: boolean;
}

export interface PlanView {
  bars: PlanBar[];
  /** "Reported by a Claude Code session 3 min ago". */
  reported: string;
}

const PLAN_ORDER = ["five_hour", "seven_day"];

function planLabel(kind: string, { t }: Translator): string {
  if (kind === "five_hour") return t("stats.plan.fiveHour");
  if (kind === "seven_day") return t("stats.plan.week");
  const words = kind.replace(/_/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** When a window frees up, as the desktop says it: today's time, tomorrow, a weekday, a date. */
export function resetText(at: number, now: number, { t, locale }: Translator): string {
  const time = clock(at, locale);
  const days = daysBefore(now, at);
  if (days <= 0) return t("stats.plan.resetToday", { time });
  if (days === 1) return t("stats.plan.resetTomorrow", { time });
  if (days < 7) return t("stats.plan.resetDay", { day: new Intl.DateTimeFormat(locale, { weekday: "long" }).format(at), time });
  return t("stats.plan.resetDate", { date: new Intl.DateTimeFormat(locale, { day: "numeric", month: "long" }).format(at), time });
}

function planBar(window: RemoteStatsWindow, now: number, tr: Translator): PlanBar {
  const past = window.resetsAt !== null && window.resetsAt <= now;
  const percent = past ? 0 : window.percentUsed;
  return {
    kind: window.kind,
    label: planLabel(window.kind, tr),
    percent,
    tone: past ? "normal" : usageTone(percent),
    used: past ? null : tr.t("stats.plan.used", { value: formatShare(percent / 100, tr.locale) }),
    left: past ? null : tr.t("stats.plan.left", { value: formatShare((100 - percent) / 100, tr.locale) }),
    reset:
      window.resetsAt === null
        ? null
        : past
          ? tr.t("stats.plan.wasReset", { time: when(window.resetsAt, now, tr) })
          : resetText(window.resetsAt, now, tr),
    past,
  };
}

/** The 5-hour window first, the week next, any other after; null when no session has said. */
export function planView(plan: RemoteStats["plan"], now: number, tr: Translator): PlanView | null {
  if (!plan || plan.windows.length === 0) return null;
  const rank = (kind: string) => {
    const at = PLAN_ORDER.indexOf(kind);
    return at < 0 ? PLAN_ORDER.length : at;
  };
  const windows = [...plan.windows].sort((a, b) => rank(a.kind) - rank(b.kind));
  return {
    bars: windows.map((window) => planBar(window, now, tr)),
    reported: tr.t("stats.plan.reported", { time: when(plan.at, now, tr) }),
  };
}

/* ── The totals ──────────────────────────────────────────────────── */

export interface Trend {
  text: string;
  /** null when it is no change to point at: flat, new, or no period before. */
  direction: "up" | "down" | null;
}

export interface Kpi {
  key: "tokens" | "cost" | "active" | "sessions";
  label: string;
  value: string;
  trend: Trend | null;
  note: string | null;
}

/** Against the period before, the page's way: new, flat under half a percent, or signed. */
export function trendOf(now: number, before: number | undefined, stats: RemoteStats, tr: Translator): Trend | null {
  const { t, locale } = tr;
  if (!stats.previous || before === undefined) {
    if (stats.firstAt === null) return null;
    const date = new Intl.DateTimeFormat(locale, { day: "numeric", month: "long", year: "numeric" }).format(stats.firstAt);
    return { text: t("stats.since", { date }), direction: null };
  }
  const n = stats.range.days;
  const ratio = change(now, before);
  if (ratio === null) return now > 0 ? { text: t("stats.trend.new", { n }), direction: null } : null;
  if (Math.abs(ratio) < 0.005) return { text: t("stats.trend.flat", { n }), direction: null };
  return { text: `${formatChange(ratio, locale)} ${t("stats.trend.vs", { n })}`, direction: ratio > 0 ? "up" : "down" };
}

export function kpis(stats: RemoteStats, tr: Translator): Kpi[] {
  const { t, tn, locale } = tr;
  const { totals, previous } = stats;
  return [
    {
      key: "tokens",
      label: t("stats.kpi.tokens"),
      value: formatCompact(totals.tokens, locale),
      trend: trendOf(totals.tokens, previous?.tokens, stats, tr),
      note: totals.tokens > 0 ? t("stats.kpi.cache", { share: formatShare(totals.cacheRead / totals.tokens, locale) }) : null,
    },
    {
      key: "cost",
      label: t("stats.kpi.cost"),
      value: formatUsd(totals.cost, locale),
      trend: trendOf(totals.cost, previous?.cost, stats, tr),
      note: t("stats.kpi.costNote"),
    },
    {
      key: "active",
      label: t("stats.kpi.active"),
      value: activeTime(totals.activeMs, tr),
      trend: trendOf(totals.activeMs, previous?.activeMs, stats, tr),
      note: stats.activeDays > 0 ? t("stats.kpi.perDay", { value: activeTime(totals.activeMs / stats.activeDays, tr) }) : null,
    },
    {
      key: "sessions",
      label: t("stats.kpi.sessions"),
      value: new Intl.NumberFormat(locale).format(totals.sessions),
      trend: trendOf(totals.sessions, previous?.sessions, stats, tr),
      note: tn("stats.prompts", totals.turns),
    },
  ];
}

/* ── The chart ───────────────────────────────────────────────────── */

/**
 * The round number at or above `value` the axis tops out at, close enough
 * that the tallest column fills most of the plot; half of it, the middle
 * gridline, is round too.
 */
export function niceTop(value: number): number {
  if (!(value > 0)) return 1;
  const power = 10 ** Math.floor(Math.log10(value));
  for (const step of [1, 2, 3, 4, 5, 6, 8, 10]) if (step * power >= value * (1 - 1e-9)) return step * power;
  return 10 * power;
}

export interface Column {
  at: number;
  value: number;
  /** Of the axis, 0–1. */
  height: number;
}

export interface Chart {
  title: string;
  columns: Column[];
  /** The gridlines, bottom up: 0, half way, the top. */
  ticks: { value: number; label: string; at: number }[];
  /** Under the columns: every weekday for a week, else the first, middle and last. */
  axis: { index: number; label: string }[];
  /** Nothing at all in the period. */
  empty: boolean;
  /** The column the readout starts on: the last with anything in it. */
  initial: number;
  /** What it shows, in a sentence, for a screen reader. */
  summary: string;
}

const shortDate = (at: number, locale: string) => new Intl.DateTimeFormat(locale, { day: "numeric", month: "short" }).format(at);

export function chartOf(stats: RemoteStats, tr: Translator): Chart {
  const { t, locale } = tr;
  const { series, measure, granularity } = stats;
  const title = t(`stats.chart.${granularity}.${measure}`);
  const max = Math.max(0, ...series.map((point) => point.value));
  const top = niceTop(max);
  const columns = series.map((point) => ({ at: point.at, value: point.value, height: point.value / top }));
  const ticks = [0, 0.5, 1].map((at) => ({ value: top * at, label: formatTick(top * at, measure, locale), at }));
  const last = series.length - 1;
  const axis =
    series.length === 0
      ? []
      : granularity === "day" && series.length <= 7
      ? series.map((point, index) => ({
          index,
          label: new Intl.DateTimeFormat(locale, { weekday: "short" }).format(point.at),
        }))
      : [...new Set([0, Math.round(last / 2), last])]
          .filter((index) => index >= 0)
          .map((index) => ({ index, label: shortDate(series[index].at, locale) }));
  let initial = last;
  while (initial > 0 && series[initial].value === 0) initial--;
  const peak = series.reduce((best, point, index) => (point.value > series[best].value ? index : best), 0);
  const total = series.reduce((sum, point) => sum + point.value, 0);
  return {
    title,
    columns,
    ticks,
    axis,
    empty: max === 0,
    initial: Math.max(0, initial),
    summary: series.length
      ? t("stats.chart.summary", {
          title,
          from: shortDate(series[0].at, locale),
          to: shortDate(series[last].at, locale),
          total: measureText(total, measure, tr),
          peak: measureText(series[peak].value, measure, tr),
          date: shortDate(series[peak].at, locale),
        })
      : title,
  };
}

/** A column read out: its day (or week) and its value. */
export function readout(stats: RemoteStats, index: number, now: number, tr: Translator): { date: string; value: string } | null {
  const point = stats.series[index];
  if (!point) return null;
  return {
    date:
      stats.granularity === "week"
        ? tr.t("stats.chart.weekOf", { date: new Intl.DateTimeFormat(tr.locale, { day: "numeric", month: "long" }).format(point.at) })
        : dayHeading(point.at, now, tr.locale),
    value: measureText(point.value, stats.measure, tr),
  };
}

/** The column under a finger `x` pixels into a plot `width` wide. */
export function columnAt(x: number, width: number, count: number): number {
  if (count <= 0 || width <= 0) return -1;
  return Math.min(count - 1, Math.max(0, Math.floor((x / width) * count)));
}

/** Under the rankings, when some model had no price: how many tokens the cost leaves out. */
export function unpricedNote(stats: RemoteStats, { t, locale }: Translator): string | null {
  return stats.unpricedTokens > 0 ? t("stats.unpricedNote", { tokens: formatCompact(stats.unpricedTokens, locale) }) : null;
}

/* ── The rankings ────────────────────────────────────────────────── */

export interface RankRow {
  key: string;
  label: string;
  /** The measure's value, or "No price known" for a model without one, by cost. */
  value: string;
  share: string;
  /** Of the heaviest row, 0–100; a sliver for anything above nothing. */
  width: number;
  /** "12 sessions · 3 h 40" for a project or an agent. */
  sub: string | null;
  other: boolean;
}

export function rankRows(
  ranking: RemoteStatsRanking,
  measure: RemoteStatsMeasure,
  tr: Translator,
  { sessions = true }: { sessions?: boolean } = {},
): RankRow[] {
  const { t, tn, locale } = tr;
  const rows = [
    ...ranking.rows.map((row) => ({ ...row, other: false, count: 0 })),
    ...(ranking.other ? [{ ...ranking.other, key: "other", label: "", sessions: 0, other: true, unpriced: undefined }] : []),
  ];
  const max = Math.max(0, ...rows.map((row) => row[measure]));
  return rows.map((row) => {
    const value = row[measure];
    return {
      key: row.key,
      label: row.other ? tn("stats.others", row.count) : row.label,
      value: row.unpriced && measure === "cost" ? t("stats.unpriced") : measureText(value, measure, tr),
      share: formatShare(row.share, locale),
      width: max > 0 ? Math.max(value > 0 ? 0.6 : 0, (value / max) * 100) : 0,
      sub: sessions && !row.other ? `${tn("stats.sessions", row.sessions)} · ${activeTime(row.activeMs, tr)}` : null,
      other: row.other,
    };
  });
}
