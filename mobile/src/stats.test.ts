import { describe, expect, it } from "vitest";
import { addDays, startOfDay } from "../lib/statsModel";
import type { RemoteStats, RemoteStatsTotals } from "../remote/types";
import { translator } from "./i18n";
import {
  activeTime,
  chartOf,
  columnAt,
  kpis,
  measureText,
  niceTop,
  planView,
  rankRows,
  readout,
  resetText,
  trendOf,
  unpricedNote,
  when,
} from "./stats";

const en = translator("en");
const fr = translator("fr");
/** Spaces as a reader sees them: the no-break ones the languages put in are spaces too. */
const plain = (text: string | null | undefined) => text?.replace(/\s/g, " ") ?? text;

const MIN = 60_000;
const HOUR = 60 * MIN;
// A Thursday afternoon.
const NOW = new Date(2026, 9, 29, 15, 0).getTime();
const today = startOfDay(NOW);

const totals = (patch: Partial<RemoteStatsTotals> = {}): RemoteStatsTotals => ({
  tokens: 0,
  cacheRead: 0,
  cost: 0,
  activeMs: 0,
  sessions: 0,
  turns: 0,
  ...patch,
});

function stats(patch: Partial<RemoteStats> = {}): RemoteStats {
  const days = 7;
  return {
    period: "7d",
    measure: "tokens",
    readAt: NOW - 2 * MIN,
    cachedOnly: false,
    range: { from: addDays(today, -6), to: addDays(today, 1), days },
    granularity: "day",
    firstAt: addDays(today, -40),
    totals: totals({ tokens: 4_500_000, cacheRead: 3_600_000, cost: 12.5, activeMs: 3 * HOUR + 12 * MIN, sessions: 9, turns: 41 }),
    previous: totals({ tokens: 3_000_000, cost: 12.5, activeMs: 3 * HOUR + 12 * MIN, sessions: 0, turns: 30 }),
    activeDays: 4,
    series: Array.from({ length: days }, (_, at) => ({ at: addDays(today, at - 6), value: [0, 1_200_000, 0, 800_000, 900_000, 1_600_000, 0][at] })),
    projects: { rows: [], other: null },
    agents: { rows: [], other: null },
    models: { rows: [], other: null },
    unpricedTokens: 0,
    plan: null,
    ...patch,
  };
}

describe("the plan's windows", () => {
  const plan = (windows: NonNullable<RemoteStats["plan"]>["windows"], at = NOW - 3 * MIN) => ({ at, windows });

  it("are left out until a session has reported them", () => {
    expect(planView(null, NOW, en)).toBeNull();
    expect(planView(plan([]), NOW, en)).toBeNull();
  });

  it("show the 5-hour window first, the week next, with what is left and when each resets", () => {
    const view = planView(
      plan([
        { kind: "seven_day", percentUsed: 38, resetsAt: addDays(today, 2) + 9 * HOUR },
        { kind: "seven_day_opus", percentUsed: 5, resetsAt: null },
        { kind: "five_hour", percentUsed: 72, resetsAt: NOW + 107 * MIN },
      ]),
      NOW,
      en,
    )!;
    expect(view.bars.map((bar) => [bar.label, bar.percent, bar.tone])).toEqual([
      ["5-hour window", 72, "warn"],
      ["Weekly window", 38, "normal"],
      ["Seven day opus", 5, "normal"],
    ]);
    expect(view.bars.map((bar) => [plain(bar.left), plain(bar.used)])).toEqual([
      ["28% left", "72% used"],
      ["62% left", "38% used"],
      ["95% left", "5% used"],
    ]);
    expect(plain(view.bars[0].reset)).toBe("resets at 4:47 PM");
    expect(plain(view.bars[1].reset)).toBe("resets Saturday at 9:00 AM");
    expect(view.bars[2].reset).toBeNull();
    expect(plain(view.reported)).toBe("Reported by a Claude Code session 3 min ago");
  });

  it("speak French", () => {
    const view = planView(plan([{ kind: "five_hour", percentUsed: 93, resetsAt: NOW + 30 * MIN }]), NOW, fr)!;
    expect([plain(view.bars[0].label), view.bars[0].tone]).toEqual(["Fenêtre de 5 h", "danger"]);
    expect(plain(view.bars[0].left)).toBe("7 % restants");
    expect(plain(view.bars[0].reset)).toBe("remise à zéro à 15:30");
  });

  it("say when a window has freed up since the report, rather than its old figure", () => {
    const view = planView(plan([{ kind: "five_hour", percentUsed: 98, resetsAt: NOW - 20 * MIN }], NOW - 2 * HOUR), NOW, en)!;
    expect(view.bars[0]).toMatchObject({ percent: 0, tone: "normal", used: null, left: null, past: true });
    expect(plain(view.bars[0].reset)).toBe("Reset 20 min ago: no session has reported since");
    expect(plain(view.reported)).toBe("Reported by a Claude Code session 2 h ago");
  });

  it("reset tomorrow, within the week, or on a date", () => {
    expect(plain(resetText(addDays(today, 1) + HOUR + 5 * MIN, NOW, en))).toBe("resets tomorrow at 1:05 AM");
    expect(plain(resetText(addDays(today, 9) + 9 * HOUR, NOW, fr))).toBe("remise à zéro le 7 novembre à 9:00");
  });
});

