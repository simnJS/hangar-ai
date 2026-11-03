//! The widgets that are hangar-ui's, driven as a person would: a button
//! sends its handler, a menu opens under its button, and a line chosen sends
//! its id.

use std::borrow::Cow;

use iced::{Settings, Size};
use iced_test::Simulator;

use super::*;

fn simulate<'a>(element: El<'a>) -> Simulator<'a, KitMessage> {
    let settings = Settings {
        fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
        default_font: hangar_theme::fonts::ui(),
        ..Settings::default()
    };
    Simulator::with_size(settings, Size::new(400.0, 300.0), element)
}

fn draw<'a>(widgets: &'a WidgetState, given: &'a ui::Element) -> El<'a> {
    let cx = Cx {
        style: KitStyle::dark(),
        seq: 7,
        widgets,
    };
    element(&cx, given)
}

fn menu() -> ui::Menu {
    let mut menu = ui::Menu::new(vec![
        ui::MenuEntry::Item(ui::MenuItem::new("refresh", "Refresh")),
        ui::MenuEntry::Separator,
        ui::MenuEntry::Item(ui::MenuItem::new("copy", "Copy the names").checked(true)),
        ui::MenuEntry::Item(ui::MenuItem::new("gone", "Not now").disabled(true)),
    ]);
    menu.label = Some("Actions".into());
    menu.on_select = Some(HandlerId(3));
    menu
}

#[test]
fn a_button_sends_its_handler_from_the_view_it_was_drawn_in() {
    let mut button = ui::Button::new("Greet");
    button.kind = ButtonKind::Primary;
    button.on_press = Some(HandlerId(2));
    let given = ui::Element::Button(button);
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.click("Greet").expect("the button");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(
        matches!(
            sent.as_slice(),
            [KitMessage::Ui(UiEvent {
                view: 7,
                handler: HandlerId(2),
                value: None
            })]
        ),
        "{sent:?}"
    );
}

#[test]
fn a_closed_menu_opens_from_its_button() {
    let given = ui::Element::Menu(menu());
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    assert!(drawn.find("Refresh").is_err(), "closed, it lists nothing");
    drawn.click("Actions").expect("the menu's button");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    let ui::Element::Menu(menu) = &given else {
        unreachable!()
    };
    let key = menu_key(menu);
    assert!(
        matches!(sent.as_slice(), [KitMessage::Local(Local::Menu(Some(opened)))] if *opened == key),
        "{sent:?}"
    );
}

/// A click at a point of the window, as a person's: moved there, pressed,
/// let go.
fn click_at(drawn: &mut Simulator<'_, KitMessage>, x: f32, y: f32) {
    use iced::{Event, Point, mouse};
    let position = Point::new(x, y);
    drawn.point_at(position);
    drawn.simulate([
        Event::Mouse(mouse::Event::CursorMoved { position }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]);
}

/// The lines of the menu open under its small button, as hangar-ui lays
/// them out: 4 px under the button, 5 px of padding, 26 px a line, 9 px a
/// separator. A menu is an overlay, which a search by text does not reach.
const LINE_X: f32 = 40.0;
const BUTTON: f32 = hangar_theme::tokens::size::CONTROL_HEIGHT_SM;
const REFRESH_Y: f32 = BUTTON + 9.0 + 13.0;
const COPY_Y: f32 = BUTTON + 44.0 + 13.0;
const DISABLED_Y: f32 = BUTTON + 70.0 + 13.0;

fn open() -> (ui::Element, WidgetState) {
    let given = ui::Element::Menu(menu());
    let ui::Element::Menu(menu) = &given else {
        unreachable!()
    };
    let widgets = WidgetState {
        menu: Some(menu_key(menu)),
        ..WidgetState::default()
    };
    (given, widgets)
}

fn chosen(sent: &[KitMessage]) -> Vec<String> {
    sent.iter()
        .filter_map(|message| match message {
            KitMessage::Ui(UiEvent {
                view: 7,
                handler: HandlerId(3),
                value: Some(EventValue::Text(id)),
            }) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_open_menu_sends_the_id_of_the_line_chosen() {
    let (given, widgets) = open();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, LINE_X, COPY_Y);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(chosen(&sent), vec!["copy"], "{sent:?}");

    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, LINE_X, REFRESH_Y);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(chosen(&sent), vec!["refresh"], "{sent:?}");
}

#[test]
fn a_disabled_line_sends_nothing() {
    let (given, widgets) = open();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, LINE_X, DISABLED_Y);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(chosen(&sent).is_empty(), "{sent:?}");
}

#[test]
fn a_press_outside_an_open_menu_closes_it() {
    let (given, widgets) = open();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 380.0, 280.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(
        matches!(sent.as_slice(), [KitMessage::Local(Local::Menu(None))]),
        "{sent:?}"
    );
}

#[test]
fn a_menu_is_told_from_another_by_what_it_lists() {
    let one = menu();
    let mut other = menu();
    other.items.pop();
    assert_eq!(menu_key(&one), menu_key(&menu()));
    assert_ne!(menu_key(&one), menu_key(&other));
}

#[test]
fn hangar_says_its_own_words_in_the_tabs_language() {
    let given = ui::Element::Unsupported;
    let widgets = WidgetState::default();
    let cx = Cx {
        style: KitStyle::light().with_locale(hangar_i18n::Locale::Fr),
        seq: 1,
        widgets: &widgets,
    };
    let mut drawn = simulate(element(&cx, &given));
    drawn
        .find("Une partie de cet onglet demande un Hangar plus récent.")
        .expect("the French words");
}
