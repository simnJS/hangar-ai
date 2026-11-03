use std::collections::HashSet;

use super::*;

#[test]
fn the_enum_is_in_the_order_of_the_table_the_handles_are_indexed_by() {
    for (index, icon) in Icon::ALL.iter().enumerate() {
        assert_eq!(*icon as usize, index, "{}", icon.name());
    }
}

#[test]
fn every_icon_of_the_web_app_is_there() {
    assert_eq!(Icon::ALL.len(), 77);
    // A few from each family of `Icon.tsx`, by the names it gives them.
    for name in [
        "layout1",
        "splitRight",
        "sidebarLeft",
        "windowReturn",
        "folderPlus",
        "orchestrator",
        "gear",
        "chevronDown",
        "arrowUpRight",
        "restart",
        "reset",
        "wireframe",
        "copy",
    ] {
        assert!(Icon::from_name(name).is_some(), "{name}");
    }
}

#[test]
fn names_are_unique_and_found_again() {
    let mut seen = HashSet::new();
    for icon in Icon::ALL {
        assert!(seen.insert(icon.name()), "{} twice", icon.name());
        assert_eq!(Icon::from_name(icon.name()), Some(icon));
    }
    assert_eq!(Icon::from_name("nope"), None);
}

#[test]
fn every_drawing_is_a_valid_svg_on_the_16_unit_grid() {
    for icon in Icon::ALL {
        let tree = usvg::Tree::from_str(&icon.svg(), &usvg::Options::default())
            .unwrap_or_else(|error| panic!("{}: {error}", icon.name()));
        let size = tree.size();
        assert_eq!(
            (size.width(), size.height()),
            (16.0, 16.0),
            "{}",
            icon.name()
        );
    }
}

#[test]
fn a_drawing_is_never_empty() {
    for icon in Icon::ALL {
        assert!(!icon.elements().is_empty(), "{}", icon.name());
    }
}

mod turning {
    use std::f32::consts::{FRAC_PI_2, TAU};

    use super::*;

    #[test]
    fn no_turn_and_a_full_turn_are_the_icon_itself() {
        let plain = Icon::ChevronDown.handle();
        assert_eq!(turned_handle(Icon::ChevronDown, Radians(0.0)), plain);
        assert_eq!(turned_handle(Icon::ChevronDown, Radians(TAU)), plain);
    }

    #[test]
    fn a_turn_is_made_once_and_kept() {
        let first = turned_handle(Icon::ChevronDown, Radians(-FRAC_PI_2));
        let again = turned_handle(Icon::ChevronDown, Radians(-FRAC_PI_2));
        assert_eq!(first, again);
        assert_ne!(first, Icon::ChevronDown.handle());
    }

    #[test]
    fn a_quarter_turn_back_is_three_quarters_forward() {
        assert_eq!(
            turned_handle(Icon::Check, Radians(-FRAC_PI_2)),
            turned_handle(Icon::Check, Radians(3.0 * FRAC_PI_2))
        );
    }
}

#[test]
fn the_tint_is_a_colour_the_renderer_replaces_not_one_that_leaks() {
    // `currentColor` has no meaning outside a web page: every drawing must be
    // in a plain colour for the renderer to tint.
    for icon in Icon::ALL {
        assert!(!icon.svg().contains("currentColor"), "{}", icon.name());
    }
}
