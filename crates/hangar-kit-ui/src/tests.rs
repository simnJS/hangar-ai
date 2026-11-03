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
