//! What every viewer of the preview draws the same way: a message in the
//! middle of the stage, a banner over it, a tool of the bar, a passing word
//! after an action, a size in bytes, a line saying what the file is
//! (`components/preview/parts.tsx`, and the `.model-*` rules of `model.css`).
//! The 3D view (P17) uses them too: they are the preview's vocabulary.

use iced::widget::{
    Space, button, column, container, mouse_area, progress_bar, row, rule, scrollable, text,
};
use iced::{Alignment, Color, Element, Length, Padding, alignment};

use hangar_i18n::{Key, Number, args};
use hangar_theme::style;
use hangar_theme::{Theme, fonts, tokens};
use hangar_ui::{Icon, icon};

use super::glyph::{Glyph, glyph};
use crate::ctx::Ctx;

/// A word after an action, gone after a moment, longer for an error, to be
/// read (`Notice`, `useNotice`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

impl Notice {
    pub fn info(text: impl Into<String>) -> Notice {
        Notice {
            text: text.into(),
            error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Notice {
        Notice {
            text: text.into(),
            error: true,
        }
    }

    /// How long it stays: 2.2 s, or 6 s for an error.
    pub fn lasts(&self) -> std::time::Duration {
        std::time::Duration::from_millis(if self.error { 6000 } else { 2200 })
    }
}

/// What a message of the stage shows above its title.
#[derive(Debug, Clone, Copy)]
pub enum Mark {
    Icon(Icon),
    Glyph(Glyph),
}

/// How a message feels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feel {
    Calm,
    Danger,
    Warning,
}

/// Over the stage, centred: a mark, a title, a detail in mono and one action
/// (`StageMessage`).
pub fn stage_message<'a, M: Clone + 'a>(
    t: &'a Theme,
    mark: Mark,
    feel: Feel,
    title: impl Into<String>,
    detail: Option<String>,
    action: Option<(String, M)>,
) -> Element<'a, M> {
    let colour = match feel {
        Feel::Calm => t.colors.fg_subtle,
        Feel::Danger => t.colors.danger,
        Feel::Warning => t.colors.warning,
    };
    let mark: Element<'a, M> = match mark {
        Mark::Icon(which) => icon(which, 20.0).color(colour).into(),
        Mark::Glyph(which) => glyph(which, 20.0, colour),
    };
    let mut body = column![
        mark,
        text(title.into())
            .size(tokens::font_size::BODY)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
            .align_x(alignment::Horizontal::Center),
    ]
    .spacing(tokens::space::MD)
    .align_x(Alignment::Center);
    if let Some(detail) = detail {
        body = body.push(
            scrollable(
                text(detail)
                    .size(tokens::font_size::CAPTION)
                    .font(fonts::mono())
                    .style(style::text::muted(t))
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill),
            )
            .height(Length::Shrink)
            .style(style::scrollable::default(t)),
        );
    }
    if let Some((label, on_press)) = action {
        body = body.push(hangar_ui::button::secondary(t, label).on_press(on_press));
    }
    container(container(body).max_width(340.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(tokens::space::XXXL)
        .into()
}

/// Something to know about what is on screen, without taking it away
/// (`Banner`): a strip over the top of the stage.
pub fn banner<'a, M: Clone + 'a>(
    t: &'a Theme,
    feel: Feel,
    text_line: impl Into<String>,
    detail: Option<String>,
    action: Option<(String, M)>,
) -> Element<'a, M> {
    let tone = match feel {
        Feel::Danger => t.colors.danger,
        _ => t.colors.warning,
    };
    let mut words = column![
        text(text_line.into())
            .size(tokens::font_size::CAPTION_LG)
            .style(style::text::primary(t)),
    ];
    if let Some(detail) = detail {
        words = words.push(
            text(detail)
                .size(tokens::font_size::LABEL)
                .font(fonts::mono())
                .style(style::text::subtle(t))
                .wrapping(text::Wrapping::None),
        );
    }
    let mut line = row![
        icon(Icon::Alert, 13.0).color(tone),
        words.width(Length::Fill)
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    if let Some((label, on_press)) = action {
        line = line.push(
            hangar_ui::button::secondary(t, label)
                .small()
                .on_press(on_press),
        );
    }
    container(
        container(line)
            .padding(Padding {
                top: 6.0,
                right: 6.0,
                bottom: 6.0,
                left: 10.0,
            })
            .style(style::container::popover(t)),
    )
    .padding(tokens::space::MD)
    .width(Length::Fill)
    .into()
}

/// A button of a viewer's bar, labelled by its tooltip; a toggle when
/// `pressed` is given (`Tool`). Without a message it is drawn disabled.
pub fn tool<'a, M: Clone + 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    label: impl Into<String>,
    pressed: Option<bool>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let body = button(container(content).center(Length::Fill))
        .padding(0)
        .width(24.0)
        .height(24.0)
        .on_press_maybe(on_press);
    let body = match pressed {
        Some(on) => body.style(style::button::toggle(t, on)),
        None => body.style(style::button::icon(t)),
    };
    hangar_ui::tip::bottom(t, body, label)
}

