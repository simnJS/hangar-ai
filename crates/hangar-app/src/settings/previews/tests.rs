//! The pictures' data: which cards there are and how a font stack resolves.

use super::*;
use crate::state::types::Settings;

#[test]
fn the_shipped_stack_is_the_first_font_card_so_a_fresh_install_is_not_custom() {
    assert_eq!(FONT_STACKS[0].1, Settings::default().font_family);
    let cards = font_cards(&Settings::default().font_family, "Custom");
    assert_eq!(cards.len(), FONT_STACKS.len());
    assert_eq!(cards[0].0, "Cascadia Code");
}

#[test]
fn a_stack_that_is_none_of_the_cards_gets_a_custom_one_after_them() {
    let cards = font_cards("Iosevka, monospace", "Custom");
    assert_eq!(cards.len(), FONT_STACKS.len() + 1);
    assert_eq!(
        cards.last(),
        Some(&("Custom".to_string(), "Iosevka, monospace".to_string()))
    );
}

#[test]
fn the_font_cards_are_those_of_the_web_app() {
    assert_eq!(
        FONT_STACKS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        [
            "Cascadia Code",
            "JetBrains Mono",
            "Fira Code",
            "Consolas",
            "Hack",
            "IBM Plex Mono",
            "System"
        ]
    );
}

#[test]
fn there_are_twenty_five_dark_themes_and_eight_light_ones_to_pick_from() {
    let themes = hangar_theme::themes();
    let dark = themes
        .iter()
        .filter(|theme| theme.appearance == Appearance::Dark)
        .count();
    assert_eq!((dark, themes.len() - dark), (25, 8));
}

#[test]
fn a_family_name_is_leaked_once() {
    assert!(std::ptr::eq(intern("Fira Code"), intern("Fira Code")));
    assert_eq!(intern("Hack"), "Hack");
}

#[test]
fn a_stack_of_fonts_nobody_has_falls_back_to_the_monospace_family() {
    let font = font_of("\"Not A Real Font 7\", \"Nor This One\", monospace");
    assert_eq!(font.family, Family::Monospace);
    // And the answer is the same the second time, from the cache.
    assert_eq!(
        font_of("\"Not A Real Font 7\", \"Nor This One\", monospace"),
        font
    );
}

#[test]
fn the_browse_dialog_opens_on_the_folder_already_chosen() {
    let here = std::env::current_dir().expect("a current directory");
    let chosen = here.to_string_lossy().to_string();
    assert_eq!(browse_start(Some(&chosen)), Some(here));
    // A folder that is gone, or nothing typed, opens on the home folder.
    let home = browse_start(None);
    assert_eq!(browse_start(Some("/no/such/folder/anywhere")), home);
    assert_eq!(browse_start(Some("   ")), home);
}
