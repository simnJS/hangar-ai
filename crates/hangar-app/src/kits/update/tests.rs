//! What the messages do to the package's state and to the model.
//!
//! The persisted state is written through `Model::apply`, whose semantics are
//! the shell's (P0b); a test here reads the write as the action it hands over
//! (`store_action`) and applies that action's patch itself, so that what is
//! checked is what this package asks to be stored.

use hangar_core::kits::{Inspection, KitInfo, StepStatus};
use hangar_core::paths::Paths;

use super::*;
use crate::kits::testing::{
    COMMANDS, HOOKS, OTHER_HOOKS, entry, folder, git_kit, inspection, installed, kit, tab,
};
use crate::state::types::{AppState, Pane};

fn env() -> Env {
    let dir = std::env::temp_dir().join(format!("hangar-kits-update-{}", std::process::id()));
    Env {
        host: Host::with_paths(Paths::rooted(&dir)),
        pty: Arc::new(PtyManager::default()),
    }
}

/// A model with a workspace `w1` that has a Claude Code pane, and these kits
/// installed.
fn model_with(kits: Vec<KitInfo>) -> Model {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w1".into(),
            name: "Studio".into(),
            panes: vec![Pane {
                id: "p1".into(),
                name: "Ava".into(),
                agent: AgentId::Claude,
                ..Pane::default()
            }],
            ..Workspace::default()
        }],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });
    m.live.kits.installed = Some(kits);
    m
}

fn send(st: &mut State, m: &mut Model, msg: Msg) {
    let _ = update(st, m, &env(), msg);
}

fn shown(tabs: Vec<hangar_core::kits::Tab>) -> Inspection {
    Inspection {
        hooks_permission: Some(HOOKS.into()),
        ..inspection(tabs)
    }
}

/// Opens the activation of `hello-kit` for a workspace being created, and
/// answers its inspection.
fn opened_for_draft(st: &mut State, m: &mut Model, kit_state: &KitInfo) {
    send(
        st,
        m,
        Msg::Toggle {
            ws: None,
            kit: kit_state.id.clone(),
            on: true,
        },
    );
    send(
        st,
        m,
        Msg::Inspected(
            kit_state.id.clone(),
            Ok(Box::new(shown(vec![tab(&["panes.read"])]))),
        ),
    );
}

#[test]
fn switching_a_kit_on_opens_its_activation_and_stores_nothing_yet() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::Toggle {
            ws: Some("w1".into()),
            kit: "hello-kit".into(),
            on: true,
        },
    );
    let act = st.activation.as_ref().expect("an activation");
    assert_eq!(act.kit, "hello-kit");
    assert_eq!(act.target, Target::Workspace("w1".into()));
    assert_eq!(act.setup_workspace(), Some(&"w1".to_string()));
    assert!(st.draft.is_empty());
    assert!(act.inspection.is_none());
}

#[test]
fn a_kit_that_is_not_installed_opens_nothing() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::Toggle {
            ws: None,
            kit: "ghost".into(),
            on: true,
        },
    );
    assert!(st.activation.is_none());
}

#[test]
fn a_workspace_being_created_has_no_pane_to_run_a_setup_step_in() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    opened_for_draft(&mut st, &mut m, &one);
    let act = st.activation.as_ref().expect("an activation");
    assert_eq!(act.target, Target::Draft);
    assert_eq!(act.setup_workspace(), None);
}

#[test]
fn enabling_everywhere_runs_setup_steps_in_the_active_workspace() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::ToggleEverywhere {
            kit: "hello-kit".into(),
            on: true,
        },
    );
    let act = st.activation.as_ref().expect("an activation");
    assert_eq!(act.target, Target::Everywhere);
    assert_eq!(act.setup_workspace(), Some(&"w1".to_string()));
}

#[test]
fn a_kit_that_cannot_be_used_cannot_be_enabled_everywhere() {
    let mut broken = kit("1.0.0", vec![]);
    broken.error = Some("needs a newer Hangar".into());
    let mut m = model_with(vec![broken]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::ToggleEverywhere {
            kit: "hello-kit".into(),
            on: true,
        },
    );
    assert!(st.activation.is_none());
}

