//! The numbers of the info strip and the animation bar, written the way
//! `Intl.NumberFormat` wrote them in `ModelPanel.tsx` (`formatCount`,
//! `formatLength`, the animation's `seconds`): `hangar-i18n`'s `Number` groups
//! digits and keeps three decimals, which is not what a status line wants.

use hangar_i18n::{Locale, Number};

/// Counts are written in full up to this many, and compact above.
const COMPACT_FROM: u64 = 100_000;

/// A count of triangles, vertices or materials: in full up to a hundred
/// thousand, compact above (`4 444`, `1.2M`, `1,2 M`).
pub fn count(value: u64, locale: Locale) -> String {
    if value < COMPACT_FROM {
        return grouped(value, locale);
    }
    // The unit is the largest the value reaches. Rounded to one decimal, a
    // value just under the next unit becomes it ("999.95K" is "1M").
    const UNITS: [(f64, &str, &str); 4] = [
        (1e3, "K", "\u{a0}k"),
        (1e6, "M", "\u{a0}M"),
        (1e9, "B", "\u{a0}Md"),
        (1e12, "T", "\u{a0}Bn"),
    ];
    let value = value as f64;
    let mut at = UNITS
        .iter()
        .rposition(|(unit, _, _)| value >= *unit)
        .unwrap_or(0);
    let mut scaled = (value / UNITS[at].0 * 10.0).round() / 10.0;
    if scaled >= 1000.0 && at + 1 < UNITS.len() {
        at += 1;
        scaled = (value / UNITS[at].0 * 10.0).round() / 10.0;
    }
    let (_, english, french) = UNITS[at];
    let number = decimal(scaled, 1, true, locale);
    match locale {
        Locale::En => format!("{number}{english}"),
        Locale::Fr => format!("{number}{french}"),
    }
}

/// An extent of the bounding box to three significant figures, which is what a
/// size in a status line can use: `2`, `1.23`, `12.3`, `1,230`, `0.0123`.
pub fn length(value: f64, locale: Locale) -> String {
    if !value.is_finite() || value == 0.0 {
        return grouped(0, locale);
    }
    let magnitude = value.abs().log10().floor() as i32;
    // Digits after the point that leave three figures; none for a number of
    // 1,000 or more (it is rounded to the tens, `1,230`).
    let decimals = (2 - magnitude).max(0) as usize;
    let scale = 10f64.powi(2 - magnitude);
    let rounded = (value.abs() * scale).round() / scale;
    decimal(rounded, decimals, true, locale)
}

/// A time in seconds with one decimal, always shown (`0.0`, `2,5`).
pub fn seconds(value: f32, locale: Locale) -> String {
    decimal(f64::from(value.max(0.0)), 1, false, locale)
}

/// A size in megabytes, to a decimal below ten and whole above
/// (`maximumFractionDigits: bytes < 10 MB ? 1 : 0`); the unit is the
/// sentence's (`model.megabytes`).
pub fn megabytes(bytes: u64, locale: Locale) -> String {
    const MB: u64 = 1024 * 1024;
    let size = bytes as f64 / MB as f64;
    decimal(size, usize::from(bytes < 10 * MB), true, locale)
}

fn grouped(value: u64, locale: Locale) -> String {
    Number::from(value).format(locale)
}

