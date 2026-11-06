//! P7: saved commands (`lib/savedCommands.ts`, `CommandMenu.tsx`,
//! `CommandDialog.tsx`; inventory §2.1/6).
//!
//! A pane offers its workspace's saved commands, then the global ones
//! ([`merged`]); that order is also what `command.runN` counts through, so
//! the menu and the shortcuts never disagree on which one is the third.
//! Running one types it into the pane, or every pane for a broadcast one
//! ([`run`]). The menu hangs off the ⚡ button of the pane header
//! (`menu.rs`), or off its « … » button when the header folded the ⚡ away;
//! the dialog writes one down (`dialog.rs`).

mod dialog;
mod menu;

use iced::{Element, Subscription, Task};

use hangar_core::bridge::bridge_enqueue;
use hangar_core::pty::pty_write;

use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Overlay;
use crate::state::types::SavedCommand;
use crate::state::{
    AgentActivity, CommandScope, Model, PaneId, Settings, StoreAction, Workspace, WsId,
};

pub use dialog::DialogMsg;

/// The package's UI state; lives in `App.pkg.saved_commands`.
#[derive(Debug, Default)]
pub struct State {
    /// The pane whose menu is open, and whether it hangs off the « … » button.
    menu: Option<OpenMenu>,
    dialog: Option<dialog::Form>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenMenu {
    pane: PaneId,
    from_more: bool,
}

#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::SavedCommands(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The ⚡ button: opens the pane's menu, or closes it.
    ToggleMenu(PaneId),
    /// Opens the pane's menu off its « … » button (the pane menu's entry).
    OpenMenu {
        pane: PaneId,
        from_more: bool,
    },
    /// Escape, Tab, a press elsewhere.
    CloseMenu,
    /// Runs a command of the pane's menu.
    Run {
        pane: PaneId,
        scope: CommandScope,
        id: String,
    },
    Edit {
        pane: PaneId,
        id: String,
    },
    Remove {
        pane: PaneId,
        scope: CommandScope,
        id: String,
    },
    Add(PaneId),
    Dialog(DialogMsg),
}

pub fn update(st: &mut State, m: &mut Model, _sv: &Services, msg: Msg) -> Task<Message> {
    match msg {
        Msg::ToggleMenu(pane) => {
            let open = OpenMenu {
                pane,
                from_more: false,
            };
            st.menu = if st.menu.as_ref() == Some(&open) {
                None
            } else {
                Some(open)
            };
            Task::none()
        }
        Msg::OpenMenu { pane, from_more } => {
            st.menu = Some(OpenMenu { pane, from_more });
            Task::none()
        }
        Msg::CloseMenu => close_menu(st, m),
        Msg::Run { pane, scope, id } => {
            let request = run_request(m, &pane, scope, &id);
            let back = close_menu(st, m);
            match request {
                // Through the intent, as `command.runN` goes: the pane logic
                // counts it as the workspace being used, which a command run
                // from the menu is as much as one run from the keyboard
                // (hibernation must not put the workspace to sleep under it).
                Some(intent) => back.chain(Task::done(Message::Intent(intent))),
                None => back,
            }
        }
        Msg::Edit { pane, id } => {
            st.menu = None;
            let Some(ws) = crate::palette::sources::workspace_of(m, &pane) else {
                return Task::none();
            };
            Task::done(Message::Intent(Intent::OpenOverlay(
                Overlay::CommandDialog {
                    ws: ws.id.clone(),
                    pane,
                    edit: Some(id),
                },
            )))
        }
        Msg::Remove { pane, scope, id } => {
            if let Some(ws) = crate::palette::sources::workspace_of(m, &pane) {
                let ws = ws.id.clone();
                m.apply(StoreAction::RemoveSavedCommand { scope, ws, id });
            }
            Task::none()
        }
        Msg::Add(pane) => {
            st.menu = None;
            let Some(ws) = crate::palette::sources::workspace_of(m, &pane) else {
                return Task::none();
            };
            Task::done(Message::Intent(Intent::OpenOverlay(
                Overlay::CommandDialog {
                    ws: ws.id.clone(),
                    pane,
                    edit: None,
                },
            )))
        }
        Msg::Dialog(msg) => dialog::update(&mut st.dialog, m, msg),
    }
}

/// Closing hands the keyboard back to the pane's terminal, wherever it was
/// in the menu: typing is what comes after a menu, whatever it did.
fn close_menu(st: &mut State, m: &Model) -> Task<Message> {
    let Some(open) = st.menu.take() else {
        return Task::none();
    };
    match crate::palette::sources::workspace_of(m, &open.pane) {
        Some(ws) => Task::done(Message::Intent(Intent::FocusPane {
            ws: ws.id.clone(),
            pane: open.pane,
            terminal: true,
        })),
        None => Task::none(),
    }
}

/// What a pane offers: the workspace's own commands, then the global ones
/// (`mergeCommands`).
pub fn merged<'a>(ws: &'a Workspace, settings: &'a Settings) -> Vec<&'a SavedCommand> {
    ws.saved_commands
        .iter()
        .chain(settings.saved_commands.iter())
        .collect()
}

