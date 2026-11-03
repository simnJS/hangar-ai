//! What fills the `{name}` placeholders of a string, and how a number is
//! written in each language.

use std::borrow::Cow;

use crate::Locale;

/// A number a sentence counts or measures with. It is kept as it was given, not
/// turned into a float, so a count of any size keeps every digit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    Int(i64),
    Float(f64),
}

macro_rules! number_from_int {
    ($($type:ty),*) => {$(
        impl From<$type> for Number {
            fn from(value: $type) -> Number {
                Number::Int(i64::from(value))
            }
        }
    )*};
}
number_from_int!(i8, i16, i32, i64, u8, u16, u32);

impl From<u64> for Number {
    fn from(value: u64) -> Number {
        // Past i64 a float is all that is left, and no sentence counts that high.
        i64::try_from(value).map_or(Number::Float(value as f64), Number::Int)
    }
}

impl From<usize> for Number {
    fn from(value: usize) -> Number {
        Number::from(value as u64)
    }
}

impl From<isize> for Number {
    fn from(value: isize) -> Number {
        Number::Int(value as i64)
    }
}

impl From<f32> for Number {
    fn from(value: f32) -> Number {
        // Through the shortest decimal text, so 0.1_f32 is 0.1 and not 0.10000000149.
        Number::Float(value.to_string().parse().unwrap_or(f64::from(value)))
    }
}

impl From<f64> for Number {
    fn from(value: f64) -> Number {
        Number::Float(value)
    }
}

impl Number {
    /// The text of this number in `locale`: grouped by thousands (a comma in
    /// English, a narrow no-break space in French), with at most three decimals
    /// after a point or a comma, like `Intl.NumberFormat` does by default, so
    /// that "10 000 lignes" and "10,000 lines" come out of the same count.
    pub fn format(self, locale: Locale) -> String {
        let (group, decimal) = match locale {
            Locale::En => (",", '.'),
            // U+202F: the space French groups digits with, which does not break.
            Locale::Fr => ("\u{202f}", ','),
        };
        let (negative, digits, fraction) = match self {
            Number::Int(value) => (value < 0, value.unsigned_abs().to_string(), String::new()),
            Number::Float(value) if value.is_nan() => return "NaN".to_owned(),
            Number::Float(value) if value.is_infinite() => {
                return if value < 0.0 { "-∞" } else { "∞" }.to_owned();
            }
            Number::Float(value) => {
                let rounded = format!("{:.3}", value.abs());
                let (whole, fraction) = rounded.split_once('.').unwrap_or((&rounded, ""));
                (
                    value.is_sign_negative(),
                    whole.to_owned(),
                    fraction.trim_end_matches('0').to_owned(),
                )
            }
        };
        let mut out = String::with_capacity(
            digits.len() + digits.len() / 3 * group.len() + fraction.len() + 2,
        );
        if negative {
            out.push('-');
        }
        for (index, digit) in digits.chars().enumerate() {
            if index > 0 && (digits.len() - index) % 3 == 0 {
                out.push_str(group);
            }
            out.push(digit);
        }
        if !fraction.is_empty() {
            out.push(decimal);
            out.push_str(&fraction);
        }
        out
    }

    /// The number without its sign and without a decimal part, if it is whole
    /// enough to compare: what the plural rules look at.
    pub(crate) fn magnitude(self) -> Magnitude {
        match self {
            Number::Int(value) => Magnitude {
                integer: value.unsigned_abs() as f64,
                whole: true,
            },
            Number::Float(value) if value.is_finite() => {
                let value = value.abs();
                Magnitude {
                    integer: value.trunc(),
                    whole: value.fract() == 0.0,
                }
            }
            Number::Float(_) => Magnitude {
                integer: f64::INFINITY,
                whole: false,
            },
        }
    }
}

/// The integer part of a number and whether anything follows it, the two facts
/// the CLDR plural rules of English and French are written in.
#[derive(Clone, Copy)]
pub(crate) struct Magnitude {
    pub integer: f64,
    pub whole: bool,
}

/// The value of one placeholder: a number, written the way the language writes
/// it, or text, which is put in as it is. Only the strings of the dictionary
/// are typeset, never what fills them, so a task title or a command reads
/// exactly as it was typed.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg<'a> {
    Num(Number),
    Str(Cow<'a, str>),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Arg<'a> {
        Arg::Str(Cow::Borrowed(value))
    }
}

