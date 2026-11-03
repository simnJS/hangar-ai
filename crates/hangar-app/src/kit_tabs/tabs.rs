//! A kit's tabs, as a workspace offers them (docs/kits.md, `lib/kitTabs.ts`):
//! which tabs its enabled kits have, the URL each page is served at by the
//! `hangar-kit` scheme, what each may ask Hangar for, what a caller may pass
//! it, and the theme a page is told. `view.rs` draws a tab, `api.rs` answers
//! it.
//!
//! Gone with the web app: the installed kits' store (`useInstalledKits` is
//! `kits::Live`, P14's), the cross-window grants of the preview's folders
//! (one process holds the one list, `preview::access::Shared::admit`), and
//! `frameTookKeyboard`, since a page is a window of its own that never sits
//! between the person and a terminal of the main window.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use hangar_core::kits::{KitInfo, Tab};
use hangar_i18n::Locale;
use hangar_theme::Theme;

use crate::kits::activation::{KitState, kit_requested, kit_state};
use crate::kits::model::{
    effective_kits, is_kit_permission, kit_command_name, kit_commands_grant, kit_permissions_in,
};
use crate::preview::store::KitTabParams;
use crate::state::types::{Settings, UI_SCALE_MAX, UI_SCALE_MIN, Workspace, WorkspaceKit};

/// Where the scheme serves a file of a kit (`kitFileUrl`).
pub fn kit_file_url(kit_id: &str, path: &str) -> String {
    hangar_webview::kit_relay::kit_file_url(kit_id, path)
}

/// What a tab may call: what its manifest asks for and the person granted
/// the kit on the workspace, both. A permission granted that the tab does not
/// declare stays out of it, and one it declares that was not granted (added
/// by an update since) waits for the person to grant it.
pub fn effective_permissions<A: AsRef<str>, G: AsRef<str>>(
    asked: &[A],
    granted: &[G],
) -> Vec<String> {
    kit_permissions_in(
        asked
            .iter()
            .map(AsRef::as_ref)
            .filter(|permission| granted.iter().any(|held| held.as_ref() == *permission)),
    )
}

/// A tab the workspace can open: its kit enabled there, installed and usable
/// (`KitTabChoice`).
#[derive(Debug, Clone, PartialEq)]
pub struct KitTabChoice {
    pub kit_id: String,
    pub kit_name: String,
    pub tab: Tab,
    pub permissions: Vec<String>,
    /// The `commands:<hash>` this tab's commands run under, which Rust
    /// compares with the kit as it is at each call. `None` for a kit that is
    /// not asked: a linked folder, or no command.
    pub commands_accepted: Option<String>,
}

/// An installed kit that can be used now, under the id it is enabled by.
fn usable<'a>(installed: &'a [KitInfo], id: &str) -> Option<&'a KitInfo> {
    installed.iter().find(|kit| kit.id == id).filter(|kit| {
        kit.error.is_none()
            && kit
                .manifest
                .as_ref()
                .is_some_and(|manifest| manifest.id == id)
    })
}

/// Every tab of the enabled kits `kits` (a workspace's, with those enabled
/// everywhere: `effective_kits`), in the order the kits were enabled
/// (`workspaceKitTabs`). Nothing while the installed kits are not read.
pub fn workspace_kit_tabs(
    kits: &[WorkspaceKit],
    installed: Option<&[KitInfo]>,
) -> Vec<KitTabChoice> {
    let Some(installed) = installed else {
        return Vec::new();
    };
    kits.iter()
        .flat_map(|WorkspaceKit { id, granted }| {
            let Some(kit) = usable(installed, id) else {
                return Vec::new();
            };
            let Some(manifest) = kit.manifest.as_ref() else {
                return Vec::new();
            };
            // A GitHub kit's commands run only as the workspace accepted
            // them: changed, its tabs hold none until they are accepted
            // again, whatever else they hold.
            let grant = kit_commands_grant(kit);
            let held = grant.is_none_or(|grant| granted.iter().any(|g| g == grant));
            manifest
                .tabs
                .iter()
                .map(|tab| {
                    let asked: Vec<&str> = tab
                        .permissions
                        .iter()
                        .map(String::as_str)
                        .filter(|permission| is_kit_permission(permission))
                        .collect();
                    KitTabChoice {
                        kit_id: id.clone(),
                        kit_name: manifest.name.clone(),
                        tab: tab.clone(),
                        permissions: effective_permissions(&asked, granted)
                            .into_iter()
                            .filter(|permission| held || kit_command_name(permission).is_none())
                            .collect(),
                        commands_accepted: if held { grant.map(str::to_owned) } else { None },
                    }
                })
                .collect()
        })
        .collect()
}

