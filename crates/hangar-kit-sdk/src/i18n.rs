//! A kit's strings in English and French: Hangar UI's `hangar.ui.i18n`
//! (docs/kit-ui.md, "Language"), English filling in what a language lacks,
//! `key_one` and `key_other` picked by a count, and the template given
//! Hangar's typography: a no-break space between a number and its unit, and
//! before French's high punctuation. What fills a template is shown as
//! written.

use std::collections::BTreeMap;

const NBSP: char = '\u{a0}';
const NARROW_NBSP: char = '\u{202f}';
const UNITS: [&str; 7] = ["min", "ms", "s", "h", "j", "d", "sem"];

#[derive(Debug, Clone, Default)]
pub struct Strings {
    dictionaries: BTreeMap<String, BTreeMap<String, String>>,
    locale: String,
}

impl Strings {
    pub fn new(locale: impl Into<String>) -> Self {
        Strings {
            dictionaries: BTreeMap::new(),
            locale: locale.into(),
        }
    }

    /// A language's strings: `.add("fr", &[("jobs_one", "{n} job")])`.
    pub fn add(mut self, lang: &str, entries: &[(&str, &str)]) -> Self {
        let dictionary = self.dictionaries.entry(lang.to_owned()).or_default();
        for (key, template) in entries {
            dictionary.insert((*key).to_owned(), (*template).to_owned());
        }
        self
    }

    /// The language to speak, as the `theme` event gives it.
    pub fn set_locale(&mut self, locale: impl Into<String>) {
        self.locale = locale.into();
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    fn lang(&self) -> &str {
        self.locale.split(['-', '_']).next().unwrap_or("en")
    }

    /// The string for `key`, the key itself when no language has it.
    pub fn t(&self, key: &str) -> String {
        self.tv(key, &[])
    }

    /// `key` with `{name}` filled in from `vars`.
    pub fn tv(&self, key: &str, vars: &[(&str, &str)]) -> String {
        self.render(key, None, vars)
    }

    /// `key_one` or `key_other` by `n`, `{n}` filled in.
    pub fn tn(&self, key: &str, n: i64, vars: &[(&str, &str)]) -> String {
        self.render(key, Some(n), vars)
    }

    fn render(&self, key: &str, n: Option<i64>, vars: &[(&str, &str)]) -> String {
        let lang = self.lang();
        let plural = n.map(|n| if plural_one(lang, n) { "one" } else { "other" });
        let find = |dictionary: Option<&BTreeMap<String, String>>| -> Option<String> {
            let dictionary = dictionary?;
            if let Some(form) = plural
                && let Some(found) = dictionary
                    .get(&format!("{key}_{form}"))
                    .or_else(|| dictionary.get(&format!("{key}_other")))
            {
                return Some(found.clone());
            }
            dictionary.get(key).cloned()
        };
        let Some(template) =
            find(self.dictionaries.get(lang)).or_else(|| find(self.dictionaries.get("en")))
        else {
            return key.to_owned();
        };
        let n_text = n.map(|n| n.to_string());
        fill(&typeset(&template, lang == "fr"), |name| {
            if name == "n"
                && let Some(n) = &n_text
            {
                return Some(n.clone());
            }
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }
}

/// The plural rules of `Intl.PluralRules` for the two languages: French
/// counts 0 as one.
fn plural_one(lang: &str, n: i64) -> bool {
    match lang {
        "fr" => n == 0 || n == 1,
        _ => n == 1,
    }
}

fn fill(template: &str, value: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close)
                if after[..close]
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_')
                    && close > 0 =>
            {
                let name = &after[..close];
                match value(name) {
                    Some(filled) => out.push_str(&filled),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_space(c: char) -> bool {
    c == ' ' || c == NBSP || c == NARROW_NBSP
}

/// The rules of src/lib/typography.ts, which kit-ui.js holds too.
fn typeset(text: &str, french: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &c) in chars.iter().enumerate() {
        if is_space(c) {
            let before = index.checked_sub(1).map(|i| chars[i]);
            let after: String = chars[index + 1..]
                .iter()
                .take_while(|c| c.is_alphabetic())
                .collect();
            let next = chars.get(index + 1).copied();
            let unit_after_number = before.is_some_and(|b| b.is_ascii_digit() || b == '}')
                && UNITS.contains(&after.as_str());
            let number_after_unit = next.is_some_and(|n| n.is_ascii_digit() || n == '{') && {
                let word: String = chars[..index]
                    .iter()
                    .rev()
                    .take_while(|c| c.is_alphabetic())
                    .collect();
                let word: String = word.chars().rev().collect();
                let start = index - word.chars().count();
                UNITS.contains(&word.as_str()) && start > 0 && chars[start - 1] == NBSP
            };
            let after_sign = before.is_some_and(|b| "<>≤≥~".contains(b))
                && next.is_some_and(|n| n.is_ascii_digit() || n == '{');
            if french && (next.is_some_and(|n| "?!;:»%".contains(n)) || before == Some('«')) {
                out.push(NARROW_NBSP);
            } else if unit_after_number || number_after_unit || after_sign {
                out.push(NBSP);
            } else {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals_languages_and_typography() {
        let mut strings = Strings::new("fr")
            .add(
                "en",
                &[
                    ("jobs_one", "{n} job"),
                    ("jobs_other", "{n} jobs"),
                    ("failed", "Failed: {name}"),
                    ("only", "English"),
                ],
            )
            .add(
                "fr",
                &[
                    ("jobs_one", "{n} job"),
                    ("jobs_other", "{n} jobs"),
                    ("failed", "Échoué : {name}"),
                ],
            );
        assert_eq!(strings.tn("jobs", 0, &[]), "0 job");
        assert_eq!(
            strings.tv("failed", &[("name", "rust")]),
            "Échoué\u{202f}: rust"
        );
        assert_eq!(strings.t("only"), "English");
        assert_eq!(strings.t("missing"), "missing");
        strings.set_locale("en");
        assert_eq!(strings.tn("jobs", 0, &[]), "0 jobs");
        assert_eq!(
            Strings::new("en")
                .add("en", &[("took", "took {n} s")])
                .tn("took", 3, &[]),
            "took 3\u{a0}s"
        );
    }
}
