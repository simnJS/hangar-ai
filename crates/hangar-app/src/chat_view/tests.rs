//! The chat view headless (`ICED_TEST_BACKEND=tiny-skia`): the switch and
//! what it writes, the three regions, a kit tab in the tools area, and the
//! grid back as it was.

use iced::window;

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{ctx, model_of, pane, simulate, workspace};
use crate::preview::store::KitTabToOpen;
use crate::state::types::{AgentId, AppState};
use crate::state::{Patch, Settings, read_state};

/// A workspace of two terminals, Ava and Leo, and a kit's tab beside them
/// as a view pane, its entry in the process's preview store under a
/// workspace id no other test uses; the panes mounted, with the experiment
/// on or off.
fn shop(id: &str, experiment: bool) -> (Model, Packages, String) {
    let ws = id.to_string();
    let mut m = model_of(AppState {
        workspaces: vec![workspace(
            &ws,
            "Shop",
            None,
            vec![
                pane("ava", "Ava", AgentId::Shell),
                pane("leo", "Leo", AgentId::Shell),
            ],
        )],
        active_workspace_id: Some(ws.clone()),
        ..AppState::default()
    });
    if experiment {
        m.apply(StoreAction::UpdateSettings(Patch::new(
            |s: &mut Settings| s.experiments = vec![experiments::CHAT_VIEW.into()],
        )));
    }
    let shared = m.live.preview.shared().clone();
    let tab = shared.edit(|store| {
        store.close_workspace_models(&ws);
        store.open_kit_tab(KitTabToOpen::new(ws.clone(), "ci-kit", "runs", "CI runs"))
    });
    m.live.preview.sync();
    crate::preview::panes::sync(&mut m);
    assert!(m.workspace(&ws).unwrap().has_view(&tab));
    let mut pkg = Packages::default();
    pkg.panes.mount_for_test(m.workspace(&ws).unwrap());
    (m, pkg, tab)
}

fn forget(m: &Model, ws: &str) {
    let shared = m.live.preview.shared().clone();
    shared.edit(|store| store.close_workspace_models(ws));
}

fn set(m: &mut Model, ws: &str, on: bool) {
    m.apply(StoreAction::SetChatView { ws: ws.into(), on });
}

/// The texts the main window shows, among `wanted`.
fn on_screen(m: &Model, pkg: &Packages, wanted: &[&str]) -> Vec<String> {
    let ctx = ctx(m, pkg);
    let mut ui = simulate(crate::shell::frame::main(ctx));
    let _ = ui
        .snapshot(&hangar_theme::to_iced(ctx.theme))
        .expect("it draws");
    wanted
        .iter()
        .filter(|text| ui.find(**text).is_ok())
        .map(|text| text.to_string())
        .collect()
}

#[test]
fn the_switch_toggles_and_is_saved_only_while_on() {
    let mut m = model_of(AppState {
        workspaces: vec![workspace("w", "Shop", None, vec![])],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    let saved = |m: &Model| m.to_json()["workspaces"][0].get("chatView").cloned();
    assert_eq!(saved(&m), None, "a workspace that never used it");

    set(&mut m, "w", true);
    assert_eq!(m.workspace("w").unwrap().chat_view, Some(true));
    assert_eq!(saved(&m), Some(serde_json::json!(true)));
    // What was written reads back as it was.
    let again = read_state(m.to_json());
    assert_eq!(again.workspaces[0].chat_view, Some(true));

    set(&mut m, "w", false);
    assert_eq!(m.workspace("w").unwrap().chat_view, None);
    assert_eq!(saved(&m), None, "off is the absence");
    let revision = m.revision();
    set(&mut m, "w", false);
    assert_eq!(m.revision(), revision, "nothing to save");
}

#[test]
fn a_workspace_is_shown_as_the_chat_view_only_while_the_experiment_is_on() {
    let (mut m, _pkg, _) = shop("chat-shown", false);
    set(&mut m, "chat-shown", true);
    assert!(!available(&m));
    assert!(!shown(&m, m.workspace("chat-shown").unwrap()));
    m.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.experiments = vec![experiments::CHAT_VIEW.into()],
    )));
    assert!(available(&m));
    assert!(shown(&m, m.workspace("chat-shown").unwrap()));
    forget(&m, "chat-shown");
}

