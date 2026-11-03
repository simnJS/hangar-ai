//! The languages Hangar speaks and how one is picked.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A language of the interface. Settings save it as its id, "en" or "fr", the
/// same text the web app wrote to `state.json`, so a settings file carries over.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum Locale {
    /// English, the reference every other language is checked against.
    #[default]
    #[serde(rename = "en")]
    En,
    /// French.
    #[serde(rename = "fr")]
    Fr,
}

/// One entry of the language picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocaleOption {
    pub id: Locale,
    /// The language's name in that language, which is how a picker shows it.
    pub label: &'static str,
}

/// The languages on offer, in the order the picker lists them (`LOCALES` of
/// `src/i18n.ts`).
pub const LOCALES: [LocaleOption; 2] = [
    LocaleOption {
        id: Locale::En,
        label: "English",
    },
    LocaleOption {
        id: Locale::Fr,
        label: "Français",
    },
];

impl Locale {
    /// Every language, in the order of `LOCALES`.
    pub const ALL: [Locale; 2] = [Locale::En, Locale::Fr];

    /// "en" or "fr": the text settings save and `FromStr` reads back.
    pub const fn id(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Fr => "fr",
        }
    }

    /// The language's name in that language: "English" or "Français".
    pub const fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Fr => "Français",
        }
    }

    /// The language a BCP 47 or POSIX tag asks for ("fr", "fr-CA",
    /// "fr_FR.UTF-8"), or `None` when it is one Hangar does not ship. Only the
    /// language part counts, as in the web app, so a regional variant gets its
    /// language.
    pub fn from_tag(tag: &str) -> Option<Locale> {
        let tag = tag.trim();
        let language = tag.split(['-', '_', '.', '@']).next()?;
        match language.to_ascii_lowercase().as_str() {
            "en" => Some(Locale::En),
            "fr" => Some(Locale::Fr),
            _ => None,
        }
    }

    /// The language of the operating system, read from the usual environment
    /// variables (`LC_ALL`, `LC_MESSAGES`, `LANG`), and English for anything
    /// Hangar does not ship, like the web app's `systemLocale()`.
    ///
    /// Windows and a macOS app launched from the Finder set none of those, so
    /// they get English here; the app can ask the platform itself and hand the
    /// tag to [`Locale::from_tag`] instead.
    pub fn system() -> Locale {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .and_then(|value| Locale::from_tag(&value))
            .unwrap_or(Locale::En)
    }
}

/// The language's name, so a pick list can show a `Locale` as it is.
impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What `Locale::from_str` says about an id that is not "en" or "fr".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownLocale(pub String);

impl fmt::Display for UnknownLocale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown locale {:?}, expected \"en\" or \"fr\"", self.0)
    }
}

impl std::error::Error for UnknownLocale {}

impl FromStr for Locale {
    type Err = UnknownLocale;

    /// Reads an id exactly: "en" and "fr", nothing looser. Use
    /// [`Locale::from_tag`] for text the operating system wrote.
    fn from_str(id: &str) -> Result<Self, Self::Err> {
        Locale::ALL
            .into_iter()
            .find(|locale| locale.id() == id)
            .ok_or_else(|| UnknownLocale(id.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_labels_are_those_of_the_web_app() {
        assert_eq!(
            LOCALES.map(|option| (option.id.id(), option.label)),
            [("en", "English"), ("fr", "Français")]
        );
        assert!(
            LOCALES
                .iter()
                .all(|option| option.id.label() == option.label)
        );
    }

    #[test]
    fn an_id_round_trips_through_text_and_json() {
        for locale in Locale::ALL {
            assert_eq!(locale.id().parse(), Ok(locale));
            let json = format!("\"{}\"", locale.id());
            assert_eq!(serde_json::to_string(&locale).unwrap(), json);
            assert_eq!(serde_json::from_str::<Locale>(&json).unwrap(), locale);
        }
        assert!("de".parse::<Locale>().is_err());
        assert!("FR".parse::<Locale>().is_err());
        assert!(serde_json::from_str::<Locale>("\"de\"").is_err());
    }

    #[test]
    fn a_tag_gets_its_language() {
        assert_eq!(Locale::from_tag("fr"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("fr-CA"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("fr_FR.UTF-8"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("FR"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag(" en_US.UTF-8"), Some(Locale::En));
        assert_eq!(Locale::from_tag("de_DE"), None);
        // A longer language code is another language, not a misspelt French.
        assert_eq!(Locale::from_tag("fra"), None);
        assert_eq!(Locale::from_tag("C"), None);
        assert_eq!(Locale::from_tag(""), None);
    }

    #[test]
    fn a_picker_shows_the_label() {
        assert_eq!(Locale::Fr.to_string(), "Français");
        assert_eq!(Locale::default(), Locale::En);
    }
}
