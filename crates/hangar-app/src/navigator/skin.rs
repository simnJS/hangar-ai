//! Surfaces and marks of the chrome that no widget of hangar-ui draws: the
//! rows of the navigator, the floating capsules of the update banner and the
//! dictation pill, the toolbar's well.
//!
//! These are written from the theme's colours, never a literal, and belong to
//! this package because they are the stylesheet's `.nav-ws`, `.update`,
//! `.voice-pill` and `.activity` rather than a widget anyone else reuses.

use hangar_theme::tokens::{BORDER_WIDTH, radius};
use hangar_theme::{Theme, style};
use iced::widget::container;
use iced::{Background, Border, Color, Shadow, Vector};

/// A hairline of `color` with these corners.
pub fn border(color: Color, radius: f32) -> Border {
    Border {
        color,
        width: BORDER_WIDTH,
        radius: radius.into(),
    }
}

/// A floating capsule (`.update`, `.voice-pill`): the toolbar's colour a
/// little see-through, a hairline, the float ring and a soft shadow. The web
/// app blurred what is behind it, which iced cannot, so it is kept nearly
/// opaque to stay legible over a busy terminal.
pub fn capsule(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.chrome_bar.scale_alpha(0.96))),
        text_color: Some(t.colors.fg),
        border: border(t.colors.line_strong, radius::FULL),
        shadow: Shadow {
            color: t.colors.shadow,
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..container::Style::default()
    }
}

/// The well the toolbar's centre capsule sits in (`.activity`): a hairline's
/// tint with a hairline around it.
pub fn well(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.line)),
        text_color: Some(t.colors.fg),
        border: border(t.colors.line, 7.0),
        ..container::Style::default()
    }
}

/// What a row of the navigator is drawn on: the selected one on the
/// selection's colour, the one under the pointer a step lighter, the others on
/// the navigator's own surface. `dragged` fades the row being dragged, and
/// `focused` rings the row the keyboard is on.
pub fn nav_row(
    t: &Theme,
    active: bool,
    hovered: bool,
    dragged: bool,
    focused: bool,
) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| {
        let fill = if active {
            Some(t.colors.surface_active)
        } else if hovered {
            Some(t.colors.surface_hover)
        } else {
            None
        };
        let fade = if dragged { 0.45 } else { 1.0 };
        container::Style {
            background: fill.map(|c| Background::Color(c.scale_alpha(fade))),
            text_color: Some(
                if active || hovered {
                    t.colors.fg
                } else {
                    t.colors.fg_secondary
                }
                .scale_alpha(fade),
            ),
            border: if focused {
                Border {
                    width: 2.0,
                    ..border(t.colors.accent_ring, radius::MD)
                }
            } else {
                Border {
                    radius: radius::MD.into(),
                    ..Border::default()
                }
            },
            ..container::Style::default()
        }
    }
}

/// The landing spot of a drag over a heading or a folder (`--into`,
/// `.nav-section--drop`, `.nav-root--drop`): the accent's tint with its ring.
pub fn drop_target(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.accent_soft)),
        text_color: Some(t.colors.fg),
        border: border(t.colors.accent_ring, radius::MD),
        ..container::Style::default()
    }
}

/// The insertion line a drag shows between two rows (`--drop-before`).
pub fn insertion(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.accent)),
        border: Border {
            radius: 1.0.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The empty folder's dashed placeholder, as a solid hairline (iced has no
/// dashed border).
pub fn placeholder(t: &Theme) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        border: border(t.colors.line_strong, radius::SM),
        text_color: Some(t.colors.fg_muted),
        ..container::Style::default()
    }
}

/// The navigator's filter field (`.nav-field`); ringed in the accent while it
/// narrows the list.
pub fn field(t: &Theme, narrowing: bool) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.line)),
        text_color: Some(t.colors.fg_muted),
        border: border(
            if narrowing {
                t.colors.accent_ring
            } else {
                t.colors.line
            },
            radius::MD,
        ),
        ..container::Style::default()
    }
}

/// The text of a state: its word colour.
pub fn state_text(
    t: &Theme,
    state: crate::state::AgentActivity,
) -> impl Fn(&iced::Theme) -> iced::widget::text::Style + '_ {
    style::text::state(t, state.into())
}
