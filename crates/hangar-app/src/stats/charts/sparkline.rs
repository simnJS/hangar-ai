//! A trend at a glance: the line in a quiet grey, today's point in the accent
//! (`Sparkline`).

use hangar_theme::Theme;
use iced::widget::canvas::{self, Canvas, LineCap, LineJoin, Path, Stroke};
use iced::{Element, Length, Point, Rectangle, Renderer, mouse};

const WIDTH: f32 = 88.0;
const HEIGHT: f32 = 26.0;
/// The room the end point's dot needs beyond the line.
const MARGIN: f32 = 2.0;

pub struct Sparkline {
    theme: &'static Theme,
    values: Vec<f64>,
}

/// Where each value sits in the box: the first at the left edge, the last at
/// the right, the highest three pixels under the top.
pub fn points(values: &[f64]) -> Vec<(f32, f32)> {
    if values.len() < 2 {
        return Vec::new();
    }
    let max = values.iter().copied().fold(0.0, f64::max);
    let step = WIDTH / (values.len() - 1) as f32;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let y = if max > 0.0 {
                HEIGHT - 3.0 - (*value / max) as f32 * (HEIGHT - 6.0)
            } else {
                HEIGHT - 3.0
            };
            (index as f32 * step, y)
        })
        .collect()
}

/// The sparkline of `values`, or nothing to draw below two of them.
pub fn sparkline<'a, M: 'a>(theme: &'static Theme, values: Vec<f64>) -> Element<'a, M> {
    if values.len() < 2 {
        return iced::widget::Space::new().into();
    }
    Canvas::new(Sparkline { theme, values })
        .width(WIDTH + 2.0 * MARGIN)
        .height(Length::Fixed(HEIGHT))
        .into()
}

impl<M> canvas::Program<M> for Sparkline {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let colors = &self.theme.colors;
        let points: Vec<Point> = points(&self.values)
            .into_iter()
            .map(|(x, y)| Point::new(x + MARGIN, y))
            .collect();
        let line = Path::new(|path| {
            for (at, point) in points.iter().enumerate() {
                if at == 0 {
                    path.move_to(*point);
                } else {
                    path.line_to(*point);
                }
            }
        });
        frame.stroke(
            &line,
            Stroke::default()
                .with_color(colors.viz_line)
                .with_width(1.5)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
        );
        if let Some(last) = points.last() {
            let dot = Path::circle(*last, 2.75);
            frame.fill(&dot, colors.viz_4);
            frame.stroke(
                &dot,
                Stroke::default().with_color(colors.card).with_width(2.0),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_two_values_to_be_a_line() {
        assert!(points(&[]).is_empty());
        assert!(points(&[3.0]).is_empty());
    }

    #[test]
    fn spans_the_box_with_the_highest_value_near_the_top() {
        let at = points(&[0.0, 5.0, 10.0]);
        assert_eq!(at.len(), 3);
        assert_eq!(at[0], (0.0, HEIGHT - 3.0));
        assert_eq!(at[2], (WIDTH, 3.0));
        assert!((at[1].0 - WIDTH / 2.0).abs() < 1e-4);
    }

    #[test]
    fn an_all_zero_series_lies_flat_on_the_baseline() {
        for (_, y) in points(&[0.0, 0.0, 0.0]) {
            assert_eq!(y, HEIGHT - 3.0);
        }
    }
}
