//! The bar of a file on its way: a segment that runs along a track, over and
//! over, for as long as it is on screen (`.model-progress.is-indeterminate`,
//! `@keyframes model-travel`).
//!
//! A bar that stood still said "something is on its way" and no more; one
//! that runs also says it has not stopped. It asks to be drawn again a frame
//! at a time while it is in the window, and not at all when it is not.

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell, Widget};
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Size, mouse, window};

/// How long one run from one end to the other takes.
pub const PERIOD: Duration = Duration::from_millis(1100);
/// How wide the segment is, as a share of the track (`width: 34%`).
pub const SEGMENT: f32 = 0.34;
/// The next frame, when the segment is moving: about sixty a second.
const FRAME: Duration = Duration::from_millis(16);

/// Where the segment's left edge is, as a share of the track, `phase` of the
/// way through a run (0 to 1): from entirely before the track to entirely
/// past it (`translateX(-100%)` to `translateX(300%)` of its own width),
/// easing in and out.
pub fn left_edge(phase: f32) -> f32 {
    let phase = phase.clamp(0.0, 1.0);
    // Smoothstep: slow at the ends, quick in the middle.
    let eased = phase * phase * (3.0 - 2.0 * phase);
    -SEGMENT + eased * (SEGMENT * 4.0)
}

pub struct Travelling {
    track: Color,
    segment: Color,
    girth: f32,
}

impl Travelling {
    pub fn new(track: Color, segment: Color) -> Self {
        Travelling {
            track,
            segment,
            girth: 3.0,
        }
    }
}

#[derive(Default)]
struct State {
    started: Option<Instant>,
    phase: f32,
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Travelling {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.girth))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, Length::Fixed(self.girth))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event
            && layout.bounds().intersects(viewport)
        {
            let state = tree.state.downcast_mut::<State>();
            let started = *state.started.get_or_insert(*now);
            let into = now.saturating_duration_since(started).as_secs_f32() % PERIOD.as_secs_f32();
            state.phase = into / PERIOD.as_secs_f32();
            shell.request_redraw_at(*now + FRAME);
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
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let radius = self.girth / 2.0;
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border {
                    radius: radius.into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(self.track),
        );
        // The segment is cut where the track ends, as `overflow: hidden` did.
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle {
                        x: bounds.x + left_edge(state.phase) * bounds.width,
                        width: SEGMENT * bounds.width,
                        ..bounds
                    },
                    border: Border {
                        radius: radius.into(),
                        ..Border::default()
                    },
                    ..Quad::default()
                },
                Background::Color(self.segment),
            );
        });
    }
}

impl<'a, M: 'a> From<Travelling> for Element<'a, M> {
    fn from(bar: Travelling) -> Self {
        Element::new(bar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_enters_from_before_the_track_and_leaves_past_its_end() {
        assert!(
            (left_edge(0.0) + SEGMENT).abs() < 1e-6,
            "starts out of sight"
        );
        assert!((left_edge(1.0) - SEGMENT * 3.0).abs() < 1e-5);
        assert!(
            left_edge(1.0) > 1.0,
            "its left edge is past the track's end"
        );
    }

    #[test]
    fn the_segment_only_ever_moves_on_and_is_quickest_in_the_middle() {
        let mut last = left_edge(0.0);
        let mut steps = Vec::new();
        for n in 1..=20 {
            let now = left_edge(n as f32 / 20.0);
            assert!(now >= last, "it never goes back");
            steps.push(now - last);
            last = now;
        }
        assert!(steps[10] > steps[0] && steps[10] > steps[19], "{steps:?}");
    }

    #[test]
    fn a_phase_outside_a_run_is_held_to_its_ends() {
        assert_eq!(left_edge(-3.0), left_edge(0.0));
        assert_eq!(left_edge(9.0), left_edge(1.0));
    }
}
