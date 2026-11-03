//! Reading `state.json` without ever failing on a value that is off.
//!
//! The file is hand-edited, and was written by versions with other fields. The
//! TS read it as plain JSON and guarded each reader (`workspaceKits`,
//! `orchestrationOf`, `readAccountChoice`…). A strict serde parse would fail
//! the whole file on one bad value, the store would quarantine it and start
//! empty, and the next save would write the empty state over the person's
//! workspaces (inventory §1.4 rule 4, §9.1 risk 4).
//!
//! So the JSON is cleaned before it is parsed: each struct's keys are tried
//! one at a time against the struct's own derived `Deserialize`, and a key
//! whose value does not parse is dropped, so that field takes its default
//! (`DEFAULT_SETTINGS` for the settings). Nested structs are cleaned first,
//! and an element of a list that is not even an object is dropped from the
//! list, so one bad pane costs that pane and not the workspace.
//!
//! What is dropped is gone on the next save, where the TS would have kept it
//! as written. That is the price of typed fields, and a value nothing could
//! read is not much of a loss; each drop is printed.
//!
//! TODO(P0b): the hydration fix-ups of `store.tsx:287` (unique pane names,
//! folder ids that name no folder) run after this, in `store.rs`.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::types::{AppState, OrchestrationSettings, Pane, Settings, Workspace};

/// `state.json`'s JSON as an `AppState`, whatever it holds. A document that is
/// not an object at all reads as the empty state.
pub fn app_state(mut value: Value) -> AppState {
    clean_app_state(&mut value);
    serde_json::from_value(value).unwrap_or_else(|err| {
        // Cleaning leaves only keys that parsed one by one, so this means a
        // combination of them failed, which the derives here cannot produce.
        eprintln!("state.json could not be read even leniently ({err}); starting empty");
        AppState::default()
    })
}

fn clean_app_state(value: &mut Value) {
    let Some(map) = value.as_object_mut() else {
        *value = Value::Object(Map::new());
        return;
    };
    if let Some(list) = map.get_mut("workspaces") {
        clean_list(list, "workspaces", clean_workspace);
    }
    if let Some(list) = map.get_mut("folders") {
        clean_list(list, "folders", |_| {});
    }
    if let Some(settings) = map.get_mut("settings") {
        clean_settings(settings);
    }
    drop_unreadable::<AppState>(map, "state");
}

fn clean_workspace(value: &mut Value) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    if let Some(list) = map.get_mut("panes") {
        clean_list(list, "panes", |pane| {
            if let Some(map) = pane.as_object_mut() {
                drop_unreadable::<Pane>(map, "pane");
            }
        });
    }
    if let Some(list) = map.get_mut("savedCommands") {
        clean_list(list, "savedCommands", |_| {});
    }
    if let Some(Value::Object(orchestration)) = map.get_mut("orchestration") {
        drop_unreadable::<OrchestrationSettings>(orchestration, "orchestration");
    }
    drop_unreadable::<Workspace>(map, "workspace");
}

fn clean_settings(value: &mut Value) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    if let Some(list) = map.get_mut("savedCommands") {
        clean_list(list, "settings.savedCommands", |_| {});
    }
    drop_unreadable::<Settings>(map, "settings");
}

/// Cleans each element of a list and drops those that are not objects. A
/// value that is not a list is left for `drop_unreadable` to drop.
fn clean_list(list: &mut Value, what: &str, clean: impl Fn(&mut Value)) {
    let Some(items) = list.as_array_mut() else {
        return;
    };
    items.retain(|item| {
        let keep = item.is_object();
        if !keep {
            eprintln!("state.json: dropped an entry of {what} that is not an object: {item}");
        }
        keep
    });
    for item in items {
        clean(item);
    }
}

