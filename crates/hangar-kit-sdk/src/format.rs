//! Numbers, durations and sizes written the way Hangar writes them, in the
//! tab's language: Hangar UI's `hangar.ui.format` (docs/kit-ui.md,
//! "Formats"), and the same rules (crates/hangar-core/kit-sdk/kit-ui.js).
//! A no-break space holds a number to its unit, so a line never ends
//! between them.

const NBSP: char = '\u{a0}';
/// French groups thousands with a narrow no-break space, as `Intl` does.
const NARROW_NBSP: char = '\u{202f}';

fn french(locale: &str) -> bool {
    locale
        .split(['-', '_'])
        .next()
        .is_some_and(|lang| lang.eq_ignore_ascii_case("fr"))
}

/// `1,204.5` in English, `1 204,5` in French, with at most `decimals`
/// digits after the point and none that are zeros at the end.
pub fn number(value: f64, decimals: usize, locale: &str) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let fixed = format!("{:.*}", decimals, value.abs());
    let (whole, fraction) = match fixed.split_once('.') {
        Some((whole, fraction)) => (whole, fraction.trim_end_matches('0')),
        None => (fixed.as_str(), ""),
    };
    let (group, point) = if french(locale) {
        (NARROW_NBSP, ',')
    } else {
        (',', '.')
    };
    let mut out = String::new();
    if value < 0.0 && (whole != "0" || !fraction.is_empty()) {
        out.push('-');
    }
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            out.push(group);
        }
        out.push(digit);
    }
    if !fraction.is_empty() {
        out.push(point);
        out.push_str(fraction);
    }
    out
}

/// `900 ms`, `2.3 s`, `1 min 28 s`, `2 h 05 min`, `3 d 4 h` (`3 j 4 h` in
/// French).
pub fn duration(ms: f64, locale: &str) -> String {
    if !ms.is_finite() || ms < 0.0 {
        return String::new();
    }
    if ms < 1000.0 {
        return format!("{}{NBSP}ms", ms.round() as u64);
    }
    if ms < 10_000.0 {
        return format!("{}{NBSP}s", number(ms / 1000.0, 1, locale));
    }
    let total = (ms / 1000.0).round() as u64;
    if total < 60 {
        return format!("{total}{NBSP}s");
    }
    let seconds = total % 60;
    let minutes = total / 60 % 60;
    let hours = total / 3600 % 24;
    let days = total / 86_400;
    if days > 0 {
        let day = if french(locale) { "j" } else { "d" };
        return match hours {
            0 => format!("{days}{NBSP}{day}"),
            hours => format!("{days}{NBSP}{day}{NBSP}{hours}{NBSP}h"),
        };
    }
    if hours > 0 {
        return match minutes {
            0 => format!("{hours}{NBSP}h"),
            minutes => format!("{hours}{NBSP}h{NBSP}{minutes:02}{NBSP}min"),
        };
    }
    match seconds {
        0 => format!("{minutes}{NBSP}min"),
        seconds => format!("{minutes}{NBSP}min{NBSP}{seconds}{NBSP}s"),
    }
}

/// `15 KB`, `15 Ko` in French, where bytes are octets.
pub fn bytes(size: u64, locale: &str) -> String {
    let units: [&str; 5] = if french(locale) {
        ["o", "Ko", "Mo", "Go", "To"]
    } else {
        ["B", "KB", "MB", "GB", "TB"]
    };
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let decimals = if value < 10.0 && unit > 0 { 1 } else { 0 };
    format!("{}{NBSP}{}", number(value, decimals, locale), units[unit])
}

// ---------------------------------------------------------------- Times.
//
// wasm32 has no time zone, so these take the person's as minutes east of UTC
// (`Start::utc_offset`, `Context::utc_offset`), and write what kit-ui.js's
// `format.clock`, `date` and `since` write with `Intl` in a browser, English
// in the 12-hour clock of `en`, as ICU writes it, with its narrow no-break
// space before AM and PM.

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// A moment as a wall clock shows it.
struct Civil {
    year: i64,
    /// From 1.
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

/// The days since 1970-01-01 of a date of the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let month = month as i64;
    let of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day as i64 - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

/// The date of a day since 1970-01-01 (`civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = (of_year - (153 * shifted + 2) / 5 + 1) as u32;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn local(at_ms: i64, utc_offset: i32) -> Civil {
    let local = at_ms + utc_offset as i64 * MINUTE_MS;
    let (year, month, day) = civil_from_days(local.div_euclid(DAY_MS));
    let of_day = local.rem_euclid(DAY_MS) / 1000;
    Civil {
        year,
        month,
        day,
        hour: (of_day / 3600) as u32,
        minute: (of_day / 60 % 60) as u32,
        second: (of_day % 60) as u32,
    }
}

fn local_day(at_ms: i64, utc_offset: i32) -> i64 {
    (at_ms + utc_offset as i64 * MINUTE_MS).div_euclid(DAY_MS)
}

