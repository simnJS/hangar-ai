//! Which form of a counted string a number takes.

use crate::{Locale, Number};

/// The plural category a count falls in. CLDR names more (zero, two, few, many)
/// for other languages; English and French use two, and the dictionaries hold
/// one string for each. French has a `many` for whole millions ("1 000 000 de
/// fichiers"), which the dictionaries never fill, so it reads as `Other`, as it
/// did when `Intl.PluralRules` picked and the missing variant fell back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluralCategory {
    One,
    Other,
}

impl PluralCategory {
    /// The category of `n` in `locale`, by the CLDR rules:
    ///
    /// - English: "one" for exactly 1, so 0 and 1.5 are "other".
    /// - French: "one" when the integer part is 0 or 1, so 0, 1 and 1.5 are
    ///   "one" ("0 fichier", "1,5 Go") and 2 is "other".
    ///
    /// The sign does not matter: -1 is as singular as 1.
    pub fn of(locale: Locale, n: Number) -> PluralCategory {
        let magnitude = n.magnitude();
        let one = match locale {
            Locale::En => magnitude.whole && magnitude.integer == 1.0,
            Locale::Fr => magnitude.integer <= 1.0,
        };
        if one {
            PluralCategory::One
        } else {
            PluralCategory::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PluralCategory::{One, Other};
    use super::*;

    fn en(n: impl Into<Number>) -> PluralCategory {
        PluralCategory::of(Locale::En, n.into())
    }

    fn fr(n: impl Into<Number>) -> PluralCategory {
        PluralCategory::of(Locale::Fr, n.into())
    }

    #[test]
    fn english_has_one_only_for_exactly_one() {
        assert_eq!(
            [en(0), en(1), en(2), en(11), en(100)],
            [Other, One, Other, Other, Other]
        );
        assert_eq!(
            [en(1.0), en(1.5), en(0.5), en(-1)],
            [One, Other, Other, One]
        );
    }

    #[test]
    fn french_counts_zero_and_one_as_one() {
        assert_eq!(
            [fr(0), fr(1), fr(2), fr(11), fr(100)],
            [One, One, Other, Other, Other]
        );
        assert_eq!(
            [fr(1.5), fr(0.5), fr(2.5), fr(-1), fr(-2)],
            [One, One, Other, One, Other]
        );
        // Whole millions are "many" in CLDR; the dictionaries have no such form.
        assert_eq!(fr(1_000_000), Other);
    }

    #[test]
    fn what_is_not_a_number_is_other() {
        assert_eq!(
            [
                en(f64::NAN),
                en(f64::INFINITY),
                fr(f64::NAN),
                fr(f64::INFINITY)
            ],
            [Other; 4]
        );
    }
}
