//! The stage: the viewport, and over it what the state of the load has to say
//! (`ModelView`'s stage, `LoadingCard`, `ProgressBar`, `StageMessage`,
//! `Banner`).
//!
//! - Nothing loaded yet, or a model dropped from memory coming back: a card
//!   with the file's name, a bar and the phase, and a button to stop.
//! - A new version loading over the old one, which stays on screen: a thin bar
//!   at the top and a chip.
//! - A new version that could not be read: a banner with the reason and Retry;
//!   the previous version stays.
//! - A file that cannot be read any more: a banner saying this is the last
//!   version loaded.
//! - A model that could not be opened at all: a message with the reason (the
//!   crate's own sentence, which says what to export instead for a Draco, KTX2
//!   or USDZ file) and Retry.
//! - A model that is loaded and cannot be drawn (the window is drawn in
//!   software, the GPU could not be set up, refused the model, or was lost):
//!   a message over the blank viewport, with Restart where it can help.

use iced::widget::{Space, column, container, progress_bar, row, stack, text};
use iced::{Alignment, Background, Element, Length, Padding, alignment};

use hangar_i18n::{Key, args};
use hangar_model3d::{LoadProgress, Phase, Rendering};
use hangar_theme::{Theme, fonts, style, tokens};
use hangar_ui::{Icon, button};

use super::slot::Slot;
use super::theme::viewer_theme;
use super::{Msg, format};
use crate::ctx::Ctx;
use crate::message::Message;
use crate::preview::parts::{self, Feel, Mark};
use crate::preview::store::{Entry, Status};

/// Steps of the travelling bar, one for each turn of the preview's spinner.
const STEPS: usize = 8;
/// How much of the track the travelling bar covers.
const TRAVELLER: u16 = 34;

fn to(msg: Msg) -> Message {
    Message::Model3d(msg)
}

/// The viewport and its overlays for the entry in front.
///
/// `missing` is why the file can no longer be read, if the preview's watch
/// found it gone; `spin` is the preview's spinner step.
pub fn view<'a>(
    ctx: Ctx<'a>,
    entry: &'a Entry,
    slot: Option<&'a Slot>,
    missing: Option<&'a String>,
    spin: usize,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let id = entry.id.as_str();
    let content = slot.filter(|slot| slot.has_content());
    let has_content = content.is_some();
    let flight = slot.and_then(|slot| slot.flight.as_ref());
    let progress = flight.and_then(|flight| flight.progress);

    let backdrop = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.colors.bg)),
            ..Default::default()
        });
    // The viewport takes the mouse and the keys itself; it tells the app only
    // that the person took hold of the camera.
    let viewport: Element<'a, Message> = match content {
        Some(slot) => {
            let owner = id.to_string();
            slot.viewer
                .view(viewer_theme(t))
                .map(move |event| to(Msg::Viewer(owner.clone(), event)))
        }
        None => Space::new().into(),
    };
    let mut layers = stack![backdrop, viewport]
        .width(Length::Fill)
        .height(Length::Fill);

    // A new version loading under the old one, which stays on screen.
    if entry.status == Status::Loading && has_content {
        layers = layers.push(reloading(ctx, progress, spin));
    }

    // Something to know about the model on screen, without taking it away.
    let mut notes = column![].width(Length::Fill);
    // What the file holds that the preview could not use (a texture it cannot
    // decode, a KTX2 picture), in the crate's own words: the model opens
    // without it, and a grey model is otherwise a puzzle.
    if let Some(words) = content
        .and_then(|slot| slot.viewer.model())
        .and_then(|model| warnings_text(&model.warnings))
    {
        notes = notes.push(parts::note(t, words, false));
    }
    if entry.status == Status::Error && has_content {
        notes = notes.push(parts::banner(
            t,
            Feel::Danger,
            ctx.t(Key::ModelReloadFailed).into_owned(),
            entry.error.clone(),
            Some((
                ctx.t(Key::ModelRetry).into_owned(),
                to(Msg::Retry(id.to_string())),
            )),
        ));
    }
    if let (Some(why), true) = (missing, entry.status != Status::Error) {
        notes = notes.push(parts::banner(
            t,
            Feel::Warning,
            ctx.t(Key::ModelMissing).into_owned(),
            Some(why.clone()),
            None,
        ));
    }
    layers = layers.push(notes);

    // A viewport that cannot draw is blank: say why, over it.
    if let Some(slot) = content
        && let Some(message) = trouble(ctx, id, slot.viewer.rendering())
    {
        layers = layers.push(message);
    }

    // Nothing on screen yet: why.
    if !has_content {
        layers = if entry.status == Status::Error {
            layers.push(parts::stage_message(
                t,
                Mark::Icon(Icon::Alert),
                Feel::Danger,
                ctx.t(Key::ModelErrorTitle).into_owned(),
                entry.error.clone(),
                Some((
                    ctx.t(Key::ModelRetry).into_owned(),
                    to(Msg::Retry(id.to_string())),
                )),
            ))
        } else {
            layers.push(loading_card(
                ctx,
                &entry.title,
                progress,
                spin,
                flight
                    .is_some()
                    .then(|| to(Msg::CancelLoad(id.to_string()))),
            ))
        };
    }
    layers.into()
}

