//! The shell's model-side rules (the parts of App.tsx's handlers that need
//! no running app): windows mounting their workspace, a window that never
//! opens, moving between panes, each window's own broadcast text.

use super::*;
use crate::shell::SettingsCategory;
use crate::state::{AppState, WorkspaceDraft};

const TYPICAL: &str = include_str!("../../state/fixtures/typical.json");

/// A hydrated model with its main window, focused.
fn model() -> (Model, window::Id) {
    let mut m = Model::default();
    let main = window::Id::unique();
    m.window_opened(main, WindowKind::Main);
    m.hydrate(AppState::default());
    (m, main)
}

fn add(m: &mut Model, panes: u8) -> (WsId, Vec<PaneId>) {
    let ws = m
        .apply(StoreAction::AddWorkspace(WorkspaceDraft {
            name: "w".into(),
            cwd: "/w".into(),
            layout: panes,
            agents: vec![AgentId::Shell; usize::from(panes)],
            ..WorkspaceDraft::default()
        }))
        .workspace
        .unwrap();
    let ids = m
        .workspace(&ws)
        .unwrap()
        .panes
        .iter()
        .map(|pane| pane.id.clone())
        .collect();
    (ws, ids)
}

/// The panes the panes package starts: every pane of a mounted workspace
/// (`panes::sync`'s `wanted`).
fn started(m: &Model) -> Vec<PaneId> {
    m.ui.opened_ids
        .iter()
        .filter_map(|id| m.workspace(id))
        .flat_map(|ws| ws.panes.iter().map(|pane| pane.id.clone()))
        .collect()
}

/// The audit's gap 3: a workspace detached when the app last quit reopened
/// with an empty grid, its panes never started.
#[test]
fn a_workspace_detached_at_the_last_quit_starts_its_panes_in_its_window() {
    let raw: serde_json::Value = serde_json::from_str(TYPICAL).unwrap();
    let (state, kept) = crate::state::read_checked(raw).unwrap();
    let mut m = Model::default();
    let main = window::Id::unique();
    m.window_opened(main, WindowKind::Main);
    m.hydrate_read(state, kept);

    let detached = m.detached_workspaces();
    let site = m
        .state()
        .workspaces
        .iter()
        .find(|ws| ws.name == "site")
        .unwrap()
        .clone();
    assert_eq!(detached, std::slice::from_ref(&site.id));
    // Not before its window is there: nothing would draw its panes.
    assert!(!m.ui.opened_ids.contains(&site.id));

    // What `open_workspace_window` does with the window it makes.
    let own = window::Id::unique();
    m.window_opened(own, WindowKind::Workspace(site.id.clone()));

    assert!(m.ui.opened_ids.contains(&site.id));
    assert_eq!(m.window_workspace(own).map(|ws| &ws.id), Some(&site.id));
    let pane = &site.panes[0].id;
    assert!(started(&m).contains(pane));
    // The main window still never draws it: it says where it is instead.
    assert!(m.window_workspace(main).is_none_or(|ws| ws.id != site.id));
}

#[test]
fn a_detach_whose_window_never_opens_comes_back_with_a_notice() {
    let (mut m, _) = model();
    let (ws, panes) = add(&mut m, 2);
    let (other, _) = add(&mut m, 1);
    m.apply(StoreAction::UpdateWorkspace(
        ws.clone(),
        Patch::new(|w: &mut Workspace| w.detached = Some(true)),
    ));
    let own = window::Id::unique();
    m.window_opened(own, WindowKind::Workspace(ws.clone()));
    assert_eq!(m.state().active_workspace_id.as_ref(), Some(&other));

    let notice = undo_open(&mut m, own, &ws, windows::Opening::Detach);

    assert_eq!(
        notice.as_deref(),
        Some("Could not open the window: the system never showed it")
    );
    assert!(!m.live.windows.contains_key(&own));
    let back = m.workspace(&ws).unwrap();
    assert_eq!(back.detached, Some(false));
    // Shown in the main window again, its panes still running there.
    assert_eq!(m.state().active_workspace_id.as_ref(), Some(&ws));
    assert!(panes.iter().all(|pane| started(&m).contains(pane)));
}

