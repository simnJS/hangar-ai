//! An image, or an SVG's source on demand (`components/preview/ImageView.tsx`).
//!
//! The image is fitted to the panel, then zoomed by the wheel, a trackpad's or
//! a screen's pinch, the bar or the keyboard, and moved by dragging; 1:1 shows
//! its own pixels. A checkerboard behind it shows what is transparent. While
//! fitted it follows the panel's size; once the person zooms or moves it, it
//! keeps their framing.
//!
//! An animated GIF or WebP plays: `decode.rs` makes its frames off the UI
//! thread and the stage shows the one the clock says (`image_stage.rs`). An
//! AVIF is decoded there too, to pixels.

use std::path::Path;

use iced::widget::{container, text};
use iced::{Element, Length};

use hangar_i18n::{Key, Number, args};
use hangar_theme::{style, tokens};
use hangar_ui::Icon;

use super::decode;
pub use super::decode::{Bytes, Loaded};
use super::glyph::Glyph;
use super::image_stage::{Gesture, Measured, NUDGE, Pic, Stage, StageKey};
use super::model::{self, FIT_MARGIN, ImageFrame, Size, ZOOM_STEP, fit_zoom, zoom_around};
use super::parts::{self, Feel, Mark};
use crate::ctx::Ctx;

/// What the image viewer is told.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The source button of an SVG: the viewer's to answer.
    ToggleSource,
    Measured(Measured),
    Stage(Size),
    Gesture(Gesture),
    ZoomOut,
    ZoomIn,
    Fit,
    ActualSize,
}

/// Reads an image file whole, and decodes what the renderer cannot: the
/// frames of an animation, an AVIF. Its handle is made from the bytes, not
/// from the path: the renderer keys what it decoded on the handle, and a path
/// that is rewritten must not come back from its cache.
pub fn read(path: &Path) -> Result<Loaded, String> {
    let bytes = decode::read_file(path)?;
    // An SVG is drawn from its source by the stage, whatever its bytes say.
    if model::has_source(&path.to_string_lossy()) {
        check_svg(&bytes).map_err(|why| format!("{}: {why}", path.display()))?;
        return Ok(Loaded::Encoded(Bytes(std::sync::Arc::new(bytes))));
    }
    decode::load(bytes).map_err(|why| format!("{}: {why}", path.display()))
}

/// Whether an SVG can be drawn. The renderer reports a size of nothing for a
/// source it cannot read, and for one that has no size of its own (which is
/// drawn as 512 square), so a broken one would be an empty stage and never
/// the card that says the image failed. A source without a size is fine.
pub fn check_svg(source: &[u8]) -> Result<(), String> {
    match usvg::Tree::from_data(source, &usvg::Options::default()) {
        Ok(_) | Err(usvg::Error::InvalidSize) => Ok(()),
        Err(why) => Err(why.to_string()),
    }
}

/// The picture for what was read: vector for an SVG, raster for the rest.
pub fn pic_of(path: &str, loaded: &Loaded) -> Pic {
    match loaded {
        Loaded::Encoded(bytes) if model::has_source(path) => {
            Pic::Vector(iced::widget::svg::Handle::from_memory(bytes.0.to_vec()))
        }
        Loaded::Encoded(bytes) => {
            Pic::Raster(iced::widget::image::Handle::from_bytes(bytes.0.to_vec()))
        }
        Loaded::Pixels(handle, ..) => Pic::Raster(handle.clone()),
        Loaded::Animated(animation) => Pic::Animated(animation.clone()),
    }
}

#[derive(Debug, Default)]
pub struct Picture {
    pub pic: Option<Pic>,
    /// Why it could not be read or drawn.
    pub failed: Option<String>,
    /// Its own size, once the renderer has it.
    pub natural: Option<Size>,
    /// The stage's size, as the stage last said.
    pub stage: Size,
    /// The person's own framing; `None` while it is fitted to the panel.
    pub chosen: Option<ImageFrame>,
}

impl Picture {
    /// Fitted to the stage with a margin, centred.
    pub fn fitted(&self) -> ImageFrame {
        let zoom = match self.natural {
            Some(natural) if self.stage.width > 0.0 => fit_zoom(natural, self.stage, FIT_MARGIN),
            _ => 1.0,
        };
        ImageFrame {
            zoom,
            x: 0.0,
            y: 0.0,
        }
    }

