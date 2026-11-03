//! A picture of the main window, for whoever builds the app without a way to
//! take one: on a GNOME Wayland session nothing but the person may capture
//! the screen, and the agents porting the views need to see them.
//!
//! `HANGAR_DEV_SCREENSHOT=/some/where.png scripts/native-dev.sh` opens the app,
//! waits for it to settle (`HANGAR_DEV_SCREENSHOT_AFTER_MS`, 3000 by default),
//! writes what the main window shows as a PNG, and quits. Debug builds only:
//! a release build ignores the variables.
//!
//! The picture is a frame of the main window's view drawn off screen at the
//! moment of the capture, at the window's size and scale and in its theme, and
//! read back at once. It is not iced's `window::screenshot`, which draws again
//! what the window's renderer recorded for its last frame: a frame holds its
//! text by weak references, and the layout iced does after every message
//! makes each text editor's buffer anew (and any text whose content, size or
//! fonts changed), so a capture taken after a message and before the window's
//! next frame lost every text area's text, and at times more (the tab labels,
//! when the fonts came in late on a loaded machine). The window itself always
//! drew them: it draws a frame and presents it in one go. On a Wayland session
//! a window draws only when the compositor asks for a frame, and a dozen
//! rebuilds between two frames are common while the app starts.
//!
//! What iced keeps in the window's widgets rather than in the app starts afresh
//! in the picture, as in a window just opened: where a list was scrolled to,
//! which field has the caret, what the pointer is over.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::advanced::clipboard;
use iced::advanced::renderer::{Headless, Style};
use iced::theme::Base;
use iced::{Element, Event, Size, Task, mouse, window};
use iced_runtime::user_interface::{Cache, UserInterface};

use crate::message::Message;

/// The file to write, when a debug build was asked for a picture.
pub fn requested() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("HANGAR_DEV_SCREENSHOT")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// What the picture needs of the window, asked of it once the wait is over.
#[derive(Debug, Clone, Copy)]
pub struct Due {
    /// The window's size in the view's units: what iced lays the view out in,
    /// after the system's scale and the interface size.
    pub size: Size,
    /// The system's scale factor for the window, which the interface size
    /// multiplies.
    pub system_scale: f32,
}

/// Waits, then asks `window` for what [`take`] needs.
pub fn capture(window: window::Id) -> Task<Message> {
    let after = std::env::var("HANGAR_DEV_SCREENSHOT_AFTER_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .unwrap_or(3000);
    Task::perform(tokio::time::sleep(Duration::from_millis(after)), |()| ())
        .then(move |()| window::size(window))
        .then(move |size| {
            window::scale_factor(window).map(move |system_scale| Due { size, system_scale })
        })
        .map(Message::DevScreenshot)
}

/// Draws `view`, the main window's, as the window is now, and writes it to
/// `path`. `ui_scale` is the interface size (`App::scale_factor`).
pub fn take(
    path: &Path,
    view: Element<'_, Message>,
    due: Due,
    ui_scale: f32,
    theme: &iced::Theme,
) -> Result<(), String> {
    // The window's own renderer is wgpu, and tiny-skia when the machine has
    // no GPU iced can use: the picture is drawn the same way.
    let shot = render(view, due.size, due.system_scale * ui_scale, theme, None)?;
    write(path, &shot)
}

/// Lays `view` out at `size`, draws it and reads the pixels back, with
/// nothing in between that could lay it out again. `backend` names iced's
/// renderer (`wgpu`, `tiny-skia`); `None` takes the first that starts.
pub(crate) fn render<M>(
    view: Element<'_, M>,
    size: Size,
    scale_factor: f32,
    theme: &iced::Theme,
    backend: Option<&str>,
) -> Result<window::Screenshot, String> {
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        hangar_theme::fonts::ui(),
        iced::Settings::default().default_text_size,
        backend,
    ))
    .ok_or_else(|| format!("no renderer could start ({})", backend.unwrap_or("any")))?;
    let mut ui = UserInterface::build(view, size, Cache::default(), &mut renderer);
    // A window's first frame starts with this event, which the widgets that
    // keep time (a caret, an animation) set themselves by. What they send is
    // the window's to act on, not the picture's.
    let mut ignored = Vec::new();
    let (state, _) = ui.update(
        &[Event::Window(
            window::Event::RedrawRequested(Instant::now()),
        )],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut ignored,
    );
    if state.has_layout_changed() {
        ui = ui.relayout(size, &mut renderer);
    }
    let base = theme.base();
    ui.draw(
        &mut renderer,
        theme,
        &Style {
            text_color: base.text_color,
        },
        mouse::Cursor::Unavailable,
    );
    let physical = Size::new(
        (size.width * scale_factor).round() as u32,
        (size.height * scale_factor).round() as u32,
    );
    let rgba = renderer.screenshot(physical, scale_factor, base.background_color);
    Ok(window::Screenshot::new(rgba, physical, scale_factor))
}

