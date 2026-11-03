//! How the statistics page writes numbers, durations and dates in English and
//! French: the `Intl` calls of `lib/statsModel.ts` and `StatsCharts.tsx`,
//! spelled out.
//!
//! The expected strings in the tests below were produced by `Intl.NumberFormat`
//! and `Intl.DateTimeFormat` (Node 22, ICU 77), including the no-break
//! spaces French puts before a percent sign or a currency symbol, so the two
//! apps print the same figures. The one place this differs from `Intl` is a
//! decimal tie that the binary float cannot hold exactly (1.15 rounded to one
//! decimal): `Intl` rounds the exact value, this rounds the nearest float.

use hangar_i18n::{Key, Locale, Number, Translator, args};

use super::model::Measure;
use super::time;

/// U+00A0, the space between a number and its unit.
const NBSP: &str = "\u{a0}";

fn decimal_mark(locale: Locale) -> char {
    match locale {
        Locale::En => '.',
        Locale::Fr => ',',
    }
}

/// `value` with `decimals` places at most, trailing zeros dropped, the
/// language's decimal mark, no grouping.
fn plain(value: f64, decimals: usize, locale: Locale) -> String {
    let factor = 10f64.powi(decimals as i32);
    let rounded = (value.abs() * factor).round() / factor;
    let mut text = format!("{rounded:.decimals$}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if decimal_mark(locale) != '.' {
        text = text.replace('.', &decimal_mark(locale).to_string());
    }
    text
}

/// Which rounding a compact figure gets.
#[derive(Clone, Copy)]
enum Rounding {
    /// At most one decimal and three significant digits, whichever is the
    /// less precise: "1.2M", "845K", "125M".
    LessPrecise,
    /// Three significant digits.
    Significant,
    /// At most this many decimals.
    Decimals(usize),
}

impl Rounding {
    fn decimals(self, scaled: f64) -> usize {
        match self {
            Rounding::LessPrecise => usize::from(scaled < 100.0),
            Rounding::Significant => {
                if scaled <= 0.0 {
                    0
                } else {
                    (2 - scaled.log10().floor() as i32).max(0) as usize
                }
            }
            Rounding::Decimals(decimals) => decimals,
        }
    }
}

/// The suffixes of thousands, millions, billions and trillions.
fn suffixes(locale: Locale) -> [&'static str; 5] {
    match locale {
        Locale::En => ["", "K", "M", "B", "T"],
        Locale::Fr => ["", "k", "M", "Md", "Bn"],
    }
}

/// A figure in the language's short form, without its sign.
fn compact_magnitude(value: f64, rounding: Rounding, locale: Locale) -> String {
    let magnitude = value.abs();
    let mut tier = if magnitude < 1000.0 {
        0
    } else {
        ((magnitude.log10() / 3.0).floor() as usize).min(4)
    };
    loop {
        let scaled = magnitude / 1000f64.powi(tier as i32);
        let decimals = rounding.decimals(scaled);
        let factor = 10f64.powi(decimals as i32);
        // 999.6K reads "1M", not "1000K".
        if (scaled * factor).round() / factor >= 1000.0 && tier < 4 {
            tier += 1;
            continue;
        }
        let number = plain(scaled, decimals, locale);
        let suffix = suffixes(locale)[tier];
        return match (suffix.is_empty(), locale) {
            (true, _) => number,
            (false, Locale::En) => format!("{number}{suffix}"),
            (false, Locale::Fr) => format!("{number}{NBSP}{suffix}"),
        };
    }
}

/// "1,2 M", "845 k", "12 Md" in French; "1.2M", "845K", "12B" in English.
/// One decimal below a hundred, none above: three digits at most.
pub fn format_compact(value: f64, locale: Locale) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    format!(
        "{sign}{}",
        compact_magnitude(value, Rounding::LessPrecise, locale)
    )
}

/// A count in full, grouped the language's way: "1 284 093".
pub fn format_integer(value: f64, locale: Locale) -> String {
    Number::Int(value.round() as i64).format(locale)
}

