//! P8: Workspace from Git.
//!
//! Ports lib/{workspaceFromGit,gitClone}.ts and strings/workspaceFromGit.ts.
//! The flow is [`run`]: the workspace dialog starts it as the person, and P18's
//! `workspace_create` starts it as an agent, both with [`AppDeps`] (or, for a
//! test, with fakes of the [`FromGitDeps`] trait). The consent an agent's
//! request waits on is the app-wide [`consent::queue`], answered by the
//! consent card (`consent_card`).
//!
//! Files: `paths` (folder arithmetic), `clone` (what the dialog and the flow
//! need around a clone), `consent` (the queue and its trace), `flow` (the
//! flow itself), `deps` (the real dependencies).

pub mod clone;
pub mod consent;
mod deps;
pub mod flow;
pub mod paths;

use iced::futures::stream;
use iced::{Subscription, Task};

use hangar_i18n::{Key, Translator, args};

use crate::message::{Intent, Message};
use crate::services::inbox::Answer;
use crate::services::{CoreEvent, Services};
use crate::shell::Notice;
use crate::state::Model;
use crate::workspace_dialog::git_form::one_line;

pub use deps::AppDeps;
#[allow(unused_imports)]
pub use flow::{
    Created, FromGitDeps, FromGitOutcome, FromGitRefusal, FromGitRequest, NewWorkspace, PaneRef,
    WorkspaceRef, first_message, run,
};

/// The package's UI state; lives in `App.pkg.git_flow`. The clones in flight
/// and the consent queue are app-wide (`flow::Inflight::global`,
/// `consent::queue`) because the flow also runs for P18's engine, away from
/// the window; nothing of them needs to be drawn from here.
#[derive(Debug, Default)]
pub struct State;

/// The package's shared read-model, which other packages may read; lives in
/// `Model.live.git_flow`.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::GitFlow(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The app has started: the trace of the consent answers gets its log.
    Started,
    /// The flow, running on tokio, asks for its workspace (the model is the
    /// UI thread's alone).
    Create(NewWorkspace, Answer<Result<Created, String>>),
    /// The flow takes back the workspace it made.
    Remove(String, Answer<Result<(), String>>),
    /// The flow mounts the terminals of the workspace it made.
    Open(String, Answer<Result<(), String>>),
    /// A request made through `Intent::CreateWorkspaceFromGit` ended.
    Finished(FromGitOutcome),
}

pub fn update(_st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Started => {
            if let Some(remote) = sv.core().core().remote() {
                consent::log_answers_to(remote.clone());
            }
            Task::none()
        }
        Msg::Create(draft, answer) => {
            answer.send(deps::create_workspace(m, draft));
            Task::none()
        }
        Msg::Remove(id, answer) => {
            deps::remove_workspace(m, id);
            answer.send(Ok(()));
            Task::none()
        }
        Msg::Open(id, answer) => {
            deps::open_workspace(m, id);
            answer.send(Ok(()));
            Task::none()
        }
        Msg::Finished(outcome) => finished(m, &outcome),
    }
}

/// What the person is told when a request that did not come from the dialog
/// ends: the dialog has its own words, in its own place.
fn finished(m: &Model, outcome: &FromGitOutcome) -> Task<Message> {
    let t = Translator::new(m.locale());
    let notice = match outcome {
        FromGitOutcome::Existing { workspace, .. } => Notice::Agent(
            t.t_with(
                Key::CreateGitExisting,
                &args!("path" => workspace.cwd.as_str()),
            )
            .into_owned(),
        ),
        FromGitOutcome::Created {
            workspace,
            reused_folder: true,
            ..
        } => Notice::Agent(
            t.t_with(
                Key::CreateGitReused,
                &args!("path" => workspace.cwd.as_str()),
            )
            .into_owned(),
        ),
        FromGitOutcome::Created { .. } => return Task::none(),
        FromGitOutcome::Refused { reason, .. } => Notice::Problem(
            t.t_with(
                Key::CreateGitFailed,
                &args!("reason" => one_line(reason, 160).as_str()),
            )
            .into_owned(),
        ),
    };
    Task::done(Message::Intent(Intent::Notice(notice)))
}

/// A stream that says once that the app has started: this package has no
/// other hook that runs before the first request.
fn started() -> impl iced::futures::Stream<Item = Message> {
    stream::once(async { Message::GitFlow(Msg::Started) })
}

pub fn subscription(_st: &State, _m: &Model) -> Subscription<Message> {
    Subscription::run(started)
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it.
pub fn on_intent(
    _st: &mut State,
    _m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    match intent {
        Intent::CreateWorkspaceFromGit(request) => Some(deps::run_request(sv, request.clone())),
        _ => None,
    }
}

/// Every core event, after the shell's own handling of it.
pub fn on_event(
    _st: &mut State,
    _m: &mut Model,
    _sv: &Services,
    _event: &CoreEvent,
) -> Option<Task<Message>> {
    None
}
