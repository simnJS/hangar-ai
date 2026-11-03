//! What `state.json` held that this version could not read, kept as written.
//!
//! The TS read the file as plain JSON and spread every write over what was
//! there (`{...ws, ...patch}`), so a value no reader understood (a `role` a
//! newer version added, a `windowBounds` edited by hand into a string) stayed
//! in the file until something wrote that very field. Typed fields cannot
//! hold such a value: the lenient reader (`lenient.rs`) gives the field its
//! default so the rest of the file still loads. This module is what makes up
//! for it: each value set aside is remembered with the object it belongs to,
//! and written back where it was on every save, for as long as the app has
//! not put a value of its own in that field.
//!
//! "A value of its own" is told by comparison: when the state settles after
//! hydration, the field's value (its default, or what the hydration fix-ups
//! made of it) is noted as its *substitute*. After every store action, an
//! entry whose field no longer holds its substitute, or whose object is gone
//! (a workspace removed, a pane closed), is dropped for good. One case slips
//! through: an action that writes exactly the substitute (a form saving the
//! default `maxWorkers: 3` over a hand-written `"many"`) is not seen as a
//! write, and the raw value comes back in the file. The app goes on reading
//! the default, as it does today, so the only trace is in the file.
//!
//! Entries of a list that were not even objects (a `3` among the panes) are
//! kept too, and written back at the end of their list.

use serde_json::{Map, Value};

use super::types::{PaneId, WsId};

/// The object a value sat in, by the ids of what holds it. Ids rather than
/// positions, since workspaces, panes and commands move.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Owner {
    /// The document itself (`workspaces`, `folders`, `activeWorkspaceId`…).
    Root,
    Settings,
    /// One of `settings.savedCommands`.
    SettingsCommand(String),
    Folder(String),
    Workspace(WsId),
    /// A workspace's `orchestration`.
    Orchestration(WsId),
    Pane(WsId, PaneId),
    /// One of a workspace's `savedCommands`.
    Command(WsId, String),
}

/// One value set aside by the reader.
#[derive(Debug, Clone, PartialEq)]
pub enum Kept {
    /// A field of `owner` that did not read as its type: the typed field has
    /// its default instead.
    Field {
        owner: Owner,
        key: String,
        raw: Value,
    },
    /// An entry of `owner`'s list `list` that was not an object.
    Item {
        owner: Owner,
        list: &'static str,
        raw: Value,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    kept: Kept,
    /// What the field held when the state settled; `None` when absent.
    substitute: Option<Value>,
}

/// The values set aside, settled against the state they were read with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Unread {
    entries: Vec<Entry>,
}

impl Unread {
    /// The values `kept` by the reader, against `state`, the hydrated state
    /// as JSON: what each field holds now is its substitute.
    pub fn settle(kept: Vec<Kept>, state: &Value) -> Unread {
        let entries = kept
            .into_iter()
            .filter_map(|kept| {
                let substitute = match &kept {
                    Kept::Field { owner, key, .. } => locate(state, owner)?.get(key).cloned(),
                    Kept::Item { owner, .. } => {
                        locate(state, owner)?;
                        None
                    }
                };
                Some(Entry { kept, substitute })
            })
            .collect();
        Unread { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Drops what the app has since written over, or whose object is gone.
    pub fn prune(&mut self, state: &Value) {
        self.entries.retain(|entry| match &entry.kept {
            Kept::Field { owner, key, .. } => locate(state, owner)
                .is_some_and(|object| object.get(key) == entry.substitute.as_ref()),
            Kept::Item { owner, .. } => locate(state, owner).is_some(),
        });
    }

    /// Writes every value set aside back into `state`, the state as JSON
    /// about to be saved.
    pub fn restore(&self, state: &mut Value) {
        for entry in &self.entries {
            match &entry.kept {
                Kept::Field { owner, key, raw } => {
                    if let Some(object) = locate_mut(state, owner) {
                        object.insert(key.clone(), raw.clone());
                    }
                }
                Kept::Item { owner, list, raw } => {
                    if let Some(Value::Array(items)) =
                        locate_mut(state, owner).and_then(|object| object.get_mut(*list))
                    {
                        items.push(raw.clone());
                    }
                }
            }
        }
    }

    /// A pane respawned under a new id keeps what it carried: the TS spread
    /// the old pane into the new one.
    pub fn rename_pane(&mut self, ws: &str, from: &str, to: &str) {
        for entry in &mut self.entries {
            let owner = match &mut entry.kept {
                Kept::Field { owner, .. } | Kept::Item { owner, .. } => owner,
            };
            if let Owner::Pane(owner_ws, pane) = owner
                && owner_ws == ws
                && pane == from
            {
                *pane = to.to_string();
            }
        }
    }
}

/// The first entry of `list` (an array of objects) whose `id` is `id`.
fn by_id<'v>(list: Option<&'v Value>, id: &str) -> Option<&'v Map<String, Value>> {
    list?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .find(|entry| entry.get("id").and_then(Value::as_str).unwrap_or("") == id)
}

fn by_id_mut<'v>(list: Option<&'v mut Value>, id: &str) -> Option<&'v mut Map<String, Value>> {
    list?
        .as_array_mut()?
        .iter_mut()
        .filter_map(Value::as_object_mut)
        .find(|entry| entry.get("id").and_then(Value::as_str).unwrap_or("") == id)
}

