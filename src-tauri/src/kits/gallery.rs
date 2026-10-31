//! The kit gallery: the public kits of this repository, listed by a static file.
//!
//! No server of ours stands behind it (no hosted service, ever): the list is
//! `kits/index.json` of the public repository, read from raw.githubusercontent.com
//! over https. That one URL is the only thing fetched — no icon, no manifest, no
//! page. The answer is size-capped and parsed defensively (an entry that is not
//! well formed is dropped, the others stay); the last good list is kept beside
//! the installed kits for the times there is no network, and the window is told
//! which one it is showing. Not reaching GitHub is an ordinary state, answered
//! as such: nothing is logged.
//!
//! Installing is not done here: the window calls `kit_add_git` with the kit's
//! folder, and nothing is enabled by it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The one source of this version.
pub const INDEX_URL: &str =
    "https://raw.githubusercontent.com/simnJS/hangar-ai/main/kits/index.json";
/// Past this, the answer is not the index.
pub const MAX_INDEX_BYTES: u64 = 256 * 1024;
const MAX_KITS: usize = 200;
const MAX_TAGS: usize = 8;
const CACHE_FILE: &str = "gallery-cache.json";

/// One public kit, as the index describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryKit {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The kit's folder in the repository, like `kits/ci-status`.
    pub path: String,
    pub version: String,
    /// What the kit will ask to run, in a sentence; the activation shows the truth.
    pub permissions_summary: String,
    pub tags: Vec<String>,
}

/// Where the list comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Fetched just now.
    Live,
    /// The last good copy: GitHub could not be reached.
    Cached,
    /// Neither: no network, and nothing kept yet.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gallery {
    pub origin: Origin,
    pub kits: Vec<GalleryKit>,
    /// When the list was fetched, in ms since the epoch; null when unavailable.
    pub fetched_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cache {
    fetched_at: u64,
    kits: Vec<GalleryKit>,
}

fn text(entry: &Value, key: &str, max: usize) -> Option<String> {
    let value = entry.get(key)?.as_str()?.trim();
    (!value.is_empty() && value.chars().count() <= max && !value.chars().any(char::is_control))
        .then(|| value.to_string())
}

fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
}

fn entry(value: &Value) -> Option<GalleryKit> {
    let id = text(value, "id", 64).filter(|id| id_ok(id))?;
    let version = text(value, "version", 64)?;
    semver::Version::parse(&version).ok()?;
    // The folder is `kits/<id>`: a path read from a file we do not control is
    // not joined to anything else.
    let path = text(value, "path", 128).filter(|path| *path == format!("kits/{id}"))?;
    let tags = match value.get("tags") {
        None => Vec::new(),
        Some(Value::Array(tags)) => tags
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|tag| !tag.is_empty() && tag.chars().count() <= 24)
            .take(MAX_TAGS)
            .map(str::to_string)
            .collect(),
        Some(_) => return None,
    };
    Some(GalleryKit {
        name: text(value, "name", 80)?,
        description: text(value, "description", 400)?,
        permissions_summary: text(value, "permissionsSummary", 300).unwrap_or_default(),
        id,
        path,
        version,
        tags,
    })
}

/// The kits of an index file, whatever else it holds. `None` when it is not an
/// index at all; an entry that is not well formed, or repeats an id, is dropped.
pub fn parse(text: &str) -> Option<Vec<GalleryKit>> {
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let entries = value.get("kits")?.as_array()?;
    let mut kits: Vec<GalleryKit> = Vec::new();
    for item in entries.iter().take(MAX_KITS) {
        if let Some(kit) = entry(item) {
            if !kits.iter().any(|seen| seen.id == kit.id) {
                kits.push(kit);
            }
        }
    }
    Some(kits)
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .user_agent("Hangar")
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(Duration::from_secs(10)))
            .max_idle_connections(0)
            .build()
            .new_agent()
    })
}

/// The index file's text. Any failure — offline, a status, too large — is `None`.
fn fetch() -> Option<String> {
    let mut response = agent().get(INDEX_URL).call().ok()?;
    // One byte more than allowed: ureq refuses a body ending right on its limit.
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_INDEX_BYTES + 1)
        .read_to_vec()
        .ok()?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn read_cache(path: &Path) -> Option<Cache> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_INDEX_BYTES * 2 {
        return None;
    }
    let cache: Cache = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    // Read back through the same filter as a fetched one.
    let again = serde_json::to_string(&serde_json::json!({ "kits": cache.kits })).ok()?;
    Some(Cache {
        kits: parse(&again)?,
        ..cache
    })
}