/// What picking a command in a pane's menu asks for: `command.runN` of its
/// place in what the pane offers ([`merged`]), for the workspace the pane is
/// in. `None` when the pane or the command is gone.
fn run_request(m: &Model, pane: &PaneId, scope: CommandScope, id: &str) -> Option<Intent> {
    let ws = crate::palette::sources::workspace_of(m, pane)?;
    // The workspace's commands come first, then the global ones.
    let (list, before) = match scope {
        CommandScope::Workspace => (&ws.saved_commands, 0),
        CommandScope::Global => (&m.settings().saved_commands, ws.saved_commands.len()),
    };
    let at = list.iter().position(|command| command.id == id)?;
    Some(Intent::RunSavedCommand {
        ws: ws.id.clone(),
        from: pane.clone(),
        index: before + at,
    })
}

/// A saved command where it is kept.
fn find<'a>(
    ws: &'a Workspace,
    settings: &'a Settings,
    scope: CommandScope,
    id: &str,
) -> Option<&'a SavedCommand> {
    let list = match scope {
        CommandScope::Workspace => &ws.saved_commands,
        CommandScope::Global => &settings.saved_commands,
    };
    list.iter().find(|command| command.id == id)
}

/// Where a saved command is kept, by its id: the workspace's list first.
fn locate(ws: &Workspace, settings: &Settings, id: &str) -> Option<(CommandScope, SavedCommand)> {
    for scope in [CommandScope::Workspace, CommandScope::Global] {
        if let Some(command) = find(ws, settings, scope, id) {
            return Some((scope, command.clone()));
        }
    }
    None
}

/// How a command reaches one pane (`runSavedCommand`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Delivery {
    /// Typed: with a CR when it runs itself, without when it waits on the
    /// prompt for the person to finish it.
    Type(String),
    /// Handed to the pane's hangar-bridge mod, which submits it once its
    /// Claude Code is free: typed into a permission dialog, the command
    /// would answer the dialog.
    Queue(String),
}

fn delivery(command: &SavedCommand, activity: Option<(AgentActivity, bool)>) -> Delivery {
    let busy = matches!(
        activity,
        Some((AgentActivity::Working | AgentActivity::Waiting, true))
    );
    if command.auto_run && busy {
        Delivery::Queue(command.command.clone())
    } else if command.auto_run {
        Delivery::Type(format!("{}\r", command.command))
    } else {
        Delivery::Type(command.command.clone())
    }
}

/// Types `command` into every pane of `panes`, off the UI thread.
pub fn run(sv: &Services, command: &SavedCommand, panes: Vec<PaneId>) -> Task<Message> {
    // What each pane is doing, as the pane logic last published it.
    let snapshot = sv.snapshot();
    let deliveries: Vec<(PaneId, Delivery)> = panes
        .into_iter()
        .map(|pane| {
            let activity = snapshot
                .activity
                .get(&pane)
                .map(|state| (state.activity, state.bridged));
            let delivery = delivery(command, activity);
            (pane, delivery)
        })
        .collect();
    let core = sv.core().clone();
    Task::perform(
        tokio::task::spawn_blocking(move || {
            for (pane, delivery) in deliveries {
                match delivery {
                    // Nothing to tell anyone when a pane has gone meanwhile:
                    // the web app dropped these errors as well.
                    Delivery::Type(data) => {
                        let _ = pty_write(core.pty(), pane, data);
                    }
                    Delivery::Queue(text) => {
                        bridge_enqueue(core.host(), pane, text, None);
                    }
                }
            }
        }),
        |_| Message::Noop,
    )
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = (st, m);
    Subscription::none()
}

/// Sets the dialog up when its overlay opens, drops it when it goes, closes
/// the menu with the other popovers, and runs `command.runN`.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let task = match intent {
        Intent::RunSavedCommand { ws, from, index } => {
            let workspace = m.workspace(ws)?;
            let command = (*merged(workspace, m.settings()).get(*index)?).clone();
            let panes = if command.broadcast {
                workspace.panes.iter().map(|pane| pane.id.clone()).collect()
            } else {
                vec![from.clone()]
            };
            Some(run(sv, &command, panes))
        }
        Intent::ClosePopovers(_) => {
            st.menu = None;
            None
        }
        _ => None,
    };
    dialog::follow(&mut st.dialog, m);
    task
}

pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}

/// The pane header's ⚡ button with its menu (embedded by the pane header).
pub fn menu_view<'a>(st: &'a State, ctx: Ctx<'a>, pane: &'a PaneId) -> Element<'a, Message> {
    menu::bolt(st, ctx, pane)
}

/// Whether the pane's menu is open off its « … » button, for the pane menu,
/// which then hangs it off that button.
pub fn open_from_more(st: &State, pane: &PaneId) -> bool {
    st.menu
        .as_ref()
        .is_some_and(|open| open.from_more && &open.pane == pane)
}

/// The pane's menu hanging off another button than the ⚡ one: the pane
/// menu's « … », when the header folded the ⚡ away.
pub fn menu_under<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    anchor: impl Into<Element<'a, Message>>,
    pane: &'a PaneId,
) -> Element<'a, Message> {
    menu::dropdown(ctx, anchor, pane, open_from_more(st, pane))
}

