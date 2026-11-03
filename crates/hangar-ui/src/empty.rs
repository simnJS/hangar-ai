//! An empty state: what a list or a page says when it has nothing in it, and
//! what to do about it.
//!
//! ```ignore
//! use hangar_ui::{button, empty, Icon};
//!
//! empty::empty(t, Icon::Board, "No tasks yet")
//!     .description("Agents add tasks here as they plan the work.")
//!     .action(button::primary(t, "New task").on_press(Msg::NewTask))
//! ```

use hangar_theme::tokens::space;
use hangar_theme::{Theme, style};
use iced::widget::{column, container};
use iced::{Alignment, Element, Length, Padding};

use crate::icon::{Icon, icon};
use crate::text as typography;

#[must_use]
pub struct EmptyState<'a, M> {
    theme: &'a Theme,
    icon: Icon,
    title: String,
    description: Option<String>,
    action: Option<Element<'a, M>>,
}

/// An empty state with a glyph and a title.
pub fn empty<'a, M: 'a>(t: &'a Theme, icon: Icon, title: impl Into<String>) -> EmptyState<'a, M> {
    EmptyState {
        theme: t,
        icon,
        title: title.into(),
        description: None,
        action: None,
    }
}

impl<'a, M: 'a> EmptyState<'a, M> {
    /// A sentence under the title.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// The one thing to do about it: a button.
    pub fn action(mut self, action: impl Into<Element<'a, M>>) -> Self {
        self.action = Some(action.into());
        self
    }
}

impl<'a, M: 'a> From<EmptyState<'a, M>> for Element<'a, M> {
    fn from(e: EmptyState<'a, M>) -> Self {
        let t = e.theme;
        let mut stack = column![
            icon(e.icon, 28.0).color(t.colors.fg_subtle),
            typography::body(t, e.title),
        ]
        .spacing(space::LG)
        .align_x(Alignment::Center);
        if let Some(description) = e.description {
            stack = stack.push(
                typography::hint(t, description)
                    .align_x(iced::alignment::Horizontal::Center)
                    .width(Length::Shrink),
            );
        }
        if let Some(action) = e.action {
            stack = stack.push(container(action).padding(Padding {
                top: space::XS,
                ..Padding::ZERO
            }));
        }
        container(container(stack).max_width(320.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .padding(space::XXXL)
            .style(style::container::plain(t))
            .into()
    }
}
