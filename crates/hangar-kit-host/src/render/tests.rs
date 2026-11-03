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
        ns: 1,
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
        ns: 1,
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
        &crate::tab::Held::default(),
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
            ns: 1,
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

/* ── what the kits' native tabs added: a canvas, shortcuts, long logs ── */

fn sketch() -> ui::Canvas {
    let mut canvas = ui::Canvas::new("shot", 200.0, 100.0)
        .zoom(Some(1.0))
        .tool(Some(ui::Tool::Rect))
        .ink(ui::Ink::Blue);
    canvas.on_draw = Some(HandlerId(4));
    canvas.on_point = Some(HandlerId(5));
    canvas
}

fn drag(drawn: &mut Simulator<'_, KitMessage>, from: (f32, f32), to: (f32, f32)) {
    use iced::{Event, Point, mouse};
    let (start, end) = (Point::new(from.0, from.1), Point::new(to.0, to.1));
    drawn.point_at(start);
    drawn.simulate([
        Event::Mouse(mouse::Event::CursorMoved { position: start }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ]);
    drawn.point_at(end);
    drawn.simulate([
        Event::Mouse(mouse::Event::CursorMoved { position: end }),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]);
}

#[test]
fn a_drag_on_a_canvas_sends_the_mark_it_drew_in_the_canvass_units() {
    let given = ui::Element::Canvas(sketch().zoom(Some(2.0)));
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    // At twice its size: 20, 10 on screen is 10, 5 of the canvas.
    drag(&mut drawn, (20.0, 10.0), (120.0, 90.0));
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    let marks = handled(&sent, 4);
    let [Some(EventValue::Mark(mark))] = marks.as_slice() else {
        panic!("{sent:?}");
    };
    assert_eq!(
        mark.shape,
        ui::Shape::Rect {
            x: 10.0,
            y: 5.0,
            w: 50.0,
            h: 40.0
        }
    );
    assert_eq!(mark.ink, ui::Ink::Blue);
}

#[test]
fn a_press_on_a_canvas_that_does_not_move_is_a_point() {
    let given = ui::Element::Canvas(sketch());
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 30.0, 40.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(
        handled(&sent, 5),
        vec![Some(EventValue::Point(ui::Point::new(30.0, 40.0)))]
    );
    assert!(handled(&sent, 4).is_empty(), "{sent:?}");
}

#[test]
fn a_canvas_without_a_zoom_fits_its_room_and_says_its_scale() {
    let mut canvas = ui::Canvas::new("wide", 800.0, 200.0);
    canvas.on_fit = Some(HandlerId(6));
    let given = ui::Element::Canvas(canvas);
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    click_at(&mut drawn, 10.0, 10.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    // 400 points of room for 800 units.
    assert_eq!(
        handled(&sent, 6).first(),
        Some(&Some(EventValue::Number(0.5)))
    );
}

fn ctrl_s() -> iced::keyboard::Event {
    use iced::keyboard::{self, Key, Modifiers};
    keyboard::Event::KeyPressed {
        key: Key::Character("s".into()),
        modified_key: Key::Character("s".into()),
        physical_key: keyboard::key::Physical::Unidentified(
            keyboard::key::NativeCode::Unidentified,
        ),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::COMMAND,
        text: None,
        repeat: false,
    }
}

#[test]
fn a_shortcut_is_sent_from_inside_the_tab_and_not_once_the_person_is_elsewhere() {
    let page = ui::Element::Column(ui::Column::new(vec![ui::Element::TextInput(text_input())]));
    let widgets = WidgetState::default();
    let specs = vec!["mod+s".to_owned(), "escape".to_owned()];
    let mut drawn = simulate(keys::shortcuts(draw(&widgets, &page), &specs));
    drawn.simulate([iced::Event::Keyboard(ctrl_s())]);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(
        matches!(sent.as_slice(), [KitMessage::Shortcut(keys)] if keys == "mod+s"),
        "{sent:?}"
    );
    // The page is a column as tall as its field: a press under it is a
    // press elsewhere in the window.
    let page =
        ui::Element::Container(ui::Container::new(page.clone()).height(ui::Length::Px(40.0)));
    let mut drawn = simulate(
        iced::widget::column![
            keys::shortcuts(draw(&widgets, &page), &specs),
            iced::widget::space().height(200),
        ]
        .into(),
    );
    click_at(&mut drawn, 30.0, 200.0);
    drawn.simulate([iced::Event::Keyboard(ctrl_s())]);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert!(
        !sent
            .iter()
            .any(|message| matches!(message, KitMessage::Shortcut(_))),
        "{sent:?}"
    );
}

#[test]
fn shortcuts_match_as_the_platform_writes_them() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    use ui::keys::Shortcut;
    let spec = |text: &str| Shortcut::parse(text).unwrap();
    let s = Key::Character("S".into());
    assert!(keys::matches(&spec("mod+s"), &s, Modifiers::COMMAND));
    assert!(!keys::matches(
        &spec("mod+s"),
        &s,
        Modifiers::COMMAND | Modifiers::SHIFT
    ));
    assert!(keys::matches(
        &spec("mod+shift+s"),
        &s,
        Modifiers::COMMAND | Modifiers::SHIFT
    ));
    assert!(!keys::matches(&spec("mod+s"), &s, Modifiers::empty()));
    assert!(keys::matches(
        &spec("escape"),
        &Key::Named(Named::Escape),
        Modifiers::empty()
    ));
    // `+` needs shift on most keyboards, which `mod++` does not ask for.
    assert!(keys::matches(
        &spec("mod++"),
        &Key::Character("+".into()),
        Modifiers::COMMAND | Modifiers::SHIFT
    ));
}

#[test]
fn radio_buttons_send_the_value_chosen_and_a_disabled_one_nothing() {
    let mut radio = ui::Radio::new(
        vec![
            ui::SelectOption::new("mine", "Yours"),
            ui::SelectOption::new("disk", "On disk").disabled(true),
        ],
        Some("disk".into()),
    );
    radio.on_select = Some(HandlerId(8));
    let given = ui::Element::Radio(radio);
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    // Fourteen points a button, six between them; a radio's label is not a
    // text a search finds.
    click_at(&mut drawn, 7.0, 7.0);
    click_at(&mut drawn, 7.0, 27.0);
    let sent: Vec<KitMessage> = drawn.into_messages().collect();
    assert_eq!(
        handled(&sent, 8),
        vec![Some(EventValue::Text("mine".into()))],
        "{sent:?}"
    );
}

fn long(lines: u32, total: Option<u32>, first: u32) -> ui::Log {
    let mut log = ui::Log::new(
        (first..first + lines)
            .map(|n| ui::LogLine::new(format!("line {n}")))
            .collect(),
    )
    .id("job");
    log.total = total;
    log.first = first;
    log.on_range = Some(HandlerId(9));
    log
}

#[test]
fn a_long_log_draws_the_lines_in_view_and_not_the_others() {
    let given = ui::Element::Log(long(20_000, None, 0));
    let widgets = WidgetState::default();
    let mut drawn = simulate(draw(&widgets, &given));
    drawn.find("line 3").expect("a line in view");
    assert!(
        drawn.find("line 19999").is_err(),
        "a line far below is not drawn"
    );
    let mut scrolled = WidgetState::default();
    scrolled
        .scrolls
        .insert("job".into(), (4.0 + 17.0 * 15_000.0, 240.0));
    let mut drawn = simulate(draw(&scrolled, &given));
    drawn
        .find("line 15004")
        .expect("a line in view once scrolled");
    assert!(drawn.find("line 3").is_err());
}

#[test]
fn a_log_given_in_parts_asks_for_the_lines_coming_into_view() {
    let root = ui::Element::Log(long(400, Some(50_000), 1000));
    // In view: what was given, nothing to ask.
    assert_eq!(range_wanted(&root, "job", 4.0 + 1100.0 * 17.0, 240.0), None);
    let (asked, handler) = range_wanted(&root, "job", 4.0 + 30_000.0 * 17.0, 240.0).expect("asked");
    assert_eq!(handler, HandlerId(9));
    assert!(asked.start <= 30_000 && asked.end >= 30_015, "{asked:?}");
    assert!(asked.end <= 50_000);
    assert_eq!(range_wanted(&root, "other", 0.0, 240.0), None);
}

#[test]
fn a_text_area_with_line_numbers_numbers_its_lines() {
    let mut area = ui::TextArea::new("editor", "a = 1\nb = \nc = 3")
        .line_numbers()
        .mark(ui::LineMark::new(2, ui::Notice::Error).label("a value is missing"));
    area.on_input = Some(HandlerId(10));
    let given = ui::Element::TextArea(area);
    let mut widgets = WidgetState::default();
    widgets.sync(
        &ui::abi::View {
            seq: 7,
            root: given.clone(),
        },
        &NoImages,
        &[],
        &crate::tab::Held::default(),
    );
    let mut drawn = simulate(draw(&widgets, &given));
    for number in ["1", "2", "3"] {
        drawn.find(number).expect("a line's number");
    }
}

#[test]
fn a_workspace_image_needs_files_read_and_a_held_one_is_the_tabs() {
    let picture =
        |source: ImageSource| ui::Element::Image(ui::Image::new(source).description("a capture"));
    let mut held = crate::tab::Held::default();
    let id = held.hold(b"\x89PNG\r\n\x1a\n not much of one".to_vec());
    let page = ui::Element::Column(ui::Column::new(vec![
        picture(ImageSource::File("shot.png".into())),
        picture(ImageSource::Held(id.clone())),
    ]));
    let mut widgets = WidgetState::default();
    widgets.sync(&ui::abi::View { seq: 7, root: page }, &NoImages, &[], &held);
    assert!(matches!(
        widgets.images.get(&ImageSource::File("shot.png".into())),
        Some(Picture::Missing(Missing::Refused(why))) if why.contains("files.read")
    ));
    assert!(matches!(
        widgets.images.get(&ImageSource::Held(id)),
        Some(Picture::Raster(_))
    ));
}