/// A dollar amount around `body`: "$" before it in English, after a no-break
/// space in French.
fn dollars(negative: bool, body: &str, locale: Locale) -> String {
    let sign = if negative { "-" } else { "" };
    match locale {
        Locale::En => format!("{sign}${body}"),
        Locale::Fr => format!("{sign}{body}{NBSP}$"),
    }
}

/// `value` with exactly `decimals` places and its thousands grouped.
fn fixed(value: f64, decimals: usize, locale: Locale) -> String {
    let factor = 10f64.powi(decimals as i32);
    let rounded = (value.abs() * factor).round() / factor;
    let text = format!("{rounded:.decimals$}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let grouped = Number::Int(whole.parse::<i64>().unwrap_or(0)).format(locale);
    if fraction.is_empty() {
        grouped
    } else {
        format!("{grouped}{}{fraction}", decimal_mark(locale))
    }
}

/// Dollars at a glance: cents under ten, whole dollars up to a hundred
/// thousand, then compact: "0,84 $", "59 $", "1 284 $", "312 k$". `exact`
/// keeps the cents, for a table or a readout where the figure is the point.
pub fn format_usd(value: f64, locale: Locale, exact: bool) -> String {
    let negative = value < 0.0;
    if !exact && value >= 100_000.0 {
        return dollars(
            negative,
            &compact_magnitude(value, Rounding::Significant, locale),
            locale,
        );
    }
    let decimals = if exact || value < 10.0 { 2 } else { 0 };
    dollars(negative, &fixed(value, decimals, locale), locale)
}

/// An axis tick: as short as it can be and still distinct.
pub fn format_tick(value: f64, measure: Measure, locale: Locale) -> String {
    match measure {
        Measure::Tokens => format_compact(value, locale),
        Measure::Cost => {
            let decimals = if value < 10.0 { 1 } else { 0 };
            dollars(
                value < 0.0,
                &compact_magnitude(value, Rounding::Decimals(decimals), locale),
                locale,
            )
        }
    }
}

/// "34 %" with the language's no-break space.
fn percent(rounded: i64, locale: Locale) -> String {
    let number = Number::Int(rounded).format(locale);
    match locale {
        Locale::En => format!("{number}%"),
        Locale::Fr => format!("{number}{NBSP}%"),
    }
}

/// "+24 %", "-8 %": a trend, signed.
pub fn format_change(ratio: f64, locale: Locale) -> String {
    let rounded = (ratio * 100.0).round() as i64;
    if rounded > 0 {
        format!("+{}", percent(rounded, locale))
    } else {
        percent(rounded, locale)
    }
}

/// "34 %": a share. Under 1 % says so rather than rounding to nothing.
pub fn format_share(ratio: f64, locale: Locale) -> String {
    if ratio > 0.0 && ratio < 0.01 {
        return format!("<{NBSP}{}", percent(1, locale));
    }
    percent((ratio * 100.0).round() as i64, locale)
}

/// "45 min", "3 h 12", "142 h".
pub fn format_duration(ms: f64, tr: Translator) -> String {
    let minutes = (ms / 60_000.0).round() as i64;
    if minutes < 1 {
        return if ms > 0.0 {
            tr.t(Key::StatsDurationUnderMinute).into_owned()
        } else {
            tr.t_with(Key::StatsDurationMinutes, &args!("m" => 0))
                .into_owned()
        };
    }
    if minutes < 60 {
        return tr
            .t_with(Key::StatsDurationMinutes, &args!("m" => minutes))
            .into_owned();
    }
    let hours = minutes / 60;
    if hours >= 100 {
        return tr
            .t_with(Key::StatsDurationHoursOnly, &args!("h" => hours))
            .into_owned();
    }
    let rest = format!("{:02}", minutes % 60);
    tr.t_with(
        Key::StatsDurationHours,
        &args!("h" => hours, "m" => rest.as_str()),
    )
    .into_owned()
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

const WEEKDAYS_SHORT_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_LONG_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const WEEKDAYS_SHORT_FR: [&str; 7] = ["lun.", "mar.", "mer.", "jeu.", "ven.", "sam.", "dim."];
const WEEKDAYS_LONG_FR: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const MONTHS_SHORT_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_SHORT_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];
const MONTHS_LONG_FR: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];

