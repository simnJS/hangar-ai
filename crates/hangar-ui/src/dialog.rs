//! Dialogs: the panel every dialog of the app shares (`.modal`), and the layer
//! that puts it over the window (`Backdrop.tsx`).
//!
//! A title bar with a close button, a body that scrolls, and a footer with
//! the actions at the bottom right.
//!
//! ```ignore
//! use hangar_ui::{button, dialog, input};
//!
//! let rename = dialog::dialog(t, "Rename workspace", input::text_field(t, &self.name)
//!         .label("Name").on_input(Msg::Name))
//!     .subtitle("~/dev/storefront")
//!     .action(button::ghost(t, "Cancel").on_press(Msg::Cancel))
//!     .action(button::primary(t, "Save").on_press_maybe(valid.then_some(Msg::Save)))
//!     .on_close(Msg::Cancel);
//!
//! // Over the page, while there is one to show:
//! dialog::over(t, page, self.renaming.then(|| self.rename_dialog(t)))
//! ```
//!
//! What the layer does, as the web app's backdrop did:
//!
//! - Escape closes it, even with the keyboard in a field.
//! - A press and release that both land on the veil close it; selecting text
//!   in a field and letting go outside the dialog does not.
//! - Tab and Shift+Tab stay inside the dialog, wrapping round its ends, and
//!   the first field has the keyboard when it opens.
//! - Nothing under the veil can be reached with the pointer or the keys.

use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{Theme, style};
use iced::widget::{column, container, row, rule, scrollable, space as iced_space};
use iced::{Alignment, Element, Length, Padding};

use crate::button::icon_button;
use crate::icon::Icon;
use crate::layer::{self, Layer};
use crate::text as typography;

/// A dialog's contents. Becomes an [`Element`] with `.into()`.
#[must_use]
pub struct Dialog<'a, M> {
    theme: &'a Theme,
    title: String,
    sub: Option<String>,
    body: Element<'a, M>,
    actions: Vec<Element<'a, M>>,
    note: Option<String>,
    lead: Option<Element<'a, M>>,
    width: f32,
    body_max_height: f32,
    body_padding: Padding,
    on_close: Option<M>,
    close_tip: String,
}

/// A dialog titled `title` over `body`.
pub fn dialog<'a, M: Clone + 'a>(
    t: &'a Theme,
    title: impl Into<String>,
    body: impl Into<Element<'a, M>>,
) -> Dialog<'a, M> {
    Dialog {
        theme: t,
        title: title.into(),
        sub: None,
        body: body.into(),
        actions: Vec::new(),
        note: None,
        lead: None,
        width: size::DIALOG_WIDTH,
        // Room for the head and the foot in a window of about 600.
        body_max_height: 420.0,
        // In line with the title above it, which starts 18 px in.
        body_padding: Padding::from([space::XL, 18.0]),
        on_close: None,
        close_tip: String::from("Close"),
    }
}

impl<'a, M: Clone + 'a> Dialog<'a, M> {
    /// A line in the mono face under the title: the folder it acts on.
    pub fn subtitle(mut self, sub: impl Into<String>) -> Self {
        self.sub = Some(sub.into());
        self
    }

    /// A button of the footer. They sit at the right, in the order given.
    pub fn action(mut self, action: impl Into<Element<'a, M>>) -> Self {
        self.actions.push(action.into());
        self
    }

    /// A sentence at the left of the footer, away from the buttons.
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Controls at the left of the footer, away from the buttons at the right:
    /// the housekeeping of a dialog, with its way out kept where every dialog
    /// has it.
    pub fn lead(mut self, lead: impl Into<Element<'a, M>>) -> Self {
        self.lead = Some(lead.into());
        self
    }

    /// The width in pixels: 520 by default, 560 for a list
    /// ([`size::DIALOG_LIST_WIDTH`]), 640 for a form with a lot in it.
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// The tallest the body grows before it scrolls.
    pub fn body_max_height(mut self, height: f32) -> Self {
        self.body_max_height = height;
        self
    }

    /// The body's padding, 10 px by default.
    pub fn body_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.body_padding = padding.into();
        self
    }

    /// Sent by the close button, and by the layer when Escape or the veil
    /// asks for the dialog to go.
    pub fn on_close(mut self, message: M) -> Self {
        self.on_close = Some(message);
        self
    }

    /// What the close button's tip says, in the interface's language.
    pub fn close_tip(mut self, words: impl Into<String>) -> Self {
        self.close_tip = words.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<Dialog<'a, M>> for Element<'a, M> {
    fn from(d: Dialog<'a, M>) -> Self {
        let t = d.theme;

        let mut titles = column![typography::title(t, d.title)].spacing(1);
        if let Some(sub) = d.sub.clone() {
            titles = titles.push(typography::mono(t, sub).size(font_size::CAPTION));
        }
        let close = icon_button(t, Icon::Close)
            .tip(format!("{} · Esc", d.close_tip))
            .on_press_maybe(d.on_close.clone());
        let head = row![titles.width(Length::Fill), close]
            .spacing(space::XL)
            .align_y(Alignment::Center);
        let head = container(head)
            .padding(Padding {
                top: space::MD,
                right: space::LG,
                bottom: space::MD,
                left: 18.0,
            })
            .align_y(Alignment::Center);
        let head = if d.sub.is_some() {
            head
        } else {
            head.height(48.0)
        };

        let body = container(
            scrollable(
                container(d.body)
                    .padding(d.body_padding)
                    .width(Length::Fill),
            )
            .style(style::scrollable::default(t)),
        )
        .max_height(d.body_max_height);

        let mut foot = row![].spacing(space::MD).align_y(Alignment::Center);
        if let Some(lead) = d.lead {
            foot = foot.push(lead);
        }
        if let Some(note) = d.note {
            foot = foot.push(typography::hint(t, note));
        }
        foot = foot.push(iced_space::horizontal());
        for action in d.actions {
            foot = foot.push(action);
        }
        let foot = container(foot)
            .padding(Padding {
                top: space::LG,
                right: 14.0,
                bottom: space::LG,
                left: 18.0,
            })
            .height(52.0);

        let line = || rule::horizontal(1).style(style::rule::hairline(t));
        container(column![head, line(), body, line(), foot])
            .width(d.width)
            .style(style::container::modal(t))
            .into()
    }
}

/// `page` with `dialog` over it, if there is one.
///
/// The dialog's `on_close` is also what Escape and a click on the veil ask
/// for; a dialog without one cannot be dismissed from inside.
pub fn over<'a, M: Clone + 'a>(
    t: &'a Theme,
    page: impl Into<Element<'a, M>>,
    dialog: Option<Dialog<'a, M>>,
) -> Layer<'a, M> {
    let on_close = dialog.as_ref().and_then(|d| d.on_close.clone());
    let mut layer = layer::modal(page, dialog.map(Element::from)).veil(style::container::scrim(t));
    if let Some(message) = on_close {
        layer = layer.on_dismiss(message);
    }
    layer
}

#[cfg(test)]
mod tests;
