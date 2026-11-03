//! `state.json` read, hydrated and written back (inventory §1.4), against two
//! made-up files shaped like real ones: one as the 1.12 app writes it, one
//! hand-edited and partly written by a newer version.

use serde_json::{Value, json};

use super::types::{AgentId, PaneRole, SplitDir, SplitNode};
use super::{Model, Patch, Settings, StoreAction, Workspace, layout, read_state_keeping};

const TYPICAL: &str = include_str!("fixtures/typical.json");
const HAND_EDITED: &str = include_str!("fixtures/hand_edited.json");

fn load(file: &str) -> (Model, Value) {
    let raw: Value = serde_json::from_str(file).unwrap();
    let (state, kept) = read_state_keeping(raw.clone());
    let mut model = Model::default();
    model.hydrate_read(state, kept);
    (model, raw)
}

/// JSON equality as JavaScript reads it: `14` and `14.0` are one number.
fn same_meaning(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_meaning(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same_meaning(value, other)))
        }
        _ => a == b,
    }
}

/// The first difference between two documents, for a readable failure.
fn difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for key in x.keys().chain(y.keys()) {
                let path = format!("{at}.{key}");
                match (x.get(key), y.get(key)) {
                    (Some(p), Some(q)) => {
                        if let Some(found) = difference(p, q, &path) {
                            return Some(found);
                        }
                    }
                    (p, q) => return Some(format!("{path}: {p:?} vs {q:?}")),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (p, q))| difference(p, q, &format!("{at}[{i}]"))),
        _ if same_meaning(a, b) => None,
        _ => Some(format!("{at}: {a} vs {b}")),
    }
}

#[test]
fn a_file_the_app_wrote_comes_back_as_it_was() {
    let (model, raw) = load(TYPICAL);
    let written = model.to_json();
    assert_eq!(difference(&written, &raw, ""), None);

    let state = model.state();
    assert_eq!(state.workspaces.len(), 3);
    let hangar = &state.workspaces[0];
    assert_eq!(hangar.panes[1].role, Some(PaneRole::Worker));
    assert_eq!(hangar.panes[1].port_base, Some(4100));
    assert_eq!(hangar.claude_account.as_deref(), Some("travail"));
    assert_eq!(state.workspaces[1].detached, Some(true));
    assert_eq!(state.workspaces[2].tree, None);
    assert_eq!(state.settings.locale, Some(hangar_i18n::Locale::Fr));
    assert_eq!(state.settings.keybindings["view.stats"], json!([]));
}

#[test]
fn a_file_the_app_wrote_survives_a_change_with_only_that_change() {
    let (mut model, mut raw) = load(TYPICAL);
    model.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.font_size = 15.0,
    )));
    raw["settings"]["fontSize"] = json!(15);
    assert_eq!(difference(&model.to_json(), &raw, ""), None);
}

#[test]
fn a_hand_edited_file_loads_whole() {
    let (model, _) = load(HAND_EDITED);
    let state = model.state();
    // The string among the workspaces is not one; both others are there.
    assert_eq!(state.workspaces.len(), 2);
    let api = &state.workspaces[0];
    // Its pane that is not an object is not a pane; the others are.
    assert_eq!(api.panes.len(), 2);
    assert_eq!(api.panes[0].role, None, "an unknown role reads as none");
    assert_eq!(api.panes[0].port_base, None);
    assert_eq!(
        api.panes[1].agent,
        AgentId::Shell,
        "an unknown agent reads as the shell"
    );
    assert_eq!(api.detached, None);
    assert_eq!(api.window_bounds, None);
    // A mode it does not know reads as inheriting, as `workspaceMcp` read it.
    assert_eq!(
        api.mcp.as_ref().map(|mcp| mcp.mode),
        Some(super::types::McpMode::Inherit)
    );
    // Hydration: unique names, and a folder that is gone is the top level.
    let names: Vec<&str> = state
        .workspaces
        .iter()
        .flat_map(|ws: &Workspace| &ws.panes)
        .map(|pane| pane.name.as_str())
        .collect();
    assert_eq!(names, ["Ava", "Max", "Mia", "Leo"]);
    assert_eq!(api.folder_id, None);
    // The legacy tree, by pane index, reads as ids.
    let legacy = &state.workspaces[1];
    assert_eq!(
        legacy.tree,
        Some(SplitNode::Split {
            dir: SplitDir::Col,
            ratio: 0.7,
            a: Box::new(layout::leaf("p-two")),
            b: Box::new(layout::leaf("p-one")),
        })
    );
    assert_eq!(legacy.orchestration.as_ref().unwrap().max_workers, 3);
    assert_eq!(state.active_workspace_id, None);
    let settings = &state.settings;
    assert_eq!(settings.font_size, 13.0);
    assert_eq!(settings.locale, None);
    assert_eq!(
        settings.scale_factor(),
        1.3,
        "a valid number, clamped where read"
    );
    assert_eq!(settings.saved_commands.len(), 1);
    assert!(!settings.saved_commands[0].auto_run);
    assert!(!state.folders[0].collapsed);
}