#[test]
fn accepting_for_a_workspace_being_created_keeps_what_the_person_was_shown() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    opened_for_draft(&mut st, &mut m, &one);
    // The kit changed under the open dialog.
    m.live.kits.installed = Some(vec![git_kit(Some(OTHER_HOOKS), true)]);
    send(&mut st, &mut m, Msg::Accept);
    assert!(st.activation.is_none());
    assert_eq!(
        st.draft,
        vec![entry("hello-kit", &["panes.read", HOOKS])],
        "the hash is the inspection's, not the kit's as it is now"
    );
    assert_eq!(crate::kits::draft_kits(&st), st.draft.as_slice());
}

#[test]
fn switching_a_kit_off_removes_it_at_once() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State {
        draft: vec![entry("hello-kit", &[]), entry("other-kit", &[])],
        ..State::default()
    };
    send(
        &mut st,
        &mut m,
        Msg::Toggle {
            ws: None,
            kit: "hello-kit".into(),
            on: false,
        },
    );
    assert_eq!(st.draft, vec![entry("other-kit", &[])]);
    send(
        &mut st,
        &mut m,
        Msg::RemoveGone {
            ws: None,
            kit: "other-kit".into(),
        },
    );
    assert!(st.draft.is_empty());
}

#[test]
fn writes_a_workspace_s_kits_as_they_are_empty_included() {
    let mut workspace = Workspace::default();
    let Some(StoreAction::UpdateWorkspace(id, patch)) = store_action(
        &Target::Workspace("w1".into()),
        vec![entry("a-kit", &["notify"])],
    ) else {
        panic!("a workspace is persisted");
    };
    assert_eq!(id, "w1");
    patch.apply(&mut workspace);
    assert_eq!(workspace.kits, Some(vec![entry("a-kit", &["notify"])]));

    let Some(StoreAction::UpdateWorkspace(_, patch)) =
        store_action(&Target::Workspace("w1".into()), vec![])
    else {
        panic!("a workspace is persisted");
    };
    patch.apply(&mut workspace);
    assert_eq!(workspace.kits, Some(vec![]));
}

#[test]
fn writes_the_kits_of_all_workspaces_into_the_settings() {
    let mut settings = Settings::default();
    let Some(StoreAction::UpdateSettings(patch)) =
        store_action(&Target::Everywhere, vec![entry("a-kit", &[])])
    else {
        panic!("the settings are persisted");
    };
    patch.apply(&mut settings);
    assert_eq!(settings.kits_everywhere, Some(vec![entry("a-kit", &[])]));
    assert!(store_action(&Target::Draft, vec![]).is_none());
}

#[test]
fn a_new_workspace_dialog_starts_from_no_kits() {
    let mut m = model_with(vec![]);
    let mut st = State {
        draft: vec![entry("hello-kit", &[])],
        restart: Some(Restart {
            ws: "w1".into(),
            kit_name: "Hello kit".into(),
        }),
        restarted: Some("w1".into()),
        ..State::default()
    };
    let acted = on_intent(
        &mut st,
        &mut m,
        &Intent::OpenOverlay(Overlay::WorkspaceDialog),
    );
    assert!(acted.is_none(), "it never claims an intent");
    assert!(st.draft.is_empty());
    assert!(st.restart.is_none());
    assert!(st.restarted.is_none());
}

#[test]
fn the_activation_goes_when_its_overlay_does() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    opened_for_draft(&mut st, &mut m, &one);

    // Open: the shell pushed the overlay.
    m.ui.overlays.push(Overlay::KitActivation {
        kit: "hello-kit".into(),
        ws: None,
    });
    on_intent(&mut st, &mut m, &Intent::CloseOverlay);
    assert!(
        st.activation.is_some(),
        "another close with the overlay still there"
    );

    // The shell popped it.
    m.ui.overlays.clear();
    on_intent(&mut st, &mut m, &Intent::CloseOverlay);
    assert!(st.activation.is_none());
}

