/**
 * The spaces a line of interface text must not break at, set by rule rather
 * than typed by hand into each string: the dictionaries in i18n.ts are written
 * with plain spaces, and every string goes through here once, when the
 * dictionaries load.
 *
 * Only the templates are typeset, never what fills their placeholders: a task
 * title, a command or an agent's answer is shown exactly as it was written.
 */

/** U+202F, the space French sets before high punctuation. */
export const NARROW_NBSP = " ";
/** U+00A0, the space between a number and its unit. */
export const NBSP = " ";

/** Any space a writer might have typed: plain, no-break or narrow. */
const SPACE = "[ \\u00a0\\u202f]";

/** The short units a count of time is written with, in both languages. */
const UNIT = "(?:min|ms|s|h|j|d|sem)";

const BEFORE_UNIT = new RegExp(`(\\d|\\})${SPACE}(${UNIT})(?![\\p{L}\\p{N}])`, "gu");
const AFTER_UNIT = new RegExp(`(${NBSP}${UNIT})${SPACE}(?=\\d|\\{)`, "gu");
const AFTER_SIGN = new RegExp(`([<>≤≥~])${SPACE}(?=\\d|\\{)`, "gu");

/**
 * "12 min", "1 h 05", "< 1 min" never split across two lines. A unit only
 * counts as one after a digit or a placeholder, and when no letter follows:
 * "{n} sessions" is a count, not "{n} s".
 */
export function bindUnits(text: string): string {
  return text
    .replace(BEFORE_UNIT, `$1${NBSP}$2`)
    .replace(AFTER_UNIT, `$1${NBSP}`)
    .replace(AFTER_SIGN, `$1${NBSP}`);
}

const BEFORE_HIGH = new RegExp(`${SPACE}+([?!;:»%])`, "g");
const AFTER_OPENING = new RegExp(`«${SPACE}*`, "g");

/**
 * French punctuation: a narrow no-break space before ? ! ; : » and %, and
 * after «. A space typed before them, of whatever kind, becomes that one. One
 * left out before them is not invented — a colon with nothing before it may
 * well belong to a path or a time — and the dictionary test catches it
 * instead; « is never anything but an opening quote, so it always gets its
 * space.
 */
export function frenchSpacing(text: string): string {
  return text.replace(BEFORE_HIGH, `${NARROW_NBSP}$1`).replace(AFTER_OPENING, `«${NARROW_NBSP}`);
}

/** Everything a template of `locale` needs before it is shown. */
export function typeset(text: string, locale: string): string {
  const bound = bindUnits(text);
  return locale === "fr" ? frenchSpacing(bound) : bound;
}
