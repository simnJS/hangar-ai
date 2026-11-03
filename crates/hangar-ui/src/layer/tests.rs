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
}