/// What to say over a viewport that cannot draw, if it cannot
/// (`StageMessage` for the engine's `failed` and `lost` states).
///
/// - Drawn by iced's software renderer: the 3D view cannot run there at all.
///   Nothing to try, so no button; the capture button and agents' pictures
///   still work, and the sentence says so.
/// - The GPU could not be set up: the device's own words, and Restart, which
///   sets it up again.
/// - The GPU refused this model: the device's words, and Restart.
/// - The device was lost (a reset, a driver crash): the card dropped the view,
///   and Restart.
fn trouble<'a>(ctx: Ctx<'a>, id: &str, rendering: Rendering) -> Option<Element<'a, Message>> {
    let t = ctx.theme;
    let restart = || {
        Some((
            ctx.t(Key::ModelRestart).into_owned(),
            to(Msg::Restart(id.to_string())),
        ))
    };
    // A reason that is empty (a refusal the device gave no words for) is not
    // a detail.
    let words = |reason: String| Some(reason).filter(|reason| !reason.trim().is_empty());
    let (feel, title, detail, action) = match rendering {
        Rendering::Starting | Rendering::Drawing => return None,
        Rendering::Software => (
            Feel::Warning,
            Key::ModelUnavailable,
            Some(ctx.t(Key::ModelSoftware).into_owned()),
            None,
        ),
        Rendering::Unavailable(reason) => (
            Feel::Danger,
            Key::ModelUnavailable,
            words(reason),
            restart(),
        ),
        Rendering::Failed(reason) => (Feel::Danger, Key::ModelDrawFailed, words(reason), restart()),
        Rendering::Lost(reason) => (Feel::Warning, Key::ModelLost, words(reason), restart()),
    };
    Some(parts::stage_message(
        t,
        Mark::Icon(Icon::Alert),
        feel,
        ctx.t(title).into_owned(),
        detail,
        action,
    ))
}

/// How many of the loader's warnings are written out; the rest are counted.
const WARNINGS_SHOWN: usize = 3;

/// The loader's warnings as the lines of a note: the first few, then how many
/// more there are. `None` when it had nothing to say.
pub fn warnings_text(warnings: &[String]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = warnings.iter().take(WARNINGS_SHOWN).cloned().collect();
    if warnings.len() > WARNINGS_SHOWN {
        lines.push(format!("… +{}", warnings.len() - WARNINGS_SHOWN));
    }
    Some(lines.join("\n"))
}

/// The thin bar and the chip over a model that is being replaced by a newer
/// version of its file.
fn reloading<'a>(
    ctx: Ctx<'a>,
    progress: Option<LoadProgress>,
    spin: usize,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let chip = container(
        text(ctx.t(Key::ModelReloading).into_owned())
            .size(tokens::font_size::CAPTION)
            .style(style::text::muted(t))
            .wrapping(text::Wrapping::None),
    )
    .padding([3.0, 8.0])
    .style(style::container::popover(t));
    column![
        bar(t, progress, spin, 2.0),
        container(chip).padding(tokens::space::LG),
    ]
    .width(Length::Fill)
    .into()
}

/// The file's name over a bar and the phase it is in, with a way to stop
/// (`LoadingCard`).
fn loading_card<'a>(
    ctx: Ctx<'a>,
    title: &'a str,
    progress: Option<LoadProgress>,
    spin: usize,
    on_cancel: Option<Message>,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let phase = match progress.map(|progress| progress.phase) {
        Some(Phase::Parse) => Key::ModelParse,
        Some(Phase::Prepare) => Key::ModelPrepare,
        _ => Key::ModelRead,
    };
    let mut words = row![
        text(ctx.t(phase).into_owned())
            .size(tokens::font_size::CAPTION)
            .style(style::text::muted(t))
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    if let Some(bytes) = bytes_line(ctx, progress) {
        words = words.push(
            text(bytes)
                .size(tokens::font_size::LABEL)
                .font(fonts::mono())
                .style(style::text::subtle(t)),
        );
    }
    let mut card = column![
        text(title)
            .size(tokens::font_size::UI)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t))
            .wrapping(text::Wrapping::None),
        bar(t, progress, spin, 3.0),
        words,
    ]
    .spacing(tokens::space::LG)
    .align_x(Alignment::Center)
    .width(240.0);
    if let Some(on_cancel) = on_cancel {
        card = card.push(
            button::ghost(t, ctx.t(Key::ModelCancel).into_owned())
                .small()
                .on_press(on_cancel),
        );
    }
    container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .padding(tokens::space::XXXL)
        .into()
}