#[test]
fn what_could_not_be_read_is_written_back_as_it_was() {
    let (model, raw) = load(HAND_EDITED);
    let written = model.to_json();
    let api = &written["workspaces"][0];
    let raw_api = &raw["workspaces"][0];
    for key in [
        "detached",
        "windowBounds",
        "mcp",
        "claudeAccount",
        "futureWorkspaceField",
    ] {
        assert_eq!(api[key], raw_api[key], "workspace.{key}");
    }
    let ava = &api["panes"][0];
    for key in ["role", "taskId", "portBase", "futurePaneField"] {
        assert_eq!(ava[key], raw_api["panes"][0][key], "pane.{key}");
    }
    assert!(
        ava.get("taskId").is_some_and(Value::is_null),
        "an explicit null stays"
    );
    assert_eq!(api["panes"][1]["agent"], json!("aider"));
    assert_eq!(api["panes"][2], json!(3), "kept, at the end of its list");
    assert_eq!(written["workspaces"][2], json!("not a workspace"));
    assert_eq!(
        written["workspaces"][1]["orchestration"]["maxWorkers"],
        json!("many")
    );
    assert_eq!(written["folders"][0]["collapsed"], json!("yes"));
    assert_eq!(written["activeWorkspaceId"], json!(42));
    assert_eq!(written["version"], json!(3));
    let settings = &written["settings"];
    for key in [
        "locale",
        "fontSize",
        "cursorStyle",
        "experiments",
        "futureSetting",
        "keybindings",
    ] {
        assert_eq!(settings[key], raw["settings"][key], "settings.{key}");
    }
    assert_eq!(settings["uiScale"], json!(300.0));
    assert_eq!(settings["savedCommands"][0]["autoRun"], json!("sometimes"));
    assert_eq!(settings["savedCommands"][1], json!("junk"));

    // And it reads back the same.
    let (again, kept) = read_state_keeping(written);
    let mut reread = Model::default();
    reread.hydrate_read(again, kept);
    assert_eq!(reread.state(), model.state());
    assert_eq!(reread.to_json(), model.to_json());
}

#[test]
fn a_field_the_app_writes_is_the_app_s_from_then_on() {
    let (mut model, raw) = load(HAND_EDITED);
    model.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.font_size = 16.0,
    )));
    model.apply(StoreAction::UpdateWorkspace(
        "w-api".into(),
        Patch::new(|ws: &mut Workspace| ws.detached = Some(true)),
    ));
    let written = model.to_json();
    assert_eq!(written["settings"]["fontSize"], json!(16.0));
    assert_eq!(written["workspaces"][0]["detached"], json!(true));
    // The rest is still as it was.
    assert_eq!(
        written["settings"]["cursorStyle"],
        raw["settings"]["cursorStyle"]
    );
    assert_eq!(
        written["workspaces"][0]["windowBounds"],
        json!("left screen")
    );

    // Back to the default, the field stays the app's: no "big" again.
    model.apply(StoreAction::UpdateSettings(Patch::new(
        |s: &mut Settings| s.font_size = 13.0,
    )));
    assert_eq!(model.to_json()["settings"]["fontSize"], json!(13.0));
}

#[test]
fn what_a_workspace_held_goes_with_it() {
    let (mut model, _) = load(HAND_EDITED);
    model.apply(StoreAction::RemoveWorkspace("w-api".into()));
    let written = model.to_json();
    let workspaces = written["workspaces"].as_array().unwrap();
    assert_eq!(
        workspaces.len(),
        2,
        "the legacy one, and the string kept at the top level"
    );
    assert_eq!(workspaces[0]["id"], json!("w-legacy"));
    assert!(!written.to_string().contains("left screen"));
}

#[test]
fn a_respawned_pane_keeps_what_it_held() {
    let (mut model, _) = load(HAND_EDITED);
    let new = model
        .apply(StoreAction::RespawnPane {
            ws: "w-api".into(),
            pane: "p-ava".into(),
            patch: Patch::none(),
        })
        .pane
        .unwrap();
    let written = model.to_json();
    let pane = &written["workspaces"][0]["panes"][0];
    assert_eq!(pane["id"], json!(new));
    assert_eq!(pane["role"], json!("reviewer"));
    assert_eq!(pane["portBase"], json!("4100"));
}