/// `value` with `decimals` digits after the separator of `locale`, the whole
/// part grouped; `trim` drops zeros at the end of the fraction.
fn decimal(value: f64, decimals: usize, trim: bool, locale: Locale) -> String {
    // Halves round away from zero, as `Intl.NumberFormat` does; formatting
    // the float would round them to even.
    let scale = 10f64.powi(decimals as i32);
    let value = (value * scale).round() / scale;
    let text = format!("{value:.decimals$}");
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let fraction = if trim {
        fraction.trim_end_matches('0')
    } else {
        fraction
    };
    let mut out = grouped(whole.parse().unwrap_or(0), locale);
    if !fraction.is_empty() {
        out.push(match locale {
            Locale::En => '.',
            Locale::Fr => ',',
        });
        out.push_str(fraction);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_written_in_full_below_a_hundred_thousand() {
        assert_eq!(count(0, Locale::En), "0");
        assert_eq!(count(4444, Locale::En), "4,444");
        assert_eq!(count(4444, Locale::Fr), "4\u{202f}444");
        assert_eq!(count(99_999, Locale::En), "99,999");
    }

    #[test]
    fn counts_turn_compact_from_a_hundred_thousand() {
        assert_eq!(count(100_000, Locale::En), "100K");
        assert_eq!(count(123_456, Locale::En), "123.5K");
        assert_eq!(count(1_234_567, Locale::En), "1.2M");
        assert_eq!(count(15_000_000, Locale::En), "15M");
        assert_eq!(count(2_500_000_000, Locale::En), "2.5B");
    }

    #[test]
    fn compact_counts_use_the_french_units_and_comma() {
        assert_eq!(count(123_456, Locale::Fr), "123,5\u{a0}k");
        assert_eq!(count(1_234_567, Locale::Fr), "1,2\u{a0}M");
        assert_eq!(count(2_500_000_000, Locale::Fr), "2,5\u{a0}Md");
    }

    #[test]
    fn a_count_that_rounds_up_to_the_next_unit_takes_it() {
        assert_eq!(count(999_960, Locale::En), "1M");
        assert_eq!(count(999_949, Locale::En), "999.9K");
    }

    #[test]
    fn lengths_keep_three_significant_figures() {
        assert_eq!(length(2.0, Locale::En), "2");
        assert_eq!(length(1.2345, Locale::En), "1.23");
        assert_eq!(length(12.345, Locale::En), "12.3");
        assert_eq!(length(123.45, Locale::En), "123");
        assert_eq!(length(0.012345, Locale::En), "0.0123");
        assert_eq!(length(0.0, Locale::En), "0");
    }

    #[test]
    fn large_lengths_are_rounded_and_grouped() {
        assert_eq!(length(1234.5, Locale::En), "1,230");
        assert_eq!(length(1234.5, Locale::Fr), "1\u{202f}230");
        // A carry into the next figure: 9.996 is 10, 99.96 is 100.
        assert_eq!(length(9.996, Locale::En), "10");
        assert_eq!(length(99.96, Locale::En), "100");
        assert_eq!(length(999.6, Locale::En), "1,000");
    }

    #[test]
    fn lengths_use_the_french_decimal_comma() {
        assert_eq!(length(1.2345, Locale::Fr), "1,23");
        assert_eq!(length(0.012345, Locale::Fr), "0,0123");
    }

    #[test]
    fn a_length_that_is_not_a_number_is_zero() {
        assert_eq!(length(f64::NAN, Locale::En), "0");
        assert_eq!(length(f64::INFINITY, Locale::En), "0");
    }

    #[test]
    fn megabytes_have_a_decimal_below_ten() {
        const MB: u64 = 1024 * 1024;
        assert_eq!(megabytes(0, Locale::En), "0");
        assert_eq!(megabytes(MB / 4, Locale::En), "0.3");
        assert_eq!(megabytes(5 * MB / 2, Locale::En), "2.5");
        assert_eq!(megabytes(5 * MB / 2, Locale::Fr), "2,5");
        assert_eq!(megabytes(12 * MB + MB / 2, Locale::En), "13");
        assert_eq!(megabytes(1500 * MB, Locale::En), "1,500");
    }

    #[test]
    fn seconds_always_show_one_decimal() {
        assert_eq!(seconds(0.0, Locale::En), "0.0");
        assert_eq!(seconds(2.5, Locale::En), "2.5");
        assert_eq!(seconds(2.5, Locale::Fr), "2,5");
        assert_eq!(seconds(12.0, Locale::En), "12.0");
        assert_eq!(seconds(-1.0, Locale::En), "0.0");
    }
}
