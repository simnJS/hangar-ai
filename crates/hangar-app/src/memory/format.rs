//! How a memory row writes where an entry came from: who signed it, when, and
//! from which folder (`lib/memoryFormat.ts`). Pure, so it is tested without a
//! window.
//!
//! The dates follow what `Intl.DateTimeFormat` wrote for "en" and "fr", which
//! the old window used and `hangar-i18n` has no counterpart for yet (its
//! `fmt` module is not there): months abbreviated, the year left out within
//! the current one. Times are in the local time zone.

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};

use hangar_i18n::Locale;

use crate::state::types::AGENTS;

/// A path that fits a narrow column, keeping its end: the folder a fact came
/// from is told apart by its last segments, never by its drive or home.
/// `C:\Users\me\Documents\IaBench` becomes `…\Documents\IaBench`.
pub fn compact_path(path: &str, max: usize) -> String {
    if path.chars().count() <= max {
        return path.to_owned();
    }
    let sep = if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    };
    let mut parts: Vec<&str> = path
        .split(['\\', '/'])
        .filter(|part| !part.is_empty())
        .collect();
    let mut tail = parts.pop().unwrap_or(path).to_owned();
    while let Some(last) = parts.last() {
        let next = format!("{last}{sep}{tail}");
        if next.chars().count() + 2 > max {
            break;
        }
        tail = next;
        parts.pop();
    }
    format!("…{sep}{tail}")
}

/// Who wrote an entry, as a row says it; `None` when it was you.
pub fn signer_name(agent: &str) -> Option<String> {
    let name = agent.trim();
    if name.is_empty() || name == "user" {
        return None;
    }
    let known = AGENTS.iter().find(|known| known.id.as_str() == name);
    Some(known.map_or(name, |known| known.label).to_owned())
}

/// A day, as short as it can be said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortDay {
    /// Anything since midnight, which the caller words.
    Today,
    /// "Oct 21" / "21 oct." this year, with the year before.
    On(String),
}

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

fn month_name(locale: Locale, month0: u32) -> &'static str {
    match locale {
        Locale::En => MONTHS_EN[month0 as usize],
        Locale::Fr => MONTHS_FR[month0 as usize],
    }
}

/// `short_day_in` in the machine's time zone, for the view.
pub fn short_day(ms: u64, now_ms: u64, locale: Locale) -> ShortDay {
    short_day_in(&Local, ms, now_ms, locale)
}

/// A day as short as it can be said: "Oct 21" this year, "Mar 3, 2025"
/// before, and `Today` for anything since midnight.
pub fn short_day_in<Tz: TimeZone>(tz: &Tz, ms: u64, now_ms: u64, locale: Locale) -> ShortDay {
    let (Some(date), Some(now)) = (at(tz, ms), at(tz, now_ms)) else {
        return ShortDay::On(String::new());
    };
    if date.date_naive() == now.date_naive() {
        return ShortDay::Today;
    }
    let month = month_name(locale, date.month0());
    let day = date.day();
    let same_year = date.year() == now.year();
    ShortDay::On(match (locale, same_year) {
        (Locale::En, true) => format!("{month} {day}"),
        (Locale::En, false) => format!("{month} {day}, {}", date.year()),
        (Locale::Fr, true) => format!("{day} {month}"),
        (Locale::Fr, false) => format!("{day} {month} {}", date.year()),
    })
}

fn at<Tz: TimeZone>(tz: &Tz, ms: u64) -> Option<DateTime<Tz>> {
    tz.timestamp_millis_opt(i64::try_from(ms).ok()?).single()
}

/// `Date.toLocaleString(locale)`: the full date and time, for a tooltip or
/// the action row ("10/21/2026, 3:04:05 PM", "21/10/2026 15:04:05").
pub fn full_date(ms: u64, locale: Locale) -> String {
    full_date_in(&Local, ms, locale)
}

pub fn full_date_in<Tz: TimeZone>(tz: &Tz, ms: u64, locale: Locale) -> String {
    let Some(date) = at(tz, ms) else {
        return String::new();
    };
    match locale {
        Locale::En => {
            let (pm, hour) = date.hour12();
            // Recent engines put a narrow no-break space before AM and PM.
            format!(
                "{}/{}/{}, {}:{:02}:{:02}\u{202f}{}",
                date.month(),
                date.day(),
                date.year(),
                hour,
                date.minute(),
                date.second(),
                if pm { "PM" } else { "AM" }
            )
        }
        Locale::Fr => format!(
            "{:02}/{:02}/{} {:02}:{:02}:{:02}",
            date.day(),
            date.month(),
            date.year(),
            date.hour(),
            date.minute(),
            date.second()
        ),
    }
}

