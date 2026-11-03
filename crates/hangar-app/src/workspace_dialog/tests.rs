//! The dialog, drawn and driven headless (`testing::Harness`), and its update
//! logic that needs no service.

use hangar_i18n::Locale;

use crate::git_flow::clone::RepoRef;
use crate::git_flow::flow::{FromGitRefusal, PaneRef, WorkspaceRef};
use crate::message::Message;

use super::testing::{Harness, messages};
use super::*;

fn parsed_ok(input: &str, name: &str) -> Parsed {
    Parsed {
        input: input.into(),
        result: ParseResult::Ok {
            repo: RepoRef {
                url: format!("https://github.com/acme/{name}.git"),
                host: "github.com".into(),
                owner: "acme".into(),
                name: name.into(),
            },
        },
    }
}

fn refused(input: &str) -> Parsed {
    Parsed {
        input: input.into(),
        result: ParseResult::Refused {
            reason: format!("\"{input}\" is not a repository address"),
        },
    }
}

fn sent(messages: &[Message], wanted: impl Fn(&Msg) -> bool) -> bool {
    messages.iter().any(|message| match message {
        Message::WorkspaceDialog(msg) => wanted(msg),
        _ => false,
    })
}

fn closed(messages: &[Message]) -> bool {
    messages
        .iter()
        .any(|message| matches!(message, Message::Intent(Intent::CloseOverlay)))
}

// ── the folder form, drawn ────────────────────────────────────────────

#[test]
fn the_folder_form_has_every_field_of_the_web_dialog() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    for text in [
        "New workspace",
        "Source",
        "Existing folder",
        "From a Git repository",
        "Project folder",
        "Browse…",
        ".code-workspace…",
        "Name",
        "Shell",
        "2 detected on this machine",
        "Number of panes",
        "Agent split",
        "Claude Code",
        "Codex",
        "Gemini",
        "OpenCode",
        "Arrangement",
        "Claude account",
        "Worktree hooks",
        "Cancel",
        "Create workspace",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is on the form");
    }
    h.shot(&mut ui, "dialog-folder-en-graphite");
}

#[test]
fn an_agent_that_is_not_installed_says_so() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    // Gemini and OpenCode are missing on the harness' machine.
    assert!(ui.find("not installed").is_ok());
}

#[test]
fn the_form_speaks_french_in_a_light_theme() {
    let h = Harness::new(Locale::Fr, "paper");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    for text in [
        "Nouveau workspace",
        "Dossier du projet",
        "Répartition des agents",
        "Créer le workspace",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is on the form");
    }
    h.shot(&mut ui, "dialog-folder-fr-paper");
}

#[test]
fn the_create_button_waits_for_a_folder() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    ui.click("Create workspace").expect("the button is drawn");
    assert!(!sent(&messages(ui), |msg| matches!(msg, Msg::Submit)));

    let mut st = State::default();
    st.folder.cwd = "/home/me/dev/shop".into();
    let mut ui = h.show(view(&st, h.ctx()));
    ui.click("Create workspace").expect("the button is drawn");
    assert!(sent(&messages(ui), |msg| matches!(msg, Msg::Submit)));
}

#[test]
fn the_source_control_switches_to_the_git_form() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    ui.click("From a Git repository").expect("the segment is drawn");
    assert!(sent(&messages(ui), |msg| matches!(msg, Msg::Source(Source::Git))));
}

#[test]
fn the_layout_preview_names_the_panes_with_the_next_free_names() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    // Ava, Max and Leo are taken by the harness' workspaces.
    for name in ["Mia", "Sam", "Zoe", "Eli"] {
        assert!(ui.find(name).is_ok(), "{name}");
    }
    assert!(ui.find("Ava").is_err());
}