/// `Overlay::CommandDialog`: a saved command, new or edited.
pub fn dialog_view<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    ws: &'a WsId,
    pane: &'a PaneId,
    edit: Option<&'a str>,
) -> Element<'a, Message> {
    let _ = (ws, pane, edit);
    dialog::view(st.dialog.as_ref(), ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(text: &str, auto_run: bool) -> SavedCommand {
        SavedCommand {
            id: "c".into(),
            command: text.into(),
            auto_run,
            ..SavedCommand::default()
        }
    }

    #[test]
    fn a_command_that_runs_itself_ends_with_a_carriage_return() {
        assert_eq!(
            delivery(&command("pnpm test", true), None),
            Delivery::Type("pnpm test\r".into())
        );
        assert_eq!(
            delivery(&command("git commit -m ", false), None),
            Delivery::Type("git commit -m ".into())
        );
    }

    #[test]
    fn a_busy_bridged_agent_gets_it_queued_and_a_free_one_typed() {
        let run = command("pnpm test", true);
        for busy in [AgentActivity::Working, AgentActivity::Waiting] {
            assert_eq!(
                delivery(&run, Some((busy, true))),
                Delivery::Queue("pnpm test".into())
            );
            // Without the mod there is nobody to hold it.
            assert_eq!(
                delivery(&run, Some((busy, false))),
                Delivery::Type("pnpm test\r".into())
            );
        }
        assert_eq!(
            delivery(&run, Some((AgentActivity::Yours, true))),
            Delivery::Type("pnpm test\r".into())
        );
        // One that waits for the person is typed whatever the agent does.
        assert_eq!(
            delivery(&command("ls ", false), Some((AgentActivity::Working, true))),
            Delivery::Type("ls ".into())
        );
    }

    #[test]
    fn a_pane_offers_its_workspace_commands_then_the_global_ones() {
        let ws = Workspace {
            saved_commands: vec![SavedCommand {
                id: "w".into(),
                ..SavedCommand::default()
            }],
            ..Workspace::default()
        };
        let settings = Settings {
            saved_commands: vec![SavedCommand {
                id: "g".into(),
                ..SavedCommand::default()
            }],
            ..Settings::default()
        };
        let ids: Vec<&str> = merged(&ws, &settings)
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(ids, ["w", "g"]);
        assert_eq!(
            locate(&ws, &settings, "g").map(|(scope, _)| scope),
            Some(CommandScope::Global)
        );
        assert_eq!(
            locate(&ws, &settings, "w").map(|(scope, _)| scope),
            Some(CommandScope::Workspace)
        );
        assert!(locate(&ws, &settings, "x").is_none());
    }

    /// A model with a workspace `w` holding saved commands `w1`, `w2` and
    /// global ones `g1`, `g2`; the pane `p` is in `w`.
    fn model_with_commands() -> Model {
        use crate::state::types::{AgentId, AppState, Pane};
        let command = |id: &str| SavedCommand {
            id: id.into(),
            command: format!("run {id}"),
            ..SavedCommand::default()
        };
        let mut state = AppState {
            workspaces: vec![Workspace {
                id: "w".into(),
                saved_commands: vec![command("w1"), command("w2")],
                panes: vec![Pane {
                    id: "p".into(),
                    agent: AgentId::Shell,
                    ..Pane::default()
                }],
                ..Workspace::default()
            }],
            active_workspace_id: Some("w".into()),
            ..AppState::default()
        };
        state.settings.saved_commands = vec![command("g1"), command("g2")];
        let mut model = Model::default();
        model.hydrate(state);
        model
    }

    #[test]
    fn a_command_picked_in_the_menu_is_run_as_the_intent_of_its_place() {
        let model = model_with_commands();
        let pane = "p".to_string();
        // The workspace's commands come first, then the global ones: the same
        // places `command.runN` counts through.
        for (scope, id, index) in [
            (CommandScope::Workspace, "w1", 0),
            (CommandScope::Workspace, "w2", 1),
            (CommandScope::Global, "g1", 2),
            (CommandScope::Global, "g2", 3),
        ] {
            match run_request(&model, &pane, scope, id) {
                Some(Intent::RunSavedCommand {
                    ws,
                    from,
                    index: at,
                }) => {
                    assert_eq!((ws.as_str(), from.as_str(), at), ("w", "p", index));
                    let ws = model.workspace("w").unwrap();
                    assert_eq!(merged(ws, model.settings())[index].id, id);
                }
                other => panic!("expected a run of {id}, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_command_or_a_pane_that_is_gone_runs_nothing() {
        let model = model_with_commands();
        let pane = "p".to_string();
        assert!(run_request(&model, &pane, CommandScope::Workspace, "g1").is_none());
        assert!(run_request(&model, &pane, CommandScope::Global, "w1").is_none());
        assert!(run_request(&model, &pane, CommandScope::Global, "nope").is_none());
        let ghost = "ghost".to_string();
        assert!(run_request(&model, &ghost, CommandScope::Global, "g1").is_none());
    }
}
