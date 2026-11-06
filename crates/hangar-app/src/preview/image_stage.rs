//! The stage an image sits on: a widget that draws it where its frame says
//! (zoom, and its centre's offset from the stage's), and turns the pointer,
//! the wheel, two fingers and a few keys into gestures
//! (`components/preview/ImageView.tsx`, `Picture`).
//!
//! The widget does the geometry the DOM did for the TS: it measures the
//! image once the renderer has it, reports the stage's size when it changes
//! (the `ResizeObserver`), and says what the person did. What a gesture does
//! to the frame is the view's, with `model::zoom_around`.

use std::sync::Arc;

use iced::advanced::image::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{self, Click, click};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::svg::Renderer as _;
use iced::advanced::widget::operation::{Focusable, Operation};
use iced::advanced::widget::{self, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, svg as svg_core};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::image;
use iced::{
    Background, Border, Color, Element, Event, Length, Point, Rectangle, Size, Vector, touch,
    window,
};

use super::decode::{Animation, Clock};
use super::model::{ImageFrame, Size as FrameSize};

/// A picture the stage can hold: a raster the renderer decodes, a vector
/// one it rasterises at the size asked, or a raster of several frames that
/// the stage plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pic {
    Raster(image::Handle),
    /// Pixels decoded off the UI thread (`decode.rs`), and the size of the
    /// picture they are of: fewer of them than that when it was reduced.
    Pixels(image::Handle, u32, u32),
    Vector(svg_core::Handle),
    Animated(Arc<Animation>),
}

/// What the person did on the stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// The wheel, or a pinch on a trackpad: `factor` around `at`, from the
    /// stage's centre.
    Zoom {
        factor: f32,
        at: (f32, f32),
    },
    /// Dragging: the pointer moved by this much.
    Pan {
        dx: f32,
        dy: f32,
    },
    /// Two fingers: the spread between them changed by `factor` around the
    /// middle they started from, and the middle moved to `to`.
    Pinch {
        factor: f32,
        from: (f32, f32),
        to: (f32, f32),
    },
    /// Two clicks: fit if it was framed by hand, else the actual size there.
    Double {
        at: (f32, f32),
    },
    Key(StageKey),
    /// The pointer went down on the stage, which has the keyboard from then
    /// on: whatever else had it (a terminal) is to be told. Nothing to the
    /// frame.
    Pressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKey {
    /// An arrow: the image moves that way by `NUDGE`.
    Nudge {
        dx: i8,
        dy: i8,
    },
    ZoomIn,
    ZoomOut,
    Fit,
    ActualSize,
}

/// An arrow key moves the image this far, in pixels.
pub const NUDGE: f32 = 40.0;

/// How far one line of wheel is, in the pixels the TS's `deltaY` counted: a
/// notch of a mouse wheel was about a hundred.
const LINE: f32 = 100.0;

/// The wheel's step: `exp(-delta × 0.002)`, or ×0.01 with Ctrl (a trackpad's
/// pinch arrives as a wheel with Ctrl held, in small events).
pub fn wheel_factor(delta_y: f32, ctrl: bool) -> f32 {
    (-delta_y * if ctrl { 0.01 } else { 0.002 }).exp()
}

/// What the stage is told about the picture and its own size, once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measured {
    /// The picture's own size in pixels. An SVG without a size of its own
    /// still needs one to fit.
    Size(FrameSize),
    /// The renderer could not decode it.
    Failed,
}

pub struct Stage<'a, M> {
    id: Option<widget::Id>,
    pic: &'a Pic,
    frame: ImageFrame,
    natural: Option<FrameSize>,
    pixelated: bool,
    background: Color,
    checker: Color,
    edge: Color,
    ring: Color,
    on_measured: Box<dyn Fn(Measured) -> M + 'a>,
    on_stage: Box<dyn Fn(FrameSize) -> M + 'a>,
    on_gesture: Box<dyn Fn(Gesture) -> M + 'a>,
}

