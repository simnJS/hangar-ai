//! Turning a key into the text of a language.

use std::borrow::Cow;

use crate::{Arg, AsKey, KeyRef, Locale, Number, PluralCategory};

/// The text of `key` in `locale`, as the dictionary has it: placeholders are
/// left as written. A counted message shows its general form, since nothing
/// says how many. A key nobody wrote comes back as itself.
pub fn t(locale: Locale, key: impl AsKey) -> Cow<'static, str> {
    render(locale, key.as_key(), None, &[])
}

/// The text of `key` with its `{name}` placeholders filled from `args`
/// (see [`args!`](crate::args)).
///
/// As in the web app, a number named `n` among the arguments is the count of a
/// counted message and picks its singular or plural form, so a call can pass
/// `args!("n" => 3, "name" => name)` or use [`tn`] to the same effect.
pub fn t_with(locale: Locale, key: impl AsKey, args: &[(&str, Arg)]) -> Cow<'static, str> {
    render(locale, key.as_key(), None, args)
}

/// The text of a counted message: the form `n` takes in `locale` (French reads
/// 0 and 1 as singular, English only 1), with `{n}` written as a number in the
/// language and the other placeholders filled from `args`.
pub fn tn(
    locale: Locale,
    key: impl AsKey,
    n: impl Into<Number>,
    args: &[(&str, Arg)],
) -> Cow<'static, str> {
    render(locale, key.as_key(), Some(n.into()), args)
}

/// The current language, held by the app in its state so a view does not
/// carry the locale to every call. It is two bytes and `Copy`: put it in a
/// message or a closure without a thought.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Translator {
    locale: Locale,
}

impl Translator {
    pub const fn new(locale: Locale) -> Translator {
        Translator { locale }
    }

    /// A translator in the language of the operating system.
    pub fn system() -> Translator {
        Translator::new(Locale::system())
    }

    pub const fn locale(self) -> Locale {
        self.locale
    }

    /// [`t`] in this language.
    pub fn t(self, key: impl AsKey) -> Cow<'static, str> {
        t(self.locale, key)
    }

    /// [`t_with`] in this language.
    pub fn t_with(self, key: impl AsKey, args: &[(&str, Arg)]) -> Cow<'static, str> {
        t_with(self.locale, key, args)
    }

    /// [`tn`] in this language.
    pub fn tn(
        self,
        key: impl AsKey,
        n: impl Into<Number>,
        args: &[(&str, Arg)],
    ) -> Cow<'static, str> {
        tn(self.locale, key, n, args)
    }
}

impl From<Locale> for Translator {
    fn from(locale: Locale) -> Translator {
        Translator::new(locale)
    }
}

fn render(
    locale: Locale,
    key: KeyRef<'_>,
    count: Option<Number>,
    args: &[(&str, Arg)],
) -> Cow<'static, str> {
    // The count a plural is chosen by: the one `tn` was given, or a number
    // named `n` in the arguments, as `params.n` was in the web app.
    let count = count.or_else(|| {
        args.iter().rev().find_map(|(name, arg)| match arg {
            Arg::Num(number) if *name == "n" => Some(*number),
            _ => None,
        })
    });

    let template: &str = match key {
        KeyRef::Known(key) => {
            let category = count.map(|n| PluralCategory::of(locale, n));
            key.message().text(locale).form(category)
        }
        // A key that names nothing is shown as written, placeholders and all.
        KeyRef::Unknown(text) => {
            return if args.is_empty() && count.is_none() {
                Cow::Owned(text.to_owned())
            } else {
                Cow::Owned(fill(text, locale, count, args))
            };
        }
    };

    if template.contains('{') && (!args.is_empty() || count.is_some()) {
        Cow::Owned(fill(template, locale, count, args))
    } else {
        Cow::Borrowed(template)
    }
}

