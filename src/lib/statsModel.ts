import type { StatsAgent, StatsBucket, UsageStats } from "./stats";
import { prettyModel } from "./inspectorFormat";
import type { Translator } from "../i18n";
import { NBSP } from "./typography";
import type { Workspace } from "../types";

/**
 * Everything the statistics page computes, from the hourly rows stats.rs
 * returns to what each chart draws. Pure — the clock, the locale and the
 * workspaces come in as arguments — so every rule here has a test.
 */

export type Period = "7d" | "30d" | "90d" | "all";
export const PERIODS: Period[] = ["7d", "30d", "90d", "all"];
const PERIOD_DAYS: Record<Exclude<Period, "all">, number> = { "7d": 7, "30d": 30, "90d": 90 };

/** What the charts and rankings measure. */
export type Measure = "tokens" | "cost";

// ---------------------------------------------------------------------------
// Days
// ---------------------------------------------------------------------------

/** Local midnight of the day holding `at`. */
export function startOfDay(at: number): number {
  const day = new Date(at);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
}

/** `days` calendar days later, at the same local time — 23 or 25 hours across a clock change. */
export function addDays(at: number, days: number): number {
  const day = new Date(at);
  day.setDate(day.getDate() + days);
  return day.getTime();
}

/** Local midnight of the Monday starting the week holding `at`. */
export function startOfWeek(at: number): number {
  const day = new Date(startOfDay(at));
  return addDays(day.getTime(), -((day.getDay() + 6) % 7));
}

/** Monday 0 … Sunday 6. */
export const weekdayOf = (at: number) => (new Date(at).getDay() + 6) % 7;

export interface Range {
  from: number;
  /** Excluded: the midnight after today. */
  to: number;
  days: number;
  /** The same length just before, for the trends; none for "all". */
  previous: { from: number; to: number } | null;
}

/** Whole days up to and including today; "all" starts on the first day with data. */
export function periodRange(period: Period, now: number, firstAt: number | null): Range {
  const to = addDays(startOfDay(now), 1);
  if (period === "all") {
    const from = firstAt === null ? startOfDay(now) : Math.min(startOfDay(firstAt), startOfDay(now));
    let days = 0;
    for (let day = from; day < to; day = addDays(day, 1)) days++;
    return { from, to, days, previous: null };
  }
  const days = PERIOD_DAYS[period];
  const from = addDays(to, -days);
  return { from, to, days, previous: { from: addDays(from, -days), to: from } };
}

/** Past this many days, a column per day would be thinner than a hairline: weeks. */
export const WEEKLY_AFTER_DAYS = 120;

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

/** A folder the sessions of a workspace may have run in. */
export interface ProjectRoot {
  workspaceId: string;
  name: string;
  /** Normalized (see normalizePath). */
  path: string;
}

export interface Project {
  key: string;
  name: string;
  /** The folder shown under the name: the workspace's, or the session's own. */
  path: string;
  workspaceId: string | null;
}

/**
 * A path in one spelling: forward slashes, no trailing one, and lowercase
 * where the file system ignores case — a Windows drive or share — so that
 * `C:\Dev\App` from one agent and `c:/dev/app/` from another are one place.
 */
export function normalizePath(path: string): string {
  let clean = path.trim().replace(/\\/g, "/");
  while (clean.length > 1 && clean.endsWith("/") && !/^[a-z]:\/$/i.test(clean)) {
    clean = clean.slice(0, -1);
  }
  return /^[a-z]:\//i.test(clean) || clean.startsWith("//") ? clean.toLowerCase() : clean;
}

const lastSegment = (path: string) =>
  path.replace(/[\\/]+$/, "").split(/[\\/]/).filter(Boolean).pop() ?? path;

/**
 * Every folder a workspace's sessions may have started in: its own, the
 * other roots of a `.code-workspace`, its worktrees, and where its panes
 * run. A pane's folder above the workspace's own (a shell in the home
 * folder) is left out — it would claim every project under it.
 */
