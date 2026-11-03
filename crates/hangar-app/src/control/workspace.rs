//! `workspace_create` and `workspace_set_check` (control.ts): what the
//! window decides before and after P8's flow from a repository, and the
//! orchestrator setting its workspace's check command.

use serde_json::{Value, json};

use crate::message::{GitOrigin, GitRequest};
use crate::state::roles::orchestrator_of;
use crate::state::types::OrchestrationSettings;
use crate::state::{AgentActivity, AppState, Patch, PaneId, Workspace};

use super::panes::{announce, tell};
use super::port::{FromGitOutcome, MadeWorkspace, ModelCmd, Port, activity_of};
use super::run::{caller_workspace, orchestration_of};
use super::types::{Caller, ControlEvent, text};

/// The longest check command Hangar keeps: a command, not a script.
const CHECK_MAX: usize = 500;

/// How often, and for how long, a new orchestrator's pane is watched for a
/// question.
const TRUST_WATCH_MS: u64 = 2_000;
const TRUST_WATCH_TICKS: u32 = 300;

/// What the agent is told comes next in a new workspace.
const FIRST_MESSAGE: &str = "queued for the orchestrator: it reads the repository, installs its dependencies, \
sets the check command, runs the tests once and reports. It then waits for an objective, which the person \
gives it or you pass on with pane_send.";

/// A string argument, or absent: anything else is refused with `refusal`.
fn string_arg(args: &Value, name: &str, null_is_absent: bool, refusal: &str) -> Result<(), String> {
    match args.get(name) {
        None | Some(Value::String(_)) => Ok(()),
        Some(Value::Null) if null_is_absent => Ok(()),
        Some(_) => Err(refusal.into()),
    }
}

/// `workspace_create`: an agent hands Hangar a repository, and Hangar clones
/// it, makes the workspace and starts an orchestrator in it. It answers once
/// the workspace exists, or says why not.
pub async fn create(
    args: &Value,
    caller: &Caller,
    state: &AppState,
    port: &Port,
) -> Result<Value, String> {
    let Some(flow) = port.from_git() else {
        return Err("This Hangar window cannot create a workspace from a repository.".into());
    };
    string_arg(
        args,
        "repo",
        false,
        "repo must be a string: owner/repo, or the repository's https or ssh address.",
    )?;
    string_arg(args, "name", true, "name must be a string.")?;
    string_arg(
        args,
        "parent",
        true,
        "parent must be a string: an absolute folder, such as E:\\ or /mnt/data.",
    )?;
    let Some(repo) = text(args.get("repo")) else {
        return Err("Give repo: owner/repo, or the repository's https or ssh address.".into());
    };

    let asker = caller_workspace(state, caller);
    let outcome = flow
        .run(GitRequest {
            repo,
            name: text(args.get("name")),
            parent: text(args.get("parent")),
            // The consent card names the pane that asked, by its name.
            origin: GitOrigin::Agent {
                pane: caller.pane_name.clone(),
            },
            asker_cwd: asker.map(|ws| ws.cwd.clone()).or(caller.cwd.clone()),
        })
        .await;
    let shown = |workspace: &MadeWorkspace| {
        json!({
            "name": workspace.name,
            "folder": workspace.cwd,
            "id": workspace.id,
            "orchestrator": workspace.orchestrator.as_ref().map(|(_, name)| name),
        })
    };
    match outcome {
        FromGitOutcome::Refused { reason } => Err(reason),
        FromGitOutcome::Existing { workspace, notes } => Ok(json!({
            "created": false,
            "workspace": shown(&workspace),
            "note": notes.join(" "),
        })),
        FromGitOutcome::Created {
            workspace,
            repo_url,
            mine,
            reused_folder,
            trusted,
            notes,
        } => {
            announce(
                port,
                ControlEvent::WorkspaceCreated {
                    by: caller.pane_name.clone(),
                    pane: workspace
                        .orchestrator
                        .as_ref()
                        .map_or_else(|| workspace.name.clone(), |(_, name)| name.clone()),
                    workspace: workspace.name.clone(),
                },
            )
            .await;
            // A folder Claude Code was not told to trust: its question comes
            // before any event of the mod, so Hangar watches the pane and
            // tells the asker.
            if !trusted
                && let (Some((orchestrator, _)), Some(asker)) =
                    (&workspace.orchestrator, &caller.pane_id)
            {
                let watch = tell_when_waiting(
                    port.clone(),
                    asker.clone(),
                    orchestrator.clone(),
                    workspace.name.clone(),
                );
                port.spawn(Box::pin(watch));
            }
            Ok(json!({
                "created": true,
                "workspace": shown(&workspace),
                "repository": repo_url,
                "yours": mine,
                "cloned": !reused_folder,
                // Where it landed: the person is told it, with no card for
                // their own repository.
                "folder": workspace.cwd,
                "firstMessage": FIRST_MESSAGE,
                "note": notes.join(" "),
            }))
        }
    }
}