/// The object `owner` names in `state`, the state as JSON.
pub(super) fn locate<'v>(state: &'v Value, owner: &Owner) -> Option<&'v Map<String, Value>> {
    let root = state.as_object()?;
    match owner {
        Owner::Root => Some(root),
        Owner::Settings => root.get("settings")?.as_object(),
        Owner::SettingsCommand(id) => by_id(root.get("settings")?.get("savedCommands"), id),
        Owner::Folder(id) => by_id(root.get("folders"), id),
        Owner::Workspace(ws) => by_id(root.get("workspaces"), ws),
        Owner::Orchestration(ws) => by_id(root.get("workspaces"), ws)?
            .get("orchestration")?
            .as_object(),
        Owner::Pane(ws, pane) => by_id(by_id(root.get("workspaces"), ws)?.get("panes"), pane),
        Owner::Command(ws, id) => {
            by_id(by_id(root.get("workspaces"), ws)?.get("savedCommands"), id)
        }
    }
}

fn locate_mut<'v>(state: &'v mut Value, owner: &Owner) -> Option<&'v mut Map<String, Value>> {
    let root = state.as_object_mut()?;
    match owner {
        Owner::Root => Some(root),
        Owner::Settings => root.get_mut("settings")?.as_object_mut(),
        Owner::SettingsCommand(id) => {
            by_id_mut(root.get_mut("settings")?.get_mut("savedCommands"), id)
        }
        Owner::Folder(id) => by_id_mut(root.get_mut("folders"), id),
        Owner::Workspace(ws) => by_id_mut(root.get_mut("workspaces"), ws),
        Owner::Orchestration(ws) => by_id_mut(root.get_mut("workspaces"), ws)?
            .get_mut("orchestration")?
            .as_object_mut(),
        Owner::Pane(ws, pane) => by_id_mut(
            by_id_mut(root.get_mut("workspaces"), ws)?.get_mut("panes"),
            pane,
        ),
        Owner::Command(ws, id) => by_id_mut(
            by_id_mut(root.get_mut("workspaces"), ws)?.get_mut("savedCommands"),
            id,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(owner: Owner, key: &str, raw: Value) -> Kept {
        Kept::Field {
            owner,
            key: key.into(),
            raw,
        }
    }

    #[test]
    fn a_value_set_aside_goes_back_where_it_was() {
        let unread = Unread::settle(
            vec![field(
                Owner::Pane("w".into(), "p".into()),
                "role",
                json!("boss"),
            )],
            &json!({"workspaces": [{"id": "w", "panes": [{"id": "p"}]}]}),
        );
        let mut saved = json!({"workspaces": [{"id": "w", "panes": [{"id": "p"}]}]});
        unread.restore(&mut saved);
        assert_eq!(saved["workspaces"][0]["panes"][0]["role"], json!("boss"));
    }

    #[test]
    fn a_field_the_app_wrote_since_is_the_app_s() {
        let settled = json!({"settings": {"fontSize": 13.0}});
        let mut unread = Unread::settle(
            vec![field(Owner::Settings, "fontSize", json!("big"))],
            &settled,
        );
        unread.prune(&settled);
        assert_eq!(unread.len(), 1, "unchanged: still kept");
        unread.prune(&json!({"settings": {"fontSize": 15.0}}));
        assert!(unread.is_empty());
        // Gone for good: back at the default, it stays the app's.
        let mut saved = settled.clone();
        unread.restore(&mut saved);
        assert_eq!(saved, settled);
    }

    #[test]
    fn a_value_goes_with_its_object() {
        let settled = json!({"workspaces": [{"id": "w", "mcp": null}]});
        let mut unread = Unread::settle(
            vec![
                field(Owner::Workspace("w".into()), "mcp", json!("on")),
                Kept::Item {
                    owner: Owner::Workspace("w".into()),
                    list: "panes",
                    raw: json!(3),
                },
            ],
            &settled,
        );
        unread.prune(&json!({"workspaces": []}));
        assert!(unread.is_empty());
    }

    #[test]
    fn an_entry_that_is_not_an_object_lands_at_the_end_of_its_list() {
        let settled = json!({"workspaces": [{"id": "w", "panes": [{"id": "p"}]}]});
        let unread = Unread::settle(
            vec![Kept::Item {
                owner: Owner::Workspace("w".into()),
                list: "panes",
                raw: json!(3),
            }],
            &settled,
        );
        let mut saved = json!({"workspaces": [{"id": "w", "panes": [{"id": "p"}, {"id": "q"}]}]});
        unread.restore(&mut saved);
        assert_eq!(saved["workspaces"][0]["panes"][2], json!(3));
    }

    #[test]
    fn a_respawned_pane_keeps_what_it_carried() {
        let settled = json!({"workspaces": [{"id": "w", "panes": [{"id": "old"}]}]});
        let mut unread = Unread::settle(
            vec![field(
                Owner::Pane("w".into(), "old".into()),
                "portBase",
                json!("x"),
            )],
            &settled,
        );
        unread.rename_pane("w", "old", "new");
        let respawned = json!({"workspaces": [{"id": "w", "panes": [{"id": "new"}]}]});
        unread.prune(&respawned);
        let mut saved = respawned.clone();
        unread.restore(&mut saved);
        assert_eq!(saved["workspaces"][0]["panes"][0]["portBase"], json!("x"));
    }
}
