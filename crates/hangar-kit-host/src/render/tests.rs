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

/* ── fields, choices and lists: hangar-ui's, driven as a person would ── */

fn handled(sent: &[KitMessage], handler: u32) -> Vec<Option<EventValue>> {
    sent.iter()
        .filter_map(|message| match message {
            KitMessage::Ui(UiEvent {
                view: 7,
                handler: HandlerId(found),
                value,
            }) if *found == handler => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn text_input() -> ui::TextInput {
    let mut input = ui::TextInput::new("Branch name", "");
    input.on_input = Some(HandlerId(1));
    input.on_submit = Some(HandlerId(2));
    input
}

#[test]
fn a_text_field_sends_what_is_typed_and_enter_from_the_view_it_was_drawn_in() {
    let given = ui::Element::TextInput(text_input());
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 30.0, 13.0);
    drawn.typewrite("hi");
    drawn.tap_key(iced::keyboard::key::Named::Enter);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    let typed = handled(&sent, 1);
    assert!(
        matches!(typed.first(), Some(Some(EventValue::Text(first))) if first == "h"),
        "{sent:?}"
    );
    assert_eq!(typed.len(), 2, "one message for each letter: {sent:?}");
    assert_eq!(handled(&sent, 2), vec![None], "Enter: no value, {sent:?}");
}

#[test]
fn a_text_field_shows_its_label_and_hint_and_an_error_instead_of_the_hint() {
    let mut input = text_input();
    input.label = Some("Branch".into());
    input.hint = Some("Letters and dashes".into());
    let widgets = WidgetState::default();
    let given = ui::Element::TextInput(input.clone());
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.find("Branch").expect("the label");
    drawn.find("Letters and dashes").expect("the hint");
    drop(drawn);

    input.error = Some("Spaces are not allowed".into());
    let given = ui::Element::TextInput(input);
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.find("Spaces are not allowed").expect("the error");
    assert!(drawn.find("Letters and dashes").is_err(), "it gives way");
}

#[test]
fn a_text_field_without_a_handler_takes_no_input() {
    let mut input = text_input();
    input.on_input = None;
    let given = ui::Element::TextInput(input);
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 30.0, 13.0);
    drawn.typewrite("hi");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(handled(&sent, 1).is_empty(), "{sent:?}");
}

/// Where the clear button of a search field sits in the 400-point window: on
/// the field's right edge, 2 points in, in a box 22 points on a side.
const CLEAR_X: f32 = 400.0 - 2.0 - 11.0;
const CLEAR_Y: f32 = 13.0;

fn search(value: &str, on_clear: bool) -> ui::Element {
    let mut search = ui::Search::new("Search a job", value);
    search.on_input = Some(HandlerId(4));
    search.on_clear = on_clear.then_some(HandlerId(5));
    ui::Element::Search(search)
}

#[test]
fn a_search_field_has_a_clear_button_only_when_the_kit_named_one_and_there_is_text() {
    let widgets = WidgetState::default();

    let given = search("deploy", true);
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, CLEAR_X, CLEAR_Y);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(handled(&sent, 5), vec![None], "{sent:?}");

    // The kit named none: nothing is there to press.
    let given = search("deploy", false);
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, CLEAR_X, CLEAR_Y);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(handled(&sent, 5).is_empty(), "{sent:?}");
    assert!(handled(&sent, 4).is_empty(), "{sent:?}");

    // Nothing typed, nothing to clear.
    let given = search("", true);
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, CLEAR_X, CLEAR_Y);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(handled(&sent, 5).is_empty(), "{sent:?}");
}

fn select(on_select: bool) -> ui::Element {
    let mut select = ui::Select::new(
        vec![
            ui::SelectOption::new("main", "Main branch"),
            ui::SelectOption::new("dev", "Develop"),
        ],
        Some("dev".into()),
    );
    select.on_select = on_select.then_some(HandlerId(6));
    ui::Element::Select(select)
}