#[test]
fn reads_the_inspection_and_the_steps_into_the_dialog() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    opened_for_draft(&mut st, &mut m, &one);
    send(
        &mut st,
        &mut m,
        Msg::StepsRead("hello-kit".into(), Err("no setup".into())),
    );
    let act = st.activation.as_ref().expect("an activation");
    assert!(act.inspection.is_some());
    assert_eq!(
        act.steps,
        Some(vec![]),
        "steps that cannot be read are none"
    );
    // An answer for another kit is not this dialog's.
    send(
        &mut st,
        &mut m,
        Msg::Inspected("other-kit".into(), Err("boom".into())),
    );
    assert!(st.activation.as_ref().expect("still open").error.is_none());
    send(
        &mut st,
        &mut m,
        Msg::Inspected("hello-kit".into(), Err("boom".into())),
    );
    assert_eq!(
        st.activation.as_ref().expect("open").error.as_deref(),
        Some("Error: boom")
    );
}

#[test]
fn a_kit_removed_under_its_dialog_takes_the_dialog_with_it() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    opened_for_draft(&mut st, &mut m, &one);
    send(
        &mut st,
        &mut m,
        Msg::Installed(Ok(vec![installed("another-kit")])),
    );
    assert!(st.activation.is_none());
    assert_eq!(m.live.kits.kits().len(), 1);
}

#[test]
fn a_change_to_the_kit_s_hooks_or_commands_under_its_dialog_reads_it_again() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one.clone()]);
    let mut st = State::default();
    opened_for_draft(&mut st, &mut m, &one);
    let seen = st.activation.as_ref().expect("open").seen.clone();
    assert_eq!(seen, (Some(HOOKS.to_owned()), None));

    // Nothing changed: nothing to ask again.
    send(&mut st, &mut m, Msg::Installed(Ok(vec![one])));
    assert_eq!(st.activation.as_ref().expect("open").seen, seen);

    let mut changed = git_kit(Some(OTHER_HOOKS), true);
    changed.commands_permission = Some(COMMANDS.into());
    send(&mut st, &mut m, Msg::Installed(Ok(vec![changed])));
    assert_eq!(
        st.activation.as_ref().expect("open").seen,
        (Some(OTHER_HOOKS.to_owned()), Some(COMMANDS.to_owned()))
    );
}

#[test]
fn a_list_that_could_not_be_read_keeps_the_list_there_was() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::Installed(Err("the index cannot be read".into())),
    );
    assert_eq!(m.live.kits.kits().len(), 1);
    assert_eq!(
        m.live.kits.error.as_deref(),
        Some("the index cannot be read")
    );
    send(&mut st, &mut m, Msg::Installed(Ok(vec![])));
    assert_eq!(m.live.kits.error, None);
    assert!(m.live.kits.installed.as_ref().is_some_and(Vec::is_empty));
}

#[test]
fn the_first_read_is_loading_until_it_answers() {
    let mut m = Model::default();
    assert!(m.live.kits.loading());
    let mut st = State::default();
    send(&mut st, &mut m, Msg::Installed(Ok(vec![])));
    assert!(!m.live.kits.loading());
    let mut failed = Model::default();
    send(&mut st, &mut failed, Msg::Installed(Err("no home".into())));
    assert!(!failed.live.kits.loading());
}

// -- setup steps -----------------------------------------------------------------------

fn step(id: &str, command: &str) -> StepStatus {
    StepStatus {
        id: id.into(),
        title: id.into(),
        description: None,
        command: command.into(),
        sha256: id.into(),
        done: false,
    }
}

/// A dialog on a workspace, whose step `deps` is being run.
fn running_step() -> (State, Model) {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::Toggle {
            ws: Some("w1".into()),
            kit: "hello-kit".into(),
            on: true,
        },
    );
    send(
        &mut st,
        &mut m,
        Msg::StepsRead("hello-kit".into(), Ok(vec![step("deps", "npm install")])),
    );
    send(&mut st, &mut m, Msg::Run("deps".into()));
    (st, m)
}

#[test]
fn run_marks_the_step_being_opened_and_clears_the_last_error() {
    let (st, _) = running_step();
    let act = st.activation.as_ref().expect("open");
    assert_eq!(act.running.as_deref(), Some("deps"));
    assert_eq!(act.error, None);
}

