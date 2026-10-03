//! The other end of the `hangar-bridge` Claude Code mod.
//!
//! Hangar launches Claude Code itself, so it can load a mod into every pane's
//! session without the user installing anything: the mod's files ship inside
//! this binary, are written under `~/.hangar/mods/` on first use, and reach
//! Claude Code through `CLAUDE_CODE_PLUGIN_DIRS` in the pane's environment,
//! along with the pane id and the loopback API's URL and token.
//!
//! From then on the mod reports what the session does — turns, permission
//! dialogs, context and plan usage, edits — to the API, which relays each
//! report to the window as a `bridge:event`. The mod also pulls prompts queued
//! here for its pane, and submits them once its session is idle.
//!
//! Only Claude Code has mods. Every other agent, and a Claude Code that cannot
//! load one (too old, mods turned off, a WSL shell the environment does not
//! cross into), simply never reports, and the pane keeps reading its state from
//! the shape of its output.
//!
//! One policy is harsher: with sideloaded plugin directories forbidden, Claude
//! Code refuses to start at all while `CLAUDE_CODE_PLUGIN_DIRS` is set. The
//! machine's managed settings file is read for it, and the variable is then
//! left out; a policy that arrives some other way (from the organisation's
//! servers) is what the setting that turns the bridge off is for.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use portable_pty::CommandBuilder;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The mod, as `claude --plugin-dir` expects to find it on disk.
const MOD_FILES: &[(&str, &str)] = &[
    (
        ".claude-plugin/plugin.json",
        include_str!("../claude-mod/hangar-bridge/.claude-plugin/plugin.json"),
    ),
    (
        "hooks/hooks.json",
        include_str!("../claude-mod/hangar-bridge/hooks/hooks.json"),
    ),
    (
        "hooks/register.js",
        include_str!("../claude-mod/hangar-bridge/hooks/register.js"),
    ),
];

/// How long an edit keeps its file "being edited" by the pane that made it.
const EDIT_MEMORY_MS: u64 = 15 * 60 * 1000;

/// Where the mod was written, or None when that failed: panes then start
/// without it.
pub fn plugin_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(install).as_deref()
}

/// Rewrites only the files that changed, so an app update reaches the mod —
/// Claude Code hot-reloads a plugin directory when its files change — while an
/// ordinary launch touches nothing.
fn install() -> Option<PathBuf> {
    let dir = dirs::home_dir()?
        .join(".hangar")
        .join("mods")
        .join("hangar-bridge");
    for (relative, body) in MOD_FILES {
        let path = dir.join(relative);
        if fs::read_to_string(&path).ok().as_deref() == Some(*body) {
            continue;
        }
        fs::create_dir_all(path.parent()?).ok()?;
        fs::write(&path, body).ok()?;
    }
    Some(dir)
}

/// The loopback API, once it is listening. Set by the setup hook.
static ENDPOINT: OnceLock<(u16, String)> = OnceLock::new();

pub fn set_endpoint(port: u16, token: &str) {
    let _ = ENDPOINT.set((port, token.to_string()));
}

/// Everything a pane's environment needs for its Claude Code to load the mod
/// and find its way back here. Nothing is set when either half is missing: a
/// mod with no URL would only make requests that fail.
pub fn apply_env(cmd: &mut CommandBuilder, pane_id: &str, pane_name: Option<&str>) {
    let (Some(dir), Some((port, token))) = (plugin_dir(), ENDPOINT.get()) else {
        return;
    };
    if sideloading_forbidden() {
        return;
    }
    let separator = if cfg!(windows) { ";" } else { ":" };
    // Directories the user loads this way already stay loaded.
    let dirs = match std::env::var("CLAUDE_CODE_PLUGIN_DIRS") {
        Ok(existing) if !existing.trim().is_empty() => {
            format!("{}{separator}{existing}", dir.display())
        }
        _ => dir.display().to_string(),
    };
    cmd.env("CLAUDE_CODE_PLUGIN_DIRS", dirs);
    cmd.env("HANGAR_PANE_ID", pane_id);
    if let Some(name) = pane_name.filter(|name| !name.trim().is_empty()) {
        cmd.env("HANGAR_PANE_NAME", name);
    }
    cmd.env("HANGAR_BRIDGE_URL", format!("http://127.0.0.1:{port}"));
    cmd.env("HANGAR_BRIDGE_TOKEN", token);
}

