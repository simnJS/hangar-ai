use serde_json::json;

use crate::abi::{Input, Outcome, Output, View};
use crate::api::{self, KitError};
use crate::*;

#[test]
fn a_view_reads_back_as_written() {
    let root: Element = Column::new(vec![
        Header::new("CI").into(),
        Button::new("Refresh").primary().small().into(),
        Table::new(
            vec![
                TableColumn::new("Job").sortable(),
                TableColumn::new("Time").numeric(),
            ],
            vec![TableRow::new(vec![
                Cell::new("rust"),
                Cell::new("4 min").key(256.0),
            ])],
        )
        .id("jobs")
        .into(),
    ])
    .spacing(8.0)
    .into();
    let view = View { seq: 3, root };
    let text = serde_json::to_string(&view).unwrap();
    assert_eq!(serde_json::from_str::<View>(&text).unwrap(), view);
}

#[test]
fn the_json_is_the_documented_one() {
    let mut button = Button::new("Go");
    button.on_press = Some(HandlerId(7));
    let element: Element = button.into();
    assert_eq!(
        serde_json::to_value(&element).unwrap(),
        json!({ "type": "button", "label": "Go", "onPress": 7 })
    );
    let width: Length = serde_json::from_value(json!({ "px": 120 })).unwrap();
    assert_eq!(width, Length::Px(120.0));
}

#[test]
fn a_widget_of_a_newer_hangar_is_unsupported_not_an_error() {
    let column: Element = serde_json::from_value(json!({
        "type": "column",
        "children": [
            { "type": "hologram", "depth": 3, "children": [] },
            { "type": "text", "text": "still here", "sparkle": true }
        ]
    }))
    .unwrap();
    let Element::Column(column) = column else {
        panic!("a column")
    };
    assert_eq!(column.children[0], Element::Unsupported);
    assert!(matches!(&column.children[1], Element::Text(text) if text.text == "still here"));
}

