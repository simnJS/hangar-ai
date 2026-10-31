//! The kits installed on this machine: `~/.hangar/kits/`, and its
//! `index.json`.
//!
//! A folder kit is linked: the index points at the folder, which is read in
//! place every time and never copied, so editing it is editing the kit. A git
//! kit is a shallow clone Hangar made in `~/.hangar/kits/<id>`, and the only
//! thing removing a kit ever deletes.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::hooks;
use super::manifest::{self, Manifest};
use super::source::{self, GitSource};
use super::HANGAR_VERSION;

const INDEX: &str = "index.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// Linked where it is.
    Folder {
        path: String,
    },
    Git(GitSource),
}

/// One installed kit, as `index.json` lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub source: Source,
    /// The kit's root on disk: the linked folder, or the folder in the clone.
    pub dir: String,
    /// The manifest's version when the kit was added or last updated.
    pub installed_version: String,
    /// Milliseconds since the epoch.
    pub added_at: u64,
    /// Whether a workspace has to have accepted this kit's hooks for them to
    /// reach its panes. Set on every git kit added since that is asked, and on
    /// an older one as soon as an update changes its hooks; a git kit installed
    /// before, with its hooks as they were, keeps working as it did on the
    /// workspaces that enabled it (docs/kits.md). A linked folder is never
    /// asked: every edit would ask again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hooks_consent: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    kits: Vec<Entry>,
}

/// What an update check found out.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    /// The version the manifest declares at the remote ref.
    pub latest: String,
    pub available: bool,
    pub checked_at: u64,
}

/// A kit that can be used: installed, its manifest valid, this Hangar one it
/// accepts.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub entry: Entry,
    /// Canonical.
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl Loaded {
    /// The kit's Claude Code plugin folder, when it has one.
    pub fn plugin_dir(&self) -> Option<PathBuf> {
        let plugin = self.manifest.plugin.as_deref()?;
        manifest::inside(&self.root, plugin).ok()
    }

    /// `hooks:<hash>` of what its plugin runs by itself; `None` without hooks.
    pub fn hooks_permission(&self) -> Option<String> {
        hooks::grant(&hooks::read(&self.plugin_dir()?))
    }
}

pub struct Store {
    root: PathBuf,
}

