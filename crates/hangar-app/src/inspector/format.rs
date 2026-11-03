//! Wording and number formats of the inspector (`lib/inspectorFormat.ts`).
//!
//! Pure: the clock comes in as an argument and the language as a `Locale`,
//! so every rule here has a test. Times are epoch milliseconds, as the web
//! app carried them, and are read in the machine's time zone.
//!
//! The web app got its number and date formats from `Intl`. Hangar has no
//! such library, and only two languages, so the few shapes the inspector
//! needs (a cost, a percentage, a time of day, a short date, a weekday) are
//! written out here for English and French, to the letter of what `Intl`
//! produced in each (`1,84 $` with a no-break space, `34 %` with a narrow one,
//! `20 oct.`, `8:01 PM`). If another module needs the same shapes, this is
//! the file to lift them from into `hangar-i18n`.

use std::sync::LazyLock;

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Weekday};
use hangar_i18n::{Key, Locale, Number, Translator, args};
use regex::Regex;

use crate::state::live::RateLimit;

/// How worrying a reading is: amber from 70 %, red from 90 %, the same for
/// the context and the plan (`usageTone`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Warn,
    Danger,
}

pub fn usage_tone(pct: f64) -> Tone {
    if pct >= 90.0 {
        Tone::Danger
    } else if pct >= 70.0 {
        Tone::Warn
    } else {
        Tone::Normal
    }
}

const FAMILIES: [&str; 4] = ["opus", "sonnet", "haiku", "fable"];

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// "claude-sonnet-4-5-20250929" → "Sonnet 4.5", for the header's sub-line.
/// The Session section still shows the id as the agent gave it; anything not
/// recognised as a Claude model is returned untouched rather than guessed at.
pub fn pretty_model(model: Option<&str>) -> Option<String> {
    static SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[.*\]$").unwrap());
    // Current naming: family first, then the version.
    static MODERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|claude-)(opus|sonnet|haiku|fable)-(\d+)(?:[-.](\d{1,2}))?(?:-\d{8})?$")
            .unwrap()
    });
    // The Claude 3 naming put the version first.
    static LEGACY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^claude-(\d+)(?:-(\d))?-(opus|sonnet|haiku)").unwrap());

    let model = model?;
    if model.is_empty() {
        return None;
    }
    let lowered = model.trim().to_lowercase();
    let id = SUFFIX.replace(&lowered, "");
    let version = |major: &str, minor: Option<&str>| match minor {
        Some(minor) => format!("{major}.{minor}"),
        None => major.to_string(),
    };
    if let Some(found) = MODERN.captures(&id) {
        let minor = found.get(3).map(|m| m.as_str());
        return Some(format!(
            "{} {}",
            capitalize(&found[1]),
            version(&found[2], minor)
        ));
    }
    if let Some(found) = LEGACY.captures(&id) {
        let minor = found.get(2).map(|m| m.as_str());
        return Some(format!(
            "{} {}",
            capitalize(&found[3]),
            version(&found[1], minor)
        ));
    }
    if FAMILIES.contains(&&*id) {
        return Some(capitalize(&id));
    }
    let trimmed = model.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// The local time of an epoch-millisecond instant.
fn local(ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Local::now)
}

/// "14:32", on a 24-hour clock whatever the language: the activity column is
/// five characters wide, and "04:59 PM" would wrap in it (`clockTime`).
pub fn clock_time(at: i64) -> String {
    let at = local(at);
    format!("{:02}:{:02}", at.hour(), at.minute())
}

/// The whole of a date and time as a tooltip carries it, in the language's
/// order (`Date.toLocaleString`): "10/26/2026, 3:00:00 PM" in English,
/// "26/10/2026 15:00:00" in French.
pub fn full_date(at: i64, locale: Locale) -> String {
    let at = local(at);
    match locale {
        Locale::En => {
            let (pm, hour) = at.hour12();
            format!(
                "{}/{}/{}, {hour}:{:02}:{:02}\u{202f}{}",
                at.month(),
                at.day(),
                at.year(),
                at.minute(),
                at.second(),
                if pm { "PM" } else { "AM" }
            )
        }
        Locale::Fr => format!(
            "{:02}/{:02}/{} {:02}:{:02}:{:02}",
            at.day(),
            at.month(),
            at.year(),
            at.hour(),
            at.minute(),
            at.second()
        ),
    }
}

