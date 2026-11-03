//! A progress bar with nothing to measure against: a segment that runs
//! along its track, as `.kit-progress` without a value does in a page,
//! where a bar standing still would read as stuck.
//!
//! It asks the window for its next frame only while it is drawn, and at
//! thirty a second rather than every frame: a tab scanning for a minute
//! does not keep the machine busy for it.

use std::time::{Duration, Instant};

use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse, window};

use crate::message::KitMessage;

/// One run along the track.
const PERIOD: Duration = Duration::from_millis(1600);
const FRAME: Duration = Duration::from_millis(33);

pub(crate) struct Running {
    pub track: Color,
    pub bar: Color,
}

/// When the bar was first drawn, which its run is counted from.
pub(crate) struct Since(Instant);

impl Default for Since {
    fn default() -> Self {
        Since(Instant::now())
    }
}

impl canvas::Program<KitMessage> for Running {
    type State = Since;

    fn update(
        &self,
        _state: &mut Since,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<Action<KitMessage>> {
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                Some(Action::request_redraw_at(*now + FRAME))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Since,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let radius = bounds.height / 2.0;
        frame.fill(
            &Path::rounded_rectangle(Point::ORIGIN, bounds.size(), radius.into()),
            self.track,
        );
        let phase = (state.0.elapsed().as_secs_f32() / PERIOD.as_secs_f32()).fract();
        let length = bounds.width * 0.3;
        // From just off the start to just off the end, eased in and out.
        let eased = 0.5 - 0.5 * (phase * std::f32::consts::PI).cos();
        let start = -length + eased * (bounds.width + length);
        let (left, right) = (start.max(0.0), (start + length).min(bounds.width));
        if right > left {
            frame.fill(
                &Path::rounded_rectangle(
                    Point::new(left, 0.0),
                    Size::new(right - left, bounds.height),
                    radius.into(),
                ),
                self.bar,
            );
        }
        vec![frame.into_geometry()]
    }
}
