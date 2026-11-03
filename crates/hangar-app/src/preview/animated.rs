//! An animated picture in the flow of a page, a GIF or WebP in a Markdown
//! file (`image_stage.rs` plays the ones a viewer shows alone).
//!
//! It is as large as the picture, or scaled down to the width the page has,
//! as `max-width: 100%` did. It asks to be drawn again only while some of it
//! is on screen: a page scrolled past it, another tab or a minimised window
//! does not run it.

use std::sync::Arc;

use iced::advanced::image::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, renderer};
use iced::{Element, Event, Length, Rectangle, Size, mouse, window};

use super::decode::{Animation, Clock};

pub struct Inline {
    animation: Arc<Animation>,
}

impl Inline {
    pub fn new(animation: Arc<Animation>) -> Self {
        Inline { animation }
    }
}

/// The size a picture of `natural` is drawn at where `available` is the most
/// room it has: never larger than itself, scaled down to the width.
pub fn fitted(natural: Size, available: Size) -> Size {
    if natural.width <= 0.0 || natural.height <= 0.0 {
        return Size::ZERO;
    }
    let scale = (available.width / natural.width)
        .min(available.height / natural.height)
        .min(1.0);
    Size::new(natural.width * scale, natural.height * scale)
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Inline {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Clock>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Clock::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let natural = Size::new(self.animation.width as f32, self.animation.height as f32);
        layout::Node::new(fitted(natural, limits.max()))
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
            let clock = tree.state.downcast_mut::<Clock>();
            shell.request_redraw_at(clock.tick(*now, &self.animation.timeline));
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
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) {
            return;
        }
        let clock = tree.state.downcast_ref::<Clock>();
        let frames = &self.animation.frames;
        let frame = &frames[clock.frame.min(frames.len() - 1)];
        renderer.draw_image(
            iced::advanced::image::Image::new(frame.handle.clone()),
            bounds,
            bounds,
        );
    }
}

impl<'a, M: 'a> From<Inline> for Element<'a, M> {
    fn from(inline: Inline) -> Self {
        Element::new(inline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_never_drawn_larger_than_it_is() {
        let size = fitted(Size::new(100.0, 50.0), Size::new(800.0, 600.0));
        assert_eq!(size, Size::new(100.0, 50.0));
    }

    #[test]
    fn a_picture_wider_than_the_page_is_scaled_down_with_its_proportions() {
        let size = fitted(Size::new(1000.0, 500.0), Size::new(400.0, f32::INFINITY));
        assert_eq!(size, Size::new(400.0, 200.0));
        // And held by the height when that is what is short.
        let size = fitted(Size::new(1000.0, 500.0), Size::new(f32::INFINITY, 100.0));
        assert_eq!(size, Size::new(200.0, 100.0));
    }

    #[test]
    fn a_picture_without_a_size_takes_no_room() {
        assert_eq!(
            fitted(Size::new(0.0, 10.0), Size::new(100.0, 100.0)),
            Size::ZERO
        );
    }
}