/// Puts the arguments where `{name}` stands. A name that is not an argument
/// stays as written, so a missing one shows instead of vanishing; `{n}` is the
/// count when no argument says otherwise. Names are letters, digits and `_`,
/// the `\w` of the web app's pattern, so braces around anything else are text.
fn fill(template: &str, locale: Locale, count: Option<Number>, args: &[(&str, Arg)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let name_len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count();
        if name_len > 0 && after[name_len..].starts_with('}') {
            let name = &after[..name_len];
            // The last of two arguments with one name wins, as in an object literal.
            let value = args
                .iter()
                .rev()
                .find(|(candidate, _)| *candidate == name)
                .map(|(_, arg)| arg);
            match (value, count) {
                (Some(Arg::Str(text)), _) => out.push_str(text),
                (Some(Arg::Num(number)), _) => out.push_str(&number.format(locale)),
                (None, Some(n)) if name == "n" => out.push_str(&n.format(locale)),
                (None, _) => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            }
            rest = &after[name_len + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, args};

    #[test]
    fn a_plain_message_is_borrowed_from_the_dictionary() {
        let text = t(Locale::En, Key::SidebarWorkspaces);
        assert_eq!(text, "Workspaces");
        assert!(matches!(text, Cow::Borrowed(_)));
        assert_eq!(t(Locale::Fr, Key::SidebarSettings), "Réglages");
    }

    #[test]
    fn a_string_key_finds_the_same_message() {
        assert_eq!(
            t(Locale::Fr, "sidebar.settings"),
            t(Locale::Fr, Key::SidebarSettings)
        );
        let computed = format!("sidebar.{}", "settings");
        assert_eq!(t(Locale::En, &computed), "Settings");
        assert_eq!(t(Locale::En, computed), "Settings");
    }

    #[test]
    fn an_unknown_key_translates_to_itself() {
        assert_eq!(t(Locale::Fr, "no.such.key"), "no.such.key");
        assert_eq!(
            t_with(Locale::Fr, "no.such.key", &args!("n" => 3)),
            "no.such.key"
        );
        assert_eq!(tn(Locale::En, "gone {n}", 3, &[]), "gone 3");
        // The two raw halves of a counted message are not keys of their own.
        assert_eq!(
            t(Locale::En, "sidebar.activityWorking_one"),
            "sidebar.activityWorking_one"
        );
    }

    #[test]
    fn placeholders_are_filled_and_text_is_left_alone() {
        let text = t_with(
            Locale::Fr,
            Key::PaletteEmpty,
            &args!("query" => "fix : tests ?"),
        );
        assert_eq!(
            text,
            "Rien ne correspond à «\u{202f}fix : tests ?\u{202f}»."
        );
        assert_eq!(
            t_with(
                Locale::En,
                Key::PaletteEmpty,
                &args!("query" => "{n} {query}")
            ),
            "Nothing matches “{n} {query}”."
        );
    }

    #[test]
    fn a_placeholder_without_an_argument_stays_as_written() {
        assert_eq!(
            t(Locale::En, Key::PaletteEmpty),
            "Nothing matches “{query}”."
        );
        assert_eq!(
            t_with(Locale::En, Key::PaletteEmpty, &args!("other" => 1)),
            "Nothing matches “{query}”."
        );
    }

    #[test]
    fn numbers_are_written_the_way_the_language_writes_them() {
        assert_eq!(
            t_with(
                Locale::Fr,
                Key::SettingsScrollbackLines,
                &args!("n" => 10_000)
            ),
            "10\u{202f}000 lignes"
        );
        assert_eq!(
            t_with(
                Locale::En,
                Key::SettingsScrollbackLines,
                &args!("n" => 10_000)
            ),
            "10,000 lines"
        );
        // A number that is already text keeps the format its caller gave it.
        assert_eq!(
            t_with(
                Locale::En,
                Key::SettingsScrollbackLines,
                &args!("n" => "10000")
            ),
            "10000 lines"
        );
    }

    #[test]
    fn a_count_picks_the_form_of_each_language() {
        let key = Key::SidebarActivityWorking;
        assert_eq!(tn(Locale::En, key, 1, &[]), "1 agent working");
        assert_eq!(tn(Locale::En, key, 0, &[]), "0 agents working");
        assert_eq!(tn(Locale::En, key, 2, &[]), "2 agents working");
        assert_eq!(tn(Locale::Fr, key, 0, &[]), "0 agent travaille");
        assert_eq!(tn(Locale::Fr, key, 1, &[]), "1 agent travaille");
        assert_eq!(tn(Locale::Fr, key, 2, &[]), "2 agents travaillent");
        assert_eq!(
            tn(Locale::Fr, key, 1_500, &[]),
            "1\u{202f}500 agents travaillent"
        );
    }

    #[test]
    fn a_number_named_n_is_a_count_in_t_with_too() {
        let key = Key::SidebarActivityWorking;
        assert_eq!(
            t_with(Locale::En, key, &args!("n" => 1)),
            tn(Locale::En, key, 1, &[])
        );
        assert_eq!(
            t_with(Locale::Fr, key, &args!("n" => 0)),
            tn(Locale::Fr, key, 0, &[])
        );
        // Text named `n` is only text: it picks no form.
        assert_eq!(
            t_with(Locale::En, key, &args!("n" => "1")),
            "1 agents working"
        );
    }

    #[test]
    fn a_counted_message_without_a_count_shows_its_general_form() {
        assert_eq!(
            t(Locale::En, Key::SidebarActivityWorking),
            "{n} agents working"
        );
    }

    #[test]
    fn a_count_leaves_a_plain_message_as_it_is() {
        assert_eq!(
            tn(Locale::Fr, Key::SettingsScrollbackLines, 1, &[]),
            "1 lignes"
        );
    }

    #[test]
    fn the_translator_speaks_the_language_it_holds() {
        let en = Translator::new(Locale::En);
        let fr = Translator::from(Locale::Fr);
        assert_eq!(en.locale(), Locale::En);
        assert_eq!(en.t(Key::SidebarSettings), "Settings");
        assert_eq!(fr.t(Key::SidebarSettings), "Réglages");
        assert_eq!(
            fr.tn(Key::SidebarActivityWorking, 0, &[]),
            "0 agent travaille"
        );
        assert_eq!(
            fr.t_with(Key::SidebarVersion, &args!("version" => "1.12.1")),
            "Hangar.AI 1.12.1"
        );
        assert_eq!(Translator::default(), en);
    }

    #[test]
    fn braces_that_are_not_placeholders_are_text() {
        assert_eq!(
            fill(
                "a { b } {} {1x} {x",
                Locale::En,
                None,
                &args!("b" => "!", "1x" => "ok")
            ),
            "a { b } {} ok {x"
        );
        assert_eq!(fill("{{n}}", Locale::En, Some(Number::Int(4)), &[]), "{4}");
        assert_eq!(fill("é{n}é", Locale::En, None, &args!("n" => 4)), "é4é");
    }
}
