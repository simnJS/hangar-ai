//! The switch (`.switch`): a pill track in `border_strong` that turns the
//! accent when on, and a white knob in every theme.
//!
//! Size it with `toggler(..).size(SWITCH_HEIGHT)`; iced makes the track twice
//! as wide as the knob's travel needs, a little wider than the 30 px the web
//! app drew ([`crate::tokens::size::SWITCH_WIDTH`]).

use iced::widget::toggler::{Status, Style};
use iced::{Background, Color};

use super::faded;
use crate::Theme;
use crate::tokens::{radius, size};

pub fn default(t: &Theme) -> impl Fn(&iced::Theme, Status) -> Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let (on, disabled) = match status {
            Status::Active { is_toggled } | Status::Hovered { is_toggled } => (is_toggled, false),
            Status::Disabled { is_toggled } => (is_toggled, true),
        };
        let track = if on { c.accent } else { c.border_strong };
        let (track, knob, label) = if disabled {
            (faded(track), faded(Color::WHITE), faded(c.fg))
        } else {
            (track, Color::WHITE, c.fg)
        };
        Style {
            background: Background::Color(track),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Background::Color(knob),
            foreground_border_width: 0.5,
            foreground_border_color: Color::from_rgba(0.0, 0.0, 0.0, 0.12),
            text_color: Some(label),
            border_radius: Some(radius::FULL.into()),
            // The knob's 2 px inset in a track 18 px high.
            padding_ratio: 2.0 / size::SWITCH_HEIGHT,
        }
    }
}
