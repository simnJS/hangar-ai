//! The tokens kits are drawn with: what the window's theme event
//! (`docs/kit-ui.md`, "The theme") sends a kit's page.
//!
//! The map from a `--kit-*` token to the chrome token it comes from is
//! `crates/hangar-core/kit-sdk/tokens.json`, included here and not copied, so
//! a token added there reaches the kits by itself, as it did in the web app.

use iced::Color;

use crate::Theme;

/// `tokens.json`, the map the kits' stylesheet is generated from.
const TOKEN_MAP: &str = include_str!("../../hangar-core/kit-sdk/tokens.json");

/// A colour as CSS writes it: `#rrggbb`, or `rgba(r, g, b, a)` when it is
/// translucent.
pub fn css(color: Color) -> String {
    let [r, g, b, _] = color.into_rgba8();
    if color.a >= 1.0 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r}, {g}, {b}, {})", color.a)
    }
}

/// The `--kit-*` colour tokens of a theme as the names and CSS values the
/// kits' theme event carries, one per line of `colors` in `tokens.json`.
///
/// The window adds `--kit-scale` and `--kit-mono` itself, since they depend
/// on settings and not on the theme.
///
/// # Panics
///
/// When `tokens.json` names a chrome token this crate does not have: the file
/// ships with the crate's sources, and a test walks it, so this is a build
/// that never got that far.
pub fn kit_tokens(theme: &Theme) -> Vec<(String, String)> {
    let map: serde_json::Value =
        serde_json::from_str(TOKEN_MAP).expect("tokens.json is not valid JSON");
    let colors = map["colors"]
        .as_object()
        .expect("tokens.json has no `colors`");
    colors
        .iter()
        .map(|(kit_name, source)| {
            let source = source.as_str().expect("a token maps to a string");
            let color = theme
                .colors
                .get(source)
                .unwrap_or_else(|| panic!("tokens.json maps {kit_name} to unknown {source}"));
            (kit_name.clone(), css(color))
        })
        .collect()
}

/// The theme `tokens.json` names for kits opened outside Hangar, as
/// `(dark, light)`. Graphite and GitHub Light; the kits' stylesheet holds the
/// values of both.
pub fn kit_default_themes() -> (&'static str, &'static str) {
    let map: serde_json::Value =
        serde_json::from_str(TOKEN_MAP).expect("tokens.json is not valid JSON");
    let id = |scheme: &str| -> &'static str {
        let id = map["themes"][scheme]
            .as_str()
            .expect("tokens.json names a theme");
        crate::by_id(id)
            .expect("tokens.json names a bundled theme")
            .id
    };
    (id("dark"), id("light"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{by_id, themes};

    #[test]
    fn every_token_in_the_map_resolves_on_every_theme() {
        for theme in themes() {
            let tokens = kit_tokens(theme);
            assert!(tokens.len() >= 30, "{}", theme.id);
            assert!(tokens.iter().all(|(name, _)| name.starts_with("--kit-")));
        }
    }

    #[test]
    fn graphite_gives_the_values_the_kits_stylesheet_has() {
        let tokens = kit_tokens(by_id("graphite").unwrap());
        let get = |name: &str| {
            tokens
                .iter()
                .find(|(token, _)| token == name)
                .map(|(_, value)| value.as_str())
        };
        // --chrome, --fg and --accent of Graphite.
        assert_eq!(get("--kit-bg"), Some("#1b1c1f"));
        assert_eq!(get("--kit-text"), Some("#c8c9ce"));
        assert_eq!(get("--kit-accent"), Some("#3e8bff"));
        assert_eq!(get("--kit-line"), Some("rgba(255, 255, 255, 0.07)"));
    }

    #[test]
    fn the_default_themes_are_the_ones_the_map_names() {
        assert_eq!(kit_default_themes(), ("graphite", "github-light"));
    }

    #[test]
    fn css_writes_opaque_colours_in_hex_and_translucent_ones_as_rgba() {
        assert_eq!(css(Color::from_rgb8(0x12, 0xab, 0xef)), "#12abef");
        assert_eq!(css(Color::from_rgba8(1, 2, 3, 0.5)), "rgba(1, 2, 3, 0.5)");
    }
}
