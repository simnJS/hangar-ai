//! The plan block's rules (`NavigatorPlan.tsx`, which had no tests): when a
//! window turns amber and red, how it is worded, and which reset the heading
//! names.

use chrono::{Local, NaiveDate, TimeZone};
use hangar_i18n::Locale;

use super::*;

const EN: Translator = Translator::new(Locale::En);
const FR: Translator = Translator::new(Locale::Fr);

/// A moment of November 2026 on this machine's clock, in ms since the epoch.
fn at(day: u32, hour: u32, minute: u32) -> i64 {
    let local = NaiveDate::from_ymd_opt(2026, 11, day)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap();
    Local
        .from_local_datetime(&local)
        .earliest()
        .unwrap()
        .timestamp_millis()
}

fn iso_ms(iso: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .timestamp_millis()
}

fn limit(kind: &str, used: f64, resets_at: Option<&str>) -> RateLimit {
    RateLimit {
        kind: kind.into(),
        percent_used: used,
        resets_at: resets_at.map(str::to_string),
    }
}

#[test]
fn a_window_is_calm_until_seventy_amber_until_ninety_then_red() {
    assert_eq!(level_of(0), Level::Ok);
    assert_eq!(level_of(69), Level::Ok);
    assert_eq!(level_of(70), Level::Warn);
    assert_eq!(level_of(89), Level::Warn);
    assert_eq!(level_of(90), Level::Hot);
    assert_eq!(level_of(100), Level::Hot);
}

#[test]
fn the_usage_is_whole_percent_held_between_zero_and_a_hundred() {
    assert_eq!(meter_pct(41.6), 42);
    assert_eq!(meter_pct(-3.0), 0);
    assert_eq!(meter_pct(180.0), 100);
    assert_eq!(meter_pct(69.5), 70);
}

#[test]
fn the_plans_own_windows_are_named_and_a_gateways_are_spelled_out() {
    assert_eq!(window_label(EN, "five_hour"), "5\u{a0}h");
    assert_eq!(window_label(EN, "seven_day"), "7\u{a0}d");
    assert_eq!(window_label(FR, "seven_day"), "7\u{a0}j");
    assert_eq!(
        window_label(EN, "monthly_opus_budget"),
        "monthly opus budget"
    );
}

#[test]
fn an_empty_report_keeps_the_last_one() {
    assert!(take(&[]).is_none());
    let report = take(&[limit("five_hour", 12.0, None)]).unwrap();
    assert_eq!(report.limits.len(), 1);
}

#[test]
fn the_heading_names_the_window_that_frees_up_first() {
    let now = at(3, 12, 0);
    let limits = [
        limit("seven_day", 30.0, Some("2030-01-01T00:00:00Z")),
        limit("five_hour", 71.0, Some("2999-01-01T00:00:00Z")),
    ];
    let soonest = next_reset(&limits, now).unwrap();
    assert_eq!(soonest, iso_ms("2030-01-01T00:00:00Z"));
}

#[test]
fn a_reset_already_past_names_nothing() {
    let now = at(3, 12, 0);
    let past = [limit("five_hour", 71.0, Some("2000-01-01T00:00:00Z"))];
    assert_eq!(next_reset(&past, now), None);
    assert_eq!(next_reset(&[limit("five_hour", 1.0, None)], now), None);
    assert_eq!(
        next_reset(&[limit("five_hour", 1.0, Some("not a date"))], now),
        None
    );
}

#[test]
fn the_tooltip_says_what_the_block_is_then_when_each_window_resets() {
    let now = at(3, 9, 0);
    // A reset two hours on is "today", whatever the zone.
    let soon = Local.timestamp_millis_opt(now).unwrap();
    let iso = (soon + chrono::Duration::hours(2)).to_rfc3339();
    let words = hint(
        EN,
        &[
            limit("five_hour", 10.0, Some(&iso)),
            limit("seven_day", 5.0, None),
        ],
        now,
    );
    let lines: Vec<&str> = words.lines().collect();
    assert_eq!(lines[0], "Plan usage, as Claude Code last reported it");
    assert!(lines[1].starts_with("5\u{a0}h: resets at "), "{}", lines[1]);
    // A window with no reset has no line.
    assert_eq!(lines.len(), 2);
}
