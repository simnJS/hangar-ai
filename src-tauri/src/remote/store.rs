//! What Hangar Mobile keeps on disk, in `remote.json` beside state.json:
//! whether it is on, the port Tailscale Serve proxies to, and the phones
//! paired with this PC.
//!
//! A phone is known by the SHA-256 of the token in its cookie. The token
//! itself only ever exists on the phone: a copy of this file opens nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Fixed rather than ephemeral like the agents' API: `tailscale serve` is
/// told this port once, and keeps proxying to it across launches.
pub const DEFAULT_PORT: u16 = 47821;

/// A phone's last visit is kept to the minute. A phone asks something every
/// few seconds while it is open, and each of those writing the file would be
/// a write for nothing anyone reads that finely.
const SEEN_EVERY_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    /// Off until the person turns it on; at launch, on again if they left it on.
    pub enabled: bool,
    pub port: u16,
    pub devices: Vec<Device>,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            devices: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// SHA-256 of the token in the phone's cookie, in hex.
    pub token_hash: String,
    /// Milliseconds since the epoch, like the board's stamps.
    pub created: u64,
    pub last_seen: u64,
}

/// A phone as the settings list it: everything but the hash.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub created: u64,
    pub last_seen: u64,
}

impl Device {
    pub fn view(&self) -> DeviceView {
        DeviceView {
            id: self.id.clone(),
            name: self.name.clone(),
            created: self.created,
            last_seen: self.last_seen,
        }
    }
}

pub fn token_hash(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub struct Store {
    file: PathBuf,
    saved: Mutex<Saved>,
}

impl Store {
    pub fn open(file: PathBuf) -> Self {
        let saved = load(&file);
        Self {
            file,
            saved: Mutex::new(saved),
        }
    }

    pub fn read(&self) -> Saved {
        self.saved.lock().unwrap().clone()
    }

    #[cfg(test)]
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Applies `change` and saves it. Nothing changes in memory unless the
    /// file was written: a phone paired on a disk that refused the write
    /// would hold a cookie that stops working at the next launch.
    pub fn update<T>(&self, change: impl FnOnce(&mut Saved) -> T) -> Result<T, String> {
        let mut saved = self.saved.lock().unwrap();
        let mut next = saved.clone();
        let outcome = change(&mut next);
        write(&self.file, &next)?;
        *saved = next;
        Ok(outcome)
    }

    /// The phone whose cookie carries `token`, its last visit brought up to
    /// `now`. Every hash is compared in constant time, so how long the answer
    /// takes says nothing about how close a guess came.
    pub fn find(&self, token: &str, now: u64) -> Option<DeviceView> {
        let hash = token_hash(token);
        let mut saved = self.saved.lock().unwrap();
        let device = saved
            .devices
            .iter_mut()
            .find(|device| bool::from(device.token_hash.as_bytes().ct_eq(hash.as_bytes())))?;
        let stale = now.saturating_sub(device.last_seen) >= SEEN_EVERY_MS;
        if stale {
            device.last_seen = now;
        }
        let view = device.view();
        if stale {
            // A refused write keeps the visit in memory, and the next one a
            // minute on tries the file again.
            let _ = write(&self.file, &saved);
        }
        Some(view)
    }
}

fn load(file: &Path) -> Saved {
    let Ok(raw) = fs::read_to_string(file) else {
        return Saved::default();
    };
    match serde_json::from_str(&raw) {
        Ok(saved) => saved,
        Err(err) => {
            // Moved aside, the way state.json is: the next save would
            // otherwise write over it. Starting again from the defaults turns
            // Hangar Mobile off and forgets the phones, which pair again —
            // never the other way round, a damaged file letting one in.
            match crate::store::quarantine(file) {
                Some(kept) => eprintln!(
                    "remote.json is unreadable ({err}); kept as {}",
                    kept.display()
                ),
                None => eprintln!("remote.json is unreadable ({err}); it could not be moved aside"),
            }
            Saved::default()
        }
    }
}

fn write(file: &Path, saved: &Saved) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)
            .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    }
    let body = serde_json::to_string_pretty(saved).map_err(|err| err.to_string())?;
    // Write-then-rename, as state.json is saved: a crash mid-write cannot
    // leave half a file.
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|err| format!("could not save remote.json: {err}"))?;
    fs::rename(&tmp, file).map_err(|err| format!("could not save remote.json: {err}"))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// A remote.json of its own, in a folder no other test uses.
    pub fn scratch_file(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "hangar-remote-{tag}-{}",
                uuid::Uuid::new_v4().simple()
            ))
            .join("remote.json")
    }

    fn phone(token: &str, last_seen: u64) -> Device {
        Device {
            id: "phone-1".into(),
            name: "Pixel".into(),
            token_hash: token_hash(token),
            created: 1_000,
            last_seen,
        }
    }

    #[test]
    fn a_first_launch_is_off_on_the_default_port() {
        let store = Store::open(scratch_file("default"));
        assert_eq!(store.read(), Saved::default());
        assert!(!store.read().enabled);
        assert_eq!(store.read().port, 47821);
    }

    #[test]
    fn what_is_saved_comes_back_and_the_token_never_reaches_the_disk() {
        let file = scratch_file("round-trip");
        let store = Store::open(file.clone());
        store
            .update(|saved| {
                saved.enabled = true;
                saved.devices.push(phone("the-secret-token", 1_000));
            })
            .unwrap();

        let reopened = Store::open(file.clone());
        assert_eq!(reopened.read(), store.read());
        let raw = fs::read_to_string(&file).unwrap();
        assert!(!raw.contains("the-secret-token"));
        assert!(raw.contains(&token_hash("the-secret-token")));
        assert!(raw.contains("\"tokenHash\"") && raw.contains("\"lastSeen\""));
    }

    #[test]
    fn a_token_finds_its_phone_and_nothing_else_does() {
        let store = Store::open(scratch_file("find"));
        store
            .update(|saved| saved.devices.push(phone("right", 1_000)))
            .unwrap();
        assert_eq!(
            store.find("right", 2_000).map(|d| d.id),
            Some("phone-1".into())
        );
        assert_eq!(store.find("wrong", 2_000), None);
        assert_eq!(store.find("", 2_000), None);
        assert_eq!(store.find(&token_hash("right"), 2_000), None);
    }

    #[test]
    fn a_visit_is_recorded_to_the_minute() {
        let file = scratch_file("seen");
        let store = Store::open(file.clone());
        store
            .update(|saved| saved.devices.push(phone("token", 1_000)))
            .unwrap();

        // Within the minute: neither in memory nor on disk.
        assert_eq!(store.find("token", 30_000).unwrap().last_seen, 1_000);
        // A minute on: both.
        assert_eq!(store.find("token", 61_000).unwrap().last_seen, 61_000);
        assert_eq!(Store::open(file).read().devices[0].last_seen, 61_000);
    }

    #[test]
    fn an_unreadable_file_is_kept_aside_and_starts_off() {
        let file = scratch_file("corrupt");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{ not json").unwrap();

        let store = Store::open(file.clone());
        assert_eq!(store.read(), Saved::default());
        assert!(!file.exists());
        let kept = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains("corrupt"));
        assert!(kept);
    }

    #[test]
    fn an_older_file_missing_fields_takes_the_defaults() {
        let file = scratch_file("partial");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, r#"{ "enabled": true }"#).unwrap();
        let saved = Store::open(file).read();
        assert!(saved.enabled);
        assert_eq!(saved.port, DEFAULT_PORT);
        assert!(saved.devices.is_empty());
    }
}
