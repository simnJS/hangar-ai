use hangar_theme::default_theme;
use iced::keyboard::Modifiers;
use iced::keyboard::key::Named;
use iced::mouse;
use iced::widget::{column, container, space};
use iced::{Element, Length};
use iced_test::Simulator;

use super::*;
use crate::button;
use crate::input::text_field;
use crate::testing::{Drive, simulate};

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Page(String),
    First(String),
    Second(String),
    Close,
    Save,
}

fn page<'a>() -> Element<'a, Msg> {
    let t = default_theme();
    column![
        text_field(t, "").on_input(Msg::Page),
        container(space()).width(Length::Fill).height(Length::Fill),
    ]
    .into()
}

fn the_dialog<'a>() -> Dialog<'a, Msg> {
    let t = default_theme();
    dialog(
        t,
        "Rename",
        column![
            text_field(t, "").on_input(Msg::First),
            text_field(t, "").on_input(Msg::Second),
        ]
        .spacing(8),
    )
    .sub("~/dev/storefront")
    .action(button::ghost(t, "Cancel").on_press(Msg::Close))
    .action(button::primary(t, "Save").on_press(Msg::Save))
    .on_close(Msg::Close)
}

fn open<'a>() -> Element<'a, Msg> {
    over(default_theme(), page(), Some(the_dialog())).into()
}

fn messages(ui: Simulator<'_, Msg>) -> Vec<Msg> {
    ui.into_messages().collect()
}

mod keys {
    use super::*;

    #[test]
    fn the_first_field_has_the_keyboard_when_the_dialog_opens() {
        let mut ui = simulate(open());
        ui.typewrite("a");
        assert_eq!(messages(ui), [Msg::First("a".into())]);
    }

    #[test]
    fn nothing_typed_reaches_the_page_under_the_veil() {
        let mut ui = simulate(open());
        ui.typewrite("abc");
        assert!(
            messages(ui)
                .iter()
                .all(|message| !matches!(message, Msg::Page(_)))
        );
    }

    #[test]
    fn tab_moves_to_the_next_field_and_wraps_round_the_end() {
        let mut ui = simulate(open());
        ui.key(Named::Tab);
        ui.typewrite("b");
        ui.key(Named::Tab);
        ui.typewrite("c");
        assert_eq!(
            messages(ui),
            [Msg::Second("b".into()), Msg::First("c".into())]
        );
    }

    #[test]
    fn shift_tab_goes_back_and_wraps_the_other_way() {
        let mut ui = simulate(open());
        ui.key_with(Named::Tab, Modifiers::SHIFT);
        ui.typewrite("z");
        assert_eq!(messages(ui), [Msg::Second("z".into())]);
    }

    #[test]
    fn escape_closes_even_with_the_keyboard_in_a_field() {
        let mut ui = simulate(open());
        ui.typewrite("a");
        ui.key(Named::Escape);
        assert_eq!(messages(ui), [Msg::First("a".into()), Msg::Close]);
    }

    #[test]
    fn every_key_is_the_dialogs() {
        let mut ui = simulate(open());
        assert_eq!(ui.key(Named::F5), iced::event::Status::Captured);
    }
}

mod veil {
    use super::*;

    #[test]
    fn a_click_on_the_veil_closes_the_dialog() {
        let mut ui = simulate(open());
        ui.click_at(5.0, 5.0);
        assert_eq!(messages(ui), [Msg::Close]);
    }

    #[test]
    fn a_press_inside_let_go_outside_does_not() {
        let mut ui = simulate(open());
        // The middle of the window is inside the dialog.
        ui.move_to(400.0, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(5.0, 5.0);
        ui.release(mouse::Button::Left);
        assert!(!messages(ui).contains(&Msg::Close));
    }

    #[test]
    fn a_press_on_the_veil_let_go_inside_does_not_and_leaves_nothing_pending() {
        let mut ui = simulate(open());
        ui.move_to(5.0, 5.0);
        ui.press(mouse::Button::Left);
        ui.move_to(400.0, 300.0);
        ui.release(mouse::Button::Left);
        // A later release outside, with no press of its own, is no click.
        ui.move_to(5.0, 5.0);
        ui.release(mouse::Button::Left);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn the_page_cannot_be_clicked_through_it() {
        let mut ui = simulate(open());
        // Where the page's field is.
        ui.click_at(100.0, 14.0);
        ui.typewrite("x");
        assert!(
            messages(ui)
                .iter()
                .all(|message| !matches!(message, Msg::Page(_)))
        );
    }
}

mod buttons {
    use super::*;

    #[test]
    fn the_footer_buttons_work() {
        let mut ui = simulate(open());
        ui.click("Save").expect("the Save button is shown");
        assert_eq!(messages(ui), [Msg::Save]);
    }

    #[test]
    fn cancel_asks_for_the_dialog_to_go() {
        let mut ui = simulate(open());
        ui.click("Cancel").expect("the Cancel button is shown");
        assert_eq!(messages(ui), [Msg::Close]);
    }
}

#[test]
fn a_dialog_without_a_close_message_cannot_be_dismissed_from_inside() {
    let t = default_theme();
    let without: Dialog<'_, Msg> = dialog(t, "Plain", column![]).action(button::primary(t, "OK"));
    let mut ui = simulate(over(t, page(), Some(without)));
    ui.key(Named::Escape);
    ui.click_at(5.0, 5.0);
    assert_eq!(messages(ui), []);
}
