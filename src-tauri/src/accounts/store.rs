//! The profiles Hangar made, and their names: `<root>/accounts.json` beside
//! one folder per profile, `<root>/<id>/`.
//!
//! The index is read defensively — it is a file a person may edit — and an
//! entry that does not hold together is skipped rather than trusted. Its
//! `dir` is written for whoever reads the file; Hangar always works from
//! `<root>/<id>`, so a hand-edited `dir` can never point a removal, or a
//! pane, anywhere else.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{json, Value};

use super::MAIN;

const INDEX: &str = "accounts.json";
/// What the main account is called until the person names it.
pub const MAIN_LABEL: &str = "Main";
const MAX_LABEL: usize = 80;
const MAX_ID: usize = 40;
/// Where a removed profile waits to be deleted (see `Store::trash`).
const TRASH_PREFIX: &str = ".removed-";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub label: String,
    pub dir: String,
    /// Epoch milliseconds.
    pub added_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Index {
    /// The main account's name, once the person gave it one.
    pub main_label: Option<String>,
    pub accounts: Vec<Entry>,
}

impl Index {
    pub fn main_label(&self) -> &str {
        self.main_label.as_deref().unwrap_or(MAIN_LABEL)
    }

    fn labels(&self) -> impl Iterator<Item = (&str, &str)> {
        std::iter::once((MAIN, self.main_label())).chain(
            self.accounts
                .iter()
                .map(|entry| (entry.id.as_str(), entry.label.as_str())),
        )
    }

    /// Another account already goes by `label`, whatever its case.
    fn label_taken(&self, label: &str, except: Option<&str>) -> bool {
        let wanted = label.to_lowercase();
        self.labels()
            .any(|(id, taken)| Some(id) != except && taken.to_lowercase() == wanted)
    }
}

/// An id is a short name of lowercase letters, digits and dashes: a folder
/// name on every system, and readable in a hand-edited state.json.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id != MAIN
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A label as it is kept: trimmed, not empty, not endless.
pub fn clean_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Give the account a name.".into());
    }
    if label.chars().count() > MAX_LABEL {
        return Err(format!(
            "An account name has {MAX_LABEL} characters at most."
        ));
    }
    Ok(label.to_string())
}

/// `Work`, `Société` → `work`, `societe`: the label as an id, accents
/// dropped, anything else a dash.
fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.to_lowercase().chars() {
        let c = match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ÿ' => 'y',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(24).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "account".to_string()
    } else {
        out
    }
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn at(root: PathBuf) -> Self {
        Store { root }
    }

    pub fn dir_of(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The index as it stands: missing is empty, unreadable is an error —
    /// writing over a file nobody could read would lose every profile in it.
    pub fn read(&self) -> Result<Index, String> {
        let raw = match fs::read_to_string(self.root.join(INDEX)) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Index::default()),
            Err(err) => return Err(format!("The accounts index cannot be read: {err}")),
        };
        let value: Value =
            serde_json::from_str(raw.trim_start_matches('\u{feff}')).map_err(|_| {
                "The accounts index is not valid JSON: fix it or delete it.".to_string()
            })?;
        Ok(self.parse(&value))
    }

    fn parse(&self, value: &Value) -> Index {
        let main_label = value
            .get("mainLabel")
            .and_then(Value::as_str)
            .and_then(|label| clean_label(label).ok());
        let mut accounts: Vec<Entry> = Vec::new();
        for item in value
            .get("accounts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = item.get("id").and_then(Value::as_str) else {
                continue;
            };
            if !valid_id(id) || accounts.iter().any(|entry| entry.id == id) {
                continue;
            }
            let label = item
                .get("label")
                .and_then(Value::as_str)
                .and_then(|label| clean_label(label).ok())
                .unwrap_or_else(|| id.to_string());
            let added_at = item.get("addedAt").and_then(Value::as_u64).unwrap_or(0);
            accounts.push(Entry {
                id: id.to_string(),
                label,
                dir: self.dir_of(id).to_string_lossy().into_owned(),
                added_at,
            });
        }
        Index {
            main_label,
            accounts,
        }
    }

    pub fn write(&self, index: &Index) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        let body = json!({
            "mainLabel": index.main_label,
            "accounts": index.accounts,
        });
        let body = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
        write_atomic(&self.root.join(INDEX), body.as_bytes())
    }

    /// A new profile in the index, named `label`. Its folder is the caller's
    /// to prepare.
    pub fn add(&self, label: &str, now: u64) -> Result<Entry, String> {
        let label = clean_label(label)?;
        let mut index = self.read()?;
        if index.label_taken(&label, None) {
            return Err("Another account already has this name.".into());
        }
        let base = slug(&label);
        let free = |id: &str| {
            id != MAIN
                && valid_id(id)
                && !index.accounts.iter().any(|entry| entry.id == id)
                && !super::link::exists(&self.dir_of(id))
        };
        let id = std::iter::once(base.clone())
            .chain((2..).map(|n| format!("{base}-{n}")))
            .find(|id| free(id))
            .expect("an unbounded list of names has a free one");
        let entry = Entry {
            dir: self.dir_of(&id).to_string_lossy().into_owned(),
            id,
            label,
            added_at: now,
        };
        index.accounts.push(entry.clone());
        self.write(&index)?;
        Ok(entry)
    }

    /// Renames an account, the main one included. Returns the new label.
    pub fn rename(&self, id: &str, label: &str) -> Result<String, String> {
        let label = clean_label(label)?;
        let mut index = self.read()?;
        if index.label_taken(&label, Some(id)) {
            return Err("Another account already has this name.".into());
        }
        if id == MAIN {
            index.main_label = Some(label.clone());
        } else {
            let entry = index
                .accounts
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or("No such account.")?;
            entry.label = label.clone();
        }
        self.write(&index)?;
        Ok(label)
    }

    /// Takes the profile out of the index.
    pub fn forget(&self, id: &str) -> Result<(), String> {
        let mut index = self.read()?;
        let before = index.accounts.len();
        index.accounts.retain(|entry| entry.id != id);
        if index.accounts.len() == before {
            return Err("No such account.".into());
        }
        self.write(&index)
    }

    /// Where a profile is moved before it is deleted: renamed in one step,
    /// so a profile still in use (Windows refuses to rename a folder with a
    /// file open in it) stays whole instead of half deleted.
    pub fn trash(&self, id: &str, now: u64) -> PathBuf {
        self.root.join(format!("{TRASH_PREFIX}{id}-{now}"))
    }

    /// The profiles moved aside and not deleted yet.
    pub fn trashed(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(TRASH_PREFIX)
            })
            .map(|entry| entry.path())
            .collect()
    }
}

