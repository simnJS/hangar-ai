//! A kit's tabs: `kit_tab_list`, `kit_tab_open`, `kit_tab_close`
//! (control.ts, "a kit's tabs"; docs/kits.md).
//!
//! The tabs are the caller's own workspace's. The workspace comes from the
//! caller's pane, or its folder, never from the request: what is passed as
//! `workspace` or `pane` is not read. The caller comes from its pane's
//! environment (core's MCP server), or from the pane a kit hook says it runs
//! in, which core checked is running. The name a tab's page is told is the
//! workspace's own for that pane: whatever a request calls itself, only a
//! pane of this workspace can be answered.
//!
//! What a kit's tabs are and which params they take is P16's to read
//! (`KitTabCatalog`); what is decided here is where they open and what the
//! agent is told.

use serde_json::{Value, json};

use crate::state::Workspace;

use super::panes::announce;
use super::port::{EntryKind, KitParams, OpenEntry, Port, Showing};
use super::preview::{closed, preview_of, shown, sight, wake_for};
use super::run::{caller_workspace, unknown};
use super::types::{Caller, same, text};

pub async fn control(
    op: &str,
    args: &Value,
    caller: &Caller,
    port: &Port,
) -> Result<Value, String> {
    let snapshot = port.snapshot();
    let state = &snapshot.state;
    let Some(ws) = caller_workspace(state, caller) else {
        return Err(match &caller.pane_id {
            Some(pane) => format!(
                "The pane \"{pane}\" is in no workspace of Hangar, so there is nowhere to open a tab."
            ),
            None => "Could not tell which workspace you are in, so there is nowhere to open a tab."
                .into(),
        });
    };
    let pane = ws
        .panes
        .iter()
        .find(|entry| caller.pane_id.as_deref() == Some(entry.id.as_str()))
        .or_else(|| {
            let name = caller.pane_name.as_deref()?;
            ws.panes.iter().find(|entry| same(&entry.name, name))
        });
    let by = pane.map(|pane| pane.name.clone());
    match op {
        "kit_tab_list" => {
            let catalog = port.kit_tabs().ok_or_else(unavailable)?;
            let installed = port.kits().await?;
            Ok(json!({
                "workspace": ws.name,
                "tabs": catalog.listing(ws, &state.settings, &installed),
            }))
        }
        "kit_tab_open" => open(ws, args, by, port).await,
        "kit_tab_close" => close(ws, args, by, port).await,
        _ => Err(unknown(op)),
    }
}

/// Said while P16's catalog is not installed: no tab can be read.
fn unavailable() -> String {
    "Kit tabs are not available in this Hangar window.".into()
}

/// The `kit` and `tab` of a request; both are needed.
fn named(args: &Value) -> Result<(String, String), String> {
    match (text(args.get("kit")), text(args.get("tab"))) {
        (Some(kit), Some(tab)) => Ok((kit, tab)),
        _ => Err("Name the \"kit\" and the \"tab\", as kit_tab_list shows them.".into()),
    }
}

/// A set of params, the same whichever order its names came in
/// (`kitParamsKey`): `[["name","value"],…]` sorted by name.
pub fn params_key(params: &KitParams) -> String {
    serde_json::to_string(&params.iter().collect::<Vec<_>>()).unwrap_or_default()
}

/// The key of params as a request gave them, unchecked: what `kit_tab_close`
/// matches tabs by.
fn raw_key(raw: &serde_json::Map<String, Value>) -> String {
    let mut entries: Vec<(&String, &Value)> = raw.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    serde_json::to_string(&entries).unwrap_or_default()
}

/// What stands for a kit's tab where a file has its path (`kitTabPath`): one
/// per tab, or, given params, one per set of them.
pub fn tab_path(kit: &str, tab: &str, params: &KitParams) -> String {
    if params.is_empty() {
        format!("kit:{kit}/{tab}")
    } else {
        format!("kit:{kit}/{tab}?{}", params_key(params))
    }
}

fn params_of(entry: &OpenEntry) -> Option<(&str, &str, &KitParams)> {
    match &entry.what {
        EntryKind::Kit {
            kit_id,
            tab_id,
            params,
        } => Some((kit_id, tab_id, params)),
        _ => None,
    }
}

