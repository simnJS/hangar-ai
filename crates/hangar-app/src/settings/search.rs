//! The search box of the settings: which rows a query keeps, which category
//! shows, and where the arrow keys of the category list go
//! (`SettingsPage.tsx`).
//!
//! Search ignores case and accents: "themes" finds "Thème". It matches the
//! label, the hint, the extra keywords of the row, the title of its section
//! and the name of its category, so typing a category's name lists all of it.

use hangar_ui::matching::fold;
use hangar_ui::menu_nav::{Step, step_index};

use super::schema::{Category, Item, Section};
use crate::shell::SettingsCategory;

/// What the query is compared with, folded.
fn haystack(item: &Item, section: &Section, category: &Category) -> String {
    fold(
        &[
            item.label.as_deref().unwrap_or(""),
            item.hint.as_deref().unwrap_or(""),
            &item.keywords,
            &section.title,
            &category.label,
        ]
        .join(" "),
    )
}

/// Keeps only the rows whose text matches, then drops what is left empty. An
/// empty query keeps everything.
pub fn filter(categories: Vec<Category>, query: &str) -> Vec<Category> {
    let needle = fold(query.trim());
    if needle.is_empty() {
        return categories;
    }
    categories
        .into_iter()
        .filter_map(|mut category| {
            let sections = std::mem::take(&mut category.sections);
            category.sections = sections
                .into_iter()
                .filter_map(|mut section| {
                    let items = std::mem::take(&mut section.items);
                    section.items = items
                        .into_iter()
                        .filter(|item| haystack(item, &section, &category).contains(&needle))
                        .collect();
                    (!section.items.is_empty()).then_some(section)
                })
                .collect();
            (!category.sections.is_empty()).then_some(category)
        })
        .collect()
}

/// The category the page shows: the wanted one if the search left it, else
/// the first that is left. `None` when nothing matched.
pub fn active(visible: &[Category], wanted: SettingsCategory) -> Option<&Category> {
    visible
        .iter()
        .find(|category| category.id == wanted)
        .or_else(|| visible.first())
}

