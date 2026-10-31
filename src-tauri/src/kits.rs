//! Kits: packages a person links to a workspace to bring a domain — a game
//! engine, a studio — into Hangar without Hangar knowing anything about it.
//!
//! A kit is a folder with a `hangar-kit.json` at its root (manifest.rs). It
//! may hold a Claude Code plugin — skills, commands, agents, MCP servers —
//! that every Claude Code pane of a workspace the kit is enabled on loads, by
//! way of `CLAUDE_CODE_PLUGIN_DIRS`; tabs of its own; and setup commands the
//! person approves and runs once per machine.
//!
//! Kits come from a local folder, linked in place, or from a GitHub link,
//! cloned shallow into `~/.hangar/kits/<id>` (store.rs, source.rs). Which
//! workspace has which kit is the workspace's business (state.json); this
//! side only ever hears kit ids, and resolves them to folders.
//!
//! The contract the other parts build on is written out in docs/kits.md.

mod manifest;
mod plugin;
mod setup;
mod source;
mod store;
#[cfg(test)]
mod testing;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

pub use manifest::Manifest;
use store::{Entry, Loaded, Source, Store, UpdateInfo};

/// What a manifest's `hangar` requirement is checked against.
pub const HANGAR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Sent with no payload whenever the list `kits_list` returns may have
/// changed: the index, or the manifest of an installed kit.
pub const CHANGED_EVENT: &str = "kits:changed";

/// `~/.hangar/kits/`, or beside the endpoint file a development build run
/// next to an installed Hangar was given, as for the bridge's mod (bridge.rs).
fn kits_root() -> Option<PathBuf> {
    if let Some(file) = std::env::var_os("HANGAR_ENDPOINT_FILE").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(file).parent()?.join("kits"));
    }
    Some(dirs::home_dir()?.join(".hangar").join("kits"))
}

fn open() -> Result<Store, String> {
    kits_root()
        .map(Store::at)
        .ok_or_else(|| "no home directory".to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// What a pane starts with
// ---------------------------------------------------------------------------

/// Why a kit a pane was given stays out of it. The pane starts all the same.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchIssue {
    pub id: String,
    pub reason: IssueReason,
    /// What went wrong, in English, for `broken`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueReason {
    /// Not installed on this machine.
    Missing,
    /// Installed, and unusable as it stands: its manifest, its folder, or the
    /// Hangar version it needs.
    Broken,
    /// The machine's Claude Code policy forbids sideloaded plugin folders.
    Forbidden,
}

impl LaunchIssue {
    pub fn new(id: &str, reason: IssueReason, detail: Option<String>) -> Self {
        LaunchIssue {
            id: id.to_string(),
            reason,
            detail,
        }
    }
}

/// The kits among `ids` that can be used, in the order given and each once,
/// and why the others cannot.
fn usable(store: &Store, ids: &[String]) -> (Vec<Loaded>, Vec<LaunchIssue>) {
    let mut kits: Vec<Loaded> = Vec::new();
    let mut issues = Vec::new();
    if ids.is_empty() {
        return (kits, issues);
    }
    let entries = match store.entries() {
        Ok(entries) => entries,
        Err(why) => {
            let issues = ids
                .iter()
                .map(|id| LaunchIssue::new(id, IssueReason::Broken, Some(why.clone())))
                .collect();
            return (kits, issues);
        }
    };
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let Some(entry) = entries.iter().find(|entry| &entry.id == id) else {
            issues.push(LaunchIssue::new(id, IssueReason::Missing, None));
            continue;
        };
        match store.loaded(entry) {
            Ok(kit) => kits.push(kit),
            Err(why) => issues.push(LaunchIssue::new(id, IssueReason::Broken, Some(why))),
        }
    }
    (kits, issues)
}

/// The kits' plugin folders for a pane, by kit id, and what kept any kit out.
#[derive(Debug, Default)]
pub struct Launch {
    pub plugins: Vec<(String, PathBuf)>,
    pub issues: Vec<LaunchIssue>,
}

