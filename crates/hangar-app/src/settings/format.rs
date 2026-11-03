//! How the settings write numbers, sizes and dates in the interface's
//! language: the `Intl` calls of `schema.tsx` (`num`), `VoiceCard.tsx`
//! (`formatBytes`) and `lib/remote.ts` (`formatStamp`).
//!
//! `hangar-i18n` writes the counts inside its sentences; these are the
//! figures the settings put in labels of their own (a slider's value, a
//! model's size, the day a phone was paired).

use chrono::{Datelike, Local, TimeZone, Timelike};
use hangar_i18n::Locale;

/// The space between a number and its unit: it must not break the line, and
/// French wants the narrow one.
fn unit_space(locale: Locale) -> char {
    match locale {
        Locale::En => '\u{00a0}',
        Locale::Fr => '\u{202f}',
    }
}

/// `value` with exactly `digits` decimals, grouped by thousands, the way the
/// language writes it: `1.30` and `10,000` in English, `1,30` and `10 000`
/// (narrow no-break space) in French. A slider's value in `toLocaleString`
/// with equal minimum and maximum fraction digits (`num` of `schema.tsx`).
pub fn num(locale: Locale, value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return if value.is_nan() {
            "NaN".to_string()
        } else if value < 0.0 {
            "-∞".to_string()
        } else {
            "∞".to_string()
        };
    }
    let (group, decimal) = match locale {
        Locale::En => (",", '.'),
        Locale::Fr => ("\u{202f}", ','),
    };
    let fixed = format!("{:.*}", digits, value.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut out = String::with_capacity(fixed.len() + whole.len() / 3 * group.len() + 1);
    // A value that rounds to zero is not negative: "-0.00" is not a number
    // anyone writes.
    let is_zero = fixed.chars().all(|c| c == '0' || c == '.');
    if value < 0.0 && !is_zero {
        out.push('-');
    }
    for (at, digit) in whole.chars().enumerate() {
        if at > 0 && (whole.len() - at) % 3 == 0 {
            out.push_str(group);
        }
        out.push(digit);
    }
    if !fraction.is_empty() {
        out.push(decimal);
        out.push_str(fraction);
    }
    out
}

/// A number the way JavaScript writes it with no formatting at all: whole
/// when it is whole (`100`, not `100.0`), otherwise as short as it can be
/// (`97.5`). The settings print some values this way (`${uiScale} %`).
pub fn plain(value: f64) -> String {
    format!("{value}")
}

/// "629 MB" in English, "629 Mo" in French, and "1,5 Go" past a gigabyte
/// (`formatBytes` of `VoiceCard.tsx`): whole megabytes up to a gigabyte, then
/// gigabytes with one decimal at most.
pub fn format_bytes(locale: Locale, bytes: u64) -> String {
    let megabytes = bytes as f64 / (1024.0 * 1024.0);
    let (value, unit) = match (megabytes >= 1024.0, locale) {
        (true, Locale::En) => (megabytes / 1024.0, "GB"),
        (true, Locale::Fr) => (megabytes / 1024.0, "Go"),
        (false, Locale::En) => (megabytes.round(), "MB"),
        (false, Locale::Fr) => (megabytes.round(), "Mo"),
    };
    // At most one decimal, none when it would be a zero ("2 GB", not "2.0 GB").
    let rounded = (value * 10.0).round() / 10.0;
    let digits = usize::from(rounded.fract() != 0.0);
    format!(
        "{}{}{unit}",
        num(locale, rounded, digits),
        unit_space(locale)
    )
}

