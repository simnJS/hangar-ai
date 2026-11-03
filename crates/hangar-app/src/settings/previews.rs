//! The settings' own pictures: the theme cards, the font cards, the terminal
//! preview and the projects folder field (the `ThemeGrid`, `FontGrid`,
//! `TerminalPreview` and picker of `schema.tsx`).
//!
//! A swatch alone says which colours a palette holds, not what working in it
//! looks like, so a theme is drawn as the window it would give: a navigator
//! column, a few lines of terminal output in the theme's own colours, and its
//! accent. These are the one place a view takes colours from a theme other
//! than the one the window wears, which is the point of a preview.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use hangar_i18n::Key;
use iced::font::Family;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding};

use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Appearance, Theme, fonts, style};
use hangar_ui::{button as buttons, input, text as typography};

use super::Msg;
use super::change::{Change, Target};
use super::schema::FONT_STACKS;
use crate::ctx::Ctx;
use crate::message::Message;
use crate::state::types::CursorStyle;

fn change(target: Target, value: &str) -> Message {
    Message::Settings(Msg::Change(Change::text(target, value)))
}

// ---- Themes -----------------------------------------------------------------------------------------

/// How many cards fit a row of the page.
const THEMES_PER_ROW: usize = 5;
const CARD_WIDTH: f32 = 116.0;
/// 16:10, as the cards of the web app.
const CARD_HEIGHT: f32 = CARD_WIDTH * 10.0 / 16.0;

/// A bar of the preview: `width` of the area it sits in, 3 high.
fn bar<'a>(color: Color, width: f32) -> Element<'a, Message> {
    container(Space::new())
        .width(width)
        .height(3)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 2.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A theme drawn as the window it would give.
fn theme_preview<'a>(theme: &'static Theme) -> Element<'a, Message> {
    let c = &theme.colors;
    let dim = c.fg_subtle;
    let nav_width = CARD_WIDTH * 0.30;
    let nav_inner = nav_width - 10.0;
    let lines_inner = CARD_WIDTH - nav_width - 14.0;
    let ansi = &theme.terminal.ansi;
    let fainter = Color {
        a: dim.a * 0.6,
        ..dim
    };

    let nav = container(
        column![
            bar(c.accent, nav_inner * 0.55),
            bar(fainter, nav_inner * 0.85),
            bar(fainter, nav_inner * 0.70),
        ]
        .spacing(4),
    )
    .width(nav_width)
    .height(Length::Fill)
    .padding(Padding::from([8.0, 5.0]))
    .style(move |_: &iced::Theme| container::Style {
        background: Some(Background::Color(c.chrome)),
        ..container::Style::default()
    });

    let lines = container(
        column![
            bar(ansi[4], lines_inner * 0.44),
            bar(dim, lines_inner * 0.70),
            bar(ansi[2], lines_inner * 0.36),
            bar(dim, lines_inner * 0.58),
            bar(ansi[3], lines_inner * 0.26),
        ]
        .spacing(5),
    )
    .width(Length::Fill)
    .padding(Padding::from([8.0, 7.0]));

    container(row![nav, lines])
        .width(CARD_WIDTH)
        .height(CARD_HEIGHT)
        .clip(true)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(c.bg)),
            border: Border {
                radius: radius::MD.into(),
                width: 1.0,
                color: c.line_strong,
            },
            ..container::Style::default()
        })
        .into()
}

/// One card: the picture, and the name under it.
fn theme_card<'a>(ctx: Ctx<'a>, theme: &'static Theme, active: bool) -> Element<'a, Message> {
    let t = ctx.theme;
    let ring = if active {
        t.colors.accent
    } else {
        Color::TRANSPARENT
    };
    // The ring of the chosen card stands off the picture by two pixels.
    let picture = container(theme_preview(theme))
        .padding(2)
        .style(move |_: &iced::Theme| container::Style {
            border: Border {
                radius: (radius::MD + 2.0).into(),
                width: 2.0,
                color: ring,
            },
            ..container::Style::default()
        });
    let name = text(theme.name)
        .size(font_size::CAPTION_LG)
        .color(if active {
            t.colors.fg
        } else {
            t.colors.fg_muted
        });
    let name = if active {
        name.font(fonts::ui_semibold())
    } else {
        name
    };
    button(
        column![picture, container(name).center_x(Length::Fill)]
            .spacing(space::SM)
            .width(CARD_WIDTH + 4.0),
    )
    .padding(0)
    .style(style::button::plain(t))
    .on_press(change(Target::Theme, theme.id))
    .into()
}