export function workspaceRoots(
  workspaces: Workspace[],
  worktrees: Record<string, string[]> = {},
): ProjectRoot[] {
  const roots: ProjectRoot[] = [];
  for (const ws of workspaces) {
    const own = normalizePath(ws.cwd);
    const paneDirs = ws.panes
      .map((pane) => pane.cwd)
      .filter((cwd): cwd is string => Boolean(cwd))
      .filter((cwd) => {
        const dir = normalizePath(cwd);
        return !own.startsWith(`${dir}/`);
      });
    const all = [ws.cwd, ...ws.extraRoots, ...(worktrees[ws.id] ?? []), ...paneDirs];
    for (const path of new Set(all.filter(Boolean).map(normalizePath))) {
      roots.push({ workspaceId: ws.id, name: ws.name, path });
    }
  }
  return roots;
}

/**
 * The project a session's folder belongs to: the workspace with the deepest
 * root above it, or the folder itself, named after its last segment.
 */
export function projectFor(cwd: string, roots: ProjectRoot[]): Project {
  const path = normalizePath(cwd);
  let best: ProjectRoot | null = null;
  for (const root of roots) {
    const inside = path === root.path || path.startsWith(`${root.path}/`);
    if (inside && (!best || root.path.length > best.path.length)) best = root;
  }
  if (best) {
    return { key: `ws:${best.workspaceId}`, name: best.name, path: best.path, workspaceId: best.workspaceId };
  }
  return { key: `dir:${path}`, name: cwd ? lastSegment(cwd) : "?", path: cwd, workspaceId: null };
}

// ---------------------------------------------------------------------------
// Summing up
// ---------------------------------------------------------------------------

export interface Totals {
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
  tokens: number;
  cost: number;
  costInput: number;
  costOutput: number;
  costCacheWrite: number;
  costCacheRead: number;
  activeMs: number;
  turns: number;
  calls: number;
  sessions: number;
}

const zero = (): Totals => ({
  input: 0,
  output: 0,
  cacheWrite: 0,
  cacheRead: 0,
  tokens: 0,
  cost: 0,
  costInput: 0,
  costOutput: 0,
  costCacheWrite: 0,
  costCacheRead: 0,
  activeMs: 0,
  turns: 0,
  calls: 0,
  sessions: 0,
});

function add(into: Totals, row: StatsBucket) {
  into.input += row.input;
  into.output += row.output;
  into.cacheWrite += row.cacheWrite;
  into.cacheRead += row.cacheRead;
  into.tokens += row.input + row.output + row.cacheWrite + row.cacheRead;
  into.cost += row.cost;
  into.costInput += row.costInput;
  into.costOutput += row.costOutput;
  into.costCacheWrite += row.costCacheWrite;
  into.costCacheRead += row.costCacheRead;
  into.activeMs += row.activeMs;
  into.turns += row.turns;
  into.calls += row.calls;
}

export const measureOf = (totals: Pick<Totals, "tokens" | "cost">, measure: Measure) =>
  measure === "cost" ? totals.cost : totals.tokens;

/** One column of the chart: a day, or a week past WEEKLY_AFTER_DAYS. */
export interface Point extends Totals {
  at: number;
}

/** A row of a ranking: a project, an agent, a model. */
export interface Ranked extends Totals {
  key: string;
  label: string;
  /** A second line: the project's folder. */
  detail: string;
  /** Of the measure, 0–1. */
  share: number;
  /** A model the price table does not know: no cost, said so. */
  unpriced?: boolean;
}

export interface SessionRow extends Totals {
  index: number;
  id: string;
  agent: StatsAgent;
  title: string;
  project: Project;
  /** The model that did most of its work in the period. */
  model: string | null;
  lastAt: number;
}

