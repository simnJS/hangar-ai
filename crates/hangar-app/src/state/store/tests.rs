//! store.test.ts, case by case, then the semantics of each action as
//! store.tsx has them (inventory §1.3), which the TS left to its callers'
//! tests.

use super::*;
use crate::state::layout::{leaf_ids, normalize_tree};
use crate::state::types::{PaneRole, WindowBounds};

fn draft(layout: u8, agents: &[AgentId]) -> WorkspaceDraft {
    WorkspaceDraft {
        name: "w".into(),
        cwd: "/p".into(),
        layout,
        agents: agents.to_vec(),
        ..WorkspaceDraft::default()
    }
}

fn model() -> Model {
    let mut model = Model::default();
    model.hydrate(AppState::default());
    model
}

/// A model holding one workspace of `layout` panes; answers its id.
fn with_workspace(model: &mut Model, layout: u8, agents: &[AgentId]) -> WsId {
    model
        .apply(StoreAction::AddWorkspace(draft(layout, agents)))
        .workspace
        .unwrap()
}

fn ws<'a>(model: &'a Model, id: &str) -> &'a Workspace {
    model.workspace(id).unwrap()
}

fn pane_ids(model: &Model, id: &str) -> Vec<PaneId> {
    ws(model, id).panes.iter().map(|p| p.id.clone()).collect()
}

fn tree_ids(model: &Model, id: &str) -> Vec<PaneId> {
    let w = ws(model, id);
    leaf_ids(&normalize_tree(&w.panes, w.tree.as_ref()))
}

// store.test.ts: makeWorkspace and the Claude account

#[test]
fn keeps_the_account_the_dialog_chose() {
    let mut d = draft(1, &[AgentId::Claude]);
    d.claude_account = Some("travail".into());
    assert_eq!(
        make_workspace(&d, Vec::<String>::new())
            .claude_account
            .as_deref(),
        Some("travail")
    );
}

#[test]
fn leaves_the_field_out_when_the_workspace_follows_the_default() {
    let absent = |account: Option<&str>| {
        let mut d = draft(1, &[AgentId::Claude]);
        d.claude_account = account.map(Into::into);
        let ws = make_workspace(&d, Vec::<String>::new());
        serde_json::to_value(&ws)
            .unwrap()
            .get("claudeAccount")
            .is_none()
    };
    assert!(absent(None));
    assert!(absent(Some("  ")));
    assert!(absent(Some("")));
}

// makeWorkspace

#[test]
fn makes_the_layout_s_panes_with_their_agents_named_against_every_workspace() {
    let ws = make_workspace(
        &draft(3, &[AgentId::Claude, AgentId::Claude]),
        ["Ava", "Leo"],
    );
    let names: Vec<&str> = ws.panes.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Max", "Mia", "Sam"]);
    let agents: Vec<AgentId> = ws.panes.iter().map(|p| p.agent).collect();
    assert_eq!(agents, [AgentId::Claude, AgentId::Claude, AgentId::Shell]);
    assert_eq!(
        ws.tree,
        Some(preset_tree(
            &ws.panes.iter().map(|p| p.id.clone()).collect::<Vec<_>>()
        ))
    );
    assert_eq!(ws.theme_id, None);
    assert!(ws.saved_commands.is_empty());
}

#[test]
fn writes_kits_only_when_some_were_chosen() {
    let mut d = draft(1, &[]);
    d.kits = Some(vec![]);
    let json = serde_json::to_value(make_workspace(&d, Vec::<String>::new())).unwrap();
    assert!(json.get("kits").is_none());
    d.kits = Some(vec![WorkspaceKit {
        id: "k".into(),
        granted: vec![],
    }]);
    assert_eq!(
        make_workspace(&d, Vec::<String>::new()).kits.unwrap().len(),
        1
    );
}

// workspaces

#[test]
fn a_new_workspace_becomes_the_active_one() {
    let mut m = model();
    let first = with_workspace(&mut m, 1, &[]);
    let second = with_workspace(&mut m, 2, &[]);
    assert_eq!(
        m.state().active_workspace_id.as_deref(),
        Some(second.as_str())
    );
    assert_ne!(first, second);
    // Names are unique across workspaces.
    let names: Vec<&str> = m
        .state()
        .workspaces
        .iter()
        .flat_map(|w| &w.panes)
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["Ava", "Max", "Leo"]);
}