#[test]
fn a_cell_of_the_preview_asks_to_change_its_agent() {
    let h = Harness::new(Locale::En, "graphite");
    let st = State::default();
    let mut ui = h.show(view(&st, h.ctx()));
    ui.click("Mia").expect("the cell is drawn");
    assert!(sent(&messages(ui), |msg| matches!(msg, Msg::Cycle(0))));
}

#[test]
fn a_workspace_file_shows_its_roots_and_the_missing_one_cannot_be_picked() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    st.folder.cwd = "/work/api".into();
    st.folder.roots = vec![
        Root {
            name: "api".into(),
            path: "/work/api".into(),
            exists: true,
        },
        Root {
            name: "web".into(),
            path: "/work/web".into(),
            exists: true,
        },
        Root {
            name: "docs".into(),
            path: "/work/docs".into(),
            exists: false,
        },
    ];
    let mut ui = h.show(view(&st, h.ctx()));
    for text in ["main", "extra", "missing"] {
        assert!(ui.find(text).is_ok(), "{text}");
    }
    h.shot(&mut ui, "dialog-folder-roots-en-graphite");
    ui.click("web").expect("the root is drawn");
    assert!(sent(&messages(ui), |msg| matches!(
        msg,
        Msg::PickRoot(path) if path == "/work/web"
    )));

    let mut ui = h.show(view(&st, h.ctx()));
    ui.click("docs").expect("the root is drawn");
    assert!(!sent(&messages(ui), |msg| matches!(msg, Msg::PickRoot(_))));
}

#[test]
fn a_workspace_file_found_beside_the_folder_is_offered_not_applied() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = State::default();
    st.folder.cwd = "/work".into();
    st.folder.suggestion = Some("/work/app.code-workspace".into());
    let mut ui = h.show(view(&st, h.ctx()));
    assert!(ui.find("app.code-workspace sits in this folder").is_ok());
    ui.click("Open it").expect("the button is drawn");
    assert!(sent(&messages(ui), |msg| matches!(msg, Msg::UseSuggestion)));
}

// ── the Git form, drawn ───────────────────────────────────────────────

fn git_state(repo: &str, parsed: Option<Parsed>) -> State {
    let mut st = State::default();
    st.source = Source::Git;
    st.git.repo = repo.into();
    st.git.parsed = parsed;
    st
}

#[test]
fn the_git_form_shows_where_the_clone_will_land_and_starts_when_ready() {
    let h = Harness::new(Locale::En, "graphite");
    // Beside the workspace in front: /home/me/dev/hangar → /home/me/dev.
    let st = git_state("acme/shop", Some(parsed_ok("acme/shop", "shop")));
    let mut ui = h.show(view(&st, h.ctx()));
    for text in [
        "Repository",
        "Clone into",
        "It will be cloned to /home/me/dev/shop",
        "Clone and create workspace",
    ] {
        assert!(ui.find(text).is_ok(), "{text} is on the form");
    }
    h.shot(&mut ui, "dialog-git-ready-en-graphite");
    ui.click("Clone and create workspace")
        .expect("the button is drawn");
    assert!(sent(&messages(ui), |msg| matches!(msg, Msg::Submit)));
}

#[test]
fn the_git_form_is_not_startable_until_the_address_is_read_and_good() {
    let h = Harness::new(Locale::En, "graphite");
    for (repo, parsed) in [
        ("", None),
        ("acme/shop", None),
        ("nonsense", Some(refused("nonsense"))),
    ] {
        let st = git_state(repo, parsed);
        let mut ui = h.show(view(&st, h.ctx()));
        ui.click("Clone and create workspace")
            .expect("the button is drawn");
        assert!(!sent(&messages(ui), |msg| matches!(msg, Msg::Submit)), "{repo:?}");
    }
}

#[test]
fn a_refused_address_says_why_under_the_field() {
    let h = Harness::new(Locale::En, "graphite");
    let st = git_state("nonsense", Some(refused("nonsense")));
    let mut ui = h.show(view(&st, h.ctx()));
    assert!(ui.find("\"nonsense\" is not a repository address").is_ok());
    h.shot(&mut ui, "dialog-git-refused-en-graphite");
}

