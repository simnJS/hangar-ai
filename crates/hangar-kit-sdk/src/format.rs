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

#[cfg(test)]
mod tests {
    use super::*;

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