#[test]
fn a_window_reopened_at_boot_that_never_opens_comes_back_quietly() {
    let (mut m, _) = model();
    let (ws, _) = add(&mut m, 1);
    let (other, _) = add(&mut m, 1);
    m.apply(StoreAction::UpdateWorkspace(
        ws.clone(),
        Patch::new(|w: &mut Workspace| w.detached = Some(true)),
    ));
    let own = window::Id::unique();
    m.window_opened(own, WindowKind::Workspace(ws.clone()));

    assert_eq!(undo_open(&mut m, own, &ws, windows::Opening::Boot), None);
    assert_eq!(m.workspace(&ws).unwrap().detached, Some(false));
    assert_eq!(m.state().active_workspace_id.as_ref(), Some(&other));
}

/// The audit's verification: `pane.focus2` with the settings open keeps the
/// overlay, and the keyboard comes to that pane once it closes.
#[test]
fn selecting_a_pane_under_an_open_overlay_keeps_the_overlay() {
    let (mut m, _) = model();
    let (ws, panes) = add(&mut m, 2);
    m.ui.focus_by_workspace.insert(ws.clone(), panes[0].clone());
    m.ui.focus = Some(Focus::Terminal(panes[0].clone()));
    m.ui.overlays = vec![Overlay::Settings(SettingsCategory::Appearance)];
    m.ui.overlay_openers = vec![Some(Focus::Terminal(panes[0].clone()))];

    select_pane(&mut m, &ws, &panes[1]);

    assert_eq!(
        m.ui.overlays,
        [Overlay::Settings(SettingsCategory::Appearance)]
    );
    assert_eq!(m.ui.focus_by_workspace.get(&ws), Some(&panes[1]));
    // The sheet keeps the keyboard meanwhile.
    assert_eq!(m.ui.focus, Some(Focus::Terminal(panes[0].clone())));
    // Nothing else moved either.
    assert_eq!(m.ui.view, View::Terminals);
    assert_eq!(m.state().active_workspace_id.as_ref(), Some(&ws));
}

#[test]
fn selecting_a_pane_gives_its_terminal_the_keyboard_and_ends_a_drawer() {
    let (mut m, _) = model();
    let (ws, panes) = add(&mut m, 3);
    m.ui.drawer = Some(Drawer::Inspector);
    m.ui.focus = Some(Focus::Region(Region::Inspector));

    select_pane(&mut m, &ws, &panes[2]);

    assert_eq!(m.ui.focus_by_workspace.get(&ws), Some(&panes[2]));
    assert_eq!(m.ui.focus, Some(Focus::Terminal(panes[2].clone())));
    assert_eq!(m.ui.drawer, None);

    // A pane that is gone changes nothing.
    select_pane(&mut m, &ws, "gone");
    assert_eq!(m.ui.focus_by_workspace.get(&ws), Some(&panes[2]));
}

#[test]
fn each_window_keeps_its_own_broadcast_text() {
    let (mut m, main) = model();
    let (ws, _) = add(&mut m, 1);
    let own = window::Id::unique();
    m.window_opened(own, WindowKind::Workspace(ws));
    m.ui.broadcast = "npm test".into();

    swap_window_ui(&mut m, own);
    assert_eq!(m.ui.broadcast, "");
    assert_eq!(m.broadcast_in(own), "");
    assert_eq!(m.broadcast_in(main), "npm test");

    m.ui.broadcast = "git pull".into();
    swap_window_ui(&mut m, main);
    assert_eq!(m.ui.broadcast, "npm test");
    assert_eq!(m.broadcast_in(own), "git pull");
    assert_eq!(m.broadcast_in(main), "npm test");
}

#[test]
fn the_settings_sheet_is_found_wherever_it_is_in_the_stack() {
    assert!(!has_settings(&[Overlay::Palette]));
    assert!(has_settings(&[
        Overlay::Settings(SettingsCategory::Kits),
        Overlay::KitActivation {
            kit: "k".into(),
            ws: None
        }
    ]));
}