    /// Where the image sits now.
    pub fn frame(&self) -> ImageFrame {
        self.chosen.unwrap_or_else(|| self.fitted())
    }

    fn change(&mut self, change: impl FnOnce(ImageFrame) -> ImageFrame) {
        self.chosen = Some(change(self.frame()));
    }

    /// Exactly 1, not what `1 / zoom × zoom` rounds to.
    fn actual_size(&mut self, point: (f32, f32)) {
        self.change(|now| ImageFrame {
            zoom: 1.0,
            ..zoom_around(now, 1.0 / now.zoom, point)
        });
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::ToggleSource => {}
            Msg::Measured(Measured::Size(size)) => self.natural = Some(size),
            Msg::Measured(Measured::Failed) => {
                self.failed = Some(String::new());
            }
            Msg::Stage(size) => self.stage = size,
            Msg::ZoomOut => self.change(|now| zoom_around(now, 1.0 / ZOOM_STEP, (0.0, 0.0))),
            Msg::ZoomIn => self.change(|now| zoom_around(now, ZOOM_STEP, (0.0, 0.0))),
            Msg::Fit => self.chosen = None,
            Msg::ActualSize => self.actual_size((0.0, 0.0)),
            Msg::Gesture(gesture) => match gesture {
                Gesture::Zoom { factor, at } => self.change(|now| zoom_around(now, factor, at)),
                Gesture::Pan { dx, dy } => self.change(|now| ImageFrame {
                    x: now.x + dx,
                    y: now.y + dy,
                    ..now
                }),
                Gesture::Pinch { factor, from, to } => self.change(|now| {
                    let zoomed = zoom_around(now, factor, from);
                    ImageFrame {
                        x: zoomed.x + to.0 - from.0,
                        y: zoomed.y + to.1 - from.1,
                        ..zoomed
                    }
                }),
                Gesture::Double { at } => {
                    if self.chosen.is_some() {
                        self.chosen = None;
                    } else {
                        self.actual_size(at);
                    }
                }
                Gesture::Key(StageKey::Nudge { dx, dy }) => self.change(|now| ImageFrame {
                    x: now.x + f32::from(dx) * NUDGE,
                    y: now.y + f32::from(dy) * NUDGE,
                    ..now
                }),
                Gesture::Key(StageKey::ZoomIn) => self.update(Msg::ZoomIn),
                Gesture::Key(StageKey::ZoomOut) => self.update(Msg::ZoomOut),
                Gesture::Key(StageKey::Fit) => self.update(Msg::Fit),
                Gesture::Key(StageKey::ActualSize) => self.update(Msg::ActualSize),
            },
        }
    }
}

/// The percent a zoom is shown as: "50%" in English, "50 %" with a narrow
/// no-break space in French (`Intl.NumberFormat` percent, no decimals).
pub fn percent(locale: hangar_i18n::Locale, zoom: f32) -> String {
    let number = Number::from((zoom * 100.0).round() as i64).format(locale);
    match locale {
        hangar_i18n::Locale::En => format!("{number}%"),
        hangar_i18n::Locale::Fr => format!("{number}\u{202f}%"),
    }
}

