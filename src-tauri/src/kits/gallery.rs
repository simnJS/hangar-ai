//! The kit gallery: the public kits of the `hangar-ai` organization on GitHub,
//! listed by a static file of its catalog repository.
//!
//! No server of ours stands behind it (no hosted service, ever): the list is
//! `index.json` of `hangar-ai/catalog`, read from raw.githubusercontent.com over
//! https. That one URL is the only thing fetched — no icon, no manifest, no page.
//! The answer is size-capped and parsed defensively (an entry that is not well
//! formed is dropped, the others stay); the last good list is kept beside the
//! installed kits for the times there is no network, and the window is told which
//! one it is showing. Not reaching GitHub is an ordinary state, answered as such,
//! and so is a catalog that is not published: nothing is logged.
//!
//! Each entry pins a kit to a tag of its own repository, so a new version reaches
//! people only once a reviewed change of the catalog names a new tag. Installing is
//! not done here: the window calls `kit_add_git` with the link to that tag, and
//! nothing is enabled by it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::source;

/// The one source of this version.
pub const INDEX_URL: &str = "https://raw.githubusercontent.com/hangar-ai/catalog/main/index.json";
/// The only schema read. An older or a newer file lists nothing here.
const SCHEMA: u64 = 2;
/// Hangar reviews what this owner publishes, and nobody else's: the catalog's word
/// on any other repository is not enough for the badge.
const VERIFIED_OWNER: &str = "hangar-ai";
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
    /// The kit's repository on GitHub, `owner/name`.
    pub repo: String,
    /// The tag of that repository the catalog pins it to.
    pub tag: String,
    pub version: String,
    /// What the kit will ask to run, in a sentence; the activation shows the truth.
    pub permissions_summary: String,
    pub tags: Vec<String>,
    /// The catalog says so and the repository is `hangar-ai`'s. Anything else is a
    /// community kit.
    pub verified: bool,
}

/// Where the list comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Fetched just now.
    Live,
    /// The last good copy: the catalog could not be read.
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

/// What asking for the index came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// The file's text.
    Body(String),
    /// GitHub answered that there is no such file: the catalog is not published.
    Missing,
    /// Offline, another status, too large.
    Failed,
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

/// `owner/name` as GitHub spells them, and nothing else: this ends up in the link
/// git is given, read from a file we do not control.
fn repo_ok(repo: &str) -> bool {
    let Some((owner, name)) = repo.split_once('/') else {
        return false;
    };
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !owner.starts_with('-')
        && !owner.ends_with('-');
    let name_ok = (1..=100).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !name.starts_with('.')
        && !name.ends_with(".git");
    owner_ok && name_ok
}

/// A tag's short name, which the link and the pin are both built from.
fn tag_ok(tag: &str) -> bool {
    source::check_ref(tag).is_ok() && tag != "HEAD" && !tag.starts_with("refs/")
}

/// Checked here, whatever the catalog says: the owner has to be `hangar-ai`.
fn verified(repo: &str, says: Option<&Value>) -> bool {
    says.and_then(Value::as_bool) == Some(true)
        && repo.split_once('/').map(|(owner, _)| owner) == Some(VERIFIED_OWNER)
}

fn entry(value: &Value) -> Option<GalleryKit> {
    let id = text(value, "id", 64).filter(|id| id_ok(id))?;
    let version = text(value, "version", 64)?;
    semver::Version::parse(&version).ok()?;
    let repo = text(value, "repo", 140).filter(|repo| repo_ok(repo))?;
    let tag = text(value, "tag", 128).filter(|tag| tag_ok(tag))?;
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
        verified: verified(&repo, value.get("verified")),
        id,
        repo,
        tag,
        version,
        tags,
    })
}