#[test]
fn a_select_shows_the_chosen_option_and_sends_the_value_of_the_one_picked() {
    let widgets = WidgetState::default();
    let given = select(true);
    let mut drawn = simulate(draw(&widgets, &given));
    // Opened, the list lies under the field: 26 points a line, the first
    // option is "Main branch". An overlay is not reached by a search by text.
    click_at(&mut drawn, 60.0, 13.0);
    click_at(&mut drawn, 60.0, 26.0 + 4.0 + 13.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(
        handled(&sent, 6),
        vec![Some(EventValue::Text("main".into()))],
        "{sent:?}"
    );
}

#[test]
fn a_select_without_a_handler_is_read_only() {
    let widgets = WidgetState::default();
    let given = select(false);
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 60.0, 13.0);
    click_at(&mut drawn, 60.0, 26.0 + 4.0 + 13.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(handled(&sent, 6).is_empty(), "{sent:?}");
}

#[test]
fn a_checkbox_and_a_switch_send_the_state_they_are_put_in() {
    let widgets = WidgetState::default();
    let mut tick = ui::Checkbox::new("Follow the log", false);
    tick.on_toggle = Some(HandlerId(7));
    let given = ui::Element::Checkbox(tick);
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.click("Follow the log").expect("the box");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(handled(&sent, 7), vec![Some(EventValue::Bool(true))]);

    let mut switch = ui::Switch::new("Notify me", true);
    switch.on_toggle = Some(HandlerId(8));
    let given = ui::Element::Switch(switch);
    let mut drawn = simulate(draw(&widgets, &given));
    // The track is at the line's start; its label is not a text a search
    // reaches.
    click_at(&mut drawn, 10.0, 9.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(handled(&sent, 8), vec![Some(EventValue::Bool(false))]);
}

fn table(on_row: bool) -> ui::Element {
    // The second row's first cell holds three lines: a row is as tall as its
    // cells, whatever a row of a list is by default.
    let tall = ui::Element::Column(ui::Column::new(vec![
        ui::Element::from("Build"),
        ui::Element::from("on the second"),
        ui::Element::from("runner"),
    ]));
    let mut table = ui::Table::new(
        vec![ui::TableColumn::new("Job"), ui::TableColumn::new("State")],
        vec![
            ui::TableRow::new(vec!["Lint".into(), "ok".into()]),
            ui::TableRow::new(vec![ui::Cell::new(tall), "failed".into()]),
        ],
    );
    table.id = "jobs".into();
    table.on_row = on_row.then_some(HandlerId(9));
    ui::Element::Table(table)
}

#[test]
fn a_table_row_takes_the_pointer_over_the_whole_height_of_its_cells() {
    let widgets = WidgetState::default();
    let given = table(true);
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.find("Job").expect("the header");
    // The last line of the tall cell is a place of the second row that a row
    // of the fixed 28 points would have cut off.
    drawn.click("runner").expect("the third line");
    drawn.click("Lint").expect("the first row");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(
        handled(&sent, 9),
        vec![Some(EventValue::Index(1)), Some(EventValue::Index(0))],
        "{sent:?}"
    );
}

#[test]
fn a_table_without_a_row_handler_sends_nothing_when_a_row_is_pressed() {
    let widgets = WidgetState::default();
    let given = table(false);
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.click("Lint").expect("the first row");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(handled(&sent, 9).is_empty(), "{sent:?}");
}

fn tree() -> ui::Element {
    let mut tree = ui::Tree::new(
        "scene",
        vec![
            ui::TreeNode::new("root", "Workspace").open(),
            ui::TreeNode::new("leaf", "Lonely"),
        ],
    );
    tree.nodes[0].children = vec![ui::TreeNode::new("child", "Camera")];
    tree.on_select = Some(HandlerId(10));
    ui::Element::Tree(tree)
}

#[test]
fn a_tree_sends_the_id_of_the_node_picked_and_opens_and_closes_what_has_children() {
    let widgets = WidgetState::default();
    let given = tree();
    let mut drawn = simulate(draw(&widgets, &given));
    drawn
        .find("Camera")
        .expect("an open node shows its children");
    drawn.click("Camera").expect("a child");
    drawn.click("Lonely").expect("a leaf");
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(
        handled(&sent, 10),
        vec![
            Some(EventValue::Text("child".into())),
            Some(EventValue::Text("leaf".into()))
        ],
        "{sent:?}"
    );

    // The chevron of the first node is Hangar's own to keep: it closes it.
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 10.0, 12.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(
        sent.iter().any(|message| matches!(
            message,
            KitMessage::Local(Local::Toggle { tree, node, open: false })
                if tree == "scene" && node == "root"
        )),
        "{sent:?}"
    );

    // Closed, its children are not drawn.
    let mut widgets = WidgetState::default();
    widgets
        .toggled
        .insert(("scene".into(), "root".into()), false);
    let mut drawn = simulate(draw(&widgets, &given));
    assert!(drawn.find("Camera").is_err());
    drawn.find("Workspace").expect("the node itself");
}

/// With `HANGAR_KIT_SNAPSHOTS=<folder>`, the fields, choices and lists of a
/// kit drawn in a dark and a light theme, in English and French, are written
/// there as PNGs to be looked at; without it, nothing is written.
#[test]
fn snapshots() {
    let Some(folder) = std::env::var_os("HANGAR_KIT_SNAPSHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    let mut input = text_input();
    input.label = Some("Branch".into());
    input.hint = Some("Letters and dashes".into());
    input.value = "feat/login".into();
    let mut wrong = text_input();
    wrong.label = Some("Remote".into());
    wrong.error = Some("Spaces are not allowed".into());
    wrong.value = "my remote".into();
    let mut tick = ui::Checkbox::new("Follow the log", true);
    tick.on_toggle = Some(HandlerId(7));
    let mut switch = ui::Switch::new("Notify me", true);
    switch.on_toggle = Some(HandlerId(8));
    let mut area = ui::TextArea::new("Notes", "A few lines\nof notes");
    area.label = Some("Notes".into());
    let page = ui::Element::Column({
        let mut column = ui::Column::new(vec![
            ui::Element::TextInput(input),
            ui::Element::TextInput(wrong),
            search("deploy", true),
            select(true),
            ui::Element::Checkbox(tick),
            ui::Element::Switch(switch),
            ui::Element::TextArea(area),
            table(true),
            tree(),
        ]);
        column.spacing = Some(8.0);
        column.padding = Some(ui::Padding::All(12.0));
        column
    });
    let mut widgets = WidgetState::default();
    widgets.sync(
        &ui::abi::View {
            seq: 7,
            root: page.clone(),
        },
        &NoImages,
        &[],
    );
    for (name, style) in [
        ("dark-en", KitStyle::dark()),
        (
            "light-fr",
            KitStyle::light().with_locale(hangar_i18n::Locale::Fr),
        ),
    ] {
        let cx = Cx {
            style,
            seq: 7,
            widgets: &widgets,
        };
        let settings = Settings {
            fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
            default_font: hangar_theme::fonts::ui(),
            ..Settings::default()
        };
        let body = container(element(&cx, &page))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(style.palette.bg)),
                ..Default::default()
            });
        let mut drawn = Simulator::with_size(settings, Size::new(420.0, 720.0), body);
        let snapshot = drawn
            .snapshot(&hangar_theme::to_iced(style.theme))
            .expect("a snapshot");
        snapshot
            .matches_image(folder.join(format!("kit-fields-{name}.png")))
            .expect("written");
    }
}

/// A tab that has no files.
struct NoImages;

impl KitApi for NoImages {
    fn granted(&self) -> Vec<String> {
        Vec::new()
    }

    fn call(&self, _: crate::api::HostCall) -> crate::api::CallFuture {
        crate::api::ready(Err(KitError::new(KitError::UNAVAILABLE, "no app")))
    }
}