/// Drops every key of `map` whose value `T` cannot read, trying each alone:
/// with `#[serde(default)]` on `T`, the others are then simply missing.
fn drop_unreadable<T: DeserializeOwned>(map: &mut Map<String, Value>, what: &str) {
    let unreadable: Vec<String> = map
        .iter()
        .filter(|(key, value)| {
            let mut alone = Map::new();
            alone.insert((*key).clone(), (*value).clone());
            serde_json::from_value::<T>(Value::Object(alone)).is_err()
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in unreadable {
        if let Some(value) = map.remove(&key) {
            eprintln!("state.json: dropped {what}.{key}, which could not be read: {value}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::types::{AgentId, SplitNode};
    use serde_json::json;

    #[test]
    fn a_file_written_by_the_old_app_round_trips_in_meaning() {
        let file = json!({
            "workspaces": [{
                "id": "w1", "name": "Hangar", "cwd": "/code/hangar", "extraRoots": [],
                "panes": [
                    {"id": "p1", "name": "Ada", "agent": "claude", "sessionId": "s1",
                     "cwd": null, "shellId": null, "title": null, "role": "orchestrator"},
                    {"id": "p2", "name": "Bob", "agent": "shell", "sessionId": null,
                     "cwd": null, "shellId": null, "title": null}
                ],
                "themeId": null, "shellId": null,
                "tree": {"type": "split", "dir": "row", "ratio": 0.5,
                         "a": {"type": "pane", "id": "p1"}, "b": {"type": "pane", "id": "p2"}},
                "savedCommands": [], "folderId": null, "hq": true,
                "futureField": {"kept": true}
            }],
            "folders": [{"id": "f1", "name": "Work", "collapsed": false}],
            "activeWorkspaceId": "w1",
            "settings": {"themeId": "nord", "fontSize": 14, "claudeAccount": null,
                         "keybindings": {"some.futureCommand": ["Ctrl+Q"]}},
            "topLevelFuture": 1
        });
        let state = app_state(file.clone());
        let ws = &state.workspaces[0];
        assert_eq!(ws.panes[0].agent, AgentId::Claude);
        assert!(matches!(ws.tree, Some(SplitNode::Split { .. })));
        assert_eq!(ws.extra["futureField"], json!({"kept": true}));
        assert_eq!(state.extra["topLevelFuture"], json!(1));
        assert_eq!(state.settings.theme_id, "nord");
        assert_eq!(state.settings.font_size, 14.0);
        // Absent settings take DEFAULT_SETTINGS; a null the person wrote stays.
        assert_eq!(state.settings.inspector_width, 296.0);
        assert_eq!(state.settings.claude_account, None);

        let written = serde_json::to_value(&state).unwrap();
        // `?` fields stay absent, `| null` fields are written as null.
        let pane = &written["workspaces"][0]["panes"][1];
        assert!(pane.get("role").is_none());
        assert_eq!(pane["sessionId"], Value::Null);
        assert_eq!(written["settings"]["claudeAccount"], Value::Null);
        assert_eq!(
            written["settings"]["keybindings"]["some.futureCommand"],
            json!(["Ctrl+Q"])
        );
        // And reading it back gives the same state.
        assert_eq!(app_state(written), state);
    }

    #[test]
    fn a_bad_value_costs_its_field_and_nothing_else() {
        let state = app_state(json!({
            "workspaces": [
                {"id": "w1", "name": 5, "panes": [{"id": "p1", "agent": "aider"}, 3],
                 "orchestration": {"maxWorkers": "many", "autoMerge": false}},
                "not a workspace"
            ],
            "settings": {"fontSize": "big", "uiScale": 110, "experiments": "all"}
        }));
        assert_eq!(state.workspaces.len(), 1);
        let ws = &state.workspaces[0];
        assert_eq!(ws.id, "w1");
        assert_eq!(ws.name, "");
        assert_eq!(ws.panes.len(), 1);
        assert_eq!(ws.panes[0].id, "p1");
        assert_eq!(ws.panes[0].agent, AgentId::Shell);
        let orchestration = ws.orchestration.as_ref().unwrap();
        assert_eq!(orchestration.max_workers, 3);
        assert!(!orchestration.auto_merge);
        assert_eq!(state.settings.font_size, 13.0);
        assert_eq!(state.settings.ui_scale, 110.0);
        assert!(state.settings.experiments.is_empty());
    }

    #[test]
    fn anything_but_an_object_is_the_empty_state() {
        assert_eq!(app_state(json!([1, 2])), AppState::default());
        assert_eq!(app_state(Value::Null), AppState::default());
    }
}
