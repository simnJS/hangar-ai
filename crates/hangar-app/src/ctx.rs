//! What every view is drawn with: `Ctx`, built once per window per frame by
//! the shell and handed to each package's view functions.
//!
//! It is `Copy` (a bundle of references and small values), so a view passes
//! it on to the embeds it calls without thinking about it.

use std::time::Instant;

use iced::window;

use hangar_i18n::Translator;
use hangar_theme::Theme;

use crate::app::Packages;
use crate::keymap::KeymapIndex;
use crate::state::Model;

#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub model: &'a Model,
    /// Every package's UI state, read-only: how a view embeds a piece of
    /// another package's surface that needs that package's state
    /// (`accounts::settings_page(&ctx.pkg.accounts, ctx)`).
    pub pkg: &'a Packages,
    /// The interface strings in the person's language (`hangar_i18n`).
    pub i18n: Translator,
    /// The theme the window is drawn in: its colours (`theme.colors`), for
    /// the `hangar_theme::style::*` functions. The other tokens (radii,
    /// spacing, sizes) are constants of `hangar_theme::tokens`.
    pub theme: &'static Theme,
    pub metrics: Metrics,
    /// The resolved shortcuts, for the key hints of tooltips and menus. P7
    /// publishes it in `Model.live.keymap.index`.
    pub keymap: &'a KeymapIndex,
    /// The window being drawn.
    pub window: window::Id,
    /// The frame's clock, for relative times and animations.
    pub now: Instant,
}

impl<'a> Ctx<'a> {
    /// A translated label, as a view wants it: `text(ctx.t(Key::SidebarNew))`.
    pub fn t(&self, key: impl hangar_i18n::AsKey) -> std::borrow::Cow<'static, str> {
        self.i18n.t(key)
    }
}

/// Sizes that depend on the settings rather than on the theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// `uiScale` as a factor (0.9–1.3). The shell hands it to iced as the
    /// window's scale factor, so views lay out in unscaled logical pixels and
    /// never multiply by it themselves; it is here for the few computations
    /// in screen pixels (`side_panels` widths are in points × scale).
    pub scale: f32,
    /// The width of the window's main body, measured, in logical pixels.
    pub body_width: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            scale: 1.0,
            body_width: crate::shell::windows::MAIN_SIZE.width,
        }
    }
}