#[test]
fn run_does_nothing_for_a_workspace_that_no_longer_exists() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::Toggle {
            ws: Some("w1".into()),
            kit: "hello-kit".into(),
            on: true,
        },
    );
    m.hydrate(AppState::default());
    send(&mut st, &mut m, Msg::Run("deps".into()));
    assert_eq!(st.activation.as_ref().expect("open").running, None);
}

fn done(result: SetupRunResult) -> Msg {
    Msg::SetupDone {
        kit: "hello-kit".into(),
        step: "deps".into(),
        ws: "w1".into(),
        pane: "p9".into(),
        pane_name: "Bea".into(),
        result,
    }
}

#[test]
fn asks_whether_it_worked_once_the_command_was_typed() {
    let (mut st, mut m) = running_step();
    send(
        &mut st,
        &mut m,
        done(SetupRunResult::Ok { pane: "p9".into() }),
    );
    let act = st.activation.as_ref().expect("open");
    assert_eq!(act.running, None);
    assert_eq!(
        act.ran,
        Some(StepRun {
            step: "deps".into(),
            pane_name: "Bea".into()
        })
    );
}

#[test]
fn names_the_pane_with_an_ellipsis_when_it_could_not_be_found() {
    let (mut st, mut m) = running_step();
    send(
        &mut st,
        &mut m,
        Msg::SetupDone {
            kit: "hello-kit".into(),
            step: "deps".into(),
            ws: "w1".into(),
            pane: "p9".into(),
            pane_name: String::new(),
            result: SetupRunResult::Ok { pane: "p9".into() },
        },
    );
    assert_eq!(
        st.activation
            .as_ref()
            .expect("open")
            .ran
            .as_ref()
            .map(|r| r.pane_name.as_str()),
        Some("…")
    );
}

#[test]
fn says_why_a_command_was_not_typed() {
    let cases = [
        (
            SetupRunResult::NotStarted,
            "The pane did not start in time, so it was closed and nothing was typed. Open the workspace, then run it again.",
        ),
        (
            SetupRunResult::WriteFailed {
                detail: "pty not found".into(),
            },
            "Could not run it: pty not found",
        ),
        (
            SetupRunResult::NoPane,
            "The workspace has no room for another pane.",
        ),
    ];
    for (result, said) in cases {
        let (mut st, mut m) = running_step();
        send(&mut st, &mut m, done(result));
        let act = st.activation.as_ref().expect("open");
        assert_eq!(act.error.as_deref(), Some(said));
        assert_eq!(act.running, None);
        assert_eq!(act.ran, None);
    }
}

#[test]
fn says_a_workspace_in_a_window_of_its_own_must_be_opened_there() {
    let (mut st, mut m) = running_step();
    let mut state = m.state().clone();
    state.workspaces[0].detached = Some(true);
    m.hydrate(state);
    send(&mut st, &mut m, done(SetupRunResult::NotStarted));
    assert!(
        st.activation
            .as_ref()
            .expect("open")
            .error
            .as_deref()
            .unwrap()
            .contains("in a window of its own")
    );
}

#[test]
fn an_answer_for_a_dialog_that_was_closed_is_dropped() {
    let (mut st, mut m) = running_step();
    st.activation = None;
    send(
        &mut st,
        &mut m,
        done(SetupRunResult::Ok { pane: "p9".into() }),
    );
    assert!(st.activation.is_none());
}

#[test]
fn not_yet_forgets_the_question_and_marking_done_replaces_the_steps() {
    let (mut st, mut m) = running_step();
    send(
        &mut st,
        &mut m,
        done(SetupRunResult::Ok { pane: "p9".into() }),
    );
    send(&mut st, &mut m, Msg::NotYet);
    assert_eq!(st.activation.as_ref().expect("open").ran, None);

    send(
        &mut st,
        &mut m,
        done(SetupRunResult::Ok { pane: "p9".into() }),
    );
    let marked = StepStatus {
        done: true,
        ..step("deps", "npm install")
    };
    send(
        &mut st,
        &mut m,
        Msg::Marked("hello-kit".into(), Ok(vec![marked.clone()])),
    );
    let act = st.activation.as_ref().expect("open");
    assert_eq!(act.steps, Some(vec![marked]));
    assert_eq!(act.ran, None);

    send(
        &mut st,
        &mut m,
        Msg::Marked("hello-kit".into(), Err("disk full".into())),
    );
    assert_eq!(
        st.activation.as_ref().expect("open").error.as_deref(),
        Some("Error: disk full")
    );
}

