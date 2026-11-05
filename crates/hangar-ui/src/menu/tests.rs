use hangar_theme::default_theme;
use iced::keyboard::key::Named;
use iced::widget::{container, space};
use iced::{Element, Length, mouse};

use super::*;
use crate::layer;
use crate::testing::{Drive, simulate};

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    First,
    Second,
    Third,
    Edit,
    Remove,
    Dismiss,
    Open(Vector),
}

/// A button-sized anchor in the top-left corner of the window.
fn anchor<'a>() -> Element<'a, Msg> {
    container(space())
        .width(Length::Fixed(80.0))
        .height(Length::Fixed(26.0))
        .into()
}

fn entries() -> Vec<Entry<Msg>> {
    vec![
        heading("Pane"),
        item("First").icon(Icon::Plus).on_select(Msg::First).into(),
        separator(),
        item("Second")
            .on_select(Msg::Second)
            .shortcut("Ctrl+K")
            .into(),
        item("Greyed").disabled().on_select(Msg::Dismiss).into(),
        item("Third")
            .danger()
            .on_select(Msg::Third)
            .action(Icon::Pencil, "Edit", Msg::Edit)
            .danger_action(Icon::Close, "Remove", Msg::Remove)
            .into(),
    ]
}

/// The menu open under the anchor, which puts it at (0, 30) with its first
/// line (after a 24 px heading) at y 59 to 85.
fn open_menu<'a>(from_keyboard: bool) -> Element<'a, Msg> {
    let t = default_theme();
    let content = menu(t, entries()).from_keyboard(from_keyboard).into();
    layer::dropdown(anchor(), Some(content))
        .on_dismiss(Msg::Dismiss)
        .into()
}

const FIRST_LINE: (f32, f32) = (40.0, 72.0);
const SECOND_LINE: (f32, f32) = (40.0, 107.0);

fn messages(ui: Simulator<'_, Msg>) -> Vec<Msg> {
    ui.into_messages().collect()
}

use iced_test::Simulator;

mod lines {
    use super::*;

    #[test]
    fn a_detail_and_a_mono_label_widen_the_line_that_carries_them() {
        let t = default_theme();
        let plain = menu::<Msg>(t, vec![item("Deploy").into()]).width_between(0.0, 1000.0);
        let with_detail = menu::<Msg>(
            t,
            vec![item("Deploy").detail("pnpm run deploy --prod").into()],
        )
        .width_between(0.0, 1000.0);
        assert!(with_detail.natural_width() > plain.natural_width());
        // An empty detail is no detail.
        let empty =
            menu::<Msg>(t, vec![item("Deploy").detail("").into()]).width_between(0.0, 1000.0);
        assert_eq!(empty.natural_width(), plain.natural_width());
        let mono = menu::<Msg>(t, vec![item("pnpm test").mono().into()]).width_between(0.0, 1000.0);
        assert!(mono.natural_width() > 0.0);
    }

    #[test]
    fn a_line_with_a_detail_is_still_picked_by_a_click() {
        let t = default_theme();
        let content = menu(
            t,
            vec![
                heading("Pane"),
                item("First").detail("ls -la").on_select(Msg::First).into(),
            ],
        )
        .into();
        let open: Element<'_, Msg> = layer::dropdown(anchor(), Some(content))
            .on_dismiss(Msg::Dismiss)
            .into();
        let mut ui = simulate(open);
        ui.click_at(FIRST_LINE.0, FIRST_LINE.1);
        assert_eq!(messages(ui), [Msg::First]);
    }
}

mod pointer {
    use super::*;

    #[test]
    fn a_click_on_a_line_picks_it() {
        let mut ui = simulate(open_menu(false));
        ui.click_at(FIRST_LINE.0, FIRST_LINE.1);
        assert_eq!(messages(ui), [Msg::First]);
    }

