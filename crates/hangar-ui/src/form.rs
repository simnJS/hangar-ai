//! The settings form: rows of a label and its control, grouped in cards under
//! a heading (`.set-row`, `.set-group`, `.set-section`).
//!
//! ```ignore
//! use hangar_ui::{form, input, Icon};
//!
//! form::section(t, "Appearance")
//!     .hint("How the window looks.")
//!     .group(vec![
//!         form::row(t, "Interface size", input::slider(t, 90.0..=130.0, s, Msg::Scale).display("100 %"))
//!             .hint("Scales every size in the window.")
//!             .modified(s != 100.0)
//!             .on_reset(Msg::ResetScale)
//!             .wide()
//!             .into(),
//!         form::row(t, "Reduce motion", input::switch(t, on).on_toggle(Msg::Motion)).into(),
//!     ])
//! ```
//!
//! A row has the label and an optional hint at the left and its control at
//! the right. A row whose value differs from its default shows a reset
//! button, in place, so that undoing a change never means finding the
//! setting's default somewhere else. Controls that take a width (a field, a
//! drop-down, a slider) ask for `.wide()` so that a page of them lines up
//! down its right edge.
//!
//! Also here: [`block`], for a control that is too big for a row, such as a
//! preview or a grid of themes, and [`group`], for the card around rows.

use hangar_theme::tokens::space;
use hangar_theme::{Theme, style};
use iced::widget::{column, container, rule};
use iced::{Alignment, Element, Length, Padding};

use crate::button::icon_button;
use crate::icon::Icon;
use crate::text as typography;

/// The width a row's control takes when it asks for [`Row::wide`].
const WIDE: f32 = 240.0;

/// A row of the form.
#[must_use]
pub struct Row<'a, M> {
    theme: &'a Theme,
    label: String,
    hint: Option<String>,
    control: Element<'a, M>,
    wide: bool,
    modified: bool,
    on_reset: Option<M>,
    reset_tip: String,
}

/// A row labelled `label` with `control` at its right.
pub fn row<'a, M: 'a>(
    t: &'a Theme,
    label: impl Into<String>,
    control: impl Into<Element<'a, M>>,
) -> Row<'a, M> {
    Row {
        theme: t,
        label: label.into(),
        hint: None,
        control: control.into(),
        wide: false,
        modified: false,
        on_reset: None,
        reset_tip: String::from("Reset to default"),
    }
}

impl<'a, M: 'a> Row<'a, M> {
    /// A line under the label saying what the setting does.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Gives the control the form's common width, 240 px.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    /// The value differs from its default: the reset button shows.
    pub fn modified(mut self, modified: bool) -> Self {
        self.modified = modified;
        self
    }

    /// Sent by the reset button, which only shows while the row is
    /// [`modified`](Row::modified).
    pub fn on_reset(mut self, message: M) -> Self {
        self.on_reset = Some(message);
        self
    }

    /// What the reset button's tip says, in the interface's language.
    pub fn reset_tip(mut self, words: impl Into<String>) -> Self {
        self.reset_tip = words.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<Row<'a, M>> for Element<'a, M> {
    fn from(r: Row<'a, M>) -> Self {
        let t = r.theme;
        let has_hint = r.hint.is_some();
        let mut text = column![typography::body(t, r.label)].spacing(2);
        if let Some(hint) = r.hint {
            text = text.push(typography::hint(t, hint));
        }
        let mut control = iced::widget::row![]
            .spacing(space::MD)
            .align_y(Alignment::Center);
        if let (true, Some(reset)) = (r.modified, r.on_reset) {
            control = control.push(
                icon_button(t, Icon::Reset)
                    .small()
                    .icon_size(12.0)
                    .tip(r.reset_tip)
                    .on_press(reset),
            );
        }
        control = control.push(r.control);
        let control: Element<'a, M> = if r.wide {
            container(control)
                .width(WIDE)
                .align_x(Alignment::End)
                .into()
        } else {
            control.into()
        };
        // 42 px at least; a row with a hint is taller than that on its own.
        let line = container(
            iced::widget::row![text.width(Length::Fill), control]
                .spacing(20)
                .align_y(Alignment::Center),
        )
        .padding(Padding::from([space::MD, space::XL]));
        if has_hint {
            line.into()
        } else {
            line.height(42.0).align_y(Alignment::Center).into()
        }
    }
}

/// A block that does not fit a label and a control: a preview, a grid. It
/// takes the whole width of the group, with a label and a hint over it if
/// they are given.
pub fn block<'a, M: 'a>(
    t: &'a Theme,
    label: Option<&str>,
    hint: Option<&str>,
    content: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let mut stack = column![].spacing(space::LG);
    if label.is_some() || hint.is_some() {
        let mut text = column![].spacing(2);
        if let Some(label) = label {
            text = text.push(typography::body(t, label.to_string()));
        }
        if let Some(hint) = hint {
            text = text.push(typography::hint(t, hint.to_string()));
        }
        stack = stack.push(text);
    }
    stack = stack.push(content.into());
    container(stack).padding(space::XL).into()
}

/// A card around rows, a hairline between them inset at both ends like the
/// system's grouped lists: the line separates two rows, it does not cut the
/// box in two.
pub fn group<'a, M: 'a>(t: &'a Theme, rows: Vec<Element<'a, M>>) -> Element<'a, M> {
    let mut stack = column![];
    for (at, one) in rows.into_iter().enumerate() {
        if at > 0 {
            stack = stack.push(
                container(rule::horizontal(1).style(style::rule::hairline(t)))
                    .padding(Padding::from([0.0, space::XL])),
            );
        }
        stack = stack.push(one);
    }
    container(stack)
        .width(Length::Fill)
        .style(style::container::group(t))
        .into()
}

/// A titled part of a settings page: a heading, a hint, and its group.
#[must_use]
pub struct Section<'a, M> {
    theme: &'a Theme,
    title: String,
    hint: Option<String>,
    body: Option<Element<'a, M>>,
}

pub fn section<'a, M: 'a>(t: &'a Theme, title: impl Into<String>) -> Section<'a, M> {
    Section {
        theme: t,
        title: title.into(),
        hint: None,
        body: None,
    }
}

impl<'a, M: 'a> Section<'a, M> {
    /// A sentence under the title.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The rows, in a card.
    pub fn group(mut self, rows: Vec<Element<'a, M>>) -> Self {
        self.body = Some(group(self.theme, rows));
        self
    }

    /// Something that is its own box and goes straight on the page.
    pub fn bare(mut self, body: impl Into<Element<'a, M>>) -> Self {
        self.body = Some(body.into());
        self
    }
}

impl<'a, M: 'a> From<Section<'a, M>> for Element<'a, M> {
    fn from(s: Section<'a, M>) -> Self {
        let t = s.theme;
        let mut head =
            column![typography::heading(t, s.title).size(hangar_theme::tokens::font_size::BODY)]
                .spacing(2);
        if let Some(hint) = s.hint {
            head = head.push(typography::hint(t, hint));
        }
        let mut stack = column![container(head).padding(Padding {
            left: space::XS,
            right: space::XS,
            ..Padding::ZERO
        })]
        .spacing(space::MD);
        if let Some(body) = s.body {
            stack = stack.push(body);
        }
        stack.into()
    }
}

/// A page of sections, spaced the way the settings space theirs.
pub fn page<'a, M: 'a>(sections: Vec<Element<'a, M>>) -> Element<'a, M> {
    iced::widget::Column::with_children(sections)
        .spacing(space::XXXL)
        .padding(Padding::from([space::XXL, space::XXXL]))
        .width(Length::Fill)
        .into()
}
