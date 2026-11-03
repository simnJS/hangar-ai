//! A strip of tabs (`.model-tabs`): document tabs along the top of a panel,
//! the one in front joined to the panel under it.
//!
//! ```ignore
//! use hangar_ui::tabs::{self, Tab};
//!
//! let items = vec![
//!     Tab::new(1, "scene.glb").icon(Icon::Cube).closable(),
//!     Tab::new(2, "report.pdf").icon(Icon::Table).closable(),
//! ];
//! tabs::strip(t, &items, &self.active, Msg::Pick).on_close(Msg::CloseTab)
//! ```
//!
//! A pressed cross closes the tab and does not also select it. The strip
//! scrolls sideways when the tabs do not fit, without a scrollbar, as the web
//! app's did.

use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{Theme, style};
use iced::widget::{button, container, mouse_area, row, rule, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::icon::{Icon, icon};

/// One tab. `Id` is whatever the app tells its tabs apart by.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab<Id> {
    pub id: Id,
    pub label: String,
    pub icon: Option<Icon>,
    pub closable: bool,
}

impl<Id> Tab<Id> {
    pub fn new(id: Id, label: impl Into<String>) -> Tab<Id> {
        Tab {
            id,
            label: label.into(),
            icon: None,
            closable: false,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Tab<Id> {
        self.icon = Some(icon);
        self
    }

    /// Shows a cross at its end.
    pub fn closable(mut self) -> Tab<Id> {
        self.closable = true;
        self
    }
}

#[must_use]
pub struct Strip<'a, Id, M> {
    theme: &'a Theme,
    items: &'a [Tab<Id>],
    active: &'a Id,
    on_select: Box<dyn Fn(Id) -> M + 'a>,
    on_close: Option<Box<dyn Fn(Id) -> M + 'a>>,
    height: f32,
}

/// A strip of `items`, `active` in front; `on_select` gets the id of the one
/// pressed.
pub fn strip<'a, Id, M>(
    t: &'a Theme,
    items: &'a [Tab<Id>],
    active: &'a Id,
    on_select: impl Fn(Id) -> M + 'a,
) -> Strip<'a, Id, M> {
    Strip {
        theme: t,
        items,
        active,
        on_select: Box::new(on_select),
        on_close: None,
        height: size::PANE_HEAD_HEIGHT,
    }
}

impl<'a, Id, M> Strip<'a, Id, M> {
    /// Sent with the id of a closable tab whose cross was pressed.
    pub fn on_close(mut self, on_close: impl Fn(Id) -> M + 'a) -> Self {
        self.on_close = Some(Box::new(on_close));
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl<'a, Id, M> From<Strip<'a, Id, M>> for Element<'a, M>
where
    Id: Clone + PartialEq + 'a,
    M: Clone + 'a,
{
    fn from(strip: Strip<'a, Id, M>) -> Self {
        let t = strip.theme;
        let mut tabs = row![].align_y(Alignment::Center).height(Length::Fill);
        for tab in strip.items {
            let front = &tab.id == strip.active;
            let mut content = row![].spacing(space::SM).align_y(Alignment::Center);
            if let Some(which) = tab.icon {
                content = content.push(icon(which, 12.0));
            }
            content = content.push(text(tab.label.clone()).size(font_size::UI));
            if let (true, Some(on_close)) = (tab.closable, &strip.on_close) {
                content = content.push(
                    mouse_area(container(icon(Icon::Close, 10.0)).padding(space::XS))
                        .on_press(on_close(tab.id.clone())),
                );
            }
            tabs = tabs.push(
                button(container(content).center_y(Length::Fill))
                    .padding(Padding {
                        left: space::LG,
                        right: space::SM,
                        ..Padding::ZERO
                    })
                    .height(Length::Fill)
                    .style(style::button::tab_strip(t, front))
                    .on_press((strip.on_select)(tab.id.clone())),
            );
            tabs = tabs.push(rule::vertical(1).style(style::rule::hairline(t)));
        }
        let strip_row = scrollable(tabs)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ))
            .height(Length::Fill)
            .style(style::scrollable::default(t));
        container(strip_row)
            .width(Length::Fill)
            .height(strip.height)
            .style(style::container::pane_head(t, false))
            .into()
    }
}

#[cfg(test)]
mod tests;