/// A number with `places` decimals in the language's way: grouped by
/// thousands, with its decimal mark, and no trailing zero beyond `keep`.
fn decimal(value: f64, places: usize, keep: usize, locale: Locale) -> String {
    let fixed = format!("{:.*}", places, value.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut fraction = fraction.to_string();
    while fraction.len() > keep && fraction.ends_with('0') {
        fraction.pop();
    }
    let whole = Number::Int(whole.parse().unwrap_or(0)).format(locale);
    let mark = match locale {
        Locale::En => '.',
        Locale::Fr => ',',
    };
    let sign = if value < 0.0 { "-" } else { "" };
    if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}{mark}{fraction}")
    }
}

/// "1,84 $" in French, "$1.84" in English: the narrow symbol, since the
/// inspector has no room for "US$" and every agent bills in dollars. Two
/// decimals, three for a cost under a cent (`formatCost`).
pub fn format_cost(usd: f64, locale: Locale) -> String {
    let places = if usd > 0.0 && usd < 0.01 { 3 } else { 2 };
    let number = decimal(usd, places, 2, locale);
    match locale {
        Locale::En => match number.strip_prefix('-') {
            Some(rest) => format!("-${rest}"),
            None => format!("${number}"),
        },
        // A no-break space, as `Intl` writes it: the pair never wraps apart.
        Locale::Fr => format!("{number}\u{a0}$"),
    }
}

/// "34 %" in French (the narrow no-break space), "34%" in English.
pub fn format_percent(pct: f64, locale: Locale) -> String {
    let whole = Number::Int(pct.round() as i64).format(locale);
    match locale {
        Locale::En => format!("{whole}%"),
        Locale::Fr => format!("{whole}\u{202f}%"),
    }
}

/// `formatTokens`' "84k", with the space French puts between a number and
/// its unit ("84 k") — narrow and unbreakable, so the pair never wraps apart.
pub fn spaced_unit(formatted: &str, locale: Locale) -> String {
    if locale != Locale::Fr {
        return formatted.to_string();
    }
    let mut chars = formatted.chars().rev();
    match (chars.next(), chars.next()) {
        (Some(unit @ ('k' | 'M')), Some(digit)) if digit.is_ascii_digit() => {
            let body = &formatted[..formatted.len() - unit.len_utf8()];
            format!("{body}\u{202f}{unit}")
        }
        _ => formatted.to_string(),
    }
}

/// A time of day inside a sentence: "20 h 01" in French, the way French
/// typography writes it in running text, and "8:01 PM" in English. A column
/// of times, where they have to line up, keeps [`clock_time`].
pub fn prose_time(at: i64, locale: Locale) -> String {
    let at = local(at);
    match locale {
        Locale::Fr => format!("{} h {:02}", at.hour(), at.minute()),
        Locale::En => {
            let (pm, hour) = at.hour12();
            let meridiem = if pm { "PM" } else { "AM" };
            // `Intl` puts a narrow no-break space before the AM/PM mark.
            format!("{hour}:{:02}\u{202f}{meridiem}", at.minute())
        }
    }
}

const MONTHS_EN: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_FR: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// "Oct 20" in English, "20 oct." in French: a day and a short month.
fn short_date(at: i64, locale: Locale) -> String {
    let at = local(at);
    let month = at.month0() as usize;
    match locale {
        Locale::En => format!("{} {}", MONTHS_EN[month], at.day()),
        Locale::Fr => format!("{} {}", at.day(), MONTHS_FR[month]),
    }
}

fn weekday_name(day: Weekday, locale: Locale) -> &'static str {
    const EN: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const FR: [&str; 7] = [
        "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
    ];
    let index = day.num_days_from_monday() as usize;
    match locale {
        Locale::En => EN[index],
        Locale::Fr => FR[index],
    }
}

