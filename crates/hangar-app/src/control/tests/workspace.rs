//! control.test.ts: "workspace_create" (what the window decides before and
//! after P8's flow) and "workspace_set_check".

use std::sync::{Arc, Mutex};

use serde_json::json;

use super::super::fake::{Fake, lock, pane};
use super::super::port::{BoxFuture, FromGit, FromGitOutcome, MadeWorkspace};
use super::super::run::DISABLED;
use super::super::types::Caller;
use super::super::workspace::set_check;
use super::{contains, control, from_ava, matches, refusal, with};
use crate::message::{GitOrigin, GitRequest};
use crate::state::types::{OrchestrationSettings, PaneRole};
use crate::state::{AgentActivity, AgentId, PaneActivity};

/// P8's flow, as the person and the clone would answer it: the requests it
/// was handed, and the outcome it gives.
struct FakeGit {
    seen: Mutex<Vec<GitRequest>>,
    outcome: FromGitOutcome,
}

impl FromGit for FakeGit {
    fn run(&self, request: GitRequest) -> BoxFuture<FromGitOutcome> {
        lock(&self.seen).push(request);
        Box::pin(std::future::ready(self.outcome.clone()))
    }
}

fn made() -> MadeWorkspace {
    MadeWorkspace {
        id: "w50".into(),
        name: "shop".into(),
        cwd: "C:\\dev\\shop".into(),
        orchestrator: Some(("p50".into(), "Kai".into())),
    }
}

fn created(mine: bool, trusted: bool) -> FromGitOutcome {
    FromGitOutcome::Created {
        workspace: made(),
        repo_url: "https://github.com/acme/shop.git".into(),
        mine,
        reused_folder: false,
        trusted,
        notes: vec!["Cloned next to Hangar.".into()],
    }
}

fn with_git(outcome: FromGitOutcome) -> (Arc<Fake>, Arc<FakeGit>) {
    let fake = Fake::setup();
    let git = Arc::new(FakeGit {
        seen: Mutex::new(Vec::new()),
        outcome,
    });
    *lock(&fake.from_git) = Some(git.clone());
    (fake, git)
}

