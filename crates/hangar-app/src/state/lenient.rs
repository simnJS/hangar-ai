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
//! whose value does not parse is taken out, so that field takes its default
//! (`DEFAULT_SETTINGS` for the settings). Nested structs are cleaned first,
//! one element of a list at a time, so one bad pane costs that pane's bad
//! field and not the workspace; an element that is not even an object is
//! taken out of its list.
//!
//! Nothing taken out is lost: each comes back as a `Kept`, which the model
//! holds (`unread.rs`) and writes back into the file on every save until the
//! app writes that field itself, as the TS kept it by spreading.
//!
//! Two shapes are translated rather than set aside. A legacy `tree` whose
//! leaves stored a pane's index (`{type: "pane", index: 1}`), which the TS
//! mapped onto pane ids each time it read the tree (`migrate`, layout.ts):
//! it is mapped once here, so the typed tree holds ids. And a pane whose
//! agent is one this version no longer runs (`RETIRED_AGENTS`: Codex, Gemini
//! CLI, OpenCode): it becomes a plain shell in the same folder, rather than
//! a shell whose old agent the next save would write back, and it is marked
//! so that its bar can say why (`retired_agent`).

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::layout;
use super::types::{
    AppState, OrchestrationSettings, Pane, RETIRED_AGENTS, SavedCommand, Settings, SplitNode,
    Workspace, WorkspaceFolder, WorkspaceMcp,
};
use super::unread::{self, Kept, Owner};

/// `state.json`'s JSON as an `AppState`, whatever it holds; what could not be
/// read is dropped. A document that is not an object reads as the empty state.
#[cfg(test)]
pub fn app_state(value: Value) -> AppState {
    read(value).0
}

/// `state.json`'s JSON as an `AppState`, and the values it held that no field
/// could read, to be written back (`Unread`). A document `read_checked`
/// turns down reads as the empty state.
#[cfg(test)]
pub fn read(value: Value) -> (AppState, Vec<Kept>) {
    read_checked(value).unwrap_or_else(|why| {
        eprintln!("state.json could not be read ({why}); starting empty");
        (AppState::default(), Vec::new())
    })
}

/// `read`, or why the document could not be read at all: its root is not an
/// object, or the cleaned document still failed to parse. Either way nothing
/// of it reaches the model, so the boot must not let the next save write the
/// empty state over it (the file is set aside first, services/core.rs).
pub fn read_checked(mut value: Value) -> Result<(AppState, Vec<Kept>), String> {
    if !value.is_object() {
        return Err("its content is not an object".into());
    }
    let mut kept = Vec::new();
    let retired = retire_agents(&mut value);
    clean_app_state(&mut value, &mut kept);
    // Cleaning leaves only keys that parsed one by one, so a failure here
    // means a combination of them failed, which the derives here cannot
    // produce; it is still answered rather than taken for an empty state.
    let mut state: AppState = serde_json::from_value(value.clone())
        .map_err(|err| format!("it could not be read even leniently: {err}"))?;
    if let Ok(written) = serde_json::to_value(&state) {
        explicit_nulls(&value, &written, &mut kept);
    }
    for ws in &mut state.workspaces {
        for pane in &mut ws.panes {
            pane.retired_agent = retired
                .iter()
                .find(|(w, p, _)| *w == ws.id && *p == pane.id)
                .map(|(_, _, agent)| *agent);
        }
    }
    Ok((state, kept))
}

