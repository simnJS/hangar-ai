//! Surfaces: what the window's areas, cards and floating panels are filled
//! with. Use as `container(..).style(style::container::sidebar(t))`.
//!
//! Every function sets the text colour too, so text inside draws in `fg`
//! without naming it.

use iced::widget::container::Style;
use iced::{Background, Border, Color, Shadow};

use super::{Tone, drop_shadow, hairline, rounded, with_alpha};
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

/// The track of a segmented control in a form (`.seg`): recessed, with a
/// hairline around it; its chosen segment is a [`super::button::tab`].
pub fn segmented(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::MD),
        ..fill(t.colors.seg_track, t.colors.fg)
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

/// A key of a keyboard shortcut (`.kbd-seq kbd`): a little cap with a
/// hairline under it, in the mono face.
pub fn kbd(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::XS),
        shadow: drop_shadow(t.colors.line_strong, 1.0, 0.0),
        ..fill(t.colors.surface_active, t.colors.fg)
    }
}

/// A key of a shortcut another command answers to as well.
pub fn kbd_conflict(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.danger, radius::XS),
        shadow: drop_shadow(t.colors.line_strong, 1.0, 0.0),
        ..fill(t.colors.surface_active, t.colors.danger)
    }
}

/// The box a shortcut is recorded in (`.key-rec`): the accent for its edge
/// and its tint behind, the way a field with the keyboard looks.
pub fn recorder(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.accent, radius::MD),
        ..fill(t.colors.accent_soft, t.colors.fg)
    }
}

/// The square an icon sits in at the start of a palette row
/// (`.palette__icon`), turned over to the accent's foreground on the chosen
/// row.
pub fn palette_icon(t: &Theme, chosen: bool) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| {
        let c = &t.colors;
        if chosen {
            Style {
                background: Some(Background::Color(with_alpha(c.accent_fg, 0.18))),
                text_color: Some(c.accent_fg),
                border: rounded(radius::SM),
                ..Style::default()
            }
        } else {
            Style {
                background: Some(Background::Color(c.line)),
                text_color: Some(c.fg_muted),
                border: rounded(radius::SM),
                ..Style::default()
            }
        }
    }
}

/// A hairline box with nothing in it (`.palette__esc`): the cap of the key
/// that closes the palette, on a surface of its own.
pub fn outline(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        border: hairline(t.colors.line_strong, radius::XS),
        text_color: Some(t.colors.fg_secondary),
        ..Style::default()
    }
}

/// A small word set on a surface (`.key-row__tag`): "custom", "new".
pub fn tag(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(t.colors.surface_active)),
        text_color: Some(t.colors.fg_muted),
        border: rounded(radius::XS),
        ..Style::default()
    }
}

/// A count or a status word in a capsule. The tone is what it says: neutral
/// for a number, the accent for what is selected, success, warning and
/// danger for what they name.
pub fn badge(t: &Theme, tone: Tone) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| {
        let (background, text) = tone_colors(t, tone);
        Style {
            background: Some(Background::Color(background)),
            text_color: Some(text),
            border: rounded(radius::XS),
            ..Style::default()
        }
    }
}

/// A notice that floats over the window and goes by itself: a popover with
/// its tone in the edge.
pub fn toast(t: &Theme, tone: Tone) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| {
        let c = &t.colors;
        let edge = match tone {
            Tone::Neutral => c.line_strong,
            Tone::Accent => c.accent,
            Tone::Success => c.success,
            Tone::Warning => c.warning,
            Tone::Danger => c.danger,
        };
        Style {
            border: hairline(edge, radius::LG),
            shadow: drop_shadow(c.shadow, 10.0, 28.0),
            ..fill(c.chrome_bar, c.fg)
        }
    }
}

/// The colour a tone's text and icons are drawn in (the glyph of a toast).
pub fn tone_text(t: &Theme, tone: Tone) -> Color {
    tone_colors(t, tone).1
}

/// A tone's tint and the colour that reads on it.
fn tone_colors(t: &Theme, tone: Tone) -> (Color, Color) {
    let c = &t.colors;
    match tone {
        Tone::Neutral => (c.line, c.fg_secondary),
        Tone::Accent => (c.accent_soft, c.accent_hi),
        Tone::Success => (with_alpha(c.success, 0.16), c.success),
        Tone::Warning => (with_alpha(c.warning, 0.16), c.warning),
        Tone::Danger => (c.danger_soft, c.danger),
    }
}

/// A group of rows with a hairline around it and nothing between them but the
/// rows' own separators (`.set-group`).
pub fn group(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    card(t)
}

/// A row of a form or a list drawn under the pointer without being a button
/// (the shortcut list's rows): a faint wash of the text colour.
pub fn row_hover(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(with_alpha(t.colors.fg, 0.03))),
        border: rounded(radius::NONE),
        ..Style::default()
    }
}

/// A row that something is wrong with: a wash of the danger colour.
pub fn row_conflict(t: &Theme) -> impl Fn(&iced::Theme) -> Style + '_ {
    move |_| Style {
        background: Some(Background::Color(with_alpha(t.colors.danger, 0.08))),
        border: rounded(radius::NONE),
        ..Style::default()
    }
}