/// One change to the store at a time, across every window and command.
fn lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Update checks remembered until the app quits, by store and kit.
fn checks() -> MutexGuard<'static, HashMap<(PathBuf, String), UpdateInfo>> {
    static CHECKS: OnceLock<Mutex<HashMap<(PathBuf, String), UpdateInfo>>> = OnceLock::new();
    CHECKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Store {
    pub fn at(root: PathBuf) -> Self {
        Store { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX)
    }

    /// Where Hangar clones a git kit. Ids are `[a-z0-9-]`, so this is always
    /// a direct child of the store, and never one of its own files.
    fn clone_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Every installed kit. A missing index is an empty store; one that is
    /// there and cannot be read is an error, rather than a store that looks
    /// empty and is then overwritten.
    pub fn entries(&self) -> Result<Vec<Entry>, String> {
        Ok(self.load()?.kits)
    }

    fn load(&self) -> Result<Index, String> {
        let path = self.index_path();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Index::default()),
            Err(err) => {
                return Err(format!(
                    "the kits index {} cannot be read: {err}",
                    path.display()
                ))
            }
        };
        let index: Index =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|err| {
                format!(
                    "the kits index {} cannot be read ({err}); fix or delete it",
                    path.display()
                )
            })?;
        // An id names the clone git is run in and that removing deletes: one
        // edited into something else would aim both outside the store.
        if let Some(bad) = index
            .kits
            .iter()
            .find(|entry| !manifest::valid_kit_id(&entry.id))
        {
            return Err(format!(
                "the kits index {} lists an invalid kit id {:?}; fix or delete it",
                path.display(),
                bad.id
            ));
        }
        Ok(index)
    }

    fn save(&self, index: &Index) -> Result<(), String> {
        let body = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
        write_atomic(&self.index_path(), &body)
    }

    pub fn entry(&self, id: &str) -> Result<Entry, String> {
        self.entries()?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("no kit {id:?} is installed"))
    }

    /// The kit as it can be used right now, or why it cannot be.
    pub fn loaded(&self, entry: &Entry) -> Result<Loaded, String> {
        let root = dunce::canonicalize(&entry.dir)
            .map_err(|_| format!("its folder {} is not there any more", entry.dir))?;
        let manifest = manifest::read(&root)?;
        if manifest.id != entry.id {
            return Err(format!(
                "its manifest now names the kit {:?}; remove it and add it again",
                manifest.id
            ));
        }
        manifest::compatible(&manifest, HANGAR_VERSION)?;
        Ok(Loaded {
            entry: entry.clone(),
            root,
            manifest,
        })
    }

    pub fn update_info(&self, id: &str) -> Option<UpdateInfo> {
        checks().get(&(self.root.clone(), id.to_string())).cloned()
    }

    /// Links the folder at `path`, which must hold a valid kit.
    pub fn add_folder(&self, path: &str, now: u64) -> Result<Entry, String> {
        let path = Path::new(path.trim());
        if !path.is_absolute() {
            return Err("a kit folder is given by its full path".into());
        }
        let dir = dunce::canonicalize(path)
            .ok()
            .filter(|dir| dir.is_dir())
            .ok_or_else(|| format!("{} is not a folder", path.display()))?;
        let manifest = manifest::read(&dir)?;
        manifest::compatible(&manifest, HANGAR_VERSION)?;

        let _guard = lock();
        let mut index = self.load()?;
        refuse_taken(&index, &manifest.id)?;
        let dir = dir.to_string_lossy().into_owned();
        let entry = Entry {
            id: manifest.id.clone(),
            source: Source::Folder { path: dir.clone() },
            dir,
            installed_version: manifest.version,
            added_at: now,
            hooks_consent: false,
        };
        index.kits.push(entry.clone());
        self.save(&index)?;
        Ok(entry)
    }

    /// Clones the kit a GitHub link points at. Only `https://github.com/`
    /// links are taken (source::parse_link).
    pub fn add_git(&self, link: &str, now: u64) -> Result<Entry, String> {
        let link = source::parse_link(link)?;
        let (git_ref, subdir) = if link.tree.is_empty() {
            (None, None)
        } else {
            source::split_tree(&link.tree, &source::remote_refs(&link.url)?)?
        };
        self.add_git_source(
            GitSource {
                url: link.url,
                git_ref,
                subdir,
            },
            now,
        )
    }

    /// The clone itself, from any URL git takes: the link was checked by the
    /// caller, and the tests clone from `file://`.
    pub fn add_git_source(&self, source: GitSource, now: u64) -> Result<Entry, String> {
        let _guard = lock();
        let mut index = self.load()?;
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        // Cloned aside first: the id is only known once the manifest is read,
        // and a clone that turns out not to be a kit leaves nothing behind.
        let staging = self
            .root
            .join(format!(".clone-{}", uuid::Uuid::new_v4().simple()));
        let checked = (|| {
            source::clone(&source, &staging)?;
            let root = kit_root(&staging, source.subdir.as_deref())?;
            let manifest = manifest::read(&root)?;
            manifest::compatible(&manifest, HANGAR_VERSION)?;
            refuse_taken(&index, &manifest.id)?;
            Ok::<_, String>(manifest)
        })();
        let manifest = match checked {
            Ok(manifest) => manifest,
            Err(why) => {
                let _ = remove_tree(&staging);
                return Err(why);
            }
        };

        let target = self.clone_dir(&manifest.id);
        // Nothing in the index points at it: a clone an interrupted removal
        // or add left behind, in Hangar's own folder.
        if target.exists() {
            remove_tree(&target).map_err(|e| {
                let _ = remove_tree(&staging);
                format!(
                    "{} is in the way and could not be deleted: {e}",
                    target.display()
                )
            })?;
        }
        rename_dir(&staging, &target).inspect_err(|_| {
            let _ = remove_tree(&staging);
        })?;
        let dir = kit_root(&target, source.subdir.as_deref())?;
        let entry = Entry {
            id: manifest.id.clone(),
            source: Source::Git(source),
            dir: dir.to_string_lossy().into_owned(),
            installed_version: manifest.version,
            added_at: now,
            hooks_consent: true,
        };
        index.kits.push(entry.clone());
        self.save(&index)?;
        Ok(entry)
    }

    /// Takes a kit out of the index, and deletes its folder only when Hangar
    /// cloned it: a linked folder is the person's, and stays as it is.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let _guard = lock();
        let mut index = self.load()?;
        let at = index
            .kits
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| format!("no kit {id:?} is installed"))?;
        let entry = index.kits.remove(at);
        self.save(&index)?;
        checks().remove(&(self.root.clone(), id.to_string()));
        super::setup::forget(self, id);
        if let Source::Git(_) = entry.source {
            let clone = self.clone_dir(&entry.id);
            if manifest::valid_kit_id(&entry.id) && clone.parent() == Some(self.root.as_path()) {
                remove_tree(&clone).map_err(|e| {
                    format!(
                        "the kit was removed, but its folder {} could not be deleted entirely ({e}); a program may still be using it",
                        clone.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    /// For a git kit, the version its manifest declares at the remote ref,
    /// against the one installed. A folder kit is always as it is on disk:
    /// there is nothing to check, and the answer is `None`.
    pub fn check_update(&self, id: &str, now: u64) -> Result<Option<UpdateInfo>, String> {
        let _guard = lock();
        let entry = self.find(id)?;
        let Source::Git(source) = &entry.source else {
            return Ok(None);
        };
        let latest = self.fetch_version(&entry, source)?;
        let available = match (
            semver::Version::parse(&latest),
            semver::Version::parse(&entry.installed_version),
        ) {
            (Ok(latest), Ok(current)) => latest > current,
            _ => latest != entry.installed_version,
        };
        let info = UpdateInfo {
            current: entry.installed_version.clone(),
            latest,
            available,
            checked_at: now,
        };
        checks().insert((self.root.clone(), id.to_string()), info.clone());
        Ok(Some(info))
    }

    /// Brings a git kit to what its remote ref holds now, and a folder kit's
    /// recorded version to what its manifest says.
    pub fn update(&self, id: &str) -> Result<Entry, String> {
        let _guard = lock();
        let mut index = self.load()?;
        let at = index
            .kits
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| format!("no kit {id:?} is installed"))?;
        let entry = index.kits[at].clone();
        let version = match &entry.source {
            Source::Folder { .. } => manifest::read(Path::new(&entry.dir))?.version,
            Source::Git(source) => {
                let latest = self.fetch_version(&entry, source)?;
                let hooks_before = self
                    .loaded(&entry)
                    .ok()
                    .and_then(|kit| kit.hooks_permission());
                source::checkout_fetched(&self.clone_dir(id))?;
                let hooks_now = self
                    .loaded(&entry)
                    .ok()
                    .and_then(|kit| kit.hooks_permission());
                if hooks_now != hooks_before {
                    index.kits[at].hooks_consent = true;
                }
                latest
            }
        };
        index.kits[at].installed_version = version;
        self.save(&index)?;
        checks().remove(&(self.root.clone(), id.to_string()));
        Ok(index.kits[at].clone())
    }

    fn find(&self, id: &str) -> Result<Entry, String> {
        self.load()?
            .kits
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| format!("no kit {id:?} is installed"))
    }

    /// Fetches the remote ref, and reads the version its manifest declares —
    /// refusing a remote that has become another kit.
    fn fetch_version(&self, entry: &Entry, source: &GitSource) -> Result<String, String> {
        let clone = self.clone_dir(&entry.id);
        source::fetch(source, &clone)?;
        let path = match &source.subdir {
            Some(subdir) => format!("{subdir}/{}", manifest::FILE),
            None => manifest::FILE.to_string(),
        };
        let text = source::fetched_file(&clone, &path)
            .map_err(|_| format!("the remote holds no {path} any more"))?;
        let (remote_id, version) = manifest::version_of(&text)
            .ok_or_else(|| format!("the remote {path} has no valid id and version"))?;
        if remote_id != entry.id {
            return Err(format!(
                "the remote now holds the kit {remote_id:?}, not {:?}",
                entry.id
            ));
        }
        Ok(version)
    }
}

fn refuse_taken(index: &Index, id: &str) -> Result<(), String> {
    if index.kits.iter().any(|entry| entry.id == id) {
        Err(format!("a kit {id:?} is already installed"))
    } else {
        Ok(())
    }
}

/// The kit's root in a clone: the clone, or the folder the link named in it.
fn kit_root(clone: &Path, subdir: Option<&str>) -> Result<PathBuf, String> {
    let clone = dunce::canonicalize(clone).map_err(|e| e.to_string())?;
    match subdir {
        None => Ok(clone),
        Some(subdir) => manifest::inside(&clone, subdir)
            .ok()
            .filter(|dir| dir.is_dir())
            .ok_or_else(|| format!("the repository has no folder {subdir}")),
    }
}

/// Written aside and renamed over, so a reader never sees half a file.
pub(crate) fn write_atomic(path: &Path, body: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// A rename that a virus scanner opening the fresh clone can delay on
/// Windows: tried again for a moment before giving up.
fn rename_dir(from: &Path, to: &Path) -> Result<(), String> {
    let mut last = None;
    for _ in 0..10 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(err) => last = Some(err),
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    Err(format!(
        "the clone could not be moved to {}: {}",
        to.display(),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Deletes a folder Hangar made, links inside it left unfollowed. Git makes
/// its pack files read-only, which Windows refuses to delete as they are.
pub(crate) fn remove_tree(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        #[cfg(windows)]
        Err(_) => {
            clear_readonly(path);
            match fs::remove_dir_all(path) {
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }
        #[cfg(not(windows))]
        Err(err) => Err(err),
    }
}

#[cfg(windows)]
fn clear_readonly(path: &Path) {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    // A link or a junction: not ours to walk into.
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return;
    }
    if meta.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                clear_readonly(&entry.path());
            }
        }
    }
    let mut permissions = meta.permissions();
    if permissions.readonly() {
        // Windows only, where this clears the read-only attribute and nothing
        // else: the unix concern of the lint does not arise.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::testing::{Folder, Remote};
    use serde_json::json;

    fn store(tag: &str) -> (Folder, Store) {
        let home = Folder::new(tag);
        let store = Store::at(home.root.join("kits"));
        (home, store)
    }

    #[test]
    fn a_folder_kit_is_linked_and_never_deleted() {
        let (_home, store) = store("folder");
        let source = Folder::new("folder-src");
        let dir = source.kit(".", "studio", "1.0.0");

        let entry = store.add_folder(&dir.to_string_lossy(), 42).unwrap();
        assert_eq!(entry.id, "studio");
        assert_eq!(entry.installed_version, "1.0.0");
        assert_eq!(entry.added_at, 42);
        assert_eq!(Path::new(&entry.dir), dir.as_path(), "linked where it is");
        assert!(matches!(entry.source, Source::Folder { .. }));
        assert_eq!(store.entries().unwrap(), vec![entry.clone()]);

        // Edited in place: the kit is the folder.
        source.manifest(".", "studio", "1.1.0");
        let loaded = store.loaded(&entry).unwrap();
        assert_eq!(loaded.manifest.version, "1.1.0");
        assert_eq!(store.check_update("studio", 1).unwrap(), None);
        assert_eq!(store.update("studio").unwrap().installed_version, "1.1.0");

        assert!(store
            .add_folder(&dir.to_string_lossy(), 43)
            .unwrap_err()
            .contains("already installed"));

        store.remove("studio").unwrap();
        assert!(store.entries().unwrap().is_empty());
        assert!(dir.join(manifest::FILE).is_file(), "a linked folder stays");
    }

    #[test]
    fn a_folder_without_a_valid_kit_is_refused() {
        let (_home, store) = store("folder-bad");
        let empty = Folder::new("folder-empty");
        assert!(store
            .add_folder(&empty.root.to_string_lossy(), 1)
            .unwrap_err()
            .contains("no hangar-kit.json"));
        empty.file(
            manifest::FILE,
            &json!({ "schema": 1, "id": "X" }).to_string(),
        );
        assert!(store.add_folder(&empty.root.to_string_lossy(), 1).is_err());
        assert!(store
            .add_folder("relative/kit", 1)
            .unwrap_err()
            .contains("full path"));
        assert!(!store.index_path().exists(), "nothing was written");
    }

    #[test]
    fn a_git_kit_is_cloned_checked_updated_and_removed() {
        let (_home, store) = store("git");
        let remote = Remote::new("git-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");

        let entry = store
            .add_git_source(
                GitSource {
                    url: remote.url(),
                    git_ref: None,
                    subdir: None,
                },
                7,
            )
            .unwrap();
        let clone = store.root().join("studio");
        assert_eq!(Path::new(&entry.dir), dunce::canonicalize(&clone).unwrap());
        assert_eq!(entry.installed_version, "1.0.0");
        assert!(clone.join(".claude-plugin/plugin.json").is_file());
        assert!(
            fs::read_dir(store.root())
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().starts_with(".clone-")),
            "no staging folder left"
        );
        let loaded = store.loaded(&entry).unwrap();
        assert_eq!(
            loaded.plugin_dir(),
            Some(dunce::canonicalize(&clone).unwrap())
        );

        // Nothing new upstream.
        let check = store.check_update("studio", 8).unwrap().unwrap();
        assert_eq!((check.latest.as_str(), check.available), ("1.0.0", false));

        remote.folder.manifest("work", "studio", "1.2.0");
        remote
            .folder
            .file("work/skills/more/SKILL.md", "---\nname: more\n---\n");
        remote.push("second");
        let check = store.check_update("studio", 9).unwrap().unwrap();
        assert_eq!(
            check,
            UpdateInfo {
                current: "1.0.0".into(),
                latest: "1.2.0".into(),
                available: true,
                checked_at: 9
            }
        );
        assert_eq!(store.update_info("studio"), Some(check));
        // Something a setup step left in the kit folder.
        fs::write(clone.join("installed.txt"), "local").unwrap();

        let updated = store.update("studio").unwrap();
        assert_eq!(updated.installed_version, "1.2.0");
        assert!(clone.join("skills/more/SKILL.md").is_file());
        assert!(
            clone.join("installed.txt").is_file(),
            "untracked files stay"
        );
        assert_eq!(store.update_info("studio"), None, "the check is stale now");
        assert!(!store.check_update("studio", 10).unwrap().unwrap().available);

        store.remove("studio").unwrap();
        assert!(!clone.exists(), "the clone is Hangar's to delete");
        assert!(store.entries().unwrap().is_empty());
        assert!(store.remove("studio").unwrap_err().contains("no kit"));
    }

    fn with_hooks(remote: &Remote, command: &str) {
        remote.folder.file(
            "work/hooks/hooks.json",
            &json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": command }] }] } })
                .to_string(),
        );
    }

    /// As `index.json` held a git kit before hooks were asked about.
    fn forget_consent(store: &Store, id: &str) {
        let mut index = store.load().unwrap();
        for entry in &mut index.kits {
            if entry.id == id {
                entry.hooks_consent = false;
            }
        }
        store.save(&index).unwrap();
        let text = fs::read_to_string(store.index_path()).unwrap();
        assert!(!text.contains("hooksConsent"), "{text}");
    }

    fn cloned(store: &Store, remote: &Remote) -> Entry {
        store
            .add_git_source(
                GitSource {
                    url: remote.url(),
                    git_ref: None,
                    subdir: None,
                },
                1,
            )
            .unwrap()
    }

    #[test]
    fn a_git_kit_added_now_is_asked_about_its_hooks_a_linked_folder_never() {
        let (_home, store) = store("consent-new");
        let remote = Remote::new("consent-new-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        with_hooks(&remote, "one.sh");
        remote.push("first");
        let entry = cloned(&store, &remote);
        assert!(entry.hooks_consent);
        assert!(store.load().unwrap().kits[0].hooks_consent, "it is on disk");
        assert!(store.loaded(&entry).unwrap().hooks_permission().is_some());

        let source = Folder::new("consent-new-folder");
        source.kit(".", "linked", "1.0.0");
        let linked = store.add_folder(&source.root.to_string_lossy(), 1).unwrap();
        assert!(!linked.hooks_consent);
        store.update("linked").unwrap();
        assert!(!store.entry("linked").unwrap().hooks_consent);
    }

    #[test]
    fn an_older_git_kit_is_asked_once_an_update_changes_its_hooks() {
        let (_home, store) = store("consent-old");
        let remote = Remote::new("consent-old-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        with_hooks(&remote, "one.sh");
        remote.push("first");
        cloned(&store, &remote);
        forget_consent(&store, "studio");
        assert!(!store.entry("studio").unwrap().hooks_consent);
        let before = store
            .loaded(&store.entry("studio").unwrap())
            .unwrap()
            .hooks_permission()
            .unwrap();

        // Another version with the same hooks: as it was.
        remote.folder.manifest("work", "studio", "1.1.0");
        remote.push("same hooks");
        let same = store.update("studio").unwrap();
        assert!(!same.hooks_consent);
        assert_eq!(same.installed_version, "1.1.0");
        assert_eq!(
            store.loaded(&same).unwrap().hooks_permission().unwrap(),
            before
        );

        with_hooks(&remote, "two.sh");
        remote.folder.manifest("work", "studio", "1.2.0");
        remote.push("other hooks");
        let changed = store.update("studio").unwrap();
        assert!(changed.hooks_consent);
        assert_ne!(
            store.loaded(&changed).unwrap().hooks_permission().unwrap(),
            before
        );
    }

    #[test]
    fn an_update_that_brings_hooks_to_a_kit_without_any_is_a_change() {
        let (_home, store) = store("consent-gained");
        let remote = Remote::new("consent-gained-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        let entry = cloned(&store, &remote);
        assert_eq!(store.loaded(&entry).unwrap().hooks_permission(), None);
        forget_consent(&store, "studio");

        with_hooks(&remote, "new.sh");
        remote.folder.manifest("work", "studio", "1.1.0");
        remote.push("hooks now");
        let updated = store.update("studio").unwrap();
        assert!(updated.hooks_consent);
        assert!(store.loaded(&updated).unwrap().hooks_permission().is_some());
    }

    #[test]
    fn a_git_kit_can_live_in_a_folder_of_its_repository() {
        let (_home, store) = store("git-subdir");
        let remote = Remote::new("git-subdir-remote");
        remote.folder.kit("work/kits/studio", "studio", "2.0.0");
        remote.folder.file("work/README.md", "a repository of kits");
        remote.push("kits");

        let entry = store
            .add_git_source(
                GitSource {
                    url: remote.url(),
                    git_ref: Some("main".into()),
                    subdir: Some("kits/studio".into()),
                },
                1,
            )
            .unwrap();
        let clone = dunce::canonicalize(store.root().join("studio")).unwrap();
        assert_eq!(Path::new(&entry.dir), clone.join("kits").join("studio"));
        assert_eq!(store.loaded(&entry).unwrap().manifest.version, "2.0.0");

        remote
            .folder
            .manifest("work/kits/studio", "studio", "2.1.0");
        remote.push("bump");
        assert!(store.check_update("studio", 2).unwrap().unwrap().available);
        assert_eq!(store.update("studio").unwrap().installed_version, "2.1.0");
        assert_eq!(store.loaded(&entry).unwrap().manifest.version, "2.1.0");
    }

    #[test]
    fn a_repository_that_is_not_a_kit_leaves_nothing_behind() {
        let (_home, store) = store("git-notkit");
        let remote = Remote::new("git-notkit-remote");
        remote.folder.file("work/README.md", "no kit here");
        remote.push("readme");
        let source = GitSource {
            url: remote.url(),
            git_ref: None,
            subdir: None,
        };
        assert!(store
            .add_git_source(source.clone(), 1)
            .unwrap_err()
            .contains("no hangar-kit.json"));
        assert!(store
            .add_git_source(
                GitSource {
                    subdir: Some("missing".into()),
                    ..source
                },
                1
            )
            .unwrap_err()
            .contains("no folder missing"));
        assert_eq!(fs::read_dir(store.root()).unwrap().count(), 0);
    }

    #[test]
    fn a_remote_that_became_another_kit_is_not_taken_as_an_update() {
        let (_home, store) = store("git-other");
        let remote = Remote::new("git-other-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        store
            .add_git_source(
                GitSource {
                    url: remote.url(),
                    git_ref: None,
                    subdir: None,
                },
                1,
            )
            .unwrap();
        remote.folder.manifest("work", "other", "9.0.0");
        remote.push("renamed");
        assert!(store
            .check_update("studio", 2)
            .unwrap_err()
            .contains("\"other\""));
        assert!(store.update("studio").is_err());
        assert_eq!(store.entry("studio").unwrap().installed_version, "1.0.0");
    }

    #[test]
    fn only_https_github_links_reach_git() {
        let (_home, store) = store("git-https");
        for link in [
            "file:///C:/kit",
            "http://github.com/o/r",
            "/home/me/kit",
            "git@github.com:o/r",
        ] {
            assert!(store.add_git(link, 1).is_err(), "{link}");
        }
        assert!(!store.root().exists(), "refused before anything ran");
    }

    #[test]
    fn a_broken_index_is_reported_not_overwritten() {
        let (_home, store) = store("index");
        fs::create_dir_all(store.root()).unwrap();
        fs::write(store.index_path(), "{ broken").unwrap();
        assert!(store.entries().unwrap_err().contains("cannot be read"));
        let source = Folder::new("index-src");
        let dir = source.kit(".", "studio", "1.0.0");
        assert!(store.add_folder(&dir.to_string_lossy(), 1).is_err());
        assert_eq!(fs::read_to_string(store.index_path()).unwrap(), "{ broken");

        // An id edited to climb out of the store is refused as well.
        let escaping = json!({ "kits": [{
            "id": "..", "source": { "kind": "git", "url": "https://github.com/o/r" },
            "dir": "C:/", "installedVersion": "1.0.0", "addedAt": 1
        }] });
        fs::write(store.index_path(), escaping.to_string()).unwrap();
        assert!(store.entries().unwrap_err().contains("invalid kit id"));
        assert!(store.update("..").is_err() && store.remove("..").is_err());
    }
}
