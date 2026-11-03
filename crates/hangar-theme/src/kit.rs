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

    /// The colour blocks of `kit.css` are written from these themes by
    /// `scripts/gen-hangar-ui.mjs`. A theme changed here without running it
    /// again would leave a kit's page in the old colours until the window's
    /// theme event arrives, and in them for good outside Hangar.
    #[test]
    fn the_kits_stylesheet_has_the_colours_of_its_two_themes() {
        const SHEET: &str = include_str!("../../hangar-core/kit-sdk/kit.css");
        let block = |opening: &str| -> Vec<(String, String)> {
            let start = SHEET.find(opening).expect(opening) + opening.len();
            let end = start + SHEET[start..].find('}').expect("a closed block");
            let mut tokens: Vec<(String, String)> = SHEET[start..end]
                .lines()
                .filter_map(|line| {
                    let (name, value) = line.trim().strip_suffix(';')?.split_once(": ")?;
                    // The two shadows keep the web app's spelling there,
                    // without spaces (see TYPED_TOGETHER in the script).
                    let value = value.replace(',', ", ").replace(",  ", ", ");
                    name.starts_with("--kit-").then(|| (name.to_owned(), value))
                })
                .collect();
            tokens.sort();
            tokens
        };
        let theme = |id: &str| {
            let mut tokens = kit_tokens(by_id(id).unwrap());
            tokens.sort();
            tokens
        };
        let (dark, light) = kit_default_themes();
        let stale = "kit.css is stale: run node scripts/gen-hangar-ui.mjs";
        assert_eq!(
            block(":root[data-kit-scheme=\"light\"] {"),
            theme(light),
            "{stale}"
        );
        assert_eq!(
            block(":root[data-kit-scheme=\"dark\"] {"),
            theme(dark),
            "{stale}"
        );
        assert_eq!(
            block(":root:not([data-kit-scheme]) {"),
            theme(dark),
            "{stale}"
        );
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

    /// Every pair `kit.css` writes one colour on another, held on every
    /// bundled theme (not only the two the sheet holds, since the window sends
    /// the theme of the tab's workspace) to the window's own rule: words at
    /// [`TEXT_CONTRAST`], a state's shape and the focus ring at
    /// [`FILL_CONTRAST`], tints included. These are the pairs the web app's
    /// `src/lib/hangarUi.test.ts` (tag v1.12.1) checked; a component that
    /// writes a new pair adds it here.
    mod contrast {
        use std::collections::HashMap;

        use super::*;
        use crate::{FILL_CONTRAST, Rgb, TEXT_CONTRAST, contrast_ratio, mix, over};

        /// A theme's `--kit-*` colours, each laid over the page as it reads.
        fn tokens(theme: &Theme) -> HashMap<String, Rgb> {
            let map: serde_json::Value = serde_json::from_str(TOKEN_MAP).unwrap();
            let page = Rgb::from(theme.colors.chrome);
            map["colors"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(token, source)| {
                    let color = theme.colors.get(source.as_str().unwrap()).unwrap();
                    (token.clone(), over(color, page))
                })
                .collect()
        }

        /// What is written on what, as (what, colour, background).
        fn pairs(theme: &Theme) -> (Vec<(String, Rgb, Rgb)>, Vec<(String, Rgb, Rgb)>) {
            let t = tokens(theme);
            let k = |name: &str| t[name];
            let flat = |name: &str, on: Rgb| over(theme.colors.get(&kit_source(name)).unwrap(), on);
            let bg = k("--kit-bg");
            let well = k("--kit-well");
            let plain = [
                "--kit-bg",
                "--kit-surface",
                "--kit-raised",
                "--kit-float",
                "--kit-field",
                "--kit-well",
            ];
            let under_pointer = ["--kit-hover", "--kit-selected"];
            let mut text = Vec::new();
            let mut on = |token: &str, background: Rgb, where_: &str| {
                text.push((format!("{token} on {where_}"), k(token), background));
            };
            for background in plain.iter().chain(&under_pointer) {
                for token in [
                    "--kit-text",
                    "--kit-text-dim",
                    "--kit-accent-text",
                    "--kit-ok",
                    "--kit-warn",
                    "--kit-danger",
                    "--kit-neutral",
                ] {
                    on(token, k(background), background);
                }
            }
            // The quietest greys answer to the plain surfaces, as they do in Hangar.
            for background in plain {
                for token in ["--kit-text-muted", "--kit-text-faint"] {
                    on(token, k(background), background);
                }
            }
            on("--kit-on-accent", k("--kit-accent"), "the primary button");
            let chip = flat("--kit-line", bg);
            on("--kit-text-dim", chip, "a chip or a filter");
            on("--kit-danger", chip, "a filter's count");
            on("--kit-text", chip, "a filter under the pointer");
            on(
                "--kit-text",
                flat("--kit-accent-soft", bg),
                "a filter left on",
            );
            on(
                "--kit-text-dim",
                flat("--kit-line", well),
                "inline code in a block",
            );
            on("--kit-text-dim", k("--kit-seg-track"), "a tab");
            on("--kit-text", k("--kit-seg-thumb"), "the chosen tab");
            on(
                "--kit-text",
                mix(k("--kit-raised"), k("--kit-danger"), 0.08),
                "a danger button under the pointer",
            );
            let error_banner = mix(bg, k("--kit-danger"), 0.08);
            let warning_banner = mix(bg, k("--kit-warn-fill"), 0.08);
            on("--kit-text", error_banner, "an error banner");
            on("--kit-danger", error_banner, "an error banner's glyph");
            on("--kit-text", warning_banner, "a warning banner");
            on("--kit-warn", warning_banner, "a warning banner's glyph");
            let error_line = mix(well, k("--kit-danger"), 0.11);
            let warning_line = mix(well, k("--kit-warn-fill"), 0.1);
            on("--kit-text", error_line, "an error line");
            on("--kit-text-dim", error_line, "an error line's number");
            on("--kit-text", warning_line, "a warning line");
            on("--kit-text-dim", warning_line, "a warning line's number");

            let mut fill = Vec::new();
            for background in plain.iter().chain(&under_pointer) {
                for token in [
                    "--kit-ok-fill",
                    "--kit-warn-fill",
                    "--kit-neutral-fill",
                    "--kit-focus",
                    "--kit-danger",
                ] {
                    fill.push((format!("{token} on {background}"), k(token), k(background)));
                }
            }
            (text, fill)
        }

        /// The chrome token a `--kit-*` one is, by the map.
        fn kit_source(token: &str) -> String {
            let map: serde_json::Value = serde_json::from_str(TOKEN_MAP).unwrap();
            map["colors"][token].as_str().unwrap().to_owned()
        }

        fn failing(pairs: Vec<(String, Rgb, Rgb)>, min: f64) -> Vec<String> {
            pairs
                .into_iter()
                .filter_map(|(what, color, background)| {
                    let ratio = contrast_ratio(color, background);
                    (ratio < min - 0.005).then(|| format!("{what}: {ratio:.2}"))
                })
                .collect()
        }

        #[test]
        fn keeps_words_at_4_5_to_1_on_every_theme() {
            for theme in themes() {
                let (text, _) = pairs(theme);
                assert_eq!(
                    failing(text, TEXT_CONTRAST),
                    Vec::<String>::new(),
                    "{}",
                    theme.id
                );
            }
        }

        #[test]
        fn keeps_a_states_shape_and_the_focus_ring_at_3_to_1_on_every_theme() {
            for theme in themes() {
                let (_, fill) = pairs(theme);
                assert_eq!(
                    failing(fill, FILL_CONTRAST),
                    Vec::<String>::new(),
                    "{}",
                    theme.id
                );
            }
        }
    }
}