/// What a kit tab of the preview runs now, or why it does not run
/// (`KitTabState`).
#[derive(Debug, Clone, PartialEq)]
pub enum KitTabState {
    Ready {
        choice: Box<KitTabChoice>,
        /// The page's URL, for a tab that has a page.
        url: Option<String>,
    },
    /// The installed kits are being read.
    Loading,
    /// The kit is not enabled on the workspace any more.
    Disabled,
    /// The kit is not installed any more, or has no such tab now.
    Missing,
    /// Installed, and unusable as it stands.
    Broken(String),
}

/// `kitTabState`, `kits` being the workspace's effective ones.
pub fn kit_tab_state(
    kits: &[WorkspaceKit],
    installed: Option<&[KitInfo]>,
    kit_id: &str,
    tab_id: &str,
) -> KitTabState {
    if !kits.iter().any(|kit| kit.id == kit_id) {
        return KitTabState::Disabled;
    }
    let Some(listed) = installed else {
        return KitTabState::Loading;
    };
    let Some(kit) = listed.iter().find(|kit| kit.id == kit_id) else {
        return KitTabState::Missing;
    };
    if usable(listed, kit_id).is_none() {
        return KitTabState::Broken(kit.error.clone().unwrap_or_default());
    }
    let Some(choice) = workspace_kit_tabs(kits, installed)
        .into_iter()
        .find(|choice| choice.kit_id == kit_id && choice.tab.id == tab_id)
    else {
        return KitTabState::Missing;
    };
    let url = choice.tab.page().map(|page| kit_file_url(kit_id, page));
    KitTabState::Ready {
        choice: Box::new(choice),
        url,
    }
}

/// The state of a tab on a workspace of the model: its own kits and those
/// enabled everywhere.
pub fn tab_state_on(
    ws: &Workspace,
    settings: &Settings,
    installed: Option<&[KitInfo]>,
    kit_id: &str,
    tab_id: &str,
) -> KitTabState {
    kit_tab_state(
        &effective_kits(Some(ws), Some(settings)),
        installed,
        kit_id,
        tab_id,
    )
}

/// Whether a kit tab's page is loaded now (`kitTabLoads`). A tab brought back
/// at startup waits for the first time it is shown; one an agent or a hook
/// opened, or asked for again, loads at once, as it may be following
/// something (a CI run). Once shown, a tab is a normal one: kept loaded,
/// hidden or not.
pub fn kit_tab_loads(restored: bool, shown: bool) -> bool {
    !restored || shown
}

/* ── what an agent or a hook may open ─────────────────────────── */

/// The longest value a param may hold, in characters.
pub const KIT_TAB_PARAM_MAX_CHARS: usize = 1024;

