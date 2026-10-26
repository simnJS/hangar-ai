import type { RateLimit } from "./bridge";
import type { Translator } from "../i18n";

/**
 * Wording and number formats of the inspector. Pure — the translator and the
 * locale come in as arguments — so every rule here has a test.
 */

export type Tone = "normal" | "warn" | "danger";

/** Amber from 70 %, red from 90 %: the same reading for context and plan. */
export function usageTone(pct: number): Tone {
  if (pct >= 90) return "danger";
  if (pct >= 70) return "warn";
  return "normal";
}

const FAMILIES = ["opus", "sonnet", "haiku", "fable"];
const capitalize = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);

/**
 * "claude-sonnet-4-5-20250929" → "Sonnet 4.5", for the header's sub-line.
 * The Session section still shows the id as the agent gave it; anything not
 * recognised as a Claude model is returned untouched rather than guessed at.
 */
export function prettyModel(model: string | null): string | null {
  if (!model) return null;
  const id = model.trim().toLowerCase().replace(/\[.*\]$/, "");
  // Current naming: family first, then the version.
  const modern =
    /(?:^|claude-)(opus|sonnet|haiku|fable)-(\d+)(?:[-.](\d{1,2}))?(?:-\d{8})?$/.exec(id);
  if (modern) {
    const [, family, major, minor] = modern;
    return `${capitalize(family)} ${minor ? `${major}.${minor}` : major}`;
  }
  // The Claude 3 naming put the version first.
  const legacy = /^claude-(\d+)(?:-(\d))?-(opus|sonnet|haiku)/.exec(id);
  if (legacy) {
    const [, major, minor, family] = legacy;
    return `${capitalize(family)} ${minor ? `${major}.${minor}` : major}`;
  }
  if (FAMILIES.includes(id)) return capitalize(id);
  return model.trim();
}

/**
 * Three bars for a priority that runs 0–9: 0 is low, 1 the board's default,
 * anything above it high — the board itself only flags priorities above 1.
 */
export function priorityBars(priority: number): 1 | 2 | 3 {
  if (priority <= 0) return 1;
  if (priority === 1) return 2;
  return 3;
}

/** Board ids are UUIDs; six characters tell them apart on one board. */
export function shortTaskId(id: string): string {
  return id.length > 8 ? id.slice(0, 6) : id;
}

/**
 * "14:32", on a 24-hour clock whatever the locale: the activity column is
 * five characters wide, and "04:59 PM" would wrap in it.
 */
export function clockTime(at: number, locale: string): string {
  return new Date(at).toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  });
}

/**
 * "1,84 $" in French, "$1.84" in English: the narrow symbol, since the
 * inspector has no room for "US$" and every agent bills in dollars.
 */
export function formatCost(usd: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: "USD",
    currencyDisplay: "narrowSymbol",
    minimumFractionDigits: 2,
    maximumFractionDigits: usd > 0 && usd < 0.01 ? 3 : 2,
  }).format(usd);
}

/** "34 %" in French, "34%" in English. */
export function formatPercent(pct: number, locale: string): string {
  return new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 }).format(
    pct / 100,
  );
}

/**
 * formatTokens' "84k", with the space French puts between a number and its
 * unit ("84 k") — narrow and unbreakable, so the pair never wraps apart.
 */
export function spacedUnit(formatted: string, locale: string): string {
  return locale === "fr" ? formatted.replace(/(\d)([kM])$/, "$1\u202f$2") : formatted;
}

/** "1 h 12", "25 min": how long something has been going on. */
export function formatElapsed(ms: number, t: Translator): string {
  const minutes = Math.floor(Math.max(0, ms) / 60_000);
  if (minutes < 1) return t("inspector.elapsedNow");
  if (minutes < 60) return t("inspector.elapsedMinutes", { m: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return t("inspector.elapsedHours", { h: hours, m: String(minutes % 60).padStart(2, "0") });
  }
  return t("inspector.elapsedDays", { d: Math.floor(hours / 24), h: hours % 24 });
}

const startOfDay = (at: number) => {
  const day = new Date(at);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
};

/**
 * When something started, worded for a glance: "12 min ago" within the hour,
 * then "today 13:18", "yesterday 18:02", and a date beyond that.
 */
export function formatSince(at: number, now: number, t: Translator, locale: string): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return t("inspector.justNow");
  if (minutes < 60) return t("inspector.minutesAgo", { n: minutes });
  const time = clockTime(at, locale);
  const days = Math.round((startOfDay(now) - startOfDay(at)) / 86_400_000);
  if (days === 0) return t("inspector.todayAt", { time });
  if (days === 1) return t("inspector.yesterdayAt", { time });
  const date = new Date(at).toLocaleDateString(locale, { day: "numeric", month: "short" });
  return t("inspector.dateAt", { date, time });
}

/** A plan window's usage, 0–100, or null when no session reported it. */
export function planUsage(
  limits: RateLimit[] | null | undefined,
  kind: string,
): RateLimit | null {
  const hit = limits?.find((limit) => limit.kind === kind);
  if (!hit) return null;
  return { ...hit, percentUsed: Math.max(0, Math.min(100, Math.round(hit.percentUsed))) };
}
