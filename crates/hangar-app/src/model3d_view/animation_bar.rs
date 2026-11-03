//! The bar under the viewport that plays a model's animation clips
//! (`AnimationBar`): a play and pause button, the clip (a menu when the file
//! has several), a slider over the clip's time, and the time.
//!
//! hangar-model3d keeps the clock (it moves the clip on with every frame it
//! draws); the bar only shows where the viewer is and sends what the person
//! does. The viewer's position is read when the view is built, and the view is
//! built again a few times a second while a clip plays (`Msg::Tick`), which is
//! all the slider and the time need.

use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, args};
use hangar_model3d::Playback;
use hangar_theme::{fonts, style, tokens};
use hangar_ui::{Icon, input, menu, tip};

use super::{Msg, format, info_strip};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::preview::parts;

/// What the bar shows: where the clip is, what the file's clips are called,
/// and whether the clip menu is open.
pub struct Clips<'a> {
    pub id: &'a str,
    /// Every clip's name, in the file's order (unnamed ones numbered).
    pub names: Vec<String>,
    pub playback: Playback,
    pub menu: bool,
}

fn to(msg: Msg) -> Message {
    Message::Model3d(msg)
}

/// The length the slider runs over: the clip's, or one second for a clip with
/// no length, as the web bar's `max={state.duration || 1}`.
pub fn track_length(duration: f32) -> f32 {
    if duration > 0.0 { duration } else { 1.0 }
}

pub fn view<'a>(ctx: Ctx<'a>, clips: Clips<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let locale = ctx.i18n.locale();
    let Clips {
        id,
        names,
        playback,
        menu: menu_open,
    } = clips;

    let (icon, label) = if playback.playing {
        (Icon::Pause, ctx.t(Key::ModelPause))
    } else {
        (Icon::Play, ctx.t(Key::ModelPlay))
    };
    let play = parts::icon_tool(
        t,
        icon,
        label.into_owned(),
        None,
        Some(to(Msg::PlayPause(id.to_string()))),
    );

    let name = |label: &str| {
        text(label.to_string())
            .size(tokens::font_size::CAPTION_LG)
            .style(style::text::muted(t))
            .wrapping(text::Wrapping::None)
    };
    // One clip is a name; several are a menu of names, the playing one checked.
    let current = names.get(playback.clip).cloned().unwrap_or_default();
    let clip: Element<'a, Message> = if names.len() > 1 {
        let opener = parts::clickable(
            row![
                name(&current),
                hangar_ui::icon(Icon::ChevronDown, 11.0).color(t.colors.fg_muted)
            ]
            .spacing(tokens::space::XS)
            .align_y(Alignment::Center),
            to(Msg::ClipsMenu(id.to_string(), !menu_open)),
        );
        let entries = menu_open.then(|| {
            names
                .iter()
                .enumerate()
                .map(|(at, line)| -> menu::Entry<Message> {
                    menu::item(line.clone())
                        .checked(at == playback.clip)
                        .on_select(to(Msg::Clip(id.to_string(), at)))
                        .into()
                })
                .collect::<Vec<_>>()
        });
        tip::bottom(
            t,
            menu::dropdown(t, opener, entries)
                .on_dismiss(to(Msg::ClipsMenu(id.to_string(), false))),
            ctx.t(Key::ModelClip),
        )
    } else {
        tip::bottom(
            t,
            container(name(&current)).max_width(200.0),
            ctx.t(Key::ModelClip),
        )
    };

    // The track, to the clip's length. A drag sends the time for every step it
    // takes; the viewer holds it to the clip.
    let owner = id.to_string();
    let scrub = input::slider(
        t,
        0.0..=track_length(playback.duration),
        playback.time.min(track_length(playback.duration)),
        move |time| to(Msg::Seek(owner.clone(), time)),
    )
    .step(0.01);
    let track = tip::bottom(
        t,
        container(scrub).width(Length::Fill),
        ctx.t(Key::ModelTime),
    );

    let time = ctx
        .i18n
        .t_with(
            Key::ModelTimeOf,
            &args!(
                "time" => format::seconds(playback.time, locale),
                "total" => format::seconds(playback.duration, locale)
            ),
        )
        .into_owned();
    let time = text(time)
        .size(tokens::font_size::LABEL)
        .font(fonts::mono())
        .style(style::text::muted(t))
        .wrapping(text::Wrapping::None);

    let line = row![play, clip, track, time]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Center);
    info_strip::band(
        t,
        line,
        32.0,
        Padding {
            top: 0.0,
            right: 10.0,
            bottom: 0.0,
            left: 6.0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_with_no_length_still_has_a_track_to_stand_on() {
        assert_eq!(track_length(0.0), 1.0);
        assert_eq!(track_length(-2.0), 1.0);
        assert_eq!(track_length(8.0), 8.0);
    }
}