/// The bar and the stage. `toggle` is the source button an SVG has, with the
/// hairline after it.
pub fn view<'a>(
    ctx: Ctx<'a>,
    picture: &'a Picture,
    path: &'a str,
    stage_id: iced::widget::Id,
    toggle: Option<Element<'a, Msg>>,
) -> Element<'a, Msg> {
    let t = ctx.theme;
    let frame = picture.frame();
    let ready = picture.natural.is_some();
    let mut tools: Vec<Element<'a, Msg>> = Vec::new();
    if let Some(toggle) = toggle {
        tools.push(toggle);
        tools.push(parts::separator(t));
    }
    tools.push(parts::icon_tool(
        t,
        Icon::Minus,
        ctx.t(Key::PreviewZoomOut),
        None,
        ready.then_some(Msg::ZoomOut),
    ));
    tools.push(hangar_ui::tip::bottom(
        t,
        container(
            text(percent(ctx.model.locale(), frame.zoom))
                .size(tokens::font_size::LABEL)
                .font(hangar_theme::fonts::mono())
                .style(style::text::muted(t)),
        )
        .width(48.0)
        .center_x(48.0),
        ctx.t(Key::PreviewZoom),
    ));
    tools.push(parts::icon_tool(
        t,
        Icon::Plus,
        ctx.t(Key::PreviewZoomIn),
        None,
        ready.then_some(Msg::ZoomIn),
    ));
    tools.push(parts::separator(t));
    tools.push(parts::icon_tool(
        t,
        Icon::Frame,
        ctx.t(Key::PreviewFit),
        Some(picture.chosen.is_none()),
        ready.then_some(Msg::Fit),
    ));
    tools.push(parts::tool(
        t,
        text("1:1")
            .size(tokens::font_size::MICRO)
            .font(hangar_theme::fonts::mono())
            .color(if frame.zoom == 1.0 {
                t.colors.accent_hi
            } else {
                t.colors.fg_muted
            }),
        ctx.t(Key::PreviewActualSize),
        Some(frame.zoom == 1.0),
        ready.then_some(Msg::ActualSize),
    ));
    let mut aside: Vec<Element<'a, Msg>> = Vec::new();
    if let Some(natural) = picture.natural {
        aside.push(parts::chip(
            t,
            ctx.i18n
                .t_with(
                    Key::PreviewPixels,
                    &args!("width" => natural.width as u64, "height" => natural.height as u64),
                )
                .into_owned(),
            true,
        ));
    }

    let stage: Element<'a, Msg> = if picture.failed.is_some() {
        parts::stage_message(
            t,
            Mark::Glyph(Glyph::Image),
            Feel::Danger,
            ctx.t(Key::PreviewImageFailed),
            Some(path.to_string()),
            None,
        )
    } else if let Some(pic) = &picture.pic {
        Stage::new(
            pic,
            frame,
            picture.natural,
            t,
            Msg::Measured,
            Msg::Stage,
            Msg::Gesture,
        )
        .id(stage_id)
        .into()
    } else {
        parts::loading(ctx, path, None)
    };
    iced::widget::column![parts::bar(t, tools, aside), stage]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_i18n::Locale;

    fn picture(natural: (f32, f32), stage: (f32, f32)) -> Picture {
        Picture {
            natural: Some(Size::new(natural.0, natural.1)),
            stage: Size::new(stage.0, stage.1),
            ..Picture::default()
        }
    }

    #[test]
    fn follows_the_panel_while_fitted() {
        let mut shown = picture((2000.0, 1000.0), (1032.0, 800.0));
        assert_eq!(shown.frame().zoom, 0.5);
        shown.update(Msg::Stage(Size::new(532.0, 800.0)));
        assert_eq!(shown.frame().zoom, 0.25);
        assert!(shown.chosen.is_none());
    }

    #[test]
    fn keeps_the_persons_own_framing_once_they_zoom_or_move_it() {
        let mut shown = picture((2000.0, 1000.0), (1032.0, 800.0));
        shown.update(Msg::Gesture(Gesture::Pan { dx: 10.0, dy: -5.0 }));
        let kept = shown.frame();
        assert_eq!((kept.x, kept.y, kept.zoom), (10.0, -5.0, 0.5));
        shown.update(Msg::Stage(Size::new(300.0, 300.0)));
        assert_eq!(shown.frame(), kept);
        shown.update(Msg::Fit);
        assert!(shown.chosen.is_none());
    }

    #[test]
    fn zooms_by_a_step_and_the_wheel_around_the_pointer() {
        let mut shown = picture((100.0, 100.0), (800.0, 600.0));
        assert_eq!(shown.frame().zoom, 1.0);
        shown.update(Msg::ZoomIn);
        assert_eq!(shown.frame().zoom, ZOOM_STEP);
        shown.update(Msg::ZoomOut);
        assert!((shown.frame().zoom - 1.0).abs() < 1e-6);
        let before = shown.frame();
        shown.update(Msg::Gesture(Gesture::Zoom {
            factor: 2.0,
            at: (100.0, -50.0),
        }));
        let after = shown.frame();
        assert_eq!(after.zoom, 2.0);
        // The image point under the pointer did not move.
        let under = |f: ImageFrame| ((100.0 - f.x) / f.zoom, (-50.0 - f.y) / f.zoom);
        assert_eq!(under(after), under(before));
    }

    #[test]
    fn a_double_click_toggles_between_fit_and_actual_size_at_the_pointer() {
        let mut shown = picture((2000.0, 1000.0), (1032.0, 800.0));
        shown.update(Msg::Gesture(Gesture::Double { at: (0.0, 0.0) }));
        assert_eq!(shown.frame().zoom, 1.0);
        assert!(shown.chosen.is_some());
        shown.update(Msg::Gesture(Gesture::Double { at: (0.0, 0.0) }));
        assert!(shown.chosen.is_none());
        assert_eq!(shown.frame().zoom, 0.5);
    }

    #[test]
    fn the_keys_nudge_zoom_fit_and_show_the_pixels() {
        let mut shown = picture((100.0, 100.0), (800.0, 600.0));
        shown.update(Msg::Gesture(Gesture::Key(StageKey::Nudge { dx: 1, dy: 0 })));
        assert_eq!(shown.frame().x, NUDGE);
        shown.update(Msg::Gesture(Gesture::Key(StageKey::Nudge {
            dx: 0,
            dy: -1,
        })));
        assert_eq!(shown.frame().y, -NUDGE);
        shown.update(Msg::Gesture(Gesture::Key(StageKey::ZoomIn)));
        assert!(shown.frame().zoom > 1.0);
        shown.update(Msg::Gesture(Gesture::Key(StageKey::ActualSize)));
        assert_eq!(shown.frame().zoom, 1.0);
        shown.update(Msg::Gesture(Gesture::Key(StageKey::Fit)));
        assert!(shown.chosen.is_none());
    }

    #[test]
    fn two_fingers_zoom_around_their_middle_and_move_with_it() {
        let mut shown = picture((100.0, 100.0), (800.0, 600.0));
        shown.update(Msg::Gesture(Gesture::Pinch {
            factor: 2.0,
            from: (0.0, 0.0),
            to: (10.0, 20.0),
        }));
        let frame = shown.frame();
        assert_eq!((frame.zoom, frame.x, frame.y), (2.0, 10.0, 20.0));
    }

    #[test]
    fn the_zoom_is_written_as_a_percent_in_each_language() {
        assert_eq!(percent(Locale::En, 0.5), "50%");
        assert_eq!(percent(Locale::Fr, 0.5), "50\u{202f}%");
        assert_eq!(percent(Locale::En, 32.0), "3,200%");
    }

    fn encoded(bytes: &[u8]) -> Loaded {
        Loaded::Encoded(Bytes(std::sync::Arc::new(bytes.to_vec())))
    }

    #[test]
    fn a_broken_svg_fails_and_one_without_a_size_does_not() {
        assert!(
            check_svg(b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>").is_ok()
        );
        // No width, height or viewBox: drawn at 512 square, as before.
        assert!(check_svg(b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_ok());
        assert!(check_svg(b"<svg xmlns='http://www.w3.org/2000/svg' width='4'><rect").is_err());
        assert!(check_svg(b"not xml at all").is_err());
        assert!(check_svg(b"").is_err());
        assert!(check_svg(&[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn reading_a_broken_svg_is_a_failed_image_and_a_good_one_is_the_source() {
        let dir = std::env::temp_dir().join(format!("hangar-svg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ok.svg"),
            "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>",
        )
        .unwrap();
        std::fs::write(dir.join("bad.svg"), "<svg><oops").unwrap();
        assert!(matches!(read(&dir.join("ok.svg")), Ok(Loaded::Encoded(_))));
        let why = read(&dir.join("bad.svg")).unwrap_err();
        assert!(why.contains("bad.svg"), "{why}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_svg_is_vector_and_the_rest_raster() {
        let bytes = encoded(b"<svg xmlns='http://www.w3.org/2000/svg'/>");
        assert!(matches!(pic_of("a/logo.svg", &bytes), Pic::Vector(_)));
        assert!(matches!(pic_of("a/shot.png", &bytes), Pic::Raster(_)));
    }

    #[test]
    fn an_image_read_again_is_not_the_one_the_renderer_decoded_before() {
        // A handle made from bytes is its own: the renderer keys what it
        // decoded on it, so a file rewritten in place (the same path) is
        // decoded afresh, where a handle made from the path would be served
        // from the renderer's memory.
        let bytes = encoded(&[1, 2, 3]);
        let before = pic_of("a/shot.png", &bytes);
        let after = pic_of("a/shot.png", &bytes);
        assert_ne!(before, after);
        assert_eq!(before.clone(), before);
    }
}
