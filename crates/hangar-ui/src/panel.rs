//! Panels: a header with its actions over a body, for the side panels and
//! the panes' bars.
//!
//! ```ignore
//! use hangar_ui::{button, panel, Icon};
//!
//! panel::panel(t, "Worktrees", column![/* the rows */])
//!     .icon(Icon::Branch)
//!     .sub("3")
//!     .action(button::icon_button(t, Icon::Plus).small().tip("New worktree").on_press(Msg::New))
//!
//! // Just the bar, for a pane's header:
//! panel::header(t, "Ben").focused(true).action(more_button)
//! ```

use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{column, container, row, rule, space as iced_space, text};
use iced::{Alignment, Element, Length, Padding};

use crate::icon::{Icon, icon};
use crate::text as typography;

/// A bar with a title at the left and actions at the right (`.pane__bar`).
#[must_use]
pub struct Header<'a, M> {
    theme: &'a Theme,
    title: String,
    icon: Option<Icon>,
    sub: Option<String>,
    leading: Vec<Element<'a, M>>,
    actions: Vec<Element<'a, M>>,
    focused: bool,
}

/// A header titled `title`.
pub fn header<'a, M: 'a>(t: &'a Theme, title: impl Into<String>) -> Header<'a, M> {
    Header {
        theme: t,
        title: title.into(),
        icon: None,
        sub: None,
        leading: Vec::new(),
        actions: Vec::new(),
        focused: false,
    }
}

impl<'a, M: 'a> Header<'a, M> {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// A word after the title in the muted text: a count, a path.
    pub fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    /// Something before the title (a state glyph).
    pub fn leading(mut self, leading: impl Into<Element<'a, M>>) -> Self {
        self.leading.push(leading.into());
        self
    }

    /// A control at the right end, in the order given.
    pub fn action(mut self, action: impl Into<Element<'a, M>>) -> Self {
        self.actions.push(action.into());
        self
    }

    /// A little lighter while the panel has the keyboard.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl<'a, M: 'a> From<Header<'a, M>> for Element<'a, M> {
    fn from(h: Header<'a, M>) -> Self {
        let t = h.theme;
        let mut line = row![].spacing(space::SM).align_y(Alignment::Center);
        for leading in h.leading {
            line = line.push(leading);
        }
        if let Some(which) = h.icon {
            line = line.push(icon(which, 13.0).color(t.colors.fg_muted));
        }
        line = line.push(
            text(h.title)
                .size(font_size::UI)
                .font(fonts::ui_semibold())
                .style(style::text::primary(t)),
        );
        if let Some(sub) = h.sub {
            line = line.push(typography::muted(t, sub).size(font_size::CAPTION_LG));
        }
        line = line.push(iced_space::horizontal());
        for action in h.actions {
            line = line.push(action);
        }
        container(line)
            .width(Length::Fill)
            .height(size::PANE_HEAD_HEIGHT)
            .padding(Padding::from([0.0, space::MD]))
            .align_y(Alignment::Center)
            .style(style::container::pane_head(t, h.focused))
            .into()
    }
}

/// A panel: a [`header`] over a hairline and a body, on the chrome.
#[must_use]
pub struct Panel<'a, M> {
    header: Header<'a, M>,
    body: Element<'a, M>,
    theme: &'a Theme,
}

pub fn panel<'a, M: 'a>(
    t: &'a Theme,
    title: impl Into<String>,
    body: impl Into<Element<'a, M>>,
) -> Panel<'a, M> {
    Panel {
        header: header(t, title),
        body: body.into(),
        theme: t,
    }
}

impl<'a, M: 'a> Panel<'a, M> {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.header = self.header.icon(icon);
        self
    }

    pub fn sub(mut self, sub: impl Into<String>) -> Self {
        self.header = self.header.sub(sub);
        self
    }

    pub fn action(mut self, action: impl Into<Element<'a, M>>) -> Self {
        self.header = self.header.action(action);
        self
    }
}

impl<'a, M: 'a> From<Panel<'a, M>> for Element<'a, M> {
    fn from(p: Panel<'a, M>) -> Self {
        let t = p.theme;
        container(column![
            Element::from(p.header),
            rule::horizontal(1).style(style::rule::sep(t)),
            p.body,
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::container::panel(t))
        .into()
    }
}
