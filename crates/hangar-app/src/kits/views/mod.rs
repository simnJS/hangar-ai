//! The screens of Settings › Kits and of the dialogs that enable a kit.
//!
//! Each screen is two things: a model that holds every sentence and decision
//! (`*_model.rs`, tested), and the drawing of it (`activation.rs`,
//! `settings.rs`, `gallery.rs`, `workspace_section.rs`). This file is the few
//! pieces they share.

pub mod activation;
pub mod activation_model;
pub mod gallery;
pub mod settings;
pub mod workspace_section;

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_i18n::Key;
use hangar_theme::{fonts, style, tokens};
use hangar_ui::badge::{self, Tone};
use hangar_ui::{Icon, button, text as typography};

use super::Msg;
use crate::ctx::Ctx;
use crate::message::Message;
use activation_model::{Tag, TagTone};

/// A message of this package, as the router takes it.
pub fn msg(msg: Msg) -> Message {
    Message::Kits(msg)
}

/// A tag (`.kit-act__tag`): a small word that says where something stands.
pub fn tag<'a>(ctx: Ctx<'a>, tag: &Tag) -> Element<'a, Message> {
    match tag.tone {
        TagTone::Plain => badge::tag(ctx.theme, tag.label.clone()),
        TagTone::Ok => badge::badge(ctx.theme, tag.label.clone(), Tone::Success),
        TagTone::New => badge::badge(ctx.theme, tag.label.clone(), Tone::Warning),
    }
}

/// The heading of a block of a dialog (`.kit-act__heading`).
pub fn heading<'a>(ctx: Ctx<'a>, words: &str) -> Element<'a, Message> {
    typography::heading(ctx.theme, words.to_uppercase())
        .size(tokens::font_size::UI)
        .into()
}

/// A line of small print (`.kit-act__note`), wrapped.
pub fn note<'a>(ctx: Ctx<'a>, words: impl Into<String>) -> Element<'a, Message> {
    typography::hint(ctx.theme, words.into())
        .width(Length::Fill)
        .into()
}

/// A notice (`.notice`): a sentence on a tint of the tone it is about.
pub fn notice<'a>(ctx: Ctx<'a>, tone: Tone, words: impl Into<String>) -> Element<'a, Message> {
    let ink = style::container::tone_text(ctx.theme, tone);
    container(
        text(words.into())
            .size(tokens::font_size::UI)
            .width(Length::Fill)
            .style(move |_: &iced::Theme| text::Style { color: Some(ink) }),
    )
    .padding([tokens::space::MD, tokens::space::XL])
    .width(Length::Fill)
    .style(style::container::badge(ctx.theme, tone))
    .into()
}

/// A command, a path, a rule: shown as it is, in the mono face, in a well of
/// its own, with a button that copies it. The web app's `<pre>` could be
/// selected; iced's text cannot, and a text editor in its place would take the
/// mouse wheel from the dialog's scrolling, so the copy button is the way to
/// take it with you. Long lines break anywhere: a hook's matcher has nowhere
/// else to.
pub fn code_block<'a>(ctx: Ctx<'a>, code: &str) -> Element<'a, Message> {
    let copy = button::icon_button(ctx.theme, Icon::Copy)
        .small()
        .tip(ctx.t(Key::PreviewCopy))
        .on_press(msg(Msg::Copy(code.to_owned())));
    container(
        row![
            text(code.to_owned())
                .size(tokens::font_size::UI)
                .font(fonts::mono())
                .wrapping(text::Wrapping::WordOrGlyph)
                .width(Length::Fill)
                .style(style::text::primary(ctx.theme)),
            copy,
        ]
        .spacing(tokens::space::MD)
        .align_y(Alignment::Start),
    )
    .padding([tokens::space::MD, tokens::space::LG])
    .width(Length::Fill)
    .style(style::container::well(ctx.theme))
    .into()
}

/// Rows of a block, spaced the way the dialog spaces them.
pub fn stack<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    iced::widget::Column::with_children(items)
        .spacing(tokens::space::MD)
        .width(Length::Fill)
        .into()
}

/// A block of the activation dialog: a heading over its body, a hairline above.
pub fn block<'a>(
    ctx: Ctx<'a>,
    title: &str,
    body: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        iced::widget::rule::horizontal(1).style(style::rule::hairline(ctx.theme)),
        container(column![heading(ctx, title), stack(body)].spacing(tokens::space::LG))
            .padding([tokens::space::LG, 0.0])
            .width(Length::Fill),
    ]
    .spacing(0)
    .width(Length::Fill)
    .into()
}
