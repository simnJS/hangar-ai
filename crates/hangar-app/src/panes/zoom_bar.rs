//! The strip over a zoomed pane (`PaneZoomBar.tsx`). Without it a zoomed
//! grid looks like a workspace that lost its other panes: it says which pane
//! fills the grid, that the rest are still running, where each of them
//! stands, and puts each one a click away.

use hangar_i18n::{Key, args};
use hangar_theme::{style, tokens};
use hangar_ui::{glyph, tip};
use iced::widget::{Space, container, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::panes::Msg;
use crate::panes::header::activity_word;
use crate::panes::status::ui_glyph;
use crate::state::{AgentActivity, GlyphState, WsId};

/// The strip's height (`--zoombar-h`, 28 px).
pub const HEIGHT: f32 = 28.0;

/// Below this the sentence goes first, keeping the facts and the way out
/// (`@container zoombar (max-width: 55em)`, at its 11.5 px).
const SENTENCE_MIN: f32 = 55.0 * 11.5;

/// A pane out of sight: its id, its name and where it stands.
pub struct Other<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub activity: AgentActivity,
}

pub fn view<'a>(
    ctx: Ctx<'a>,
    ws: &'a WsId,
    zoomed_name: &'a str,
    others: Vec<Other<'a>>,
    width: f32,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let tag = container(
        text(ctx.t(Key::ZoomTag))
            .size(tokens::font_size::MICRO)
            .font(hangar_theme::fonts::mono())
            .style(style::text::accent(t)),
    )
    .padding(Padding::from([2.0, 6.0]))
    .style(style::container::accent_soft(t));

    let mut bar = row![
        tag,
        text(zoomed_name)
            .size(tokens::font_size::CAPTION_LG)
            .font(hangar_theme::fonts::ui_semibold())
            .wrapping(text::Wrapping::None)
            .style(style::text::primary(t)),
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);

    if width > SENTENCE_MIN {
        bar = bar.push(
            row![
                text("·")
                    .size(tokens::font_size::CAPTION_LG)
                    .style(style::text::subtle(t)),
                text(ctx.i18n.tn(Key::ZoomOthers, others.len(), &[]))
                    .size(tokens::font_size::CAPTION_LG)
                    .wrapping(text::Wrapping::None)
                    .style(style::text::muted(t)),
            ]
            .spacing(tokens::space::MD),
        );
    }

    let mut chips = row![].spacing(1).align_y(Alignment::Center);
    for other in others {
        let words = format!(
            "{} · {}",
            ctx.i18n.t_with(Key::ZoomShow, &args!("name" => other.name)),
            activity_word(&ctx, other.activity),
        );
        let state = match other.activity {
            AgentActivity::Working => GlyphState::Working,
            AgentActivity::Waiting => GlyphState::Waiting,
            AgentActivity::Yours => GlyphState::Yours,
            AgentActivity::Idle => GlyphState::Idle,
        };
        let chip = iced::widget::button(
            row![
                glyph::state(t, ui_glyph(state), 7.0),
                text(other.name)
                    .size(tokens::font_size::CAPTION_LG)
                    .font(hangar_theme::fonts::ui_medium())
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(tokens::space::SM)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([0.0, 7.0]))
        .height(20)
        .style(style::button::ghost(t))
        .on_press(Message::Panes(Msg::Zoom(
            ws.clone(),
            Some(other.id.to_owned()),
        )));
        chips = chips.push(tip::bottom(t, chip, words));
    }
    bar = bar.push(chips);
    bar = bar.push(Space::new().width(Length::Fill));
    bar = bar.push(
        iced::widget::button(
            text(ctx.t(Key::ZoomRestore))
                .size(tokens::font_size::CAPTION_LG)
                .font(hangar_theme::fonts::ui_medium()),
        )
        .padding(Padding::from([0.0, 9.0]))
        .height(22)
        .style(style::button::secondary(t))
        .on_press(Message::Panes(Msg::Zoom(ws.clone(), None))),
    );

    container(bar.height(Length::Fill))
        .padding(Padding {
            top: 0.0,
            right: 6.0,
            bottom: 0.0,
            left: 10.0,
        })
        .width(Length::Fill)
        .height(HEIGHT)
        .clip(true)
        .style(style::container::pane_head(t, false))
        .into()
}
