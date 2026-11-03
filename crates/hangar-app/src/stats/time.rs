//! Days, weeks and hours in the person's own time zone, on millisecond
//! timestamps: the `Date` arithmetic of `lib/statsModel.ts`.
//!
//! The statistics page counts calendar days, not 24-hour spans, so a day that
//! a clock change makes 23 or 25 hours long is still one column of the chart.
//! JavaScript's `Date` resolves a local time that does not exist (the hour a
//! spring-forward skips) by moving forward, and one that exists twice by
//! taking the first; the functions here do the same, so both apps cut the
//! days at the same instants.

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};

/// The current time, ms since the epoch (`Date.now()`).
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn local(at: i64) -> DateTime<Local> {
    // A timestamp the calendar cannot hold is not one a transcript has.
    Local
        .timestamp_millis_opt(at)
        .single()
        .unwrap_or_else(|| Local.timestamp_millis_opt(0).unwrap())
}

/// A local wall-clock time as an instant: the first one when it happens
/// twice, and the first existing one after it when it does not happen.
fn resolve(mut wall: NaiveDateTime) -> i64 {
    // The longest gap a zone has skipped is an hour; a quarter of an hour at a
    // time finds the first minute that exists, whatever the offset.
    for _ in 0..24 {
        if let Some(found) = Local.from_local_datetime(&wall).earliest() {
            return found.timestamp_millis();
        }
        wall += chrono::Duration::minutes(15);
    }
    wall.and_utc().timestamp_millis()
}

fn midnight(date: NaiveDate) -> i64 {
    resolve(date.and_hms_opt(0, 0, 0).unwrap_or_default())
}

/// Local midnight of the day holding `at`.
pub fn start_of_day(at: i64) -> i64 {
    midnight(local(at).date_naive())
}

/// `days` calendar days later, at the same local time: 23 or 25 hours across
/// a clock change.
pub fn add_days(at: i64, days: i64) -> i64 {
    let wall = local(at).naive_local();
    let moved = if days >= 0 {
        wall.checked_add_days(Days::new(days as u64))
    } else {
        wall.checked_sub_days(Days::new(days.unsigned_abs()))
    };
    moved.map_or(at, resolve)
}

/// Local midnight of the Monday starting the week holding `at`.
pub fn start_of_week(at: i64) -> i64 {
    let date = local(at).date_naive();
    let back = date.weekday().num_days_from_monday() as u64;
    midnight(date.checked_sub_days(Days::new(back)).unwrap_or(date))
}

/// Monday 0 … Sunday 6.
pub fn weekday_of(at: i64) -> usize {
    local(at).weekday().num_days_from_monday() as usize
}

/// The local hour, 0–23.
pub fn hour_of(at: i64) -> usize {
    local(at).hour() as usize
}

/// The local date of `at`: year, month (1–12) and day.
pub fn date_of(at: i64) -> (i32, u32, u32) {
    let date = local(at).date_naive();
    (date.year(), date.month(), date.day())
}

/// The local clock time of `at`: hour and minute.
pub fn clock_of(at: i64) -> (u32, u32) {
    let time = local(at);
    (time.hour(), time.minute())
}

#[cfg(test)]
pub(super) mod testing {
    use super::*;

    /// A local time of October 2026, so the tests hold in any time zone.
    pub fn at(day: u32, hour: u32) -> i64 {
        resolve(
            NaiveDate::from_ymd_opt(2026, 10, day)
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap(),
        )
    }

    /// A local time of any month of 2026.
    pub fn on(month: u32, day: u32, hour: u32) -> i64 {
        resolve(
            NaiveDate::from_ymd_opt(2026, month, day)
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn a_day_starts_at_local_midnight() {
        let midnight = start_of_day(at(22, 15));
        assert_eq!(date_of(midnight), (2026, 10, 22));
        assert_eq!(clock_of(midnight), (0, 0));
        assert_eq!(start_of_day(midnight), midnight);
    }

    #[test]
    fn adding_days_keeps_the_wall_clock() {
        assert_eq!(add_days(at(22, 10), 3), at(25, 10));
        assert_eq!(add_days(at(22, 10), -22), on(9, 30, 10));
        assert_eq!(add_days(at(22, 10), 0), at(22, 10));
    }

    #[test]
    fn a_week_starts_on_monday() {
        // 22 October 2026 is a Thursday.
        assert_eq!(weekday_of(at(22, 9)), 3);
        assert_eq!(start_of_week(at(22, 9)), start_of_day(at(19, 0)));
        // The Monday itself, and the Sunday before the next one.
        assert_eq!(start_of_week(at(19, 0)), start_of_day(at(19, 12)));
        assert_eq!(start_of_week(at(25, 23)), start_of_day(at(19, 12)));
        assert_eq!(weekday_of(at(25, 23)), 6);
        assert_eq!(hour_of(at(25, 23)), 23);
    }
}
