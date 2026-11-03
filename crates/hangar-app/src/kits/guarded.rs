//! The library's filter and the field for a kit's git address, each given
//! Mod+Shift+Enter: the keymap's shortcut must be spent before the field
//! sees it, so that it runs without also typing or submitting in the field
//! (`keymap::guard`).

use super::*;
use crate::settings::guard_testing::Fixture;

#[test]
fn the_git_address_field_leaves_the_shortcut_to_the_keymap() {
    let fixture = Fixture::new();
    let st = State::default();
    let said = fixture.split_chord(views::settings::view(&st, fixture.ctx()));
    assert!(
        said.iter().all(|word| word == "Noop") && !said.is_empty(),
        "{said:?}"
    );
}

#[test]
fn the_library_filter_leaves_it_too() {
    let fixture = Fixture::new();
    let st = State::default();
    let installed: Vec<hangar_core::kits::KitInfo> = Vec::new();
    let said = fixture.split_chord(views::gallery::view(
        fixture.ctx(),
        &st.page,
        Some(&installed),
    ));
    assert_eq!(said, ["Noop"]);
}