/// Where Up (`Step::Previous`) and Down go in the list of categories: the
/// neighbour of the active one, and nowhere past either end.
pub fn step_category(
    visible: &[Category],
    active: Option<SettingsCategory>,
    step: Step,
) -> Option<SettingsCategory> {
    let current = active.and_then(|id| visible.iter().position(|category| category.id == id));
    step_index(current, visible.len(), step, false).map(|at| visible[at].id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::change::Target;
    use crate::settings::schema::Kind;
    use crate::settings::schema::{Block, Choice};
    use hangar_ui::Icon;

    fn row(id: &str, label: &str, hint: Option<&str>, keywords: &str) -> Item {
        Item {
            id: id.into(),
            label: Some(label.into()),
            hint: hint.map(str::to_string),
            keywords: keywords.into(),
            kind: Kind::Toggle {
                value: false,
                target: Target::AutoResume,
            },
        }
    }

    fn category(id: SettingsCategory, label: &str, sections: Vec<Section>) -> Category {
        Category {
            id,
            label: label.into(),
            icon: Icon::Gear,
            sections,
        }
    }

    fn section(id: &'static str, title: &str, items: Vec<Item>) -> Section {
        Section {
            id,
            title: title.into(),
            hint: None,
            items,
        }
    }

    fn sample() -> Vec<Category> {
        vec![
            category(
                SettingsCategory::General,
                "General",
                vec![
                    section(
                        "language",
                        "Language",
                        vec![row("locale", "Language", None, "langue locale")],
                    ),
                    section(
                        "sessions",
                        "Sessions",
                        vec![
                            row("resume", "Resume sessions", Some("On launch"), ""),
                            row("bridge", "Claude bridge", None, "plugin"),
                        ],
                    ),
                ],
            ),
            category(
                SettingsCategory::Appearance,
                "Apparence",
                vec![section(
                    "theme",
                    "Thème",
                    vec![
                        row("scope", "Per workspace", None, ""),
                        Item {
                            id: "themePick".into(),
                            label: None,
                            hint: None,
                            keywords: "theme Nord Dracula".into(),
                            kind: Kind::Custom {
                                bare: false,
                                block: Block::Themes {
                                    active: "nord".into(),
                                },
                            },
                        },
                    ],
                )],
            ),
            category(
                SettingsCategory::Shell,
                "Shell",
                vec![section(
                    "shell",
                    "Shell",
                    vec![Item {
                        id: "shellDefault".into(),
                        label: Some("Default shell".into()),
                        hint: None,
                        keywords: String::new(),
                        kind: Kind::Select {
                            value: String::new(),
                            choices: vec![Choice {
                                value: String::new(),
                                label: "Auto".into(),
                            }],
                            target: Target::ShellDefault,
                        },
                    }],
                )],
            ),
        ]
    }

    fn ids(categories: &[Category]) -> Vec<&str> {
        categories
            .iter()
            .flat_map(|c| &c.sections)
            .flat_map(|s| &s.items)
            .map(|item| item.id.as_str())
            .collect()
    }

    #[test]
    fn an_empty_query_keeps_everything() {
        assert_eq!(filter(sample(), ""), sample());
        assert_eq!(filter(sample(), "   "), sample());
    }

    #[test]
    fn the_label_the_hint_and_the_keywords_are_matched() {
        assert_eq!(ids(&filter(sample(), "resume")), ["resume"]);
        assert_eq!(ids(&filter(sample(), "on launch")), ["resume"]);
        assert_eq!(ids(&filter(sample(), "plugin")), ["bridge"]);
        assert_eq!(ids(&filter(sample(), "dracula")), ["themePick"]);
    }

    #[test]
    fn case_and_accents_are_ignored_on_both_sides() {
        // "themes" finds "Thème", and "THEME" finds the same.
        assert_eq!(ids(&filter(sample(), "THEME")), ["scope", "themePick"]);
        assert_eq!(ids(&filter(sample(), "thème")), ["scope", "themePick"]);
        assert_eq!(ids(&filter(sample(), "  Théme ")), ["scope", "themePick"]);
    }

    #[test]
    fn the_section_title_and_the_category_name_match_every_row_under_them() {
        assert_eq!(ids(&filter(sample(), "sessions")), ["resume", "bridge"]);
        assert_eq!(ids(&filter(sample(), "apparence")), ["scope", "themePick"]);
    }

    #[test]
    fn emptied_sections_and_categories_are_dropped() {
        let kept = filter(sample(), "bridge");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, SettingsCategory::General);
        assert_eq!(kept[0].sections.len(), 1);
        assert_eq!(kept[0].sections[0].id, "sessions");
        assert!(filter(sample(), "no such setting").is_empty());
    }

    #[test]
    fn a_row_is_counted_once_in_its_category() {
        let kept = filter(sample(), "sessions");
        assert_eq!(kept[0].item_count(), 2);
    }

    #[test]
    fn the_wanted_category_shows_if_the_search_left_it() {
        let kept = filter(sample(), "theme");
        assert_eq!(
            active(&kept, SettingsCategory::Appearance).map(|c| c.id),
            Some(SettingsCategory::Appearance)
        );
    }

    #[test]
    fn otherwise_the_first_category_left_shows() {
        let kept = filter(sample(), "theme");
        assert_eq!(
            active(&kept, SettingsCategory::General).map(|c| c.id),
            Some(SettingsCategory::Appearance)
        );
        assert!(active(&filter(sample(), "zzz"), SettingsCategory::General).is_none());
    }

    #[test]
    fn the_arrows_walk_the_categories_and_stop_at_the_ends() {
        let all = sample();
        let step = |from, step| step_category(&all, from, step);
        assert_eq!(
            step(Some(SettingsCategory::General), Step::Next),
            Some(SettingsCategory::Appearance)
        );
        assert_eq!(
            step(Some(SettingsCategory::Appearance), Step::Previous),
            Some(SettingsCategory::General)
        );
        assert_eq!(step(Some(SettingsCategory::General), Step::Previous), None);
        assert_eq!(step(Some(SettingsCategory::Shell), Step::Next), None);
        // With no active category, Down enters at the top and Up at the bottom.
        assert_eq!(step(None, Step::Next), Some(SettingsCategory::General));
        assert_eq!(step(None, Step::Previous), Some(SettingsCategory::Shell));
        assert_eq!(step_category(&[], None, Step::Next), None);
    }
}