#[test]
fn an_address_read_for_an_older_text_is_not_believed() {
    let h = Harness::new(Locale::En, "graphite");
    // The field says one thing, the reading is of another: still checking.
    let st = git_state("acme/shop2", Some(parsed_ok("acme/shop", "shop")));
    let mut ui = h.show(view(&st, h.ctx()));
    assert!(ui.find("Reading the address…").is_ok());
}

#[test]
fn a_running_clone_shows_its_progress_and_offers_to_stop_it() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = git_state("acme/shop", Some(parsed_ok("acme/shop", "shop")));
    st.git.running = true;
    st.git.job = Some("job-1".into());
    st.git.progress = Some(CloneProgress {
        job_id: "job-1".into(),
        phase: "Receiving objects".into(),
        percent: Some(45),
    });
    let mut ui = h.show(view(&st, h.ctx()));
    assert!(ui.find("Receiving objects 45 %").is_ok());
    assert!(ui.find("Cloning…").is_ok());
    h.shot(&mut ui, "dialog-git-running-en-graphite");
    ui.click("Cancel the clone").expect("the stop button is drawn");
    assert!(sent(&messages(ui), |msg| matches!(msg, Msg::Stop)));
}

#[test]
fn a_clone_that_has_not_started_says_so() {
    let h = Harness::new(Locale::En, "graphite");
    let mut st = git_state("acme/shop", Some(parsed_ok("acme/shop", "shop")));
    st.git.running = true;
    let mut ui = h.show(view(&st, h.ctx()));
    assert!(ui.find("Starting the clone…").is_ok());
}

#[test]
fn a_failed_clone_shows_the_failure_in_the_form() {
    let h = Harness::new(Locale::Fr, "graphite");
    let mut st = git_state("acme/shop", Some(parsed_ok("acme/shop", "shop")));
    st.git.error = Some("Impossible de créer le workspace : dépôt introuvable".into());
    let mut ui = h.show(view(&st, h.ctx()));
    assert!(
        ui.find("Impossible de créer le workspace : dépôt introuvable")
            .is_ok()
    );
    h.shot(&mut ui, "dialog-git-failed-fr-graphite");
}

// ── the kits dialog ───────────────────────────────────────────────────

#[test]
fn the_kits_dialog_names_the_workspace_and_closes_with_done() {
    let h = Harness::new(Locale::En, "graphite");
    let ws = "w1".to_string();
    let mut st = State::default();
    st.kits = kits_dialog::KitsDialog::open(&h.model, &ws);
    let mut ui = h.show(kits_dialog_view(&st, h.ctx(), &ws));
    assert!(ui.find("Done").is_ok());
    h.shot(&mut ui, "dialog-kits-en-graphite");
    ui.click("Done").expect("the button is drawn");
    assert!(closed(&messages(ui)));
}

// ── the update logic that needs no service ────────────────────────────

fn on_top() -> Model {
    let mut model = Model::default();
    model.ui.overlays.push(Overlay::WorkspaceDialog);
    model
}

#[test]
fn nothing_of_the_form_changes_while_a_clone_runs() {
    let mut m = on_top();
    let mut st = git_state("acme/shop", None);
    st.git.running = true;
    st.name = "shop".into();
    st.git.parent = "/x".into();
    for msg in [
        Msg::Source(Source::Folder),
        Msg::Name("other".into()),
        Msg::Repo("other/repo".into()),
        Msg::Parent("/y".into()),
        Msg::Dismiss,
    ] {
        let task = form(&mut st, &mut m, msg);
        assert_eq!(task.units(), 0);
    }
    assert_eq!(st.source, Source::Git);
    assert_eq!(st.name, "shop");
    assert_eq!(st.git.repo, "acme/shop");
    assert_eq!(st.git.parent, "/x");
}