/// Turns every pane whose agent this version no longer runs into a plain
/// shell, in the JSON before it is read: by workspace and pane id, the panes
/// it turned and the name of the agent each ran.
///
/// The pane keeps its folder, name and place in the layout. What belonged to
/// the old agent goes with it: the session id (a conversation Claude Code
/// cannot resume) and the model (one Claude Code would not start with), so
/// that switching the pane to Claude Code later starts it clean.
fn retire_agents(value: &mut Value) -> Vec<(String, String, &'static str)> {
    let mut retired = Vec::new();
    let workspaces = value
        .get_mut("workspaces")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten();
    for ws in workspaces {
        let ws_id = id_of(ws);
        let panes = ws
            .get_mut("panes")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten();
        for pane in panes {
            let pane_id = id_of(pane);
            let Some(map) = pane.as_object_mut() else {
                continue;
            };
            let Some(agent) = map
                .get("agent")
                .and_then(Value::as_str)
                .and_then(|agent| RETIRED_AGENTS.iter().find(|(id, _)| *id == agent))
                .map(|(_, name)| *name)
            else {
                continue;
            };
            map.insert("agent".into(), Value::String("shell".into()));
            map.insert("sessionId".into(), Value::Null);
            map.remove("model");
            retired.push((ws_id.clone(), pane_id, agent));
        }
    }
    retired
}

/// The `null`s of `?: T | null` fields (`taskId`, `model`, `portBase`,
/// `claudeAccount`…): read as `None`, which the typed struct leaves out of
/// the file, where the TS kept the `null` it read. Each comes back as a value
/// set aside, so it is written as it was until the app sets the field.
fn explicit_nulls(read: &Value, written: &Value, kept: &mut Vec<Kept>) {
    let mut owners = vec![Owner::Root, Owner::Settings];
    let ids = |list: Option<&Value>| -> Vec<String> {
        list.and_then(Value::as_array)
            .map(|items| items.iter().map(id_of).collect())
            .unwrap_or_default()
    };
    owners.extend(ids(read.get("folders")).into_iter().map(Owner::Folder));
    owners.extend(
        ids(read.get("settings").and_then(|s| s.get("savedCommands")))
            .into_iter()
            .map(Owner::SettingsCommand),
    );
    for ws in read
        .get("workspaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = id_of(ws);
        owners.push(Owner::Workspace(id.clone()));
        owners.push(Owner::Orchestration(id.clone()));
        owners.push(Owner::Mcp(id.clone()));
        owners.extend(
            ids(ws.get("panes"))
                .into_iter()
                .map(|pane| Owner::Pane(id.clone(), pane)),
        );
        owners.extend(
            ids(ws.get("savedCommands"))
                .into_iter()
                .map(|command| Owner::Command(id.clone(), command)),
        );
    }
    for owner in owners {
        let (Some(before), Some(after)) = (
            unread::locate(read, &owner),
            unread::locate(written, &owner),
        ) else {
            continue;
        };
        for (key, value) in before {
            if value.is_null() && !after.contains_key(key) {
                kept.push(Kept::Field {
                    owner: owner.clone(),
                    key: key.clone(),
                    raw: Value::Null,
                });
            }
        }
    }
}

fn clean_app_state(value: &mut Value, kept: &mut Vec<Kept>) {
    let Some(map) = value.as_object_mut() else {
        eprintln!("state.json is not an object; starting empty");
        *value = Value::Object(Map::new());
        return;
    };
    if let Some(list) = map.get_mut("workspaces") {
        clean_list(list, &Owner::Root, "workspaces", kept, clean_workspace);
    }
    if let Some(list) = map.get_mut("folders") {
        clean_list(list, &Owner::Root, "folders", kept, |folder, kept| {
            clean_object::<WorkspaceFolder>(folder, Owner::Folder(id_of(folder)), kept);
        });
    }
    if let Some(settings) = map.get_mut("settings") {
        clean_settings(settings, kept);
    }
    drop_unreadable::<AppState>(map, &Owner::Root, kept);
}

