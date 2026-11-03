//! The tests of `src/lib/paletteMatch.test.ts`, one for one.

use super::*;

/// The highlighted part of `text`, as the palette would bold it.
fn marked(text: &str, query: &str) -> Option<Vec<String>> {
    let found = match_text(text, query)?;
    Some(
        highlight(text, &found.ranges)
            .into_iter()
            .filter(|part| part.hit)
            .map(|part| part.text.to_string())
            .collect(),
    )
}

fn score(text: &str, query: &str) -> Option<f32> {
    match_text(text, query).map(|found| found.score)
}

struct Entry {
    group: &'static str,
    label: &'static str,
    keywords: Vec<String>,
}

impl Searchable for Entry {
    fn group(&self) -> &str {
        self.group
    }
    fn label(&self) -> &str {
        self.label
    }
    fn keywords(&self) -> &[String] {
        &self.keywords
    }
}

fn entry(group: &'static str, label: &'static str) -> Entry {
    Entry {
        group,
        label,
        keywords: Vec::new(),
    }
}

fn labels(group: &PaletteGroup<'_, Entry>) -> Vec<&'static str> {
    group
        .entries
        .iter()
        .map(|ranked| ranked.entry.label)
        .collect()
}

mod fold {
    use super::*;

    #[test]
    fn drops_accents_and_case() {
        assert_eq!(fold("Mémoire Œuvre ÉCRAN"), "memoire œuvre ecran");
    }
}

mod match_text {
    use super::*;

    #[test]
    fn ignores_accents_and_case_on_both_sides() {
        assert_eq!(score("Mémoire", "memoire"), Some(SCORE_PREFIX));
        assert_eq!(score("Memoire", "MÉMOIRE"), Some(SCORE_PREFIX));
    }

    #[test]
    fn ranks_a_prefix_over_a_word_start_over_a_substring() {
        assert_eq!(
            score("Relancer tous les panneaux", "rel"),
            Some(SCORE_PREFIX)
        );
        assert_eq!(score("Tout relancer", "rel"), Some(SCORE_WORD));
        assert_eq!(score("Panneau précédent", "rec"), Some(SCORE_SUBSTRING));
    }

    #[test]
    fn counts_a_word_start_after_punctuation_not_only_after_a_space() {
        assert_eq!(score("Aller à « Ben »", "ben"), Some(SCORE_WORD));
        assert_eq!(score("src/lib/keys.ts", "keys"), Some(SCORE_WORD));
    }

    #[test]
    fn finds_every_word_of_the_query_in_any_order() {
        assert_eq!(score("Tout relancer", "relancer tout"), Some(SCORE_WORDS));
        assert_eq!(
            score("Diviser le panneau actif", "div ctif"),
            Some(SCORE_TOKENS)
        );
        assert_eq!(score("Tout relancer", "relancer rien"), None);
    }

    #[test]
    fn matches_everything_unmarked_when_nothing_is_typed() {
        assert_eq!(
            match_text("Anything", "   "),
            Some(Match {
                score: 0.0,
                ranges: Vec::new()
            })
        );
    }

    #[test]
    fn returns_none_when_the_text_does_not_hold_the_query() {
        assert_eq!(match_text("Afficher le tableau", "memoire"), None);
    }
}

mod highlight {
    use super::*;

    #[test]
    fn marks_the_letters_as_typed_in_the_original_accents_included() {
        assert_eq!(
            marked("Afficher la mémoire", "memo"),
            Some(vec!["mémo".to_string()])
        );
        assert_eq!(marked("Réorganiser", "reo"), Some(vec!["Réo".to_string()]));
    }

    #[test]
    fn keeps_decomposed_accents_with_their_letter() {
        let decomposed = "Me\u{301}moire";
        assert_eq!(
            marked(decomposed, "mem"),
            Some(vec!["Me\u{301}m".to_string()])
        );
    }

    #[test]
    fn marks_each_word_of_a_multi_word_query_merging_neighbours() {
        assert_eq!(
            marked("Tout relancer", "relancer tout"),
            Some(vec!["Tout".to_string(), "relancer".to_string()])
        );
        assert_eq!(
            highlight("abcdef", &[1..3, 2..4]),
            vec![
                Part {
                    text: "a",
                    hit: false
                },
                Part {
                    text: "bcd",
                    hit: true
                },
                Part {
                    text: "ef",
                    hit: false
                },
            ]
        );
    }

    #[test]
    fn returns_the_text_whole_when_nothing_matched() {
        assert_eq!(
            highlight("Tableau", &[]),
            vec![Part {
                text: "Tableau",
                hit: false
            }]
        );
    }

    #[test]
    fn survives_a_range_that_does_not_belong_to_the_text() {
        // A match computed for another label must never panic the renderer.
        let stale = 1..9;
        let parts = highlight("é", std::slice::from_ref(&stale));
        assert_eq!(parts.iter().map(|part| part.text).collect::<String>(), "é");
    }
}

mod match_entry {
    use super::*;

    #[test]
    fn falls_back_to_the_keywords_below_any_label_match() {
        let keywords = vec!["Codex".to_string(), "attend ta réponse".to_string()];
        assert_eq!(
            super::super::match_entry("Aller à Ben", &keywords, "codex"),
            Some(Match {
                score: SCORE_KEYWORD,
                ranges: Vec::new()
            })
        );
        assert_eq!(
            super::super::match_entry("Aller à Ben", &["Codex".to_string()], "ben")
                .map(|found| found.score),
            Some(SCORE_WORD)
        );
        assert_eq!(super::super::match_entry("Aller à Ben", &[], "codex"), None);
    }
}

mod filter_palette {
    use super::*;

    fn entries() -> Vec<Entry> {
        vec![
            entry("Panneaux", "Aller à Ava"),
            entry("Panneaux", "Tout relancer"),
            entry("Vues", "Afficher le tableau"),
            entry("Workspaces", "Ouvrir storefront"),
            entry("Vues", "Relancer la vue"),
        ]
    }

    #[test]
    fn keeps_the_given_order_and_groups_when_nothing_is_typed() {
        let entries = entries();
        let groups = super::super::filter_palette(&entries, "");
        assert_eq!(
            groups.iter().map(|group| group.group).collect::<Vec<_>>(),
            ["Panneaux", "Vues", "Workspaces"]
        );
        assert_eq!(
            labels(&groups[1]),
            ["Afficher le tableau", "Relancer la vue"]
        );
    }

    #[test]
    fn puts_the_group_holding_the_best_match_first() {
        let entries = entries();
        let groups = super::super::filter_palette(&entries, "relancer");
        assert_eq!(
            groups.iter().map(|group| group.group).collect::<Vec<_>>(),
            ["Vues", "Panneaux"]
        );
        assert_eq!(groups[0].entries[0].entry.label, "Relancer la vue");
    }

    #[test]
    fn sorts_inside_a_group_and_keeps_ties_in_the_given_order() {
        let list = vec![
            entry("G", "Le tableau"),
            entry("G", "Tableau"),
            entry("G", "Autre tableau"),
        ];
        let groups = super::super::filter_palette(&list, "tab");
        assert_eq!(
            labels(&groups[0]),
            ["Tableau", "Le tableau", "Autre tableau"]
        );
    }

    #[test]
    fn drops_groups_left_empty() {
        let entries = entries();
        let groups = super::super::filter_palette(&entries, "storefront");
        assert_eq!(
            groups.iter().map(|group| group.group).collect::<Vec<_>>(),
            ["Workspaces"]
        );
        assert!(super::super::filter_palette(&entries, "zzz").is_empty());
    }

    #[test]
    fn remembers_where_each_entry_was_in_the_list_it_came_from() {
        let entries = entries();
        let groups = super::super::filter_palette(&entries, "relancer");
        let indexes: Vec<usize> = groups
            .iter()
            .flat_map(|group| group.entries.iter().map(|ranked| ranked.index))
            .collect();
        assert_eq!(indexes, [4, 1]);
    }
}

mod synonyms {
    use super::*;

    // A command as the palette lists it in French: its label, then its hidden
    // keywords: the English label, the synonyms, the group.
    fn settings() -> Entry {
        let mut keywords = vec!["Open or close the settings".to_string()];
        keywords.extend(
            command_synonyms("view.settings")
                .iter()
                .map(|word| word.to_string()),
        );
        keywords.push("Affichage".to_string());
        Entry {
            group: "Affichage",
            label: "Ouvrir ou fermer les réglages",
            keywords,
        }
    }

    #[test]
    fn finds_a_command_by_a_word_neither_of_its_labels_uses() {
        let list = vec![settings()];
        assert!(!filter_palette(&list, "parametres").is_empty());
        assert!(!filter_palette(&list, "préférences").is_empty());
    }

    #[test]
    fn finds_a_command_by_its_label_in_the_other_language() {
        let list = vec![settings()];
        assert!(!filter_palette(&list, "settings").is_empty());
    }

    #[test]
    fn still_ranks_a_label_match_above_a_synonym() {
        let list = vec![settings(), entry("Affichage", "Options de la police")];
        let groups = filter_palette(&list, "options");
        assert_eq!(
            labels(&groups[0]),
            ["Options de la police", "Ouvrir ou fermer les réglages"]
        );
        assert_eq!(groups[0].entries[1].matched.score, SCORE_KEYWORD);
    }

    #[test]
    fn has_none_for_a_command_nobody_calls_anything_else() {
        assert!(command_synonyms("pane.next").is_empty());
    }
}

mod no_break_spaces {
    use super::*;

    #[test]
    fn match_the_plain_spaces_that_are_typed() {
        assert_eq!(
            score("Remise à zéro à 20\u{a0}h\u{a0}01", "20 h 01"),
            Some(SCORE_WORD)
        );
        assert_eq!(
            marked("Aller à «\u{202f}Ben\u{202f}»", "a « ben"),
            Some(vec!["à «\u{202f}Ben".to_string()])
        );
    }
}
