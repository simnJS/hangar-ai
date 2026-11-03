//! The small drawings of the inspector's sections that no other surface
//! shares: the context ring, the plan meter, the five squares of a file's
//! changes, and the grip on the column's left edge.

use std::f32::consts::{FRAC_PI_2, TAU};

use hangar_theme::tokens::{font_size, radius, space};
use hangar_theme::{Theme, fonts, style};
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::click::{self, Click};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::widget::{canvas, column, container, progress_bar, row, space as filler, stack, text};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Point, Radians, Rectangle, Size,
    keyboard, mouse,
};

use super::files_model::DiffSegment;
use super::format::{Tone, usage_tone};

/// The ink of a reading: neutral, amber, red.
fn tone_color(t: &Theme, tone: Tone) -> Color {
    match tone {
        Tone::Normal => t.colors.fg_muted,
        Tone::Warn => t.colors.warning,
        Tone::Danger => t.colors.danger,
    }
}

// ---- The context ring ---------------------------------------------------------------------

/// The ring's side and the radius and stroke of its 64-unit drawing
/// (`.insp-ring`: r = 27, stroke 5).
const RING: f32 = 64.0;
const RING_RADIUS: f32 = 27.0 / 64.0;
const RING_STROKE: f32 = 5.0 / 64.0;

struct Arc {
    pct: f64,
    track: Color,
    value: Color,
}

