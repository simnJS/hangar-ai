//! The glue the shell lays at boot between packages that must not know each
//! other (PACKAGES.md, rule 4): what one package's engine reaches of another,
//! installed once, as App.tsx built `controlDeps` from every lib it held.
//!
//! - `workspace_create`, the control op (P18), runs P8's git flow
//!   (`fromGit: fromGitDeps` in App.tsx's `controlDeps`).

use std::sync::Arc;

use iced::Task;

use crate::control::port::{BoxFuture, FromGit, FromGitOutcome, MadeWorkspace};
use crate::control::{Hook, Msg};
use crate::git_flow::{self, AppDeps, WorkspaceRef};
use crate::message::{GitRequest, Message};
use crate::services::Services;

/// The messages that install every hook on the control bus.
pub fn install(services: &Services) -> Task<Message> {
    let from_git: Arc<dyn FromGit> = Arc::new(GitFlow(AppDeps::new(services)));
    Task::done(Message::Control(Msg::Install(Hook::FromGit(from_git))))
}

/// P8's flow as P18's port asks for it.
struct GitFlow(AppDeps);

impl FromGit for GitFlow {
    fn run(&self, request: GitRequest) -> BoxFuture<FromGitOutcome> {
        let deps = self.0.clone();
        Box::pin(async move { outcome(git_flow::run(&request, &deps).await) })
    }
}

/// The flow's answer in the port's words: the refusal's reason as the agent
/// reads it, the workspace with its orchestrator.
fn outcome(outcome: git_flow::FromGitOutcome) -> FromGitOutcome {
    match outcome {
        git_flow::FromGitOutcome::Refused { reason, .. } => FromGitOutcome::Refused { reason },
        git_flow::FromGitOutcome::Existing { workspace, notes } => FromGitOutcome::Existing {
            workspace: made(workspace),
            notes,
        },
        git_flow::FromGitOutcome::Created {
            workspace,
            repo,
            mine,
            trusted,
            reused_folder,
            notes,
            ..
        } => FromGitOutcome::Created {
            workspace: made(workspace),
            repo_url: repo.url,
            mine,
            reused_folder,
            trusted,
            notes,
        },
    }
}

fn made(workspace: WorkspaceRef) -> MadeWorkspace {
    MadeWorkspace {
        id: workspace.id,
        name: workspace.name,
        cwd: workspace.cwd,
        orchestrator: workspace.orchestrator.map(|pane| (pane.id, pane.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_flow::{FromGitRefusal, PaneRef};

    fn workspace() -> WorkspaceRef {
        WorkspaceRef {
            id: "w".into(),
            name: "site".into(),
            cwd: "/code/site".into(),
            orchestrator: Some(PaneRef {
                id: "p".into(),
                name: "Ava".into(),
            }),
        }
    }

    #[test]
    fn the_flow_s_outcome_reads_as_the_port_s() {
        assert_eq!(
            outcome(git_flow::FromGitOutcome::Refused {
                kind: FromGitRefusal::Declined,
                reason: "The person said no.".into(),
            }),
            FromGitOutcome::Refused {
                reason: "The person said no.".into()
            }
        );
        let FromGitOutcome::Existing { workspace, notes } =
            outcome(git_flow::FromGitOutcome::Existing {
                workspace: workspace(),
                notes: vec!["shown".into()],
            })
        else {
            panic!("an existing workspace expected");
        };
        assert_eq!(workspace.orchestrator, Some(("p".into(), "Ava".into())));
        assert_eq!(notes, ["shown"]);
    }
}
