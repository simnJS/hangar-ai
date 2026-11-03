//! A PDF, a web page, a video or a sound: kinds only a browser engine shows,
//! so the panel holds their bar and a card, and the file itself is in a window
//! of the helper process (`PdfView.tsx`, `HtmlView.tsx`, `MediaView.tsx`;
//! webview.rs).
//!
//! What stays in the panel is what was native in the TS: the bar's tools, a
//! page's source (read by the app, coloured by the preview's own highlighter),
//! the note on what a page cannot do. What the web view of the old app did
//! inside the panel is a button here: bring the window forward, open it
//! again, reload it.

use iced::widget::{Space, column, container, row, text};
use iced::{Alignment, Element, Length};

use hangar_i18n::Key;
use hangar_theme::{style, tokens};
use hangar_ui::{Icon, icon, input};

use super::glyph::{Glyph, glyph};
use super::model::{FileKind, Kind};
use super::parts::{self, Feel, Mark, Notice};
use super::store::Entry;
use super::webview::{Win, WinState};
use crate::ctx::Ctx;

/// What the person asked of a window.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Opens it, or again after it was closed.
    Open,
    /// Raises it.
    Focus,
    /// Loads the page again.
    Reload,
    /// Hands the file to the app the system opens it with (a PDF).
    OpenExternally,
    /// Shows the file selected in the system's file manager.
    ShowInFolder,
    Loop(bool),
    Speed(f32),
    /// A page's source instead of its window (the tool of the bar).
    ToggleSource,
}

/// From half speed to double: past that, speech is noise and frames are
/// skipped (`SPEEDS`).
pub const SPEEDS: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

#[derive(Debug, Clone, PartialEq)]
struct Speed {
    value: f32,
    label: String,
}

impl std::fmt::Display for Speed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A speed written the way the language writes a number, with the times sign:
/// "0.75×", "0,75×".
pub fn speed_label(locale: hangar_i18n::Locale, value: f32) -> String {
    format!("{}×", hangar_i18n::Number::from(value).format(locale))
}

/// What the bar of a kind holds.
pub struct Bar<'a> {
    pub kind: FileKind,
    pub entry: &'a Entry,
    pub notice: Option<&'a Notice>,
    /// A page or an SVG shown as its source.
    pub source: bool,
    /// The wrap tool is pressed, for a source.
    pub wrap: bool,
    pub looping: bool,
    pub speed: f32,
    pub win: Option<&'a Win>,
}