/// Whole calendar days from `from` to `to`; negative when `to` is earlier.
fn days_between(from: i64, to: i64) -> i64 {
    (local(to).date_naive() - local(from).date_naive()).num_days()
}

/// When something started, worded for a glance (`formatSince`): "12 min ago"
/// within the hour, then "today at 13 h 18", "yesterday at 18 h 02", and a
/// date beyond that. The parts, not the sentence, so the wording is checked
/// against the language that shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Since {
    JustNow,
    MinutesAgo(i64),
    TodayAt(String),
    YesterdayAt(String),
    DateAt { date: String, time: String },
}

pub fn since(at: i64, now: i64, locale: Locale) -> Since {
    let minutes = (now - at).div_euclid(60_000);
    if minutes < 1 {
        return Since::JustNow;
    }
    if minutes < 60 {
        return Since::MinutesAgo(minutes);
    }
    let time = prose_time(at, locale);
    match days_between(at, now) {
        0 => Since::TodayAt(time),
        1 => Since::YesterdayAt(time),
        _ => Since::DateAt {
            date: short_date(at, locale),
            time,
        },
    }
}

impl Since {
    pub fn text(&self, t: Translator) -> String {
        match self {
            Since::JustNow => t.t(Key::InspectorJustNow).into_owned(),
            Since::MinutesAgo(n) => t
                .t_with(Key::InspectorMinutesAgo, &args!("n" => *n))
                .into_owned(),
            Since::TodayAt(time) => t
                .t_with(Key::InspectorTodayAt, &args!("time" => time.as_str()))
                .into_owned(),
            Since::YesterdayAt(time) => t
                .t_with(Key::InspectorYesterdayAt, &args!("time" => time.as_str()))
                .into_owned(),
            Since::DateAt { date, time } => t
                .t_with(
                    Key::InspectorDateAt,
                    &args!("date" => date.as_str(), "time" => time.as_str()),
                )
                .into_owned(),
        }
    }
}

/// When a plan window frees up, as the end of a sentence (`resetWhen`):
/// "remise à zéro à 20 h 01" today, "demain à 1 h 05", "lundi à 9 h 00"
/// within the week, a date beyond. The navigator and the inspector both say
/// it this way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reset {
    Today(String),
    Tomorrow(String),
    Day { day: String, time: String },
    Date { date: String, time: String },
}

pub fn reset_when(at: i64, now: i64, locale: Locale) -> Reset {
    let time = prose_time(at, locale);
    let days = days_between(now, at);
    if days <= 0 {
        Reset::Today(time)
    } else if days == 1 {
        Reset::Tomorrow(time)
    } else if days < 7 {
        Reset::Day {
            day: weekday_name(local(at).weekday(), locale).to_string(),
            time,
        }
    } else {
        Reset::Date {
            date: short_date(at, locale),
            time,
        }
    }
}

impl Reset {
    pub fn text(&self, t: Translator) -> String {
        match self {
            Reset::Today(time) => t
                .t_with(Key::SidebarPlanResetToday, &args!("time" => time.as_str()))
                .into_owned(),
            Reset::Tomorrow(time) => t
                .t_with(
                    Key::SidebarPlanResetTomorrow,
                    &args!("time" => time.as_str()),
                )
                .into_owned(),
            Reset::Day { day, time } => t
                .t_with(
                    Key::SidebarPlanResetDay,
                    &args!("day" => day.as_str(), "time" => time.as_str()),
                )
                .into_owned(),
            Reset::Date { date, time } => t
                .t_with(
                    Key::SidebarPlanResetDate,
                    &args!("date" => date.as_str(), "time" => time.as_str()),
                )
                .into_owned(),
        }
    }
}

/// A plan window's usage, 0–100, or `None` when no session reported it
/// (`planUsage`).
pub fn plan_usage(limits: &[RateLimit], kind: &str) -> Option<RateLimit> {
    let hit = limits.iter().find(|limit| limit.kind == kind)?;
    Some(RateLimit {
        percent_used: hit.percent_used.round().clamp(0.0, 100.0),
        ..hit.clone()
    })
}

