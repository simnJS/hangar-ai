//! The conversation of the chat view: the left, main region. A placeholder for now, a title and "coming soon": the task that fills it replaces this folder.

use hangar_i18n::Key;
use hangar_theme::{fonts, style, tokens};
use iced::widget::{column, container, text};
use iced::{Element, Length, Subscription, Task};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::services::{CoreEvent, Services};
use crate::state::{Model, Workspace};

/// What the region keeps between frames: nothing yet.
#[derive(Debug, Default)]
pub struct State {}

/// What the region answers: nothing yet.
#[derive(Debug, Clone)]
pub enum Msg {}

pub fn update(_st: &mut State, _m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {}
}

/// What the region listens to on its own: nothing yet.
pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::none()
}

/// A core event, offered to the region: none is heard yet.
pub fn on_event(_st: &mut State, _m: &mut Model, _event: &CoreEvent) -> Option<Task<Message>> {
    None
}

pub fn view<'a>(_st: &'a State, ctx: Ctx<'a>, _ws: &'a Workspace) -> Element<'a, Message> {
    let t = ctx.theme;
    container(
        column![
            text(ctx.t(Key::ChatViewConversation))
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
