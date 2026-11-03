//! The engines' model writes, carried out on the UI thread
//! (`Msg::Model(cmd, answer)`, inventory A6 step 4).
//!
//! The answer goes out in a task rather than from here: the shell republishes
//! the snapshot after `update` returns, and the engine that awaits the answer
//! reads the snapshot next (`pane_create` finds the pane it just made in it).
//! Answered from here, it could wake on another thread before the snapshot
//! has the change.

use hangar_i18n::{Key, args, t, t_with};

use crate::message::{Intent, Message};
use crate::services::inbox::Answer;
use crate::shell::Notice;
use crate::state::{Model, StoreAction, StoreOutcome};

use super::port::ModelCmd;
use super::types::ControlEvent;

pub fn carry_out(
    m: &mut Model,
    cmd: ModelCmd,
    answer: Answer<Result<StoreOutcome, String>>,
) -> iced::Task<Message> {
    let mut then = None;
    let outcome = match cmd {
        ModelCmd::AddPane {
            ws,
            agent,
            near,
            dir,
            cwd,
            name,
            init,
        } => m.apply(StoreAction::AddPane {
            ws,
            agent,
            near,
            dir,
            cwd,
            name,
            init,
        }),
        ModelCmd::ClosePane { ws, pane } => m.apply(StoreAction::ClosePane { ws, pane }),
        ModelCmd::RespawnPane {
            ws,
            pane,
            patch,
            fresh,
        } => {
            let outcome = m.apply(StoreAction::RespawnPane {
                ws: ws.clone(),
                pane: pane.clone(),
                patch,
            });
            // The focus follows the pane to its new id (App.tsx's respawnPane),
            // so the person's keys keep reaching it.
            if let Some(next) = &outcome.pane
                && m.ui.focus_by_workspace.get(&ws) == Some(&pane)
            {
                m.ui.focus_by_workspace.insert(ws, next.clone());
            }
            // TODO(P4): `carryResume(pane, next)` unless `fresh`: a pane
            // restarted while its workspace slept is still owed its session.
            // Hibernation is P4's, which has no entry for it yet.
            let _ = fresh;
            outcome
        }
        ModelCmd::UpdatePane { ws, pane, patch } => {
            m.apply(StoreAction::UpdatePane { ws, pane, patch })
        }
        ModelCmd::UpdateWorkspace { ws, patch } => m.apply(StoreAction::UpdateWorkspace(ws, patch)),
        ModelCmd::OpenWorkspace(ws) => {
            // As the shell's ActivateWorkspace mounts one, without switching to it.
            if !m.ui.opened_ids.contains(&ws) {
                m.ui.opened_ids.push(ws);
            }
            StoreOutcome::default()
        }
        ModelCmd::Touch(ws) => {
            // TODO(P4): `touchWorkspace(ws)`, which puts off the workspace's
            // sleep. Hibernation is P4's, which has no entry for it yet; until
            // it does, no workspace sleeps either.
            let _ = ws;
            StoreOutcome::default()
        }
        ModelCmd::Activate { ws, pane } => {
            then = Some(Intent::FocusPane {
                ws,
                pane,
                terminal: true,
            });
            StoreOutcome::default()
        }
        ModelCmd::Detach(ws) => {
            then = Some(Intent::Detach(ws));
            StoreOutcome::default()
        }
        ModelCmd::Reattach(ws) => {
            then = Some(Intent::Reattach(ws));
            StoreOutcome::default()
        }
        ModelCmd::Announce(event) => {
            then = Some(Intent::Notice(Notice::Agent(announcement(m, &event))));
            StoreOutcome::default()
        }
        ModelCmd::CreateWorkspace { draft, activate } => {
            let previous = m.state().active_workspace_id.clone();
            let outcome = m.apply(StoreAction::AddWorkspace(draft));
            if !activate && outcome.workspace.is_some() {
                m.apply(StoreAction::SetActiveWorkspace(previous));
            }
            outcome
        }
        ModelCmd::RemoveWorkspace(ws) => m.apply(StoreAction::RemoveWorkspace(ws)),
        ModelCmd::BeginDelivery { ws, task } => {
            m.live.control.begin(ws, task);
            m.live.version += 1;
            StoreOutcome::default()
        }
        ModelCmd::EndDelivery { ws, task } => {
            m.live.control.end(&ws, &task);
            m.live.version += 1;
            StoreOutcome::default()
        }
    };
    let reply = iced::Task::future(async move {
        answer.send(Ok(outcome));
        Message::Noop
    });
    match then {
        Some(intent) => iced::Task::batch([reply, iced::Task::done(Message::Intent(intent))]),
        None => reply,
    }
}

/// The line the window shows for what an agent did (App.tsx's `announce`):
/// `control.<kind>` with the event's fields, an unnamed agent as
/// `control.someone`.
pub fn announcement(m: &Model, event: &ControlEvent) -> String {
    let locale = m.locale();
    let key = match event {
        ControlEvent::Created { .. } => Key::ControlCreated,
        ControlEvent::Closed { .. } => Key::ControlClosed,
        ControlEvent::Restarted { .. } => Key::ControlRestarted,
        ControlEvent::Reset { .. } => Key::ControlReset,
        ControlEvent::WorkspaceCreated { .. } => Key::ControlWorkspaceCreated,
        ControlEvent::CheckSet { .. } => Key::ControlCheckSet,
        ControlEvent::ModelShown { .. } => Key::ControlModelShown,
        ControlEvent::ModelClosed { .. } => Key::ControlModelClosed,
        ControlEvent::ModelShownElsewhere { .. } => Key::ControlModelShownElsewhere,
        ControlEvent::ModelClosedElsewhere { .. } => Key::ControlModelClosedElsewhere,
    };
    let by = match event.by() {
        Some(name) => name.to_string(),
        None => t(locale, Key::ControlSomeone).into_owned(),
    };
    let subject = event.subject();
    t_with(
        locale,
        key,
        &args!(
            "by" => by.as_str(),
            "pane" => subject,
            "file" => subject,
            "command" => subject,
            "workspace" => event.workspace(),
        ),
    )
    .into_owned()
}