#[test]
fn the_toolbar_offers_the_switch_only_while_the_experiment_is_on() {
    let (m, pkg, _) = shop("chat-toolbar-off", false);
    let mut ui = simulate(crate::toolbar::view(&pkg.toolbar, ctx(&m, &pkg)));
    assert!(ui.find("Chat").is_err());
    forget(&m, "chat-toolbar-off");

    let (m, pkg, _) = shop("chat-toolbar", true);
    let mut ui = simulate(crate::toolbar::view(&pkg.toolbar, ctx(&m, &pkg)));
    ui.click("Chat").expect("the switch is in the toolbar");
    let asked: Vec<(WsId, bool)> = ui
        .into_messages()
        .filter_map(|message| match message {
            Message::ChatView(Msg::SetShown(ws, on)) => Some((ws, on)),
            _ => None,
        })
        .collect();
    assert_eq!(asked, vec![("chat-toolbar".to_string(), true)]);
    forget(&m, "chat-toolbar");
}

#[test]
fn the_palette_offers_the_other_face_of_the_workspace() {
    use crate::palette::sources::{Target, items};
    let (mut m, _pkg, _) = shop("chat-palette", true);
    let window = m.main_window().unwrap_or_else(window::Id::unique);
    let i18n = hangar_i18n::Translator::new(hangar_i18n::Locale::En);
    let entry = |m: &Model| {
        items(m, window, i18n, &m.live.keymap.index)
            .into_iter()
            .find_map(|item| match item.value {
                Target::ChatView { ws, on } => Some((ws, on)),
                _ => None,
            })
    };
    assert_eq!(entry(&m), Some(("chat-palette".to_string(), true)));
    set(&mut m, "chat-palette", true);
    assert_eq!(entry(&m), Some(("chat-palette".to_string(), false)));
    forget(&m, "chat-palette");

    let (m, _pkg, _) = shop("chat-palette-off", false);
    assert_eq!(entry(&m), None);
    forget(&m, "chat-palette-off");
}

#[test]
fn the_chat_view_draws_the_conversation_the_tasks_and_a_kit_tab_in_the_tools_area() {
    let (mut m, pkg, tab) = shop("chat-regions", true);
    set(&mut m, "chat-regions", true);
    let ws = m.workspace("chat-regions").unwrap();
    let ctx = ctx(&m, &pkg);
    let mut ui = simulate(view(ctx, ws));
    let _ = ui
        .snapshot(&hangar_theme::to_iced(ctx.theme))
        .expect("it draws");
    assert!(
        ui.find("No orchestrator in this workspace").is_ok(),
        "the conversation: its empty state, the workspace has no orchestrator"
    );
    assert!(ui.find("Tasks").is_ok());
    assert!(ui.find("CI runs").is_ok(), "the kit tab, in the tools area");
    // A press in the tab's pane gives it the focus, as in the grid.
    ui.click("CI runs").expect("the tab's header is there");
    let focused: Vec<String> = ui
        .into_messages()
        .filter_map(|message| match message {
            Message::Panes(crate::panes::Msg::Focus(_, pane)) => Some(pane),
            _ => None,
        })
        .collect();
    assert_eq!(focused, vec![tab]);
    forget(&m, "chat-regions");
}

#[test]
fn a_workspace_without_a_view_pane_has_no_tools_area() {
    let mut m = model_of(AppState {
        workspaces: vec![workspace(
            "chat-bare",
            "Shop",
            None,
            vec![pane("ava", "Ava", AgentId::Shell)],
        )],
        active_workspace_id: Some("chat-bare".into()),
        ..AppState::default()
    });
    set(&mut m, "chat-bare", true);
    let pkg = Packages::default();
    let ws = m.workspace("chat-bare").unwrap();
    assert!(crate::panes::view_tools(&pkg.panes, ctx(&m, &pkg), ws, 400.0).is_none());
}

#[test]
fn the_main_window_switches_between_the_grid_and_the_chat_view_and_back_unchanged() {
    let (mut m, pkg, _) = shop("chat-frame", true);
    let wanted = [
        "Ava",
        "Leo",
        "CI runs",
        "No orchestrator in this workspace",
        "Tasks",
    ];
    let grid = m.workspace("chat-frame").unwrap().clone();
    assert_eq!(
        on_screen(&m, &pkg, &wanted),
        ["Ava", "Leo", "CI runs"],
        "the grid: both terminals and the tab"
    );

    set(&mut m, "chat-frame", true);
    assert_eq!(
        on_screen(&m, &pkg, &wanted),
        ["CI runs", "No orchestrator in this workspace", "Tasks"],
        "the chat view: no terminal, the tab in the tools area"
    );
    // The terminals are still mounted: only what draws them changed.
    assert!(pkg.panes.is_mounted("ava") && pkg.panes.is_mounted("leo"));

    set(&mut m, "chat-frame", false);
    assert_eq!(on_screen(&m, &pkg, &wanted), ["Ava", "Leo", "CI runs"]);
    let back = m.workspace("chat-frame").unwrap();
    assert_eq!(back.tree, grid.tree, "the same arrangement");
    assert_eq!(back.panes, grid.panes);
    assert_eq!(back.view_panes, grid.view_panes);
    forget(&m, "chat-frame");
}
