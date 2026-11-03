use hangar_theme::default_theme;
use iced::Element;
use iced_test::Simulator;

use super::*;
use crate::testing::simulate;

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Pick(u8),
    Close(u8),
}

fn strip_of<'a>(items: &'a [Tab<u8>], active: &'a u8) -> Element<'a, Msg> {
    strip(default_theme(), items, active, Msg::Pick)
        .on_close(Msg::Close)
        .into()
}

fn messages(ui: Simulator<'_, Msg>) -> Vec<Msg> {
    ui.into_messages().collect()
}

fn items() -> Vec<Tab<u8>> {
    vec![
        Tab::new(1, "scene.glb").icon(Icon::Cube).closable(),
        Tab::new(2, "report.pdf").closable(),
        Tab::new(3, "pinned"),
    ]
}

#[test]
fn pressing_a_tab_picks_it() {
    let items = items();
    let active = 1;
    let mut ui = simulate(strip_of(&items, &active));
    ui.click("report.pdf").expect("the tab is shown");
    assert_eq!(messages(ui), [Msg::Pick(2)]);
}

#[test]
fn pressing_the_active_tab_picks_it_again() {
    let items = items();
    let active = 1;
    let mut ui = simulate(strip_of(&items, &active));
    ui.click("scene.glb").expect("the tab is shown");
    assert_eq!(messages(ui), [Msg::Pick(1)]);
}

#[test]
fn a_tab_that_cannot_be_closed_has_no_cross() {
    let items = items();
    let active = 1;
    let mut ui = simulate(strip_of(&items, &active));
    // A press at the far end of "pinned" selects it and closes nothing.
    ui.click("pinned").expect("the tab is shown");
    assert_eq!(messages(ui), [Msg::Pick(3)]);
}

#[test]
fn without_a_close_handler_no_tab_shows_a_cross() {
    let items = items();
    let active = 1;
    let element: Element<'_, Msg> = strip(default_theme(), &items, &active, Msg::Pick).into();
    let mut ui = simulate(element);
    ui.click("report.pdf").expect("the tab is shown");
    assert_eq!(messages(ui), [Msg::Pick(2)]);
}
