//! The tasks panel of the chat view: the right column, at a fixed width. A placeholder for now, a title and "coming soon": the task that fills it replaces this folder.

use hangar_i18n::Key;
use hangar_theme::{fonts, style, tokens};
use iced::widget::{column, container, text};
use iced::{Element, Length, Task};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::{Model, Workspace};

/// What the region keeps between frames: nothing yet.
#[derive(Debug, Default)]
pub struct State {}

/// What the region answers: nothing yet.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(_st: &mut State, _m: &mut Model, msg: Msg) -> Task<Message> {
    match msg {}
}

pub fn view<'a>(_st: &'a State, ctx: Ctx<'a>, _ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    container(
        column![
            text(ctx.t(Key::ChatViewTasks))
                .size(tokens::font_size::TITLE)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t)),
            text(ctx.t(Key::ChatViewComingSoon))
                .size(tokens::font_size::BODY)
                .style(style::text::muted(t)),
        ]
        .spacing(tokens::space::SM),
    )
    .padding(tokens::space::XXL)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