#[allow(clippy::too_many_arguments)]
impl<'a, M> Stage<'a, M> {
    pub fn new(
        pic: &'a Pic,
        frame: ImageFrame,
        natural: Option<FrameSize>,
        t: &hangar_theme::Theme,
        on_measured: impl Fn(Measured) -> M + 'a,
        on_stage: impl Fn(FrameSize) -> M + 'a,
        on_gesture: impl Fn(Gesture) -> M + 'a,
    ) -> Self {
        let c = &t.colors;
        // A tenth of the text colour over the page: the squares that show
        // what is transparent (`--checker`).
        let mix = |a: Color, b: Color| {
            Color::from_rgb(
                b.r + (a.r - b.r) * 0.10,
                b.g + (a.g - b.g) * 0.10,
                b.b + (a.b - b.b) * 0.10,
            )
        };
        Stage {
            id: None,
            pic,
            frame,
            natural,
            pixelated: frame.zoom >= PIXELATED,
            background: c.bg_sunken,
            checker: mix(c.fg, c.bg),
            edge: c.line,
            ring: c.accent_ring,
            on_measured: Box::new(on_measured),
            on_stage: Box::new(on_stage),
            on_gesture: Box::new(on_gesture),
        }
    }

    /// The id F6 focuses the stage by.
    pub fn id(mut self, id: widget::Id) -> Self {
        self.id = Some(id);
        self
    }
}

/// From this zoom up, pixels are drawn as squares: what someone zooming that
/// far wants to see.
pub const PIXELATED: f32 = 3.0;

/// The size of a square of the checkerboard.
const CHECK: f32 = 16.0;

#[derive(Default)]
struct State {
    focused: bool,
    dragging: bool,
    last: Option<Point>,
    modifiers: keyboard::Modifiers,
    last_click: Option<Click>,
    fingers: Vec<(touch::Finger, Point)>,
    /// What the renderer found of the picture, and of which.
    measured: Option<(Pic, Measured)>,
    reported_measure: bool,
    reported_stage: Option<Size>,
    /// Where an animation is; it begins again for another picture.
    clock: Clock,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }

    fn focus(&mut self) {
        self.focused = true;
    }

    fn unfocus(&mut self) {
        self.focused = false;
    }
}

/// Where the picture is drawn, from the stage's bounds and the frame.
pub fn picture_rect(bounds: Rectangle, natural: FrameSize, frame: ImageFrame) -> Rectangle {
    let width = natural.width * frame.zoom;
    let height = natural.height * frame.zoom;
    Rectangle {
        x: bounds.center_x() + frame.x - width / 2.0,
        y: bounds.center_y() + frame.y - height / 2.0,
        width,
        height,
    }
}

/// Where an event is, from the stage's centre.
fn local(bounds: Rectangle, point: Point) -> (f32, f32) {
    (point.x - bounds.center_x(), point.y - bounds.center_y())
}