#[test]
fn removing_the_active_workspace_activates_the_first_one_left() {
    let mut m = model();
    let a = with_workspace(&mut m, 1, &[]);
    let b = with_workspace(&mut m, 1, &[]);
    let c = with_workspace(&mut m, 1, &[]);
    m.apply(StoreAction::SetActiveWorkspace(Some(b.clone())));
    m.apply(StoreAction::RemoveWorkspace(c.clone()));
    assert_eq!(m.state().active_workspace_id.as_deref(), Some(b.as_str()));
    m.apply(StoreAction::RemoveWorkspace(b));
    assert_eq!(m.state().active_workspace_id.as_deref(), Some(a.as_str()));
    m.apply(StoreAction::RemoveWorkspace(a));
    assert_eq!(m.state().active_workspace_id, None);
}

#[test]
fn an_update_is_a_shallow_merge_and_a_no_op_changes_nothing() {
    let mut m = model();
    let id = with_workspace(&mut m, 1, &[]);
    let revision = m.revision();
    let outcome = m.apply(StoreAction::UpdateWorkspace(
        id.clone(),
        Patch::new(|w: &mut Workspace| {
            w.detached = Some(true);
            w.window_bounds = Some(WindowBounds {
                x: 1.0,
                y: 2.0,
                width: 800.0,
                height: 600.0,
            });
        }),
    ));
    assert!(outcome.changed);
    assert_eq!(m.revision(), revision + 1);
    assert_eq!(ws(&m, &id).detached, Some(true));
    assert_eq!(ws(&m, &id).panes.len(), 1);
    let again = m.apply(StoreAction::UpdateWorkspace(
        id.clone(),
        Patch::new(|w: &mut Workspace| w.detached = Some(true)),
    ));
    assert!(!again.changed);
    assert_eq!(m.revision(), revision + 1);
    assert!(
        !m.apply(StoreAction::UpdateWorkspace("nope".into(), Patch::none()))
            .changed
    );
}

// folders

#[test]
fn folders_are_added_expanded_renamed_moved_and_removed() {
    let mut m = model();
    let a = m.apply(StoreAction::AddFolder("A".into())).folder.unwrap();
    let b = m.apply(StoreAction::AddFolder("B".into())).folder.unwrap();
    assert!(!m.state().folders[0].collapsed);
    m.apply(StoreAction::UpdateFolder(
        a.clone(),
        Patch::new(|f: &mut WorkspaceFolder| {
            f.name = "Work".into();
            f.id = "hijacked".into();
        }),
    ));
    assert_eq!(m.state().folders[0].name, "Work");
    assert_eq!(m.state().folders[0].id, a, "an update never changes the id");
    m.apply(StoreAction::MoveFolder {
        id: b.clone(),
        before: Some(a.clone()),
    });
    let order: Vec<&str> = m.state().folders.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(order, [b.as_str(), a.as_str()]);
    assert!(
        !m.apply(StoreAction::MoveFolder {
            id: b.clone(),
            before: Some(b.clone()),
        })
        .changed
    );

    let w = with_workspace(&mut m, 1, &[]);
    m.apply(StoreAction::MoveWorkspace {
        id: w.clone(),
        folder: Some(a.clone()),
        before: None,
    });
    assert_eq!(ws(&m, &w).folder_id.as_deref(), Some(a.as_str()));
    // Removing a folder sends its workspaces back to the top level.
    m.apply(StoreAction::RemoveFolder(a.clone()));
    assert_eq!(m.state().folders.len(), 1);
    assert_eq!(ws(&m, &w).folder_id, None);
    assert_eq!(m.state().workspaces.len(), 1);
}

#[test]
fn a_workspace_moved_into_a_folder_deleted_meanwhile_lands_at_the_top_level() {
    let mut m = model();
    let a = with_workspace(&mut m, 1, &[]);
    let b = with_workspace(&mut m, 1, &[]);
    m.apply(StoreAction::MoveWorkspace {
        id: b.clone(),
        folder: Some("gone".into()),
        before: Some(a.clone()),
    });
    let order: Vec<&str> = m.state().workspaces.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(order, [b.as_str(), a.as_str()]);
    assert_eq!(ws(&m, &b).folder_id, None);
}

// presets and tidying

