//! The themes of Hangar: the 33 palettes of `src/themes.ts`, the colours of
//! the whole window derived from them, the design tokens of the stylesheets,
//! and how all of it maps onto iced.
//!
//! See docs/native/PLAN.md, "hangar-theme".
//!
//! - [`themes`], [`by_id`], [`DEFAULT_ID`]: the bundled [`Theme`]s. A theme
//!   restyles the whole window, not only the terminals: [`Theme::colors`] has
//!   every token of the chrome (surfaces, hairlines, text levels, the accent,
//!   the agent states, the statistics) and [`Theme::terminal`] the colours a
//!   terminal is drawn with.
//! - [`tokens`]: what is not a colour: radii, spacing, font sizes, weights,
//!   durations, the sizes of the window's parts.
//! - [`to_iced`] and [`style`]: the theme for iced, and the style functions
//!   with which views name a role instead of a colour.
//! - [`fonts`]: Inter, as bytes for iced to load and `Font`s to ask for.
//! - [`kit_tokens`]: what kits are told of the theme.
//! - [`contrast`] and [`derive_colors`]: the colour arithmetic and the rules
//!   that turn a palette into a window, ported from `src/lib/contrast.ts` and
//!   `deriveChromeTokens`.
//!
//! The tables in `data.rs` are generated from the TypeScript by
//! `tools/gen-themes.mjs`, so that the values are the web app's own and not a
//! second copy kept by hand.

pub mod contrast;
mod data;
mod derive;
pub mod fonts;
mod iced_theme;
mod kit;
pub mod style;
mod theme;
pub mod tokens;

pub use contrast::{
    HueAndSaturation, Rgb, contrast_ratio, ensure_contrast, farthest_pole, hue_and_saturation,
    hue_distance, mix, over, perceived_lightness, relative_luminance, with_hue, worst_contrast,
};
pub use derive::{FILL_CONTRAST, TEXT_CONTRAST, derive_colors};
pub use iced_theme::to_iced;
pub use kit::{css, kit_default_themes, kit_tokens};
pub use theme::{
    AgentState, Appearance, Colors, DEFAULT_ID, SearchColors, StateColors, TerminalColors, Theme,
    by_id, by_id_or_default, default_theme, themes,
};
pub(crate) use theme::{rgb, rgba};

#[cfg(test)]
mod tests;
