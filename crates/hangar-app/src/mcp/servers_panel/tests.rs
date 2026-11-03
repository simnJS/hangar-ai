//! The footer and the restart, against the pane read-model (P4's `Live`),
//! which is where a launch records what it started Claude Code with and where
//! a turn shows.

use hangar_core::mcp_scope::{Scope, Transport};

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{ctx, model_of, pane, simulate, workspace};
use crate::state::live::{AgentActivity, PaneMcp as Launched, PaneMcpMode};
use crate::state::types::AppState;

fn server(name: &str, hangar: bool) -> ServerInfo {
    ServerInfo {
        name: name.into(),
        scope: Scope::User,
        transport: Transport::Stdio,
        summary: name.into(),
        status: Status::Active,
        hangar,
        source: "~/.claude.json".into(),
    }
}

fn servers() -> Vec<ServerInfo> {
    vec![server("hangar", true), server("robloxstudio", false)]
}

/// Workspace `w` has chosen only `robloxstudio`; it holds the Claude Code
/// panes `a`, `b` and `c`, a shell and a Codex.
fn model() -> Model {
    let mut ws = workspace(
        "w",
        "Studio",
        None,
        vec![
            pane("a", "Ava", AgentId::Claude),
            pane("b", "Bo", AgentId::Claude),
            pane("c", "Cy", AgentId::Claude),
            pane("s", "Sam", AgentId::Shell),
            pane("x", "Xi", AgentId::Codex),
        ],
    );
    ws.mcp = Some(WorkspaceMcp {
        mode: McpMode::Custom,
        enabled: vec!["robloxstudio".into()],
    });
    model_of(AppState {
        workspaces: vec![ws],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    })
}

/// What the workspace's choice puts in a launch's file.
fn matching() -> Launched {
    Launched {
        mode: PaneMcpMode::Custom {
            servers: vec!["hangar".into(), "robloxstudio".into()],
        },
        hangar: Some(true),
    }
}

fn inherited() -> Launched {
    Launched {
        mode: PaneMcpMode::Inherit,
        hangar: Some(false),
    }
}

fn launch(m: &mut Model, pane: &str, mcp: Launched) {
    m.live.panes_logic.set_launch_mcp(pane, Some(mcp));
}

fn work(m: &mut Model, pane: &str, activity: AgentActivity) {
    m.live.panes_logic.seed_activity(pane, "w", activity);
}

fn plan(m: &Model) -> RestartPlan {
    let ws = m.workspace("w").unwrap();
    restart_plan(m, ws, &workspace_mcp(Some(ws)), &servers())
}

/// The panes the restart button asks the shell to restart, if it asks.
fn restarted(m: &Model) -> Option<Vec<PaneId>> {
    match restart_request(m, &"w".to_string(), &servers())? {
        Intent::RestartPanes { ws, panes } => {
            assert_eq!(ws, "w");
            Some(panes)
        }
        other => panic!("expected a restart, got {other:?}"),
    }
}

fn ids(names: &[&str]) -> Vec<PaneId> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_launch_is_read_back_from_the_pane_read_model() {
    let mut m = model();
    let a = m.workspace("w").unwrap().panes[0].clone();
    assert_eq!(running_mcp(&m, &a), None, "nothing launched yet");
    launch(&mut m, "a", matching());
    assert_eq!(
        running_mcp(&m, &a),
        Some(PaneMcp {
            mode: McpMode::Custom,
            servers: ids(&["hangar", "robloxstudio"]),
            hangar: Some(true),
        })
    );
}

#[test]
fn every_agent_that_launched_with_the_choice_is_up_to_date() {
    let mut m = model();
    for id in ["a", "b", "c"] {
        launch(&mut m, id, matching());
    }
    let plan = plan(&m);
    assert!(plan.up_to_date());
    assert!(plan.now.is_empty() && plan.held.is_empty());
    assert_eq!(restarted(&m), None);
}

#[test]
fn only_the_panes_running_something_else_are_in_the_restart() {
    let mut m = model();
    launch(&mut m, "a", matching());
    launch(&mut m, "b", inherited());
    // `c` never said what it launched with, and a shell and a Codex have no
    // Claude Code servers to be out of date.
    let plan = plan(&m);
    assert_eq!(plan.now, ids(&["b", "c"]));
    assert!(!plan.up_to_date());
    assert_eq!(restarted(&m), Some(ids(&["b", "c"])));
}

#[test]
fn a_pane_in_the_middle_of_a_turn_is_never_restarted() {
    let mut m = model();
    launch(&mut m, "a", matching());
    // `b` works, `c` waits for an answer: both are inside a turn.
    work(&mut m, "b", AgentActivity::Working);
    work(&mut m, "c", AgentActivity::Waiting);
    let plan = plan(&m);
    assert!(plan.now.is_empty(), "{plan:?}");
    assert_eq!(plan.held, ids(&["b", "c"]));
    // Something out of date is not "up to date", whether or not the button
    // may take it.
    assert!(!plan.up_to_date());
    assert_eq!(restarted(&m), None);
}

