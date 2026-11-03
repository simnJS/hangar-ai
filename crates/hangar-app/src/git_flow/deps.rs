//! The real dependencies of the flow: hangar-core for git, the consent queue
//! for the person, and the model (through the inbox) for the workspace.
//!
//! The flow runs on tokio, so everything that reads or writes the model goes
//! through a message and waits for its answer (`services::inbox::Answer`, the
//! rule of PACKAGES.md for what runs off the UI thread); what only reads
//! (`find_workspace`, the projects folder) reads the snapshot. The dialog
//! starts the flow with this, and so can P18's `workspace_create`:
//!
//! ```ignore
//! let outcome = git_flow::run(&request, &git_flow::AppDeps::new(&services)).await;
//! ```

use hangar_core::git_clone::{git_clone, git_clone_discard, git_repo_ownership};
use iced::Task;

use crate::accounts;
use crate::message::{Intent, Message};
use crate::services::Services;
use crate::services::inbox::Answer;
use crate::state::roles::orchestrator_of;
use crate::state::{AgentId, Model, Pane, Patch, StoreAction, WorkspaceDraft};
use crate::state::types::PaneRole;

use super::Msg;
use super::clone::{
    CloneOutcome, DiscardOutcome, Ownership, ParseResult, new_clone_job_id, parse_repo_input,
};
use super::consent::{self, CONSENT_TIMEOUT_MS, ConsentAnswer, ConsentRequest};
use super::flow::{Created, FromGitDeps, NewWorkspace, PaneRef, WorkspaceRef};
use super::paths::{projects_folder_of, same_folder};

#[derive(Clone)]
pub struct AppDeps {
    services: Services,
    /// The folder the person chose in the dialog. It stands for the « Projects
    /// folder » setting, which it never changes.
    parent: Option<String>,
    /// The job the dialog follows and cancels.
    job_id: Option<String>,
}

impl AppDeps {
    pub fn new(services: &Services) -> AppDeps {
        // The trace of the person's answers goes to the activity log from the
        // first request on, whichever package started the flow.
        if let Some(remote) = services.core().core().remote() {
            consent::log_answers_to(remote.clone());
        }
        AppDeps {
            services: services.clone(),
            parent: None,
            job_id: None,
        }
    }

    /// The clone goes into `parent` (the dialog's field), over the setting.
    pub fn clone_into(mut self, parent: impl Into<String>) -> AppDeps {
        self.parent = Some(parent.into());
        self
    }

    /// The clone runs under this job id, so that its progress can be followed
    /// and the clone cancelled.
    pub fn job(mut self, job_id: impl Into<String>) -> AppDeps {
        self.job_id = Some(job_id.into());
        self
    }

    /// A message for the model, and its answer.
    async fn ask<T: Send + 'static>(
        &self,
        message: impl FnOnce(Answer<Result<T, String>>) -> Msg,
    ) -> Result<T, String> {
        let (answer, receiver) = Answer::channel();
        if !self.services.send(Message::GitFlow(message(answer))) {
            return Err("Hangar is closing.".to_string());
        }
        receiver
            .await
            .map_err(|_| "Hangar did not answer.".to_string())?
    }
}

impl FromGitDeps for AppDeps {
    async fn parse(&self, input: &str) -> Result<ParseResult, String> {
        Ok(parse_repo_input(input))
    }

    async fn ownership(&self, owner: &str, host: &str) -> Result<Ownership, String> {
        Ok(git_repo_ownership(owner.to_string(), host.to_string()).await)
    }

    fn projects_folder(&self) -> Option<String> {
        self.parent
            .clone()
            .or_else(|| projects_folder_of(&self.services.snapshot().state.settings))
    }

    fn find_workspace(&self, folder: &str) -> Option<WorkspaceRef> {
        let snapshot = self.services.snapshot();
        let ws = snapshot
            .state
            .workspaces
            .iter()
            .find(|ws| same_folder(&ws.cwd, folder))?;
        Some(WorkspaceRef {
            id: ws.id.clone(),
            name: ws.name.clone(),
            cwd: ws.cwd.clone(),
            orchestrator: orchestrator_of(ws).map(|pane| PaneRef {
                id: pane.id.clone(),
                name: pane.name.clone(),
            }),
        })
    }

    async fn consent(&self, request: ConsentRequest) -> ConsentAnswer {
        consent::queue().ask(request, CONSENT_TIMEOUT_MS).await
    }

    async fn clone_repo(
        &self,
        url: &str,
        parent: &str,
        name: &str,
        job_id: &str,
    ) -> Result<CloneOutcome, String> {
        Ok(git_clone(
            self.services.core().host().clone(),
            url.to_string(),
            parent.to_string(),
            name.to_string(),
            job_id.to_string(),
        )
        .await)
    }

    fn new_job_id(&self) -> String {
        self.job_id.clone().unwrap_or_else(new_clone_job_id)
    }

    fn new_consent_id(&self) -> String {
        new_clone_job_id()
    }

    // `git_clone_discard` refuses a reused folder, a link or anything it did
    // not clone; the flow then reports where the folder is left.
    fn can_discard(&self) -> bool {
        true
    }