impl<'a> From<&'a String> for Arg<'a> {
    fn from(value: &'a String) -> Arg<'a> {
        Arg::Str(Cow::Borrowed(value))
    }
}

impl From<String> for Arg<'_> {
    fn from(value: String) -> Self {
        Arg::Str(Cow::Owned(value))
    }
}

impl<'a> From<Cow<'a, str>> for Arg<'a> {
    fn from(value: Cow<'a, str>) -> Arg<'a> {
        Arg::Str(value)
    }
}

impl From<Number> for Arg<'_> {
    fn from(value: Number) -> Self {
        Arg::Num(value)
    }
}

macro_rules! arg_from_number {
    ($($type:ty),*) => {$(
        impl From<$type> for Arg<'_> {
            fn from(value: $type) -> Self {
                Arg::Num(Number::from(value))
            }
        }
    )*};
}
arg_from_number!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

/// The arguments of a translation, as a list a call can borrow:
/// `t_with(Key::ZoomShow, &args!("name" => name))`. A name is looked up for
/// each `{name}` of the string; one that is not in the string is ignored, and a
/// `{name}` with no argument stays as written.
#[macro_export]
macro_rules! args {
    ($($name:literal => $value:expr),* $(,)?) => {
        [$(($name, $crate::Arg::from($value))),*]
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_grouped_the_way_each_language_groups_them() {
        assert_eq!(Number::from(10_000).format(Locale::En), "10,000");
        assert_eq!(Number::from(10_000).format(Locale::Fr), "10\u{202f}000");
        assert_eq!(Number::from(999).format(Locale::Fr), "999");
        assert_eq!(Number::from(1_000).format(Locale::En), "1,000");
        assert_eq!(
            Number::from(1_234_567).format(Locale::Fr),
            "1\u{202f}234\u{202f}567"
        );
        assert_eq!(Number::from(0).format(Locale::En), "0");
        assert_eq!(Number::from(-12_345).format(Locale::En), "-12,345");
        assert_eq!(
            Number::from(i64::MIN).format(Locale::En),
            "-9,223,372,036,854,775,808"
        );
    }

    #[test]
    fn decimals_follow_the_language_and_stop_at_three() {
        assert_eq!(Number::from(1234.5).format(Locale::En), "1,234.5");
        assert_eq!(Number::from(1234.5).format(Locale::Fr), "1\u{202f}234,5");
        assert_eq!(Number::from(0.25).format(Locale::Fr), "0,25");
        assert_eq!(Number::from(3.0).format(Locale::En), "3");
        assert_eq!(Number::from(0.12345).format(Locale::En), "0.123");
        assert_eq!(Number::from(2.9999).format(Locale::En), "3");
        assert_eq!(Number::from(-1.5).format(Locale::Fr), "-1,5");
        assert_eq!(Number::from(0.1_f32).format(Locale::En), "0.1");
    }

    #[test]
    fn what_is_not_a_finite_number_still_reads() {
        assert_eq!(Number::from(f64::NAN).format(Locale::En), "NaN");
        assert_eq!(Number::from(f64::INFINITY).format(Locale::Fr), "∞");
        assert_eq!(Number::from(f64::NEG_INFINITY).format(Locale::Fr), "-∞");
    }

    #[test]
    fn an_argument_keeps_the_kind_it_was_given() {
        assert_eq!(Arg::from(3_usize), Arg::Num(Number::Int(3)));
        assert_eq!(Arg::from("Ava"), Arg::Str("Ava".into()));
        assert_eq!(Arg::from(String::from("Ava")), Arg::Str("Ava".into()));
        // A count that is already text keeps the format its caller chose.
        assert_eq!(Arg::from("10000"), Arg::Str("10000".into()));
    }

    #[test]
    fn the_macro_builds_a_borrowable_list() {
        let name = String::from("Ava");
        let list = args!("name" => &name, "n" => 2, "ratio" => 0.5);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], ("name", Arg::Str("Ava".into())));
        let none: [(&str, Arg); 0] = args!();
        assert!(none.is_empty());
    }
}