fn clean_workspace(value: &mut Value, kept: &mut Vec<Kept>) {
    let ws = id_of(value);
    let Some(map) = value.as_object_mut() else {
        return;
    };
    if let Some(list) = map.get_mut("panes") {
        clean_list(
            list,
            &Owner::Workspace(ws.clone()),
            "panes",
            kept,
            |pane, kept| {
                let owner = Owner::Pane(ws.clone(), id_of(pane));
                clean_object::<Pane>(pane, owner, kept);
            },
        );
    }
    if let Some(list) = map.get_mut("savedCommands") {
        clean_list(
            list,
            &Owner::Workspace(ws.clone()),
            "savedCommands",
            kept,
            |command, kept| {
                let owner = Owner::Command(ws.clone(), id_of(command));
                clean_object::<SavedCommand>(command, owner, kept);
            },
        );
    }
    if let Some(Value::Object(orchestration)) = map.get_mut("orchestration") {
        drop_unreadable::<OrchestrationSettings>(
            orchestration,
            &Owner::Orchestration(ws.clone()),
            kept,
        );
    }
    if let Some(Value::Object(mcp)) = map.get_mut("mcp") {
        drop_unreadable::<WorkspaceMcp>(mcp, &Owner::Mcp(ws.clone()), kept);
    }
    migrate_tree(map);
    drop_unreadable::<Workspace>(map, &Owner::Workspace(ws), kept);
}