/// Dark and light palettes are listed apart: the two rarely interest the same
/// person.
pub fn theme_grid<'a>(ctx: Ctx<'a>, active: &str) -> Element<'a, Message> {
    let t = ctx.theme;
    let mut groups = column![].spacing(18);
    for (appearance, label) in [
        (Appearance::Dark, Key::SettingsThemeDark),
        (Appearance::Light, Key::SettingsThemeLight),
    ] {
        let themes: Vec<&'static Theme> = hangar_theme::themes()
            .iter()
            .filter(|theme| theme.appearance == appearance)
            .collect();
        if themes.is_empty() {
            continue;
        }
        let mut grid = column![].spacing(space::XL + 2.0);
        for chunk in themes.chunks(THEMES_PER_ROW) {
            grid = grid.push(
                row(chunk
                    .iter()
                    .map(|theme| theme_card(ctx, theme, theme.id == active)))
                .spacing(space::XL),
            );
        }
        groups = groups.push(
            column![
                typography::heading(t, ctx.t(label).into_owned()).size(font_size::CAPTION),
                grid
            ]
            .spacing(space::LG),
        );
    }
    groups.into()
}

// ---- Fonts -------------------------------------------------------------------------------------------

/// A `'static` copy of a family name, which `iced::Font` wants. Each distinct
/// name is leaked once; the set of fonts a person tries out is small, so this
/// stays bounded (hangar-term does the same for its own).
fn intern(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().expect("names");
    if let Some(known) = names.iter().find(|known| **known == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.push(leaked);
    leaked
}

/// Whether the font system has a family called `name`.
fn installed(name: &str) -> bool {
    let mut system = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system lock");
    system.raw().db().faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.eq_ignore_ascii_case(name))
    })
}

/// The font a CSS stack gives: the first of its families the machine has, else
/// the system's monospace one, which is how the terminal itself resolves it
/// (`hangar_term::Font`). Asked of the font system once per stack, since
/// listing its faces is not free and a page draws several cards.
pub fn font_of(stack: &str) -> Font {
    static CACHE: OnceLock<Mutex<HashMap<String, Font>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(font) = cache.lock().expect("fonts").get(stack) {
        return *font;
    }
    let families = hangar_term::Font::from_css_stack(stack, 13.0, 1.3).families;
    let family = families
        .iter()
        .find(|name| installed(name))
        .map_or(Family::Monospace, |name| Family::Name(intern(name)));
    let font = Font {
        family,
        ..Font::DEFAULT
    };
    cache.lock().expect("fonts").insert(stack.to_string(), font);
    font
}

/// The cards, plus one for a stack that is none of them.
pub fn font_cards(current: &str, custom_label: &str) -> Vec<(String, String)> {
    let mut cards: Vec<(String, String)> = FONT_STACKS
        .iter()
        .map(|(name, stack)| (name.to_string(), stack.to_string()))
        .collect();
    if !FONT_STACKS.iter().any(|(_, stack)| *stack == current) {
        cards.push((custom_label.to_string(), current.to_string()));
    }
    cards
}