/// The bar of a kind (`model-panel__bar` of each viewer).
pub fn bar<'a, M: Clone + 'a>(
    ctx: Ctx<'a>,
    bar: &Bar<'a>,
    act: impl Fn(Action) -> M + Copy + 'a,
    wrap: M,
    copy: Option<M>,
) -> Element<'a, M> {
    let t = ctx.theme;
    let mut tools: Vec<Element<'a, M>> = Vec::new();
    let mut aside: Vec<Element<'a, M>> = Vec::new();
    match bar.kind {
        FileKind::Pdf => {
            tools.push(
                hangar_ui::button::secondary(t, ctx.t(Key::PreviewOpenDefault).into_owned())
                    .icon(Icon::ArrowUpRight)
                    .small()
                    .on_press(act(Action::OpenExternally))
                    .into(),
            );
        }
        FileKind::Html => {
            tools.push(parts::icon_tool(
                t,
                Icon::Restart,
                ctx.t(Key::PreviewReload),
                None,
                Some(act(Action::Reload)),
            ));
            tools.push(parts::glyph_tool(
                t,
                Glyph::Code,
                ctx.t(Key::PreviewSourceHint),
                Some(bar.source),
                Some(act(Action::ToggleSource)),
            ));
            if bar.source {
                tools.push(parts::glyph_tool(
                    t,
                    Glyph::Wrap,
                    ctx.t(Key::PreviewWrap),
                    Some(bar.wrap),
                    Some(wrap),
                ));
                tools.push(parts::icon_tool(
                    t,
                    Icon::Copy,
                    ctx.t(Key::PreviewCopy),
                    None,
                    copy,
                ));
            }
            aside.push(
                container(
                    row![
                        icon(Icon::Lock, 12.0).color(t.colors.accent_hi),
                        text(ctx.t(Key::PreviewIsolated).into_owned())
                            .size(tokens::font_size::CAPTION)
                            .color(t.colors.accent_hi),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([2.0, 8.0])
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(t.colors.accent_soft)),
                    border: iced::Border {
                        radius: tokens::radius::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
            );
        }
        FileKind::Video | FileKind::Audio => {
            let failed = !matches!(
                bar.win.map(|win| &win.state),
                Some(WinState::Open | WinState::Opening | WinState::Waiting)
            );
            tools.push(parts::glyph_tool(
                t,
                Glyph::Loop,
                ctx.t(Key::PreviewLoop),
                Some(bar.looping),
                (!failed).then(|| act(Action::Loop(!bar.looping))),
            ));
            let locale = ctx.model.locale();
            let options: Vec<Speed> = SPEEDS
                .iter()
                .map(|value| Speed {
                    value: *value,
                    label: speed_label(locale, *value),
                })
                .collect();
            let chosen = options.iter().find(|s| s.value == bar.speed).cloned();
            let picker: Element<'a, M> = if failed {
                Space::new().into()
            } else {
                container(input::select(t, options, chosen, move |s: Speed| {
                    act(Action::Speed(s.value))
                }))
                .width(76.0)
                .into()
            };
            tools.push(hangar_ui::tip::bottom(t, picker, ctx.t(Key::PreviewSpeed)));
        }
        _ => {}
    }
    if let Some(notice) = bar.notice {
        aside.insert(0, parts::notice_text(t, notice));
    }
    parts::bar(t, tools, aside)
}

/// The card where the page would have been, said by the state of its window.
pub fn card<'a, M: Clone + 'a>(
    ctx: Ctx<'a>,
    entry: &'a Entry,
    kind: Kind,
    win: Option<&'a Win>,
    act: impl Fn(Action) -> M + Copy + 'a,
) -> Element<'a, M> {
    let t = ctx.theme;
    let open = || {
        Some((
            ctx.t(Key::PreviewWindowOpen).into_owned(),
            act(Action::Open),
        ))
    };
    match win.map(|win| &win.state) {
        None => shown_apart(ctx, entry, kind, open()),
        Some(WinState::Waiting | WinState::Opening) => parts::loading(
            ctx,
            &entry.title,
            Some(ctx.t(Key::PreviewWindowStarting).into_owned()),
        ),
        // The engine could not play it (no codec): the TS said so in place of
        // the player, with a way to find the file.
        Some(WinState::Open) if win.is_some_and(|win| win.media_error.is_some()) => {
            let reason = win
                .and_then(|win| win.media_error.clone())
                .unwrap_or_default();
            parts::stage_message(
                t,
                mark_of(kind),
                Feel::Warning,
                ctx.t(Key::PreviewMediaFailed),
                Some(if reason.is_empty() {
                    ctx.t(Key::PreviewMediaFailedHint).into_owned()
                } else {
                    format!("{} ({reason})", ctx.t(Key::PreviewMediaFailedHint))
                }),
                Some((
                    ctx.t(Key::PreviewShowInFolder).into_owned(),
                    act(Action::ShowInFolder),
                )),
            )
        }
        Some(WinState::Open) => shown_apart(
            ctx,
            entry,
            kind,
            Some((
                ctx.t(Key::PreviewWindowFocus).into_owned(),
                act(Action::Focus),
            )),
        ),
        Some(WinState::Closed) => parts::stage_message(
            t,
            mark_of(kind),
            Feel::Calm,
            ctx.t(Key::PreviewWindowClosed),
            None,
            open(),
        ),
        Some(WinState::Missing) => parts::stage_message(
            t,
            Mark::Icon(Icon::Alert),
            Feel::Warning,
            ctx.t(Key::PreviewWindowMissing),
            Some(ctx.t(Key::PreviewWindowMissingHint).into_owned()),
            matches!(kind, Kind::Pdf)
                .then(|| {
                    (
                        ctx.t(Key::PreviewOpenDefault).into_owned(),
                        act(Action::OpenExternally),
                    )
                })
                .or_else(|| {
                    Some((
                        ctx.t(Key::PreviewShowInFolder).into_owned(),
                        act(Action::ShowInFolder),
                    ))
                }),
        ),
        Some(WinState::Failed(why)) => parts::stage_message(
            t,
            Mark::Icon(Icon::Alert),
            Feel::Danger,
            ctx.t(Key::PreviewWindowFailed),
            Some(why.clone()),
            open(),
        ),
        Some(WinState::NoPdfViewer) => parts::stage_message(
            t,
            Mark::Glyph(Glyph::Pdf),
            Feel::Calm,
            ctx.t(Key::PreviewPdfUnsupported),
            Some(ctx.t(Key::PreviewPdfUnsupportedHint).into_owned()),
            Some((
                ctx.t(Key::PreviewOpenDefault).into_owned(),
                act(Action::OpenExternally),
            )),
        ),
    }
}

