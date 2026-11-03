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
    let mut touched = None;
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
            // Restarted while its workspace slept: still owed its session,
            // unless the restart asked for a fresh one (`carryResume`).
            if let Some(next) = &outcome.pane
                && !fresh
            {
                m.live.panes_logic.carry_resume(&pane, next);
            }
            outcome
        }
        ModelCmd::UpdatePane { ws, pane, patch } => {
            m.apply(StoreAction::UpdatePane { ws, pane, patch })
        }
        ModelCmd::UpdateWorkspace { ws, patch } => m.apply(StoreAction::UpdateWorkspace(ws, patch)),
        ModelCmd::OpenWorkspace(ws) => {
            // Opened by an agent, an asleep workspace is awake again (P4's
            // hibernation), and mounted as the shell's ActivateWorkspace
            // mounts one, without switching to it.
            m.live.panes_logic.mark_awake(&ws);
            if !m.ui.opened_ids.contains(&ws) {
                m.ui.opened_ids.push(ws);
            }
            m.live.version += 1;
            StoreOutcome::default()
        }
        ModelCmd::Touch(ws) => {
            // Sending it something is using it, as typing is: its sleep waits.
            touched = Some(ws);
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
    let mut tasks = vec![reply];
    if let Some(intent) = then {
        tasks.push(iced::Task::done(Message::Intent(intent)));
    }
    if let Some(workspace) = touched {
        tasks.push(iced::Task::done(Message::PanesLogic(
            crate::panes_logic::Msg::Typed { workspace },
        )));
    }
    iced::Task::batch(tasks)
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hangar_i18n::Locale;

    use super::*;
    use crate::state::{AppState, Snapshot};

    fn delivery(m: &mut Model, begin: bool) {
        let (ws, task) = ("w1".to_string(), "t1".to_string());
        let cmd = if begin {
            ModelCmd::BeginDelivery { ws, task }
        } else {
            ModelCmd::EndDelivery { ws, task }
        };
        let _ = carry_out(m, cmd, Answer::channel().0);
    }

    #[test]
    fn a_delivery_runs_until_each_of_its_holders_ends_it_and_the_snapshot_says_so() {
        let mut m = Model::default();
        // The person's « Fusionner » and the engine's own run of it.
        delivery(&mut m, true);
        delivery(&mut m, true);
        delivery(&mut m, false);
        assert!(m.live.control.task_delivering("w1", "t1"));
        assert_eq!(
            Snapshot::of(&m).delivering.get("w1"),
            Some(&HashSet::from(["t1".to_string()]))
        );
        delivery(&mut m, false);
        assert!(!m.live.control.is_delivering("w1"));
        assert!(Snapshot::of(&m).delivering.is_empty());
        // Ending once more is harmless.
        delivery(&mut m, false);
        assert!(Snapshot::of(&m).delivering.is_empty());
    }

    #[test]
    fn says_on_screen_what_an_agent_did_in_the_persons_language() {
        let mut m = Model::default();
        let mut state = AppState::default();
        state.settings.locale = Some(Locale::En);
        m.hydrate(state.clone());
        let created = ControlEvent::Created {
            by: Some("Ava".into()),
            pane: "Reviewer".into(),
            workspace: "Shop".into(),
        };
        assert_eq!(announcement(&m, &created), "Ava opened Reviewer in Shop");
        let shown = ControlEvent::ModelShownElsewhere {
            by: None,
            file: "chair.glb".into(),
            workspace: "Shop".into(),
        };
        assert_eq!(
            announcement(&m, &shown),
            "An agent opened chair.glb in the preview of Shop"
        );
        state.settings.locale = Some(Locale::Fr);
        m.hydrate(state);
        assert_eq!(
            announcement(&m, &created),
            "Ava a ouvert Reviewer dans Shop"
        );
        assert!(announcement(&m, &shown).starts_with("Un agent a ouvert chair.glb dans"));
    }
}