pub fn font_grid<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let current = ctx.model.settings().font_family.clone();
    let cards = font_cards(&current, &ctx.t(Key::SettingsFontCustom));
    let (accent, line, bg) = (c.accent, c.line_strong, c.bg);
    let mut grid = column![].spacing(space::LG);
    for chunk in cards.chunks(3) {
        let mut line_row = row![].spacing(space::LG);
        for (name, stack) in chunk {
            let active = *stack == current;
            let label = text(name.clone())
                .size(font_size::CAPTION)
                .color(if active { c.accent_hi } else { c.fg_secondary });
            let sample = text("Aa Bb 0123 => != ->")
                .size(font_size::BODY)
                .font(font_of(stack));
            let face = container(column![label, sample].spacing(3))
                .padding(Padding::from([space::LG, space::XL - 2.0]))
                .width(Length::Fill)
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: radius::MD.into(),
                        width: if active { 2.0 } else { 1.0 },
                        color: if active { accent } else { line },
                    },
                    ..container::Style::default()
                });
            line_row = line_row.push(
                button(face)
                    .padding(0)
                    .width(Length::FillPortion(1))
                    .style(style::button::plain(t))
                    .on_press(change(Target::FontFamily, stack)),
            );
        }
        // A short last row keeps its cards the width of the others.
        for _ in chunk.len()..3 {
            line_row = line_row.push(Space::new().width(Length::FillPortion(1)));
        }
        grid = grid.push(line_row);
    }
    grid.into()
}

// ---- The terminal -------------------------------------------------------------------------------------

/// Typography, cursor and padding, shown on the live theme.
pub fn terminal_preview<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let c = &t.colors;
    let settings = ctx.model.settings();
    let font = font_of(&settings.font_family);
    let size = settings.font_size as f32;
    let line = iced::widget::text::LineHeight::Relative(settings.line_height as f32);
    // A cell of a monospaced face is about 0.6 of its size wide.
    let cell = size * 0.6;
    let tall = size * 1.1;
    let accent = c.accent;

    let cursor = {
        let (width, height) = match settings.cursor_style {
            CursorStyle::Bar => (2.0, tall),
            CursorStyle::Block => (cell, tall),
            CursorStyle::Underline => (cell, 2.0),
        };
        container(Space::new())
            .width(width)
            .height(height)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(accent)),
                ..container::Style::default()
            })
    };
    let prompt = row![
        text("❯")
            .size(size)
            .font(font)
            .line_height(line)
            .color(c.success),
        text(" claude --resume")
            .size(size)
            .font(font)
            .line_height(line)
            .color(c.fg),
    ];
    let dim = text("Hangar.AI · 4 panes · session restored")
        .size(size)
        .font(font)
        .line_height(line)
        .color(c.fg_subtle);
    let last = row![
        text("0123456789 => != -> |&")
            .size(size)
            .font(font)
            .line_height(line)
            .color(c.fg),
        container(cursor).padding(Padding {
            left: 1.0,
            top: size * 0.15,
            ..Padding::ZERO
        }),
    ]
    .align_y(Alignment::Start);

    let body = column![prompt, dim, last];
    let border = c.line_strong;
    let bg = c.bg;
    container(body)
        .padding(settings.padding as f32)
        .width(Length::Fill)
        .clip(true)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: radius::LG.into(),
                width: 1.0,
                color: border,
            },
            ..container::Style::default()
        })
        .into()
}

// ---- The projects folder ------------------------------------------------------------------------------

/// The path field and its Browse button (`.picker`). The path is stored raw
/// on each keystroke.
pub fn projects_folder<'a>(ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    let path = ctx
        .model
        .settings()
        .projects_folder
        .as_deref()
        .unwrap_or("");
    let placeholder = match ctx.i18n.t(Key::SettingsProjectsFolderPlaceholder) {
        std::borrow::Cow::Borrowed(words) => words,
        std::borrow::Cow::Owned(_) => "",
    };
    row![
        crate::keymap::guard(
            ctx,
            input::text_field(t, path)
                .placeholder(placeholder)
                .mono()
                .on_input(|text| change(Target::ProjectsFolder, &text)),
        ),
        buttons::secondary(t, ctx.t(Key::CreateBrowse).into_owned())
            .on_press(Message::Settings(Msg::BrowseProjects)),
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center)
    .into()
}

/// Where the Browse dialog opens: the folder already chosen, else the home.
pub fn browse_start(current: Option<&str>) -> Option<std::path::PathBuf> {
    current
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_dir())
        .or_else(dirs_home)
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_dir())
}

#[cfg(test)]
mod tests;