#[test]
fn cancel_and_dismiss_close_the_dialog_when_it_is_in_front() {
    let mut m = on_top();
    let mut st = State::default();
    assert_eq!(form(&mut st, &mut m, Msg::Cancel).units(), 1);
    assert_eq!(form(&mut st, &mut m, Msg::Dismiss).units(), 1);
    // Something else is in front: closing would close that.
    m.ui.overlays.push(Overlay::Palette);
    assert_eq!(form(&mut st, &mut m, Msg::Cancel).units(), 0);
}

#[test]
fn typing_an_address_asks_for_it_to_be_read_after_a_pause_and_only_the_last_counts() {
    let mut m = on_top();
    let mut st = State::default();
    st.source = Source::Git;
    assert_eq!(form(&mut st, &mut m, Msg::Repo("a".into())).units(), 1);
    assert_eq!(st.git.generation, 1);
    assert_eq!(form(&mut st, &mut m, Msg::Repo("ac".into())).units(), 1);
    assert_eq!(st.git.generation, 2);
    // The pause that ended for the first edit is not believed.
    assert_eq!(form(&mut st, &mut m, Msg::ParseDue(1)).units(), 0);
    assert_eq!(form(&mut st, &mut m, Msg::ParseDue(2)).units(), 1);
    // Cleared: nothing to read.
    assert_eq!(form(&mut st, &mut m, Msg::Repo("  ".into())).units(), 0);
    assert_eq!(form(&mut st, &mut m, Msg::ParseDue(3)).units(), 0);
}

#[test]
fn a_reading_that_arrives_late_is_ignored() {
    let mut m = on_top();
    let mut st = State::default();
    st.source = Source::Git;
    st.git.repo = "acme/shop".into();
    st.git.generation = 4;
    let late = Msg::Parsed {
        generation: 3,
        input: "acme/sho".into(),
        result: ParseResult::Refused { reason: "x".into() },
    };
    form(&mut st, &mut m, late);
    assert!(st.git.parsed.is_none());
    let on_time = Msg::Parsed {
        generation: 4,
        input: "acme/shop".into(),
        result: parsed_ok("acme/shop", "shop").result,
    };
    form(&mut st, &mut m, on_time);
    assert!(matches!(st.git.parse(), Some(ParseResult::Ok { .. })));
}

#[test]
fn a_new_layout_cuts_or_pads_the_agents() {
    let mut m = on_top();
    let mut st = State::default();
    form(&mut st, &mut m, Msg::More(AgentId::Claude));
    form(&mut st, &mut m, Msg::Layout(2));
    assert_eq!(st.folder.assignments, [AgentId::Claude, AgentId::Shell]);
    form(&mut st, &mut m, Msg::Layout(8));
    assert_eq!(st.folder.assignments.len(), 8);
    // Not one of the sizes offered: refused.
    form(&mut st, &mut m, Msg::Layout(3));
    assert_eq!(st.folder.layout, 8);
    form(&mut st, &mut m, Msg::Cycle(1));
    assert_eq!(st.folder.assignments[1], AgentId::Claude);
    form(&mut st, &mut m, Msg::Less(AgentId::Claude));
    assert_eq!(st.folder.assignments[0], AgentId::Claude);
    assert_eq!(st.folder.assignments[1], AgentId::Shell);
}

#[test]
fn picking_a_folder_fills_the_name_only_while_it_is_empty() {
    let mut m = on_top();
    let mut st = State::default();
    form(&mut st, &mut m, Msg::Picked(Some("/home/me/dev/shop".into())));
    assert_eq!(st.folder.cwd, "/home/me/dev/shop");
    assert_eq!(st.name, "shop");
    form(&mut st, &mut m, Msg::Name("My shop".into()));
    form(&mut st, &mut m, Msg::Picked(Some("/home/me/dev/other".into())));
    assert_eq!(st.name, "My shop");
    // A cancelled dialog changes nothing.
    form(&mut st, &mut m, Msg::Picked(None));
    assert_eq!(st.folder.cwd, "/home/me/dev/other");
}

