use hangar_theme::default_theme;
use iced::keyboard::Modifiers;
use iced::keyboard::key::Named;
use iced::widget::{container, space};
use iced::{Element, Length, mouse};
use iced_test::Simulator;

use super::*;
use crate::testing::{Drive, simulate};

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Resize(f32),
    End,
    Reset,
}

fn region<'a>() -> Element<'a, Msg> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn columns<'a>(ratio: f32) -> Element<'a, Msg> {
    split(default_theme(), region(), region())
        .ratio(ratio)
        .min_first(100.0)
        .min_second(100.0)
        .on_resize(Msg::Resize)
        .on_resize_end(Msg::End)
        .on_reset(Msg::Reset)
        .into()
}

fn rows<'a>(ratio: f32) -> Element<'a, Msg> {
    split(default_theme(), region(), region())
        .arrange(Arrange::Rows)
        .ratio(ratio)
        .on_resize(Msg::Resize)
        .on_resize_end(Msg::End)
        .into()
}

fn messages(ui: Simulator<'_, Msg>) -> Vec<Msg> {
    ui.into_messages().collect()
}

/// The window is 800 by 600 and the divider takes 1 px of it.
const USABLE_WIDE: f32 = 799.0;
const USABLE_TALL: f32 = 599.0;

fn resized(sent: &[Msg]) -> Vec<f32> {
    sent.iter()
        .filter_map(|message| match message {
            Msg::Resize(ratio) => Some(*ratio),
            _ => None,
        })
        .collect()
}

mod sizes {
    use super::*;

    #[test]
    fn the_first_region_gets_its_share() {
        assert_eq!(first_size(0.25, 800.0, 0.0, 0.0), 200.0);
    }

    #[test]
    fn neither_region_goes_under_its_minimum() {
        assert_eq!(first_size(0.01, 800.0, 100.0, 100.0), 100.0);
        assert_eq!(first_size(0.99, 800.0, 100.0, 100.0), 700.0);
    }

    #[test]
    fn a_window_too_small_for_both_minimums_shares_in_proportion_to_them() {
        assert_eq!(first_size(0.9, 300.0, 200.0, 100.0), 200.0);
        assert_eq!(first_size(0.1, 150.0, 200.0, 100.0), 100.0);
    }

    #[test]
    fn a_ratio_outside_zero_to_one_is_taken_to_the_edge() {
        assert_eq!(first_size(-1.0, 800.0, 0.0, 0.0), 0.0);
        assert_eq!(first_size(2.0, 800.0, 0.0, 0.0), 800.0);
    }

    #[test]
    fn the_ratio_of_a_size_is_the_inverse() {
        let size = first_size(0.3, 1000.0, 0.0, 0.0);
        assert!((ratio_for(size, 1000.0) - 0.3).abs() < 1e-6);
        assert_eq!(ratio_for(10.0, 0.0), 0.5);
    }
}

mod dragging {
    use super::*;

