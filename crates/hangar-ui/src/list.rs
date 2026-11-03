//! Rows of a list: the navigator's workspaces, a settings page's entries.
//!
//! ```ignore
//! use hangar_ui::list;
//!
//! column(workspaces.iter().map(|ws| {
//!     list::row(t, row![glyph, text(&ws.name)].spacing(8), ws.id == self.active)
//!         .on_press(Msg::Activate(ws.id))
//!         .into()
//! }))
//! ```
//!
//! The selected row is on the selection's colour; the others light up under
//! the pointer and a little more when pressed.

use hangar_theme::tokens::{size, space};
use hangar_theme::{Theme, style};
use iced::widget::{button, container};
use iced::{Alignment, Element, Length, Padding};

/// A row of a list.
#[must_use]
pub struct Row<'a, M> {
    theme: &'a Theme,
    content: Element<'a, M>,
    selected: bool,
    on_press: Option<M>,
    height: f32,
    fit: bool,
    padding: Padding,
}

/// A row holding `content`, `selected` or not.
pub fn row<'a, M: 'a>(
    t: &'a Theme,
    content: impl Into<Element<'a, M>>,
    selected: bool,
) -> Row<'a, M> {
    Row {
        theme: t,
        content: content.into(),
        selected,
        on_press: None,
        height: size::NAV_ROW_HEIGHT,
        fit: false,
        padding: Padding::from([0.0, space::MD]),
    }
}

impl<'a, M: 'a> Row<'a, M> {
    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn on_press_maybe(mut self, message: Option<M>) -> Self {
        self.on_press = message;
        self
    }

    /// 28 px by default (`--nav-row`).
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// As tall as its content and its [`padding`](Row::padding), for a row
    /// whose content can wrap or hold several lines (a table's cells), which a
    /// fixed height would cut. [`height`](Row::height) is then not used.
    pub fn fit(mut self) -> Self {
        self.fit = true;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<Row<'a, M>> for Element<'a, M> {
    fn from(r: Row<'a, M>) -> Self {
        let content = container(r.content).align_y(Alignment::Center);
        let (content, outer) = if r.fit {
            (content.height(Length::Shrink), Length::Shrink)
        } else {
            (content.height(Length::Fill), Length::Fixed(r.height))
        };
        button(content)
            .padding(r.padding)
            .width(Length::Fill)
            .height(outer)
            .style(style::button::row(r.theme, r.selected))
            .on_press_maybe(r.on_press)
            .into()
    }
}