#[test]
fn a_growing_preset_copies_the_agent_of_the_pane_the_person_is_in() {
    let mut m = model();
    let w = with_workspace(&mut m, 2, &[AgentId::Shell, AgentId::Claude]);
    let ids = pane_ids(&m, &w);
    m.apply(StoreAction::ApplyPreset {
        ws: w.clone(),
        count: 4,
        model_pane: Some(ids[1].clone()),
    });
    let panes = &ws(&m, &w).panes;
    assert_eq!(panes.len(), 4);
    assert_eq!(&panes[0].id, &ids[0], "the panes kept keep their ids");
    assert_eq!(panes[2].agent, AgentId::Claude);
    assert_eq!(panes[3].agent, AgentId::Claude);
    assert_eq!(tree_ids(&m, &w), pane_ids(&m, &w));
}

#[test]
fn without_a_focused_pane_a_preset_copies_the_last_pane_kept() {
    let mut m = model();
    let w = with_workspace(&mut m, 1, &[AgentId::Claude]);
    m.apply(StoreAction::ApplyPreset {
        ws: w.clone(),
        count: 2,
        model_pane: None,
    });
    assert_eq!(ws(&m, &w).panes[1].agent, AgentId::Claude);
}

#[test]
fn a_shrinking_preset_closes_the_panes_read_last() {
    let mut m = model();
    let w = with_workspace(&mut m, 3, &[]);
    let ids = pane_ids(&m, &w);
    // The third pane dragged to the front: it is read first now.
    m.apply(StoreAction::MovePane {
        ws: w.clone(),
        drag: ids[2].clone(),
        target: ids[0].clone(),
        zone: DropZone::Center,
    });
    m.apply(StoreAction::ApplyPreset {
        ws: w.clone(),
        count: 1,
        model_pane: None,
    });
    assert_eq!(pane_ids(&m, &w), [ids[2].clone()]);
}

#[test]
fn tidying_keeps_the_ids_and_their_reading_order() {
    let mut m = model();
    let w = with_workspace(&mut m, 3, &[]);
    let ids = pane_ids(&m, &w);
    m.apply(StoreAction::MovePane {
        ws: w.clone(),
        drag: ids[0].clone(),
        target: ids[2].clone(),
        zone: DropZone::Bottom,
    });
    let order = tree_ids(&m, &w);
    m.apply(StoreAction::TidyLayout(w.clone()));
    assert_eq!(tree_ids(&m, &w), order);
    assert_eq!(ws(&m, &w).tree, Some(preset_tree(&order)));
}

// panes

#[test]
fn a_new_pane_copies_the_agent_of_the_pane_it_splits() {
    let mut m = model();
    let w = with_workspace(&mut m, 2, &[AgentId::Shell, AgentId::Claude]);
    let ids = pane_ids(&m, &w);
    let new = m
        .apply(StoreAction::AddPane {
            ws: w.clone(),
            agent: None,
            near: Some(ids[1].clone()),
            dir: Some(SplitDir::Col),
            cwd: Some("/elsewhere".into()),
            name: None,
            init: None,
        })
        .pane
        .unwrap();
    let pane = ws(&m, &w)
        .panes
        .iter()
        .find(|p| p.id == new)
        .unwrap()
        .clone();
    assert_eq!(pane.agent, AgentId::Claude);
    assert_eq!(pane.cwd.as_deref(), Some("/elsewhere"));
    assert_eq!(pane.name, "Leo");
    // Below the pane it split.
    assert_eq!(tree_ids(&m, &w), [ids[0].clone(), ids[1].clone(), new]);
}

#[test]
fn an_explicit_agent_wins_and_a_free_name_is_used() {
    let mut m = model();
    let w = with_workspace(&mut m, 1, &[AgentId::Claude]);
    let new = m
        .apply(StoreAction::AddPane {
            ws: w.clone(),
            agent: Some(AgentId::Shell),
            near: None,
            dir: None,
            cwd: None,
            name: Some("  Zed ".into()),
            init: Some(Patch::new(|p: &mut Pane| {
                p.role = Some(PaneRole::Worker);
                p.id = "hijacked".into();
            })),
        })
        .pane
        .unwrap();
    let pane = ws(&m, &w).panes.last().unwrap().clone();
    assert_eq!(pane.id, new, "init never changes the id");
    assert_eq!(pane.agent, AgentId::Shell);
    assert_eq!(pane.name, "Zed");
    assert_eq!(pane.role, Some(PaneRole::Worker));
    // A name already taken is not.
    let taken = m
        .apply(StoreAction::AddPane {
            ws: w.clone(),
            agent: None,
            near: None,
            dir: None,
            cwd: None,
            name: Some("Ava".into()),
            init: None,
        })
        .pane
        .unwrap();
    let pane = ws(&m, &w).panes.iter().find(|p| p.id == taken).unwrap();
    assert_eq!(pane.name, "Max");
}