/// Writes the capture as a PNG.
pub fn write(path: &Path, shot: &window::Screenshot) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|err| err.to_string())?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
    writer
        .write_image_data(&shot.rgba)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use iced::widget::{column, text, text_editor};

    use super::*;

    const SIZE: Size = Size::new(320.0, 140.0);

    /// The renderer the tests are drawn with (`ICED_TEST_BACKEND`, as
    /// iced_test reads it), with Inter loaded as the app loads it.
    fn backend() -> Option<String> {
        iced::advanced::graphics::text::font_system()
            .write()
            .expect("the font system")
            .load_font(Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE));
        std::env::var("ICED_TEST_BACKEND").ok()
    }

    fn page(content: &text_editor::Content) -> Element<'_, ()> {
        column![
            text("A label above"),
            text_editor(content).height(80).on_action(|_| ()),
        ]
        .spacing(8)
        .padding(12)
        .into()
    }

    /// How many pixels of two pictures of one size are visibly apart.
    fn apart(a: &[u8], b: &[u8]) -> usize {
        assert_eq!(a.len(), b.len());
        let (a, b) = (a.as_chunks::<4>().0, b.as_chunks::<4>().0);
        a.iter()
            .zip(b)
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 40))
            .count()
    }

    fn text_of_the_page() -> text_editor::Content {
        text_editor::Content::with_text("The first line of a note\nand a second one")
    }

    /// Why the capture draws its own frame: what iced's `window::screenshot`
    /// does, drawing again the last frame's record once a message had the
    /// view laid out again, draws the text area empty. If this starts to fail,
    /// iced has changed, and the window's own capture may be right again.
    #[test]
    fn a_frame_drawn_again_after_a_relayout_has_lost_the_text_of_its_text_area() {
        let backend = backend();
        let theme = iced::Theme::Dark;
        let content = text_of_the_page();
        let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            hangar_theme::fonts::ui(),
            iced::Settings::default().default_text_size,
            backend.as_deref(),
        ))
        .expect("a renderer");
        let style = Style {
            text_color: theme.base().text_color,
        };
        let physical = Size::new(SIZE.width as u32, SIZE.height as u32);
        let background = theme.base().background_color;

        let mut ui = UserInterface::build(page(&content), SIZE, Cache::default(), &mut renderer);
        ui.draw(&mut renderer, &theme, &style, mouse::Cursor::Unavailable);
        let fresh = renderer.screenshot(physical, 1.0, background);
        // A message: iced builds the view again and lays it out, and the
        // window has not drawn since.
        let ui = ui.relayout(SIZE, &mut renderer);
        let stale = renderer.screenshot(physical, 1.0, background);
        drop(ui);

        let blank = text_editor::Content::new();
        let empty = render(page(&blank), SIZE, 1.0, &theme, backend.as_deref()).expect("drawn");
        assert!(apart(&fresh, &empty.rgba) > 150, "the text is drawn");
        assert!(
            apart(&stale, &empty.rgba) < 10,
            "drawn again after a relayout, the text area is empty"
        );
    }

    /// The capture draws a text area's text however many times the window laid
    /// it out since its last frame, and a label's.
    #[test]
    fn the_capture_draws_the_text_of_a_text_area_laid_out_many_times_before() {
        let backend = backend();
        let theme = iced::Theme::Dark;
        let content = text_of_the_page();
        let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            hangar_theme::fonts::ui(),
            iced::Settings::default().default_text_size,
            backend.as_deref(),
        ))
        .expect("a renderer");
        // The window's own rebuilds, a frame drawn after the first only.
        let mut ui = UserInterface::build(page(&content), SIZE, Cache::default(), &mut renderer);
        ui.draw(
            &mut renderer,
            &theme,
            &Style {
                text_color: theme.base().text_color,
            },
            mouse::Cursor::Unavailable,
        );
        let mut cache = ui.into_cache();
        for _ in 0..12 {
            cache = UserInterface::build(page(&content), SIZE, cache, &mut renderer).into_cache();
        }

        let shot = render(page(&content), SIZE, 2.0, &theme, backend.as_deref()).expect("drawn");
        assert_eq!(shot.size, Size::new(640, 280), "at the window's scale");
        let blank = text_editor::Content::new();
        let empty = render(page(&blank), SIZE, 2.0, &theme, backend.as_deref()).expect("drawn");
        assert!(
            apart(&shot.rgba, &empty.rgba) > 600,
            "the text area's text is drawn"
        );

        let labelled = render::<()>(
            text("A label").into(),
            SIZE,
            1.0,
            &theme,
            backend.as_deref(),
        )
        .expect("drawn");
        let unlabelled =
            render::<()>(text("").into(), SIZE, 1.0, &theme, backend.as_deref()).expect("drawn");
        assert!(
            apart(&labelled.rgba, &unlabelled.rgba) > 30,
            "a label is drawn"
        );
    }

    #[test]
    fn a_capture_is_written_as_a_png_of_its_size() {
        let backend = backend();
        let shot = render::<()>(
            text("Written").into(),
            Size::new(90.0, 30.0),
            1.5,
            &iced::Theme::Light,
            backend.as_deref(),
        )
        .expect("drawn");
        let dir = std::env::temp_dir().join(format!("hangar-dev-shot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let path = dir.join("shot.png");
        write(&path, &shot).expect("written");
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).expect("the file"),
        ));
        let reader = decoder.read_info().expect("a PNG");
        assert_eq!(
            (reader.info().width, reader.info().height),
            (135, 45),
            "90×30 at a scale of 1.5"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
