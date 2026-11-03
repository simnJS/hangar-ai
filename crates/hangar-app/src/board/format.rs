//! How the board writes a task's numbers in words: its priority, its id, how
//! long it has been in progress (`src/lib/boardFormat.ts`, tested by
//! `boardFormat.test.ts`). Pure, so the cards, the dock, the task dialog and
//! the inspector all say the same thing, and so it tests without a window.

use chrono::{Local, NaiveDateTime, TimeZone};
use hangar_i18n::{Key, Locale, Translator, args};

/// The three steps a card shows for a task's numeric priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityLevel {
    Low = 1,
    Medium = 2,
    High = 3,
}

impl PriorityLevel {
    /// How many of the three bars are lit.
    pub fn bars(self) -> usize {
        self as usize
    }
}

/// The three steps a card shows for a task's numeric priority.
///
/// Tasks are created at 1 (`hangar-core/src/board.rs`), so 1 is the ordinary,
/// medium case; 0 is work that can wait, and anything above 1 is what an agent
/// or a person flagged as urgent. The scale is open-ended upwards, and every
/// value above 1 reads as high: the exact number stays in the tooltip.
pub fn priority_level(priority: f64) -> PriorityLevel {
    if !priority.is_finite() || priority <= 0.0 {
        PriorityLevel::Low
    } else if priority == 1.0 {
        PriorityLevel::Medium
    } else {
        PriorityLevel::High
    }
}

/// The word for the priority: `board.priority.low`, `.medium`, `.high`.
pub fn priority_key(priority: f64) -> Key {
    match priority_level(priority) {
        PriorityLevel::Low => Key::BoardPriorityLow,
        PriorityLevel::Medium => Key::BoardPriorityMedium,
        PriorityLevel::High => Key::BoardPriorityHigh,
    }
}

/// The short handle a card shows for its task. Ids are UUIDs, whose first
/// characters are as good as any to tell a dozen tasks apart at a glance; the
/// whole id stays in the tooltip for when an agent quotes it.
pub fn short_task_id(id: &str) -> String {
    let compact: String = id.chars().filter(|c| *c != '-').collect();
    let source = if compact.is_empty() { id } else { &compact };
    source.chars().take(4).collect()
}

/// How long something has been going on, at a glance rather than to the
/// second, before it is written in a language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Elapsed {
    Now,
    Minutes(u64),
    /// The minutes are padded to two digits, as the clock writes them.
    Hours {
        hours: u64,
        minutes: String,
    },
    Days {
        days: u64,
        hours: u64,
    },
}

/// A card in `doing`, the session a cost adds up over.
pub fn elapsed(ms: i64) -> Elapsed {
    let minutes = ms.max(0) as u64 / 60_000;
    if minutes < 1 {
        return Elapsed::Now;
    }
    if minutes < 60 {
        return Elapsed::Minutes(minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return Elapsed::Hours {
            hours,
            minutes: format!("{:02}", minutes % 60),
        };
    }
    Elapsed::Days {
        days: hours / 24,
        hours: hours % 24,
    }
}

/// `elapsedLabel`: [`elapsed`] in the person's language.
pub fn elapsed_label(ms: i64, tr: Translator) -> String {
    match elapsed(ms) {
        Elapsed::Now => tr.t(Key::BoardElapsedNow).into_owned(),
        Elapsed::Minutes(m) => tr
            .t_with(Key::BoardElapsedMinutes, &args!("m" => m))
            .into_owned(),
        Elapsed::Hours { hours, minutes } => tr
            .t_with(
                Key::BoardElapsedHours,
                &args!("h" => hours, "m" => minutes.as_str()),
            )
            .into_owned(),
        Elapsed::Days { days, hours } => tr
            .t_with(Key::BoardElapsedDays, &args!("d" => days, "h" => hours))
            .into_owned(),
    }
}

/// How much of a date to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    /// `toLocaleString(locale, { dateStyle: "short", timeStyle: "short" })`:
    /// a comment's time, a block's date.
    Short,
    /// `toLocaleString(locale)`: down to the second.
    Full,
}

/// A date and time in the person's language, as the platform wrote them.
fn render_when(at: NaiveDateTime, locale: Locale, style: DateStyle) -> String {
    let pattern = match (locale, style) {
        (Locale::En, DateStyle::Short) => "%-m/%-d/%y, %-I:%M %p",
        (Locale::En, DateStyle::Full) => "%-m/%-d/%Y, %-I:%M:%S %p",
        (Locale::Fr, DateStyle::Short) => "%d/%m/%Y %H:%M",
        (Locale::Fr, DateStyle::Full) => "%d/%m/%Y %H:%M:%S",
    };
    at.format(pattern).to_string()
}

