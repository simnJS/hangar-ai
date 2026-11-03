//! The bar under the viewport that lists a model's animation clips
//! (`AnimationBar`).
//!
//! hangar-model3d reads the clips of a glTF (their names and lengths) but does
//! not play them yet, so the bar lists them and says so: the play button, the
//! clip menu's lines and the track are drawn disabled, each with the reason,
//! rather than left out. When the crate gains playback, the controls get their
//! messages here and nowhere else.

use iced::widget::{Space, container, progress_bar, row, text};
use iced::{Alignment, Element, Length, Padding};

use hangar_i18n::{Key, args};
use hangar_theme::{fonts, style, tokens};
use hangar_ui::{Icon, menu, tip};

use super::{Msg, format, info_strip};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::preview::parts;

/// A clip as the bar lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipLine {
    pub name: String,
    /// In seconds.
    pub duration: f32,
}

/// What the bar shows: the clips, and whether the clip menu is open.
pub struct Clips<'a> {
    pub id: &'a str,
    pub lines: Vec<ClipLine>,
    pub menu: bool,
}

pub fn view<'a>(ctx: Ctx<'a>, clips: Clips<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let locale = ctx.model.locale();
    let reason = ctx.t(Key::ModelAnimationsLater).into_owned();
    let first = clips.lines.first();

    // The play button, drawn as the old bar had it and unable to be pressed.
    let play = parts::icon_tool(t, Icon::Play, reason.clone(), None, None::<Message>);

    // One clip is a name; several are a menu of names, so the person can see
    // what the file holds even though none can be chosen yet.
    let name = |label: &str| {
        text(label.to_string())
            .size(tokens::font_size::CAPTION_LG)
            .style(style::text::muted(t))
            .wrapping(text::Wrapping::None)
    };
    let clip: Element<'a, Message> = match (first, clips.lines.len()) {
        (None, _) => Space::new().into(),
        (Some(only), 1) => tip::bottom(
            t,
            container(name(&only.name)).max_width(200.0),
            ctx.t(Key::ModelClip),
        ),
        (Some(first), _) => {
            let id = clips.id;
            let opener = parts::clickable(
                row![
                    name(&first.name),
                    hangar_ui::icon(Icon::ChevronDown, 11.0).color(t.colors.fg_muted)
                ]
                .spacing(tokens::space::XS)
                .align_y(Alignment::Center),
                Message::Model3d(Msg::ClipsMenu(id.to_string(), !clips.menu)),
            );
            let entries = clips.menu.then(|| {
                clips
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(at, line)| -> menu::Entry<Message> {
                        menu::item(line.name.clone())
                            .checked(at == 0)
                            .disabled_because(reason.clone())
                            .into()
                    })
                    .collect::<Vec<_>>()
            });
            tip::bottom(
                t,
                menu::dropdown(t, opener, entries)
                    .on_dismiss(Message::Model3d(Msg::ClipsMenu(id.to_string(), false))),
                ctx.t(Key::ModelClip),
            )
        }
    };

    // The track, empty: nothing plays, so there is nothing to scrub.
    let track = tip::bottom(
        t,
        container(
            progress_bar(0.0..=1.0, 0.0)
                .girth(3.0)
                .style(style::progress_bar::accent(t)),
        )
        .width(Length::Fill),
        reason,
    );

    let duration = first.map_or(0.0, |line| line.duration);
    let time = ctx
        .i18n
        .t_with(
            Key::ModelTimeOf,
            &args!(
                "time" => format::seconds(0.0, locale),
                "total" => format::seconds(duration, locale)
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
