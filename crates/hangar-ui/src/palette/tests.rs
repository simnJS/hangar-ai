use hangar_theme::default_theme;
use iced::keyboard::key::Named;
use iced::widget::{container, space};
use iced::{Element, Length};
use iced_test::Simulator;

use super::*;
use crate::testing::{Drive, simulate};

#[derive(Debug, Clone, PartialEq)]
enum Target {
    Settings,
    Relaunch,
    RelaunchAll,
    Ben,
    Storefront,
}

fn items() -> Vec<Item<Target>> {
    vec![
        Item::new(Target::Ben, "Panes", "Go to Ben")
            .state(GlyphState::Waiting)
            .detail(
                Detail::new()
                    .plain("Codex · ")
                    .state(AgentState::Waiting, "waiting"),
            ),
        Item::new(Target::Settings, "View", "Open the settings")
            .icon(Icon::Gear)
            .keys("Ctrl+,")
            .keywords(["preferences", "options"]),
        Item::new(Target::Relaunch, "Panes", "Restart the pane").icon(Icon::Restart),
        Item::new(Target::RelaunchAll, "Panes", "Restart all").icon(Icon::Restart),
        Item::new(Target::Storefront, "Workspaces", "Open storefront").icon(Icon::Folder),
    ]
}

fn palette() -> Palette<Target> {
    Palette::new(items())
}

fn run<T: Clone>(palette: &mut Palette<T>, message: Msg) -> Event<T> {
    palette.update(message).0
}

mod choosing {
    use super::*;

    #[test]
    fn starts_on_the_first_row() {
        let palette = palette();
        assert_eq!(palette.selected(), 0);
        assert_eq!(palette.chosen().map(|item| &item.value), Some(&Target::Ben));
    }

    #[test]
    fn the_rows_are_listed_group_by_group_in_the_order_given() {
        // Panes first (three of them), then View, then Workspaces: the order
        // the groups first appear in, not the order of the items.
        let mut palette = palette();
        let mut seen = Vec::new();
        for _ in 0..palette.len() {
            seen.push(palette.chosen().unwrap().value.clone());
            run(&mut palette, Msg::Next);
        }
        assert_eq!(
            seen,
            [
                Target::Ben,
                Target::Relaunch,
                Target::RelaunchAll,
                Target::Settings,
                Target::Storefront
            ]
        );
    }