    #[test]
    fn a_press_on_one_line_released_on_another_picks_neither() {
        let mut ui = simulate(open_menu(false));
        ui.move_to(FIRST_LINE.0, FIRST_LINE.1);
        ui.press(mouse::Button::Left);
        ui.move_to(SECOND_LINE.0, SECOND_LINE.1);
        ui.release(mouse::Button::Left);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn a_greyed_line_does_nothing_and_the_menu_stays() {
        let mut ui = simulate(open_menu(false));
        // Greyed sits under "Second": 26 px lower.
        ui.click_at(SECOND_LINE.0, SECOND_LINE.1 + 26.0);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn a_heading_and_a_separator_cannot_be_picked() {
        let mut ui = simulate(open_menu(false));
        ui.click_at(40.0, 45.0);
        ui.click_at(40.0, 90.0);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn a_press_outside_the_menu_asks_to_close_it() {
        let mut ui = simulate(open_menu(false));
        ui.click_at(600.0, 400.0);
        assert!(messages(ui).contains(&Msg::Dismiss));
    }

    #[test]
    fn a_press_on_the_anchor_is_left_to_the_button_that_toggles() {
        let mut ui = simulate(open_menu(false));
        ui.click_at(40.0, 13.0);
        assert!(!messages(ui).contains(&Msg::Dismiss));
    }

    #[test]
    fn the_small_actions_of_a_line_appear_with_the_highlight_and_send_their_own_message() {
        let mut ui = simulate(open_menu(false));
        // "Third" is the fifth line: 26 below "Greyed".
        let y = SECOND_LINE.1 + 52.0;
        ui.move_to(40.0, y);
        // The menu is 200 wide, 8 px in from the window's edge; the actions
        // are its last two 20 px cells, 4 px in from the line's end.
        ui.click_at(8.0 + 200.0 - 5.0 - 4.0 - 10.0, y);
        let sent = messages(ui);
        assert!(sent.contains(&Msg::Remove), "{sent:?}");
        assert!(!sent.contains(&Msg::Third));
    }
}

mod keyboard {
    use super::*;

    #[test]
    fn down_enters_at_the_top_and_enter_picks() {
        let mut ui = simulate(open_menu(false));
        ui.key(Named::ArrowDown);
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::First]);
    }

    #[test]
    fn a_menu_opened_from_the_keyboard_starts_on_its_first_line() {
        let mut ui = simulate(open_menu(true));
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::First]);
    }

    #[test]
    fn the_arrows_skip_headings_separators_and_greyed_lines() {
        let mut ui = simulate(open_menu(false));
        for _ in 0..3 {
            ui.key(Named::ArrowDown);
        }
        // First, Second, then past "Greyed" to Third.
        ui.key(Named::Space);
        assert_eq!(messages(ui), [Msg::Third]);
    }

    #[test]
    fn the_arrows_wrap_round_the_ends() {
        let mut ui = simulate(open_menu(false));
        ui.key(Named::ArrowUp);
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::Third]);

        let mut ui = simulate(open_menu(false));
        for _ in 0..4 {
            ui.key(Named::ArrowDown);
        }
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::First]);
    }

    #[test]
    fn home_and_end_jump() {
        let mut ui = simulate(open_menu(false));
        ui.key(Named::End);
        ui.key(Named::Enter);
        ui.key(Named::Home);
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::Third, Msg::First]);
    }

    #[test]
    fn the_keys_a_menu_used_are_spent() {
        let mut ui = simulate(open_menu(false));
        assert_eq!(ui.key(Named::ArrowDown), iced::event::Status::Captured);
        // A key it has no use for goes on to the window's shortcuts.
        assert_eq!(ui.key(Named::F5), iced::event::Status::Ignored);
    }

    #[test]
    fn escape_and_tab_ask_to_close_the_menu() {
        let mut ui = simulate(open_menu(false));
        ui.key(Named::Escape);
        ui.key(Named::Tab);
        assert_eq!(messages(ui), [Msg::Dismiss, Msg::Dismiss]);
    }

    #[test]
    fn the_pointer_takes_the_highlight_over_from_the_keys() {
        let mut ui = simulate(open_menu(false));
        ui.key(Named::ArrowDown);
        ui.move_to(SECOND_LINE.0, SECOND_LINE.1);
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::Second]);
    }

    #[test]
    fn a_resting_pointer_does_not_undo_the_keys() {
        let mut ui = simulate(open_menu(false));
        ui.move_to(FIRST_LINE.0, FIRST_LINE.1);
        ui.key(Named::ArrowDown);
        // Any event that is not the pointer moving.
        ui.simulate([iced::Event::Window(iced::window::Event::RedrawRequested(
            std::time::Instant::now(),
        ))]);
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::Second]);
    }
}

mod context {
    use super::*;

    fn row<'a>() -> Element<'a, Msg> {
        container(space())
            .width(Length::Fixed(300.0))
            .height(Length::Fixed(28.0))
            .into()
    }

    fn menu_at<'a>(at: Option<Vector>) -> Element<'a, Msg> {
        context_menu(default_theme(), row(), at, entries)
            .on_open(Msg::Open)
            .on_dismiss(Msg::Dismiss)
            .into()
    }

    #[test]
    fn the_right_button_asks_for_a_menu_where_the_pointer_is() {
        let mut ui = simulate(menu_at(None));
        ui.move_to(120.0, 10.0);
        ui.press(mouse::Button::Right);
        assert_eq!(messages(ui), [Msg::Open(Vector::new(120.0, 10.0))]);
    }

    #[test]
    fn the_left_button_does_not() {
        let mut ui = simulate(menu_at(None));
        ui.click_at(120.0, 10.0);
        assert_eq!(messages(ui), []);
    }

    #[test]
    fn an_open_menu_is_where_it_was_asked_for() {
        // Asked for at (100, 10): the menu's first line is a heading below,
        // then "First" at y 10 + 24 + 5 to + 26.
        let mut ui = simulate(menu_at(Some(Vector::new(100.0, 10.0))));
        ui.click_at(150.0, 10.0 + 5.0 + 24.0 + 13.0);
        assert_eq!(messages(ui), [Msg::First]);
    }