/// A value's rule, anchored; `None` for one this Hangar cannot read.
/// Compiled alone first: `a)|(?:b` only parses once wrapped, and would then
/// match past the anchors (`whole_value_rule` of kits/manifest.rs).
fn param_rule(pattern: &str) -> Option<regex::Regex> {
    regex::Regex::new(pattern).ok()?;
    regex::Regex::new(&format!(r"\A(?:{pattern})\z")).ok()
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// The params a caller gave, held to what the tab declares: known names,
/// strings, each matching its rule whole (`readKitTabParams`). Rust held them
/// to the same rules first, where a rule cannot backtrack for ever
/// (`Tab::param_refusal`). The message is the caller's to act on.
pub fn read_kit_tab_params(tab: &Tab, raw: Option<&Value>) -> Result<KitTabParams, String> {
    let entries: &Map<String, Value> = match raw {
        None | Some(Value::Null) => return Ok(KitTabParams::new()),
        Some(Value::Object(entries)) => entries,
        Some(_) => {
            return Err(
                "params must be an object of strings, by the names the tab declares.".into(),
            );
        }
    };
    let names: Vec<&str> = tab.params.keys().map(String::as_str).collect();
    let taken = if names.is_empty() {
        "It takes none.".to_string()
    } else {
        format!("It takes: {}.", names.join(", "))
    };
    let mut read = BTreeMap::new();
    for (name, value) in entries {
        let Some(declared) = tab.params.get(name) else {
            return Err(format!(
                "The tab \"{}\" declares no param {}. {taken}",
                tab.id,
                quoted(name)
            ));
        };
        let Value::String(value) = value else {
            return Err(format!("Param \"{name}\" must be a string."));
        };
        // Counted as the window counted it, in UTF-16 units.
        if value.encode_utf16().count() > KIT_TAB_PARAM_MAX_CHARS {
            return Err(format!(
                "Param \"{name}\" must be at most {KIT_TAB_PARAM_MAX_CHARS} characters."
            ));
        }
        if let Some(pattern) = &declared.pattern {
            let Some(rule) = param_rule(pattern) else {
                return Err(format!(
                    "Param \"{name}\" cannot be checked: Hangar's window cannot read the kit's rule {pattern}, so nothing passes. \
                     The kit has to write it in the syntax Rust and JavaScript share."
                ));
            };
            if !rule.is_match(value) {
                let shown: String = value.chars().take(60).collect();
                return Err(format!(
                    "Param \"{name}\" must match its rule {pattern}, and {} does not.",
                    quoted(&shown)
                ));
            }
        }
        read.insert(name.clone(), value.clone());
    }
    Ok(read)
}

/// The kit and its tab, for an agent or a hook opening it on the workspace;
/// the reason when they may not, said so it can be acted on
/// (`kitTabToOpen`). What the person opens from the palette or the menu does
/// not go through here.
pub fn kit_tab_to_open<'a>(
    ws: &Workspace,
    settings: &Settings,
    installed: &'a [KitInfo],
    kit_id: &str,
    tab_id: &str,
) -> Result<(&'a KitInfo, &'a Tab), String> {
    let enabled = effective_kits(Some(ws), Some(settings));
    let Some(entry) = enabled.iter().find(|kit| kit.id == kit_id) else {
        let list: Vec<&str> = enabled.iter().map(|kit| kit.id.as_str()).collect();
        let there = if list.is_empty() {
            "No kit is enabled there.".to_string()
        } else {
            format!("Enabled there: {}.", list.join(", "))
        };
        return Err(format!(
            "The kit \"{kit_id}\" is not enabled on \"{}\". {there} Only the person can enable one.",
            ws.name
        ));
    };
    let Some(kit) = installed.iter().find(|kit| kit.id == kit_id) else {
        return Err(format!(
            "The kit \"{kit_id}\" is enabled on \"{}\", and no longer installed.",
            ws.name
        ));
    };
    let Some(manifest) = usable(installed, kit_id).and_then(|kit| kit.manifest.as_ref()) else {
        return Err(format!(
            "The kit \"{kit_id}\" cannot be used right now: {}.",
            kit.error.as_deref().unwrap_or("its manifest is unreadable")
        ));
    };
    if kit_state(Some(entry), kit_requested(kit).as_deref()) == KitState::Review {
        return Err(format!(
            "The kit \"{kit_id}\" has changes to review on \"{}\": nothing of it can be opened until the person accepts them, \
             in the workspace's Kits dialog or in Settings › Kits. Tell them.",
            ws.name
        ));
    }
    let Some(tab) = manifest.tabs.iter().find(|tab| tab.id == tab_id) else {
        let tabs: Vec<&str> = manifest.tabs.iter().map(|tab| tab.id.as_str()).collect();
        let listed = if tabs.is_empty() {
            "It has no tabs.".to_string()
        } else {
            format!("Its tabs: {}.", tabs.join(", "))
        };
        return Err(format!(
            "The kit \"{kit_id}\" has no tab \"{tab_id}\". {listed}"
        ));
    };
    Ok((kit, tab))
}

