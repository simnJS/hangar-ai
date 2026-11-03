//! What the chrome reads from the other packages' read-models: the shortcut
//! of a command and the tabs of a workspace's kits, each for the window
//! that asks.

use std::collections::BTreeMap;

use hangar_core::kits::{KitInfo, Manifest, Source, Tab};

use super::*;
use crate::app::Packages;
use crate::navigator::testing::{ctx, model_of, pane, workspace};
use crate::state::types::{AgentId, AppState, WorkspaceKit};

fn model() -> Model {
    model_of(AppState {
        workspaces: vec![
            workspace("a", "alpha", None, vec![pane("a1", "Ava", AgentId::Claude)]),
            workspace("b", "beta", None, vec![pane("b1", "Bo", AgentId::Shell)]),
        ],
        active_workspace_id: Some("a".into()),
        ..AppState::default()
    })
}

// ---- Activity ----------------------------------------------------------------------------------

#[test]
fn a_pane_that_reports_nothing_is_idle_and_a_workspace_of_them_counts_none() {
    let m = model();
    let ws = m.workspace("a").unwrap();
    assert_eq!(activity(&m, &"a1".to_string()), AgentActivity::Idle);
    assert_eq!(counts(&m, ws), Counts::default());
    assert!(!wants_you(&m, &"a1".to_string()));
    for state in BUSY {
        assert!(agents_in(&m, &[ws], state).is_empty());
    }
    assert_eq!(asleep_at(&m, ws), None);
}

// ---- Shortcuts ---------------------------------------------------------------------------------

#[test]
fn a_commands_shortcut_is_the_keymaps_as_the_settings_write_it() {
    let m = model();
    let pkg = Packages::default();
    let ctx = ctx(&m, &pkg);
    assert_eq!(binding(&ctx, CommandId::ViewPalette), "Mod+Shift+P");
    assert_eq!(binding(&ctx, CommandId::ViewSettings), "Mod+,");
}

#[test]
fn a_command_without_a_shortcut_has_an_empty_one_so_nothing_is_shown() {
    let mut m = model();
    let overrides = match serde_json::json!({ "view.palette": [] }) {
        serde_json::Value::Object(map) => map,
        _ => unreachable!(),
    };
    m.live.keymap.index = std::sync::Arc::new(crate::keymap::KeymapIndex::new(&overrides));
    let pkg = Packages::default();
    let ctx = ctx(&m, &pkg);
    assert_eq!(binding(&ctx, CommandId::ViewPalette), "");
}

#[test]
fn a_rebound_commands_new_key_is_what_the_chrome_shows() {
    let mut m = model();
    let overrides = match serde_json::json!({ "view.palette": ["Mod+Alt+K"] }) {
        serde_json::Value::Object(map) => map,
        _ => unreachable!(),
    };
    m.live.keymap.index = std::sync::Arc::new(crate::keymap::KeymapIndex::new(&overrides));
    let pkg = Packages::default();
    assert_eq!(binding(&ctx(&m, &pkg), CommandId::ViewPalette), "Mod+Alt+K");
}

// ---- The Kits menu -----------------------------------------------------------------------------

fn tab(id: &str, title: &str) -> Tab {
    Tab {
        id: id.into(),
        title: title.into(),
        entry: format!("tabs/{id}/index.html"),
        native: None,
        icon: None,
        permissions: Vec::new(),
        params: BTreeMap::new(),
        singleton: false,
    }
}

/// A linked folder kit named `name`, usable, with these tabs.
fn kit(id: &str, name: &str, tabs: Vec<Tab>) -> KitInfo {
    KitInfo {
        id: id.into(),
        source: Source::Folder {
            path: format!("/kits/{id}"),
        },
        dir: format!("/kits/{id}"),
        installed_version: "1.0.0".into(),
        added_at: 0,
        manifest: Some(Manifest {
            schema: 1,
            id: id.into(),
            name: name.into(),
            version: "1.0.0".into(),
            description: String::new(),
            author: None,
            homepage: None,
            icon: None,
            hangar: None,
            plugin: None,
            tabs,
            setup: Vec::new(),
            commands: BTreeMap::new(),
        }),
        error: None,
        update: None,
        hooks_permission: None,
        hooks_consent: false,
        commands_permission: None,
    }
}