/// Monday to Sunday, in the language's short or long form.
pub fn weekday_names(locale: Locale, long: bool) -> [&'static str; 7] {
    match (locale, long) {
        (Locale::En, false) => WEEKDAYS_SHORT_EN,
        (Locale::En, true) => WEEKDAYS_LONG_EN,
        (Locale::Fr, false) => WEEKDAYS_SHORT_FR,
        (Locale::Fr, true) => WEEKDAYS_LONG_FR,
    }
}

fn month_name(locale: Locale, month: u32, long: bool) -> &'static str {
    let at = (month as usize).saturating_sub(1).min(11);
    match (locale, long) {
        (Locale::En, false) => MONTHS_SHORT_EN[at],
        (Locale::En, true) => MONTHS_LONG_EN[at],
        (Locale::Fr, false) => MONTHS_SHORT_FR[at],
        (Locale::Fr, true) => MONTHS_LONG_FR[at],
    }
}

/// "Oct 22" / "22 oct.".
pub fn day_month(at: i64, locale: Locale) -> String {
    let (_, month, day) = time::date_of(at);
    match locale {
        Locale::En => format!("{} {day}", month_name(locale, month, false)),
        Locale::Fr => format!("{day} {}", month_name(locale, month, false)),
    }
}

/// "Thursday, October 22" / "jeudi 22 octobre".
pub fn weekday_day_month_long(at: i64, locale: Locale) -> String {
    let (_, month, day) = time::date_of(at);
    let weekday = weekday_names(locale, true)[time::weekday_of(at)];
    let month = month_name(locale, month, true);
    match locale {
        Locale::En => format!("{weekday}, {month} {day}"),
        Locale::Fr => format!("{weekday} {day} {month}"),
    }
}

/// "22 Thu" / "jeu. 22": the weekday and the day of the month, as `Intl`
/// orders them.
pub fn weekday_day(at: i64, locale: Locale) -> String {
    let (_, _, day) = time::date_of(at);
    let weekday = weekday_names(locale, false)[time::weekday_of(at)];
    match locale {
        Locale::En => format!("{day} {weekday}"),
        Locale::Fr => format!("{weekday} {day}"),
    }
}

/// "Oct 22, 2026" / "22 oct. 2026", with the short weekday before it when
/// asked: "Thu, Oct 22, 2026" / "jeu. 22 oct. 2026".
pub fn full_date(at: i64, locale: Locale, weekday: bool) -> String {
    let (year, month, day) = time::date_of(at);
    let month = month_name(locale, month, false);
    let name = weekday_names(locale, false)[time::weekday_of(at)];
    match (locale, weekday) {
        (Locale::En, false) => format!("{month} {day}, {year}"),
        (Locale::En, true) => format!("{name}, {month} {day}, {year}"),
        (Locale::Fr, false) => format!("{day} {month} {year}"),
        (Locale::Fr, true) => format!("{name} {day} {month} {year}"),
    }
}

/// "October 22, 2026" / "22 octobre 2026".
pub fn long_date(at: i64, locale: Locale) -> String {
    let (year, month, day) = time::date_of(at);
    let month = month_name(locale, month, true);
    match locale {
        Locale::En => format!("{month} {day}, {year}"),
        Locale::Fr => format!("{day} {month} {year}"),
    }
}