// -- restarting -----------------------------------------------------------------------------

#[test]
fn a_restart_notice_can_be_dismissed() {
    let mut m = model_with(vec![]);
    let mut st = State {
        restart: Some(Restart {
            ws: "w1".into(),
            kit_name: "Hello kit".into(),
        }),
        ..State::default()
    };
    send(&mut st, &mut m, Msg::DismissRestart);
    assert!(st.restart.is_none());
}

#[test]
fn restarting_the_panes_says_so_and_forgets_the_notice() {
    let mut m = model_with(vec![]);
    let mut st = State {
        restart: Some(Restart {
            ws: "w1".into(),
            kit_name: "Hello kit".into(),
        }),
        ..State::default()
    };
    send(&mut st, &mut m, Msg::RestartPanes("w1".into()));
    assert!(st.restart.is_none());
    assert_eq!(st.restarted.as_deref(), Some("w1"));
    // Switching a kit afterwards forgets that it was.
    send(
        &mut st,
        &mut m,
        Msg::Toggle {
            ws: Some("w1".into()),
            kit: "hello-kit".into(),
            on: false,
        },
    );
    assert!(st.restarted.is_none());
}

// -- Settings › Kits ----------------------------------------------------------------------------

#[test]
fn typing_in_the_link_field_forgets_the_last_error() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    st.page.git_error = Some("not a GitHub link".into());
    send(
        &mut st,
        &mut m,
        Msg::GitUrl("https://github.com/o/r".into()),
    );
    assert_eq!(st.page.git_url, "https://github.com/o/r");
    assert_eq!(st.page.git_error, None);
}

#[test]
fn adds_from_a_link_only_when_there_is_one() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    send(&mut st, &mut m, Msg::AddGit);
    assert_eq!(st.page.adding, None);
    st.page.git_url = "  https://github.com/o/r  ".into();
    send(&mut st, &mut m, Msg::AddGit);
    assert_eq!(st.page.adding, Some(Adding::Git));
    // While one is being added, a second press adds nothing.
    st.page.git_error = Some("old".into());
    send(&mut st, &mut m, Msg::AddGit);
    assert_eq!(st.page.git_error.as_deref(), Some("old"));
}

#[test]
fn an_added_kit_is_said_and_the_field_emptied_a_failed_one_is_explained() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    st.page.git_url = "https://github.com/o/r".into();
    st.page.adding = Some(Adding::Git);
    send(
        &mut st,
        &mut m,
        Msg::GitAdded(Ok(Box::new(kit("1.0.0", vec![])))),
    );
    assert_eq!(st.page.adding, None);
    assert_eq!(st.page.git_url, "");
    assert_eq!(st.page.flash.as_deref(), Some("Hello kit added."));

    st.page.adding = Some(Adding::Git);
    send(
        &mut st,
        &mut m,
        Msg::GitAdded(Err("no hangar-kit.json".into())),
    );
    assert_eq!(st.page.git_error.as_deref(), Some("no hangar-kit.json"));

    st.page.adding = Some(Adding::Folder);
    send(
        &mut st,
        &mut m,
        Msg::FolderAdded(Err("already installed".into())),
    );
    assert_eq!(st.page.folder_error.as_deref(), Some("already installed"));
    assert_eq!(st.page.adding, None);
    send(&mut st, &mut m, Msg::FolderPicked(None));
    assert_eq!(st.page.adding, None, "a cancelled picker adds nothing");
}

