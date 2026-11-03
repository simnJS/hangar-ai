//! The vitest suites of control.ts, managerControl.ts and deliveryControl.ts,
//! case for case, against the fake port (fake.rs).
//!
//! Left to the packages that own what they test: `readLines` (P5's
//! terminals), the preview store's own behaviour behind `localModelHost`
//! (P15: a closed panel opened by `reveal`, who opened a tab as its page is
//! told), the kit tabs' rules (P16's `kitTabToOpen`/`readKitTabParams`:
//! changes to review, a kit gone or broken, unknown tabs and params, a
//! param's pattern), and `workspaceFromGit`'s flow (P8: where a clone goes,
//! the folders that can never take one). What control.ts itself decides
//! around each of them is here.

mod delivery;
mod kit_tabs;
mod manager;
mod panes;
mod preview;
mod workspace;

use std::sync::Arc;

use serde_json::{Value, json};

use super::fake::Fake;
use super::run::{Gate, run};

/// Ava, the Claude Code pane of "Hangar", calling.
fn from_ava() -> Value {
    json!({"caller": {"paneId": "p1", "paneName": "Ava", "cwd": "C:\\dev\\hangar"}})
}

/// Ava of "Shop" calling.
fn from_shop() -> Value {
    json!({"caller": {"paneId": "p3", "paneName": "Ava", "cwd": "C:\\dev\\shop"}})
}

/// `{ ...base, ...extra }`.
fn with(base: Value, extra: Value) -> Value {
    let (Value::Object(mut base), Value::Object(extra)) = (base, extra) else {
        panic!("objects only");
    };
    base.extend(extra);
    Value::Object(base)
}

async fn control(op: &str, args: Value, fake: &Arc<Fake>) -> Result<Value, String> {
    run(op, args, &fake.port(), Gate::Agent).await
}

/// The refusal of an operation that must fail.
async fn refusal(op: &str, args: Value, fake: &Arc<Fake>) -> String {
    match control(op, args, fake).await {
        Ok(answer) => panic!("{op} answered {answer} instead of refusing"),
        Err(error) => error,
    }
}

/// `expect(actual).toMatchObject(expected)`: every field of `expected` is in
/// `actual` with that value, objects compared the same way.
#[track_caller]
fn matches(actual: &Value, expected: Value) {
    fn walk(actual: &Value, expected: &Value, path: &str) {
        match (actual, expected) {
            (Value::Object(actual), Value::Object(expected)) => {
                for (key, value) in expected {
                    let found = actual
                        .get(key)
                        .unwrap_or_else(|| panic!("{path}.{key} is missing in {actual:?}"));
                    walk(found, value, &format!("{path}.{key}"));
                }
            }
            _ => assert_eq!(actual, expected, "at {path}"),
        }
    }
    walk(actual, &expected, "");
}

#[track_caller]
fn contains(text: &str, part: &str) {
    assert!(text.contains(part), "{text:?} does not contain {part:?}");
}
