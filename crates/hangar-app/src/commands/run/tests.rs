//! The command table (App.tsx `buildHandlers`) against models in known
//! states.

use super::*;
use crate::state::{AppState, WorkspaceDraft};

use CommandId::*;

/// A hydrated model with its main window, and that window's id.
fn model() -> (Model, window::Id) {
    let mut m = Model::default();
    let main = window::Id::unique();
    m.live.windows.insert(main, WindowKind::Main);
    m.hydrate(AppState::default());
    (m, main)
}

fn add(m: &mut Model, agents: &[AgentId]) -> String {
    let ws = m
        .apply(StoreAction::AddWorkspace(WorkspaceDraft {
            name: "w".into(),
            cwd: "/w".into(),
            layout: agents.len() as u8,
            agents: agents.to_vec(),
            ..WorkspaceDraft::default()
        }))
        .workspace
        .unwrap();
    let first = m.workspace(&ws).unwrap().panes[0].id.clone();
    m.ui.focus_by_workspace.insert(ws.clone(), first);
    ws
}

fn has(m: &Model, w: window::Id, ids: &[CommandId]) -> bool {
    let live = available(m, w);
    ids.iter().all(|id| live.contains(id))
}

fn lacks(m: &Model, w: window::Id, ids: &[CommandId]) -> bool {
    let live = available(m, w);
    ids.iter().all(|id| !live.contains(id))
}

#[test]
fn without_a_workspace_only_the_window_s_own_commands_are_live() {
    let (m, w) = model();
    assert!(has(
        &m,
        w,
        &[
            ViewSettings,
            ViewPalette,
            ViewNavigator,
            ViewFocusNavigator,
            AppZoomIn,
            AppFullscreen
        ]
    ));
    assert!(lacks(
        &m,
        w,
        &[
            PaneSplit,
            ViewBoard,
            ViewFocusInspector,
            WorkspaceGo1,
            VoiceDictate
        ]
    ));
    // In keymap order.
    let live = available(&m, w);
    let mut sorted = live.clone();
    sorted.sort_by_key(|id| CommandId::ALL.iter().position(|c| c == id));
    assert_eq!(live, sorted);
}

#[test]
fn the_pane_commands_need_a_pane_to_act_on_in_the_terminals_view() {
    let (mut m, w) = model();
    let ws = add(&mut m, &[AgentId::Claude, AgentId::Shell]);
    assert!(has(
        &m,
        w,
        &[
            PaneSplit,
            PaneClose,
            PaneRestart,
            PaneRestartAll,
            PaneResetAll,
            PaneTidy,
            PaneZoom,
            PaneSessions,
            PanePinOrchestrator,
            PaneFocus1,
            PaneFocus2,
            PaneNext,
            PaneFocusLeft,
            PaneSwapDown,
            TerminalCopy,
            TerminalFind,
            ViewBroadcast,
            WorkspaceGo1,
            WorkspaceNewOrchestrator,
        ]
    ));
    assert!(lacks(
        &m,
        w,
        &[
            PaneFocus3,
            PaneUnpinOrchestrator,
            WorkspaceStopWorkers,
            WorkspaceNext,
            WorkspaceSleep,
            CommandRun1,
            ViewModelPreview,
            ViewNeedsYou,
        ]
    ));

    // Focused on the shell: nothing to resume, nothing to pin.
    let shell = m.workspace(&ws).unwrap().panes[1].id.clone();
    m.ui.focus_by_workspace.insert(ws.clone(), shell);
    assert!(lacks(&m, w, &[PaneSessions, PanePinOrchestrator]));

    // Off the terminals, no key is about a pane.
    m.ui.view = View::Board;
    assert!(lacks(
        &m,
        w,
        &[PaneSplit, PaneRestart, TerminalCopy, ViewBroadcast]
    ));
    assert!(has(
        &m,
        w,
        &[ViewTerminals, ViewBoardDock, WorkspaceMcpServers]
    ));
}

#[test]
fn a_single_pane_cannot_be_closed_zoomed_or_tidied() {
    let (mut m, w) = model();
    add(&mut m, &[AgentId::Shell]);
    assert!(lacks(
        &m,
        w,
        &[PaneClose, PaneZoom, PaneTidy, PaneNext, PaneResetAll]
    ));
    assert!(has(&m, w, &[PaneRestart, PaneFocus1]));
}

#[test]
fn what_packages_tell_the_shell_turns_their_commands_on() {
    let (mut m, w) = model();
    let ws = add(&mut m, &[AgentId::Shell]);
    m.live.signals.needs_you = 2;
    m.live.signals.preview.insert(
        ws,
        crate::state::PreviewPanel {
            entries: 1,
            ..Default::default()
        },
    );
    m.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.voice_enabled = true,
    )));
    assert!(has(
        &m,
        w,
        &[
            ViewNeedsYou,
            ViewModelPreview,
            ViewCloseModels,
            VoiceDictate
        ]
    ));
}

