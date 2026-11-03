//! Which setup steps already ran on this machine: `~/.hangar/kits/setup.json`.
//!
//! A step is recorded with the SHA-256 of the command it ran for this OS, so
//! it is not asked again while the command stays the same, and is asked again
//! as soon as a kit update changes it. Removing a kit forgets its steps: added
//! back, it asks again.

use std::fs;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::store::{write_atomic, Store};

const FILE: &str = "setup.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Records {
    #[serde(default)]
    done: Vec<Done>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Done {
    kit: String,
    step: String,
    sha256: String,
    /// Milliseconds since the epoch.
    at: u64,
}

/// One step as this machine would run it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStatus {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Exactly as the manifest writes it for this OS.
    pub command: String,
    pub sha256: String,
    /// Run already, with this very command.
    pub done: bool,
}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn path(store: &Store) -> std::path::PathBuf {
    store.root().join(FILE)
}

/// A missing or unreadable record is one where nothing ran: the worst it
/// costs is a step asked for again.
fn load(store: &Store) -> Records {
    fs::read_to_string(path(store))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(store: &Store, records: &Records) -> Result<(), String> {
    let body = serde_json::to_string_pretty(records).map_err(|e| e.to_string())?;
    write_atomic(&path(store), &body)
}

/// The kit's steps that have a command for `os`, in manifest order.
pub fn status(store: &Store, id: &str, os: &str) -> Result<Vec<StepStatus>, String> {
    let loaded = store.loaded(&store.entry(id)?)?;
    let records = load(store);
    Ok(loaded
        .manifest
        .setup
        .iter()
        .filter_map(|step| {
            let command = step.run.for_os(os)?.to_string();
            let sha256 = sha256_hex(&command);
            let done = records
                .done
                .iter()
                .any(|done| done.kit == id && done.step == step.id && done.sha256 == sha256);
            Some(StepStatus {
                id: step.id.clone(),
                title: step.title.clone(),
                description: step.description.clone(),
                command,
                sha256,
                done,
            })
        })
        .collect())
}

/// Records that `step` ran, with the command it has for `os` right now.
pub fn mark(
    store: &Store,
    id: &str,
    step: &str,
    os: &str,
    now: u64,
) -> Result<Vec<StepStatus>, String> {
    let steps = status(store, id, os)?;
    let Some(current) = steps.iter().find(|s| s.id == step) else {
        return Err(format!(
            "the kit {id:?} has no setup step {step:?} for {os}"
        ));
    };
    let mut records = load(store);
    records
        .done
        .retain(|done| !(done.kit == id && done.step == step));
    records.done.push(Done {
        kit: id.to_string(),
        step: step.to_string(),
        sha256: current.sha256.clone(),
        at: now,
    });
    save(store, &records)?;
    status(store, id, os)
}

/// Drops every record of a kit.
pub fn forget(store: &Store, id: &str) {
    let mut records = load(store);
    let before = records.done.len();
    records.done.retain(|done| done.kit != id);
    if records.done.len() != before {
        let _ = save(store, &records);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::manifest;
    use crate::kits::testing::Folder;
    use serde_json::json;

    fn kit_with_steps(folder: &Folder, windows: &str) {
        folder.file(
            manifest::FILE,
            &json!({
                "schema": 1, "id": "studio", "name": "Studio", "version": "1.0.0",
                "description": "d",
                "setup": [
                    { "id": "bridge", "title": "Bridge", "run": { "windows": windows, "linux": "./install.sh" } },
                    { "id": "mac-only", "title": "Mac", "run": { "macos": "brew install studio" } }
                ]
            })
            .to_string(),
        );
    }

    #[test]
    fn a_step_is_done_until_its_command_changes() {
        let home = Folder::new("setup");
        let store = Store::at(home.root.join("kits"));
        let source = Folder::new("setup-src");
        kit_with_steps(&source, "npx bridge install");
        store.add_folder(&source.root.to_string_lossy(), 1).unwrap();

        let steps = status(&store, "studio", "windows").unwrap();
        assert_eq!(steps.len(), 1, "only the steps this OS has a command for");
        assert_eq!(steps[0].command, "npx bridge install");
        assert_eq!(steps[0].sha256, sha256_hex("npx bridge install"));
        assert!(!steps[0].done);
        assert_eq!(status(&store, "studio", "macos").unwrap()[0].id, "mac-only");

        let marked = mark(&store, "studio", "bridge", "windows", 5).unwrap();
        assert!(marked[0].done);
        assert!(
            !status(&store, "studio", "linux").unwrap()[0].done,
            "per OS command"
        );

        // An update changes the command: asked again.
        kit_with_steps(&source, "npx bridge@2 install");
        assert!(!status(&store, "studio", "windows").unwrap()[0].done);
        mark(&store, "studio", "bridge", "windows", 6).unwrap();
        assert!(status(&store, "studio", "windows").unwrap()[0].done);
        assert_eq!(load(&store).done.len(), 1, "one record per step");

        assert!(mark(&store, "studio", "mac-only", "windows", 7).is_err());
        assert!(mark(&store, "studio", "nope", "windows", 7).is_err());

        store.remove("studio").unwrap();
        assert!(load(&store).done.is_empty(), "a removed kit is forgotten");
    }

    #[test]
    fn the_hash_is_sha256_of_the_command_as_written() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(sha256_hex("npm i"), sha256_hex("npm i "));
    }
}
