//! A widget whose layout is kept while what it shows has not changed.
//!
//! iced lays the whole window out again after every message the app
//! handles, and while the panes print (hidden behind the chat view, their
//! output still comes in many times a second) that is the conversation's
//! every message laid out again, its Markdown measured, for nothing: the
//! conversation changes on a read, a click, a colouring, not on a pane's
//! output. `Settled` wraps the conversation's messages with a `key` that
//! changes whenever what they show does (`Chat::revision` and what else the
//! words come from), and hands back the layout it kept while the key and the
//! room it is given stay the same. Everything else (the drawing, the events,
//! the overlays) goes to the content as it is, and a content that asks to be
//! laid out again (`Shell::invalidate_layout`) is.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse};

/// `content`, laid out again only when `key` or its room changes.
pub fn settled<'a, Message, Theme, Renderer>(
    key: u64,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Settled<'a, Message, Theme, Renderer> {
    Settled {
        key,
        content: content.into(),
    }
}

pub struct Settled<'a, Message, Theme, Renderer> {
    key: u64,
    content: Element<'a, Message, Theme, Renderer>,
}

/// What is kept between frames: the layout made, with what it was made for.
#[derive(Debug, Default)]
struct Kept {
    key: Option<u64>,
    limits: Option<layout::Limits>,
    node: Option<layout::Node>,
    /// Layouts made, for the tests.
    made: u64,
}

impl Kept {
    fn holds(&self, key: u64, limits: &layout::Limits) -> Option<&layout::Node> {
        (self.key == Some(key) && self.limits.as_ref() == Some(limits))
            .then_some(self.node.as_ref())
            .flatten()
    }
}

/// How many layouts the `Settled` at the root of `tree` made: what the tests
/// count.
#[cfg(test)]
pub fn layouts_made(tree: &Tree) -> u64 {
    tree.state.downcast_ref::<Kept>().made
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Settled<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Kept>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Kept::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.content.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
        let kept = tree.state.downcast_mut::<Kept>();
        if kept.key != Some(self.key) {
            kept.node = None;
        }
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let kept = tree.state.downcast_mut::<Kept>();
        if let Some(node) = kept.holds(self.key, limits) {
            return node.clone();
        }
        let node = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let kept = tree.state.downcast_mut::<Kept>();
        kept.key = Some(self.key);
        kept.limits = Some(*limits);
        kept.node = Some(node.clone());
        kept.made += 1;
        node
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        // Asked by the content, or by a widget beside it (the shell is the
        // window's): either way, the next layout is made, not kept.
        if shell.is_layout_invalid() {
            tree.state.downcast_mut::<Kept>().node = None;
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Settled<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(settled: Settled<'a, Message, Theme, Renderer>) -> Self {
        Element::new(settled)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use iced::advanced::renderer::Quad;
    use iced::advanced::{Shell, clipboard, image};
    use iced::{Background, Point, Size, Transformation};

    use super::*;

    /// Draws nothing: iced's own such renderer is in its debug builds only.
    struct Nothing;

    impl renderer::Renderer for Nothing {
        fn start_layer(&mut self, _: Rectangle) {}
        fn end_layer(&mut self) {}
        fn start_transformation(&mut self, _: Transformation) {}
        fn end_transformation(&mut self) {}
        fn fill_quad(&mut self, _: Quad, _: impl Into<Background>) {}
        fn reset(&mut self, _: Rectangle) {}
        fn allocate_image(
            &mut self,
            _: &image::Handle,
            _: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
        ) {
        }
    }

    /// A widget that counts its layouts, and asks for one more on any event.
    struct Counted(Rc<Cell<u32>>);

    impl Widget<(), (), Nothing> for Counted {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Shrink)
        }

        fn layout(&mut self, _: &mut Tree, _: &Nothing, limits: &layout::Limits) -> layout::Node {
            self.0.set(self.0.get() + 1);
            layout::Node::new(Size::new(limits.max().width, 40.0))
        }

        fn draw(
            &self,
            _: &Tree,
            _: &mut Nothing,
            _: &(),
            _: &renderer::Style,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Rectangle,
        ) {
        }

        fn update(
            &mut self,
            _: &mut Tree,
            _: &Event,
            _: Layout<'_>,
            _: mouse::Cursor,
            _: &Nothing,
            _: &mut dyn Clipboard,
            shell: &mut Shell<'_, ()>,
            _: &Rectangle,
        ) {
            shell.invalidate_layout();
        }
    }

    /// One frame as iced makes it: the widget built again, reconciled with
    /// the tree, laid out.
    fn frame(tree: &mut Tree, layouts: &Rc<Cell<u32>>, key: u64, width: f32) -> layout::Node {
        let mut element: Element<'_, (), (), Nothing> =
            settled(key, Element::new(Counted(layouts.clone()))).into();
        tree.diff(&element);
        let limits = layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
        element.as_widget_mut().layout(tree, &Nothing, &limits)
    }

    #[test]
    fn the_layout_is_kept_while_the_key_and_the_room_stay_and_made_again_when_either_changes() {
        let layouts = Rc::new(Cell::new(0));
        let first: Element<'_, (), (), Nothing> =
            settled(1, Element::new(Counted(layouts.clone()))).into();
        let mut tree = Tree::new(&first);
        drop(first);

        let node = frame(&mut tree, &layouts, 1, 600.0);
        assert_eq!(node.size(), Size::new(600.0, 40.0));
        for _ in 0..5 {
            assert_eq!(frame(&mut tree, &layouts, 1, 600.0).size(), node.size());
        }
        assert_eq!(layouts.get(), 1, "the same key, the same room: kept");
        assert_eq!(layouts_made(&tree), 1);

        let _ = frame(&mut tree, &layouts, 2, 600.0);
        assert_eq!(layouts.get(), 2, "what it shows changed");
        let wider = frame(&mut tree, &layouts, 2, 800.0);
        assert_eq!(layouts.get(), 3, "the room changed");
        assert_eq!(wider.size(), Size::new(800.0, 40.0));
        let _ = frame(&mut tree, &layouts, 2, 800.0);
        assert_eq!(layouts.get(), 3);
    }

    #[test]
    fn a_content_that_asks_to_be_laid_out_again_is() {
        let layouts = Rc::new(Cell::new(0));
        let first: Element<'_, (), (), Nothing> =
            settled(7, Element::new(Counted(layouts.clone()))).into();
        let mut tree = Tree::new(&first);
        drop(first);
        let node = frame(&mut tree, &layouts, 7, 600.0);
        assert_eq!(layouts.get(), 1);

        let mut element: Element<'_, (), (), Nothing> =
            settled(7, Element::new(Counted(layouts.clone()))).into();
        tree.diff(&element);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        element.as_widget_mut().update(
            &mut tree,
            &Event::Mouse(mouse::Event::CursorMoved {
                position: Point::ORIGIN,
            }),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &Nothing,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::with_size(Size::new(600.0, 400.0)),
        );
        drop(element);

        let _ = frame(&mut tree, &layouts, 7, 600.0);
        assert_eq!(layouts.get(), 2);
    }
}