/// `9:14 PM`, `12:05 AM`: the 12-hour clock of `en`; `09:14 PM` with
/// `two_digits`, as `Intl` writes an hour of `2-digit`.
fn twelve_hour(when: &Civil, two_digits: bool) -> String {
    let hour = match when.hour % 12 {
        0 => 12,
        hour => hour,
    };
    let half = if when.hour < 12 { "AM" } else { "PM" };
    if two_digits {
        format!("{hour:02}:{:02}{NARROW_NBSP}{half}", when.minute)
    } else {
        format!("{hour}:{:02}{NARROW_NBSP}{half}", when.minute)
    }
}

/// A time in a column, where times line up: `21:14`, or with `seconds`
/// `21:14:56`, in both languages.
pub fn clock(at_ms: i64, utc_offset: i32, seconds: bool) -> String {
    let when = local(at_ms, utc_offset);
    if seconds {
        format!("{:02}:{:02}:{:02}", when.hour, when.minute, when.second)
    } else {
        format!("{:02}:{:02}", when.hour, when.minute)
    }
}

/// A moment on its own: `Oct 31, 09:14 PM`, `31 oct., 21:14`, with the year
/// when it is not this one (`Oct 31, 2024, 09:14 PM`, `31 oct. 2024, 21:14`).
pub fn date(at_ms: i64, now_ms: i64, utc_offset: i32, locale: &str) -> String {
    let when = local(at_ms, utc_offset);
    let same_year = when.year == local(now_ms, utc_offset).year;
    let month = when.month as usize - 1;
    if french(locale) {
        let day = format!("{} {}", when.day, MONTHS_FR[month]);
        let day = if same_year {
            format!("{day},")
        } else {
            format!("{day} {},", when.year)
        };
        format!("{day} {:02}:{:02}", when.hour, when.minute)
    } else {
        let day = format!("{} {}", MONTHS_EN[month], when.day);
        let day = if same_year {
            day
        } else {
            format!("{day}, {}", when.year)
        };
        format!("{day}, {}", twelve_hour(&when, true))
    }
}

/// A time inside a sentence: `21 h 13` in French, `9:13 PM` in English.
fn prose_time(when: &Civil, locale: &str) -> String {
    if french(locale) {
        format!("{}{NBSP}h{NBSP}{:02}", when.hour, when.minute)
    } else {
        twelve_hour(when, false)
    }
}

/// How long ago, as Hangar's inspector says it: `just now`, `5 min ago`,
/// `today at 9:13 PM`, `yesterday at 9:13 PM`, `Oct 31 at 9:13 PM`; in
/// French `à l'instant`, `il y a 5 min`, `aujourd'hui à 21 h 13`,
/// `hier à 21 h 13`, `le 31 oct. à 21 h 13`.
pub fn since(at_ms: i64, now_ms: i64, utc_offset: i32, locale: &str) -> String {
    let fr = french(locale);
    let minutes = (now_ms - at_ms).div_euclid(MINUTE_MS);
    if minutes < 1 {
        return if fr { "à l'instant" } else { "just now" }.to_owned();
    }
    if minutes < 60 {
        return if fr {
            format!("il y a {minutes}{NBSP}min")
        } else {
            format!("{minutes}{NBSP}min ago")
        };
    }
    let when = local(at_ms, utc_offset);
    let time = prose_time(&when, locale);
    match local_day(now_ms, utc_offset) - local_day(at_ms, utc_offset) {
        0 if fr => format!("aujourd'hui à {time}"),
        0 => format!("today at {time}"),
        1 if fr => format!("hier à {time}"),
        1 => format!("yesterday at {time}"),
        _ if fr => format!(
            "le {} {} à {time}",
            when.day,
            MONTHS_FR[when.month as usize - 1]
        ),
        _ => format!(
            "{} {} at {time}",
            MONTHS_EN[when.month as usize - 1],
            when.day
        ),
    }
}