/// What `kit_tab_list` says of the tabs a workspace's kits bring
/// (`kitTabListing`).
pub fn kit_tab_listing(ws: &Workspace, settings: &Settings, installed: &[KitInfo]) -> Vec<Value> {
    let enabled = effective_kits(Some(ws), Some(settings));
    workspace_kit_tabs(&enabled, Some(installed))
        .into_iter()
        .map(
            |KitTabChoice {
                 kit_id,
                 kit_name,
                 tab,
                 ..
             }| {
                let kit = installed.iter().find(|kit| kit.id == kit_id);
                let entry = enabled.iter().find(|kit| kit.id == kit_id);
                let mut row = json!({
                    "kit": kit_id,
                    "kitName": kit_name,
                    "tab": tab.id,
                    "title": tab.title,
                    "description": kit
                        .and_then(|kit| kit.manifest.as_ref())
                        .map(|manifest| manifest.description.clone())
                        .unwrap_or_default(),
                    "singleton": tab.singleton,
                    "params": tab.params,
                });
                let review = kit.is_some_and(|kit| {
                    kit_state(entry, kit_requested(kit).as_deref()) == KitState::Review
                });
                if review {
                    row["toReview"] = json!(true);
                }
                row
            },
        )
        .collect()
}

/* ── the theme a tab is told (docs/kit-ui.md) ─────────────────── */

/// The interface size as the window applies it: 1 at 100 % (`kitUiScale`).
pub fn kit_ui_scale(percent: f64) -> f64 {
    let percent = if percent == 0.0 || !percent.is_finite() {
        100.0
    } else {
        percent
    };
    percent.clamp(UI_SCALE_MIN, UI_SCALE_MAX) / 100.0
}

/// The `theme` event of the tab message protocol (`kitThemeEvent`): the
/// scheme of the tab's own workspace's theme, Hangar's language, and the
/// tokens of Hangar UI from that theme's chrome, with the interface size and
/// the terminals' face.
pub fn kit_theme_event(theme: &Theme, ui_scale: f64, mono: &str, locale: Locale) -> Value {
    let mut tokens = Map::new();
    tokens.insert("--kit-scale".into(), json!(ui_scale.to_string()));
    tokens.insert("--kit-mono".into(), json!(mono));
    for (token, value) in hangar_theme::kit_tokens(theme) {
        tokens.insert(token, json!(value));
    }
    hangar_webview::kit_relay::event(
        "theme",
        json!({
            "scheme": theme.appearance.as_str(),
            "locale": locale.id(),
            "tokens": tokens,
        }),
    )
}

/// Tells one page its theme: when it has loaded, and at each change from
/// then on (`kitThemeSender`). Before the load there is nobody to hear it, so
/// only the latest is kept. Each answer is what to post now.
#[derive(Debug, Default, Clone)]
pub struct ThemeSender {
    loaded: bool,
    latest: Option<Value>,
}

impl ThemeSender {
    /// The page loaded: the latest theme goes now.
    pub fn load(&mut self) -> Option<Value> {
        self.loaded = true;
        self.latest.clone()
    }

    /// The theme is now `event`: it goes at once if the page has loaded and
    /// it is not the one sent last.
    pub fn set(&mut self, event: Value) -> Option<Value> {
        if self.latest.as_ref() == Some(&event) {
            return None;
        }
        self.latest = Some(event);
        if self.loaded {
            self.latest.clone()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