/// Written to a temporary file renamed over the target, so a reader gets one
/// version or the other, whole — and a link at `path` is replaced, never
/// written through.
pub fn write_atomic(path: &Path, body: &[u8]) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(
        ".{name}.hangar-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    // What is copied here can hold tokens (an MCP server's environment), so
    // it is kept as private as Claude Code keeps its own files.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::testing::Scratch;

    #[test]
    fn ids_come_from_labels_and_never_collide() {
        let scratch = Scratch::new("store-ids");
        let store = Store::at(scratch.root.join("accounts"));
        assert_eq!(store.add("Work", 1).unwrap().id, "work");
        assert_eq!(
            store.add("Société Générale", 2).unwrap().id,
            "societe-generale"
        );
        // The main account's id is never handed out, nor a folder left behind.
        assert_eq!(
            store.add("main", 3).unwrap_err(),
            "Another account already has this name."
        );
        assert_eq!(store.add("Main pro", 4).unwrap().id, "main-pro");
        fs::create_dir_all(store.dir_of("perso")).unwrap();
        assert_eq!(store.add("Perso", 5).unwrap().id, "perso-2");
        assert_eq!(store.add("!!!", 6).unwrap().id, "account");
        let ids: Vec<String> = store
            .read()
            .unwrap()
            .accounts
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(
            ids,
            ["work", "societe-generale", "main-pro", "perso-2", "account"]
        );
    }

    #[test]
    fn labels_are_trimmed_unique_and_renamable() {
        let scratch = Scratch::new("store-labels");
        let store = Store::at(scratch.root.join("accounts"));
        let work = store.add("  Work ", 1).unwrap();
        assert_eq!(work.label, "Work");
        assert!(store.add("WORK", 2).is_err());
        assert!(store.add("   ", 2).is_err());
        assert!(store.add(&"x".repeat(81), 2).is_err());

        assert_eq!(store.rename("work", "Pro").unwrap(), "Pro");
        assert_eq!(store.rename("main", "Perso").unwrap(), "Perso");
        assert!(store.rename("work", "perso").is_err());
        assert!(store.rename("gone", "Other").is_err());
        let index = store.read().unwrap();
        assert_eq!(index.main_label(), "Perso");
        assert_eq!(index.accounts[0].label, "Pro");
        assert_eq!(index.accounts[0].id, "work");
    }

    #[test]
    fn a_hand_edited_index_is_read_defensively() {
        let scratch = Scratch::new("store-defensive");
        let store = Store::at(scratch.root.join("accounts"));
        scratch.file(
            "accounts/accounts.json",
            &json!({
                "mainLabel": "   ",
                "accounts": [
                    { "id": "work", "label": "Work", "dir": "C:/Windows", "addedAt": 5 },
                    { "id": "work", "label": "Duplicate" },
                    { "id": "main", "label": "Not main" },
                    { "id": "../escape", "label": "Escape" },
                    { "id": "Upper", "label": "Upper" },
                    { "label": "No id" },
                    { "id": "perso", "label": 3 },
                    "nonsense"
                ]
            })
            .to_string(),
        );
        let index = store.read().unwrap();
        assert_eq!(index.main_label(), MAIN_LABEL);
        let ids: Vec<&str> = index.accounts.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["work", "perso"]);
        // Its folder is always the store's, whatever the file says.
        assert_eq!(PathBuf::from(&index.accounts[0].dir), store.dir_of("work"));
        assert_eq!(index.accounts[1].label, "perso");

        // Broken JSON is an error, and nothing is written over it.
        scratch.file("accounts/accounts.json", "{ not json");
        assert!(store.read().is_err());
        assert!(store.add("New", 1).is_err());
        assert_eq!(
            fs::read_to_string(scratch.root.join("accounts/accounts.json")).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn forgetting_takes_only_the_entry() {
        let scratch = Scratch::new("store-forget");
        let store = Store::at(scratch.root.join("accounts"));
        store.add("Work", 1).unwrap();
        store.add("Perso", 2).unwrap();
        store.forget("work").unwrap();
        assert!(store.forget("work").is_err());
        let ids: Vec<String> = store
            .read()
            .unwrap()
            .accounts
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, ["perso"]);
    }
}