/// Milliseconds since the Unix epoch of an ISO 8601 date, as `Date.parse`
/// reads what tools write: `2024-10-31T21:14:56Z`, with a fraction of any
/// length (`.123456789Z`, Docker's), an offset (`+02:00`, `-0500`), or a
/// date alone (`2024-10-31`, midnight UTC). A time without a zone is taken
/// as UTC. None for anything else.
pub fn parse_iso(text: &str) -> Option<i64> {
    let text = text.trim();
    let number = |part: &str| -> Option<u32> {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let (date, rest) = match text.find(['T', 't', ' ']) {
        Some(at) => (&text[..at], Some(&text[at + 1..])),
        None => (text, None),
    };
    let mut parts = date.splitn(3, '-');
    let (year, month, day) = (
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year as i64, month, day);
    let Some(rest) = rest else {
        return Some(days * DAY_MS);
    };
    let (time, offset) = match rest.find(['Z', 'z', '+', '-']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let (clock, fraction) = match time.split_once('.') {
        Some((clock, fraction)) => (clock, fraction),
        None => (time, ""),
    };
    let mut fields = clock.split(':');
    let hour = number(fields.next()?)?;
    let minute = number(fields.next()?)?;
    let second = match fields.next() {
        Some(second) => number(second)?,
        None => 0,
    };
    if fields.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let millis = match fraction {
        "" => 0,
        digits if digits.bytes().all(|b| b.is_ascii_digit()) => {
            let mut three: String = digits.chars().take(3).collect();
            while three.len() < 3 {
                three.push('0');
            }
            three.parse::<i64>().ok()?
        }
        _ => return None,
    };
    let offset_minutes = match offset {
        "" | "Z" | "z" => 0,
        signed => {
            let sign = if signed.starts_with('-') { -1 } else { 1 };
            let digits: String = signed[1..].chars().filter(|c| *c != ':').collect();
            if digits.len() != 4 {
                return None;
            }
            let hours = number(&digits[..2])? as i64;
            let minutes = number(&digits[2..])? as i64;
            sign * (hours * 60 + minutes)
        }
    };
    Some(
        days * DAY_MS + (hour as i64 * 3600 + minute as i64 * 60 + second as i64) * 1000 + millis
            - offset_minutes * MINUTE_MS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2025-10-31T21:14:56Z, a Friday.
    const AT: i64 = 1_761_945_296_000;

    #[test]
    fn times_are_written_as_kit_ui_writes_them() {
        assert_eq!(parse_iso("2025-10-31T21:14:56Z"), Some(AT));
        assert_eq!(clock(AT, 60, false), "22:14");
        assert_eq!(clock(AT, 0, true), "21:14:56");
        assert_eq!(date(AT, AT, 0, "en"), "Oct 31, 09:14\u{202f}PM");
        assert_eq!(date(AT, AT, 0, "fr"), "31 oct., 21:14");
        let next_year = AT + 400 * DAY_MS;
        assert_eq!(
            date(AT, next_year, 0, "en"),
            "Oct 31, 2025, 09:14\u{202f}PM"
        );
        assert_eq!(date(AT, next_year, 0, "fr"), "31 oct. 2025, 21:14");
    }

    #[test]
    fn since_says_how_long_ago_in_the_persons_day() {
        let minute = MINUTE_MS;
        assert_eq!(since(AT, AT + 30_000, 0, "en"), "just now");
        assert_eq!(since(AT, AT - 5 * minute, 0, "fr"), "à l'instant");
        assert_eq!(since(AT, AT + 5 * minute, 0, "en"), "5\u{a0}min ago");
        assert_eq!(since(AT, AT + 5 * minute, 0, "fr"), "il y a 5\u{a0}min");
        // 21:14 UTC, then 23:30: the same day in London, the next in Paris.
        let later = AT + 136 * minute;
        assert_eq!(since(AT, later, 0, "en"), "today at 9:14\u{202f}PM");
        assert_eq!(since(AT, later, 60, "fr"), "hier à 22\u{a0}h\u{a0}14");
        assert_eq!(
            since(AT, AT + 3 * DAY_MS, 0, "en"),
            "Oct 31 at 9:14\u{202f}PM"
        );
        assert_eq!(
            since(AT, AT + 3 * DAY_MS, 0, "fr"),
            "le 31 oct. à 21\u{a0}h\u{a0}14"
        );
        assert_eq!(
            since(AT - 21 * 3_600_000, AT, 0, "en"),
            "today at 12:14\u{202f}AM"
        );
    }

    #[test]
    fn iso_dates_read_as_date_parse_reads_them() {
        assert_eq!(parse_iso("2025-10-31T21:14:56.5Z"), Some(AT + 500));
        assert_eq!(parse_iso("2025-10-31T21:14:56.123456789Z"), Some(AT + 123));
        assert_eq!(parse_iso("2025-10-31T23:14:56+02:00"), Some(AT));
        assert_eq!(parse_iso("2025-10-31T16:14:56-0500"), Some(AT));
        assert_eq!(
            parse_iso("2025-10-31"),
            Some(AT - (21 * 3600 + 14 * 60 + 56) * 1000)
        );
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert!(parse_iso("0001-01-01T00:00:00Z").unwrap() < 0);
        for wrong in [
            "",
            "yesterday",
            "2025-13-01",
            "2025-10-31T25:00Z",
            "2025-10-31T21:14:56+2",
        ] {
            assert_eq!(parse_iso(wrong), None, "{wrong:?}");
        }
    }

    #[test]
    fn as_hangar_writes_them() {
        assert_eq!(duration(900.0, "en"), "900\u{a0}ms");
        assert_eq!(duration(2300.0, "fr"), "2,3\u{a0}s");
        assert_eq!(duration(88_000.0, "en"), "1\u{a0}min\u{a0}28\u{a0}s");
        assert_eq!(duration(7_500_000.0, "en"), "2\u{a0}h\u{a0}05\u{a0}min");
        assert_eq!(duration(273_600_000.0, "fr"), "3\u{a0}j\u{a0}4\u{a0}h");
        assert_eq!(number(1204.5, 1, "en"), "1,204.5");
        assert_eq!(number(1204.5, 1, "fr"), "1\u{202f}204,5");
        assert_eq!(number(-3.0, 0, "en"), "-3");
        assert_eq!(bytes(15 * 1024, "fr"), "15\u{a0}Ko");
        assert_eq!(bytes(1536, "en"), "1.5\u{a0}KB");
    }
}
