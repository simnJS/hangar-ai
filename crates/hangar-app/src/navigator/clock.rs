//! Times and percentages as the navigator words them (`proseTime`,
//! `resetWhen` of `lib/inspectorFormat.ts`, and `Intl.NumberFormat` percent).
//!
//! The web app asked the browser for these; the native one writes them itself
//! from the local clock, in the two languages Hangar has.
//!
//! TODO(P9): `inspectorFormat.ts` is P9's and `proseTime`/`resetWhen` are
//! shared with the inspector, which says a plan window's reset the same way;
//! P1's formatting module did not ship them. When one lands in `hangar-i18n`,
//! this file is the one to delete.

use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone, Timelike};
use hangar_i18n::{Key, Locale, Translator, args};

/// A time of day inside a sentence: "20 h 01" in French, the way French
/// typography writes it in running text, and "8:01 PM" in English.
pub fn prose_time(at: NaiveDateTime, locale: Locale) -> String {
    match locale {
        Locale::Fr => format!("{} h {:02}", at.hour(), at.minute()),
        Locale::En => {
            let (pm, hour) = at.hour12();
            let meridiem = if pm { "PM" } else { "AM" };
            // ICU puts a narrow no-break space before the meridiem.
            format!("{hour}:{:02}\u{202f}{meridiem}", at.minute())
        }
    }
}

const WEEKDAYS_EN: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const WEEKDAYS_FR: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];
const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

fn weekday(at: NaiveDateTime, locale: Locale) -> &'static str {
    let index = at.weekday().num_days_from_monday() as usize;
    match locale {
        Locale::Fr => WEEKDAYS_FR[index],
        Locale::En => WEEKDAYS_EN[index],
    }
}

/// "3 nov." in French, "Nov 3" in English.
fn short_date(at: NaiveDateTime, locale: Locale) -> String {
    let month = at.month0() as usize;
    match locale {
        Locale::Fr => format!("{} {}", at.day(), MONTHS_FR[month]),
        Locale::En => format!("{} {}", MONTHS_EN[month], at.day()),
    }
}

/// Whole calendar days from `from` to `to`; negative when `to` is earlier.
fn days_between(from: NaiveDateTime, to: NaiveDateTime) -> i64 {
    (to.date() - from.date()).num_days()
}

/// When a plan window frees up, as the end of a sentence: "remise à zéro à
/// 20 h 01" today, "demain à 1 h 05", "lundi à 9 h 00" within the week, a date
/// beyond. The navigator and the inspector both say it this way.
pub fn reset_when(at: NaiveDateTime, now: NaiveDateTime, tr: Translator) -> String {
    let locale = tr.locale();
    let time = prose_time(at, locale);
    let days = days_between(now, at);
    if days <= 0 {
        return tr
            .t_with(Key::SidebarPlanResetToday, &args!("time" => time))
            .into_owned();
    }
    if days == 1 {
        return tr
            .t_with(Key::SidebarPlanResetTomorrow, &args!("time" => time))
            .into_owned();
    }
    if days < 7 {
        return tr
            .t_with(
                Key::SidebarPlanResetDay,
                &args!("day" => weekday(at, locale), "time" => time),
            )
            .into_owned();
    }
    tr.t_with(
        Key::SidebarPlanResetDate,
        &args!("date" => short_date(at, locale), "time" => time),
    )
    .into_owned()
}

/// A whole percentage the way the language writes one: "42%", "42 %" (with
/// the narrow no-break space French typography puts before the sign).
pub fn percent(pct: u8, locale: Locale) -> String {
    match locale {
        Locale::Fr => format!("{pct}\u{202f}%"),
        Locale::En => format!("{pct}%"),
    }
}

/// A moment in ms since the epoch, on this machine's clock.
pub fn local(ms: i64) -> Option<NaiveDateTime> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| time.naive_local())
}

/// The present, on this machine's clock.
pub fn local_now() -> NaiveDateTime {
    Local::now().naive_local()
}

/// A rate limit's `resetsAt`, an ISO date, on this machine's clock.
pub fn parse_reset(iso: &str) -> Option<NaiveDateTime> {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .and_then(|time| local(time.timestamp_millis()))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    const EN: Translator = Translator::new(Locale::En);
    const FR: Translator = Translator::new(Locale::Fr);

    #[test]
    fn writes_a_time_of_day_as_each_language_does() {
        assert_eq!(prose_time(at(3, 20, 1), Locale::Fr), "20 h 01");
        assert_eq!(prose_time(at(3, 20, 1), Locale::En), "8:01\u{202f}PM");
        assert_eq!(prose_time(at(3, 0, 5), Locale::En), "12:05\u{202f}AM");
        assert_eq!(prose_time(at(3, 12, 0), Locale::En), "12:00\u{202f}PM");
        assert_eq!(prose_time(at(3, 1, 5), Locale::Fr), "1 h 05");
    }

    #[test]
    fn says_a_reset_later_today_with_its_time() {
        let now = at(3, 9, 0);
        assert_eq!(reset_when(at(3, 20, 1), now, FR), "remise à zéro à 20 h 01");
        assert_eq!(
            reset_when(at(3, 20, 1), now, EN),
            "resets at 8:01\u{202f}PM"
        );
    }

    #[test]
    fn says_tomorrow_a_weekday_within_the_week_and_a_date_beyond() {
        let now = at(3, 23, 50);
        assert_eq!(
            reset_when(at(4, 1, 5), now, FR),
            "remise à zéro demain à 1 h 05"
        );
        // 2026-11-03 is a Tuesday: the 6th is a Friday.
        assert_eq!(
            reset_when(at(6, 9, 0), now, FR),
            "remise à zéro vendredi à 9 h 00"
        );
        assert_eq!(
            reset_when(at(6, 9, 0), now, EN),
            "resets Friday at 9:00\u{202f}AM"
        );
        assert_eq!(
            reset_when(at(12, 9, 0), now, FR),
            "remise à zéro le 12 nov. à 9 h 00"
        );
        assert_eq!(
            reset_when(at(12, 9, 0), now, EN),
            "resets Nov 12 at 9:00\u{202f}AM"
        );
    }

    #[test]
    fn counts_calendar_days_not_hours() {
        // 23:50 to 00:10 the next day is one day away, not zero.
        let late = at(3, 23, 50);
        let early = at(4, 0, 10);
        assert_eq!(days_between(late, early), 1);
        assert_eq!(days_between(early, late), -1);
    }

    #[test]
    fn writes_a_percentage_as_each_language_does() {
        assert_eq!(percent(42, Locale::En), "42%");
        assert_eq!(percent(42, Locale::Fr), "42\u{202f}%");
    }

    #[test]
    fn reads_an_iso_reset_date() {
        assert!(parse_reset("2026-11-03T20:01:00Z").is_some());
        assert!(parse_reset("tomorrow").is_none());
    }
}