/// A moment, in ms since the epoch, as local time in the person's language.
pub fn when(ms: u64, locale: Locale, style: DateStyle) -> String {
    let local = i64::try_from(ms)
        .ok()
        .and_then(|ms| Local.timestamp_millis_opt(ms).single());
    match local {
        Some(at) => render_when(at.naive_local(), locale, style),
        None => String::new(),
    }
}

/// The time now, in ms since the epoch: what `Date.now()` was.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_date_the_way_each_language_does() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 3)
            .and_then(|day| day.and_hms_opt(14, 5, 9))
            .expect("a date");
        assert_eq!(
            render_when(at, Locale::En, DateStyle::Short),
            "11/3/26, 2:05 PM"
        );
        assert_eq!(
            render_when(at, Locale::En, DateStyle::Full),
            "11/3/2026, 2:05:09 PM"
        );
        assert_eq!(
            render_when(at, Locale::Fr, DateStyle::Short),
            "03/11/2026 14:05"
        );
        assert_eq!(
            render_when(at, Locale::Fr, DateStyle::Full),
            "03/11/2026 14:05:09"
        );
    }

    #[test]
    fn reads_the_default_priority_as_medium() {
        assert_eq!(priority_level(1.0), PriorityLevel::Medium);
        assert_eq!(priority_key(1.0), Key::BoardPriorityMedium);
    }

    #[test]
    fn reads_zero_and_anything_nonsensical_as_low() {
        assert_eq!(priority_level(0.0), PriorityLevel::Low);
        assert_eq!(priority_level(-3.0), PriorityLevel::Low);
        assert_eq!(priority_level(f64::NAN), PriorityLevel::Low);
        assert_eq!(priority_key(0.0), Key::BoardPriorityLow);
    }

    #[test]
    fn reads_everything_above_the_default_as_high() {
        assert_eq!(priority_level(2.0), PriorityLevel::High);
        assert_eq!(priority_level(9.0), PriorityLevel::High);
        assert_eq!(priority_level(255.0), PriorityLevel::High);
        assert_eq!(priority_key(5.0), Key::BoardPriorityHigh);
    }

    #[test]
    fn lights_one_two_or_three_bars() {
        assert_eq!(PriorityLevel::Low.bars(), 1);
        assert_eq!(PriorityLevel::Medium.bars(), 2);
        assert_eq!(PriorityLevel::High.bars(), 3);
    }

    #[test]
    fn keeps_the_first_four_characters_of_a_uuid() {
        assert_eq!(
            short_task_id("3f2a9c1e-0b7d-4c55-9a43-2f1e8d0c6b11"),
            "3f2a"
        );
    }

    #[test]
    fn skips_the_dashes_so_a_short_first_group_cannot_leave_one_in() {
        assert_eq!(short_task_id("ab-cdef"), "abcd");
    }

    #[test]
    fn leaves_a_short_id_whole() {
        assert_eq!(short_task_id("09"), "09");
        assert_eq!(short_task_id("-"), "-");
    }

    #[test]
    fn rounds_anything_under_a_minute_to_now() {
        assert_eq!(elapsed(59_000), Elapsed::Now);
        assert_eq!(elapsed(-5_000), Elapsed::Now);
    }

    #[test]
    fn counts_minutes_then_hours_with_padded_minutes_then_days() {
        assert_eq!(elapsed(24 * 60_000), Elapsed::Minutes(24));
        assert_eq!(
            elapsed(72 * 60_000),
            Elapsed::Hours {
                hours: 1,
                minutes: "12".into()
            }
        );
        assert_eq!(
            elapsed(65 * 60_000),
            Elapsed::Hours {
                hours: 1,
                minutes: "05".into()
            }
        );
        assert_eq!(elapsed(27 * 3_600_000), Elapsed::Days { days: 1, hours: 3 });
    }

    /// The typesetter puts no-break spaces between a number and its unit;
    /// the test reads the words.
    fn plain(text: String) -> String {
        text.replace(['\u{a0}', '\u{202f}'], " ")
    }

    #[test]
    fn writes_the_elapsed_time_in_both_languages() {
        let en = Translator::new(Locale::En);
        let fr = Translator::new(Locale::Fr);
        assert_eq!(plain(elapsed_label(30_000, en)), "< 1 min");
        assert_eq!(plain(elapsed_label(24 * 60_000, en)), "24 min");
        assert_eq!(plain(elapsed_label(65 * 60_000, en)), "1 h 05");
        assert_eq!(plain(elapsed_label(27 * 3_600_000, en)), "1 d 3 h");
        assert_eq!(plain(elapsed_label(27 * 3_600_000, fr)), "1 j 3 h");
    }
}