    #[test]
    fn next_and_previous_wrap_round_the_ends() {
        let mut palette = palette();
        run(&mut palette, Msg::Previous);
        assert_eq!(palette.selected(), 4);
        run(&mut palette, Msg::Next);
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn pages_stop_at_the_ends() {
        let mut palette = palette();
        run(&mut palette, Msg::PageDown);
        assert_eq!(palette.selected(), 4);
        run(&mut palette, Msg::PageUp);
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn the_pointer_resting_on_a_row_chooses_it() {
        let mut palette = palette();
        run(&mut palette, Msg::Hover(3));
        assert_eq!(palette.selected(), 3);
        // A row that is no longer there (the list shrank under the pointer).
        run(&mut palette, Msg::Hover(40));
        assert_eq!(palette.selected(), 3);
    }
}

mod running {
    use super::*;

    #[test]
    fn enter_runs_the_chosen_row() {
        let mut palette = palette();
        run(&mut palette, Msg::Next);
        assert_eq!(run(&mut palette, Msg::Run), Event::Run(Target::Relaunch));
    }

    #[test]
    fn a_click_runs_the_row_clicked_whatever_is_chosen() {
        let mut palette = palette();
        assert_eq!(
            run(&mut palette, Msg::Click(4)),
            Event::Run(Target::Storefront)
        );
    }

    #[test]
    fn nothing_runs_when_nothing_matches() {
        let mut palette = palette();
        run(&mut palette, Msg::Query("zzzz".into()));
        assert!(palette.is_empty());
        assert_eq!(run(&mut palette, Msg::Run), Event::None);
        assert_eq!(run(&mut palette, Msg::Click(0)), Event::None);
    }

    #[test]
    fn escape_dismisses() {
        assert_eq!(run(&mut palette(), Msg::Dismiss), Event::Dismiss);
    }
}

mod searching {
    use super::*;

    #[test]
    fn the_best_match_is_on_top_and_chosen() {
        let mut palette = palette();
        run(&mut palette, Msg::Query("restart a".into()));
        assert_eq!(
            palette.chosen().map(|item| &item.value),
            Some(&Target::RelaunchAll)
        );
    }

    #[test]
    fn a_keyword_finds_a_row_the_label_does_not() {
        let mut palette = palette();
        run(&mut palette, Msg::Query("preferences".into()));
        assert_eq!(palette.len(), 1);
        assert_eq!(
            palette.chosen().map(|item| &item.value),
            Some(&Target::Settings)
        );
    }

    #[test]
    fn typing_puts_the_choice_back_on_top() {
        let mut palette = palette();
        run(&mut palette, Msg::Next);
        run(&mut palette, Msg::Next);
        run(&mut palette, Msg::Query("o".into()));
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn the_choice_stays_inside_a_list_that_shrank() {
        let mut palette = palette();
        run(&mut palette, Msg::PageDown);
        run(&mut palette, Msg::Query("storefront".into()));
        assert_eq!(palette.len(), 1);
        assert_eq!(palette.selected(), 0);
    }

    #[test]
    fn accents_and_case_do_not_matter() {
        let mut palette = Palette::new(vec![Item::new(1, "G", "Afficher la mémoire")]);
        run(&mut palette, Msg::Query("MEMOIRE".into()));
        assert_eq!(palette.len(), 1);
    }
}

mod scrolling {
    use super::*;

    fn long() -> Palette<usize> {
        Palette::new(
            (0..40)
                .map(|n| Item::new(n, "All", format!("Row {n}")))
                .collect(),
        )
    }

    #[test]
    fn the_first_row_brings_its_heading_with_it() {
        let palette = long();
        let (top, bottom) = palette.extent(0);
        assert_eq!(top, 4.0);
        assert_eq!(bottom, 4.0 + HEADING + ROW);
    }

    #[test]
    fn a_later_row_is_one_row_lower_than_the_one_before() {
        let palette = long();
        let (_, first) = palette.extent(1);
        let (second_top, _) = palette.extent(2);
        assert_eq!(second_top, first);
    }

    #[test]
    fn the_scroll_follows_the_choice_off_the_bottom_of_the_list() {
        let mut palette = long();
        for _ in 0..20 {
            run(&mut palette, Msg::Next);
        }
        let (_, bottom) = palette.extent(20);
        assert_eq!(palette.scroll, bottom - LIST_HEIGHT);
    }

    #[test]
    fn the_scroll_goes_back_up_with_the_choice() {
        let mut palette = long();
        run(&mut palette, Msg::PageDown);
        run(&mut palette, Msg::PageDown);
        run(&mut palette, Msg::PageDown);
        assert!(palette.scroll > 0.0);
        run(&mut palette, Msg::Query(String::new()));
        assert_eq!(palette.scroll, 0.0);
    }

    #[test]
    fn a_row_already_in_view_does_not_scroll_the_list() {
        let mut palette = long();
        run(&mut palette, Msg::Next);
        assert_eq!(palette.scroll, 0.0);
    }
}

// ---- Through a window -------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Outer {
    Palette(Msg),
    Dismiss,
}

fn page<'a>() -> Element<'a, Outer> {
    container(space())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn open<'a>(palette: &'a Palette<Target>, labels: &'a Labels) -> Element<'a, Outer> {
    let t = default_theme();
    over(
        t,
        page(),
        Some(palette.view(t, labels).map(Outer::Palette)),
        Outer::Dismiss,
    )
    .into()
}

fn messages(ui: Simulator<'_, Outer>) -> Vec<Outer> {
    ui.into_messages().collect()
}

mod in_a_window {
    use super::*;

    #[test]
    fn the_field_has_the_keyboard_when_it_opens() {
        let palette = palette();
        let labels = Labels::default();
        let mut ui = simulate(open(&palette, &labels));
        ui.typewrite("r");
        assert_eq!(messages(ui), [Outer::Palette(Msg::Query("r".into()))]);
    }

    #[test]
    fn the_arrows_choose_and_enter_runs() {
        let palette = palette();
        let labels = Labels::default();
        let mut ui = simulate(open(&palette, &labels));
        ui.key(Named::ArrowDown);
        ui.key(Named::ArrowUp);
        ui.key(Named::PageDown);
        ui.key(Named::Enter);
        assert_eq!(
            messages(ui),
            [
                Outer::Palette(Msg::Next),
                Outer::Palette(Msg::Previous),
                Outer::Palette(Msg::PageDown),
                Outer::Palette(Msg::Run),
            ]
        );
    }

    #[test]
    fn escape_and_a_click_on_the_veil_dismiss() {
        let palette = palette();
        let labels = Labels::default();
        let mut ui = simulate(open(&palette, &labels));
        ui.key(Named::Escape);
        ui.click_at(5.0, 590.0);
        assert_eq!(messages(ui), [Outer::Dismiss, Outer::Dismiss]);
    }

    #[test]
    fn tab_leaves_the_field_where_it_is() {
        let palette = palette();
        let labels = Labels::default();
        let mut ui = simulate(open(&palette, &labels));
        ui.key(Named::Tab);
        ui.typewrite("x");
        assert_eq!(messages(ui), [Outer::Palette(Msg::Query("x".into()))]);
    }

    #[test]
    fn a_click_on_a_row_runs_it() {
        let palette = palette();
        let labels = Labels::default();
        let mut ui = simulate(open(&palette, &labels));
        // The panel starts 14 % down a 600-high window; the field is 50
        // high, then a hairline, then 4 px of padding, a heading, and the
        // first row.
        let first_row = 84.0 + 50.0 + 1.0 + 4.0 + HEADING + ROW / 2.0;
        ui.click_at(400.0, first_row);
        let sent = messages(ui);
        assert!(sent.contains(&Outer::Palette(Msg::Click(0))), "{sent:?}");
    }

    #[test]
    fn the_esc_cap_dismisses() {
        let palette = palette();
        let labels = Labels::default();
        let mut ui = simulate(open(&palette, &labels));
        ui.click("Esc").expect("the cap is shown");
        let sent = messages(ui);
        assert!(sent.contains(&Outer::Palette(Msg::Dismiss)), "{sent:?}");
    }
}