fn launch_from(store: &Store, ids: &[String]) -> Launch {
    let (kits, mut issues) = usable(store, ids);
    let separator = if cfg!(windows) { ';' } else { ':' };
    let mut plugins = Vec::new();
    for kit in kits {
        let Some(dir) = kit.plugin_dir() else {
            continue;
        };
        if dir.to_string_lossy().contains(separator) {
            issues.push(LaunchIssue::new(
                &kit.entry.id,
                IssueReason::Broken,
                Some(format!(
                    "its plugin folder's path holds a `{separator}`, which CLAUDE_CODE_PLUGIN_DIRS cannot carry"
                )),
            ));
            continue;
        }
        plugins.push((kit.entry.id, dir));
    }
    Launch { plugins, issues }
}

/// What the kits `ids` add to a pane's Claude Code. Never fails: whatever is
/// wrong with a kit is an issue, and the pane starts without that kit.
pub fn launch(ids: &[String]) -> Launch {
    if ids.is_empty() {
        return Launch::default();
    }
    match open() {
        Ok(store) => launch_from(&store, ids),
        Err(why) => Launch {
            plugins: Vec::new(),
            issues: ids
                .iter()
                .map(|id| LaunchIssue::new(id, IssueReason::Broken, Some(why.clone())))
                .collect(),
        },
    }
}

/// The MCP servers of the kits `ids`, named and written out for a
/// `--strict-mcp-config` file (plugin.rs). Kits that cannot be used bring
/// none; the pane's launch reports them.
pub fn mcp_servers(ids: &[String]) -> Vec<(String, Value)> {
    if ids.is_empty() {
        return Vec::new();
    }
    let Ok(store) = open() else {
        return Vec::new();
    };
    plugin::scoped_servers(&usable(&store, ids).0)
}

// ---------------------------------------------------------------------------
// The commands
// ---------------------------------------------------------------------------

/// An installed kit, as Settings › Kits lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitInfo {
    pub id: String,
    pub source: Source,
    pub dir: String,
    pub installed_version: String,
    pub added_at: u64,
    /// Null when the manifest cannot be read or is invalid.
    pub manifest: Option<Manifest>,
    /// Why the kit cannot be used right now; null when it can. A manifest
    /// may be there with an error: a kit that needs a newer Hangar.
    pub error: Option<String>,
    /// The last update check, while the app runs; null before any.
    pub update: Option<UpdateInfo>,
}

fn info(store: &Store, entry: Entry) -> KitInfo {
    let (manifest, error) = match store.loaded(&entry) {
        Ok(kit) => (Some(kit.manifest), None),
        Err(why) => {
            // Read on its own, to say what it is even when it cannot be used.
            let manifest = manifest::read(Path::new(&entry.dir))
                .ok()
                .filter(|m| m.id == entry.id);
            (manifest, Some(why))
        }
    };
    KitInfo {
        update: store.update_info(&entry.id),
        id: entry.id,
        source: entry.source,
        dir: entry.dir,
        installed_version: entry.installed_version,
        added_at: entry.added_at,
        manifest,
        error,
    }
}

fn list(store: &Store) -> Result<Vec<KitInfo>, String> {
    Ok(store
        .entries()?
        .into_iter()
        .map(|entry| info(store, entry))
        .collect())
}

/// Tells the windows, and lets the watcher know this change was announced.
fn changed(app: &AppHandle) {
    remember_fingerprint();
    let _ = app.emit(CHANGED_EVENT, ());
}

#[tauri::command]
pub async fn kits_list() -> Result<Vec<KitInfo>, String> {
    crate::git::off_thread(|| list(&open()?)).await
}