/// The time of day in running text: "9:05 AM" in English, "9 h 05" in French
/// (`proseTime`, which the typography writes with plain spaces).
pub fn prose_time(at: i64, locale: Locale) -> String {
    let (hour, minute) = time::clock_of(at);
    match locale {
        Locale::Fr => format!("{hour} h {minute:02}"),
        Locale::En => {
            let twelve = match hour % 12 {
                0 => 12,
                other => other,
            };
            let half = if hour < 12 { "AM" } else { "PM" };
            format!("{twelve}:{minute:02} {half}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::time::testing::at;
    use super::*;

    /// The TS test's `toMatch(/^1,2\s?M$/)`: the figure and its unit, with
    /// at most one space of any kind between.
    fn matches_compact(text: &str, number: &str, unit: &str) -> bool {
        text.strip_prefix(number)
            .and_then(|rest| rest.strip_suffix(unit))
            .is_some_and(|space| {
                space.chars().count() <= 1 && space.chars().all(char::is_whitespace)
            })
    }

    const EN: Locale = Locale::En;
    const FR: Locale = Locale::Fr;

    #[test]
    fn compacts_token_counts_the_way_each_language_does() {
        assert_eq!(format_compact(1_234_000.0, EN), "1.2M");
        assert_eq!(format_compact(124_500_000.0, EN), "125M");
        assert_eq!(format_compact(845_000.0, EN), "845K");
        assert!(matches_compact(
            &format_compact(1_234_000.0, FR),
            "1,2",
            "M"
        ));
        assert!(matches_compact(
            &format_compact(30_080_000_000.0, FR),
            "30,1",
            "Md"
        ));
    }

    #[test]
    fn compact_matches_intl_at_the_edges() {
        for (value, en, fr) in [
            (0.0, "0", "0"),
            (0.5, "0.5", "0,5"),
            (12.34, "12.3", "12,3"),
            (999.0, "999", "999"),
            (999.6, "1K", "1\u{a0}k"),
            (1000.0, "1K", "1\u{a0}k"),
            (1234.0, "1.2K", "1,2\u{a0}k"),
            (12_345.0, "12.3K", "12,3\u{a0}k"),
            (123_456.0, "123K", "123\u{a0}k"),
            (999_999.0, "1M", "1\u{a0}M"),
            (30_080_000_000.0, "30.1B", "30,1\u{a0}Md"),
            (2e12, "2T", "2\u{a0}Bn"),
        ] {
            assert_eq!(format_compact(value, EN), en, "{value} in English");
            assert_eq!(format_compact(value, FR), fr, "{value} in French");
        }
    }

    #[test]
    fn writes_dollars_with_cents_only_where_they_matter() {
        assert_eq!(format_usd(0.84, EN, false), "$0.84");
        assert_eq!(format_usd(59.44, EN, false), "$59");
        assert_eq!(format_usd(1284.4, EN, false), "$1,284");
        assert_eq!(format_usd(312_000.0, EN, false), "$312K");
        assert_eq!(format_usd(59.44, EN, true), "$59.44");
        assert_eq!(format_usd(312_000.0, EN, true), "$312,000.00");
    }

    #[test]
    fn writes_dollars_the_french_way() {
        assert_eq!(format_usd(0.84, FR, false), "0,84\u{a0}$");
        assert_eq!(format_usd(1284.4, FR, false), "1\u{202f}284\u{a0}$");
        assert_eq!(format_usd(312_000.0, FR, false), "312\u{a0}k\u{a0}$");
        assert_eq!(format_usd(1_234_567.0, FR, false), "1,23\u{a0}M\u{a0}$");
        assert_eq!(format_usd(312_000.0, FR, true), "312\u{202f}000,00\u{a0}$");
        assert_eq!(format_usd(0.0, EN, false), "$0.00");
        assert_eq!(format_usd(-3.0, EN, true), "-$3.00");
    }

    #[test]
    fn ticks_are_as_short_as_they_can_be() {
        let cost = |value| format_tick(value, Measure::Cost, EN);
        assert_eq!(cost(0.0), "$0");
        assert_eq!(cost(0.5), "$0.5");
        assert_eq!(cost(2.5), "$2.5");
        assert_eq!(cost(10.0), "$10");
        assert_eq!(cost(1500.0), "$2K");
        assert_eq!(cost(25_000.0), "$25K");
        assert_eq!(cost(2_500_000.0), "$3M");
        assert_eq!(format_tick(2.0, Measure::Cost, FR), "2\u{a0}$");
        assert_eq!(format_tick(25_000.0, Measure::Cost, FR), "25\u{a0}k\u{a0}$");
        assert_eq!(format_tick(1500.0, Measure::Tokens, EN), "1.5K");
    }

    #[test]
    fn signs_a_trend_and_says_a_sliver_of_a_share_is_under_one_percent() {
        assert_eq!(format_change(0.24, EN), "+24%");
        assert_eq!(format_change(-0.08, EN), "-8%");
        assert_eq!(format_change(12.34, EN), "+1,234%");
        assert_eq!(format_change(0.24, FR), "+24\u{a0}%");
        assert_eq!(format_change(12.34, FR), "+1\u{202f}234\u{a0}%");
        assert_eq!(format_share(0.004, EN), "<\u{a0}1%");
        assert_eq!(format_share(0.34, EN), "34%");
        assert_eq!(format_share(0.34, FR), "34\u{a0}%");
        assert_eq!(format_share(1.0, EN), "100%");
        assert_eq!(format_share(0.0, EN), "0%");
    }

    #[test]
    fn groups_counts() {
        assert_eq!(format_integer(1_284_093.0, EN), "1,284,093");
        assert_eq!(format_integer(1_284_093.0, FR), "1\u{202f}284\u{202f}093");
    }

    #[test]
    fn writes_durations_in_minutes_hours_and_minutes_or_hours() {
        let en = Translator::new(EN);
        assert_eq!(format_duration(45.0 * 60_000.0, en), "45\u{a0}min");
        assert_eq!(
            format_duration((3.0 * 60.0 + 5.0) * 60_000.0, en),
            "3\u{a0}h\u{a0}05"
        );
        assert_eq!(format_duration(142.0 * 3_600_000.0, en), "142\u{a0}h");
        assert_eq!(format_duration(20_000.0, en), "<\u{a0}1\u{a0}min");
        assert_eq!(format_duration(0.0, en), "0\u{a0}min");
        // 59 min 40 s is an hour on the minute count that rounds.
        assert_eq!(format_duration(3_580_000.0, en), "1\u{a0}h\u{a0}00");
    }

    #[test]
    fn dates_read_as_intl_writes_them() {
        let thursday = at(22, 9);
        assert_eq!(day_month(thursday, EN), "Oct 22");
        assert_eq!(day_month(thursday, FR), "22 oct.");
        assert_eq!(weekday_day_month_long(thursday, EN), "Thursday, October 22");
        assert_eq!(weekday_day_month_long(thursday, FR), "jeudi 22 octobre");
        assert_eq!(weekday_day(thursday, EN), "22 Thu");
        assert_eq!(weekday_day(thursday, FR), "jeu. 22");
        assert_eq!(full_date(thursday, EN, true), "Thu, Oct 22, 2026");
        assert_eq!(full_date(thursday, FR, true), "jeu. 22 oct. 2026");
        assert_eq!(full_date(thursday, EN, false), "Oct 22, 2026");
        assert_eq!(full_date(thursday, FR, false), "22 oct. 2026");
        assert_eq!(long_date(thursday, EN), "October 22, 2026");
        assert_eq!(long_date(thursday, FR), "22 octobre 2026");
        assert_eq!(prose_time(thursday, EN), "9:00 AM");
        assert_eq!(prose_time(at(22, 0), EN), "12:00 AM");
        assert_eq!(prose_time(at(22, 15), EN), "3:00 PM");
        assert_eq!(prose_time(at(22, 15), FR), "15 h 00");
        assert_eq!(weekday_names(FR, false)[0], "lun.");
        assert_eq!(weekday_names(EN, true)[6], "Sunday");
    }
}