#[test]
fn a_full_workspace_takes_no_new_pane_and_answers_no_id() {
    let mut m = model();
    let w = with_workspace(&mut m, 16, &[]);
    let outcome = m.apply(StoreAction::AddPane {
        ws: w.clone(),
        agent: None,
        near: None,
        dir: None,
        cwd: None,
        name: None,
        init: None,
    });
    assert_eq!(outcome.pane, None);
    assert!(!outcome.changed);
    assert_eq!(ws(&m, &w).panes.len(), 16);
    assert_eq!(
        m.apply(StoreAction::AddPane {
            ws: "nope".into(),
            agent: None,
            near: None,
            dir: None,
            cwd: None,
            name: None,
            init: None,
        })
        .pane,
        None
    );
}

#[test]
fn the_last_pane_stays_and_a_closed_pane_s_space_goes_to_its_sibling() {
    let mut m = model();
    let w = with_workspace(&mut m, 2, &[]);
    let ids = pane_ids(&m, &w);
    m.apply(StoreAction::ClosePane {
        ws: w.clone(),
        pane: ids[0].clone(),
    });
    assert_eq!(pane_ids(&m, &w), [ids[1].clone()]);
    assert_eq!(ws(&m, &w).tree, Some(leaf(ids[1].clone())));
    let outcome = m.apply(StoreAction::ClosePane {
        ws: w.clone(),
        pane: ids[1].clone(),
    });
    assert!(!outcome.changed);
    assert_eq!(pane_ids(&m, &w).len(), 1);
}

#[test]
fn a_pane_dropped_on_itself_moves_nothing() {
    let mut m = model();
    let w = with_workspace(&mut m, 2, &[]);
    let ids = pane_ids(&m, &w);
    let outcome = m.apply(StoreAction::MovePane {
        ws: w.clone(),
        drag: ids[0].clone(),
        target: ids[0].clone(),
        zone: DropZone::Left,
    });
    assert!(!outcome.changed);
    m.apply(StoreAction::MovePane {
        ws: w.clone(),
        drag: ids[0].clone(),
        target: ids[1].clone(),
        zone: DropZone::Center,
    });
    assert_eq!(tree_ids(&m, &w), [ids[1].clone(), ids[0].clone()]);
}

#[test]
fn a_respawn_gives_a_new_id_in_the_same_place_and_is_announced() {
    let mut m = model();
    let w = with_workspace(&mut m, 2, &[AgentId::Claude, AgentId::Claude]);
    let ids = pane_ids(&m, &w);
    m.apply(StoreAction::UpdatePane {
        ws: w.clone(),
        pane: ids[0].clone(),
        patch: Patch::new(|p: &mut Pane| p.session_id = Some("s1".into())),
    });
    let name = ws(&m, &w).panes[0].name.clone();
    let new = m
        .apply(StoreAction::RespawnPane {
            ws: w.clone(),
            pane: ids[0].clone(),
            patch: Patch::none(),
        })
        .pane
        .unwrap();
    assert_ne!(new, ids[0]);
    let pane = &ws(&m, &w).panes[0];
    assert_eq!(pane.id, new);
    assert_eq!(pane.name, name);
    assert_eq!(pane.session_id.as_deref(), Some("s1"));
    assert_eq!(tree_ids(&m, &w), [new.clone(), ids[1].clone()]);
    // A reset: a fresh conversation.
    let fresh = m
        .apply(StoreAction::RespawnPane {
            ws: w.clone(),
            pane: new.clone(),
            patch: Patch::new(|p: &mut Pane| p.session_id = None),
        })
        .pane
        .unwrap();
    assert_eq!(
        m.take_respawns(),
        [
            Respawn {
                ws: w.clone(),
                from: ids[0].clone(),
                to: new.clone(),
                keeps_session: true,
            },
            Respawn {
                ws: w.clone(),
                from: new.clone(),
                to: fresh,
                keeps_session: false,
            },
        ]
    );
    // Gone, or respawned already: nothing to do, no id.
    assert_eq!(
        m.apply(StoreAction::RespawnPane {
            ws: w.clone(),
            pane: ids[0].clone(),
            patch: Patch::none(),
        })
        .pane,
        None
    );
    assert!(m.take_respawns().is_empty());
}

