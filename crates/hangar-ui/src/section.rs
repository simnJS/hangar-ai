//! A foldable block of a side panel (`InspectorSection.tsx`).
//!
//! The title row is one button that says whether the block is open; what sits
//! at its right end (a count, a total) is part of the title, and an action
//! ("Open", "View diff") sits beside the button and not in it, so each stays
//! one control.
//!
//! ```ignore
//! use hangar_ui::{badge, button, section};
//!
//! section::section(t, "Changes", self.folded.contains("changes"), Msg::Toggle("changes"))
//!     .meta("12")
//!     .action(button::link(t, "View diff", Msg::OpenDiff))
//!     .body(column![/* the rows */])
//! ```
//!
//! The block is separated from the next by a hairline under it; the last of a
//! panel asks for none with [`Section::last`].

use std::f32::consts::FRAC_PI_2;

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{button, column, container, row, rule, space as iced_space, text};
use iced::{Alignment, Element, Length, Padding};

use crate::icon::{Icon, icon};

/// A block of a panel, open or folded.
#[must_use]
pub struct Section<'a, M> {
    theme: &'a Theme,
    title: String,
    collapsed: bool,
    on_toggle: M,
    meta: Option<Element<'a, M>>,
    action: Option<Element<'a, M>>,
    body: Option<Element<'a, M>>,
    last: bool,
    padding: f32,
}

/// A block titled `title`. `on_toggle` is sent by its title row.
pub fn section<'a, M: Clone + 'a>(
    t: &'a Theme,
    title: impl Into<String>,
    collapsed: bool,
    on_toggle: M,
) -> Section<'a, M> {
    Section {
        theme: t,
        title: title.into(),
        collapsed,
        on_toggle,
        meta: None,
        action: None,
        body: None,
        last: false,
        padding: space::XL,
    }
}

impl<'a, M: Clone + 'a> Section<'a, M> {
    /// A count or a total at the right end of the title, read with it.
    pub fn meta(mut self, meta: impl Into<String>) -> Self {
        let t = self.theme;
        self.meta = Some(
            text(meta.into())
                .size(font_size::LABEL)
                .font(fonts::mono())
                .style(style::text::subtle(t))
                .into(),
        );
        self
    }

    /// The same place, for a total that is more than a word: coloured counts
    /// ("+12 −3"). The caller sets it as it wants it read.
    pub fn meta_element(mut self, meta: impl Into<Element<'a, M>>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    /// A link-like control at the far right of the title row. Hidden while
    /// the block is folded.
    pub fn action(mut self, action: impl Into<Element<'a, M>>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// What the block holds. Not built into the tree while it is folded.
    pub fn body(mut self, body: impl Into<Element<'a, M>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// No hairline under it: the last block of a panel.
    pub fn last(mut self) -> Self {
        self.last = true;
        self
    }

    /// The space at both sides (the panel's own padding, 12 by default).
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }
}

impl<'a, M: Clone + 'a> From<Section<'a, M>> for Element<'a, M> {
    fn from(s: Section<'a, M>) -> Self {
        let t = s.theme;
        let c = &t.colors;

        // The chevron points down while the block is open and turns a quarter
        // to point at the title when it is folded.
        let chevron = icon(Icon::ChevronDown, 10.0)
            .color(c.fg_subtle)
            .rotate(if s.collapsed { -FRAC_PI_2 } else { 0.0 });
        let mut title = row![
            chevron,
            text(s.title)
                .size(font_size::CAPTION)
                .font(fonts::ui_semibold()),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);
        if let Some(meta) = s.meta {
            title = title.push(iced_space::horizontal());
            title = title.push(meta);
        }
        let toggle = button(title)
            .padding(Padding::from([0.0, space::XXS]))
            .width(Length::Fill)
            .height(18.0)
            .style(style::button::plain(t))
            .on_press(s.on_toggle);
        let mut head = row![toggle].spacing(space::SM).align_y(Alignment::Center);
        if let (Some(action), false) = (s.action, s.collapsed) {
            head = head.push(action);
        }

        let bottom = if s.collapsed { 9.0 } else { 12.0 };
        let mut inside = column![head].spacing(8);
        if let (Some(body), false) = (s.body, s.collapsed) {
            inside = inside.push(body);
        }
        let block = container(inside).padding(Padding {
            top: 9.0,
            right: s.padding,
            bottom,
            left: s.padding,
        });
        if s.last {
            block.into()
        } else {
            column![block, rule::horizontal(1).style(style::rule::hairline(t))].into()
        }
    }
}