    #[test]
    fn a_right_press_outside_closes_it_and_asks_for_the_next_one() {
        let mut ui = simulate(menu_at(Some(Vector::new(100.0, 10.0))));
        ui.move_to(10.0, 10.0);
        ui.press(mouse::Button::Right);
        assert_eq!(
            messages(ui),
            [Msg::Dismiss, Msg::Open(Vector::new(10.0, 10.0))]
        );
    }

    #[test]
    fn opened_from_the_keyboard_the_first_line_has_the_highlight() {
        let mut ui = simulate(
            context_menu(
                default_theme(),
                row(),
                Some(Vector::new(100.0, 10.0)),
                entries,
            )
            .from_keyboard(true)
            .on_open(Msg::Open)
            .on_dismiss(Msg::Dismiss),
        );
        ui.key(Named::Enter);
        assert_eq!(messages(ui), [Msg::First]);
    }
}

mod panel {
    use super::*;
    use crate::button::secondary;
    use crate::input::text_field;
    use iced::widget::column;

    #[derive(Debug, Clone, PartialEq)]
    enum Inner {
        Typed(String),
        Dismiss,
    }

    fn open<'a>() -> Element<'a, Inner> {
        let t = default_theme();
        let anchor: Element<'a, Inner> = container(space())
            .width(Length::Fixed(80.0))
            .height(Length::Fixed(26.0))
            .into();
        let content = super::super::panel(
            t,
            column![
                text_field(t, "").on_input(Inner::Typed),
                secondary(t, "Confirm").on_press(Inner::Dismiss),
            ]
            .spacing(8),
        );
        layer::dropdown(anchor, Some(content))
            .tab_inside()
            .autofocus()
            .on_dismiss(Inner::Dismiss)
            .into()
    }

    #[test]
    fn a_panel_with_a_field_takes_the_keyboard_when_it_opens() {
        let mut ui = simulate(open());
        ui.typewrite("a");
        let sent: Vec<Inner> = ui.into_messages().collect();
        assert_eq!(sent, [Inner::Typed("a".into())]);
    }

    #[test]
    fn tab_stays_inside_it_where_a_list_would_close() {
        let mut ui = simulate(open());
        ui.key(Named::Tab);
        ui.typewrite("b");
        let sent: Vec<Inner> = ui.into_messages().collect();
        assert!(!sent.contains(&Inner::Dismiss), "{sent:?}");
    }

    #[test]
    fn escape_closes_it() {
        let mut ui = simulate(open());
        ui.key(Named::Escape);
        let sent: Vec<Inner> = ui.into_messages().collect();
        assert_eq!(sent, [Inner::Dismiss]);
    }
}

mod short_window {
    use super::*;

    fn long_menu<'a>() -> Menu<'a, Msg> {
        let lines: Vec<Entry<Msg>> = (0..40)
            .map(|n| item(format!("Line {n}")).on_select(Msg::First).into())
            .collect();
        menu(default_theme(), lines).max_height(2000.0)
    }

    #[test]
    fn the_scroll_range_follows_the_height_the_menu_was_given() {
        let m = long_menu();
        let content = m.content_height();
        // Room for the whole menu: nothing to scroll.
        assert_eq!(m.max_scroll(content), 0.0);
        // A 300 px box: the rest of the content is reachable.
        assert_eq!(m.max_scroll(300.0), content - 300.0);
    }

    #[test]
    fn revealing_the_last_line_scrolls_to_the_end_of_a_short_box() {
        let m = long_menu();
        let mut state = State::default();
        m.reveal(&mut state, 39, 300.0);
        assert_eq!(state.scroll, m.content_height() - 300.0);
    }

    #[test]
    fn a_line_below_a_short_box_is_revealed() {
        let m = long_menu();
        let mut state = State::default();
        let (top, height) = m.extent(20);
        assert!(top + height > 300.0);
        m.reveal(&mut state, 20, 300.0);
        assert!(state.scroll > 0.0 && top + height <= state.scroll + 300.0);
    }

    #[test]
    fn a_menu_that_shrank_does_not_leave_a_gap_at_the_bottom() {
        let t = default_theme();
        let lines: Vec<Entry<Msg>> = (0..3)
            .map(|n| item(format!("Line {n}")).on_select(Msg::First).into())
            .collect();
        let mut m = menu(t, lines);
        let mut tree = Tree::new(Element::<Msg>::from(menu(t, vec![])));
        tree.state.downcast_mut::<State>().scroll = 500.0;
        let node = m.layout(
            &mut tree,
            &simulated_renderer(),
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 400.0)),
        );
        let layout = Layout::new(&node);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        m.update(
            &mut tree,
            &Event::Mouse(mouse::Event::CursorLeft),
            layout,
            mouse::Cursor::Unavailable,
            &simulated_renderer(),
            &mut iced_test::core::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(Size::new(400.0, 400.0)),
        );
        assert_eq!(tree.state.downcast_ref::<State>().scroll, 0.0);
    }

    fn simulated_renderer() -> iced::Renderer {
        iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced::Font::DEFAULT,
                iced::Pixels(16.0),
                std::env::var("ICED_TEST_BACKEND").ok().as_deref(),
            ),
        )
        .expect("a headless renderer")
    }
}