impl<M> canvas::Program<M> for Arc {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let centre = frame.center();
        let radius = bounds.width * RING_RADIUS;
        let width = bounds.width * RING_STROKE;
        let stroke = |color: Color| {
            canvas::Stroke::default()
                .with_width(width)
                .with_color(color)
        };
        frame.stroke(&canvas::Path::circle(centre, radius), stroke(self.track));
        let fraction = (self.pct / 100.0).clamp(0.0, 1.0) as f32;
        if fraction >= 1.0 {
            frame.stroke(&canvas::Path::circle(centre, radius), stroke(self.value));
        } else if fraction > 0.0 {
            // From twelve o'clock, clockwise, with round ends.
            let start = -FRAC_PI_2;
            let arc = canvas::Path::new(|path| {
                path.arc(canvas::path::Arc {
                    center: centre,
                    radius,
                    start_angle: Radians(start),
                    end_angle: Radians(start + TAU * fraction),
                });
            });
            frame.stroke(
                &arc,
                stroke(self.value).with_line_cap(canvas::LineCap::Round),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// The context gauge as a ring with its percentage inside: neutral, amber from
/// 70 %, red from 90 % (`Ring` of `InspectorPanels.tsx`).
pub fn ring<'a, M: 'a>(t: &'a Theme, pct: f64) -> Element<'a, M> {
    let tone = usage_tone(pct);
    let drawing = canvas(Arc {
        pct,
        track: t.colors.line_strong,
        value: tone_color(t, tone),
    })
    .width(RING)
    .height(RING);
    let label = row![
        text(format!("{}", pct.round() as i64))
            .size(font_size::TITLE)
            .font(fonts::ui_semibold())
            .style(style::text::primary(t)),
        text("%")
            .size(font_size::MICRO)
            .font(fonts::ui_medium())
            .style(style::text::subtle(t)),
    ]
    .spacing(1)
    .align_y(Alignment::End);
    stack![drawing, container(label).center(Length::Fill)]
        .width(RING)
        .height(RING)
        .into()
}

// ---- The plan meter -----------------------------------------------------------------------

/// A thin bar for a plan window's usage, in the tone of the reading
/// (`.insp-meter__bar`).
pub fn meter_bar<'a, M: 'a>(t: &'a Theme, pct: f64) -> Element<'a, M> {
    let fill = tone_color(t, usage_tone(pct));
    let track = t.colors.line_strong;
    progress_bar(0.0..=100.0, pct as f32)
        .girth(4.0)
        .length(Length::Fill)
        .style(move |_: &iced::Theme| progress_bar::Style {
            background: Background::Color(track),
            bar: Background::Color(fill),
            border: Border {
                radius: 2.0.into(),
                ..Border::default()
            },
        })
        .into()
}

// ---- The five squares of a file's changes -------------------------------------------------

/// GitHub's five squares: lit green for additions, red for deletions, grey
/// for the rest (`.insp-diffbar`).
pub fn diff_bar<'a, M: 'a>(t: &'a Theme, segments: &[DiffSegment]) -> Element<'a, M> {
    let c = &t.colors;
    let squares = segments.iter().map(|segment| {
        let color = match segment {
            DiffSegment::Add => c.success,
            DiffSegment::Del => c.danger,
            DiffSegment::None => c.line_strong,
        };
        container(filler::horizontal())
            .width(5.0)
            .height(6.0)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 1.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into()
    });
    row(squares).spacing(1).into()
}

/// A row of a key/value grid: a label in the quiet colour, right-aligned in a
/// column of its own, and its value (`.insp-kv`).
pub fn kv<'a, M: 'a>(
    t: &'a Theme,
    label: impl Into<String>,
    value: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    row![
        container(
            text(label.into())
                .size(font_size::CAPTION_LG)
                .style(style::text::subtle(t))
                .wrapping(text::Wrapping::None)
        )
        .width(54.0)
        .align_x(iced::alignment::Horizontal::Right),
        container(value.into()).width(Length::Fill).clip(true),
    ]
    .spacing(space::LG)
    .align_y(Alignment::Center)
    .into()
}

/// A hairline-edged raised block, as the stat tiles, the task card and the
/// queue's lines are (`--raised` with `inset 0 0 0 1px --line`).
pub fn tile(t: &Theme, radius_of: f32) -> impl Fn(&iced::Theme) -> container::Style + '_ {
    move |_| container::Style {
        background: Some(Background::Color(t.colors.raised)),
        border: Border {
            color: t.colors.line,
            width: 1.0,
            radius: radius_of.into(),
        },
        ..container::Style::default()
    }
}

/// The corner radii the blocks use.
pub const TILE_RADIUS: f32 = radius::MD;
pub const CARD_RADIUS: f32 = radius::LG;

/// A column of `children` with a gap, for the sections' bodies.
pub fn stacked<'a, M: 'a>(
    gap: f32,
    children: impl IntoIterator<Item = Element<'a, M>>,
) -> Element<'a, M> {
    column(children).spacing(gap).width(Length::Fill).into()
}

// ---- The grip -----------------------------------------------------------------------------

/// The hit area of the grip, centred on the column's edge (`.inspector__grip`).
pub const GRIP_WIDTH: f32 = 7.0;

/// What the grip tells the inspector.
pub struct GripEvents<M> {
    /// The press, with the pointer's x in the window.
    pub pressed: Box<dyn Fn(f32) -> M>,
    /// The pointer moved while it was held, to this x.
    pub moved: Box<dyn Fn(f32) -> M>,
    pub released: M,
    /// Escape while it was held: the drag is dropped.
    pub cancelled: M,
    /// A second press close behind the first.
    pub reset: M,
}

/// The handle on the left edge of the column: dragged, it resizes; pressed
/// twice, it goes back to the default width. At rest it is not drawn; under
/// the pointer, or held, it is a single accent hairline.
pub struct Grip<M> {
    events: GripEvents<M>,
    accent: Color,
}

pub fn grip<M>(t: &Theme, events: GripEvents<M>) -> Grip<M> {
    Grip {
        events,
        accent: t.colors.accent,
    }
}

#[derive(Debug, Default)]
struct GripState {
    held: bool,
    hovered: bool,
    last_click: Option<Click>,
}

impl<M: Clone, T> Widget<M, T, iced::Renderer> for Grip<M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<GripState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(GripState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(GRIP_WIDTH), Length::Fill)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fixed(GRIP_WIDTH), Length::Fill)
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
        let state = tree.state.downcast_mut::<GripState>();
        let over = cursor.is_over(layout.bounds());
        if over != state.hovered && !matches!(event, Event::Window(_)) {
            state.hovered = over;
            shell.request_redraw();
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if over => {
                let Some(position) = cursor.position() else {
                    return;
                };
                // iced decides what counts as a double click: the same spot,
                // close in time.
                let click = Click::new(position, mouse::Button::Left, state.last_click);
                state.last_click = Some(click);
                if click.kind() == click::Kind::Double {
                    state.held = false;
                    shell.publish(self.events.reset.clone());
                } else {
                    state.held = true;
                    shell.publish((self.events.pressed)(position.x));
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.held => {
                shell.publish((self.events.moved)(position.x));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.held => {
                state.held = false;
                shell.publish(self.events.released.clone());
                shell.capture_event();
                shell.request_redraw();
            }
            // Captured, so the shell does not also close an overlay or a
            // drawer on the same press.
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) if state.held => {
                state.held = false;
                shell.publish(self.events.cancelled.clone());
                shell.capture_event();
                shell.request_redraw();
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
        let state = tree.state.downcast_ref::<GripState>();
        if state.held || cursor.is_over(layout.bounds()) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::None
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &T,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<GripState>();
        if !(state.held || cursor.is_over(layout.bounds())) {
            return;
        }
        let bounds = layout.bounds();
        let line = Rectangle::new(
            Point::new(bounds.center_x() - 0.5, bounds.y),
            Size::new(1.0, bounds.height),
        );
        renderer.fill_quad(
            Quad {
                bounds: line,
                ..Quad::default()
            },
            Background::Color(self.accent),
        );
    }
}

impl<'a, M: Clone + 'a, T: 'a> From<Grip<M>> for Element<'a, M, T, iced::Renderer> {
    fn from(grip: Grip<M>) -> Self {
        Element::new(grip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_theme::default_theme;
    use iced_test::Simulator;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Pressed(i32),
        Moved(i32),
        Released,
        Cancelled,
        Reset,
    }

    fn grip_events() -> GripEvents<Msg> {
        GripEvents {
            pressed: Box::new(|x| Msg::Pressed(x as i32)),
            moved: Box::new(|x| Msg::Moved(x as i32)),
            released: Msg::Released,
            cancelled: Msg::Cancelled,
            reset: Msg::Reset,
        }
    }

    fn at_the_edge<'a>() -> Element<'a, Msg> {
        // The grip, 7 px wide, at the left of a 300 px column in a 800 px window.
        container(row![
            filler::horizontal().width(Length::Fill),
            container(grip(default_theme(), grip_events()))
                .width(GRIP_WIDTH)
                .height(Length::Fill),
            filler::horizontal().width(300.0),
        ])
        .into()
    }

    fn drive(ui: &mut Simulator<'_, Msg>, events: Vec<Event>) {
        ui.simulate(events);
    }

    fn moved(x: f32) -> Event {
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, 100.0),
        })
    }

    const GRIP_X: f32 = 800.0 - 300.0 - GRIP_WIDTH / 2.0;

    #[test]
    fn dragging_reports_the_pointer_until_the_button_is_let_go() {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(800.0, 400.0),
            at_the_edge(),
        );
        ui.point_at(Point::new(GRIP_X, 100.0));
        drive(
            &mut ui,
            vec![
                moved(GRIP_X),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                moved(GRIP_X - 40.0),
                moved(GRIP_X - 80.0),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                // After the release, moving says nothing.
                moved(GRIP_X - 90.0),
            ],
        );
        let messages: Vec<Msg> = ui.into_messages().collect();
        assert_eq!(
            messages,
            vec![
                Msg::Pressed(GRIP_X as i32),
                Msg::Moved((GRIP_X - 40.0) as i32),
                Msg::Moved((GRIP_X - 80.0) as i32),
                Msg::Released,
            ]
        );
    }

    fn escape() -> Event {
        let key = keyboard::Key::Named(keyboard::key::Named::Escape);
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Unidentified(
                keyboard::key::NativeCode::Unidentified,
            ),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn escape_lets_go_of_a_held_grip_without_a_release() {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(800.0, 400.0),
            at_the_edge(),
        );
        ui.point_at(Point::new(GRIP_X, 100.0));
        drive(
            &mut ui,
            vec![
                // Not held: Escape is somebody else's.
                escape(),
                moved(GRIP_X),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                moved(GRIP_X - 40.0),
                escape(),
                // Let go already: neither the move nor the release says more.
                moved(GRIP_X - 80.0),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ],
        );
        let messages: Vec<Msg> = ui.into_messages().collect();
        assert_eq!(
            messages,
            vec![
                Msg::Pressed(GRIP_X as i32),
                Msg::Moved((GRIP_X - 40.0) as i32),
                Msg::Cancelled,
            ]
        );
    }

    #[test]
    fn a_press_beside_the_grip_does_nothing() {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(800.0, 400.0),
            at_the_edge(),
        );
        ui.point_at(Point::new(100.0, 100.0));
        drive(
            &mut ui,
            vec![
                moved(100.0),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                moved(60.0),
            ],
        );
        assert_eq!(ui.into_messages().count(), 0);
    }

    #[test]
    fn two_quick_presses_ask_for_the_default_width() {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(800.0, 400.0),
            at_the_edge(),
        );
        ui.point_at(Point::new(GRIP_X, 100.0));
        drive(
            &mut ui,
            vec![
                moved(GRIP_X),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            ],
        );
        let messages: Vec<Msg> = ui.into_messages().collect();
        assert!(messages.contains(&Msg::Reset), "{messages:?}");
    }
}