export interface Summary {
  range: Range;
  granularity: "day" | "week";
  totals: Totals;
  /** The period before, for the trends; null for "all". */
  previous: Totals | null;
  series: Point[];
  /** Every project of the period, whatever the filter, so any can be picked. */
  projects: Ranked[];
  agents: Ranked[];
  models: Ranked[];
  /** Active time by weekday (Monday first) and hour. */
  heat: number[][];
  heatMax: number;
  /** The heaviest sessions of the period, by the measure. */
  sessions: SessionRow[];
  /** Tokens of models without a price, left out of the cost. */
  unpricedTokens: number;
  /** Days of the period with any active time. */
  activeDays: number;
  /** The first hour with anything in it, over all time. */
  firstAt: number | null;
}

export interface SummaryOptions {
  period: Period;
  /** A project key, or null for all of them. */
  project: string | null;
  measure: Measure;
  now: number;
  /** The project of each session, by index (see projectFor). */
  projects: Project[];
  /** Display names of the agents. */
  agentLabel: (agent: StatsAgent) => string;
  topSessions?: number;
}

/**
 * A model's name as people say it: "Opus 5.5" for a Claude id (see
 * prettyModel), "GPT-5.6 Sol" for an OpenAI one, and anything else exactly as
 * the transcript wrote it.
 */
export function modelLabel(id: string): string {
  const gpt = /^gpt-(\d[\d.]*)((?:-[a-z0-9]+)*)$/i.exec(id.trim());
  if (gpt) {
    const words = gpt[2]
      .split("-")
      .filter(Boolean)
      .map((word) => word.charAt(0).toUpperCase() + word.slice(1));
    return [`GPT-${gpt[1]}`, ...words].join(" ");
  }
  return prettyModel(id) ?? id;
}

/** The first hour with anything in it. */
export const firstActivity = (stats: UsageStats): number | null =>
  stats.buckets.length ? Math.min(...stats.buckets.map((row) => row.at)) : null;

function rank(
  entries: Map<string, Ranked>,
  measure: Measure,
  total: number,
): Ranked[] {
  const rows = [...entries.values()];
  for (const row of rows) row.share = total > 0 ? measureOf(row, measure) / total : 0;
  return rows.sort(
    (a, b) =>
      measureOf(b, measure) - measureOf(a, measure) ||
      b.activeMs - a.activeMs ||
      a.label.localeCompare(b.label),
  );
}