/// The kits of an index file, whatever else it holds. `None` when it is not an
/// index at all. Another schema lists nothing, and an entry that is not well
/// formed, or repeats an id, is dropped.
pub fn parse(text: &str) -> Option<Vec<GalleryKit>> {
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let entries = value.get("kits")?.as_array()?;
    if value.get("schema").and_then(Value::as_u64) != Some(SCHEMA) {
        return Some(Vec::new());
    }
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

/// The tag the catalog names for the kit `id` cloned from `url`. An entry of that
/// id in another repository says nothing of this kit: following it would swap the
/// code of an installed kit for somebody else's.
pub fn tag_for(kits: &[GalleryKit], id: &str, url: &str) -> Option<String> {
    kits.iter()
        .find(|kit| {
            kit.id == id && format!("https://github.com/{}", kit.repo).eq_ignore_ascii_case(url)
        })
        .map(|kit| kit.tag.clone())
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

/// A 404 is GitHub's answer for a repository that is not public yet: no catalog,
/// which is not a failure to reach it.
fn refused(error: &ureq::Error) -> Fetched {
    match error {
        ureq::Error::StatusCode(404) => Fetched::Missing,
        _ => Fetched::Failed,
    }
}

/// The index file's text, or what stood in the way of it.
pub fn fetch() -> Fetched {
    let mut response = match agent().get(INDEX_URL).call() {
        Ok(response) => response,
        Err(error) => return refused(&error),
    };
    // One byte more than allowed: ureq refuses a body ending right on its limit.
    let Ok(bytes) = response
        .body_mut()
        .with_config()
        .limit(MAX_INDEX_BYTES + 1)
        .read_to_vec()
    else {
        return Fetched::Failed;
    };
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Fetched::Failed;
    }
    String::from_utf8(bytes).map_or(Fetched::Failed, Fetched::Body)
}

fn read_cache(path: &Path) -> Option<Cache> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_INDEX_BYTES * 2 {
        return None;
    }
    let cache: Cache = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    // Read back through the same filter as a fetched one, the badge's check included.
    let again =
        serde_json::to_string(&serde_json::json!({ "schema": SCHEMA, "kits": cache.kits })).ok()?;
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
pub fn load(root: Option<&Path>, now_ms: u64, fetch: impl FnOnce() -> Fetched) -> Gallery {
    let cache_path = root.map(|root| root.join(CACHE_FILE));
    let answer = fetch();
    if let Some(kits) = match &answer {
        Fetched::Body(body) => parse(body),
        _ => None,
    } {
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
    if let Some(cache) = cache_path.as_deref().and_then(read_cache) {
        return Gallery {
            origin: Origin::Cached,
            kits: cache.kits,
            fetched_at: Some(cache.fetched_at),
        };
    }
    // GitHub answered, and no catalog is published: an empty library, not one that
    // cannot be reached.
    if answer == Fetched::Missing {
        return Gallery {
            origin: Origin::Live,
            kits: Vec::new(),
            fetched_at: Some(now_ms),
        };
    }
    Gallery {
        origin: Origin::Unavailable,
        kits: Vec::new(),
        fetched_at: None,
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
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn entry_of(id: &str, version: &str) -> Value {
        json!({
            "id": id,
            "name": format!("Kit {id}"),
            "description": "d",
            "repo": format!("hangar-ai/{id}"),
            "tag": format!("v{version}"),
            "version": version,
            "permissionsSummary": "runs gh",
            "tags": ["ci"],
            "verified": true,
        })
    }

    /// The entry with one field changed, or taken out when `value` is null.
    fn with(mut entry: Value, key: &str, value: Value) -> Value {
        match value {
            Value::Null => entry.as_object_mut().unwrap().remove(key),
            value => entry.as_object_mut().unwrap().insert(key.into(), value),
        };
        entry
    }

    fn index(entries: &[Value]) -> String {
        json!({ "schema": 2, "kits": entries }).to_string()
    }

    fn body(entries: &[Value]) -> Fetched {
        Fetched::Body(index(entries))
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
    fn the_url_is_the_catalog_of_the_organization() {
        assert_eq!(
            INDEX_URL,
            "https://raw.githubusercontent.com/hangar-ai/catalog/main/index.json"
        );
    }

    #[test]
    fn parses_a_well_formed_index() {
        let kits = parse(&index(&[entry_of("ci-status", "0.1.0")])).unwrap();
        assert_eq!(
            kits,
            vec![GalleryKit {
                id: "ci-status".into(),
                name: "Kit ci-status".into(),
                description: "d".into(),
                repo: "hangar-ai/ci-status".into(),
                tag: "v0.1.0".into(),
                version: "0.1.0".into(),
                permissions_summary: "runs gh".into(),
                tags: vec!["ci".into()],
                verified: true,
            }]
        );
    }

    #[test]
    fn another_schema_lists_nothing() {
        let entries = [entry_of("ci-status", "0.1.0")];
        for schema in [json!(1), json!(3), json!("2"), Value::Null] {
            let text = json!({ "schema": schema, "kits": entries }).to_string();
            assert_eq!(parse(&text), Some(vec![]), "schema {schema}");
        }
        let without = json!({ "kits": entries }).to_string();
        assert_eq!(parse(&without), Some(vec![]));

        // What this very repository listed until now: `path`, no `repo`.
        let old = r#"{"schema":1,"kits":[{"id":"ci-status","name":"CI","description":"d","path":"kits/ci-status","version":"0.1.0","tags":[]}]}"#;
        assert_eq!(parse(old), Some(vec![]));
    }

    #[test]
    fn drops_what_is_not_well_formed_and_keeps_the_rest() {
        let good = || entry_of("good", "1.0.0");
        let mut bad: Vec<Value> = vec![
            json!(42),
            entry_of("old", "1.0"),
            entry_of("Bad_Id", "1.0.0"),
            with(good(), "name", Value::Null),
            with(good(), "description", Value::Null),
            with(good(), "version", Value::Null),
            with(good(), "tags", json!("ci")),
        ];
        for tag in [
            Value::Null,
            json!(1),
            json!(""),
            json!("-upload-pack=x"),
            json!("a..b"),
            json!("a b"),
            json!("x^"),
            json!("HEAD"),
            json!("refs/heads/main"),
        ] {
            bad.push(with(good(), "tag", tag));
        }
        for entry in &bad {
            let alone = std::slice::from_ref(entry);
            assert_eq!(parse(&index(alone)), Some(vec![]), "{entry}");
        }

        bad.push(good());
        bad.push(entry_of("good", "2.0.0"));
        let kits = parse(&index(&bad)).unwrap();
        assert_eq!(kits.len(), 1);
        assert_eq!(kits[0].version, "1.0.0");
    }

    #[test]
    fn a_repo_is_owner_slash_name_and_nothing_else() {
        for good in [
            "hangar-ai/ci-status",
            "simnJS/hangar-ai",
            "a/b",
            "o/my.kit_2",
            "0wner/1",
        ] {
            assert!(repo_ok(good), "{good}");
        }
        let long_owner = format!("{}/x", "o".repeat(40));
        let long_name = format!("o/{}", "n".repeat(101));
        for bad in [
            "",
            "hangar-ai",
            "/x",
            "x/",
            "/",
            "a/b/c",
            "../x",
            "o/..",
            "o/.hidden",
            "o/kit.git",
            "-o/x",
            "o-/x",
            "o_o/x",
            "o.o/x",
            "o/a b",
            "o/a?b",
            "o/a#b",
            "o\\x",
            "o/a\\b",
            "hаngar-ai/x", // A Cyrillic а.
            "https://github.com/o/x",
            "git@github.com:o/x",
            long_owner.as_str(),
            long_name.as_str(),
        ] {
            assert!(!repo_ok(bad), "{bad}");
            let entry = with(entry_of("kit", "1.0.0"), "repo", json!(bad));
            assert_eq!(parse(&index(&[entry])), Some(vec![]), "{bad}");
        }
    }

    #[test]
    fn verified_takes_the_catalogs_word_and_the_owner_hangar_ai() {
        let badge = |repo: &str, says: Value| {
            let entry = with(
                with(entry_of("kit", "1.0.0"), "repo", json!(repo)),
                "verified",
                says,
            );
            parse(&index(&[entry])).unwrap()[0].verified
        };
        assert!(badge("hangar-ai/kit", json!(true)));

        // The catalog does not say so, or not as a boolean.
        assert!(!badge("hangar-ai/kit", json!(false)));
        assert!(!badge("hangar-ai/kit", Value::Null));
        assert!(!badge("hangar-ai/kit", json!("true")));
        assert!(!badge("hangar-ai/kit", json!(1)));

        // The catalog says so of somebody else's repository.
        assert!(!badge("someone/kit", json!(true)));
        assert!(!badge("someone/hangar-ai", json!(true)));
        assert!(!badge("hangar-ai-kits/kit", json!(true)));
        assert!(!badge("not-hangar-ai/kit", json!(true)));
        assert!(!badge("Hangar-AI/kit", json!(true)));
    }

    #[test]
    fn something_that_is_not_an_index_is_nothing() {
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"kits":"x"}"#).is_none());
        assert!(parse("[]").is_none());
        assert_eq!(parse(r#"{"schema":2,"kits":[]}"#), Some(vec![]));
    }

    #[test]
    fn a_fetched_list_is_kept_and_served_when_offline() {
        let dir = scratch();
        let live = load(Some(&dir), 10, || body(&[entry_of("a", "1.0.0")]));
        assert_eq!(live.origin, Origin::Live);
        assert_eq!(live.fetched_at, Some(10));

        let offline = load(Some(&dir), 20, || Fetched::Failed);
        assert_eq!(offline.origin, Origin::Cached);
        assert_eq!(offline.kits, live.kits);
        assert_eq!(offline.fetched_at, Some(10));

        // A garbled answer is no better than none: the good copy stays.
        let garbled = load(Some(&dir), 30, || Fetched::Body("<html>".into()));
        assert_eq!(garbled.origin, Origin::Cached);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_fetched_and_nothing_kept_is_unavailable() {
        let dir = scratch();
        let gallery = load(Some(&dir), 1, || Fetched::Failed);
        assert_eq!(gallery.origin, Origin::Unavailable);
        assert!(gallery.kits.is_empty());
        assert_eq!(gallery.fetched_at, None);
        assert_eq!(
            load(None, 1, || Fetched::Failed).origin,
            Origin::Unavailable
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_catalog_that_is_not_published_is_an_empty_library() {
        let dir = scratch();
        let gallery = load(Some(&dir), 5, || Fetched::Missing);
        assert_eq!(
            gallery,
            Gallery {
                origin: Origin::Live,
                kits: vec![],
                fetched_at: Some(5),
            }
        );
        assert!(!dir.join(CACHE_FILE).exists(), "nothing to keep");
        assert_eq!(load(None, 5, || Fetched::Missing).origin, Origin::Live);

        // And it does not take away a good copy, should it ever be withdrawn.
        load(Some(&dir), 6, || body(&[entry_of("a", "1.0.0")]));
        let withdrawn = load(Some(&dir), 7, || Fetched::Missing);
        assert_eq!(withdrawn.origin, Origin::Cached);
        assert_eq!(withdrawn.kits.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_404_means_no_catalog() {
        assert_eq!(refused(&ureq::Error::StatusCode(404)), Fetched::Missing);
        for status in [403, 429, 500, 503] {
            assert_eq!(refused(&ureq::Error::StatusCode(status)), Fetched::Failed);
        }
        assert_eq!(refused(&ureq::Error::ConnectionFailed), Fetched::Failed);
    }

    #[test]
    fn a_damaged_cache_is_ignored() {
        let dir = scratch();
        fs::write(dir.join(CACHE_FILE), "{ nope").unwrap();
        assert_eq!(
            load(Some(&dir), 1, || Fetched::Failed).origin,
            Origin::Unavailable
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_copy_a_previous_version_kept_is_ignored() {
        let dir = scratch();
        fs::write(
            dir.join(CACHE_FILE),
            r#"{"fetchedAt":1,"kits":[{"id":"ci-status","name":"CI","description":"d","path":"kits/ci-status","version":"0.1.0","permissionsSummary":"","tags":[]}]}"#,
        )
        .unwrap();
        assert_eq!(
            load(Some(&dir), 2, || Fetched::Failed).origin,
            Origin::Unavailable
        );
        assert_eq!(load(Some(&dir), 2, || Fetched::Missing).kits, vec![]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_kept_copy_goes_through_the_owner_check_again() {
        let dir = scratch();
        let edited = json!({ "fetchedAt": 1, "kits": [
            with(entry_of("a", "1.0.0"), "repo", json!("someone/a")),
            with(entry_of("b", "1.0.0"), "repo", json!("../b")),
        ] });
        fs::write(dir.join(CACHE_FILE), edited.to_string()).unwrap();
        let gallery = load(Some(&dir), 2, || Fetched::Failed);
        assert_eq!(gallery.origin, Origin::Cached);
        assert_eq!(gallery.kits.len(), 1);
        assert_eq!(gallery.kits[0].id, "a");
        assert!(!gallery.kits[0].verified);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tag_of_a_kit_is_read_from_its_own_repository_only() {
        let kits = parse(&index(&[
            entry_of("ci-status", "0.2.0"),
            with(entry_of("other", "3.0.0"), "repo", json!("someone/other")),
        ]))
        .unwrap();
        let at = "https://github.com/hangar-ai/ci-status";
        assert_eq!(tag_for(&kits, "ci-status", at).as_deref(), Some("v0.2.0"));
        // GitHub reads names whatever their case.
        assert_eq!(
            tag_for(&kits, "ci-status", "https://github.com/Hangar-AI/CI-status").as_deref(),
            Some("v0.2.0")
        );
        assert_eq!(tag_for(&kits, "gone", at), None);

        // The same id elsewhere: a fork of it, or the catalog moved it.
        assert_eq!(
            tag_for(&kits, "ci-status", "https://github.com/someone/ci-status"),
            None
        );
        assert_eq!(tag_for(&kits, "other", at), None);
    }
}