    async fn discard_clone(&self, path: &str) -> Result<(), String> {
        match git_clone_discard(path.to_string()).await {
            DiscardOutcome::Removed => Ok(()),
            DiscardOutcome::Refused { reason, .. } => Err(reason),
        }
    }

    async fn create_workspace(&self, draft: NewWorkspace) -> Result<Created, String> {
        self.ask(|answer| Msg::Create(draft, answer)).await
    }

    async fn remove_workspace(&self, id: &str) -> Result<(), String> {
        self.ask(|answer| Msg::Remove(id.to_string(), answer)).await
    }

    fn can_trust(&self) -> bool {
        true
    }

    // The account the new workspace's panes will start on: nothing is chosen
    // for it yet, so the settings' default, as `launch_account` decides.
    async fn trust_folder(&self, path: &str) -> Result<(), String> {
        let default = self.services.snapshot().state.settings.claude_account.clone();
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let account = accounts::launch_account(default.as_deref(), None, None);
            let account = (account.id != accounts::MAIN_ACCOUNT).then_some(account.id);
            hangar_core::mcp_scope::claude_trust_folder(path, account).map(|_| ())
        })
        .await
        .map_err(|err| format!("the task stopped: {err}"))?
    }

    async fn send_first_message(&self, pane: &str, text: &str) -> Result<(), String> {
        hangar_core::bridge::bridge_enqueue(
            self.services.core().host(),
            pane.to_string(),
            text.to_string(),
            Some("hangar".to_string()),
        );
        Ok(())
    }

    async fn open_workspace(&self, id: &str) -> Result<(), String> {
        self.ask(|answer| Msg::Open(id.to_string(), answer)).await
    }

    fn show_workspace(&self, id: &str) {
        self.services
            .send(Message::Intent(Intent::ActivateWorkspace(id.to_string())));
    }
}

// ── on the UI thread: what the flow asked of the model ────────────────

/// The workspace with a single Claude Code pane, which is its orchestrator
/// from the start, so that it launches with Hangar's server.
pub(super) fn create_workspace(m: &mut Model, draft: NewWorkspace) -> Result<Created, String> {
    let before = m.state().active_workspace_id.clone();
    let made = m.apply(StoreAction::AddWorkspace(WorkspaceDraft {
        name: draft.name,
        cwd: draft.cwd,
        layout: 1,
        agents: vec![AgentId::Claude],
        shell_id: None,
        ..WorkspaceDraft::default()
    }));
    let unmade = || "Hangar could not create the workspace.".to_string();
    let id = made.workspace.ok_or_else(unmade)?;
    let Some((pane, model)) = m.workspace(&id).and_then(|ws| {
        let pane = ws.panes.first()?;
        let model = ws.orchestration.clone().unwrap_or_default().orchestrator_model;
        Some((pane.id.clone(), model))
    }) else {
        m.apply(StoreAction::RemoveWorkspace(id));
        return Err(unmade());
    };
    // Same tick as the add: the pane mounts, and launches, as the
    // orchestrator it was created to be, so it holds Hangar's server.
    m.apply(StoreAction::UpdatePane {
        ws: id.clone(),
        pane: pane.clone(),
        patch: Patch::new(move |pane: &mut Pane| {
            pane.role = Some(PaneRole::Orchestrator);
            pane.model = Some(model.clone());
        }),
    });
    // An agent asked: the person stays where they were.
    if !draft.activate {
        if let Some(before) = before {
            m.apply(StoreAction::SetActiveWorkspace(Some(before)));
        }
    }
    let ws = m.workspace(&id).ok_or_else(unmade)?;
    let orchestrator = ws
        .panes
        .iter()
        .find(|candidate| candidate.id == pane)
        .map(|pane| PaneRef {
            id: pane.id.clone(),
            name: pane.name.clone(),
        })
        .ok_or_else(unmade)?;
    Ok(Created {
        workspace: WorkspaceRef {
            id: ws.id.clone(),
            name: ws.name.clone(),
            cwd: ws.cwd.clone(),
            orchestrator: Some(orchestrator.clone()),
        },
        orchestrator,
    })
}

/// Takes a workspace made for the flow back.
pub(super) fn remove_workspace(m: &mut Model, id: String) {
    m.ui.opened_ids.retain(|opened| opened != &id);
    m.apply(StoreAction::RemoveWorkspace(id));
}

/// Mounts the workspace's terminals.
pub(super) fn open_workspace(m: &mut Model, id: String) {
    if !m.ui.opened_ids.contains(&id) {
        m.ui.opened_ids.push(id);
    }
}

/// The task that runs `request` and reports to `Msg::Finished`: what
/// `Intent::CreateWorkspaceFromGit` does.
pub(super) fn run_request(
    services: &Services,
    request: crate::message::GitRequest,
) -> Task<Message> {
    let deps = AppDeps::new(services);
    Task::perform(
        async move { super::flow::run(&request, &deps).await },
        |outcome| Message::GitFlow(Msg::Finished(outcome)),
    )
}
