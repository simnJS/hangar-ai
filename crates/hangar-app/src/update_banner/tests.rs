//! The banner's rules: when it shows, what it counts, and what stays silent.

use hangar_update::State as Updater;

use super::*;

fn release() -> ReleaseInfo {
    ReleaseInfo {
        version: "1.13.0".into(),
        notes: None,
        date: None,
    }
}

#[test]
fn nothing_shows_until_a_newer_release_is_found() {
    assert_eq!(stage_of(&Updater::Idle), Phase::Idle);
    assert_eq!(stage_of(&Updater::Checking), Phase::Idle);
    assert_eq!(stage_of(&Updater::Available(release())), Phase::Available);
}

#[test]
fn a_check_that_fails_is_silent() {
    // No network, or a build with no release feed, never reaches the person.
    let failed = Updater::Error {
        stage: Stage::Check,
        message: "dns error".into(),
    };
    assert_eq!(stage_of(&failed), Phase::Idle);
}

#[test]
fn a_download_or_an_install_that_fails_is_shown() {
    for stage in [Stage::Download, Stage::Install] {
        let failed = Updater::Error {
            stage,
            message: "the signature does not match".into(),
        };
        assert_eq!(stage_of(&failed), Phase::Failed);
    }
}

#[test]
fn a_downloaded_update_waits_to_be_installed() {
    assert_eq!(stage_of(&Updater::Ready(release())), Phase::Ready);
    // Between the click and the end of the process it still reads as ready.
    assert_eq!(stage_of(&Updater::Installing(release())), Phase::Ready);
    assert_eq!(stage_of(&Updater::Installed(release())), Phase::Ready);
}

#[test]
fn progress_is_whole_percent_and_zero_while_the_size_is_unknown() {
    assert_eq!(progress_of(0, Some(200)), 0);
    assert_eq!(progress_of(50, Some(200)), 25);
    assert_eq!(progress_of(199, Some(200)), 100);
    assert_eq!(progress_of(200, Some(200)), 100);
    assert_eq!(progress_of(5_000, None), 0);
    assert_eq!(progress_of(5_000, Some(0)), 0);
}

#[test]
fn progress_never_passes_the_whole() {
    // A server that sends more than it announced.
    assert_eq!(progress_of(300, Some(200)), 100);
}

#[test]
fn the_banner_follows_the_updater_through_a_whole_update() {
    let mut st = State::default();
    st.take(&Updater::Available(release()));
    assert_eq!(st.stage(), Phase::Available);
    assert_eq!(st.release.as_ref().unwrap().version, "1.13.0");

    st.take(&Updater::Downloading {
        release: release(),
        bytes: 10,
        total: Some(40),
    });
    assert_eq!(st.stage(), Phase::Downloading);
    assert_eq!(st.progress, 25);

    st.take(&Updater::Ready(release()));
    assert_eq!(st.stage(), Phase::Ready);
    assert_eq!(st.progress, 100);
}

#[test]
fn a_failure_keeps_the_release_and_says_why() {
    let mut st = State::default();
    st.take(&Updater::Available(release()));
    st.take(&Updater::Error {
        stage: Stage::Download,
        message: "connection reset".into(),
    });
    assert_eq!(st.stage(), Phase::Failed);
    assert_eq!(st.error.as_deref(), Some("connection reset"));
    // The offer it comes back to still names its version.
    assert_eq!(st.release.as_ref().unwrap().version, "1.13.0");
}

#[test]
fn a_silent_check_failure_leaves_no_error_behind() {
    let mut st = State::default();
    st.take(&Updater::Error {
        stage: Stage::Check,
        message: "offline".into(),
    });
    assert_eq!(st.stage(), Phase::Idle);
    assert_eq!(st.error, None);
}

#[test]
fn a_restart_that_fails_says_why_and_can_be_tried_again() {
    let mut st = State::default();
    st.take(&Updater::Ready(release()));
    assert!(st.begin_restart());
    assert!(
        !st.begin_restart(),
        "a restart under way is not started twice"
    );

    // pkexec dismissed: the updater reports the install error, then the
    // restart's own result arrives.
    st.take(&Updater::Installing(release()));
    st.take(&Updater::Error {
        stage: Stage::Install,
        message: "administrator rights were not granted".into(),
    });
    st.restart_failed("install failed: administrator rights were not granted".into());
    assert_eq!(st.stage(), Phase::Failed);
    assert!(!st.restarting, "the failure unsticks the button");
    assert_eq!(
        st.error.as_deref(),
        Some("install failed: administrator rights were not granted")
    );

    // Retry, the download again, and Restart is live once more.
    st.retry();
    assert_eq!(st.stage(), Phase::Available);
    st.take(&Updater::Downloading {
        release: release(),
        bytes: 0,
        total: Some(10),
    });
    st.take(&Updater::Ready(release()));
    assert_eq!(st.stage(), Phase::Ready);
    assert!(st.begin_restart());
}

#[test]
fn a_relaunch_that_fails_after_the_install_is_shown_too() {
    // The updater is at Installed, which reads as ready; only the restart's
    // result says it went wrong.
    let mut st = State::default();
    st.take(&Updater::Ready(release()));
    assert!(st.begin_restart());
    st.take(&Updater::Installed(release()));
    st.restart_failed("starting the new version: no such file".into());
    assert_eq!(st.stage(), Phase::Failed);
    assert!(!st.restarting);

    st.retry();
    // The download that follows has nothing to do and reports the state it
    // found, Installed, which offers Restart again.
    st.take(&Updater::Installed(release()));
    assert_eq!(st.stage(), Phase::Ready);
    assert!(st.begin_restart());
}
