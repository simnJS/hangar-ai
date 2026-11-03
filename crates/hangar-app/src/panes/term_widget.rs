//! hangar-term's widget over a terminal that lives behind a lock.
//!
//! `TerminalView` borrows a `&Terminal` for the life of the view. A pane's
//! terminal is shared with its reader thread, which feeds it, and with the
//! engines, which read and paste into it (pty_link.rs, hub.rs), so the view
//! cannot hold a plain reference for a whole frame. This widget holds the
//! link instead and, in each call iced makes (layout, an event, a draw),
//! locks the terminal, builds the `TerminalView` over the guard and hands
//! the call on. Building one is a handful of fields; what the widget keeps
//! between frames (the measured cell, the cached rows, a composition in
//! progress) lives in its tree state, which is the inner widget's own.

use hangar_term::{KeyPress, TerminalView, terminal_view};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Shell, Widget};
use iced::{Element, Event, Length, Padding, Rectangle, Size, mouse};

use crate::message::Message;
use crate::panes::Msg;
use crate::panes::pty_link::PaneLink;
use crate::state::PaneId;

type Inner<'b> = TerminalView<'b, Message, iced::Theme, iced::Renderer>;

/// Whether the app's keymap takes a key before the terminal sees it
/// (`keymap::terminal_filter`).
pub type KeyFilter<'a> = Box<dyn Fn(&KeyPress) -> bool + 'a>;

/// A pane's terminal, drawn.
pub struct PaneTerminal<'a> {
    link: &'a PaneLink,
    pane: &'a PaneId,
    padding: Padding,
    filter: KeyFilter<'a>,
}

/// The terminal of `link`. `filter` says which keys the app's keymap takes
/// before the terminal sees them (hangar-term's `key_filter`): those are
/// neither typed nor captured, and reach the dispatcher.
pub fn pane_terminal<'a>(
    link: &'a PaneLink,
    pane: &'a PaneId,
    padding: f32,
    filter: KeyFilter<'a>,
) -> PaneTerminal<'a> {
    PaneTerminal {
        link,
        pane,
        padding: Padding::new(padding),
        filter,
    }
}

impl PaneTerminal<'_> {
    /// Runs `f` on the inner widget, built over the locked terminal.
    fn with<R>(&self, f: impl FnOnce(&mut Inner<'_>) -> R) -> R {
        let guard = self.link.lock();
        let pane = self.pane;
        let filter = &self.filter;
        let mut view: Inner<'_> = terminal_view(&guard, move |action| {
            Message::Panes(Msg::Term(pane.clone(), action))
        })
        .padding(self.padding)
        .key_filter(move |press| filter(press));
        f(&mut view)
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for PaneTerminal<'_> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        self.with(|view| Widget::<Message, iced::Theme, iced::Renderer>::tag(view))
    }

    fn state(&self) -> tree::State {
        self.with(|view| Widget::<Message, iced::Theme, iced::Renderer>::state(view))
    }

    fn children(&self) -> Vec<Tree> {
        self.with(|view| Widget::<Message, iced::Theme, iced::Renderer>::children(view))
    }

    fn diff(&self, tree: &mut Tree) {
        self.with(|view| Widget::<Message, iced::Theme, iced::Renderer>::diff(view, tree));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.with(|view| view.layout(tree, renderer, limits))
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.with(|view| view.operate(tree, layout, renderer, operation));
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // A key a widget before the terminal already used (a text field with
        // the keyboard, the search bar over it) is not the program's too:
        // iced hands every key to every widget, and the terminal's own
        // handling does not ask whether one was spent.
        if matches!(event, Event::Keyboard(_) | Event::InputMethod(_)) && shell.is_event_captured()
        {
            return;
        }
        self.with(|view| {
            view.update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            )
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.with(|view| view.mouse_interaction(tree, layout, cursor, viewport, renderer))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.with(|view| view.draw(tree, renderer, theme, style, layout, cursor, viewport));
    }
}

impl<'a> From<PaneTerminal<'a>> for Element<'a, Message> {
    fn from(widget: PaneTerminal<'a>) -> Self {
        Element::new(widget)
    }
}