/// Where an administrator puts Claude Code's managed settings on this OS.
fn managed_settings_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        ["ProgramFiles", "ProgramData"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|root| {
                PathBuf::from(root)
                    .join("ClaudeCode")
                    .join("managed-settings.json")
            })
            .collect()
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Library/Application Support/ClaudeCode/managed-settings.json",
        )]
    } else {
        vec![PathBuf::from("/etc/claude-code/managed-settings.json")]
    }
}

fn forbids_sideloading(settings: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(settings)
        .ok()
        .and_then(|value| value.get("disableSideloadFlags")?.as_bool())
        .unwrap_or(false)
}

/// Read once: a policy change reaches Claude Code at its next start anyway,
/// and so it reaches Hangar at its own.
fn sideloading_forbidden() -> bool {
    static FORBIDDEN: OnceLock<bool> = OnceLock::new();
    *FORBIDDEN.get_or_init(|| {
        managed_settings_paths()
            .iter()
            .filter_map(|path| fs::read_to_string(path).ok())
            .any(|settings| forbids_sideloading(&settings))
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Prompts queued for a pane until its agent is free
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Queued {
    pub id: String,
    pub text: String,
}

#[derive(Default)]
struct Inbox {
    by_pane: HashMap<String, VecDeque<Queued>>,
}

impl Inbox {
    fn push(&mut self, pane_id: &str, text: String) -> Queued {
        let queued = Queued {
            id: uuid::Uuid::new_v4().to_string(),
            text,
        };
        self.by_pane
            .entry(pane_id.to_string())
            .or_default()
            .push_back(queued.clone());
        queued
    }

    fn pop(&mut self, pane_id: &str) -> Option<Queued> {
        let queue = self.by_pane.get_mut(pane_id)?;
        let next = queue.pop_front();
        if queue.is_empty() {
            self.by_pane.remove(pane_id);
        }
        next
    }

    /// One message by id, or the whole queue when `id` is None.
    fn cancel(&mut self, pane_id: &str, id: Option<&str>) {
        match id {
            None => {
                self.by_pane.remove(pane_id);
            }
            Some(id) => {
                if let Some(queue) = self.by_pane.get_mut(pane_id) {
                    queue.retain(|queued| queued.id != id);
                    if queue.is_empty() {
                        self.by_pane.remove(pane_id);
                    }
                }
            }
        }
    }

    fn pending(&self, pane_id: &str) -> Vec<Queued> {
        self.by_pane
            .get(pane_id)
            .map(|queue| queue.iter().cloned().collect())
            .unwrap_or_default()
    }
}

fn inbox() -> MutexGuard<'static, Inbox> {
    static INBOX: OnceLock<Mutex<Inbox>> = OnceLock::new();
    INBOX
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueueChanged {
    pane_id: String,
    pending: Vec<Queued>,
}

fn queue_changed(app: &AppHandle, pane_id: &str, pending: Vec<Queued>) {
    let _ = app.emit(
        "bridge:queue",
        QueueChanged {
            pane_id: pane_id.to_string(),
            pending,
        },
    );
}

#[tauri::command]
pub fn bridge_enqueue(app: AppHandle, pane_id: String, text: String) -> Queued {
    let (queued, pending) = {
        let mut inbox = inbox();
        let queued = inbox.push(&pane_id, text);
        (queued, inbox.pending(&pane_id))
    };
    queue_changed(&app, &pane_id, pending);
    queued
}

#[tauri::command]
pub fn bridge_cancel(app: AppHandle, pane_id: String, id: Option<String>) {
    let pending = {
        let mut inbox = inbox();
        inbox.cancel(&pane_id, id.as_deref());
        inbox.pending(&pane_id)
    };
    queue_changed(&app, &pane_id, pending);
}

#[tauri::command]
pub fn bridge_queue(pane_id: String) -> Vec<Queued> {
    inbox().pending(&pane_id)
}

/// The mod's pull: the oldest prompt for its pane, handed out once.
pub fn take_next(app: &AppHandle, pane_id: &str) -> Option<Queued> {
    let (next, pending) = {
        let mut inbox = inbox();
        let next = inbox.pop(pane_id);
        (next, inbox.pending(pane_id))
    };
    if next.is_some() {
        queue_changed(app, pane_id, pending);
    }
    next
}

/// A pane that is gone takes its queue and its edits with it.
pub fn forget(pane_id: &str) {
    inbox().cancel(pane_id, None);
    edits().forget(pane_id);
}

// ---------------------------------------------------------------------------
// Who edited which file last
// ---------------------------------------------------------------------------

struct Edit {
    pane_id: String,
    pane_name: String,
    at: u64,
}

#[derive(Default)]
struct Edits {
    by_file: HashMap<String, Edit>,
}

/// Same file, same key: Windows paths compare without case and with either
/// slash, which is how the tools may spell them from one call to the next.
fn file_key(file: &str) -> String {
    let unified = file.replace('\\', "/");
    if cfg!(windows) {
        unified.to_lowercase()
    } else {
        unified
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub pane_id: String,
    pub name: String,
    pub ago_ms: u64,
}

impl Edits {
    fn record(&mut self, pane_id: &str, pane_name: &str, file: &str, at: u64) {
        self.by_file
            .retain(|_, edit| at.saturating_sub(edit.at) < EDIT_MEMORY_MS);
        self.by_file.insert(
            file_key(file),
            Edit {
                pane_id: pane_id.to_string(),
                pane_name: pane_name.to_string(),
                at,
            },
        );
    }

    /// Another pane's recent edit of this file, if there is one.
    fn owner(&self, pane_id: &str, file: &str, now: u64) -> Option<Owner> {
        let edit = self.by_file.get(&file_key(file))?;
        let ago_ms = now.saturating_sub(edit.at);
        (edit.pane_id != pane_id && ago_ms < EDIT_MEMORY_MS).then(|| Owner {
            pane_id: edit.pane_id.clone(),
            name: edit.pane_name.clone(),
            ago_ms,
        })
    }

    fn forget(&mut self, pane_id: &str) {
        self.by_file.retain(|_, edit| edit.pane_id != pane_id);
    }
}

fn edits() -> MutexGuard<'static, Edits> {
    static EDITS: OnceLock<Mutex<Edits>> = OnceLock::new();
    EDITS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn record_edit(pane_id: &str, pane_name: &str, file: &str) {
    edits().record(pane_id, pane_name, file, now_ms());
}

pub fn conflict(pane_id: &str, file: &str) -> Option<Owner> {
    edits().owner(pane_id, file, now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_hands_out_in_order_and_forgets_empty_panes() {
        let mut inbox = Inbox::default();
        let first = inbox.push("a", "one".into());
        inbox.push("a", "two".into());
        assert_eq!(inbox.pop("a"), Some(first));
        assert_eq!(inbox.pending("a").len(), 1);
        assert_eq!(inbox.pop("a").map(|q| q.text), Some("two".into()));
        assert_eq!(inbox.pop("a"), None);
        assert!(inbox.by_pane.is_empty());
    }

    #[test]
    fn inbox_cancels_one_or_all() {
        let mut inbox = Inbox::default();
        let first = inbox.push("a", "one".into());
        inbox.push("a", "two".into());
        inbox.cancel("a", Some(&first.id));
        assert_eq!(inbox.pending("a").len(), 1);
        inbox.cancel("a", None);
        assert!(inbox.pending("a").is_empty());
    }

    #[test]
    fn an_edit_belongs_to_another_pane_only_while_recent() {
        let mut edits = Edits::default();
        edits.record("a", "alice", "/repo/src/App.tsx", 1_000);
        assert_eq!(edits.owner("a", "/repo/src/App.tsx", 2_000), None);
        let owner = edits.owner("b", "/repo/src/App.tsx", 61_000).unwrap();
        assert_eq!((owner.name.as_str(), owner.ago_ms), ("alice", 60_000));
        assert_eq!(
            edits.owner("b", "/repo/src/App.tsx", 1_000 + EDIT_MEMORY_MS),
            None
        );
    }

    #[test]
    fn only_an_explicit_true_forbids_sideloading() {
        assert!(forbids_sideloading(r#"{ "disableSideloadFlags": true }"#));
        assert!(!forbids_sideloading(r#"{ "disableSideloadFlags": false }"#));
        assert!(!forbids_sideloading(r#"{ "permissions": {} }"#));
        assert!(!forbids_sideloading("not json"));
    }

    #[test]
    fn a_closed_pane_releases_its_files() {
        let mut edits = Edits::default();
        edits.record("a", "alice", "/repo/x", 0);
        edits.forget("a");
        assert_eq!(edits.owner("b", "/repo/x", 1), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_match_across_case_and_slashes() {
        let mut edits = Edits::default();
        edits.record("a", "alice", r"C:\Repo\src\App.tsx", 0);
        assert!(edits.owner("b", "c:/repo/src/app.tsx", 1).is_some());
    }
}