fn enable(m: &mut Model, ws: &str, kits: &[&str]) {
    let mut state = m.state().clone();
    for entry in &mut state.workspaces {
        if entry.id == ws {
            entry.kits = Some(
                kits.iter()
                    .map(|id| WorkspaceKit {
                        id: (*id).into(),
                        granted: Vec::new(),
                    })
                    .collect(),
            );
        }
    }
    m.hydrate(state);
}

fn titles(tabs: &[KitTab]) -> Vec<String> {
    tabs.iter()
        .map(|tab| format!("{}/{}", tab.kit, tab.title))
        .collect()
}

#[test]
fn a_workspace_with_no_kit_has_no_kits_button() {
    let mut m = model();
    m.live.kits.installed = Some(vec![kit("deploy", "Deploy", vec![tab("runs", "Runs")])]);
    assert_eq!(kit_tabs(&m, m.workspace("a").unwrap()), None);
}

#[test]
fn while_the_installed_kits_are_being_read_the_menu_says_so() {
    let mut m = model();
    enable(&mut m, "a", &["deploy"]);
    let (tabs, loading) = kit_tabs(&m, m.workspace("a").unwrap()).expect("a kit is enabled");
    assert!(tabs.is_empty());
    assert!(loading);
}

#[test]
fn the_menu_lists_the_tabs_of_the_enabled_kits_in_the_order_they_were_enabled() {
    let mut m = model();
    m.live.kits.installed = Some(vec![
        kit(
            "deploy",
            "Deploy",
            vec![tab("runs", "Runs"), tab("logs", "Logs")],
        ),
        kit("notes", "Notes", vec![tab("all", "All notes")]),
        kit("unused", "Unused", vec![tab("x", "X")]),
    ]);
    enable(&mut m, "a", &["notes", "deploy"]);
    let (tabs, loading) = kit_tabs(&m, m.workspace("a").unwrap()).unwrap();
    assert!(!loading);
    assert_eq!(
        titles(&tabs),
        ["Notes/All notes", "Deploy/Runs", "Deploy/Logs"]
    );
    assert_eq!(
        (tabs[0].kit_id.as_str(), tabs[0].tab_id.as_str()),
        ("notes", "all")
    );
}

#[test]
fn a_kit_that_is_gone_or_cannot_be_used_lists_no_tab_but_keeps_the_button() {
    let mut m = model();
    let mut broken = kit("broken", "Broken", vec![tab("t", "T")]);
    broken.error = Some("needs a newer Hangar".into());
    let mut renamed = kit("renamed", "Renamed", vec![tab("t", "T")]);
    renamed.manifest.as_mut().unwrap().id = "another".into();
    m.live.kits.installed = Some(vec![broken, renamed]);
    enable(&mut m, "a", &["broken", "renamed", "gone"]);
    let (tabs, loading) = kit_tabs(&m, m.workspace("a").unwrap()).unwrap();
    assert!(tabs.is_empty());
    assert!(!loading);
}

#[test]
fn a_kit_enabled_everywhere_is_offered_by_every_workspace() {
    let mut m = model();
    m.live.kits.installed = Some(vec![kit("deploy", "Deploy", vec![tab("runs", "Runs")])]);
    let mut state = m.state().clone();
    state.settings.kits_everywhere = Some(vec![WorkspaceKit {
        id: "deploy".into(),
        granted: Vec::new(),
    }]);
    m.hydrate(state);
    for ws in ["a", "b"] {
        let (tabs, _) = kit_tabs(&m, m.workspace(ws).unwrap()).unwrap();
        assert_eq!(titles(&tabs), ["Deploy/Runs"]);
    }
}