/// An entry counts as rewritten when it changed well after it was created: an
/// upsert a few seconds later is still the first writing.
pub fn was_updated(created: u64, updated: u64) -> bool {
    updated.saturating_sub(created) > 60_000
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    #[test]
    fn leaves_a_path_that_fits_alone() {
        assert_eq!(compact_path("C:\\dev\\hangar-ai", 30), "C:\\dev\\hangar-ai");
    }

    #[test]
    fn keeps_the_end_of_a_long_windows_path() {
        assert_eq!(
            compact_path("C:\\Users\\me\\Documents\\IaBench", 20),
            "…\\Documents\\IaBench"
        );
    }

    #[test]
    fn keeps_the_end_of_a_long_posix_path() {
        assert_eq!(
            compact_path("/home/someone/work/clients/storefront", 24),
            "…/clients/storefront"
        );
    }

    #[test]
    fn never_drops_the_last_segment_however_long() {
        assert_eq!(
            compact_path("/srv/a-folder-name-longer-than-the-limit", 12),
            "…/a-folder-name-longer-than-the-limit"
        );
    }

    #[test]
    fn says_nothing_for_the_user_who_is_addressed_as_you() {
        assert_eq!(signer_name("user"), None);
        assert_eq!(signer_name("  "), None);
    }

    #[test]
    fn names_a_bare_agent_id_after_the_agent() {
        assert_eq!(signer_name("claude").as_deref(), Some("Claude Code"));
        assert_eq!(signer_name("shell").as_deref(), Some("Shell"));
    }

    #[test]
    fn keeps_any_other_signature_as_it_was_written() {
        assert_eq!(signer_name("Ava").as_deref(), Some("Ava"));
    }

    fn ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> u64 {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
            .unwrap()
            .timestamp_millis() as u64
    }

    #[test]
    fn says_today_for_anything_since_midnight() {
        let now = ms(2026, 10, 26, 15, 0);
        assert_eq!(
            short_day_in(&Utc, ms(2026, 10, 26, 0, 5), now, Locale::Fr),
            ShortDay::Today
        );
    }

    #[test]
    fn drops_the_year_within_the_current_one() {
        let now = ms(2026, 10, 26, 15, 0);
        let ShortDay::On(text) = short_day_in(&Utc, ms(2026, 10, 21, 12, 0), now, Locale::Fr)
        else {
            panic!("not today");
        };
        assert!(text.contains("21"));
        assert!(!text.contains("2026"));
        assert_eq!(text, "21 oct.");
        assert_eq!(
            short_day_in(&Utc, ms(2026, 10, 21, 12, 0), now, Locale::En),
            ShortDay::On("Oct 21".into())
        );
    }

    #[test]
    fn keeps_the_year_before_it() {
        let now = ms(2026, 10, 26, 15, 0);
        let ShortDay::On(text) = short_day_in(&Utc, ms(2025, 3, 3, 12, 0), now, Locale::En) else {
            panic!("not today");
        };
        assert!(text.contains("2025"));
        assert_eq!(text, "Mar 3, 2025");
    }

    #[test]
    fn writes_the_full_date_the_way_each_language_does() {
        let at = ms(2026, 10, 21, 15, 4) + 5_000;
        assert_eq!(
            full_date_in(&Utc, at, Locale::En),
            "10/21/2026, 3:04:05\u{202f}PM"
        );
        assert_eq!(full_date_in(&Utc, at, Locale::Fr), "21/10/2026 15:04:05");
        // Midnight is 12 AM, noon 12 PM.
        assert!(
            full_date_in(&Utc, ms(2026, 1, 2, 0, 0), Locale::En).contains("12:00:00\u{202f}AM")
        );
        assert!(
            full_date_in(&Utc, ms(2026, 1, 2, 12, 0), Locale::En).contains("12:00:00\u{202f}PM")
        );
    }

    #[test]
    fn treats_an_upsert_right_after_creation_as_the_first_writing() {
        assert!(!was_updated(1_000, 31_000));
        assert!(was_updated(1_000, 1_000 + 3_600_000));
        // A clock that went backwards is not an update.
        assert!(!was_updated(5_000, 1_000));
    }
}
