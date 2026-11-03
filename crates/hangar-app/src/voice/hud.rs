//! What dictation looks like while it is happening (`VoiceHud.tsx`).
//!
//! A pill rather than a sentence. Push-to-talk lasts a few seconds and the
//! finger is already on the key, so instructions are not what is wanted on
//! screen: the one question is whether it can hear you, and a row of bars
//! moving with your voice answers it faster than any wording could. The text
//! is still there as the tooltip.

use std::time::Duration;

use iced::widget::{Space, container, row, tooltip};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::Key;
use hangar_theme::style;
use hangar_theme::tokens::radius;
use hangar_ui::logo::logo;

use super::{BARS, Phase, State};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::navigator::skin;

/// The meter's height; fixed, so the pill does not breathe with the voice
/// inside it.
const METER_HEIGHT: f32 = 18.0;
const BAR_WIDTH: f32 = 3.0;
/// A bar never drops below this share of the meter: the floor is a quarter of
/// the height rather than a sliver, because a 2 px bar on a 3 px width is a
/// dot, and a row of dots reads as a dotted line, not as an idle meter.
const FLOOR: f32 = 24.0;
const SPAN: f32 = 76.0;

/// A bar's height as a share of the meter, 0 to 100, for a peak of `level`.
/// Square-rooted: a peak meter on a linear scale spends its life on the floor.
pub fn bar_percent(level: f32) -> f32 {
    FLOOR + level.clamp(0.0, 1.0).sqrt() * SPAN
}

/// The bars marking time once the audio is gone (`voice-wave`): the same shape
/// as a real signal so the pill does not jump, kept shallower than one reaches
/// so a glance tells the two apart without reading the colour. Each bar lags
/// the one before by 70 ms; the wave takes a second to turn.
pub fn wave_percent(index: usize, elapsed: Duration) -> f32 {
    const BASE: f32 = 26.0;
    const PEAK: f32 = 2.6;
    let lag = Duration::from_millis(70 * index as u64);
    let turn = elapsed.saturating_sub(lag).as_secs_f32() % 1.0;
    // Ease in and out between 1x and the peak, like the keyframes.
    let eased = 0.5 - 0.5 * (turn * std::f32::consts::TAU).cos();
    BASE * (1.0 + (PEAK - 1.0) * eased)
}

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    if st.phase() == Phase::Idle {
        return Space::new().into();
    }
    let t = ctx.theme;
    let transcribing = st.phase() == Phase::Transcribing;
    let label = ctx.t(if transcribing {
        Key::VoiceTranscribing
    } else {
        Key::VoiceRecording
    });

    let elapsed = st.since().map_or(Duration::ZERO, |since| {
        ctx.now.saturating_duration_since(since)
    });
    let bars = (0..BARS).map(|index| {
        let percent = if transcribing {
            wave_percent(index, elapsed)
        } else {
            bar_percent(st.bars()[index])
        };
        // Ink: the foreground while it traces a voice, the accent while it
        // marks time.
        let ink = if transcribing {
            t.colors.accent
        } else {
            t.colors.fg
        };
        container(Space::new())
            .width(BAR_WIDTH)
            .height(METER_HEIGHT * percent / 100.0)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(ink)),
                border: iced::Border {
                    radius: radius::FULL.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            })
            .into()
    });
    let meter = container(row(bars).spacing(2).align_y(Alignment::Center))
        .height(METER_HEIGHT)
        .align_y(Alignment::Center);

    let rule = t.colors.line_strong;
    let separator = container(Space::new())
        .width(1)
        .height(16)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(rule)),
            ..container::Style::default()
        });
    let pill = container(
        row![logo(t, 15.0), separator, meter]
            .spacing(10)
            .align_y(Alignment::Center),
    )
    .height(34)
    .padding(Padding {
        top: 0.0,
        right: 14.0,
        bottom: 0.0,
        left: 12.0,
    })
    .align_y(Alignment::Center)
    .width(Length::Shrink)
    .style(skin::capsule(t));
    tooltip(
        pill,
        container(iced::widget::text(label).size(hangar_theme::tokens::font_size::CAPTION_LG))
            .padding([4, 8])
            .style(style::container::tooltip(t)),
        tooltip::Position::Top,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silent_meter_rests_on_a_quarter_of_its_height() {
        assert_eq!(bar_percent(0.0), 24.0);
        assert_eq!(bar_percent(1.0), 100.0);
    }

    #[test]
    fn the_peak_is_square_rooted_so_a_quiet_voice_still_shows() {
        // A linear scale would give 24 + 0.04 * 76 = 27: all but invisible.
        assert!(bar_percent(0.04) > 35.0);
        assert!(bar_percent(0.25) > bar_percent(0.04));
    }

    #[test]
    fn a_level_out_of_range_is_held_inside_the_meter() {
        assert_eq!(bar_percent(7.0), 100.0);
        assert_eq!(bar_percent(-1.0), 24.0);
    }

    #[test]
    fn the_wave_rises_and_falls_once_a_second() {
        let low = wave_percent(0, Duration::ZERO);
        let high = wave_percent(0, Duration::from_millis(500));
        assert!((low - 26.0).abs() < 0.01);
        assert!((high - 26.0 * 2.6).abs() < 0.01);
        assert!((wave_percent(0, Duration::from_millis(1000)) - low).abs() < 0.01);
    }

    #[test]
    fn each_bar_follows_the_one_before_it() {
        let at = Duration::from_millis(300);
        assert_ne!(wave_percent(0, at), wave_percent(4, at));
        // Before its turn a bar rests at the base.
        assert!((wave_percent(8, Duration::from_millis(100)) - 26.0).abs() < 0.01);
    }
}
