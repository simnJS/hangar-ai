//! A kit's canvas on screen: its background and marks, drawn from the same
//! primitives as the picture `canvas.export` writes (paint.rs), and the drag
//! of a person drawing on it, which Hangar draws itself as the pointer
//! moves and sends the kit once, finished, as a [`Mark`].

use std::f32::consts::TAU;

use iced::alignment::Vertical;
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, LineCap, LineJoin, Path, Stroke};
use iced::widget::{responsive, text};
use iced::{Color, Font, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

use hangar_kit_ui as ui;
use hangar_kit_ui::{EventValue, Mark, Tool, UiEvent};

use crate::message::KitMessage;
use crate::paint::{self, Prim};

use super::{Cx, El, Picture};

/// How far, in logical pixels, a press moves before it is a drag rather
/// than a point; and how far a freehand stroke moves before it takes a new
/// point.
const DRAG_SLOP: f32 = 3.0;
const STROKE_STEP: f32 = 2.0;

/// A drag in progress, in the canvas's units, and the scale last told to
/// the kit.
#[derive(Default)]
pub(crate) struct Drag {
    points: Vec<ui::Point>,
    /// Where it was pressed, in logical pixels of the canvas.
    pressed: Option<Point>,
    moved: bool,
    told: Option<f32>,
}

struct Board<'a> {
    canvas: &'a ui::Canvas,
    seq: u32,
    scale: f32,
    background: Option<&'a Picture>,
    prims: Vec<Prim>,
    font: Font,
    bold: Font,
}

/// The canvas, drawn at its zoom or fitted to its room.
pub(crate) fn canvas_widget<'a>(cx: &Cx<'a>, w: &'a ui::Canvas) -> El<'a> {
    let style = cx.style;
    let seq = cx.seq;
    let background = w
        .background
        .as_ref()
        .and_then(|source| cx.widgets.images.get(source));
    let board = move |scale: f32| -> El<'a> {
        let prims = paint::layout(&w.marks, w.width, w.height, w.line_width());
        iced::widget::canvas(Board {
            canvas: w,
            seq,
            scale,
            background,
            prims,
            font: style.font,
            bold: style.weighted(ui::Weight::Bold, false),
        })
        .width(Length::Fixed((w.width * scale).max(1.0)))
        .height(Length::Fixed((w.height * scale).max(1.0)))
        .into()
    };
    match w.zoom {
        Some(zoom) => board(zoom.clamp(0.01, 16.0)),
        None => responsive(move |room: Size| board(w.fit(room.width)))
            .height(Length::Shrink)
            .into(),
    }
}

fn color([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb8(r, g, b)
}

impl Board<'_> {
    fn message(&self, handler: Option<ui::HandlerId>, value: EventValue) -> Option<KitMessage> {
        handler.map(|handler| {
            KitMessage::Ui(UiEvent {
                view: self.seq,
                handler,
                value: Some(value),
            })
        })
    }

    /// A point of the canvas, in logical pixels from its corner, in its
    /// units, held inside it.
    fn units(&self, at: Point) -> ui::Point {
        ui::Point::new(
            (at.x / self.scale).clamp(0.0, self.canvas.width),
            (at.y / self.scale).clamp(0.0, self.canvas.height),
        )
    }

    /// The mark a drag draws, so far.
    fn drafted(&self, points: &[ui::Point]) -> Option<Mark> {
        let (first, last) = (*points.first()?, *points.last()?);
        let (x, y) = (first.x.min(last.x), first.y.min(last.y));
        let (w, h) = ((last.x - first.x).abs(), (last.y - first.y).abs());
        let mark = match self.canvas.tool? {
            Tool::Line => Mark::line(first, last),
            Tool::Arrow => Mark::arrow(first, last),
            Tool::Rect => Mark::rect(x, y, w, h),
            Tool::Ellipse => Mark::ellipse(x, y, w, h),
            Tool::Free => Mark::path(points.iter().copied()),
            Tool::Point => return None,
        };
        let mark = mark.ink(self.canvas.ink);
        Some(match self.canvas.stroke {
            Some(stroke) => mark.width(stroke),
            None => mark,
        })
    }

    fn draw_prims(&self, frame: &mut Frame, prims: &[Prim]) {
        let k = self.scale;
        let at = |point: &ui::Point| Point::new(point.x * k, point.y * k);
        let round = |width: f32, rgb: [u8; 3]| Stroke {
            width: (width * k).max(0.5),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default().with_color(color(rgb))
        };
        for prim in prims {
            match prim {
                Prim::Polyline { points, width, rgb } => {
                    let line = Path::new(|path| {
                        path.move_to(at(&points[0]));
                        for point in &points[1..] {
                            path.line_to(at(point));
                        }
                    });
                    frame.stroke(&line, round(*width, *rgb));
                }
                Prim::Rect {
                    x,
                    y,
                    w,
                    h,
                    stroke,
                    fill,
                } => {
                    let (x, w) = if *w < 0.0 { (x + w, -w) } else { (*x, *w) };
                    let (y, h) = if *h < 0.0 { (y + h, -h) } else { (*y, *h) };
                    let shape = Path::rectangle(Point::new(x * k, y * k), Size::new(w * k, h * k));
                    if let Some(fill) = fill {
                        frame.fill(&shape, color(*fill));
                    }
                    if let Some((width, rgb)) = stroke {
                        let mut edge = round(*width, *rgb);
                        edge.line_cap = LineCap::Square;
                        edge.line_join = LineJoin::Miter;
                        frame.stroke(&shape, edge);
                    }
                }
                Prim::Ellipse {
                    cx,
                    cy,
                    rx,
                    ry,
                    stroke,
                    fill,
                } => {
                    let shape = Path::new(|path| {
                        path.ellipse(canvas::path::arc::Elliptical {
                            center: Point::new(cx * k, cy * k),
                            radii: Vector::new(rx * k, ry * k),
                            rotation: iced::Radians(0.0),
                            start_angle: iced::Radians(0.0),
                            end_angle: iced::Radians(TAU),
                        })
                    });
                    if let Some(fill) = fill {
                        frame.fill(&shape, color(*fill));
                    }
                    if let Some((width, rgb)) = stroke {
                        frame.stroke(&shape, round(*width, *rgb));
                    }
                }
                Prim::Text {
                    x,
                    y,
                    text: words,
                    size,
                    bold,
                    rgb,
                } => frame.fill_text(canvas::Text {
                    content: words.clone(),
                    position: Point::new(x * k, y * k),
                    color: color(*rgb),
                    size: Pixels(size * k),
                    font: if *bold { self.bold } else { self.font },
                    align_x: text::Alignment::Left,
                    align_y: Vertical::Center,
                    ..canvas::Text::default()
                }),
            }
        }
    }
}

