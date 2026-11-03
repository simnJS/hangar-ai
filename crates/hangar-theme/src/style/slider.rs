//! Sliders (`.set-range`): a thin track filled with the accent up to the knob,
//! and a white knob in every theme, as the system draws it.
//!
//! The knob is a physical part of the control and not a colour that means
//! something, so it does not follow the theme. Size the widget's height with
//! `slider(..).height(SLIDER_HEIGHT)`.

use iced::widget::slider::{Handle, HandleShape, Rail, Status, Style};
use iced::{Background, Color};

use super::rounded;
use crate::Theme;

/// The height the widget is given so that the knob and its focus halo fit
/// (`.set-range` is 18 px).
pub const HEIGHT: f32 = 18.0;

/// The thickness of the track.
const TRACK: f32 = 4.0;

/// The knob's radius, 14 px across.
const KNOB: f32 = 7.0;

pub fn default(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let (edge, width) = match status {
            Status::Active => (Color::from_rgba(0.0, 0.0, 0.0, 0.3), 0.5),
            // The halo of the focus ring, since a dragged knob has the keyboard.
            Status::Hovered => (c.border_strong, 1.0),
            Status::Dragged => (c.accent_ring, 3.0),
        };
        Style {
            rail: Rail {
                backgrounds: (
                    Background::Color(c.accent),
                    Background::Color(c.border_strong),
                ),
                width: TRACK,
                border: rounded(TRACK / 2.0),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: KNOB },
                background: Background::Color(Color::WHITE),
                border_width: width,
                border_color: edge,
            },
        }
    }
}