/// "12 of 40 MB" while a file is read, or what has been read when the size is
/// not known.
fn bytes_line(ctx: Ctx<'_>, progress: Option<LoadProgress>) -> Option<String> {
    let progress = progress.filter(|p| p.phase == Phase::Read && p.loaded > 0)?;
    let loaded = megabytes(ctx, progress.loaded);
    if progress.total == 0 {
        return Some(loaded);
    }
    Some(
        ctx.i18n
            .t_with(
                Key::ModelDownloadBytes,
                &args!("loaded" => loaded, "total" => megabytes(ctx, progress.total)),
            )
            .into_owned(),
    )
}

/// A size as the card says it: "2.5 MB".
fn megabytes(ctx: Ctx<'_>, bytes: u64) -> String {
    let size = format::megabytes(bytes, ctx.i18n.locale());
    ctx.i18n
        .t_with(Key::ModelMegabytes, &args!("size" => size))
        .into_owned()
}

/// How far a read has got, as a ratio: only while a file of a known size is
/// being read (`determinate`).
pub fn read_ratio(progress: Option<LoadProgress>) -> Option<f32> {
    let progress = progress.filter(|p| p.phase == Phase::Read && p.total > 0)?;
    Some((progress.loaded as f64 / progress.total as f64).min(1.0) as f32)
}

/// A bar that fills as a file is read, and travels when there is no size to
/// measure against (`ProgressBar`).
fn bar<'a>(
    t: &'a Theme,
    progress: Option<LoadProgress>,
    spin: usize,
    girth: f32,
) -> Element<'a, Message> {
    match read_ratio(progress) {
        Some(ratio) => container(
            progress_bar(0.0..=1.0, ratio)
                .girth(girth)
                .style(style::progress_bar::accent(t)),
        )
        .width(Length::Fill)
        .into(),
        None => traveller(t, spin, girth),
    }
}

/// A short bar going along a track: the spinner's step says where.
fn traveller<'a>(t: &'a Theme, spin: usize, girth: f32) -> Element<'a, Message> {
    let before = ((spin % STEPS) as u16 * (100 - TRAVELLER)) / (STEPS as u16 - 1);
    let after = 100 - TRAVELLER - before;
    let piece = container(Space::new())
        .width(Length::FillPortion(TRAVELLER))
        .height(girth)
        .style(move |_| container::Style {
            background: Some(Background::Color(t.colors.accent)),
            border: iced::Border {
                radius: (girth / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        });
    container(
        row![
            Space::new().width(Length::FillPortion(before.max(1))),
            piece,
            Space::new().width(Length::FillPortion(after.max(1))),
        ]
        .height(girth),
    )
    .width(Length::Fill)
    .style(move |_| container::Style {
        background: Some(Background::Color(t.colors.line_strong)),
        border: iced::Border {
            radius: (girth / 2.0).into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .padding(Padding::ZERO)
    .align_y(alignment::Vertical::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_model3d::Phase;

    fn reading(loaded: u64, total: u64) -> Option<LoadProgress> {
        Some(LoadProgress {
            phase: Phase::Read,
            loaded,
            total,
        })
    }

    #[test]
    fn a_read_of_a_known_size_fills_the_bar() {
        assert_eq!(read_ratio(reading(0, 100)), Some(0.0));
        assert_eq!(read_ratio(reading(25, 100)), Some(0.25));
        assert_eq!(read_ratio(reading(120, 100)), Some(1.0));
    }

    #[test]
    fn a_model_with_nothing_to_warn_of_has_no_note() {
        assert_eq!(warnings_text(&[]), None);
    }

    #[test]
    fn the_first_warnings_are_written_out_and_the_rest_counted() {
        let some = |n: usize| -> Vec<String> { (1..=n).map(|i| format!("warning {i}")).collect() };
        assert_eq!(warnings_text(&some(1)).unwrap(), "warning 1");
        assert_eq!(
            warnings_text(&some(3)).unwrap(),
            "warning 1\nwarning 2\nwarning 3"
        );
        assert_eq!(
            warnings_text(&some(5)).unwrap(),
            "warning 1\nwarning 2\nwarning 3\n… +2"
        );
    }

    #[test]
    fn a_bar_with_nothing_to_measure_against_travels() {
        assert_eq!(read_ratio(None), None);
        assert_eq!(read_ratio(reading(10, 0)), None);
        let parsing = Some(LoadProgress {
            phase: Phase::Parse,
            loaded: 100,
            total: 100,
        });
        assert_eq!(read_ratio(parsing), None);
    }
}