describe("the totals", () => {
  it("say each figure the desktop's way, with the trend against the days before", () => {
    const [tokens, cost, active, sessions] = kpis(stats(), en);
    expect([tokens.label, plain(tokens.value), plain(tokens.trend?.text), tokens.trend?.direction]).toEqual([
      "Tokens",
      "4.5M",
      "+50% vs the 7 days before",
      "up",
    ]);
    expect(plain(tokens.note)).toBe("80% read from the cache");
    expect([cost.label, cost.value, plain(cost.trend?.text), cost.trend?.direction]).toEqual([
      "Cost, API equivalent",
      "$13",
      "same as the 7 days before",
      null,
    ]);
    expect(cost.note).toBe("At API prices, not what your plan costs");
    expect([plain(active.value), plain(active.note)]).toEqual(["3 h 12", "48 min per active day"]);
    expect([sessions.value, plain(sessions.trend?.text), sessions.note]).toEqual(["9", "nothing the 7 days before", "41 prompts"]);
  });

  it("go down too, and stay quiet when there was nothing either time", () => {
    const quiet = stats({ totals: totals({ tokens: 1_000 }), previous: totals({ tokens: 2_000 }) });
    expect(trendOf(1_000, 2_000, quiet, en)).toEqual({ text: "-50% vs the 7 days before", direction: "down" });
    expect(trendOf(0, 0, quiet, en)).toBeNull();
    const [, , active] = kpis(quiet, en);
    expect(plain(active.note)).toBe("0 min per active day");
    expect(kpis(stats({ activeDays: 0 }), en)[2].note).toBeNull();
  });

  it("count from the first day for all of it", () => {
    const all = stats({ period: "all", previous: null, firstAt: new Date(2026, 5, 3).getTime() });
    expect(kpis(all, en)[0].trend).toEqual({ text: "since June 3, 2026", direction: null });
    expect(kpis(all, fr)[0].trend?.text).toBe("depuis le 3 juin 2026");
    expect(kpis(stats({ previous: null, firstAt: null }), en)[0].trend).toBeNull();
  });

  it("speak French, with the language's spaces", () => {
    const [tokens, cost, , sessions] = kpis(stats({ totals: totals({ tokens: 1_234_567, cost: 1_284.4, turns: 1 }) }), fr);
    expect(plain(tokens.value)).toBe("1,2 M");
    expect(plain(cost.value)).toBe("1 284 $");
    expect(sessions.note).toBe("1 prompt");
    expect(cost.label).toBe("Coût, équivalent API");
  });
});

describe("the chart", () => {
  it("tops its axis at a round number", () => {
    expect([0, 0.95, 1, 1.3, 2.2, 3, 5.5, 7_400, 57_000_000, 1_600_000].map(niceTop)).toEqual([
      1, 1, 1, 2, 3, 3, 6, 8_000, 60_000_000, 2_000_000,
    ]);
  });

  it("draws every day of the week against that top, each weekday named, read out on the last day with anything", () => {
    const chart = chartOf(stats(), en);
    expect(chart.title).toBe("Tokens per day");
    expect(chart.columns.map((column) => column.height)).toEqual([0, 0.6, 0, 0.4, 0.45, 0.8, 0]);
    expect(chart.ticks.map((tick) => [tick.at, plain(tick.label)])).toEqual([
      [0, "0"],
      [0.5, "1M"],
      [1, "2M"],
    ]);
    expect(chart.axis.map((label) => label.label)).toEqual(["Fri", "Sat", "Sun", "Mon", "Tue", "Wed", "Thu"]);
    expect(chart.initial).toBe(5);
    expect(chart.empty).toBe(false);
    expect(plain(chart.summary)).toBe("Tokens per day, Oct 23 to Oct 29: 4.5M tokens in all, at most 1.6M tokens on Oct 28.");
  });

  it("names the first, middle and last of a longer period, and draws cost in dollars", () => {
    const series = Array.from({ length: 30 }, (_, at) => ({ at: addDays(today, at - 29), value: at === 12 ? 18.4 : 2 }));
    const chart = chartOf(stats({ measure: "cost", period: "30d", series }), fr);
    expect(chart.title).toBe("Coût par jour, équivalent API");
    expect(chart.axis.map((label) => [label.index, plain(label.label)])).toEqual([
      [0, "30 sept."],
      [15, "15 oct."],
      [29, "29 oct."],
    ]);
    expect(chart.ticks.map((tick) => plain(tick.label))).toEqual(["0 $", "10 $", "20 $"]);
  });

  it("says when the period holds nothing", () => {
    const chart = chartOf(stats({ series: stats().series.map((point) => ({ ...point, value: 0 })) }), en);
    expect(chart.empty).toBe(true);
    expect(chart.initial).toBe(0);
  });

  it("reads a day out by its name, a week by its first day", () => {
    expect(readout(stats(), 6, NOW, en)).toEqual({ date: "Today", value: "0 tokens" });
    expect(plain(readout(stats(), 5, NOW, en)?.value)).toBe("1.6M tokens");
    expect(readout(stats(), 5, NOW, fr)?.date).toBe("Hier");
    const weekly = stats({ granularity: "week", series: [{ at: new Date(2026, 2, 2).getTime(), value: 5 }], measure: "cost" });
    expect(readout(weekly, 0, NOW, en)).toEqual({ date: "Week of March 2", value: "$5.00" });
    expect(readout(stats(), 9, NOW, en)).toBeNull();
  });

  it("finds the column under a finger, never past either end", () => {
    expect(columnAt(0, 300, 30)).toBe(0);
    expect(columnAt(155, 300, 30)).toBe(15);
    expect(columnAt(299.9, 300, 30)).toBe(29);
    expect(columnAt(-12, 300, 30)).toBe(0);
    expect(columnAt(340, 300, 30)).toBe(29);
    expect(columnAt(10, 300, 0)).toBe(-1);
  });
});

