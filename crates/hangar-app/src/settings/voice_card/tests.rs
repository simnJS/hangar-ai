//! The parts of the voice cards that do not need the PC: the download's
//! percent, which model shows, what the key card offers.

use super::*;

fn model(id: &str, installed: bool) -> VoiceModel {
    VoiceModel {
        id: id.into(),
        label: id.into(),
        approx_bytes: 181 * 1024 * 1024,
        languages: "99".into(),
        license: "MIT".into(),
        installed,
    }
}

fn progress(id: &str, received: u64, total: u64, done: bool) -> Progress {
    Progress {
        id: id.into(),
        received,
        total,
        done,
    }
}

#[test]
fn a_download_is_a_whole_percent_and_never_past_a_hundred() {
    assert_eq!(percent(0, 100), 0);
    assert_eq!(percent(1, 3), 33);
    assert_eq!(percent(2, 3), 67);
    assert_eq!(percent(100, 100), 100);
    assert_eq!(percent(150, 100), 100);
}

#[test]
fn a_download_of_unknown_size_does_not_divide_by_zero() {
    assert_eq!(percent(0, 0), 0);
    assert_eq!(percent(10, 0), 100);
}

#[test]
fn the_card_shows_the_selected_model_else_the_first() {
    let st = State {
        models: vec![model("a", true), model("b", false)],
        ..State::default()
    };
    assert_eq!(st.model("b").map(|m| m.id.as_str()), Some("b"));
    // An id that no catalogue knows resolves to the one there is.
    assert_eq!(
        st.model("whisper-from-an-older-release")
            .map(|m| m.id.as_str()),
        Some("a")
    );
    assert!(State::default().model("a").is_none());
}

#[test]
fn a_download_in_progress_is_kept_and_a_finished_one_asks_for_the_catalogue_again() {
    let mut st = State::default();
    assert!(!st.download_event(progress("a", 10, 100, false)));
    assert_eq!(st.progress.as_ref().map(|p| p.received), Some(10));
    assert!(st.download_event(progress("a", 100, 100, true)));
    assert!(st.progress.is_none());
}

#[test]
fn the_key_can_be_typed_when_none_is_saved_or_while_one_is_replaced() {
    let mut st = State::default();
    // Not known yet: nothing to ask for.
    assert!(!st.asking());
    st.saved = Some(false);
    assert!(st.asking());
    st.saved = Some(true);
    assert!(!st.asking());
    st.replacing = true;
    assert!(st.asking());
}

#[test]
fn a_key_is_sent_without_the_spaces_around_it() {
    assert_eq!(trimmed("  gsk_abc \n"), "gsk_abc");
}

#[test]
fn the_catalogue_is_the_cores() {
    let info = models::ModelInfo {
        id: "m".into(),
        label: "Model".into(),
        approx_bytes: 5,
        languages: "99".into(),
        license: "MIT".into(),
        installed: true,
    };
    assert_eq!(
        VoiceModel::from(info),
        VoiceModel {
            approx_bytes: 5,
            label: "Model".into(),
            ..model("m", true)
        }
    );
}