#[test]
fn a_workspace_file_makes_its_first_existing_folder_the_main_one() {
    let mut m = on_top();
    let mut st = State::default();
    st.folder.suggestion = Some("/work/My app.code-workspace".into());
    let root = |name: &str, path: &str, exists: bool| Root {
        name: name.into(),
        path: path.into(),
        exists,
    };
    form(
        &mut st,
        &mut m,
        Msg::FileRead {
            path: "/work/My app.code-workspace".into(),
            result: Ok(vec![
                root("gone", "/work/gone", false),
                root("api", "/work/api", true),
                root("web", "/work/web", true),
            ]),
        },
    );
    assert_eq!(st.folder.cwd, "/work/api");
    assert_eq!(st.name, "My app");
    assert_eq!(st.folder.roots.len(), 3);
    assert!(st.folder.suggestion.is_none());
    // Another root is picked by a click; a missing one is not.
    form(&mut st, &mut m, Msg::PickRoot("/work/web".into()));
    assert_eq!(st.folder.cwd, "/work/web");
    form(&mut st, &mut m, Msg::PickRoot("/work/gone".into()));
    assert_eq!(st.folder.cwd, "/work/web");
}

#[test]
fn a_workspace_file_that_cannot_be_read_says_so_and_keeps_the_folder() {
    let mut m = on_top();
    let mut st = State::default();
    st.folder.cwd = "/work".into();
    form(
        &mut st,
        &mut m,
        Msg::FileRead {
            path: "/work/broken.code-workspace".into(),
            result: Err("no \"folders\" list".into()),
        },
    );
    assert_eq!(st.folder.file_error.as_deref(), Some("/work/broken.code-workspace"));
    assert_eq!(st.folder.cwd, "/work");
    assert!(st.folder.roots.is_empty());
}

fn created(reused: bool) -> FromGitOutcome {
    FromGitOutcome::Created {
        workspace: WorkspaceRef {
            id: "w".into(),
            name: "shop".into(),
            cwd: "/home/me/dev/shop".into(),
            orchestrator: Some(PaneRef {
                id: "p".into(),
                name: "Kai".into(),
            }),
        },
        repo: RepoRef {
            url: "https://github.com/acme/shop.git".into(),
            host: "github.com".into(),
            owner: "acme".into(),
            name: "shop".into(),
        },
        mine: true,
        trusted: true,
        consented: false,
        reused_folder: reused,
        notes: vec![],
    }
}

fn running(job: &str) -> State {
    let mut st = git_state("acme/shop", None);
    st.git.running = true;
    st.git.job = Some(job.into());
    st
}

#[test]
fn a_flow_that_ended_for_another_job_is_ignored() {
    let mut m = on_top();
    let mut st = running("job-2");
    let task = form(
        &mut st,
        &mut m,
        Msg::GitDone {
            job: "job-1".into(),
            outcome: created(false),
        },
    );
    assert_eq!(task.units(), 0);
    assert!(st.git.running);
}

#[test]
fn a_created_workspace_closes_the_dialog_and_a_reused_folder_is_told() {
    let mut m = on_top();
    let mut st = running("job-1");
    let task = form(
        &mut st,
        &mut m,
        Msg::GitDone {
            job: "job-1".into(),
            outcome: created(false),
        },
    );
    // Only the closing: a workspace that was cloned needs no word.
    assert_eq!(task.units(), 1);
    assert!(!st.git.running && st.git.job.is_none());

    let mut st = running("job-1");
    let task = form(
        &mut st,
        &mut m,
        Msg::GitDone {
            job: "job-1".into(),
            outcome: created(true),
        },
    );
    // The notice and the closing.
    assert_eq!(task.units(), 2);
}

