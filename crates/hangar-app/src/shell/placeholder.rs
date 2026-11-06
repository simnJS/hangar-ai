//! What the window shows when there is nothing else to: the boot screen until
//! the state is in, the empty window, the workspace shown in its own window
//! (App.tsx:1740–1790).

use iced::Element;
use iced::widget::{button, center, column, row, text};

use hangar_i18n::{Key, args};
use hangar_theme::{style, tokens};

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::shell::Overlay;
use crate::state::types::Workspace;

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
    let body = ctx.t(Key::PlaceholderBody);
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