#[tokio::test]
async fn clones_next_to_the_asking_workspace_as_an_agents_request_and_says_what_it_made() {
    let (fake, git) = with_git(created(true, true));
    let answer = control(
        "workspace_create",
        with(from_ava(), json!({"repo": " acme/shop "})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &answer,
        json!({
            "created": true,
            "workspace": {"name": "shop", "folder": "C:\\dev\\shop", "orchestrator": "Kai"},
            "yours": true,
            "cloned": true,
            "folder": "C:\\dev\\shop",
            "repository": "https://github.com/acme/shop.git",
        }),
    );
    contains(
        answer["firstMessage"].as_str().unwrap(),
        "queued for the orchestrator",
    );
    // "Hangar" lives in C:\dev\hangar: the clone goes beside it, as the
    // agent's request, named by its pane.
    assert_eq!(
        *lock(&git.seen),
        vec![GitRequest {
            repo: "acme/shop".into(),
            name: None,
            parent: None,
            origin: GitOrigin::Agent {
                pane: Some("Ava".into())
            },
            asker_cwd: Some("C:\\dev\\hangar".into()),
        }]
    );
    assert!(
        fake.log()
            .contains(&"announce:workspaceCreated:Kai:Ava".into())
    );
}

/// The messages the watch on the new orchestrator left for Ava.
fn told_ava(fake: &Fake) -> Vec<String> {
    fake.logged("enqueue:p1:The orchestrator")
}

#[tokio::test]
async fn tells_the_asking_agent_when_the_pane_waits_for_the_person_on_the_trust_question() {
    let (fake, _) = with_git(created(false, false));
    lock(&fake.activity).insert(
        "p50".into(),
        PaneActivity {
            workspace_id: "w50".into(),
            activity: AgentActivity::Waiting,
            bridged: false,
        },
    );
    control(
        "workspace_create",
        with(from_ava(), json!({"repo": "evil/tool"})),
        &fake,
    )
    .await
    .unwrap();
    fake.settle().await;
    let told = told_ava(&fake);
    assert_eq!(told.len(), 1);
    contains(&told[0], "waiting for the person");
    contains(&told[0], "trust the cloned folder");
    contains(&told[0], "Only they can answer");
    assert!(lock(&fake.senders)[0].ends_with("<- hangar"));
}

#[tokio::test]
async fn says_nothing_when_the_session_is_up_and_reported_by_the_mod_or_the_pane_never_waits() {
    for (activity, bridged) in [
        (AgentActivity::Waiting, true),
        (AgentActivity::Working, false),
    ] {
        let (fake, _) = with_git(created(false, false));
        lock(&fake.activity).insert(
            "p50".into(),
            PaneActivity {
                workspace_id: "w50".into(),
                activity,
                bridged,
            },
        );
        control(
            "workspace_create",
            with(from_ava(), json!({"repo": "evil/tool"})),
            &fake,
        )
        .await
        .unwrap();
        fake.settle().await;
        assert!(told_ava(&fake).is_empty());
    }
}

#[tokio::test]
async fn does_not_watch_a_folder_claude_code_was_told_to_trust() {
    let (fake, _) = with_git(created(true, true));
    lock(&fake.activity).insert(
        "p50".into(),
        PaneActivity {
            workspace_id: "w50".into(),
            activity: AgentActivity::Waiting,
            bridged: false,
        },
    );
    control(
        "workspace_create",
        with(from_ava(), json!({"repo": "me/tool"})),
        &fake,
    )
    .await
    .unwrap();
    fake.settle().await;
    assert!(told_ava(&fake).is_empty());
}

#[tokio::test]
async fn shows_a_workspace_already_on_the_folder_and_clones_nothing() {
    let (fake, _) = with_git(FromGitOutcome::Existing {
        workspace: MadeWorkspace {
            id: "w9".into(),
            name: "Octo".into(),
            cwd: "C:\\dev\\octo".into(),
            orchestrator: None,
        },
        notes: vec!["Octo is already a workspace.".into()],
    });
    let answer = control(
        "workspace_create",
        with(from_ava(), json!({"repo": "me/octo"})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &answer,
        json!({"created": false, "workspace": {"name": "Octo", "id": "w9", "orchestrator": null},
               "note": "Octo is already a workspace."}),
    );
    assert!(fake.logged("announce:").is_empty());
}

#[tokio::test]
async fn hands_the_flow_where_the_person_said_the_clone_goes() {
    let (fake, git) = with_git(created(true, true));
    control(
        "workspace_create",
        with(
            from_ava(),
            json!({"repo": "boykopovar/AnyPS5", "parent": "E:\\", "name": " Any "}),
        ),
        &fake,
    )
    .await
    .unwrap();
    let seen = lock(&git.seen);
    assert_eq!(seen[0].parent.as_deref(), Some("E:\\"));
    assert_eq!(seen[0].name.as_deref(), Some("Any"));
}

#[tokio::test]
async fn tells_a_model_why_not_in_words_and_refuses_what_is_not_a_repository() {
    let (fake, _) = with_git(FromGitOutcome::Refused {
        reason: "not a repository address".into(),
    });
    assert_eq!(
        refusal(
            "workspace_create",
            with(from_ava(), json!({"repo": "x y"})),
            &fake
        )
        .await,
        "not a repository address"
    );
    contains(
        &refusal("workspace_create", from_ava(), &fake).await,
        "Give repo",
    );
    contains(
        &refusal(
            "workspace_create",
            with(from_ava(), json!({"repo": 3})),
            &fake,
        )
        .await,
        "must be a string",
    );
    contains(
        &refusal(
            "workspace_create",
            with(from_ava(), json!({"repo": "a/b", "name": 4})),
            &fake,
        )
        .await,
        "name must be a string",
    );
    contains(
        &refusal(
            "workspace_create",
            with(from_ava(), json!({"repo": "a/b", "parent": 3})),
            &fake,
        )
        .await,
        "parent must be a string",
    );
}

#[tokio::test]
async fn is_turned_off_with_the_rest_of_the_pane_control() {
    let (fake, _) = with_git(created(true, true));
    lock(&fake.state).settings.agent_pane_control = false;
    assert_eq!(
        refusal(
            "workspace_create",
            with(from_ava(), json!({"repo": "a/b"})),
            &fake
        )
        .await,
        DISABLED
    );
}

#[tokio::test]
async fn needs_a_window_that_can_clone() {
    let fake = Fake::setup();
    contains(
        &refusal(
            "workspace_create",
            with(from_ava(), json!({"repo": "a/b"})),
            &fake,
        )
        .await,
        "cannot create a workspace",
    );
}

/* ── workspace_set_check ─────────────────────────────────────── */

fn with_orchestrator() -> Arc<Fake> {
    let fake = Fake::setup();
    fake.patch("w1", |ws| {
        let mut ava = pane("p1", "Ava", AgentId::Claude, None);
        ava.role = Some(PaneRole::Orchestrator);
        ws.panes = vec![ava, pane("p2", "Max", AgentId::Shell, None)];
    });
    fake
}

#[tokio::test]
async fn sets_the_check_command_of_the_orchestrators_own_workspace_and_says_so_on_screen() {
    let fake = with_orchestrator();
    let answer = control(
        "workspace_set_check",
        with(from_ava(), json!({"command": "  pnpm build && pnpm test "})),
        &fake,
    )
    .await
    .unwrap();
    matches(
        &answer,
        json!({"workspace": "Hangar", "checkCommand": "pnpm build && pnpm test", "previous": null}),
    );
    let patches = fake.logged("patch:");
    assert_eq!(patches.len(), 1);
    let saved: serde_json::Value = serde_json::from_str(&patches[0]["patch:w1:".len()..]).unwrap();
    matches(
        &saved,
        json!({"checkCommand": "pnpm build && pnpm test", "maxWorkers": 3}),
    );
    assert!(
        fake.log()
            .contains(&"announce:checkSet:pnpm build && pnpm test:Ava".into())
    );
}

#[tokio::test]
async fn is_the_orchestrators_alone_and_for_its_own_workspace_only() {
    let fake = with_orchestrator();
    // A shell pane of the same workspace, a pane of another workspace, a
    // caller Hangar does not know.
    for caller in [
        json!({"paneId": "p2", "paneName": "Max", "cwd": "C:\\dev\\hangar"}),
        json!({"paneId": "p3", "paneName": "Ava", "cwd": "C:\\dev\\shop"}),
        json!({"paneId": null, "paneName": null, "cwd": "C:\\elsewhere"}),
    ] {
        contains(
            &refusal(
                "workspace_set_check",
                json!({"caller": caller, "command": "pnpm test"}),
                &fake,
            )
            .await,
            "for a workspace's orchestrator",
        );
    }
    // Naming another workspace changes nothing about whose it is.
    control(
        "workspace_set_check",
        with(
            from_ava(),
            json!({"workspace": "Shop", "command": "pnpm test"}),
        ),
        &fake,
    )
    .await
    .unwrap();
    let patched: Vec<String> = fake
        .logged("patch:")
        .iter()
        .map(|line| line[..8].to_string())
        .collect();
    assert_eq!(patched, vec!["patch:w1"]);
}

#[tokio::test]
async fn keeps_a_command_that_is_one_line_of_reasonable_length() {
    let fake = with_orchestrator();
    for command in [
        json!(""),
        json!("   "),
        json!("a\nb"),
        json!("a\u{7}b"),
        json!("x".repeat(501)),
        json!(7),
    ] {
        contains(
            &refusal(
                "workspace_set_check",
                with(from_ava(), json!({"command": command})),
                &fake,
            )
            .await,
            "Give command",
        );
    }
    assert!(fake.logged("patch:").is_empty());
}

#[tokio::test]
async fn a_panel_edit_made_while_the_check_is_set_survives_it() {
    let fake = with_orchestrator();
    // The operation reads the model, and the person changes the workers and
    // automatic landing in the panel before its write lands.
    let read = fake.port().snapshot();
    fake.patch("w1", |ws| {
        ws.orchestration = Some(OrchestrationSettings {
            max_workers: 5,
            auto_merge: false,
            worker_model: "opus".into(),
            ..OrchestrationSettings::default()
        });
    });
    let answer = set_check(
        &json!({"command": "make check"}),
        &Caller::of(&from_ava()),
        &read.state,
        &fake.port(),
    )
    .await
    .unwrap();
    matches(
        &answer,
        json!({"checkCommand": "make check", "previous": null}),
    );
    let saved = lock(&fake.state).workspaces[0]
        .orchestration
        .clone()
        .unwrap();
    assert_eq!(saved.check_command.as_deref(), Some("make check"));
    assert_eq!(saved.max_workers, 5);
    assert!(!saved.auto_merge);
    assert_eq!(saved.worker_model, "opus");
}