describe("the rankings", () => {
  const row = (key: string, tokens: number, cost: number, share: number, extra: object = {}) => ({
    key,
    label: key,
    tokens,
    cost,
    activeMs: 2 * HOUR + 5 * MIN,
    sessions: 3,
    share,
    ...extra,
  });

  it("measure each row against the heaviest, with its share, sessions and active time", () => {
    const rows = rankRows(
      { rows: [row("IaBench", 8_000_000, 40, 0.8), row("storefront", 1_000_000, 9, 0.1)], other: { count: 3, tokens: 1_000_000, cost: 1, activeMs: HOUR, share: 0.1 } },
      "tokens",
      en,
    );
    expect(rows.map((entry) => [entry.label, plain(entry.value), plain(entry.share), entry.width, entry.other])).toEqual([
      ["IaBench", "8M tokens", "80%", 100, false],
      ["storefront", "1M tokens", "10%", 12.5, false],
      ["3 others", "1M tokens", "10%", 12.5, true],
    ]);
    expect(plain(rows[0].sub)).toBe("3 sessions · 2 h 05");
    expect(rows[2].sub).toBeNull();
  });

  it("say a model without a price has none, by cost, and give a sliver to anything above nothing", () => {
    const rows = rankRows(
      { rows: [row("Opus 5.5", 9_000_000, 50, 0.99), row("Haiku 5.5", 10, 0.0001, 0.01), row("qwen3", 500, 0, 0, { unpriced: true })], other: null },
      "cost",
      fr,
      { sessions: false },
    );
    expect(rows.map((entry) => [plain(entry.value), entry.width])).toEqual([
      ["50 $", 100],
      ["0,00 $", 0.6],
      ["Prix inconnu", 0],
    ]);
    expect(rows.every((entry) => entry.sub === null)).toBe(true);
    expect(plain(rankRows({ rows: [], other: { count: 1, tokens: 5, cost: 0, activeMs: 0, share: 1 } }, "tokens", fr)[0].label)).toBe("1 autre");
  });

  it("note the tokens left out of the cost", () => {
    expect(unpricedNote(stats(), en)).toBeNull();
    expect(plain(unpricedNote(stats({ unpricedTokens: 2_400_000 }), en))).toBe(
      "2.4M tokens of models without a known price are left out of the cost.",
    );
  });
});

describe("the words around the numbers", () => {
  it("say active time as the desktop does", () => {
    expect([0, 20_000, 45 * MIN, 3 * HOUR + 12 * MIN, 142 * HOUR].map((ms) => plain(activeTime(ms, en)))).toEqual([
      "0 min",
      "< 1 min",
      "45 min",
      "3 h 12",
      "142 h",
    ]);
  });

  it("say tokens and dollars", () => {
    expect(plain(measureText(1_500, "tokens", en))).toBe("1.5K tokens");
    expect(plain(measureText(0.84, "cost", fr))).toBe("0,84 $");
  });

  it("say when something was read: just now, minutes ago, at a time today, a day before", () => {
    expect(when(NOW - 10_000, NOW, en)).toBe("just now");
    expect(plain(when(NOW - 12 * MIN, NOW, en))).toBe("12 min ago");
    expect(plain(when(today + 8 * HOUR, NOW, en))).toBe("at 8:00 AM");
    expect(plain(when(today + 8 * HOUR, NOW, fr))).toBe("à 8:00");
    expect(plain(when(today - 2 * HOUR, NOW, en))).toBe("yesterday 10:00 PM");
  });
});