#[test]
fn replies_read_with_their_result_or_their_error() {
    let ok: Input =
        serde_json::from_value(json!({ "type": "reply", "id": 4, "result": null })).unwrap();
    assert!(
        matches!(ok, Input::Reply(reply) if reply.id == 4 && reply.outcome == Outcome::Result(json!(null)))
    );
    let refused: Input = serde_json::from_value(
        json!({ "type": "reply", "id": 5, "error": { "code": "denied", "message": "no" } }),
    )
    .unwrap();
    assert!(matches!(
        refused,
        Input::Reply(reply) if reply.outcome == Outcome::Error(KitError::new(KitError::DENIED, "no"))
    ));
    let text = serde_json::to_string(&Input::Reply(crate::abi::Reply {
        id: 9,
        outcome: Outcome::Result(json!([1])),
    }))
    .unwrap();
    assert_eq!(text, r#"{"type":"reply","id":9,"result":[1]}"#);
}

#[test]
fn an_empty_output_is_an_empty_object() {
    assert_eq!(serde_json::to_string(&Output::default()).unwrap(), "{}");
}

#[test]
fn map_handlers_meets_them_depth_first() {
    let mut first = Button::<u32>::new("a");
    first.on_press = Some(10);
    let mut input = TextInput::<u32>::new("", "");
    input.on_input = Some(20);
    input.on_submit = Some(30);
    let tree: Element<u32> =
        Column::new(vec![first.into(), Row::new(vec![input.into()]).into()]).into();
    let mut seen = Vec::new();
    let mapped = tree.map_handlers(&mut |handler| {
        seen.push(handler);
        HandlerId(seen.len() as u32 - 1)
    });
    assert_eq!(seen, vec![10, 20, 30]);
    let text = serde_json::to_string(&mapped).unwrap();
    assert!(text.contains(r#""onSubmit":2"#), "{text}");
}

#[test]
fn permissions_are_read_as_kits_ts_reads_them() {
    let granted = vec![
        "panes.read".to_owned(),
        "loopback:8765".to_owned(),
        "command:ci-run".to_owned(),
    ];
    assert!(api::may_call("panes.list", &json!(null), &granted));
    assert!(!api::may_call("panes.send", &json!({}), &granted));
    assert!(api::may_call(
        "loopback.fetch",
        &json!({ "port": 8765 }),
        &granted
    ));
    assert!(!api::may_call(
        "loopback.fetch",
        &json!({ "port": 8766 }),
        &granted
    ));
    assert!(api::may_call(
        "command.run",
        &json!({ "name": "ci-run" }),
        &granted
    ));
    assert!(!api::may_call(
        "command.run",
        &json!({ "name": "other" }),
        &granted
    ));
    assert!(api::may_call("tab.setTitle", &json!({}), &[]));
    assert!(!api::may_call("files.delete", &json!({}), &granted));
    assert_eq!(api::loopback_port("loopback:08765"), None);
    assert_eq!(api::loopback_port("loopback:80"), None);
    assert_eq!(api::loopback_port("loopback:65535"), Some(65535));
    assert_eq!(api::command_name("command:Ci"), None);
    assert!(api::may_hear("tab.params", &[]));
    assert!(!api::may_hear("panes.changed", &[]));
}

#[test]
fn sort_turns_around_on_the_same_column() {
    assert_eq!(Sort::next(None, 2), Sort::ascending(2));
    assert_eq!(Sort::next(Some(Sort::ascending(2)), 2), Sort::descending(2));
    assert_eq!(Sort::next(Some(Sort::descending(2)), 1), Sort::ascending(1));
}

#[test]
fn a_canvas_and_its_marks_are_the_documented_json() {
    let mut canvas: Canvas = Canvas::new("shot", 1280.0, 720.0)
        .background(ImageSource::File("captures/a.png".into()))
        .tool(Some(Tool::Arrow))
        .ink(Ink::Blue)
        .mark(Mark::arrow((10.0, 20.0), (300.0, 40.0)).ink(Ink::Red))
        .mark(
            Mark::text((40.0, 50.0), "1", 18.0)
                .bold()
                .align(Align::Center)
                .fill(Ink::Rgb([0x11, 0x22, 0x33])),
        );
    canvas.on_draw = Some(HandlerId(2));
    let element: Element = canvas.into();
    let value = serde_json::to_value(&element).unwrap();
    assert_eq!(
        value,
        json!({
            "type": "canvas", "id": "shot", "width": 1280.0, "height": 720.0,
            "background": { "file": "captures/a.png" },
            "marks": [
                { "type": "line", "from": { "x": 10.0, "y": 20.0 }, "to": { "x": 300.0, "y": 40.0 }, "arrow": true, "ink": "red" },
                { "type": "text", "at": { "x": 40.0, "y": 50.0 }, "text": "1", "size": 18.0, "bold": true, "align": "center", "ink": "red", "fill": "#112233" }
            ],
            "tool": "arrow", "ink": "blue", "onDraw": 2
        })
    );
    assert_eq!(serde_json::from_value::<Element>(value).unwrap(), element);
}

#[test]
fn a_mark_of_a_newer_hangar_and_an_ink_it_does_not_know_still_read() {
    let mark: Mark =
        serde_json::from_value(json!({ "type": "star", "points": 5, "ink": "chartreuse" }))
            .unwrap();
    assert_eq!(mark.shape, Shape::Unsupported);
    assert_eq!(mark.ink, Ink::Black);
    let ink: Ink = serde_json::from_value(json!("#E5484D")).unwrap();
    assert_eq!(ink.rgb(), Ink::Red.rgb());
    assert_eq!(Ink::Yellow.contrast(), Ink::Black);
    assert_eq!(Ink::Blue.contrast(), Ink::White);
}

#[test]
fn a_canvas_fits_its_room_and_never_grows_past_its_size() {
    let canvas: Canvas = Canvas::new("c", 2000.0, 1000.0);
    assert_eq!(canvas.fit(500.0), 0.25);
    assert_eq!(canvas.fit(4000.0), 0.48);
    assert_eq!(Canvas::<HandlerId>::new("c", 100.0, 100.0).fit(4000.0), 1.0);
    assert_eq!(canvas.line_width(), 8.0);
    assert_eq!(default_stroke(100.0, 80.0), 2.0);
    assert_eq!(
        Mark::path([Point::new(4.0, 9.0), Point::new(1.0, 12.0)]).bounds(),
        Some([1.0, 9.0, 3.0, 3.0])
    );
}

#[test]
fn shortcuts_read_as_written_and_write_one_way() {
    use crate::keys::{Key, NamedKey, Shortcut};
    let save = Shortcut::parse("Mod+S").unwrap();
    assert_eq!(save.key, Key::Char('s'));
    assert!(save.command && !save.shift && !save.ctrl && !save.alt);
    assert_eq!(save.to_string(), "mod+s");
    assert_eq!(
        Shortcut::parse("shift+mod+z").unwrap().to_string(),
        "mod+shift+z"
    );
    assert_eq!(Shortcut::parse("mod++").unwrap().key, Key::Char('+'));
    assert_eq!(Shortcut::parse("+").unwrap().key, Key::Char('+'));
    assert_eq!(
        Shortcut::parse("escape").unwrap().key,
        Key::Named(NamedKey::Escape)
    );
    assert_eq!(
        Shortcut::parse("f5").unwrap().key,
        Key::Named(NamedKey::F(5))
    );
    for wrong in ["", "mod+", "hyper+s", "mod+mod+s", "f13", "mod+save"] {
        assert_eq!(Shortcut::parse(wrong), None, "{wrong:?}");
    }
}

#[test]
fn an_output_carries_operations_and_a_newer_one_is_ignored() {
    let output: Output = serde_json::from_value(json!({
        "operations": [
            { "type": "focus", "id": "note-2" },
            { "type": "scrollTo", "id": "log", "line": 340 },
            { "type": "cursorTo", "id": "editor", "line": 12 },
            { "type": "teleport", "where": "moon" }
        ],
        "subscriptions": { "shortcuts": ["mod+s"] }
    }))
    .unwrap();
    assert_eq!(
        output.operations,
        vec![
            abi::Operation::Focus {
                id: "note-2".into()
            },
            abi::Operation::ScrollTo {
                id: "log".into(),
                line: Some(340),
                y: None,
                end: false
            },
            abi::Operation::CursorTo {
                id: "editor".into(),
                line: 12,
                column: 1
            },
            abi::Operation::Unsupported,
        ]
    );
    assert_eq!(output.subscriptions.unwrap().shortcuts, vec!["mod+s"]);
    let input: Input =
        serde_json::from_value(json!({ "type": "shortcut", "keys": "mod+s" })).unwrap();
    assert_eq!(
        input,
        Input::Shortcut(abi::ShortcutPressed {
            keys: "mod+s".into()
        })
    );
}

#[test]
fn a_long_log_says_which_lines_it_holds() {
    let log: Log = Log::new(vec![LogLine::new("a"), LogLine::new("b")])
        .id("job")
        .window(400, 20_000);
    let element: Element = log.into();
    assert_eq!(
        serde_json::to_value(&element).unwrap(),
        json!({
            "type": "log", "id": "job", "first": 400, "total": 20000,
            "lines": [{ "text": "a" }, { "text": "b" }]
        })
    );
    let value: EventValue =
        serde_json::from_value(json!({ "range": { "start": 200, "end": 800 } })).unwrap();
    assert_eq!(
        value,
        EventValue::Range(Range {
            start: 200,
            end: 800
        })
    );
}

#[test]
fn the_native_only_methods_need_what_they_say() {
    let granted = vec!["files.read".to_owned()];
    assert!(api::may_call(api::method::FILES_PICK, &json!({}), &granted));
    assert!(!api::may_call(
        api::method::CANVAS_EXPORT,
        &json!({}),
        &granted
    ));
    assert!(api::may_call(api::method::UI_PASTE_IMAGE, &json!({}), &[]));
    assert!(api::NATIVE_ONLY.contains(&api::method::CANVAS_EXPORT));
    let start: abi::Start = serde_json::from_value(json!({
        "abi": 1, "hangar": "2.0.0",
        "kit": { "id": "k", "name": "K", "version": "1" },
        "tab": { "id": "t" },
        "theme": { "scheme": "dark", "locale": "fr" },
        "utcOffset": 60
    }))
    .unwrap();
    assert_eq!(start.utc_offset, 60);
}
