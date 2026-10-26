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

const startOfDay = (at: number) => {
  const day = new Date(at);
  day.setHours(0, 0, 0, 0);
  return day.getTime();
};

/**
 * A time of day inside a sentence: "20 h 01" in French, the way French
 * typography writes it in running text, and "8:01 PM" in English. A column
 * of times, where they have to line up, keeps clockTime.
 */
export function proseTime(at: number, locale: string): string {
  const date = new Date(at);
  if (locale === "fr") {
    const minutes = String(date.getMinutes()).padStart(2, "0");
    return `${date.getHours()} h ${minutes}`;
  }
  return date.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" });
}

const shortDate = (at: number, locale: string) =>
  new Date(at).toLocaleDateString(locale, { day: "numeric", month: "short" });

/** Whole calendar days from `from` to `to`; negative when `to` is earlier. */
const daysBetween = (from: number, to: number) =>
  Math.round((startOfDay(to) - startOfDay(from)) / 86_400_000);

/**
 * When something started, worded for a glance: "12 min ago" within the hour,
 * then "today at 13 h 18", "yesterday at 18 h 02", and a date beyond that.
 */
export function formatSince(at: number, now: number, t: Translator, locale: string): string {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return t("inspector.justNow");
  if (minutes < 60) return t("inspector.minutesAgo", { n: minutes });
  const time = proseTime(at, locale);
  const days = daysBetween(at, now);
  if (days === 0) return t("inspector.todayAt", { time });
  if (days === 1) return t("inspector.yesterdayAt", { time });
  return t("inspector.dateAt", { date: shortDate(at, locale), time });
}

/**
 * When a plan window frees up, as the end of a sentence: "remise à zéro à
 * 20 h 01" today, "demain à 1 h 05", "lundi à 9 h 00" within the week, a
 * date beyond. The navigator and the inspector both say it this way.
 */
export function resetWhen(at: number, now: number, t: Translator, locale: string): string {
  const time = proseTime(at, locale);
  const days = daysBetween(now, at);
  if (days <= 0) return t("sidebar.planResetToday", { time });
  if (days === 1) return t("sidebar.planResetTomorrow", { time });
  if (days < 7) {
    const day = new Date(at).toLocaleDateString(locale, { weekday: "long" });
    return t("sidebar.planResetDay", { day, time });
  }
  return t("sidebar.planResetDate", { date: shortDate(at, locale), time });
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
