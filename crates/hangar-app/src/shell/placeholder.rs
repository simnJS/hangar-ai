//! What the window shows when there is nothing else to: the boot screen until
//! the state is in, the empty window, the workspace shown in its own window
//! (App.tsx:1740–1790), and the stand-in every package view draws until the
//! package lands.

use iced::widget::{button, center, column, container, row, text};
use iced::{Element, Length};

use hangar_i18n::{Key, args};
use hangar_theme::{style, tokens};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::shell::Overlay;
use crate::state::types::{AGENTS, AgentId, Workspace};

/// `app.loading`, until `load_state` answers.
pub fn boot<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    center(
        text(ctx.t(Key::AppLoading))
            .size(tokens::font_size::BODY)
            .style(style::text::muted(ctx.theme)),
    )
    .into()
}

/// No workspace yet: what Hangar runs, and the button that makes one.
pub fn no_workspace<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    let agents = AGENTS
        .iter()
        .filter(|agent| agent.id != AgentId::Shell)
        .map(|agent| agent.label)
        .collect::<Vec<_>>()
        .join(", ");
    let body = ctx
        .i18n
        .t_with(Key::PlaceholderBody, &args!("agents" => agents.as_str()));
    let content = column![
        text(ctx.t(Key::PlaceholderTitle))
            .size(tokens::font_size::TITLE)
            .font(hangar_theme::fonts::ui_semibold()),
        text(body)
            .size(tokens::font_size::BODY)
            .align_x(iced::Center)
            .style(style::text::secondary(ctx.theme)),
        button(text(ctx.t(Key::SidebarNew)).size(tokens::font_size::UI))
            .style(style::button::primary(ctx.theme))
            .on_press(Message::Intent(Intent::OpenOverlay(
                Overlay::WorkspaceDialog
            ))),
    ]
    .spacing(tokens::space::XL)
    .align_x(iced::Center)
    .max_width(tokens::size::DIALOG_WIDTH);
    center(content).into()
}

/// The active workspace is drawn by its own window: say so, with the way to
/// that window (activating a detached workspace brings its window forward)
/// and the way back.
pub fn elsewhere<'a>(ctx: Ctx<'a>, ws: &'a Workspace) -> Element<'a, Message> {
    let title = ctx.i18n.t_with(
        Key::WindowElsewhereTitle,
        &args!("name" => ws.name.as_str()),
    );
    let actions = row![
        button(text(ctx.t(Key::WindowShow)).size(tokens::font_size::UI))
            .style(style::button::primary(ctx.theme))
            .on_press(Message::Intent(Intent::ActivateWorkspace(ws.id.clone()))),
        button(text(ctx.t(Key::WindowBringBack)).size(tokens::font_size::UI))
            .style(style::button::secondary(ctx.theme))
            .on_press(Message::Intent(Intent::Reattach(ws.id.clone()))),
    ]
    .spacing(tokens::space::MD);
    let content = column![
        text(title)
            .size(tokens::font_size::TITLE)
            .font(hangar_theme::fonts::ui_semibold()),
        text(ctx.t(Key::WindowElsewhereBody))
            .size(tokens::font_size::BODY)
            .align_x(iced::Center)
            .style(style::text::secondary(ctx.theme)),
        actions,
    ]
    .spacing(tokens::space::XL)
    .align_x(iced::Center)
    .max_width(tokens::size::DIALOG_WIDTH);
    center(content).into()
}

/// The stand-in a package's view draws until the package replaces it: the
/// name of the slot, small and muted, centred in the space the shell gave it.
/// Not translated on purpose: it is a marker for whoever builds the package,
/// never part of a finished surface.
pub fn stub<'a>(ctx: Ctx<'a>, slot: &'static str) -> Element<'a, Message> {
    container(
        text(slot)
            .size(tokens::font_size::CAPTION)
            .font(hangar_theme::fonts::mono())
            .style(style::text::subtle(ctx.theme)),
    )
    .center(Length::Fill)
    .into()
}

/// The stand-in of an overlay: a scrim that closes it when pressed, and a
/// dialog-sized card with the slot's name.
pub fn stub_modal<'a>(ctx: Ctx<'a>, slot: &'static str) -> Element<'a, Message> {
    let card = container(stub(ctx, slot))
        .width(tokens::size::DIALOG_WIDTH)
        .height(240)
        .style(style::container::modal(ctx.theme));
    let scrim = button(center(iced::widget::opaque(card)))
        .padding(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_, _| button::Style::default())
        .on_press(Message::Intent(Intent::CloseOverlay));
    container(scrim)
        .style(style::container::scrim(ctx.theme))
        .into()
}

/// A stand-in for an embed (a badge, a button inside another surface): just
/// the name, no space of its own.
pub fn stub_inline<'a>(ctx: Ctx<'a>, slot: &'static str) -> Element<'a, Message> {
    text(slot)
        .size(tokens::font_size::MICRO)
        .font(hangar_theme::fonts::mono())
        .style(style::text::subtle(ctx.theme))
        .into()
}