export function summarize(stats: UsageStats, options: SummaryOptions): Summary {
  const firstAt = firstActivity(stats);
  const range = periodRange(options.period, options.now, firstAt);
  const granularity = range.days > WEEKLY_AFTER_DAYS ? "week" : "day";
  const columnOf = granularity === "week" ? startOfWeek : startOfDay;

  // Every column of the period, empty ones included: a quiet day is a gap
  // in the chart, not a day that never happened.
  const series: Point[] = [];
  const column = new Map<number, Point>();
  for (let at = columnOf(range.from); at < range.to; at = addDays(at, granularity === "week" ? 7 : 1)) {
    const point = { ...zero(), at };
    series.push(point);
    column.set(at, point);
  }

  const inRange = (at: number) => at >= range.from && at < range.to;
  const previousRange = range.previous;
  const inPrevious = (at: number) =>
    previousRange !== null && at >= previousRange.from && at < previousRange.to;
  const projectOf = (session: number) => options.projects[session];
  const picked = (session: number) =>
    options.project === null || projectOf(session)?.key === options.project;

  const totals = zero();
  const previous = previousRange ? zero() : null;
  const sessionsNow = new Set<number>();
  const sessionsBefore = new Set<number>();
  const projects = new Map<string, Ranked>();
  const projectSessions = new Map<string, Set<number>>();
  const agents = new Map<string, Ranked>();
  const agentSessions = new Map<string, Set<number>>();
  const models = new Map<string, Ranked>();
  const perSession = new Map<number, SessionRow>();
  const sessionModels = new Map<number, Map<string, number>>();
  const heat = Array.from({ length: 7 }, () => new Array<number>(24).fill(0));
  let unpricedTokens = 0;
  const activeDays = new Set<number>();
  const columnSessions = new Map<number, Set<number>>();

  const ranked = (into: Map<string, Ranked>, key: string, label: string, detail = ""): Ranked => {
    let row = into.get(key);
    if (!row) {
      row = { ...zero(), key, label, detail, share: 0 };
      into.set(key, row);
    }
    return row;
  };
  const counted = <K,>(into: Map<K, Set<number>>, key: K, session: number) => {
    let set = into.get(key);
    if (!set) into.set(key, (set = new Set()));
    set.add(session);
  };

  for (const row of stats.buckets) {
    const session = stats.sessions[row.session];
    if (!session) continue;
    if (inPrevious(row.at) && previous && picked(row.session)) {
      add(previous, row);
      sessionsBefore.add(row.session);
    }
    if (!inRange(row.at)) continue;

    // The project ranking ignores the project filter: it is how one is picked.
    const project = projectOf(row.session);
    if (project) {
      add(ranked(projects, project.key, project.name, project.path), row);
      counted(projectSessions, project.key, row.session);
    }
    if (!picked(row.session)) continue;

    add(totals, row);
    sessionsNow.add(row.session);
    const point = column.get(columnOf(row.at));
    if (point) {
      add(point, row);
      counted(columnSessions, point.at, row.session);
    }
    if (row.activeMs > 0) activeDays.add(startOfDay(row.at));

    add(ranked(agents, session.agent, options.agentLabel(session.agent)), row);
    counted(agentSessions, session.agent, row.session);

    const tokens = row.input + row.output + row.cacheWrite + row.cacheRead;
    if (row.model >= 0) {
      const model = stats.models[row.model];
      const id = model?.id ?? "unknown";
      const label = modelLabel(id);
      const entry = ranked(models, id, label, label === id ? "" : id);
      add(entry, row);
      if (model && !model.priced) {
        entry.unpriced = true;
        unpricedTokens += tokens;
      }
      let byModel = sessionModels.get(row.session);
      if (!byModel) sessionModels.set(row.session, (byModel = new Map()));
      byModel.set(id, (byModel.get(id) ?? 0) + tokens);
    }

    if (row.activeMs > 0) heat[weekdayOf(row.at)][new Date(row.at).getHours()] += row.activeMs;

    let entry = perSession.get(row.session);
    if (!entry) {
      entry = {
        ...zero(),
        index: row.session,
        id: session.id,
        agent: session.agent,
        title: session.title,
        project: project ?? { key: "", name: "", path: session.cwd, workspaceId: null },
        model: null,
        lastAt: session.lastAt,
      };
      perSession.set(row.session, entry);
    }
    add(entry, row);
  }

  totals.sessions = sessionsNow.size;
  for (const point of series) point.sessions = columnSessions.get(point.at)?.size ?? 0;
  if (previous) previous.sessions = sessionsBefore.size;
  for (const [key, set] of projectSessions) projects.get(key)!.sessions = set.size;
  for (const [key, set] of agentSessions) agents.get(key)!.sessions = set.size;

  const projectTotal = [...projects.values()].reduce((sum, row) => sum + measureOf(row, options.measure), 0);
  const measured = measureOf(totals, options.measure);

  const sessions = [...perSession.values()]
    .filter((row) => row.calls > 0 || row.activeMs > 0)
    .sort(
      (a, b) =>
        measureOf(b, options.measure) - measureOf(a, options.measure) || b.activeMs - a.activeMs,
    )
    .slice(0, options.topSessions ?? 8);
  for (const row of sessions) {
    row.sessions = 1;
    const byModel = sessionModels.get(row.index);
    if (byModel?.size) {
      const [top] = [...byModel.entries()].sort((a, b) => b[1] - a[1])[0];
      row.model = modelLabel(top);
    }
  }

  return {
    range,
    granularity,
    totals,
    previous,
    series,
    projects: rank(projects, options.measure, projectTotal),
    agents: rank(agents, options.measure, measured),
    models: rank(models, options.measure, measured),
    heat,
    heatMax: Math.max(0, ...heat.flat()),
    sessions,
    unpricedTokens,
    activeDays: activeDays.size,
    firstAt,
  };
}

