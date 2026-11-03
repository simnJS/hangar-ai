use super::*;

const WINDOW: Size = Size::new(800.0, 600.0);

fn anchor(x: f32, y: f32) -> Rectangle {
    Rectangle::new(Point::new(x, y), Size::new(80.0, 26.0))
}

mod position {
    use super::*;

    #[test]
    fn a_dialog_is_centred() {
        let at = position(
            Placement::Center,
            Rectangle::with_size(Size::ZERO),
            Size::new(400.0, 200.0),
            WINDOW,
            4.0,
            24.0,
        );
        assert_eq!(at, Point::new(200.0, 200.0));
    }

    #[test]
    fn a_dialog_larger_than_the_window_starts_at_its_corner() {
        let at = position(
            Placement::Center,
            Rectangle::with_size(Size::ZERO),
            Size::new(900.0, 700.0),
            WINDOW,
            4.0,
            24.0,
        );
        assert_eq!(at, Point::ORIGIN);
    }

    #[test]
    fn the_palette_sits_a_fraction_down_and_centred_across() {
        let at = position(
            Placement::Top(0.14),
            Rectangle::with_size(Size::ZERO),
            Size::new(640.0, 300.0),
            WINDOW,
            4.0,
            16.0,
        );
        assert_eq!(at.x, 80.0);
        assert!((at.y - 84.0).abs() < 1e-3);
    }

    #[test]
    fn a_tall_palette_is_pulled_up_to_stay_in_the_window() {
        let at = position(
            Placement::Top(0.14),
            Rectangle::with_size(Size::ZERO),
            Size::new(640.0, 560.0),
            WINDOW,
            4.0,
            16.0,
        );
        // 600 - 560 - 16 = 24: lower than 84 would leave it off the bottom.
        assert_eq!(at.y, 24.0);
    }

    #[test]
    fn a_menu_opens_under_its_button_at_the_buttons_left_edge() {
        let at = position(
            Placement::Below { align_end: false },
            anchor(100.0, 50.0),
            Size::new(200.0, 150.0),
            WINDOW,
            4.0,
            8.0,
        );
        assert_eq!(at, Point::new(100.0, 80.0));
    }

    #[test]
    fn a_menu_can_end_at_its_buttons_right_edge() {
        let at = position(
            Placement::Below { align_end: true },
            anchor(700.0, 10.0),
            Size::new(200.0, 150.0),
            WINDOW,
            4.0,
            8.0,
        );
        // The button ends at 780; the menu ends there too.
        assert_eq!(at.x, 580.0);
    }

    #[test]
    fn a_menu_flips_above_its_button_when_there_is_no_room_under_it() {
        let at = position(
            Placement::Below { align_end: false },
            anchor(100.0, 550.0),
            Size::new(200.0, 150.0),
            WINDOW,
            4.0,
            8.0,
        );
        assert_eq!(at.y, 550.0 - 4.0 - 150.0);
    }

    #[test]
    fn a_menu_stays_inside_the_window_sideways() {
        let at = position(
            Placement::Below { align_end: false },
            anchor(760.0, 10.0),
            Size::new(200.0, 100.0),
            WINDOW,
            4.0,
            8.0,
        );
        assert_eq!(at.x, 800.0 - 200.0 - 8.0);
        let left = position(
            Placement::Below { align_end: true },
            anchor(0.0, 10.0),
            Size::new(200.0, 100.0),
            WINDOW,
            4.0,
            8.0,
        );
        assert_eq!(left.x, 8.0);
    }

    #[test]
    fn a_context_menu_opens_at_the_pointer() {
        let at = position(
            Placement::At(Vector::new(30.0, 12.0)),
            anchor(100.0, 200.0),
            Size::new(220.0, 180.0),
            WINDOW,
            4.0,
            8.0,
        );
        assert_eq!(at, Point::new(130.0, 212.0));
    }

    #[test]
    fn a_context_menu_near_an_edge_is_pulled_back_inside() {
        let at = position(
            Placement::At(Vector::new(70.0, 20.0)),
            anchor(700.0, 560.0),
            Size::new(220.0, 180.0),
            WINDOW,
            4.0,
            8.0,
        );
        assert_eq!(at, Point::new(800.0 - 220.0 - 8.0, 600.0 - 180.0 - 8.0));
    }

    #[test]
    fn a_dock_sits_in_the_bottom_right_corner_a_margin_from_the_edges() {
        let at = position(
            Placement::BottomEnd,
            Rectangle::with_size(Size::ZERO),
            Size::new(460.0, 270.0),
            WINDOW,
            4.0,
            16.0,
        );
        assert_eq!(at, Point::new(800.0 - 460.0 - 16.0, 600.0 - 270.0 - 16.0));
    }

    #[test]
    fn a_dock_larger_than_the_window_is_pinned_to_the_margin() {
        let at = position(
            Placement::BottomEnd,
            Rectangle::with_size(Size::ZERO),
            Size::new(900.0, 700.0),
            WINDOW,
            4.0,
            16.0,
        );
        assert_eq!(at, Point::new(16.0, 16.0));
    }
}

mod dock_interaction {
    use iced::widget::{button, container};
    use iced::{Element, Length};

    use super::*;
    use crate::testing::{Drive, simulate};

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Page,
        Dock,
        Close,
    }

    fn page<'a>() -> Element<'a, Msg> {
        button("page")
            .width(Length::Fill)
            .height(Length::Fill)
            .on_press(Msg::Page)
            .into()
    }

    fn panel<'a>() -> Element<'a, Msg> {
        container(button("dock").width(200).height(100).on_press(Msg::Dock)).into()
    }

    fn messages(ui: iced_test::Simulator<'_, Msg>) -> Vec<Msg> {
        ui.into_messages().collect()
    }

    #[test]
    fn the_window_under_a_dock_stays_reachable() {
        let layer = dock(page(), Some(panel())).on_dismiss(Msg::Close);
        let mut ui = simulate(Element::from(layer));
        ui.click_at(40.0, 40.0);
        assert_eq!(messages(ui), [Msg::Page]);
    }

    #[test]
    fn the_dock_itself_takes_its_clicks_in_the_corner() {
        let layer = dock(page(), Some(panel())).on_dismiss(Msg::Close);
        let mut ui = simulate(Element::from(layer));
        // 800 x 600 window, 200 x 100 panel, 16 px from the edges.
        ui.click_at(800.0 - 16.0 - 100.0, 600.0 - 16.0 - 50.0);
        assert_eq!(messages(ui), [Msg::Dock]);
    }

    #[test]
    fn escape_still_dismisses_a_dock() {
        let layer = dock(page(), Some(panel())).on_dismiss(Msg::Close);
        let mut ui = simulate(Element::from(layer));
        ui.key(iced::keyboard::key::Named::Escape);
        assert_eq!(messages(ui), [Msg::Close]);
    }

    #[test]
    fn a_modal_blocks_the_same_click() {
        let layer = modal(page(), Some(panel())).on_dismiss(Msg::Close);
        let mut ui = simulate(Element::from(layer));
        ui.click_at(40.0, 40.0);
        assert!(!messages(ui).contains(&Msg::Page));
    }
}