#[test]
fn turns_the_answer_of_an_update_check_into_a_note() {
    use hangar_core::kits::UpdateInfo;
    let mut m = model_with(vec![]);
    let mut st = State::default();
    let found = |available: bool| {
        Ok(Some(UpdateInfo {
            current: "1.0.0".into(),
            latest: "1.1.0".into(),
            available,
            checked_at: 1,
        }))
    };
    send(&mut st, &mut m, Msg::Checked("a-kit".into(), found(true)));
    assert_eq!(
        st.page.notes["a-kit"],
        UpdateNote::Available {
            current: "1.0.0".into(),
            latest: "1.1.0".into()
        }
    );
    send(&mut st, &mut m, Msg::Checked("a-kit".into(), found(false)));
    assert_eq!(
        st.page.notes["a-kit"],
        UpdateNote::None {
            version: "1.0.0".into()
        }
    );
    send(&mut st, &mut m, Msg::Checked("a-kit".into(), Ok(None)));
    assert_eq!(st.page.notes["a-kit"], UpdateNote::Folder);
    send(
        &mut st,
        &mut m,
        Msg::Checked("a-kit".into(), Err("offline".into())),
    );
    assert_eq!(st.page.notes["a-kit"], UpdateNote::Error("offline".into()));
    send(&mut st, &mut m, Msg::Check("a-kit".into()));
    assert_eq!(st.page.notes["a-kit"], UpdateNote::Checking);
}

#[test]
fn says_what_an_update_did() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    st.page.gallery_busy = Some("hello-kit".into());
    let before = git_kit(Some(HOOKS), true);
    let after = KitInfo {
        installed_version: "1.1.0".into(),
        hooks_permission: Some(OTHER_HOOKS.into()),
        ..before.clone()
    };
    send(
        &mut st,
        &mut m,
        Msg::Updated {
            id: "hello-kit".into(),
            before: Box::new(before.clone()),
            result: Ok(Box::new(after)),
        },
    );
    assert_eq!(
        st.page.notes["hello-kit"],
        UpdateNote::Done {
            from: "1.0.0".into(),
            to: "1.1.0".into(),
            new_permissions: 1
        }
    );
    assert_eq!(
        st.page.gallery_busy, None,
        "the library's button is free again"
    );

    let linked = KitInfo {
        source: folder(),
        installed_version: "2.0.0".into(),
        ..before.clone()
    };
    send(
        &mut st,
        &mut m,
        Msg::Updated {
            id: "hello-kit".into(),
            before: Box::new(before.clone()),
            result: Ok(Box::new(linked)),
        },
    );
    assert_eq!(
        st.page.notes["hello-kit"],
        UpdateNote::Reloaded {
            version: "2.0.0".into()
        }
    );

    send(
        &mut st,
        &mut m,
        Msg::Updated {
            id: "hello-kit".into(),
            before: Box::new(before),
            result: Err("merge conflict".into()),
        },
    );
    assert_eq!(
        st.page.notes["hello-kit"],
        UpdateNote::Error("merge conflict".into())
    );
}

#[test]
fn removing_asks_twice_and_forgets_the_kit_s_note() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    send(&mut st, &mut m, Msg::AskRemove(Some("a-kit".into())));
    assert_eq!(st.page.removing.as_deref(), Some("a-kit"));
    send(&mut st, &mut m, Msg::AskRemove(None));
    assert_eq!(st.page.removing, None);

    st.page.notes.insert("a-kit".into(), UpdateNote::Folder);
    st.page.removing = Some("a-kit".into());
    send(&mut st, &mut m, Msg::Remove("a-kit".into()));
    assert_eq!(st.page.removing, None);
    send(&mut st, &mut m, Msg::Removed("a-kit".into(), Ok(())));
    assert!(!st.page.notes.contains_key("a-kit"));
    send(
        &mut st,
        &mut m,
        Msg::Removed("a-kit".into(), Err("busy".into())),
    );
    assert_eq!(st.page.notes["a-kit"], UpdateNote::Error("busy".into()));
}