/** The heatmap's five steps above empty, by the square root of the value: a few long days must not wash every other hour out. */
export function heatLevel(value: number, max: number): number {
  if (value <= 0 || max <= 0) return 0;
  return Math.min(5, Math.max(1, Math.ceil(Math.sqrt(value / max) * 5)));
}

/** Relative change, or null when there was nothing to compare with. */
export function change(now: number, before: number): number | null {
  if (before <= 0) return null;
  return (now - before) / before;
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

const formats = new Map<string, Intl.NumberFormat>();
function format(locale: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  const key = `${locale}|${JSON.stringify(options)}`;
  let found = formats.get(key);
  if (!found) {
    found = new Intl.NumberFormat(locale, options);
    formats.set(key, found);
  }
  return found;
}

/**
 * "1,2 M", "845 k", "12 Md" in French; "1.2M", "845K", "12B" in English.
 * One decimal below a hundred, none above: three digits at most.
 */
export function formatCompact(value: number, locale: string): string {
  // `roundingPriority` is ES2023, newer than the lib this compiles against;
  // every web view the app runs in has it.
  const options: Intl.NumberFormatOptions & { roundingPriority: string } = {
    notation: "compact",
    maximumFractionDigits: 1,
    maximumSignificantDigits: 3,
    roundingPriority: "lessPrecision",
  };
  return format(locale, options).format(value);
}

/** A count in full, grouped the language's way: "1 284 093". */
export const formatInteger = (value: number, locale: string) =>
  format(locale, { maximumFractionDigits: 0 }).format(value);

/**
 * Dollars at a glance: cents under ten, whole dollars up to a hundred
 * thousand, then compact — "0,84 $", "59 $", "1 284 $", "312 k$". `exact`
 * keeps the cents, for a table or a readout where the figure is the point.
 */
export function formatUsd(value: number, locale: string, exact = false): string {
  const base: Intl.NumberFormatOptions = {
    style: "currency",
    currency: "USD",
    currencyDisplay: "narrowSymbol",
  };
  if (!exact && value >= 100_000) {
    return format(locale, { ...base, notation: "compact", maximumSignificantDigits: 3 }).format(value);
  }
  const cents = exact || value < 10;
  return format(locale, {
    ...base,
    minimumFractionDigits: cents ? 2 : 0,
    maximumFractionDigits: cents ? 2 : 0,
  }).format(value);
}

/** An axis tick: as short as it can be and still distinct. */
export function formatTick(value: number, measure: Measure, locale: string): string {
  if (measure === "tokens") return formatCompact(value, locale);
  return format(locale, {
    style: "currency",
    currency: "USD",
    currencyDisplay: "narrowSymbol",
    notation: "compact",
    maximumFractionDigits: value < 10 ? 1 : 0,
  }).format(value);
}

/** "+24 %", "−8 %": a trend, signed. */
export const formatChange = (ratio: number, locale: string) =>
  format(locale, { style: "percent", maximumFractionDigits: 0, signDisplay: "exceptZero" }).format(ratio);

/** "34 %": a share. Under 1 % says so rather than rounding to nothing. */
export function formatShare(ratio: number, locale: string): string {
  const percent = format(locale, { style: "percent", maximumFractionDigits: 0 });
  if (ratio > 0 && ratio < 0.01) return `<${NBSP}${percent.format(0.01)}`;
  return percent.format(ratio);
}

/** "45 min", "3 h 12", "142 h". */
export function formatDuration(ms: number, t: Translator): string {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 1) return ms > 0 ? t("stats.durationUnderMinute") : t("stats.durationMinutes", { m: 0 });
  if (minutes < 60) return t("stats.durationMinutes", { m: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours >= 100) return t("stats.durationHoursOnly", { h: hours });
  return t("stats.durationHours", { h: hours, m: String(minutes % 60).padStart(2, "0") });
}