#[test]
fn saved_commands_count_the_workspace_s_then_the_global_ones() {
    let (mut m, w) = model();
    let ws = add(&mut m, &[AgentId::Shell]);
    for scope in [
        crate::state::CommandScope::Workspace,
        crate::state::CommandScope::Global,
    ] {
        m.apply(StoreAction::AddSavedCommand {
            scope,
            ws: ws.clone(),
            draft: Default::default(),
        });
    }
    assert!(has(&m, w, &[CommandRun1, CommandRun2]));
    assert!(lacks(&m, w, &[CommandRun3]));
    assert!(matches!(
        message(&m, w, CommandRun2),
        Some(Message::Intent(Intent::RunSavedCommand { index: 1, .. }))
    ));
}

#[test]
fn several_workspaces_step_in_navigator_order() {
    let (mut m, w) = model();
    let first = add(&mut m, &[AgentId::Shell]);
    let second = add(&mut m, &[AgentId::Shell]);
    assert!(has(
        &m,
        w,
        &[
            WorkspaceGo1,
            WorkspaceGo2,
            WorkspaceNext,
            WorkspacePrev,
            WorkspaceSleep
        ]
    ));
    // The second is active: next wraps to the first.
    assert!(matches!(
        message(&m, w, WorkspaceNext),
        Some(Message::Intent(Intent::ActivateWorkspace(id))) if id == first
    ));
    assert!(matches!(
        message(&m, w, WorkspaceGo2),
        Some(Message::Intent(Intent::ActivateWorkspace(id))) if id == second
    ));
}

#[test]
fn a_workspace_window_has_no_settings_navigator_or_workspace_list() {
    let (mut m, _) = model();
    let ws = add(&mut m, &[AgentId::Shell, AgentId::Shell]);
    add(&mut m, &[AgentId::Shell]);
    let own = window::Id::unique();
    m.live
        .windows
        .insert(own, WindowKind::Workspace(ws.clone()));
    assert!(lacks(
        &m,
        own,
        &[
            ViewSettings,
            ViewShortcuts,
            WorkspaceNew,
            ViewNavigator,
            ViewFocusNavigator,
            WorkspaceGo1,
            WorkspaceNext
        ]
    ));
    // It acts on its own workspace, not the main window's active one.
    assert!(has(&m, own, &[PaneClose, PaneFocus2]));
    assert!(matches!(
        message(&m, own, PaneTidy),
        Some(Message::Intent(Intent::Tidy(id))) if id == ws
    ));
}

#[test]
fn focus_and_zoom_commands_name_their_pane() {
    let (mut m, w) = model();
    let ws = add(&mut m, &[AgentId::Shell, AgentId::Shell]);
    let panes: Vec<String> = m
        .workspace(&ws)
        .unwrap()
        .panes
        .iter()
        .map(|p| p.id.clone())
        .collect();
    assert!(matches!(
        message(&m, w, PaneFocusRight),
        Some(Message::Intent(Intent::FocusPane { pane, terminal: true, .. })) if pane == panes[1]
    ));
    // Nothing on the left of the first pane: nothing to do.
    assert!(message(&m, w, PaneFocusLeft).is_none());
    assert!(matches!(
        message(&m, w, PanePrev),
        Some(Message::Intent(Intent::FocusPane { pane, .. })) if pane == panes[1]
    ));
    assert!(matches!(
        message(&m, w, PaneZoom),
        Some(Message::Intent(Intent::SetZoom { on: true, .. }))
    ));
    assert!(matches!(
        message(&m, w, TerminalPaste),
        Some(Message::Intent(Intent::TerminalAction {
            action: TerminalAction::Paste,
            ..
        }))
    ));
}

#[test]
fn zooming_the_text_stays_within_the_slider_s_bounds() {
    let (mut m, w) = model();
    m.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.font_size = FONT_MAX,
    )));
    let Some(Message::Store(StoreAction::UpdateSettings(patch))) = message(&m, w, AppZoomIn) else {
        panic!("a settings change expected");
    };
    let mut settings = Settings::default();
    patch.apply(&mut settings);
    assert_eq!(settings.font_size, FONT_MAX);
}

#[test]
fn a_command_that_is_not_live_does_nothing() {
    let (m, w) = model();
    assert!(!is_available(&m, w, PaneSplit));
    // `run` checks first; `message` alone would find no workspace either.
    assert!(message(&m, w, PaneSplit).is_none());
}