/// A tool drawn with an icon of the app's set.
pub fn icon_tool<'a, M: Clone + 'a>(
    t: &'a Theme,
    which: Icon,
    label: impl Into<String>,
    pressed: Option<bool>,
    on_press: Option<M>,
) -> Element<'a, M> {
    tool(t, icon(which, tokens::size::ICON), label, pressed, on_press)
}

/// A tool drawn with a glyph of the preview's set.
pub fn glyph_tool<'a, M: Clone + 'a>(
    t: &'a Theme,
    which: Glyph,
    label: impl Into<String>,
    pressed: Option<bool>,
    on_press: Option<M>,
) -> Element<'a, M> {
    let colour = if pressed == Some(true) {
        t.colors.accent_hi
    } else {
        t.colors.fg_muted
    };
    tool(
        t,
        glyph(which, tokens::size::ICON, colour),
        label,
        pressed,
        on_press,
    )
}

/// The bar over a viewer (`.model-panel__bar`): tools on the left, a spacer,
/// what it has to say on the right; a hairline under it.
pub fn bar<'a, M: 'a>(
    t: &'a Theme,
    tools: Vec<Element<'a, M>>,
    aside: Vec<Element<'a, M>>,
) -> Element<'a, M> {
    let mut line = row![].spacing(2).align_y(Alignment::Center);
    for tool in tools {
        line = line.push(tool);
    }
    line = line.push(Space::new().width(Length::Fill));
    for item in aside {
        line = line.push(item);
    }
    column![
        container(line)
            .padding([0.0, 6.0])
            .height(32.0)
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .style(style::container::panel(t)),
        rule::horizontal(tokens::BORDER_WIDTH).style(style::rule::hairline(t)),
    ]
    .into()
}

/// A hairline between two groups of tools (`.model-panel__sep`).
pub fn separator<'a, M: 'a>(t: &'a Theme) -> Element<'a, M> {
    container(rule::vertical(1).style(style::rule::strong(t)))
        .height(14.0)
        .padding([0.0, 5.0])
        .into()
}

/// A passing word after an action (`NoticeText`): a check before a success.
pub fn notice_text<'a, M: 'a>(t: &'a Theme, notice: &'a Notice) -> Element<'a, M> {
    let words = text(notice.text.as_str())
        .size(tokens::font_size::CAPTION)
        .wrapping(text::Wrapping::None);
    if notice.error {
        row![words.style(style::text::danger(t))].into()
    } else {
        row![
            icon(Icon::Check, 12.0).color(t.colors.success),
            words.style(style::text::muted(t))
        ]
        .spacing(5)
        .align_y(Alignment::Center)
        .into()
    }
}

