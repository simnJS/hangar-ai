//! The shape of the generated dictionary and how a key finds its message.

use crate::generated::{Key, MESSAGES};
use crate::{Locale, PluralCategory};

/// One language's text for a message: a single string, or the `_one` and
/// `_other` pair of a counted one. A plain message holds the same string twice,
/// so a lookup never has to ask which kind it is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Text {
    one: &'static str,
    other: &'static str,
}

impl Text {
    pub(crate) const fn plain(text: &'static str) -> Text {
        Text {
            one: text,
            other: text,
        }
    }

    pub(crate) const fn plural(one: &'static str, other: &'static str) -> Text {
        Text { one, other }
    }

    /// The string for `category`; `None` is a message read without a count, which
    /// shows the general form.
    pub(crate) fn form(self, category: Option<PluralCategory>) -> &'static str {
        match category {
            Some(PluralCategory::One) => self.one,
            Some(PluralCategory::Other) | None => self.other,
        }
    }
}

/// A message in every language, under its id.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Message {
    pub(crate) id: &'static str,
    pub(crate) en: Text,
    pub(crate) fr: Text,
}

impl Message {
    pub(crate) fn text(&self, locale: Locale) -> Text {
        match locale {
            Locale::En => self.en,
            Locale::Fr => self.fr,
        }
    }
}

impl Key {
    /// The key as the web app spells it, `"sidebar.workspaces"`: what a dynamic
    /// lookup builds, and what shows in place of a string nobody wrote.
    pub fn id(self) -> &'static str {
        self.message().id
    }

    /// The key that has this id, if there is one. For the keys a program
    /// computes (`format!("pane.state.{state}")`), where a typo cannot be caught
    /// by the compiler but a test can ask for every id it builds.
    pub fn from_id(id: &str) -> Option<Key> {
        MESSAGES
            .binary_search_by(|message| message.id.cmp(id))
            .ok()
            .map(|index| Key::ALL[index])
    }

    /// The dictionary's string for this key, with its placeholders as written:
    /// the `_one` or `_other` string of a counted message, the one string of a
    /// plain one. For tests and tools that look at the strings themselves;
    /// showing text goes through [`t`](crate::t), which fills them.
    pub fn template(self, locale: Locale, form: PluralCategory) -> &'static str {
        self.message().text(locale).form(Some(form))
    }

    pub(crate) fn message(self) -> &'static Message {
        &MESSAGES[self as usize]
    }
}

/// What a lookup was given: a key the compiler knows, or text that may or may
/// not be one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRef<'a> {
    Known(Key),
    Unknown(&'a str),
}

/// Anything that names a message: a [`Key`], or a string for the keys that are
/// computed at run time. A string is looked up once; one that names nothing
/// translates to itself, so a mistake shows on screen where it can be seen
/// instead of as an empty label.
pub trait AsKey {
    fn as_key(&self) -> KeyRef<'_>;
}

impl AsKey for Key {
    fn as_key(&self) -> KeyRef<'_> {
        KeyRef::Known(*self)
    }
}

impl AsKey for str {
    fn as_key(&self) -> KeyRef<'_> {
        Key::from_id(self).map_or(KeyRef::Unknown(self), KeyRef::Known)
    }
}

impl AsKey for String {
    fn as_key(&self) -> KeyRef<'_> {
        self.as_str().as_key()
    }
}

impl<T: AsKey + ?Sized> AsKey for &T {
    fn as_key(&self) -> KeyRef<'_> {
        (**self).as_key()
    }
}

impl AsKey for std::borrow::Cow<'_, str> {
    fn as_key(&self) -> KeyRef<'_> {
        self.as_ref().as_key()
    }
}
