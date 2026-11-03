//! Surfaces: what the window's areas, cards and floating panels are filled
//! with. Use as `container(..).style(style::container::sidebar(t))`.
//!
//! Every function sets the text colour too, so text inside draws in `fg`
//! without naming it.

use iced::widget::container::Style;
use iced::{Background, Border, Color, Shadow};

use super::{drop_shadow, hairline, rounded};
use crate::Theme;
use crate::tokens::radius;

fn fill(color: Color, text: Color) -> Style {
    Style {
        background: Some(Background::Color(color)),
        text_color: Some(text),
        border: Border::default(),
        shadow: Shadow::default(),
        ..Style::default()
    }
}

/// Nothing but the theme's text colour: for a container that only lays
/// things out over an area that already has its own surface.
pub fn plain(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        text_color: Some(t.colors.fg),
        ..Style::default()
    }
}

/// The page under everything (`body`): the terminal's own background.
pub fn window(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| fill(t.colors.bg, t.colors.fg)
}

/// The navigator (`.sidebar`). Its right edge is a [`super::rule::sep`].
pub fn sidebar(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| fill(t.colors.chrome, t.colors.fg)
}

/// The inspector and the other side panels: the same chrome as the navigator.
pub fn panel(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    sidebar(t)
}

/// The bar over the workspace (`.toolbar`), a step above the navigator.
pub fn toolbar(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| fill(t.colors.chrome_bar, t.colors.fg)
}

/// The bar along the bottom (`.status`).
pub fn statusbar(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| fill(t.colors.chrome, t.colors.fg)
}

/// A pane's header (`.pane__bar`), a little lighter while the pane has the
/// focus.
pub fn pane_head(t: &Theme, focused: bool) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| {
        let color = if focused {
            t.colors.pane_head_focus
        } else {
            t.colors.pane_head
        };
        fill(color, t.colors.fg)
    }
}

/// A card or a group of settings: lifted off the page on a dark theme, the
/// page's own colour on a light one, with a hairline around it.
pub fn card(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line, radius::LG),
        ..fill(t.colors.card, t.colors.fg)
    }
}

/// A well inside a dialog (`--group-bg`): lists and grouped boxes sitting
/// back in the panel.
pub fn well(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line, radius::MD),
        ..fill(t.colors.bg, t.colors.fg)
    }
}

/// A chip or a capsule: a hairline's tint over whatever it sits on.
pub fn chip(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line, radius::SM),
        ..fill(t.colors.line, t.colors.fg_secondary)
    }
}

/// The track of a segmented control in the toolbar (`.segmented`); its chosen
/// segment is a [`super::button::tab`].
pub fn segmented_track(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(t.colors.line)),
        border: rounded(7.0),
        ..Style::default()
    }
}

/// A context menu or any popover (`.ctx-menu`, `.pane-menu`): the toolbar's
/// colour, an edge, and a shadow that lifts it off the window.
pub fn popover(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::LG),
        shadow: drop_shadow(t.colors.shadow, 12.0, 32.0),
        ..fill(t.colors.chrome_bar, t.colors.fg)
    }
}

/// A dialog (`.modal`).
pub fn modal(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::XL),
        shadow: drop_shadow(t.colors.shadow, 24.0, 64.0),
        ..fill(t.colors.chrome, t.colors.fg)
    }
}

/// The command palette's panel (`.palette__panel`). The web app blurred what
/// is behind it through 90 % of the toolbar's colour; this is opaque.
pub fn palette(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::XL),
        shadow: drop_shadow(t.colors.shadow, 30.0, 80.0),
        ..fill(t.colors.chrome_bar, t.colors.fg)
    }
}

/// A tooltip: a small popover.
pub fn tooltip(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::SM),
        shadow: drop_shadow(t.colors.shadow, 4.0, 12.0),
        ..fill(t.colors.chrome_bar, t.colors.fg)
    }
}

/// The veil behind a dialog (`.modal-backdrop`), lighter on a light theme
/// since the same veil reads twice as heavy over white.
pub fn scrim(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(Color::from_rgba(
            0.0,
            0.0,
            0.0,
            if t.is_dark() { 0.36 } else { 0.16 },
        ))),
        ..Style::default()
    }
}

/// The veil behind the command palette (`.palette`): the separator colour at
/// 42 %, so the window stays readable and it is clear nothing has moved yet.
pub fn palette_scrim(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(t.colors.sep.scale_alpha(0.42))),
        ..Style::default()
    }
}

/// A badge that warns (`--danger-soft` behind `--danger`).
pub fn badge_danger(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(t.colors.danger_soft)),
        text_color: Some(t.colors.danger),
        border: rounded(radius::XS),
        ..Style::default()
    }
}

/// A tint of the accent: a selection, a drop target (`--accent-soft`).
pub fn accent_soft(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(t.colors.accent_soft)),
        text_color: Some(t.colors.fg),
        border: rounded(radius::SM),
        ..Style::default()
    }
}
