//! The banner driven with real pointer events, one stage at a time.

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{ctx, model_of, simulate};
use crate::state::types::AppState;

fn state(stage: Phase) -> State {
    State {
        stage,
        release: Some(ReleaseInfo {
            version: "1.13.0".into(),
            notes: None,
            date: None,
        }),
        progress: 42,
        error: Some("the signature does not match".into()),
        ..State::default()
    }
}

fn run(st: &State, drive: impl FnOnce(&mut iced_test::Simulator<'_, Message>)) -> Vec<Msg> {
    let model = model_of(AppState::default());
    let pkg = Packages::default();
    let mut ui = simulate(view(st, ctx(&model, &pkg)));
    drive(&mut ui);
    ui.into_messages()
        .filter_map(|message| match message {
            Message::UpdateBanner(msg) => Some(msg),
            _ => None,
        })
        .collect()
}

fn finds(st: &State, words: &str) -> bool {
    let model = model_of(AppState::default());
    let pkg = Packages::default();
    simulate(view(st, ctx(&model, &pkg))).find(words).is_ok()
}

#[test]
fn nothing_shows_while_there_is_nothing_to_offer() {
    let st = State::default();
    assert!(!finds(&st, "Update"));
    assert!(!finds(&state(Phase::Idle), "Update"));
}

#[test]
fn an_available_update_names_its_version_and_offers_later_or_update() {
    let st = state(Phase::Available);
    assert!(finds(&st, "Version 1.13.0 available"));
    let later = run(&st, |ui| {
        ui.click("Later").unwrap();
    });
    assert!(matches!(later.as_slice(), [Msg::Later]), "{later:?}");
    let install = run(&st, |ui| {
        ui.click("Update").unwrap();
    });
    assert!(matches!(install.as_slice(), [Msg::Install]), "{install:?}");
}

#[test]
fn a_download_shows_its_progress_and_offers_nothing() {
    let st = state(Phase::Downloading);
    assert!(finds(&st, "Downloading… 42%"));
    assert!(!finds(&st, "Later"));
}

#[test]
fn a_download_of_unknown_size_says_only_that_it_is_downloading() {
    let mut st = state(Phase::Downloading);
    st.progress = 0;
    assert!(finds(&st, "Downloading…"));
}

#[test]
fn a_ready_update_waits_for_a_restart() {
    let st = state(Phase::Ready);
    assert!(finds(&st, "Update installed"));
    let restart = run(&st, |ui| {
        ui.click("Restart").unwrap();
    });
    assert!(matches!(restart.as_slice(), [Msg::Restart]), "{restart:?}");
}

#[test]
fn a_restart_that_is_under_way_is_not_asked_twice() {
    let mut st = state(Phase::Ready);
    st.restarting = true;
    let restart = run(&st, |ui| {
        ui.click("Restart").unwrap();
    });
    assert!(restart.is_empty(), "{restart:?}");
}

#[test]
fn a_failure_says_so_and_offers_to_retry() {
    let st = state(Phase::Failed);
    assert!(finds(&st, "Update failed"));
    let retry = run(&st, |ui| {
        ui.click("Retry").unwrap();
    });
    assert!(matches!(retry.as_slice(), [Msg::Retry]), "{retry:?}");
}