#[test]
fn a_refused_flow_stays_in_the_form_with_its_failure_on_one_line() {
    let mut m = on_top();
    let mut st = running("job-1");
    form(
        &mut st,
        &mut m,
        Msg::GitDone {
            job: "job-1".into(),
            outcome: FromGitOutcome::Refused {
                kind: FromGitRefusal::CloneFailed,
                reason: "The clone failed: repository not found\nsecond line".into(),
            },
        },
    );
    assert!(!st.git.running);
    assert_eq!(
        st.git.error.as_deref(),
        Some("Could not create the workspace: The clone failed: repository not found")
    );
}

#[test]
fn a_cancelled_clone_says_cancelled_rather_than_failed() {
    let mut m = on_top();
    let mut st = running("job-1");
    st.git.cancelled = true;
    form(
        &mut st,
        &mut m,
        Msg::GitDone {
            job: "job-1".into(),
            outcome: FromGitOutcome::Refused {
                kind: FromGitRefusal::CloneFailed,
                reason: "The clone was refused (cancelled): stopped".into(),
            },
        },
    );
    assert_eq!(st.git.error.as_deref(), Some("Clone cancelled"));
}

#[test]
fn a_failure_after_the_dialog_was_closed_is_told_where_the_person_is() {
    // Escape closed the dialog while the clone ran: the failure becomes a
    // notice rather than a message in a form nobody sees.
    let mut m = Model::default();
    let mut st = running("job-1");
    let task = form(
        &mut st,
        &mut m,
        Msg::GitDone {
            job: "job-1".into(),
            outcome: FromGitOutcome::Refused {
                kind: FromGitRefusal::CloneFailed,
                reason: "nope".into(),
            },
        },
    );
    assert_eq!(task.units(), 1);
    assert!(st.git.error.is_none());
}

#[test]
fn the_progress_of_the_clone_followed_is_kept_and_another_clones_is_not() {
    let mut m = on_top();
    let mut st = running("job-1");
    let progress = |job: &str| CloneProgress {
        job_id: job.into(),
        phase: "Receiving objects".into(),
        percent: Some(10),
    };
    assert!(!follow(&mut st, &progress("job-2")));
    assert!(st.git.progress.is_none());
    assert!(follow(&mut st, &progress("job-1")));
    assert_eq!(st.git.progress.as_ref().and_then(|p| p.percent), Some(10));
    // Nothing runs, nothing is followed.
    st.git.job = None;
    assert!(!follow(&mut st, &progress("job-1")));
    let _ = &mut m;
}

#[test]
fn opening_the_dialog_starts_a_clean_form_unless_a_clone_is_still_running() {
    let mut m = on_top();
    m.live.workspace_dialog.draft_kits = vec![WorkspaceKit {
        id: "kit".into(),
        granted: vec![],
    }];
    let mut st = git_state("acme/shop", None);
    st.name = "left over".into();
    opened(&mut st, &mut m);
    assert!(st.name.is_empty() && st.git.repo.is_empty());
    assert_eq!(st.source, Source::Folder);
    assert!(m.live.workspace_dialog.draft_kits.is_empty());

    // The dialog was closed with a clone running: it comes back as it was.
    let mut st = running("job-1");
    st.name = "shop".into();
    opened(&mut st, &mut m);
    assert!(st.git.running);
    assert_eq!(st.name, "shop");
    assert_eq!(st.git.repo, "acme/shop");
}

#[test]
fn the_kits_dialog_opens_on_the_workspace_it_is_asked_for() {
    let h = Harness::new(Locale::En, "graphite");
    let mut m = h.model;
    let mut st = State::default();
    // `on_intent` hands the intent to this; the services are not reached.
    st.kits = kits_dialog::KitsDialog::open(&m, &"w2".to_string());
    assert_eq!(st.kits.as_ref().map(|kits| kits.ws.as_str()), Some("w2"));
    assert!(kits_dialog::KitsDialog::open(&m, &"gone".to_string()).is_none());
    let _ = &mut m;
}