/// A plan window's reset instant in epoch milliseconds, when it names one.
pub fn resets_at_ms(limit: &RateLimit) -> Option<i64> {
    let at = limit.resets_at.as_deref()?;
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Days, NaiveDate};

    #[test]
    fn tone_turns_amber_at_70_and_red_at_90() {
        assert_eq!(usage_tone(69.0), Tone::Normal);
        assert_eq!(usage_tone(70.0), Tone::Warn);
        assert_eq!(usage_tone(89.0), Tone::Warn);
        assert_eq!(usage_tone(90.0), Tone::Danger);
    }

    #[test]
    fn claude_models_are_named_the_way_people_say_them() {
        let pretty = |model| pretty_model(Some(model)).unwrap();
        assert_eq!(pretty("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(pretty("claude-opus-4-1-20250805"), "Opus 4.1");
        assert_eq!(pretty("claude-opus-4-20250514"), "Opus 4");
        assert_eq!(pretty("claude-opus-5"), "Opus 5");
        assert_eq!(pretty("claude-sonnet-4-5[1m]"), "Sonnet 4.5");
        assert_eq!(pretty("claude-3-7-sonnet-20250219"), "Sonnet 3.7");
        assert_eq!(pretty("claude-3-haiku-20240307"), "Haiku 3");
        assert_eq!(pretty("opus"), "Opus");
    }

    #[test]
    fn any_other_model_is_left_as_it_came() {
        assert_eq!(pretty_model(Some("gpt-5-codex")).unwrap(), "gpt-5-codex");
        assert_eq!(pretty_model(None), None);
        assert_eq!(pretty_model(Some("")), None);
    }

    #[test]
    fn a_cost_is_written_the_way_the_language_does() {
        assert_eq!(format_cost(1.84, Locale::Fr), "1,84\u{a0}$");
        assert_eq!(format_cost(1.84, Locale::En), "$1.84");
        assert_eq!(format_cost(0.004, Locale::En), "$0.004");
        assert_eq!(format_cost(0.0, Locale::En), "$0.00");
        assert_eq!(format_cost(12.5, Locale::En), "$12.50");
        // Thousands are grouped the language's way.
        assert_eq!(format_cost(1234.5, Locale::En), "$1,234.50");
        assert_eq!(format_cost(1234.5, Locale::Fr), "1\u{202f}234,50\u{a0}$");
    }

    #[test]
    fn a_percentage_is_written_the_way_the_language_does() {
        assert_eq!(format_percent(34.0, Locale::Fr), "34\u{202f}%");
        assert_eq!(format_percent(34.0, Locale::En), "34%");
        assert_eq!(format_percent(34.5, Locale::En), "35%");
    }

    #[test]
    fn a_token_unit_is_spaced_in_french_only() {
        assert_eq!(spaced_unit("84k", Locale::Fr), "84\u{202f}k");
        assert_eq!(spaced_unit("1.2M", Locale::Fr), "1.2\u{202f}M");
        assert_eq!(spaced_unit("84k", Locale::En), "84k");
        assert_eq!(spaced_unit("512", Locale::Fr), "512");
    }

    fn limit(kind: &str, percent: f64) -> RateLimit {
        RateLimit {
            kind: kind.into(),
            percent_used: percent,
            resets_at: None,
        }
    }

    #[test]
    fn a_plan_window_is_read_clamped() {
        let limits = [limit("five_hour", 34.4), limit("seven_day", 140.0)];
        assert_eq!(plan_usage(&limits, "five_hour").unwrap().percent_used, 34.0);
        assert_eq!(
            plan_usage(&limits, "seven_day").unwrap().percent_used,
            100.0
        );
        assert!(plan_usage(&limits, "opus").is_none());
        assert!(plan_usage(&[], "five_hour").is_none());
    }

    #[test]
    fn a_reset_instant_is_read_from_its_iso_text() {
        let mut window = limit("five_hour", 10.0);
        assert_eq!(resets_at_ms(&window), None);
        window.resets_at = Some("2026-10-26T20:01:00Z".into());
        assert_eq!(resets_at_ms(&window), Some(1_793_044_860_000));
        window.resets_at = Some("soon".into());
        assert_eq!(resets_at_ms(&window), None);
    }

    /// October 2026, `day` counted past the 31st rolling into November as
    /// `new Date(2026, 9, day)` does.
    fn at(day: u64, hours: u32, minutes: u32) -> i64 {
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap() + Days::new(day - 1);
        Local
            .from_local_datetime(&date.and_hms_opt(hours, minutes, 0).unwrap())
            .earliest()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn a_time_in_a_sentence_is_written_the_way_each_language_does() {
        assert_eq!(prose_time(at(26, 20, 1), Locale::Fr), "20 h 01");
        assert_eq!(prose_time(at(26, 9, 5), Locale::Fr), "9 h 05");
        assert_eq!(prose_time(at(26, 20, 1), Locale::En), "8:01\u{202f}PM");
        assert_eq!(prose_time(at(26, 0, 7), Locale::En), "12:07\u{202f}AM");
        assert_eq!(prose_time(at(26, 12, 0), Locale::En), "12:00\u{202f}PM");
    }

    #[test]
    fn the_activity_column_keeps_a_24_hour_clock() {
        assert_eq!(clock_time(at(26, 14, 32)), "14:32");
        assert_eq!(clock_time(at(26, 4, 59)), "04:59");
    }

    #[test]
    fn a_start_time_is_worded_relative_to_now() {
        let now = at(26, 15, 0);
        assert_eq!(since(now - 20_000, now, Locale::En), Since::JustNow);
        assert_eq!(
            since(now - 12 * 60_000, now, Locale::En),
            Since::MinutesAgo(12)
        );
        assert_eq!(
            since(at(26, 13, 18), now, Locale::Fr),
            Since::TodayAt("13 h 18".into())
        );
        assert_eq!(
            since(at(25, 18, 2), now, Locale::Fr),
            Since::YesterdayAt("18 h 02".into())
        );
        assert_eq!(
            since(at(20, 9, 5), now, Locale::Fr),
            Since::DateAt {
                date: "20 oct.".into(),
                time: "9 h 05".into()
            }
        );
        assert_eq!(
            since(at(20, 9, 5), now, Locale::En),
            Since::DateAt {
                date: "Oct 20".into(),
                time: "9:05\u{202f}AM".into()
            }
        );
        // A start in the future (a clock set back) reads as just now.
        assert_eq!(since(now + 5 * 60_000, now, Locale::En), Since::JustNow);
    }

    #[test]
    fn a_since_is_spoken_in_the_language_asked() {
        let fr = Translator::new(Locale::Fr);
        assert_eq!(Since::MinutesAgo(12).text(fr), "il y a 12\u{a0}min");
        assert_eq!(
            Since::TodayAt("13 h 18".into()).text(Translator::new(Locale::En)),
            "today at 13 h 18"
        );
    }

    #[test]
    fn a_plan_window_says_when_it_resets_nearest_first() {
        let now = at(26, 15, 0);
        assert_eq!(
            reset_when(at(26, 20, 1), now, Locale::Fr),
            Reset::Today("20 h 01".into())
        );
        assert_eq!(
            reset_when(at(27, 1, 5), now, Locale::Fr),
            Reset::Tomorrow("1 h 05".into())
        );
        assert_eq!(
            reset_when(at(30, 9, 0), now, Locale::Fr),
            Reset::Day {
                day: "vendredi".into(),
                time: "9 h 00".into()
            }
        );
        assert_eq!(
            reset_when(at(26 + 9, 9, 0), now, Locale::Fr),
            Reset::Date {
                date: "4 nov.".into(),
                time: "9 h 00".into()
            }
        );
        assert_eq!(
            reset_when(at(30, 9, 0), now, Locale::En),
            Reset::Day {
                day: "Friday".into(),
                time: "9:00\u{202f}AM".into()
            }
        );
    }
}