fn mark_of(kind: Kind) -> Mark {
    super::glyph::of(kind).map_or(Mark::Icon(Icon::Cube), Mark::Glyph)
}

/// "Shown in its own window", with what to do about it. A sound shows its
/// title above, as its player did (`.preview-audio`).
fn shown_apart<'a, M: Clone + 'a>(
    ctx: Ctx<'a>,
    entry: &'a Entry,
    kind: Kind,
    action: Option<(String, M)>,
) -> Element<'a, M> {
    let t = ctx.theme;
    let mut body = column![
        mark_view(t, kind),
        text(entry.title.as_str())
            .size(tokens::font_size::BODY)
            .font(hangar_theme::fonts::ui_semibold())
            .style(style::text::primary(t))
            .wrapping(text::Wrapping::None),
        text(ctx.t(Key::PreviewWindowTitle).into_owned())
            .size(tokens::font_size::CAPTION_LG)
            .style(style::text::secondary(t)),
        text(ctx.t(Key::PreviewWindowHint).into_owned())
            .size(tokens::font_size::CAPTION)
            .style(style::text::muted(t))
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .spacing(tokens::space::MD)
    .align_x(Alignment::Center);
    if let Some((label, on_press)) = action {
        body = body.push(hangar_ui::button::secondary(t, label).on_press(on_press));
    }
    container(container(body).max_width(360.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(tokens::space::XXXL)
        .into()
}

fn mark_view<'a, M: 'a>(t: &'a hangar_theme::Theme, kind: Kind) -> Element<'a, M> {
    match mark_of(kind) {
        Mark::Glyph(which) => glyph(which, 40.0, t.colors.fg_subtle),
        Mark::Icon(which) => icon(which, 40.0).color(t.colors.fg_subtle).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    #[test]
    fn speeds_are_written_the_way_each_language_writes_a_number() {
        assert_eq!(speed_label(Locale::En, 0.75), "0.75×");
        assert_eq!(speed_label(Locale::Fr, 0.75), "0,75×");
        assert_eq!(speed_label(Locale::En, 1.0), "1×");
        assert_eq!(speed_label(Locale::Fr, 1.5), "1,5×");
    }

    #[test]
    fn the_speeds_run_from_half_to_double_and_include_normal() {
        assert_eq!(SPEEDS.first(), Some(&0.5));
        assert_eq!(SPEEDS.last(), Some(&2.0));
        assert!(SPEEDS.contains(&1.0));
        assert!(SPEEDS.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_speed_in_the_list_reads_as_its_label() {
        let speed = Speed {
            value: 1.25,
            label: speed_label(Locale::En, 1.25),
        };
        assert_eq!(speed.to_string(), "1.25×");
    }
}
