//! A sparkline, drawn on a canvas: the one widget made of a path rather
//! than of boxes.

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};

use crate::message::KitMessage;

pub(crate) struct Spark {
    pub values: Vec<f32>,
    pub color: Color,
    pub fill: bool,
    pub min: Option<f32>,
    pub max: Option<f32>,
}

impl canvas::Program<KitMessage> for Spark {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let values: Vec<f32> = self
            .values
            .iter()
            .copied()
            .filter(|value| value.is_finite())
            .collect();
        if values.len() >= 2 {
            let low = self
                .min
                .unwrap_or_else(|| values.iter().copied().fold(f32::INFINITY, f32::min));
            let high = self
                .max
                .unwrap_or_else(|| values.iter().copied().fold(f32::NEG_INFINITY, f32::max));
            let span = if (high - low).abs() < f32::EPSILON {
                1.0
            } else {
                high - low
            };
            // A line's width of room at the top and the bottom, so neither
            // end is cut.
            let inset = 1.5;
            let height = (bounds.height - 2.0 * inset).max(1.0);
            let step = bounds.width / (values.len() - 1) as f32;
            let point = |index: usize, value: f32| {
                let y = inset + height * (1.0 - ((value - low) / span).clamp(0.0, 1.0));
                Point::new(index as f32 * step, y)
            };
            let line = Path::new(|builder| {
                builder.move_to(point(0, values[0]));
                for (index, value) in values.iter().enumerate().skip(1) {
                    builder.line_to(point(index, *value));
                }
            });
            if self.fill {
                let area = Path::new(|builder| {
                    builder.move_to(Point::new(0.0, bounds.height));
                    for (index, value) in values.iter().enumerate() {
                        builder.line_to(point(index, *value));
                    }
                    builder.line_to(Point::new(bounds.width, bounds.height));
                    builder.close();
                });
                frame.fill(
                    &area,
                    Color {
                        a: 0.16,
                        ..self.color
                    },
                );
            }
            frame.stroke(
                &line,
                Stroke::default().with_color(self.color).with_width(1.5),
            );
        }
        vec![frame.into_geometry()]
    }
}
