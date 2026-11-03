//! The checks `src/i18n.test.ts` made on the dictionaries, on the generated
//! ones: what the TypeScript compiler guaranteed (a French string for every
//! English one) and what its tests held every French string to.

use std::collections::BTreeSet;

use hangar_i18n::{KEY_COUNT, Key, Locale, PluralCategory, args, t, t_with, tn};

const NNBSP: char = '\u{202f}';
const NBSP: char = '\u{a0}';

/// Every string of `locale`, with the key and form it belongs to, for a
/// failure message that says which one.
fn strings(locale: Locale) -> Vec<(String, &'static str)> {
    Key::ALL
        .into_iter()
        .flat_map(|key| {
            [
                (PluralCategory::One, "one"),
                (PluralCategory::Other, "other"),
            ]
            .map(|(form, name)| (format!("{}[{name}]", key.id()), key.template(locale, form)))
        })
        .collect()
}

/// Every French string that breaks `rule`, as "key: text", the way the
/// TypeScript test listed them.
fn offenders(rule: impl Fn(&str) -> bool) -> Vec<String> {
    strings(Locale::Fr)
        .into_iter()
        .filter(|(_, text)| rule(text))
        .map(|(key, text)| format!("{key}: {text:?}"))
        .collect()
}

/// The `{name}`s of a string, in a set: their order may change in translation.
fn placeholders(text: &str) -> BTreeSet<&str> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        if let Some(close) = rest.find('}') {
            let name = &rest[..close];
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                found.insert(name);
            }
        }
    }
    found
}

#[test]
fn the_dictionary_is_not_empty_and_its_ids_are_sorted() {
    // `black_box`: the count is a constant, and asserting on one is a lint.
    let count = std::hint::black_box(KEY_COUNT);
    assert!(
        count > 1_500,
        "the strings of the web app are missing: {count}"
    );
    assert_eq!(Key::ALL.len(), KEY_COUNT);
    let ids: Vec<_> = Key::ALL.iter().map(|key| key.id()).collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "ids must be sorted and unique"
    );
}

#[test]
fn every_key_is_found_again_by_its_id() {
    for key in Key::ALL {
        assert_eq!(Key::from_id(key.id()), Some(key), "{}", key.id());
    }
    assert_eq!(Key::from_id(""), None);
    assert_eq!(Key::from_id("sidebar.workspace"), None);
    assert_eq!(Key::from_id("zzz"), None);
}

#[test]
fn every_english_key_has_a_french_string() {
    // The type checker did this in the web app (`fr: Record<Key, string>`); here
    // the generator refuses a missing key, and this holds the result to it: no
    // French string is empty where the English one is not.
    let empty: Vec<_> = Key::ALL
        .into_iter()
        .flat_map(|key| {
            [PluralCategory::One, PluralCategory::Other]
                .into_iter()
                .filter_map(move |form| {
                    let (en, fr) = (
                        key.template(Locale::En, form),
                        key.template(Locale::Fr, form),
                    );
                    (!en.is_empty() && fr.is_empty()).then(|| key.id())
                })
        })
        .collect();
    assert_eq!(empty, Vec::<&str>::new());
}

#[test]
fn both_languages_fill_the_same_placeholders() {
    let mut mismatched = Vec::new();
    for key in Key::ALL {
        for form in [PluralCategory::One, PluralCategory::Other] {
            let en = key.template(Locale::En, form);
            let fr = key.template(Locale::Fr, form);
            if placeholders(en) != placeholders(fr) {
                mismatched.push(format!("{} {form:?}: {en:?} / {fr:?}", key.id()));
            }
        }
    }
    assert_eq!(mismatched, Vec::<String>::new());
}

#[test]
fn a_counted_message_keeps_its_count_in_both_forms_unless_it_names_it_otherwise() {
    // "{n} agent" and "{n} agents": a form that drops `{n}` ("Un agent") would
    // still be right, but none does today, and one that dropped it by mistake
    // would print no number.
    let mut lost = Vec::new();
    for key in Key::ALL {
        for locale in Locale::ALL {
            let one = placeholders(key.template(locale, PluralCategory::One));
            let other = placeholders(key.template(locale, PluralCategory::Other));
            if one != other && !(one.contains("n") || other.contains("n")) {
                lost.push(key.id());
            }
        }
    }
    assert_eq!(lost, Vec::<&str>::new());
}

mod french_typography {
    use super::*;

    #[test]
    fn has_a_narrow_no_break_space_before_high_punctuation() {
        let bad = offenders(|text| {
            let mut previous = None;
            text.chars().any(|c| {
                let wrong = "?!;:»%".contains(c) && previous.is_some_and(|p| p != NNBSP);
                previous = Some(c);
                wrong
            })
        });
        assert_eq!(bad, Vec::<String>::new());
    }

    #[test]
    fn has_a_narrow_no_break_space_after_an_opening_guillemet() {
        let bad = offenders(|text| {
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '«' && chars.peek().is_some_and(|next| *next != NNBSP) {
                    return true;
                }
            }
            false
        });
        assert_eq!(bad, Vec::<String>::new());
    }

    #[test]
    fn closes_every_guillemet_it_opens() {
        let count = |text: &str, mark: char| text.chars().filter(|c| *c == mark).count();
        assert_eq!(
            offenders(|text| count(text, '«') != count(text, '»')),
            Vec::<String>::new()
        );
    }

    #[test]
    fn quotes_with_guillemets_never_with_english_quotes() {
        assert_eq!(
            offenders(|text| text.contains(['"', '“', '”'])),
            Vec::<String>::new()
        );
    }

    #[test]
    fn writes_an_ellipsis_as_one_character() {
        assert_eq!(offenders(|text| text.contains("...")), Vec::<String>::new());
    }

    #[test]
    fn has_no_plain_space_where_a_no_break_one_belongs() {
        // What the typesetter does to a number and its unit, both languages: the
        // generated strings are already bound, so none keeps the plain space.
        let unit = |text: &str| {
            ["min", "ms", "h", "j", "d", "sem"].iter().any(|unit| {
                text.match_indices(&format!(" {unit}")).any(|(at, _)| {
                    let before = text[..at].chars().next_back();
                    let after = text[at + 1 + unit.len()..].chars().next();
                    before.is_some_and(|c| c.is_ascii_digit() || c == '}')
                        && !after.is_some_and(char::is_alphanumeric)
                })
            })
        };
        assert_eq!(offenders(unit), Vec::<String>::new());
        let english: Vec<_> = strings(Locale::En)
            .into_iter()
            .filter(|(_, text)| unit(text))
            .collect();
        assert_eq!(english, Vec::<(String, &str)>::new());
    }
}

