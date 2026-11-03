import type { Locale } from "./i18n";

/**
 * Times and sizes as the phone shows them: short, in the phone's language,
 * with a no-break space between a number and its unit.
 */

const NBSP = " ";
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

const WORDS = {
  en: { now: "just now", ago: "{t} ago", yesterday: "yesterday", today: "Today", yesterdayDay: "Yesterday", day: "d" },
  fr: { now: "à l'instant", ago: "il y a {t}", yesterday: "hier", today: "Aujourd'hui", yesterdayDay: "Hier", day: "j" },
} as const;

/** A length of time: 40 s, 12 min, 3 h, 2 d (2 j). */
export function duration(ms: number, locale: Locale): string {
  const span = Math.max(0, ms);
  if (span < MINUTE) return `${Math.max(1, Math.floor(span / 1000))}${NBSP}s`;
  if (span < HOUR) return `${Math.floor(span / MINUTE)}${NBSP}min`;
  if (span < DAY) return `${Math.floor(span / HOUR)}${NBSP}h`;
  return `${Math.floor(span / DAY)}${NBSP}${WORDS[locale].day}`;
}

const startOfDay = (at: number) => {
  const date = new Date(at);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
};

/** Whole calendar days from `at` to `now`: 0 today, 1 yesterday. */
export function daysBefore(at: number, now: number): number {
  return Math.round((startOfDay(now) - startOfDay(at)) / DAY);
}

/** The time of day: 14:32, 2:32 PM. */
export function clock(at: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(at);
}

/**
 * A moment: its time today, "yesterday 14:32", a weekday within the week, a
 * date beyond — for "unreachable since…" and the history's "since…".
 */
export function moment(at: number, now: number, locale: Locale): string {
  const days = daysBefore(at, now);
  if (days <= 0) return clock(at, locale);
  if (days === 1) return `${WORDS[locale].yesterday} ${clock(at, locale)}`;
  const options: Intl.DateTimeFormatOptions =
    days < 7 ? { weekday: "long" } : { day: "numeric", month: "short" };
  return `${new Intl.DateTimeFormat(locale, options).format(at)} ${clock(at, locale)}`;
}

/** How long ago: just now, 12 min ago, 3 h ago, then the moment itself. */
export function ago(at: number, now: number, locale: Locale): string {
  const span = now - at;
  if (span < 45_000) return WORDS[locale].now;
  if (span < HOUR || (span < 6 * HOUR && daysBefore(at, now) === 0)) {
    return WORDS[locale].ago.replace("{t}", duration(span, locale));
  }
  return moment(at, now, locale);
}

/** A day's heading in a list: Today, Yesterday, Monday 27 October. */
export function dayHeading(at: number, now: number, locale: Locale): string {
  const days = daysBefore(at, now);
  if (days <= 0) return WORDS[locale].today;
  if (days === 1) return WORDS[locale].yesterdayDay;
  const text = new Intl.DateTimeFormat(locale, { weekday: "long", day: "numeric", month: "long" }).format(at);
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** A file's size: 840 B, 12 kB, 2.4 MB (840 o, 12 ko, 2,4 Mo). */
export function fileSize(bytes: number, locale: Locale): string {
  const units = locale === "fr" ? ["o", "ko", "Mo", "Go"] : ["B", "kB", "MB", "GB"];
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: digits }).format(value);
  return `${number}${NBSP}${units[unit]}`;
}