#[tauri::command]
pub async fn kit_add_folder(app: AppHandle, path: String) -> Result<KitInfo, String> {
    crate::git::off_thread(move || {
        let store = open()?;
        let entry = store.add_folder(&path, now_ms())?;
        changed(&app);
        Ok(info(&store, entry))
    })
    .await
}

#[tauri::command]
pub async fn kit_add_git(app: AppHandle, url: String) -> Result<KitInfo, String> {
    crate::git::off_thread(move || {
        let store = open()?;
        let entry = store.add_git(&url, now_ms())?;
        changed(&app);
        Ok(info(&store, entry))
    })
    .await
}

#[tauri::command]
pub async fn kit_remove(app: AppHandle, id: String) -> Result<(), String> {
    crate::git::off_thread(move || {
        let result = open()?.remove(&id);
        // Out of the index even when its clone could not all be deleted.
        changed(&app);
        result
    })
    .await
}

#[tauri::command]
pub async fn kit_check_update(app: AppHandle, id: String) -> Result<Option<UpdateInfo>, String> {
    crate::git::off_thread(move || {
        let found = open()?.check_update(&id, now_ms())?;
        if found.is_some() {
            let _ = app.emit(CHANGED_EVENT, ());
        }
        Ok(found)
    })
    .await
}

#[tauri::command]
pub async fn kit_update(app: AppHandle, id: String) -> Result<KitInfo, String> {
    crate::git::off_thread(move || {
        let store = open()?;
        let entry = store.update(&id)?;
        changed(&app);
        Ok(info(&store, entry))
    })
    .await
}

#[tauri::command]
pub async fn kit_inspect(id: String) -> Result<plugin::Inspection, String> {
    crate::git::off_thread(move || {
        let store = open()?;
        let kit = store.loaded(&store.entry(&id)?)?;
        Ok(plugin::inspect(&kit, manifest::current_os()))
    })
    .await
}

#[tauri::command]
pub async fn kit_setup_status(id: String) -> Result<Vec<setup::StepStatus>, String> {
    crate::git::off_thread(move || setup::status(&open()?, &id, manifest::current_os())).await
}

#[tauri::command]
pub async fn kit_setup_mark(id: String, step_id: String) -> Result<Vec<setup::StepStatus>, String> {
    crate::git::off_thread(move || {
        setup::mark(&open()?, &id, &step_id, manifest::current_os(), now_ms())
    })
    .await
}

// ---------------------------------------------------------------------------
// Watching
// ---------------------------------------------------------------------------

/// How often the index and the manifests are looked at. A stat of a handful
/// of files: nothing worth a file-watching crate.
const WATCH_EVERY: Duration = Duration::from_secs(2);

/// The index's and every installed manifest's size and modification time.
fn fingerprint(store: &Store) -> Vec<(PathBuf, Option<(u64, SystemTime)>)> {
    let stamp = |path: PathBuf| {
        let found = std::fs::metadata(&path)
            .ok()
            .map(|meta| (meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)));
        (path, found)
    };
    let mut files = vec![stamp(store.index_path())];
    if let Ok(entries) = store.entries() {
        files.extend(
            entries
                .into_iter()
                .map(|entry| stamp(Path::new(&entry.dir).join(manifest::FILE))),
        );
    }
    files
}

type Fingerprint = Vec<(PathBuf, Option<(u64, SystemTime)>)>;

fn last_fingerprint() -> &'static Mutex<Option<Fingerprint>> {
    static LAST: OnceLock<Mutex<Option<Fingerprint>>> = OnceLock::new();
    LAST.get_or_init(Mutex::default)
}

fn remember_fingerprint() {
    if let Ok(store) = open() {
        *last_fingerprint()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(fingerprint(&store));
    }
}