#[test]
fn a_number_and_its_unit_do_not_split() {
    assert_eq!(t(Locale::En, Key::SidebarPlanFiveHour), format!("5{NBSP}h"));
    assert_eq!(t(Locale::Fr, Key::SidebarPlanWeek), format!("7{NBSP}j"));
    assert_eq!(t(Locale::En, Key::SidebarPlanWeek), format!("7{NBSP}d"));
}

#[test]
fn groups_the_digits_of_a_number_the_french_way() {
    let scrollback = |locale| t_with(locale, Key::SettingsScrollbackLines, &args!("n" => 10_000));
    assert_eq!(scrollback(Locale::Fr), format!("10{NNBSP}000 lignes"));
    assert_eq!(scrollback(Locale::En), "10,000 lines");
}

#[test]
fn never_touches_what_fills_a_placeholder() {
    assert_eq!(
        t_with(
            Locale::Fr,
            Key::PaletteEmpty,
            &args!("query" => "fix : tests ?")
        ),
        format!("Rien ne correspond à «{NNBSP}fix : tests ?{NNBSP}».")
    );
}

#[test]
fn counted_messages_follow_each_languages_rule() {
    // The web app's `Intl.PluralRules`: French calls 0 and 1 singular.
    let waits = |locale, n| tn(locale, Key::SidebarActivityWaiting, n, &[]).into_owned();
    assert_eq!(waits(Locale::En, 1), "1 agent waits for your answer");
    assert_eq!(waits(Locale::En, 0), "0 agents wait for your answer");
    assert_eq!(waits(Locale::Fr, 0), "0 agent attend ta réponse");
    assert_eq!(waits(Locale::Fr, 1), "1 agent attend ta réponse");
    assert_eq!(waits(Locale::Fr, 2), "2 agents attendent ta réponse");
}

#[test]
fn computed_keys_still_resolve() {
    // The families `t(`prefix.${x}`)` builds in the web app, with the values
    // their types allow: a typo in the dictionary or a rename breaks one here
    // instead of showing a raw key on screen.
    let families: &[(&str, &[&str])] = &[
        (
            "control.",
            &[
                "created",
                "closed",
                "restarted",
                "reset",
                "workspaceCreated",
                "checkSet",
            ],
        ),
        (
            "control.",
            &[
                "modelShown",
                "modelClosed",
                "modelShownElsewhere",
                "modelClosedElsewhere",
                "someone",
            ],
        ),
        ("manager.", &["created", "restored"]),
        (
            "pane.state.",
            &[
                "exited", "idle", "running", "starting", "waiting", "working", "yours",
            ],
        ),
        (
            "stats.kind.",
            &["cacheRead", "cacheWrite", "input", "output"],
        ),
        ("stats.period.", &["7d", "30d", "90d", "all"]),
        ("stats.periodHint.", &["7d", "30d", "90d", "all"]),
        ("stats.measure.", &["cost", "tokens"]),
        ("stats.measureHint.", &["cost", "tokens"]),
        (
            "keymap.section.",
            &[
                "app",
                "commands",
                "navigation",
                "panes",
                "terminal",
                "view",
                "voice",
                "workspaces",
            ],
        ),
        ("view.", &["board", "group", "memory", "stats", "terminals"]),
        ("cmd.view.", &["board", "memory", "stats", "terminals"]),
        ("diff.status.", &["M", "A", "D", "R", "C", "T", "U", "?"]),
        ("inspector.status.", &["A", "C", "D", "M", "R", "U"]),
        (
            "kitsTab.badge.",
            &["failure", "neutral", "running", "success"],
        ),
        ("mcpScope.scope.", &["local", "project", "user"]),
        ("mcpScope.scopeHint.", &["local", "project", "user"]),
        (
            "orch.permission.",
            &["acceptEdits", "auto", "bypassPermissions"],
        ),
        (
            "preview.kind.",
            &[
                "audio", "html", "image", "markdown", "model", "pdf", "text", "video",
            ],
        ),
        ("experiments.stage.", &["experimental", "beta"]),
    ];
    let mut missing = Vec::new();
    for (prefix, suffixes) in families {
        for suffix in *suffixes {
            let id = format!("{prefix}{suffix}");
            for locale in Locale::ALL {
                let text = t(locale, &id);
                if text == id {
                    missing.push(format!("{locale:?} {id}"));
                }
            }
        }
    }
    assert_eq!(missing, Vec::<String>::new());
}

#[test]
fn the_permission_names_of_a_kit_resolve() {
    for permission in [
        "command",
        "data.read",
        "data.write",
        "files.read",
        "files.write",
        "loopback",
        "notify",
        "panes.read",
        "panes.send",
        "preview.show",
        "workspace.read",
    ] {
        let id = format!("kitsTab.permission.{permission}");
        assert!(Key::from_id(&id).is_some(), "{id}");
    }
}
