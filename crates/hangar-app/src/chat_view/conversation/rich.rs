//! Claude's words as Markdown, drawn with iced's `markdown` the way the
//! preview draws a file (`preview::markdown`), sized for a chat bubble: the
//! same preparation before the parser (no raw HTML, bare addresses linked),
//! the same highlighter for the code blocks, coloured off the UI thread once
//! and only looked up here. An image is its alt text: the chat reads no file
//! and fetches nothing a message names.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use iced::widget::markdown::{self, HeadingLevel, Highlight, Item, Settings, Style, Uri};
use iced::widget::{column, container, rich_text, scrollable, span, text};
use iced::{Element, Font, Length, border, padding};

use hangar_theme::{Theme, fonts, tokens};

use crate::message::Message;
use crate::preview::highlight::{self, Lines};
use crate::preview::markdown::{CodeBlock, block_key, prepare};

/// The size of a message's text.
const TEXT: f32 = tokens::font_size::UI_LG;

/// A message's Markdown, parsed once.
pub fn parse(source: &str) -> markdown::Content {
    markdown::Content::parse(&prepare(source))
}

/// The code blocks of a message that name a language, each once: what
/// `preview::markdown::highlight_blocks` colours.
pub fn code_blocks(content: &markdown::Content) -> Vec<CodeBlock> {
    fn walk(items: &[Item], out: &mut Vec<CodeBlock>, seen: &mut HashSet<u64>) {
        for item in items {
            match item {
                Item::CodeBlock {
                    language: Some(language),
                    code,
                    ..
                } => {
                    let key = block_key(language, code);
                    if seen.insert(key) {
                        out.push(CodeBlock {
                            key,
                            language: language.clone(),
                            code: code.clone(),
                        });
                    }
                }
                Item::Quote(inner) => walk(inner, out, seen),
                Item::List { bullets, .. } => {
                    for bullet in bullets {
                        match bullet {
                            markdown::Bullet::Point { items }
                            | markdown::Bullet::Task { items, .. } => walk(items, out, seen),
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(content.items(), &mut out, &mut HashSet::new());
    out
}

fn settings(t: &Theme) -> Settings {
    let c = &t.colors;
    let mut settings = Settings::with_text_size(
        TEXT,
        Style {
            font: fonts::ui(),
            inline_code_highlight: Highlight {
                background: iced::Background::Color(c.surface_hover),
                border: border::rounded(tokens::radius::XS),
            },
            inline_code_padding: padding::left(3).right(3),
            inline_code_color: c.fg,
            inline_code_font: fonts::mono(),
            code_block_font: fonts::mono(),
            link_color: c.accent_hi,
        },
    );
    let size = |level: HeadingLevel| {
        TEXT * match level {
            HeadingLevel::H1 => 1.4,
            HeadingLevel::H2 => 1.25,
            HeadingLevel::H3 => 1.12,
            _ => 1.0,
        }
    };
    settings.h1_size = size(HeadingLevel::H1).into();
    settings.h2_size = size(HeadingLevel::H2).into();
    settings.h3_size = size(HeadingLevel::H3).into();
    settings.h4_size = TEXT.into();
    settings.h5_size = TEXT.into();
    settings.h6_size = TEXT.into();
    settings.code_size = (TEXT * 0.88).into();
    settings.spacing = (TEXT * 0.7).into();
    settings
}

struct Bubble<'a> {
    t: &'a Theme,
    code: &'a HashMap<u64, Arc<Lines>>,
}

fn link(url: Uri) -> Message {
    super::msg(super::Msg::Link(url))
}

impl<'a> markdown::Viewer<'a, Message, iced::Theme, iced::Renderer> for Bubble<'a> {
    fn on_link_click(url: Uri) -> Message {
        link(url)
    }

    fn image(
        &self,
        settings: Settings,
        _url: &'a Uri,
        _title: &'a str,
        alt: &markdown::Text,
    ) -> Element<'a, Message> {
        rich_text(alt.spans(settings.style))
            .size(settings.text_size)
            .on_link_click(link)
            .into()
    }

    fn code_block(
        &self,
        settings: Settings,
        language: Option<&'a str>,
        code: &'a str,
        lines: &'a [markdown::Text],
    ) -> Element<'a, Message> {
        let t = self.t;
        let coloured: Option<&'a Lines> = language
            .and_then(|language| self.code.get(&block_key(language, code)))
            .map(|lines| lines.as_ref());
        let mono = fonts::mono();
        let rows: Vec<Element<'a, Message>> = match coloured {
            Some(coloured) => coloured
                .iter()
                .map(|runs| {
                    let spans: Vec<text::Span<'a, Uri, Font>> = runs
                        .iter()
                        .map(|run| {
                            let part = span(run.text.as_str()).font(mono);
                            match highlight::colour(t, run.class) {
                                Some(colour) => part.color(colour),
                                None => part,
                            }
                        })
                        .collect();
                    rich_text(spans).size(settings.code_size).into()
                })
                .collect(),
            None => lines
                .iter()
                .map(|line| {
                    rich_text(line.spans(settings.style))
                        .font(mono)
                        .size(settings.code_size)
                        .color(t.colors.fg)
                        .on_link_click(link)
                        .into()
                })
                .collect(),
        };
        container(
            scrollable(container(column(rows)).padding(settings.code_size))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default()
                        .width(settings.code_size / 2.0)
                        .scroller_width(settings.code_size / 2.0),
                ))
                .style(hangar_theme::style::scrollable::default(t)),
        )
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(t.colors.bg_sunken)),
            border: iced::Border {
                color: t.colors.line,
                width: 1.0,
                radius: tokens::radius::MD.into(),
            },
            ..Default::default()
        })
        .into()
    }
}

/// A message's Markdown, drawn: its code blocks coloured once `code` has
/// their colours, plain until then.
pub fn view<'a>(
    t: &'a Theme,
    content: &'a markdown::Content,
    code: &'a HashMap<u64, Arc<Lines>>,
) -> Element<'a, Message> {
    markdown::view_with(content.items(), settings(t), &Bubble { t, code })
}