fn measure(renderer: &iced::Renderer, pic: &Pic) -> Measured {
    match pic {
        // Decoded off the UI thread (`decode.rs`): its size is known, and
        // asking the renderer would upload it in the middle of the layout.
        Pic::Pixels(_, width, height) | Pic::Raster(image::Handle::Rgba { width, height, .. }) => {
            Measured::Size(FrameSize::new(*width as f32, *height as f32))
        }
        Pic::Raster(handle) => match renderer.load_image(handle) {
            Ok(allocation) => {
                let size = allocation.size();
                Measured::Size(FrameSize::new(size.width as f32, size.height as f32))
            }
            Err(_) => Measured::Failed,
        },
        Pic::Vector(handle) => {
            let size = renderer.measure_svg(handle);
            // An SVG without a size of its own is treated as 512 square.
            let side = |value: u32| if value == 0 { 512.0 } else { value as f32 };
            Measured::Size(FrameSize::new(side(size.width), side(size.height)))
        }
        // Its frames were decoded to the canvas's size: nothing to ask.
        Pic::Animated(animation) => Measured::Size(FrameSize::new(
            animation.width as f32,
            animation.height as f32,
        )),
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Stage<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let known = state
            .measured
            .as_ref()
            .is_some_and(|(pic, _)| pic == self.pic);
        if !known {
            state.measured = Some((self.pic.clone(), measure(renderer, self.pic)));
            state.reported_measure = false;
            state.clock.reset();
        }
        layout::Node::new(limits.max())
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();
        operation.focusable(self.id.as_ref(), layout.bounds(), state);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(bounds);
        let gesture = |gesture: Gesture, shell: &mut Shell<'_, M>| {
            shell.publish((self.on_gesture)(gesture));
            shell.capture_event();
            shell.request_redraw();
        };

        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                // Said once the renderer has the picture, and each time the
                // stage changes size.
                if !state.reported_measure
                    && let Some((_, measured)) = &state.measured
                {
                    state.reported_measure = true;
                    shell.publish((self.on_measured)(*measured));
                }
                if state.reported_stage != Some(bounds.size()) {
                    state.reported_stage = Some(bounds.size());
                    shell.publish((self.on_stage)(FrameSize::new(bounds.width, bounds.height)));
                }
                // An animation asks to be drawn again when its frame ends.
                // Nothing asks while the stage is not drawn at all (another
                // tab, a hidden panel, a minimised window), so an animation
                // out of sight costs nothing.
                if let Pic::Animated(animation) = self.pic {
                    shell.request_redraw_at(state.clock.tick(*now, &animation.timeline));
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if over => {
                let delta_y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * LINE,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                // Iced's wheel is positive upward; the DOM's `deltaY` is
                // positive downward.
                let factor = wheel_factor(-delta_y, state.modifiers.control());
                if let Some(point) = cursor.position() {
                    gesture(
                        Gesture::Zoom {
                            factor,
                            at: local(bounds, point),
                        },
                        shell,
                    );
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if over && let Some(point) = cursor.position() {
                    state.focused = true;
                    shell.publish((self.on_gesture)(Gesture::Pressed));
                    state.last = Some(point);
                    let click = Click::new(point, mouse::Button::Left, state.last_click);
                    state.last_click = Some(click);
                    if click.kind() == click::Kind::Double {
                        state.dragging = false;
                        gesture(
                            Gesture::Double {
                                at: local(bounds, point),
                            },
                            shell,
                        );
                    } else {
                        state.dragging = true;
                        shell.capture_event();
                    }
                    shell.request_redraw();
                } else if state.focused {
                    // A press anywhere else takes the keyboard back.
                    state.focused = false;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                if let Some(last) = state.last.replace(*position) {
                    let (dx, dy) = (position.x - last.x, position.y - last.y);
                    if dx != 0.0 || dy != 0.0 {
                        gesture(Gesture::Pan { dx, dy }, shell);
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                state.last = None;
                shell.request_redraw();
            }
            Event::Touch(touch::Event::FingerPressed { id, position })
                if bounds.contains(*position) =>
            {
                state.focused = true;
                state.fingers.retain(|(finger, _)| finger != id);
                state.fingers.push((*id, *position));
                shell.capture_event();
            }
            Event::Touch(touch::Event::FingerMoved { id, position }) => {
                let Some(at) = state.fingers.iter().position(|(finger, _)| finger == id) else {
                    return;
                };
                let before = state.fingers[at].1;
                let others: Vec<Point> = state
                    .fingers
                    .iter()
                    .filter(|(finger, _)| finger != id)
                    .map(|(_, point)| *point)
                    .collect();
                state.fingers[at].1 = *position;
                match others.as_slice() {
                    [] => gesture(
                        Gesture::Pan {
                            dx: position.x - before.x,
                            dy: position.y - before.y,
                        },
                        shell,
                    ),
                    [other, ..] => {
                        let spread = |p: Point| (p.x - other.x).hypot(p.y - other.y);
                        let middle = |p: Point| {
                            local(
                                bounds,
                                Point::new((p.x + other.x) / 2.0, (p.y + other.y) / 2.0),
                            )
                        };
                        let factor = if spread(before) > 0.0 {
                            spread(*position) / spread(before)
                        } else {
                            1.0
                        };
                        gesture(
                            Gesture::Pinch {
                                factor,
                                from: middle(before),
                                to: middle(*position),
                            },
                            shell,
                        );
                    }
                }
            }
            Event::Touch(
                touch::Event::FingerLifted { id, .. } | touch::Event::FingerLost { id, .. },
            ) => state.fingers.retain(|(finger, _)| finger != id),
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if state.focused
                    && !modifiers.control()
                    && !modifiers.alt()
                    && !modifiers.command() =>
            {
                let stage_key = match key.as_ref() {
                    Key::Named(Named::ArrowLeft) => Some(StageKey::Nudge { dx: 1, dy: 0 }),
                    Key::Named(Named::ArrowRight) => Some(StageKey::Nudge { dx: -1, dy: 0 }),
                    Key::Named(Named::ArrowUp) => Some(StageKey::Nudge { dx: 0, dy: 1 }),
                    Key::Named(Named::ArrowDown) => Some(StageKey::Nudge { dx: 0, dy: -1 }),
                    Key::Character("+" | "=") => Some(StageKey::ZoomIn),
                    Key::Character("-") => Some(StageKey::ZoomOut),
                    Key::Character("0") => Some(StageKey::Fit),
                    Key::Character("1") => Some(StageKey::ActualSize),
                    _ => None,
                };
                if let Some(stage_key) = stage_key {
                    gesture(Gesture::Key(stage_key), shell);
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        renderer.fill_quad(
            Quad {
                bounds,
                ..Quad::default()
            },
            Background::Color(self.background),
        );
        let Some(natural) = self.natural else {
            return;
        };
        let rect = picture_rect(bounds, natural, self.frame);
        renderer.with_layer(bounds, |renderer| {
            // The squares, only where they show.
            let seen = rect.intersection(&bounds);
            if let Some(seen) = seen {
                renderer.fill_quad(
                    Quad {
                        bounds: seen,
                        ..Quad::default()
                    },
                    Background::Color(self.background),
                );
                let first_column = ((seen.x - rect.x) / CHECK).floor() as i64;
                let first_row = ((seen.y - rect.y) / CHECK).floor() as i64;
                let columns = ((seen.x + seen.width - rect.x) / CHECK).ceil() as i64;
                let rows = ((seen.y + seen.height - rect.y) / CHECK).ceil() as i64;
                for row in first_row..rows {
                    for column in first_column..columns {
                        if (row + column).rem_euclid(2) != 0 {
                            continue;
                        }
                        let square = Rectangle {
                            x: rect.x + column as f32 * CHECK,
                            y: rect.y + row as f32 * CHECK,
                            width: CHECK,
                            height: CHECK,
                        };
                        if let Some(visible) = square.intersection(&seen) {
                            renderer.fill_quad(
                                Quad {
                                    bounds: visible,
                                    ..Quad::default()
                                },
                                Background::Color(self.checker),
                            );
                        }
                    }
                }
            }
            match self.pic {
                Pic::Raster(handle) | Pic::Pixels(handle, ..) => renderer.draw_image(
                    iced::advanced::image::Image::new(handle.clone()).filter_method(
                        if self.pixelated {
                            image::FilterMethod::Nearest
                        } else {
                            image::FilterMethod::Linear
                        },
                    ),
                    rect,
                    bounds,
                ),
                Pic::Vector(handle) => {
                    renderer.draw_svg(svg_core::Svg::new(handle.clone()), rect, bounds)
                }
                Pic::Animated(animation) => {
                    let frame =
                        &animation.frames[state.clock.frame.min(animation.frames.len() - 1)];
                    renderer.draw_image(
                        iced::advanced::image::Image::new(frame.handle.clone()).filter_method(
                            if self.pixelated {
                                image::FilterMethod::Nearest
                            } else {
                                image::FilterMethod::Linear
                            },
                        ),
                        rect,
                        bounds,
                    );
                }
            }
            renderer.fill_quad(
                Quad {
                    bounds: rect,
                    border: Border {
                        color: self.edge,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Quad::default()
                },
                Background::Color(Color::TRANSPARENT),
            );
        });
        if state.focused {
            renderer.fill_quad(
                Quad {
                    bounds,
                    border: Border {
                        color: self.ring,
                        width: 2.0,
                        radius: 0.0.into(),
                    },
                    ..Quad::default()
                },
                Background::Color(Color::TRANSPARENT),
            );
        }
        let _ = Vector::ZERO;
    }
}

impl<'a, M: Clone + 'a> From<Stage<'a, M>> for Element<'a, M> {
    fn from(stage: Stage<'a, M>) -> Self {
        Element::new(stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_zooms_by_its_distance_and_a_pinch_in_finer_steps() {
        assert_eq!(wheel_factor(0.0, false), 1.0);
        assert!(wheel_factor(-100.0, false) > 1.2, "scrolling up zooms in");
        assert!(wheel_factor(100.0, false) < 0.83);
        let notch = wheel_factor(-100.0, false);
        let pinch = wheel_factor(-100.0, true);
        // A pinch arrives as many small wheel events: each counts five times
        // as much as a notch's distance would.
        assert!(pinch > notch, "{pinch} {notch}");
    }

    #[test]
    fn the_picture_sits_on_the_centre_of_the_stage_shifted_by_the_frame() {
        let stage = Rectangle::new(Point::new(10.0, 20.0), Size::new(400.0, 200.0));
        let natural = FrameSize::new(100.0, 50.0);
        let centred = picture_rect(
            stage,
            natural,
            ImageFrame {
                zoom: 1.0,
                x: 0.0,
                y: 0.0,
            },
        );
        assert_eq!(
            centred,
            Rectangle::new(Point::new(160.0, 95.0), Size::new(100.0, 50.0))
        );
        let moved = picture_rect(
            stage,
            natural,
            ImageFrame {
                zoom: 2.0,
                x: 30.0,
                y: -10.0,
            },
        );
        assert_eq!(
            moved,
            Rectangle::new(Point::new(140.0, 60.0), Size::new(200.0, 100.0))
        );
    }

    /// A 2 by 2 picture of one colour.
    fn solid(rgba: [u8; 4]) -> image::Handle {
        image::Handle::from_rgba(2, 2, rgba.repeat(4))
    }

    /// The colour at the middle of a snapshot, read back from the PNG the
    /// simulator writes.
    fn middle(
        snapshot: &iced_test::simulator::Snapshot,
        dir: &std::path::Path,
        name: &str,
    ) -> [u8; 4] {
        snapshot.matches_image(dir.join(name)).expect("written");
        let file = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|file| file.to_string_lossy().starts_with(name))
            })
            .expect("the png");
        let mut reader =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(file).unwrap()))
                .read_info()
                .unwrap();
        let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut bytes).unwrap();
        let at = ((info.height / 2) * info.width + info.width / 2) as usize * 4;
        [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
    }

    #[test]
    fn an_animation_shows_the_frame_its_clock_has_reached() {
        use std::time::Duration;
        let frames = vec![
            super::super::decode::Frame {
                handle: solid([255, 0, 0, 255]),
                delay: Duration::from_millis(500),
            },
            super::super::decode::Frame {
                handle: solid([0, 0, 255, 255]),
                delay: Duration::from_millis(500),
            },
        ];
        let timeline = super::super::decode::Timeline::new(frames.iter().map(|f| f.delay));
        let pic = Pic::Animated(Arc::new(Animation {
            width: 2,
            height: 2,
            frames,
            timeline,
        }));
        let theme = hangar_theme::by_id_or_default("graphite");
        let stage = Stage::new(
            &pic,
            ImageFrame {
                zoom: 40.0,
                x: 0.0,
                y: 0.0,
            },
            Some(FrameSize::new(2.0, 2.0)),
            theme,
            |_| (),
            |_| (),
            |_| (),
        );
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(200.0, 200.0),
            iced::Element::from(stage),
        );
        let dir = crate::kit_tabs::testdir::scratch("stage-anim");
        let iced_theme = iced::Theme::Dark;
        // The clock starts at the first draw: the first frame.
        let first = middle(&ui.snapshot(&iced_theme).unwrap(), &dir, "first");
        assert!(first[0] > 200 && first[2] < 60, "{first:?}");
        // Past the first frame's half second, and not yet past the second's.
        std::thread::sleep(Duration::from_millis(700));
        let second = middle(&ui.snapshot(&iced_theme).unwrap(), &dir, "second");
        assert!(second[2] > 200 && second[0] < 60, "{second:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_point_is_told_from_the_centre_of_the_stage() {
        let stage = Rectangle::new(Point::new(0.0, 0.0), Size::new(400.0, 200.0));
        assert_eq!(local(stage, Point::new(200.0, 100.0)), (0.0, 0.0));
        assert_eq!(local(stage, Point::new(300.0, 50.0)), (100.0, -50.0));
    }
}