/// The open tab a request would land on (`findKitTab`): a singleton's
/// whatever it holds, else the one with those very params.
fn find_tab<'a>(
    entries: &'a [OpenEntry],
    kit: &str,
    tab: &str,
    params: &KitParams,
    singleton: bool,
) -> Option<&'a OpenEntry> {
    let key = params_key(params);
    entries.iter().find(|entry| {
        params_of(entry).is_some_and(|(kit_id, tab_id, held)| {
            kit_id == kit && tab_id == tab && (singleton || params_key(held) == key)
        })
    })
}

async fn open(
    ws: &Workspace,
    args: &Value,
    by: Option<String>,
    port: &Port,
) -> Result<Value, String> {
    let (kit, tab) = named(args)?;
    let reveal = match args.get("reveal") {
        None => true,
        Some(Value::Bool(reveal)) => *reveal,
        Some(_) => return Err("reveal must be true or false.".into()),
    };
    let catalog = port.kit_tabs().ok_or_else(unavailable)?;
    let installed = port.kits().await?;
    let settings = port.snapshot().state.settings.clone();
    let spec = catalog.to_open(ws, &settings, &installed, &kit, &tab, args.get("params"))?;
    let params = spec.params.clone();

    let preview = preview_of(ws, port).await?;
    let before = preview.list().await?;
    let before = find_tab(&before.entries, &kit, &tab, &params, spec.singleton).cloned();
    let woke = wake_for(ws, port).await;
    let none = KitParams::new();
    let id = preview
        .show(Showing::Kit {
            path: tab_path(&kit, &tab, if spec.singleton { &none } else { &params }),
            kit_id: kit.clone(),
            tab_id: tab.clone(),
            title: spec.title.clone(),
            params: params.clone(),
            singleton: spec.singleton,
            opened_by: by.clone(),
            reveal,
        })
        .await?;
    let outcome = match before.as_ref().and_then(params_of) {
        None => "opened",
        Some((_, _, held)) if !params.is_empty() && params_key(&params) != params_key(held) => {
            "replaced"
        }
        Some(_) => "focused",
    };
    let mut answer = json!({
        "outcome": outcome,
        "kit": kit,
        "tab": tab,
        "params": params,
        "workspace": ws.name,
    });
    if !reveal {
        let listing = preview.list().await.unwrap_or_default();
        answer["visible"] =
            json!(listing.shown && listing.active_id.as_deref() == Some(id.as_str()));
        answer["note"] = json!(
            "Added without opening the panel: the preview's toolbar button tells the person there is something to see."
        );
        return Ok(answer);
    }
    announce(port, shown(port, ws, by, spec.title)).await;
    if let Value::Object(map) = &mut answer {
        map.extend(sight(ws, &preview, woke).await.0);
    }
    Ok(answer)
}

/// With params, the tab holding those very ones; without, every tab of it
/// open in the workspace. Nothing is asked of the kit: one taken off the
/// workspace since can still have its tabs closed.
async fn close(
    ws: &Workspace,
    args: &Value,
    by: Option<String>,
    port: &Port,
) -> Result<Value, String> {
    let (kit, tab) = named(args)?;
    let preview = preview_of(ws, port).await?;
    let listing = preview.list().await?;
    let open: Vec<(&OpenEntry, &KitParams)> = listing
        .entries
        .iter()
        .filter_map(|entry| {
            let (kit_id, tab_id, params) = params_of(entry)?;
            (kit_id == kit && tab_id == tab).then_some((entry, params))
        })
        .collect();
    let wanted = match args.get("params") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Object(raw)) => Some(raw_key(raw)),
        // Not params at all: nothing holds them.
        Some(_) => Some(String::new()),
    };
    let (closing, left): (Vec<_>, Vec<_>) = open
        .into_iter()
        .partition(|(_, params)| wanted.as_ref().is_none_or(|key| *key == params_key(params)));
    let left: Vec<&KitParams> = left.into_iter().map(|(_, params)| params).collect();
    if closing.is_empty() {
        return Ok(json!({ "closed": [], "open": left }));
    }
    preview
        .close(closing.iter().map(|(entry, _)| entry.id.clone()).collect())
        .await;
    let titles = closing
        .iter()
        .map(|(entry, _)| entry.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    announce(port, closed(port, ws, by, titles)).await;
    Ok(json!({
        "closed": closing.iter().map(|(_, params)| *params).collect::<Vec<_>>(),
        "open": left,
    }))
}
