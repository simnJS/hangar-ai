//! The bar of a clone whose progress is not known yet.
//!
//! The web app drew a `<progress>` without a value, which the browser
//! animates as a segment sliding along the track. A clone is in that state
//! until git prints its first percentage, and again for the phases that give
//! none (`overallPercent` answers nothing for them): a bar stuck at zero
//! there would read as a frozen clone. iced's progress bar has no such mode,
//! so the segment is drawn here, moved by the dialog's own tick.

use std::time::Duration;

use hangar_theme::{Theme, tokens};
use iced::widget::{Space, container, row};
use iced::{Background, Border, Element, Length};

use crate::message::Message;

/// One trip of the segment from one end of the track to the other and back.
pub const PERIOD: Duration = Duration::from_millis(1600);
/// How often the segment is moved: smooth enough for a thin bar.
pub const TICK: Duration = Duration::from_millis(50);

/// The track is split in this many parts.
const PARTS: u16 = 100;
/// How many of them the moving segment takes.
const SEGMENT: u16 = 30;

/// The parts of the track before the segment and after it, `elapsed` into the
/// animation. The segment goes to the end and comes back, slowing at each
/// end the way a swing does.
pub fn sweep(elapsed: Duration) -> (u16, u16) {
    let turn = (elapsed.as_secs_f32() / PERIOD.as_secs_f32()).fract();
    // 0 → 1 → 0 over one period.
    let there = 1.0 - (2.0 * turn - 1.0).abs();
    let eased = (1.0 - (there * std::f32::consts::PI).cos()) / 2.0;
    let room = PARTS - SEGMENT;
    let before = (eased * f32::from(room)).round() as u16;
    (before, room - before.min(room))
}

/// The bar, `elapsed` into the animation, as thick as `girth`.
pub fn bar<'a>(t: &'a Theme, girth: f32, elapsed: Duration) -> Element<'a, Message> {
    let (before, after) = sweep(elapsed);
    let space = |parts: u16| Space::new().width(Length::FillPortion(parts.max(1)));
    let segment = container(Space::new())
        .width(Length::FillPortion(SEGMENT))
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(t.colors.accent)),
            border: Border {
                radius: tokens::radius::FULL.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    container(row![space(before), segment, space(after)].height(Length::Fill))
        .width(Length::Fill)
        .height(girth)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(t.colors.border)),
            border: Border {
                radius: tokens::radius::FULL.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_starts_at_the_left_end_and_reaches_the_right_one_halfway() {
        assert_eq!(sweep(Duration::ZERO), (0, PARTS - SEGMENT));
        assert_eq!(sweep(PERIOD / 2), (PARTS - SEGMENT, 0));
    }

    #[test]
    fn the_segment_comes_back_and_the_animation_repeats() {
        assert_eq!(sweep(PERIOD), sweep(Duration::ZERO));
        let (a, b) = sweep(PERIOD / 4);
        let (c, d) = sweep(PERIOD * 3 / 4);
        assert_eq!((a, b), (c, d));
    }

    #[test]
    fn the_track_is_always_whole() {
        for ms in (0..4000).step_by(37) {
            let (before, after) = sweep(Duration::from_millis(ms));
            assert_eq!(before + after + SEGMENT, PARTS, "at {ms} ms");
        }
    }
}