/// A small word on a bar (`.preview-chip`): outlined, or quiet and mono.
pub fn chip<'a, M: 'a>(t: &'a Theme, label: impl Into<String>, quiet: bool) -> Element<'a, M> {
    let words = text(label.into())
        .size(tokens::font_size::LABEL)
        .wrapping(text::Wrapping::None);
    if quiet {
        words
            .font(fonts::mono())
            .style(style::text::subtle(t))
            .into()
    } else {
        container(words.style(style::text::muted(t)))
            .padding([1.0, 6.0])
            .style(move |_| iced::widget::container::Style {
                border: iced::Border {
                    color: t.colors.line_strong,
                    width: 1.0,
                    radius: tokens::radius::XS.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

/// A line over the stage that says the file is shown in part or gone
/// (`.preview-note`): the warning's tint, or quiet for what a kind of file
/// cannot do.
pub fn note<'a, M: 'a>(t: &'a Theme, words: impl Into<String>, quiet: bool) -> Element<'a, M> {
    let background = if quiet {
        t.colors.chrome
    } else {
        warning_wash(t)
    };
    let tone = if quiet {
        t.colors.fg_muted
    } else {
        t.colors.fg_secondary
    };
    container(
        text(words.into())
            .size(tokens::font_size::CAPTION_LG)
            .line_height(tokens::line_height::UI)
            .color(tone),
    )
    .padding([6.0, 12.0])
    .width(Length::Fill)
    .style(move |_| iced::widget::container::Style {
        background: Some(iced::Background::Color(background)),
        ..Default::default()
    })
    .into()
}

/// The warning colour over the chrome, at 9 % (`color-mix`).
fn warning_wash(t: &Theme) -> Color {
    let (wash, base) = (t.colors.warning, t.colors.chrome);
    let mix = |a: f32, b: f32| b + (a - b) * 0.09;
    Color::from_rgb(
        mix(wash.r, base.r),
        mix(wash.g, base.g),
        mix(wash.b, base.b),
    )
}

/// A size in bytes, in the unit that reads best: "812 bytes", "48.2 KB",
/// "12 MB" (`useByteSize`).
pub fn byte_size(ctx: Ctx<'_>, bytes: u64) -> String {
    let locale = ctx.model.locale();
    let number = |value: f64| {
        let rounded = if value < 10.0 {
            (value * 10.0).round() / 10.0
        } else {
            value.round()
        };
        Number::from(rounded).format(locale)
    };
    if bytes < 1024 {
        ctx.i18n
            .t_with(Key::PreviewBytes, &args!("size" => number(bytes as f64)))
            .into_owned()
    } else if bytes < 1024 * 1024 {
        ctx.i18n
            .t_with(
                Key::PreviewKilobytes,
                &args!("size" => number(bytes as f64 / 1024.0)),
            )
            .into_owned()
    } else {
        ctx.i18n
            .t_with(
                Key::ModelMegabytes,
                &args!("size" => number(bytes as f64 / (1024.0 * 1024.0))),
            )
            .into_owned()
    }
}

/// A file on its way: its name over a bar that travels, as the 3D view's
/// first load (`Loading`).
pub fn loading<'a, M: 'a>(ctx: Ctx<'a>, title: &'a str, phase: Option<String>) -> Element<'a, M> {
    let t = ctx.theme;
    // A bar that says something is on its way. It does not travel: nothing
    // asks for a frame while a file is read, which takes a moment.
    let bar = container(
        progress_bar(0.0..=1.0, 0.35)
            .girth(4.0)
            .style(style::progress_bar::accent(t)),
    )
    .width(Length::Fill);
    container(
        column![
            text(title)
                .size(tokens::font_size::BODY)
                .style(style::text::primary(t))
                .wrapping(text::Wrapping::None),
            bar,
            text(phase.unwrap_or_else(|| ctx.t(Key::PreviewLoading).into_owned()))
                .size(tokens::font_size::CAPTION)
                .style(style::text::muted(t)),
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .width(240.0),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center(Length::Fill)
    .into()
}

/// A mouse area that swallows nothing but gives a pointer, for a plain button
/// look: a label that does something.
pub fn clickable<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    on_press: M,
) -> Element<'a, M> {
    mouse_area(content)
        .on_press(on_press)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_lasts_longer_when_it_is_an_error() {
        assert!(Notice::error("no").lasts() > Notice::info("ok").lasts());
        assert_eq!(Notice::info("ok").lasts().as_millis(), 2200);
        assert_eq!(Notice::error("no").lasts().as_millis(), 6000);
    }
}
