//! What `NeedsYou.tsx` keeps between renders: a failure belongs to the row it
//! happened on, and a landing that never answers must not hold its button for
//! ever.

use std::time::Duration;

use serde_json::json;

use super::*;
use model::{ItemBase, NeedsYouItem};

fn land(key: &str) -> NeedsYouItem {
    NeedsYouItem::Land {
        base: ItemBase {
            key: key.into(),
            workspace_id: "w".into(),
            workspace_name: "w".into(),
        },
        cwd: "/p".into(),
        task: serde_json::from_value(json!({
            "id": "1", "title": "Ship it", "column": "review",
            "created_at": 1, "updated_at": 1
        }))
        .unwrap(),
    }
}

#[test]
fn a_row_that_left_the_list_takes_its_failure_and_its_landing_with_it() {
    let mut st = State::default();
    st.failures
        .insert("task:/p:1:ready-to-land".into(), "conflict".into());
    st.failures
        .insert("task:/p:2:ready-to-land".into(), "conflict".into());
    st.landing
        .insert("task:/p:2:ready-to-land".into(), Instant::now());

    st.forget_gone(&[land("task:/p:1:ready-to-land")]);

    assert_eq!(st.failure("task:/p:1:ready-to-land"), Some("conflict"));
    assert_eq!(st.failure("task:/p:2:ready-to-land"), None);
    assert!(!st.is_landing("task:/p:2:ready-to-land", Instant::now()));
}

#[test]
fn a_delivery_asked_to_land_says_so_for_a_while() {
    let mut st = State::default();
    let now = Instant::now();
    st.landing.insert("k".into(), now);
    assert!(st.is_landing("k", now));
    assert!(st.is_landing("k", now + Duration::from_secs(10)));
    assert!(!st.is_landing("other", now));
}

#[test]
fn a_landing_that_never_answers_lets_the_person_try_again() {
    let mut st = State::default();
    let now = Instant::now();
    st.landing.insert("k".into(), now);
    assert!(!st.is_landing("k", now + LAND_PATIENCE + Duration::from_secs(1)));
}

#[test]
fn the_panel_is_closed_until_asked() {
    // The flag is the shell's (`Model.ui.needs_you_open`); this package only
    // flips it, so a fresh model starts closed.
    let model = Model::default();
    assert!(!model.ui.needs_you_open);
}