#[test]
fn switching_everywhere_off_says_so_and_asks_for_a_restart() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State::default();
    send(
        &mut st,
        &mut m,
        Msg::ToggleEverywhere {
            kit: "hello-kit".into(),
            on: false,
        },
    );
    assert_eq!(
        st.page.flash.as_deref(),
        Some("Turned off for all workspaces. The workspaces that enabled it themselves keep it.")
    );
    assert_eq!(st.page.restart_for.as_deref(), Some("Hello kit"));
    send(&mut st, &mut m, Msg::DismissEverywhereRestart);
    assert_eq!(st.page.restart_for, None);
}

#[test]
fn the_library_keeps_its_answer_and_its_filter() {
    let mut m = model_with(vec![]);
    let mut st = State::default();
    st.page.gallery_refreshing = true;
    send(
        &mut st,
        &mut m,
        Msg::Gallery(Ok(crate::kits::gallery::Gallery {
            origin: crate::kits::gallery::Origin::Live,
            kits: vec![],
            fetched_at: Some(1),
        })),
    );
    assert!(!st.page.gallery_refreshing);
    assert!(st.page.gallery.is_some());
    send(&mut st, &mut m, Msg::Gallery(Err("no runtime".into())));
    assert_eq!(st.page.gallery_error.as_deref(), Some("no runtime"));
    assert!(st.page.gallery.is_some(), "the last answer stays");
    send(&mut st, &mut m, Msg::GalleryQuery("ci".into()));
    assert_eq!(st.page.gallery_query, "ci");
    send(&mut st, &mut m, Msg::GalleryRefresh);
    assert!(st.page.gallery_refreshing);
}

#[test]
fn installing_from_the_library_marks_the_kit_busy_and_reports_a_failure_by_name() {
    use crate::kits::gallery::{Gallery, GalleryKit, Origin};
    let mut m = model_with(vec![]);
    let mut st = State::default();
    st.page.gallery = Some(Gallery {
        origin: Origin::Live,
        kits: vec![GalleryKit {
            id: "ci-status".into(),
            name: "CI status".into(),
            description: String::new(),
            repo: "hangar-ai/ci-status".into(),
            tag: "v0.2.0".into(),
            version: "0.2.0".into(),
            permissions_summary: String::new(),
            tags: vec![],
            verified: true,
        }],
        fetched_at: Some(1),
    });
    // A kit the library does not list is not installed.
    send(&mut st, &mut m, Msg::GalleryInstall("ghost".into()));
    assert_eq!(st.page.gallery_busy, None);

    send(&mut st, &mut m, Msg::GalleryInstall("ci-status".into()));
    assert_eq!(st.page.gallery_busy.as_deref(), Some("ci-status"));
    send(
        &mut st,
        &mut m,
        Msg::GalleryInstalled {
            id: "ci-status".into(),
            name: "CI status".into(),
            result: Err("not a kit".into()),
        },
    );
    assert_eq!(st.page.gallery_busy, None);
    assert_eq!(
        st.page.gallery_failure,
        Some(GalleryFailure {
            id: "ci-status".into(),
            name: "CI status".into(),
            detail: "not a kit".into()
        })
    );
    send(
        &mut st,
        &mut m,
        Msg::GalleryInstalled {
            id: "ci-status".into(),
            name: "CI status".into(),
            result: Ok(Box::new(kit("1.0.0", vec![]))),
        },
    );
    assert!(
        st.page
            .flash
            .as_deref()
            .unwrap()
            .contains("Hello kit installed.")
    );
}

#[test]
fn updating_from_the_library_marks_the_kit_busy_until_the_update_ends() {
    let one = git_kit(Some(HOOKS), true);
    let mut m = model_with(vec![one]);
    let mut st = State::default();
    st.page.gallery_failure = Some(GalleryFailure {
        id: "x".into(),
        name: "x".into(),
        detail: "x".into(),
    });
    send(&mut st, &mut m, Msg::GalleryUpdate("hello-kit".into()));
    assert_eq!(st.page.gallery_busy.as_deref(), Some("hello-kit"));
    assert_eq!(st.page.gallery_failure, None);
    assert_eq!(st.page.notes["hello-kit"], UpdateNote::Updating);
    // For a kit that is gone there is nothing to update, and nothing stays busy.
    send(&mut st, &mut m, Msg::GalleryUpdate("ghost".into()));
    assert_eq!(st.page.gallery_busy, None);
}
