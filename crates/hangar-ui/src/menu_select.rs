//! A closed field that opens a menu: a choice among options that iced's
//! `pick_list` cannot show, because some are grouped under headings and some
//! are there but not available, and say why (the worktree panel's branch
//! pickers: "feat/login, already checked out").
//!
//! ```ignore
//! use hangar_ui::menu::{self, item};
//! use hangar_ui::menu_select::menu_select;
//!
//! menu_select(t, "main", vec![
//!     menu::heading("Local"),
//!     item("main").checked(true).on_select(Msg::Pick(0)).into(),
//!     item("feat/login").disabled_because("already checked out").into(),
//!     menu::heading("Remote"),
//!     item("origin/dev").on_select(Msg::Pick(2)).into(),
//! ])
//! .open(self.picker_open)
//! .on_toggle(Msg::TogglePicker)
//! .on_dismiss(Msg::ClosePicker)
//! ```
//!
//! The field is a button like a drop-down's, and the list is a [`menu`]
//! under it, so the highlight, the arrow keys, the disabled lines and their
//! reasons are the menu's. As everywhere in hangar-ui, whether it is open is
//! the view's: the field only asks to be toggled, and the menu to be closed.

use hangar_theme::Theme;
use hangar_theme::tokens::{font_size, radius, size, space};
use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use crate::icon::{Icon, icon};
use crate::layer;
use crate::menu::{Entry, menu};

/// A field that opens a menu. Becomes an [`Element`] with `.into()`.
#[must_use]
pub struct MenuSelect<'a, M> {
    theme: &'a Theme,
    shown: String,
    placeholder: bool,
    entries: Vec<Entry<M>>,
    open: bool,
    on_toggle: Option<M>,
    on_dismiss: Option<M>,
    width: Length,
    menu_width: (f32, f32),
}

/// A field showing `shown`, over a menu of `entries`.
pub fn menu_select<'a, M>(
    t: &'a Theme,
    shown: impl Into<String>,
    entries: Vec<Entry<M>>,
) -> MenuSelect<'a, M> {
    MenuSelect {
        theme: t,
        shown: shown.into(),
        placeholder: false,
        entries,
        open: false,
        on_toggle: None,
        on_dismiss: None,
        width: Length::Fill,
        menu_width: (240.0, 420.0),
    }
}

impl<'a, M> MenuSelect<'a, M> {
    /// What the field shows is a prompt ("Pick a branch"), not a choice: it
    /// is set in the quiet colour.
    pub fn placeholder(mut self, placeholder: bool) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Whether the menu is showing.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Sent when the field is pressed, to open the menu or to close it.
    pub fn on_toggle(mut self, message: M) -> Self {
        self.on_toggle = Some(message);
        self
    }

    /// Sent when Escape, Tab or a press elsewhere asks for the menu to go.
    pub fn on_dismiss(mut self, message: M) -> Self {
        self.on_dismiss = Some(message);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// The bounds the menu's width follows its widest line between.
    pub fn menu_width(mut self, min: f32, max: f32) -> Self {
        self.menu_width = (min, max);
        self
    }
}

impl<'a, M: Clone + 'a> From<MenuSelect<'a, M>> for Element<'a, M> {
    fn from(s: MenuSelect<'a, M>) -> Self {
        let t = s.theme;
        let c = &t.colors;
        let ink = if s.placeholder { c.fg_subtle } else { c.fg };
        let line = row![
            text(s.shown)
                .size(font_size::UI)
                .style(move |_: &iced::Theme| text::Style { color: Some(ink) })
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
            icon(Icon::ChevronDown, 10.0).color(c.fg_muted),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);

        let open = s.open;
        let look = move |_: &iced::Theme, status: button::Status| {
            let c = &t.colors;
            let edge = if open {
                c.accent
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                c.border_strong
            } else {
                c.line_strong
            };
            button::Style {
                background: Some(Background::Color(c.bg)),
                text_color: c.fg,
                border: Border {
                    color: edge,
                    width: 1.0,
                    radius: radius::SM.into(),
                },
                ..button::Style::default()
            }
        };
        let field = button(container(line).center_y(Length::Fill))
            .padding(Padding::from([0.0, space::LG]))
            .width(s.width)
            .height(size::CONTROL_HEIGHT)
            .style(look)
            .on_press_maybe(s.on_toggle);

        let (min, max) = s.menu_width;
        let list = s
            .open
            .then(|| Element::from(menu(t, s.entries).width_between(min, max)));
        // The layer spares a press on its anchor, which toggles the menu: it
        // must not close it on the way down and open it again on the way up.
        let mut layer = layer::dropdown(field, list);
        if let Some(dismiss) = s.on_dismiss {
            layer = layer.on_dismiss(dismiss);
        }
        layer.into()
    }
}

#[cfg(test)]
mod tests {
    use hangar_theme::default_theme;
    use iced_test::Simulator;

    use super::*;
    use crate::menu::{self, item};
    use crate::testing::{Drive, simulate};

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Toggle,
        Close,
        Pick(usize),
    }

    fn field<'a>(open: bool) -> Element<'a, Msg> {
        menu_select(
            default_theme(),
            "main",
            vec![
                menu::heading("Local"),
                item("main").checked(true).on_select(Msg::Pick(0)).into(),
                item("feat/login")
                    .disabled_because("already checked out")
                    .into(),
                menu::heading("Remote"),
                item("origin/dev").on_select(Msg::Pick(2)).into(),
            ],
        )
        .open(open)
        .on_toggle(Msg::Toggle)
        .on_dismiss(Msg::Close)
        .into()
    }

    fn messages(ui: Simulator<'_, Msg>) -> Vec<Msg> {
        ui.into_messages().collect()
    }

    #[test]
    fn pressing_the_field_asks_to_toggle_the_menu() {
        let mut ui = simulate(field(false));
        ui.click_at(60.0, 13.0);
        assert_eq!(messages(ui), vec![Msg::Toggle]);
    }

    #[test]
    fn a_line_of_the_open_menu_is_picked_and_a_disabled_one_is_not() {
        // Under the field: a heading, then the lines (26 px each).
        let mut ui = simulate(field(true));
        ui.click_at(40.0, 72.0);
        assert_eq!(messages(ui), vec![Msg::Pick(0)]);

        let mut ui = simulate(field(true));
        ui.click_at(40.0, 98.0);
        assert_eq!(messages(ui), Vec::<Msg>::new());

        let mut ui = simulate(field(true));
        ui.click_at(40.0, 148.0);
        assert_eq!(messages(ui), vec![Msg::Pick(2)]);
    }

    #[test]
    fn escape_asks_for_the_menu_to_close() {
        let mut ui = simulate(field(true));
        ui.key(iced::keyboard::key::Named::Escape);
        assert!(messages(ui).contains(&Msg::Close));
    }
}