/// `m:ss`, the way a countdown reads (`formatCountdown`).
pub fn format_countdown(seconds: i64) -> String {
    let whole = seconds.max(0);
    format!("{}:{:02}", whole / 60, whole % 60)
}

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// "Oct 28, 08:41 PM" / "28 oct., 20:41" in the person's time zone; the year
/// only when it is not this one (`formatStamp`). Both arguments are
/// milliseconds since the epoch, `now` being what "this year" is.
pub fn format_stamp(locale: Locale, ms: u64, now: u64) -> String {
    let Some(date) = Local.timestamp_millis_opt(ms as i64).single() else {
        return String::new();
    };
    let same_year = Local
        .timestamp_millis_opt(now as i64)
        .single()
        .is_none_or(|now| now.year() == date.year());
    let month = (date.month0() as usize).min(11);
    match locale {
        Locale::En => {
            let (hour, meridiem) = match date.hour() {
                0 => (12, "AM"),
                hour @ 1..=11 => (hour, "AM"),
                12 => (12, "PM"),
                hour => (hour - 12, "PM"),
            };
            let year = if same_year {
                String::new()
            } else {
                format!(", {}", date.year())
            };
            format!(
                "{} {}{year}, {hour:02}:{:02}\u{202f}{meridiem}",
                MONTHS_EN[month],
                date.day(),
                date.minute()
            )
        }
        Locale::Fr => {
            let year = if same_year {
                String::new()
            } else {
                format!(" {}", date.year())
            };
            format!(
                "{} {}{year}, {:02}:{:02}",
                date.day(),
                MONTHS_FR[month],
                date.hour(),
                date.minute()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    #[test]
    fn numbers_keep_the_decimals_asked_for() {
        assert_eq!(num(Locale::En, 1.3, 2), "1.30");
        assert_eq!(num(Locale::Fr, 1.3, 2), "1,30");
        assert_eq!(num(Locale::Fr, 3.0, 1), "3,0");
        assert_eq!(num(Locale::En, 130.0, 0), "130");
    }

    #[test]
    fn thousands_are_grouped_as_the_language_groups_them() {
        assert_eq!(num(Locale::En, 10_000.0, 0), "10,000");
        assert_eq!(num(Locale::Fr, 10_000.0, 0), "10\u{202f}000");
        assert_eq!(num(Locale::En, 50_000.0, 0), "50,000");
        assert_eq!(num(Locale::En, 999.0, 0), "999");
        assert_eq!(num(Locale::En, 1_234_567.891, 1), "1,234,567.9");
    }

    #[test]
    fn a_negative_keeps_its_sign_unless_it_rounds_to_nothing() {
        assert_eq!(num(Locale::En, -1.0, 1), "-1.0");
        assert_eq!(num(Locale::En, -0.04, 1), "0.0");
    }

    #[test]
    fn a_value_that_is_not_a_number_is_still_text() {
        assert_eq!(num(Locale::En, f64::NAN, 0), "NaN");
        assert_eq!(num(Locale::En, f64::INFINITY, 0), "∞");
    }

    #[test]
    fn plain_numbers_are_written_as_javascript_writes_them() {
        assert_eq!(plain(100.0), "100");
        assert_eq!(plain(97.5), "97.5");
        assert_eq!(plain(-0.5), "-0.5");
        assert_eq!(plain(1.3), "1.3");
    }

    #[test]
    fn a_size_is_in_megabytes_up_to_a_gigabyte() {
        assert_eq!(format_bytes(Locale::En, 629 * MB), "629\u{a0}MB");
        assert_eq!(format_bytes(Locale::Fr, 629 * MB), "629\u{202f}Mo");
        assert_eq!(format_bytes(Locale::En, 181 * MB), "181\u{a0}MB");
    }

    #[test]
    fn a_size_past_a_gigabyte_is_in_gigabytes_with_one_decimal() {
        assert_eq!(format_bytes(Locale::Fr, 1536 * MB), "1,5\u{202f}Go");
        assert_eq!(format_bytes(Locale::En, 1536 * MB), "1.5\u{a0}GB");
        assert_eq!(format_bytes(Locale::En, 2048 * MB), "2\u{a0}GB");
    }

    #[test]
    fn a_countdown_reads_minutes_and_seconds_and_stops_at_zero() {
        assert_eq!(format_countdown(300), "5:00");
        assert_eq!(format_countdown(65), "1:05");
        assert_eq!(format_countdown(9), "0:09");
        assert_eq!(format_countdown(-3), "0:00");
    }

    fn local_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> u64 {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64
    }

    #[test]
    fn this_years_stamps_have_no_year() {
        let now = local_ms(2026, 10, 28, 12, 0);
        let this_year = format_stamp(Locale::En, local_ms(2026, 10, 27, 20, 41), now);
        let last_year = format_stamp(Locale::En, local_ms(2025, 10, 27, 20, 41), now);
        assert!(!this_year.contains("2026"), "{this_year}");
        assert!(last_year.contains("2025"), "{last_year}");
    }

    #[test]
    fn a_stamp_reads_as_the_language_writes_a_date() {
        let now = local_ms(2026, 10, 28, 12, 0);
        let evening = local_ms(2026, 10, 27, 20, 41);
        assert_eq!(
            format_stamp(Locale::En, evening, now),
            "Oct 27, 08:41\u{202f}PM"
        );
        assert_eq!(format_stamp(Locale::Fr, evening, now), "27 oct., 20:41");
        let last_year = local_ms(2025, 1, 3, 0, 5);
        assert_eq!(
            format_stamp(Locale::En, last_year, now),
            "Jan 3, 2025, 12:05\u{202f}AM"
        );
        assert_eq!(
            format_stamp(Locale::Fr, last_year, now),
            "3 janv. 2025, 00:05"
        );
    }
}
