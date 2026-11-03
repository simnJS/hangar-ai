//! Where the person finds a workspace's kit tabs: the command palette, after
//! the view commands (`extraEntries` of App.tsx), and the toolbar's Kits menu
//! (`kitTabs`, `kitsLoading`).

use hangar_i18n::{Locale, Translator};

use super::testing::{entry, explorer, installed, status};
use super::{choices, has_kits};
use crate::keymap::KeymapIndex;
use crate::palette::sources::{Target, items};
use crate::state::Model;
use crate::state::types::{AppState, Workspace, WorkspaceKit};

fn model(kits: Vec<WorkspaceKit>) -> Model {
    let ws = Workspace {
        id: "ws-1".into(),
        name: "Game".into(),
        kits: Some(kits),
        ..Workspace::default()
    };
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![ws],
        active_workspace_id: Some("ws-1".into()),
        ..AppState::default()
    });
    model
}

#[test]
fn the_tabs_of_the_kits_enabled_on_the_workspace_are_its_choices() {
    let mut m = model(vec![
        entry("roblox", &["files.read"]),
        entry("hello-kit", &["panes.read"]),
    ]);
    let ws = m.workspace("ws-1").unwrap().clone();
    assert!(has_kits(&m, &ws));
    assert!(
        choices(&m, &ws).is_empty(),
        "nothing before the kits are read"
    );
    m.live.kits.installed = Some(installed());
    let listed: Vec<(String, String)> = choices(&m, &ws)
        .into_iter()
        .map(|choice| (choice.kit_id, choice.tab.id))
        .collect();
    assert_eq!(
        listed,
        vec![
            ("roblox".to_string(), explorer().id),
            ("hello-kit".to_string(), status().id),
        ]
    );
    assert!(!has_kits(&model(Vec::new()), &Workspace::default()));
}

#[test]
fn the_palette_offers_each_tab_after_the_view_commands_under_kits() {
    let mut m = model(vec![entry("hello-kit", &["panes.read", "notify"])]);
    m.live.kits.installed = Some(installed());
    let window = iced::window::Id::unique();
    for (locale, label, group) in [
        (Locale::En, "Open the kit tab Status", "Kits"),
        (Locale::Fr, "Ouvrir l'onglet de kit Status", "Kits"),
    ] {
        let list = items(
            &m,
            window,
            Translator::from(locale),
            &KeymapIndex::default(),
        );
        let at = list
            .iter()
            .position(|item| {
                item.value
                    == Target::KitTab {
                        ws: "ws-1".into(),
                        kit: "hello-kit".into(),
                        tab: "status".into(),
                    }
            })
            .expect("the kit's tab is listed");
        assert_eq!(list[at].label, label);
        assert_eq!(list[at].group, group);
    }
}

#[test]
fn the_toolbar_lists_them_and_says_while_the_kits_are_read() {
    let m = model(vec![entry("hello-kit", &["panes.read"])]);
    let ws = m.workspace("ws-1").unwrap().clone();
    let (tabs, loading) = crate::navigator::reads::kit_tabs(&m, &ws).expect("a kit is enabled");
    assert!(tabs.is_empty());
    assert!(loading);
    let mut m = m;
    m.live.kits.installed = Some(installed());
    let (tabs, loading) = crate::navigator::reads::kit_tabs(&m, &ws).unwrap();
    assert!(!loading);
    assert_eq!(tabs[0].title, "Status");
    assert_eq!(tabs[0].kit, "hello-kit name");
    assert!(crate::navigator::reads::kit_tabs(&m, &Workspace::default()).is_none());
}
