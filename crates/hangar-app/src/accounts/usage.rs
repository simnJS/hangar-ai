//! The last usage each account reported, kept on disk
//! (`components/accounts/usageStore.ts`).
//!
//! A session counts for the account its pane was launched on, so the figures
//! come from the hangar-bridge's `usage` events (`accounts::on_event`). The web
//! app kept them in `localStorage`; the native app has none, so they live in
//! `<app data>/account-usage.json`, the same JSON, and a restart still has the
//! last known figures (the interface shows « unknown » for a window never
//! reported).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use hangar_core::Host;
use serde_json::Value;

use super::switch::{AccountUsage, Usage};

const FILE: &str = "account-usage.json";

/// Where the figures are kept; `None` when the system names no data folder.
pub fn path(host: &Host) -> Option<PathBuf> {
    host.paths()
        .app_data_dir()
        .ok()
        .map(|folder| folder.join(FILE))
}

/// What the file holds. A file that is missing, unreadable or not what it
/// should be reads as empty, and an account whose entry is off is left out
/// without costing the others: these are figures to show, never to fail on.
pub fn parse(text: &str) -> Usage {
    let Ok(Value::Object(accounts)) = serde_json::from_str::<Value>(text) else {
        return Usage::new();
    };
    accounts
        .into_iter()
        .filter_map(|(account, usage)| {
            serde_json::from_value::<AccountUsage>(usage)
                .ok()
                .map(|usage| (account, usage))
        })
        .collect()
}

pub fn load(path: &Path) -> Usage {
    fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Written to a temporary file renamed over the target, so a reader (and a
/// crash) gets one version or the other, whole.
pub fn save(path: &Path, usage: &Usage) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let body = serde_json::to_vec(usage).map_err(|err| err.to_string())?;
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_file_name(format!(
        ".{FILE}.{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, body).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        err.to_string()
    })
}

/// Whether a write is running, and whether the figures changed while it
/// did: two writes never run at once, so the last figures are always the
/// last written.
#[derive(Debug, Default)]
pub struct Writes {
    running: bool,
    again: bool,
}

impl Writes {
    /// A change to save. `true`: start a write now; `false`: one is running
    /// and will start another when it ends.
    pub fn wanted(&mut self) -> bool {
        if self.running {
            self.again = true;
            false
        } else {
            self.running = true;
            true
        }
    }

    /// The write ended. `true`: the figures changed meanwhile, write again.
    pub fn ended(&mut self) -> bool {
        if self.again {
            self.again = false;
            true
        } else {
            self.running = false;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::switch::WindowUsage;
    use super::*;

    fn window(percent_used: f64, at: u64) -> Option<WindowUsage> {
        Some(WindowUsage {
            percent_used,
            resets_at: None,
            at,
        })
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hangar-p8-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        fs::create_dir_all(&folder).expect("a scratch folder");
        folder
    }

    #[test]
    fn what_was_saved_is_read_back() {
        let folder = scratch("roundtrip");
        let file = folder.join(FILE);
        let usage = Usage::from([(
            "perso".to_string(),
            AccountUsage {
                five_hour: window(12.0, 5),
                weekly: window(30.5, 6),
            },
        )]);
        save(&file, &usage).expect("saves");
        assert_eq!(load(&file), usage);
        // No temporary file is left next to it.
        let names: Vec<_> = fs::read_dir(&folder)
            .expect("lists")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [FILE]);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn saving_creates_the_folder_and_replaces_the_file() {
        let folder = scratch("replace");
        let file = folder.join("data").join(FILE);
        save(&file, &Usage::new()).expect("creates the folder");
        let usage = Usage::from([(
            "main".to_string(),
            AccountUsage {
                five_hour: window(1.0, 1),
                weekly: None,
            },
        )]);
        save(&file, &usage).expect("replaces");
        assert_eq!(load(&file), usage);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_missing_or_broken_file_reads_as_empty() {
        let folder = scratch("broken");
        assert!(load(&folder.join(FILE)).is_empty());
        assert!(parse("").is_empty());
        assert!(parse("not json").is_empty());
        assert!(parse("[1, 2]").is_empty());
        assert!(parse("null").is_empty());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_account_whose_entry_is_off_costs_only_itself() {
        let usage = parse(
            r#"{"good":{"fiveHour":{"percentUsed":40,"resetsAt":null,"at":9}},
                "bad":{"fiveHour":"lots"},
                "empty":{}}"#,
        );
        assert_eq!(usage.len(), 2);
        assert_eq!(
            usage["good"]
                .five_hour
                .as_ref()
                .map(|window| window.percent_used),
            Some(40.0)
        );
        assert!(usage["empty"].five_hour.is_none());
        assert!(!usage.contains_key("bad"));
    }

    #[test]
    fn the_file_lives_in_the_app_data_folder() {
        let host = Host::with_paths(hangar_core::paths::Paths::rooted(Path::new("/data/root")));
        let file = path(&host).expect("a data folder");
        assert!(file.ends_with(FILE));
        assert!(file.starts_with("/data/root"));
    }

    #[test]
    fn two_writes_never_run_at_once_and_the_last_figures_are_written() {
        let mut writes = Writes::default();
        assert!(writes.wanted());
        assert!(!writes.wanted());
        assert!(!writes.wanted());
        // One more write covers both changes made while it ran.
        assert!(writes.ended());
        assert!(!writes.ended());
        // Idle again.
        assert!(writes.wanted());
    }
}