/// Tells the agent that asked for a workspace when the new orchestrator's
/// pane stops waiting for the person, on Claude Code's question about
/// trusting the folder, which only they answer. Once, and never an error for
/// the caller: it runs after the answer was given.
async fn tell_when_waiting(port: Port, asker: PaneId, orchestrator: PaneId, workspace: String) {
    for _ in 0..TRUST_WATCH_TICKS {
        port.sleep(TRUST_WATCH_MS).await;
        let snapshot = port.snapshot();
        let seen = activity_of(&snapshot, &orchestrator);
        // The mod reports the session: it is past the question, whatever it
        // waits on now.
        if seen.is_some_and(|seen| seen.bridged) {
            return;
        }
        if seen.map(|seen| seen.activity) != Some(AgentActivity::Waiting) {
            continue;
        }
        let message = format!(
            "The orchestrator of the new workspace \"{workspace}\" is waiting for the person: Claude Code asks \
             them to trust the cloned folder (\"Do you trust the files in this folder?\"). Only they can answer \
             it, in that pane. Do not answer it for them; the orchestrator reads its first message once they have."
        );
        // The asker is gone, or the queue is: nobody is left to tell.
        let _ = port.enqueue(&asker, &message, Some("hangar")).await;
        return;
    }
}

/// `workspace_set_check`: the orchestrator sets the check command of its own
/// workspace. The panel shows the field, and the window says on screen that
/// it was set and by whom.
pub async fn set_check(
    args: &Value,
    caller: &Caller,
    state: &AppState,
    port: &Port,
) -> Result<Value, String> {
    let ws: Option<&Workspace> = caller_workspace(state, caller);
    let orchestrator = ws.and_then(orchestrator_of);
    let (Some(ws), Some(_)) = (
        ws,
        orchestrator.filter(|pane| caller.pane_id.as_deref() == Some(pane.id.as_str())),
    ) else {
        return Err(
            "workspace_set_check is for a workspace's orchestrator, and only for its own workspace. If you are \
             not the orchestrator, say in a comment on your task which command checks the work."
                .into(),
        );
    };
    let command = text(args.get("command")).filter(|command| {
        command.encode_utf16().count() <= CHECK_MAX
            && !command.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
    });
    let Some(command) = command else {
        return Err(format!(
            "Give command: one line of at most {CHECK_MAX} characters, the one a change has to pass (build and tests)."
        ));
    };
    let settings = orchestration_of(ws);
    let updated = OrchestrationSettings {
        check_command: Some(command.clone()),
        ..settings.clone()
    };
    tell(
        port,
        ModelCmd::UpdateWorkspace {
            ws: ws.id.clone(),
            patch: Patch::new(move |ws: &mut Workspace| ws.orchestration = Some(updated.clone())),
        },
    )
    .await;
    announce(
        port,
        ControlEvent::CheckSet {
            by: caller.pane_name.clone(),
            workspace: ws.name.clone(),
            command: command.clone(),
        },
    )
    .await;
    Ok(json!({
        "workspace": ws.name,
        "checkCommand": command,
        "previous": settings.check_command,
        "note": "Workers' deliveries are checked with it from now on; the person sees it in the workspace's \
                 orchestration panel.",
    }))
}