/// Written aside then renamed, so a crash never leaves half a copy. Failing to
/// keep it is not an error: the list is shown all the same.
fn write_cache(path: &Path, cache: &Cache) {
    let Ok(json) = serde_json::to_string(cache) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::write(&tmp, json).is_ok() && fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// The gallery for a folder to keep its copy in, the fetch given.
pub fn load(root: Option<&Path>, now_ms: u64, fetch: impl FnOnce() -> Option<String>) -> Gallery {
    let cache_path = root.map(|root| root.join(CACHE_FILE));
    if let Some(kits) = fetch().and_then(|body| parse(&body)) {
        if let Some(path) = &cache_path {
            write_cache(
                path,
                &Cache {
                    fetched_at: now_ms,
                    kits: kits.clone(),
                },
            );
        }
        return Gallery {
            origin: Origin::Live,
            kits,
            fetched_at: Some(now_ms),
        };
    }
    match cache_path.as_deref().and_then(read_cache) {
        Some(cache) => Gallery {
            origin: Origin::Cached,
            kits: cache.kits,
            fetched_at: Some(cache.fetched_at),
        },
        None => Gallery {
            origin: Origin::Unavailable,
            kits: Vec::new(),
            fetched_at: None,
        },
    }
}

#[tauri::command]
pub async fn kit_gallery_list() -> Result<Gallery, String> {
    crate::git::off_thread(|| {
        let root: Option<PathBuf> = super::kits_root();
        Ok(load(root.as_deref(), super::now_ms(), fetch))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn entry_json(id: &str, version: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Kit {id}","description":"d","path":"kits/{id}","version":"{version}","permissionsSummary":"runs gh","tags":["ci"]}}"#
        )
    }

    fn index(entries: &[String]) -> String {
        format!(r#"{{"schema":1,"kits":[{}]}}"#, entries.join(","))
    }

    fn scratch() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hangar-gallery-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_url_is_the_one_https_source() {
        assert!(INDEX_URL.starts_with("https://raw.githubusercontent.com/simnJS/hangar-ai/main/"));
    }

    #[test]
    fn parses_a_well_formed_index() {
        let kits = parse(&index(&[entry_json("ci-status", "0.1.0")])).unwrap();
        assert_eq!(kits.len(), 1);
        assert_eq!(kits[0].id, "ci-status");
        assert_eq!(kits[0].tags, vec!["ci"]);
        assert_eq!(kits[0].permissions_summary, "runs gh");
    }

    #[test]
    fn drops_what_is_not_well_formed_and_keeps_the_rest() {
        let bad_path = entry_json("evil", "1.0.0").replace("kits/evil", "../../etc");
        let bad_version = entry_json("old", "1.0");
        let bad_id = entry_json("Bad_Id", "1.0.0");
        let kits = parse(&index(&[
            bad_path,
            bad_version,
            bad_id,
            "42".into(),
            entry_json("good", "1.0.0"),
            entry_json("good", "2.0.0"),
        ]))
        .unwrap();
        assert_eq!(kits.len(), 1);
        assert_eq!(kits[0].version, "1.0.0");
    }

    #[test]
    fn something_that_is_not_an_index_is_nothing() {
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"kits":"x"}"#).is_none());
        assert!(parse("[]").is_none());
        assert_eq!(parse(r#"{"kits":[]}"#), Some(vec![]));
    }

    #[test]
    fn a_fetched_list_is_kept_and_served_when_offline() {
        let dir = scratch();
        let live = load(Some(&dir), 10, || Some(index(&[entry_json("a", "1.0.0")])));
        assert_eq!(live.origin, Origin::Live);
        assert_eq!(live.fetched_at, Some(10));

        let offline = load(Some(&dir), 20, || None);
        assert_eq!(offline.origin, Origin::Cached);
        assert_eq!(offline.kits, live.kits);
        assert_eq!(offline.fetched_at, Some(10));

        // A garbled answer is no better than none: the good copy stays.
        let garbled = load(Some(&dir), 30, || Some("<html>".into()));
        assert_eq!(garbled.origin, Origin::Cached);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_fetched_and_nothing_kept_is_unavailable() {
        let dir = scratch();
        let gallery = load(Some(&dir), 1, || None);
        assert_eq!(gallery.origin, Origin::Unavailable);
        assert!(gallery.kits.is_empty());
        assert_eq!(gallery.fetched_at, None);
        assert_eq!(load(None, 1, || None).origin, Origin::Unavailable);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_cache_is_ignored() {
        let dir = scratch();
        fs::write(dir.join(CACHE_FILE), "{ nope").unwrap();
        assert_eq!(load(Some(&dir), 1, || None).origin, Origin::Unavailable);
        let _ = fs::remove_dir_all(&dir);
    }

    /// `kits/index.json` of this repository lists every kit folder, with the
    /// id, name and version of its `hangar-kit.json`.
    #[test]
    fn the_index_of_the_repository_follows_the_kits() {
        let kits_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("kits");
        let index_text = fs::read_to_string(kits_dir.join("index.json")).unwrap();
        let listed = parse(&index_text).expect("kits/index.json is an index");
        let raw: Value = serde_json::from_str(&index_text).unwrap();
        assert_eq!(
            raw["kits"].as_array().unwrap().len(),
            listed.len(),
            "every entry of kits/index.json is well formed, with an id used once"
        );

        let mut folders = Vec::new();
        for dir in fs::read_dir(&kits_dir).unwrap().flatten() {
            let manifest = dir.path().join("hangar-kit.json");
            if !manifest.is_file() {
                continue;
            }
            let value: Value = serde_json::from_str(&fs::read_to_string(&manifest).unwrap())
                .unwrap_or_else(|_| panic!("{} is JSON", manifest.display()));
            let folder = dir.file_name().to_string_lossy().into_owned();
            let id = value["id"].as_str().unwrap().to_string();
            assert_eq!(id, folder, "a kit's folder is named after its id");
            let item = listed
                .iter()
                .find(|kit| kit.id == id)
                .unwrap_or_else(|| panic!("kit {id} is missing from kits/index.json"));
            assert_eq!(
                Some(item.name.as_str()),
                value["name"].as_str(),
                "name of {id}"
            );
            assert_eq!(
                Some(item.version.as_str()),
                value["version"].as_str(),
                "version of {id}"
            );
            folders.push(id);
        }
        for kit in &listed {
            assert!(
                folders.contains(&kit.id),
                "kits/index.json lists {}, which has no folder",
                kit.id
            );
        }
    }
}