/// A tree that does not read as a `SplitNode` but that `migrate` makes sense
/// of (a legacy one, by pane index) is replaced by what it means.
fn migrate_tree(workspace: &mut Map<String, Value>) {
    let Some(tree) = workspace.get("tree") else {
        return;
    };
    if tree.is_null() || serde_json::from_value::<SplitNode>(tree.clone()).is_ok() {
        return;
    }
    let ids: Vec<String> = workspace
        .get("panes")
        .and_then(Value::as_array)
        .map(|panes| {
            panes
                .iter()
                .map(|pane| {
                    pane.get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(migrated) = layout::migrate(tree, &ids)
        && let Ok(value) = serde_json::to_value(migrated)
    {
        workspace.insert("tree".into(), value);
    }
}

fn clean_settings(value: &mut Value, kept: &mut Vec<Kept>) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    if let Some(list) = map.get_mut("savedCommands") {
        clean_list(
            list,
            &Owner::Settings,
            "savedCommands",
            kept,
            |command, kept| {
                let owner = Owner::SettingsCommand(id_of(command));
                clean_object::<SavedCommand>(command, owner, kept);
            },
        );
    }
    drop_unreadable::<Settings>(map, &Owner::Settings, kept);
}

/// The `id` an object of the file goes by, as its typed struct will read it:
/// the string, or empty when it has none that reads.
fn id_of(value: &Value) -> String {
    value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn clean_object<T: DeserializeOwned>(value: &mut Value, owner: Owner, kept: &mut Vec<Kept>) {
    if let Some(map) = value.as_object_mut() {
        drop_unreadable::<T>(map, &owner, kept);
    }
}

/// Cleans each element of a list and takes out those that are not objects. A
/// value that is not a list is left for `drop_unreadable` to set aside.
fn clean_list(
    list: &mut Value,
    owner: &Owner,
    key: &'static str,
    kept: &mut Vec<Kept>,
    mut clean: impl FnMut(&mut Value, &mut Vec<Kept>),
) {
    let Some(items) = list.as_array_mut() else {
        return;
    };
    let mut objects = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if item.is_object() {
            objects.push(item);
        } else {
            eprintln!("state.json: kept aside an entry of {key} that is not an object: {item}");
            kept.push(Kept::Item {
                owner: owner.clone(),
                list: key.to_string(),
                raw: item,
            });
        }
    }
    for item in &mut objects {
        clean(item, kept);
    }
    *items = objects;
}

/// Takes out every key of `map` whose value `T` cannot read, trying each
/// alone: with `#[serde(default)]` on `T`, the others are then simply
/// missing.
fn drop_unreadable<T: DeserializeOwned>(
    map: &mut Map<String, Value>,
    owner: &Owner,
    kept: &mut Vec<Kept>,
) {
    let reads = |key: &str, value: Value| {
        let mut alone = Map::new();
        alone.insert(key.to_string(), value);
        serde_json::from_value::<T>(Value::Object(alone)).is_ok()
    };
    let unreadable: Vec<String> = map
        .iter()
        .filter(|(key, value)| !reads(key, (*value).clone()))
        .map(|(key, _)| key.clone())
        .collect();
    for key in unreadable {
        let Some(raw) = map.remove(&key) else {
            continue;
        };
        // A list that reads once the entries that do not are out of it (a
        // number among names) keeps the rest, as the TS readers filtered
        // them (`workspaceMcp` kept the strings of a custom list).
        if let Value::Array(items) = &raw {
            let (good, bad): (Vec<Value>, Vec<Value>) = items
                .iter()
                .cloned()
                .partition(|item| reads(&key, Value::Array(vec![item.clone()])));
            if !good.is_empty() && reads(&key, Value::Array(good.clone())) {
                for item in bad {
                    eprintln!("state.json: kept aside an entry of {owner:?}.{key}: {item}");
                    kept.push(Kept::Item {
                        owner: owner.clone(),
                        list: key.clone(),
                        raw: item,
                    });
                }
                map.insert(key, Value::Array(good));
                continue;
            }
        }
        eprintln!("state.json: kept aside {owner:?}.{key}, which could not be read: {raw}");
        kept.push(Kept::Field {
            owner: owner.clone(),
            key,
            raw,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::types::{AgentId, SplitDir};
    use serde_json::json;

    /// A document that holds no state at all is told apart from an empty
    /// one: the boot sets the file aside rather than saving over it.
    #[test]
    fn a_document_that_is_not_an_object_is_turned_down_not_read_as_empty() {
        for root in [json!([]), json!(null), json!("state"), json!(3)] {
            assert!(read_checked(root.clone()).is_err(), "{root}");
            assert_eq!(read(root).0, AppState::default());
        }
        let (state, kept) = read_checked(json!({})).expect("an empty state is a state");
        assert_eq!(state, AppState::default());
        assert!(kept.is_empty());
    }

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
        let (state, kept) = read(file.clone());
        assert!(kept.is_empty());
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
        let (state, kept) = read(json!({
            "workspaces": [
                {"id": "w1", "name": 5, "panes": [{"id": "p1", "agent": "aider"}, 3],
                 "orchestration": {"workerModel": 5, "autoMerge": false}},
                "not a workspace"
            ],
            "folders": [{"id": "f1", "name": "Work", "collapsed": "yes"}, {"id": "f2"}],
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
        assert_eq!(orchestration.worker_model, "sonnet");
        assert!(!orchestration.auto_merge);
        // One folder's bad field does not cost the others.
        assert_eq!(state.folders.len(), 2);
        assert_eq!(state.folders[0].name, "Work");
        assert!(!state.folders[0].collapsed);
        assert_eq!(state.settings.font_size, 13.0);
        assert_eq!(state.settings.ui_scale, 110.0);
        assert!(state.settings.experiments.is_empty());

        // And all of it is kept, by where it was.
        let has = |owner: Owner, key: &str, raw: Value| {
            kept.contains(&Kept::Field {
                owner,
                key: key.into(),
                raw,
            })
        };
        assert!(has(Owner::Workspace("w1".into()), "name", json!(5)));
        assert!(has(
            Owner::Pane("w1".into(), "p1".into()),
            "agent",
            json!("aider")
        ));
        assert!(has(
            Owner::Orchestration("w1".into()),
            "workerModel",
            json!(5)
        ));
        assert!(has(Owner::Folder("f1".into()), "collapsed", json!("yes")));
        assert!(has(Owner::Settings, "fontSize", json!("big")));
        assert!(has(Owner::Settings, "experiments", json!("all")));
        assert!(kept.contains(&Kept::Item {
            owner: Owner::Workspace("w1".into()),
            list: "panes".into(),
            raw: json!(3)
        }));
        assert!(kept.contains(&Kept::Item {
            owner: Owner::Root,
            list: "workspaces".into(),
            raw: json!("not a workspace")
        }));
    }

    /// 1.x's limits, `maxWorkers` and `maxAttempts`, which this version does
    /// not have: a file that holds them reads as one that does not, however
    /// they are written, and nothing of them is set aside to come back.
    #[test]
    fn the_limits_of_the_old_app_read_as_nothing_and_are_not_written_back() {
        let (state, kept) = read(json!({
            "workspaces": [
                {"id": "w1", "orchestration": {"maxWorkers": 5, "maxAttempts": 4, "autoMerge": false}},
                {"id": "w2", "orchestration": {"maxWorkers": "many", "maxAttempts": 2.5}}
            ]
        }));
        assert!(kept.is_empty(), "{kept:?}");
        let orchestration = |at: usize| state.workspaces[at].orchestration.clone().unwrap();
        assert_eq!(
            orchestration(0),
            OrchestrationSettings {
                auto_merge: false,
                ..OrchestrationSettings::default()
            }
        );
        assert_eq!(orchestration(1), OrchestrationSettings::default());
        let written = serde_json::to_value(&state).unwrap();
        for ws in written["workspaces"].as_array().unwrap() {
            let orchestration = ws["orchestration"].as_object().unwrap();
            assert!(!orchestration.contains_key("maxWorkers"), "{ws}");
            assert!(!orchestration.contains_key("maxAttempts"), "{ws}");
        }
    }

    #[test]
    fn a_legacy_tree_by_pane_index_reads_as_ids() {
        let (state, kept) = read(json!({
            "workspaces": [{
                "id": "w", "panes": [{"id": "a"}, {"id": "b"}],
                "tree": {"type": "split", "dir": "col", "ratio": 0.6,
                         "a": {"type": "pane", "index": 1}, "b": {"type": "pane", "index": 0}}
            }]
        }));
        assert!(kept.is_empty());
        assert_eq!(
            state.workspaces[0].tree,
            Some(SplitNode::Split {
                dir: SplitDir::Col,
                ratio: 0.6,
                a: Box::new(layout::leaf("b")),
                b: Box::new(layout::leaf("a")),
            })
        );
    }

    #[test]
    fn a_list_keeps_the_entries_that_read_and_sets_the_others_aside() {
        let (state, kept) = read(json!({
            "workspaces": [{
                "id": "w", "mcp": {"mode": "custom", "enabled": ["hangar", 5, "github"]},
                "extraRoots": ["/a", null]
            }],
            "settings": {"experiments": [7]}
        }));
        let ws = &state.workspaces[0];
        let mcp = ws
            .mcp
            .as_ref()
            .expect("a custom list with a number in it is still custom");
        assert_eq!(mcp.mode, crate::state::types::McpMode::Custom);
        assert_eq!(mcp.enabled, ["hangar", "github"]);
        assert_eq!(ws.extra_roots, ["/a"]);
        assert!(state.settings.experiments.is_empty());
        assert!(kept.contains(&Kept::Item {
            owner: Owner::Mcp("w".into()),
            list: "enabled".into(),
            raw: json!(5)
        }));
        assert!(kept.contains(&Kept::Item {
            owner: Owner::Workspace("w".into()),
            list: "extraRoots".into(),
            raw: Value::Null
        }));
        // Nothing in it reads: the whole value is set aside.
        assert!(kept.contains(&Kept::Field {
            owner: Owner::Settings,
            key: "experiments".into(),
            raw: json!([7])
        }));
    }

    /// A file of the old app with a Codex, a Gemini CLI and an OpenCode pane:
    /// each opens as a shell in its own folder, marked for its note, and is
    /// written back as a shell. Nothing of it is set aside to come back.
    #[test]
    fn a_pane_of_a_retired_agent_reads_as_a_shell_in_the_same_folder() {
        let (state, kept) = read(json!({
            "workspaces": [{
                "id": "w1", "name": "Shop", "cwd": "/code/shop",
                "panes": [
                    {"id": "p1", "name": "Ada", "agent": "claude", "sessionId": "s1"},
                    {"id": "p2", "name": "Bob", "agent": "codex",
                     "sessionId": "rollout-1", "model": "gpt-5.5", "cwd": "/code/shop/api"},
                    {"id": "p3", "name": "Cy", "agent": "gemini", "sessionId": null},
                    {"id": "p4", "name": "Di", "agent": "opencode", "title": "tests"}
                ],
                "tree": {"type": "split", "dir": "row", "ratio": 0.5,
                         "a": {"type": "pane", "id": "p1"}, "b": {"type": "pane", "id": "p2"}}
            }]
        }));
        assert!(kept.is_empty(), "{kept:?}");
        let panes = &state.workspaces[0].panes;
        assert_eq!(panes.len(), 4, "no pane is dropped");
        assert_eq!(panes[0].agent, AgentId::Claude);
        assert_eq!(panes[0].retired_agent, None);
        assert_eq!(panes[0].session_id.as_deref(), Some("s1"));
        let shells: Vec<_> = panes[1..]
            .iter()
            .map(|pane| (pane.name.as_str(), pane.agent, pane.retired_agent))
            .collect();
        assert_eq!(
            shells,
            [
                ("Bob", AgentId::Shell, Some("Codex")),
                ("Cy", AgentId::Shell, Some("Gemini CLI")),
                ("Di", AgentId::Shell, Some("OpenCode")),
            ]
        );
        let bob = &panes[1];
        assert_eq!(bob.cwd.as_deref(), Some("/code/shop/api"), "its own folder");
        assert_eq!(
            (bob.session_id.as_deref(), bob.model.as_deref()),
            (None, None)
        );
        assert_eq!(panes[3].title.as_deref(), Some("tests"));
        assert!(matches!(
            state.workspaces[0].tree,
            Some(SplitNode::Split { .. })
        ));

        let written = serde_json::to_value(&state).unwrap();
        for (at, pane) in written["workspaces"][0]["panes"].as_array().unwrap()[1..]
            .iter()
            .enumerate()
        {
            assert_eq!(pane["agent"], json!("shell"), "pane {at}");
            assert!(pane.get("model").is_none());
            assert!(
                pane.get("retiredAgent").is_none(),
                "the mark is not written"
            );
        }
        // Read again, a shell is a shell: no mark, no note a second time.
        let (again, _) = read(written);
        assert!(
            again.workspaces[0]
                .panes
                .iter()
                .all(|p| p.retired_agent.is_none())
        );
    }

    /// `agentCommands` of the old app held a command per agent: Claude Code's
    /// is read, the others' are not, and the next save leaves them out.
    #[test]
    fn only_claude_codes_launch_command_is_read_and_written() {
        let (state, kept) = read(json!({
            "settings": {"agentCommands": {
                "claude": "ccr code", "codex": "codex --yolo", "gemini": "gemini -y"
            }}
        }));
        assert!(kept.is_empty());
        assert_eq!(
            state.settings.agent_commands.claude.as_deref(),
            Some("ccr code")
        );
        let written = serde_json::to_value(&state).unwrap();
        assert_eq!(
            written["settings"]["agentCommands"],
            json!({"claude": "ccr code"})
        );

        // None for Claude Code, or one that is not text: not customised, and
        // nothing is set aside to come back.
        for commands in [json!({"opencode": "oc"}), json!({"claude": 5})] {
            let (state, kept) = read(json!({"settings": {"agentCommands": commands}}));
            assert!(kept.is_empty());
            assert_eq!(state.settings.agent_commands.claude, None);
            let written = serde_json::to_value(&state).unwrap();
            assert_eq!(written["settings"]["agentCommands"], json!({}));
        }
    }

    #[test]
    fn anything_but_an_object_is_the_empty_state() {
        assert_eq!(app_state(json!([1, 2])), AppState::default());
        assert_eq!(app_state(Value::Null), AppState::default());
    }
}