/// Sends `kits:changed` when the index or a kit's manifest changes on disk —
/// a linked folder being edited, mostly.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        remember_fingerprint();
        loop {
            std::thread::sleep(WATCH_EVERY);
            let Ok(store) = open() else {
                continue;
            };
            let now = fingerprint(&store);
            let mut last = last_fingerprint()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if last.as_ref() != Some(&now) {
                let first = last.is_none();
                *last = Some(now);
                drop(last);
                if !first {
                    let _ = app.emit(CHANGED_EVENT, ());
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use testing::Folder;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn a_pane_gets_the_plugin_folders_of_its_kits_only() {
        let home = Folder::new("launch");
        let store = Store::at(home.root.join("kits"));
        let studio = Folder::new("launch-studio");
        let studio_dir = studio.kit(".", "studio", "1.0.0");
        let docs = Folder::new("launch-docs");
        docs.manifest(".", "tabs-only", "1.0.0");
        store.add_folder(&studio_dir.to_string_lossy(), 1).unwrap();
        store.add_folder(&docs.root.to_string_lossy(), 1).unwrap();

        let launch = launch_from(&store, &ids(&["studio", "tabs-only", "studio"]));
        assert_eq!(
            launch.plugins,
            vec![("studio".to_string(), studio_dir.clone())]
        );
        assert!(launch.issues.is_empty(), "{:?}", launch.issues);

        // Another workspace, no kit: nothing.
        let other = launch_from(&store, &[]);
        assert!(other.plugins.is_empty() && other.issues.is_empty());
    }

    #[test]
    fn a_missing_or_broken_kit_is_skipped_and_said() {
        let home = Folder::new("launch-broken");
        let store = Store::at(home.root.join("kits"));
        let good = Folder::new("launch-good");
        let good_dir = good.kit(".", "good", "1.0.0");
        let bad = Folder::new("launch-bad");
        bad.kit(".", "bad", "1.0.0");
        store.add_folder(&good_dir.to_string_lossy(), 1).unwrap();
        store.add_folder(&bad.root.to_string_lossy(), 1).unwrap();
        // Broken after it was added: a linked folder edited by hand.
        bad.file(manifest::FILE, "{ \"schema\": 1, \"id\": \"bad\" ");

        let launch = launch_from(&store, &ids(&["gone", "bad", "good"]));
        assert_eq!(launch.plugins, vec![("good".to_string(), good_dir)]);
        assert_eq!(launch.issues.len(), 2);
        assert_eq!(
            launch.issues[0],
            LaunchIssue::new("gone", IssueReason::Missing, None)
        );
        assert_eq!(launch.issues[1].reason, IssueReason::Broken);
        assert!(launch.issues[1]
            .detail
            .as_deref()
            .unwrap()
            .contains("not valid JSON"));

        // And the list still shows it, with its error.
        let listed = list(&store).unwrap();
        let bad_info = listed.iter().find(|kit| kit.id == "bad").unwrap();
        assert!(bad_info.manifest.is_none() && bad_info.error.is_some());
        assert!(listed
            .iter()
            .find(|kit| kit.id == "good")
            .unwrap()
            .error
            .is_none());
    }

    #[test]
    fn an_unreadable_index_keeps_every_kit_out_without_failing() {
        let home = Folder::new("launch-index");
        let store = Store::at(home.root.join("kits"));
        home.file("kits/index.json", "nope");
        let launch = launch_from(&store, &ids(&["studio"]));
        assert!(launch.plugins.is_empty());
        assert_eq!(launch.issues[0].reason, IssueReason::Broken);
    }

    #[test]
    fn the_watcher_sees_a_linked_manifest_change() {
        let home = Folder::new("watch");
        let store = Store::at(home.root.join("kits"));
        let kit = Folder::new("watch-src");
        kit.kit(".", "studio", "1.0.0");
        store.add_folder(&kit.root.to_string_lossy(), 1).unwrap();
        let before = fingerprint(&store);
        assert_eq!(before, fingerprint(&store));
        kit.file(
            manifest::FILE,
            "{ \"schema\": 1, \"id\": \"studio\", \"changed\": true }",
        );
        assert_ne!(before, fingerprint(&store));
    }
}