// settings

#[test]
fn resetting_the_settings_keeps_what_the_defaults_do_not_name() {
    let mut m = model();
    m.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| {
            s.font_size = 18.0;
            s.projects_folder = Some("/code".into());
            s.extra.insert("future".into(), serde_json::json!(1));
        },
    )));
    assert_eq!(m.settings().font_size, 18.0);
    m.apply(StoreAction::ResetSettings);
    assert_eq!(m.settings().font_size, 13.0);
    assert_eq!(m.settings().projects_folder.as_deref(), Some("/code"));
    assert_eq!(m.settings().extra["future"], serde_json::json!(1));
}

// saved commands

fn command(label: &str) -> SavedCommandDraft {
    SavedCommandDraft {
        label: label.into(),
        command: format!("echo {label}"),
        ..SavedCommandDraft::default()
    }
}

#[test]
fn saved_commands_live_in_either_scope_and_keep_their_id_when_they_move() {
    let mut m = model();
    let w = with_workspace(&mut m, 1, &[]);
    let local = m
        .apply(StoreAction::AddSavedCommand {
            scope: CommandScope::Workspace,
            ws: w.clone(),
            draft: command("build"),
        })
        .command
        .unwrap();
    let global = m
        .apply(StoreAction::AddSavedCommand {
            scope: CommandScope::Global,
            ws: w.clone(),
            draft: command("test"),
        })
        .command
        .unwrap();
    assert_eq!(ws(&m, &w).saved_commands[0].id, local);
    assert_eq!(m.settings().saved_commands[0].id, global);

    m.apply(StoreAction::UpdateSavedCommand {
        scope: CommandScope::Workspace,
        ws: w.clone(),
        id: local.clone(),
        patch: Patch::new(|c: &mut SavedCommand| {
            c.auto_run = true;
            c.id = "hijacked".into();
        }),
    });
    assert!(ws(&m, &w).saved_commands[0].auto_run);
    assert_eq!(ws(&m, &w).saved_commands[0].id, local);

    m.apply(StoreAction::MoveSavedCommand {
        from: CommandScope::Workspace,
        to: CommandScope::Global,
        ws: w.clone(),
        id: local.clone(),
    });
    assert!(ws(&m, &w).saved_commands.is_empty());
    let ids: Vec<&str> = m
        .settings()
        .saved_commands
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(ids, [global.as_str(), local.as_str()], "it lands last");

    // Nowhere to take it: a workspace that is gone.
    assert!(
        !m.apply(StoreAction::MoveSavedCommand {
            from: CommandScope::Global,
            to: CommandScope::Workspace,
            ws: "gone".into(),
            id: local.clone(),
        })
        .changed
    );

    m.apply(StoreAction::RemoveSavedCommand {
        scope: CommandScope::Global,
        ws: w.clone(),
        id: global,
    });
    assert_eq!(m.settings().saved_commands.len(), 1);
}

// hydration

#[test]
fn hydration_names_every_pane_uniquely_and_drops_dangling_folder_ids() {
    let pane = |id: &str, name: &str| Pane {
        id: id.into(),
        name: name.into(),
        ..Pane::default()
    };
    let state = AppState {
        workspaces: vec![
            Workspace {
                id: "one".into(),
                panes: vec![pane("a", "Ava"), pane("b", "")],
                folder_id: Some("gone".into()),
                ..Workspace::default()
            },
            Workspace {
                id: "two".into(),
                panes: vec![pane("c", "Ava")],
                folder_id: Some("f".into()),
                ..Workspace::default()
            },
        ],
        folders: vec![WorkspaceFolder {
            id: "f".into(),
            ..WorkspaceFolder::default()
        }],
        active_workspace_id: Some("two".into()),
        ..AppState::default()
    };
    let mut m = Model::default();
    m.hydrate(state);
    assert!(m.live.hydrated);
    assert_eq!(m.revision(), 0, "hydrating is not a change to save");
    let names: Vec<&str> = m
        .state()
        .workspaces
        .iter()
        .flat_map(|w| &w.panes)
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["Ava", "Max", "Leo"]);
    assert_eq!(m.workspace("one").unwrap().folder_id, None);
    assert_eq!(m.workspace("two").unwrap().folder_id.as_deref(), Some("f"));
    assert_eq!(m.ui.opened_ids, ["two"]);
}