#[test]
fn the_restart_takes_the_idle_panes_and_leaves_the_busy_ones() {
    let mut m = model();
    launch(&mut m, "a", matching());
    work(&mut m, "b", AgentActivity::Working);
    work(&mut m, "c", AgentActivity::Yours);
    let plan = plan(&m);
    assert_eq!(plan.now, ids(&["c"]));
    assert_eq!(plan.held, ids(&["b"]));
    assert_eq!(restarted(&m), Some(ids(&["c"])));
}

#[test]
fn a_pane_joins_the_restart_when_its_turn_ends() {
    let mut m = model();
    for id in ["a", "c"] {
        launch(&mut m, id, matching());
    }
    work(&mut m, "b", AgentActivity::Working);
    assert_eq!(plan(&m).held, ids(&["b"]));
    work(&mut m, "b", AgentActivity::Idle);
    let plan = plan(&m);
    assert_eq!(plan.now, ids(&["b"]));
    assert!(plan.held.is_empty());
}

#[test]
fn a_workspace_without_claude_code_has_nothing_to_be_up_to_date() {
    let m = model_of(AppState {
        workspaces: vec![workspace(
            "w",
            "Shells",
            None,
            vec![pane("s", "Sam", AgentId::Shell)],
        )],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    let plan = plan(&m);
    assert_eq!(plan.claude, 0);
    assert!(!plan.up_to_date());
}

/// The panel open on workspace `w`, its servers read.
fn open_panel() -> State {
    State {
        servers: Some(Servers {
            ws: "w".into(),
            found: Found::Ready(Discovery {
                servers: servers(),
                unreadable: Vec::new(),
            }),
        }),
        ..State::default()
    }
}

/// Whether `words` is anywhere in the panel drawn on `m`.
fn draws(m: &Model, words: &str) -> bool {
    let (st, pkg, ws) = (open_panel(), Packages::default(), "w".to_string());
    simulate(view(&st, ctx(m, &pkg), &ws)).find(words).is_ok()
}

/// Presses the footer's button, labelled `label`, and says whether the panel
/// asked for a restart.
fn presses_restart(m: &Model, label: &str) -> bool {
    let (st, pkg, ws) = (open_panel(), Packages::default(), "w".to_string());
    let mut ui = simulate(view(&st, ctx(m, &pkg), &ws));
    // A button with nothing to press has no width to click on.
    let clicked = ui.click(label).is_ok();
    clicked
        && ui
            .into_messages()
            .any(|message| matches!(message, Message::Mcp(Msg::Servers(ServersMsg::Restart))))
}

#[test]
fn the_footer_says_when_every_agent_is_up_to_date() {
    let mut m = model();
    for id in ["a", "b", "c"] {
        launch(&mut m, id, matching());
    }
    assert!(draws(&m, "Every agent already runs with this choice."));
    assert!(draws(&m, "Restart the agents"));
    assert!(!draws(&m, "Applies to agents launched from now on."));
}

#[test]
fn the_footer_counts_the_restart_and_names_the_agents_it_leaves() {
    let mut m = model();
    launch(&mut m, "a", matching());
    // `b` works; `c` is idle and out of date.
    work(&mut m, "b", AgentActivity::Working);
    assert!(draws(&m, "Restart the agent"));
    assert!(draws(
        &m,
        "Applies to agents launched from now on. 1 agent is in the middle of a turn: it keeps its servers until the turn ends."
    ));
    assert!(!draws(&m, "Every agent already runs with this choice."));
}

#[test]
fn the_footer_offers_nothing_while_every_stale_agent_is_in_a_turn() {
    let mut m = model();
    launch(&mut m, "a", matching());
    for id in ["b", "c"] {
        work(&mut m, id, AgentActivity::Working);
    }
    assert!(draws(
        &m,
        "Applies to agents launched from now on. 2 agents are in the middle of a turn: they keep their servers until the turns end."
    ));
    assert!(draws(&m, "Restart the agents"));
    assert!(!draws(&m, "Restart 2 agents"));
}

#[test]
fn the_button_is_live_for_an_idle_stale_agent_and_dead_for_a_busy_one() {
    let mut m = model();
    launch(&mut m, "a", matching());
    launch(&mut m, "b", matching());
    // `c` is out of date and between turns.
    assert!(presses_restart(&m, "Restart the agent"));

    work(&mut m, "c", AgentActivity::Working);
    assert!(!presses_restart(&m, "Restart the agents"));
}