    #[test]
    fn moves_the_ratio_with_the_pointer() {
        let mut ui = simulate(columns(0.25));
        let line = 0.25 * USABLE_WIDE;
        ui.move_to(line, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(line + 100.0, 300.0);
        let sent = messages(ui);
        let ratios = resized(&sent);
        assert_eq!(ratios.len(), 1, "{sent:?}");
        assert!((ratios[0] - (line + 100.0) / USABLE_WIDE).abs() < 1e-3);
    }

    #[test]
    fn keeps_the_distance_between_the_pointer_and_the_line_it_took_hold_at() {
        let mut ui = simulate(columns(0.25));
        let line = 0.25 * USABLE_WIDE;
        // Four pixels to the side of the line, still on the grab area.
        ui.move_to(line + 3.0, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(line + 3.0 + 50.0, 300.0);
        let ratios = resized(&messages(ui));
        assert!((ratios[0] - (line + 50.0) / USABLE_WIDE).abs() < 1e-3);
    }

    #[test]
    fn stops_at_the_minimums() {
        let mut ui = simulate(columns(0.5));
        let line = 0.5 * USABLE_WIDE;
        ui.move_to(line, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(0.0, 300.0);
        ui.move_to(799.0, 300.0);
        let ratios = resized(&messages(ui));
        assert!((ratios[0] - 100.0 / USABLE_WIDE).abs() < 1e-3);
        assert!((ratios[1] - (USABLE_WIDE - 100.0) / USABLE_WIDE).abs() < 1e-3);
    }

    #[test]
    fn says_when_the_divider_is_let_go() {
        let mut ui = simulate(columns(0.25));
        let line = 0.25 * USABLE_WIDE;
        ui.move_to(line, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(line + 40.0, 300.0);
        ui.release(mouse::Button::Left);
        let sent = messages(ui);
        assert_eq!(sent.last(), Some(&Msg::End));
    }

    #[test]
    fn a_pointer_that_does_not_move_sends_nothing() {
        let mut ui = simulate(columns(0.25));
        let line = 0.25 * USABLE_WIDE;
        ui.move_to(line, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(line, 310.0);
        assert!(resized(&messages(ui)).is_empty());
    }

    #[test]
    fn a_press_away_from_the_divider_takes_nothing() {
        let mut ui = simulate(columns(0.25));
        ui.move_to(500.0, 300.0);
        ui.press(mouse::Button::Left);
        ui.move_to(520.0, 300.0);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn works_the_same_stacked() {
        let mut ui = simulate(rows(0.5));
        let line = 0.5 * USABLE_TALL;
        ui.move_to(400.0, line);
        ui.press(mouse::Button::Left);
        ui.move_to(400.0, line + 60.0);
        let ratios = resized(&messages(ui));
        assert!((ratios[0] - (line + 60.0) / USABLE_TALL).abs() < 1e-3);
    }

    #[test]
    fn a_double_click_puts_it_back() {
        let mut ui = simulate(columns(0.25));
        let line = 0.25 * USABLE_WIDE;
        ui.move_to(line, 300.0);
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
        assert!(messages(ui).contains(&Msg::Reset));
    }
}

mod keyboard {
    use super::*;

    fn focused() -> Simulator<'static, Msg> {
        let mut ui = simulate(columns(0.25));
        let line = 0.25 * USABLE_WIDE;
        ui.move_to(line, 300.0);
        ui.press(mouse::Button::Left);
        ui.release(mouse::Button::Left);
        ui
    }

    #[test]
    fn the_arrows_move_it_by_steps_once_it_has_the_keyboard() {
        let mut ui = focused();
        ui.key(Named::ArrowRight);
        let sent = messages(ui);
        let ratios = resized(&sent);
        assert_eq!(ratios.len(), 1, "{sent:?}");
        assert!((ratios[0] - (0.25 * USABLE_WIDE + 16.0) / USABLE_WIDE).abs() < 1e-3);
        assert!(sent.contains(&Msg::End));
    }

    #[test]
    fn shift_makes_the_steps_bigger() {
        let mut ui = focused();
        ui.key_with(Named::ArrowLeft, Modifiers::SHIFT);
        let ratios = resized(&messages(ui));
        assert!((ratios[0] - (0.25 * USABLE_WIDE - 64.0) / USABLE_WIDE).abs() < 1e-3);
    }

    #[test]
    fn home_and_end_go_to_the_limits() {
        let mut ui = focused();
        ui.key(Named::Home);
        ui.key(Named::End);
        let ratios = resized(&messages(ui));
        assert!((ratios[0] - 100.0 / USABLE_WIDE).abs() < 1e-3);
        assert!((ratios[1] - (USABLE_WIDE - 100.0) / USABLE_WIDE).abs() < 1e-3);
    }

    #[test]
    fn the_arrows_of_the_other_axis_do_nothing() {
        let mut ui = focused();
        assert_eq!(ui.key(Named::ArrowUp), iced::event::Status::Ignored);
        assert!(resized(&messages(ui)).is_empty());
    }

    #[test]
    fn without_the_keyboard_the_arrows_are_not_taken() {
        let mut ui = simulate(columns(0.25));
        assert_eq!(ui.key(Named::ArrowRight), iced::event::Status::Ignored);
    }

    #[test]
    fn a_press_elsewhere_gives_the_keyboard_back() {
        let mut ui = focused();
        ui.click_at(600.0, 300.0);
        assert_eq!(ui.key(Named::ArrowRight), iced::event::Status::Ignored);
    }
}