impl canvas::Program<KitMessage> for Board<'_> {
    type State = Drag;

    fn update(
        &self,
        state: &mut Drag,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<KitMessage>> {
        // Without a zoom, the kit is told the scale it is drawn at, once for
        // each: what its zoom control says for "fit".
        if self.canvas.zoom.is_none() && state.told != Some(self.scale) {
            state.told = Some(self.scale);
            if let Some(told) =
                self.message(self.canvas.on_fit, EventValue::Number(self.scale as f64))
            {
                return Some(Action::publish(told));
            }
        }
        let drawing = self.canvas.on_draw.is_some() || self.canvas.on_point.is_some();
        if !drawing {
            return None;
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_in(bounds)?;
                let point = self.units(at);
                if self.canvas.tool == Some(Tool::Point) {
                    return self
                        .message(self.canvas.on_point, EventValue::Point(point))
                        .map(|told| Action::publish(told).and_capture());
                }
                *state = Drag {
                    points: vec![point],
                    pressed: Some(at),
                    moved: false,
                    told: state.told,
                };
                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let pressed = state.pressed?;
                let at = cursor.position_from(bounds.position())?;
                if !state.moved && at.distance(pressed) >= DRAG_SLOP {
                    state.moved = true;
                }
                let point = self.units(at);
                match self.canvas.tool {
                    Some(Tool::Free) => {
                        let last = *state.points.last()?;
                        let far = Point::new(last.x * self.scale, last.y * self.scale)
                            .distance(Point::new(point.x * self.scale, point.y * self.scale));
                        if far >= STROKE_STEP {
                            state.points.push(point);
                        }
                    }
                    _ => {
                        state.points.truncate(1);
                        state.points.push(point);
                    }
                }
                Some(Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.pressed?;
                let drag = std::mem::take(&mut state.points);
                let moved = state.moved;
                *state = Drag {
                    told: state.told,
                    ..Drag::default()
                };
                let told = if moved {
                    self.drafted(&drag)
                        .and_then(|mark| self.message(self.canvas.on_draw, EventValue::Mark(mark)))
                } else {
                    self.message(self.canvas.on_point, EventValue::Point(drag[0]))
                };
                Some(match told {
                    Some(told) => Action::publish(told).and_capture(),
                    None => Action::request_redraw(),
                })
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Drag,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let whole = Rectangle::new(Point::ORIGIN, bounds.size());
        // White where there is no picture, as the file written is.
        frame.fill(&Path::rectangle(Point::ORIGIN, bounds.size()), Color::WHITE);
        match self.background {
            Some(Picture::Raster(handle)) => frame.draw_image(whole, handle),
            Some(Picture::Vector(handle)) => frame.draw_svg(whole, handle),
            _ => {}
        }
        self.draw_prims(&mut frame, &self.prims);
        if state.moved
            && let Some(draft) = self.drafted(&state.points)
        {
            let prims = paint::layout(
                &[draft],
                self.canvas.width,
                self.canvas.height,
                self.canvas.line_width(),
            );
            self.draw_prims(&mut frame, &prims);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Drag,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let drawing = self.canvas.on_draw.is_some() || self.canvas.on_point.is_some();
        if drawing && (state.pressed.is_some() || cursor.is_over(bounds)) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}
