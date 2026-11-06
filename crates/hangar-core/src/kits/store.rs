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
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::commands;
use super::manifest::{self, Manifest};
use super::plugin;
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
    /// Whether a workspace has to have accepted what this kit runs by itself, its
    /// hooks and all else (plugin.rs), for it to reach its panes. Set on every git
    /// kit added since that is asked, and on an older one as soon as an update
    /// changes any of it; a git kit installed before, with all that as it was,
    /// keeps working as it did on the workspaces that enabled it (docs/kits.md). A
    /// linked folder is never asked: every edit would ask again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hooks_consent: bool,
    /// The commit a git kit's clone holds, as it was added or last updated. Null
    /// for a linked folder, and for a git kit no update has moved since a Hangar
    /// that did not record it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

/// The commit a tag of a repository pointed at the first time this machine
/// installed a kit from it. Kept when the kit is removed: a tag its owner moves
/// after the catalog's review is refused at the next install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub url: String,
    pub tag: String,
    pub commit: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    kits: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pins: Vec<Pin>,
}

impl Index {
    /// Refuses a tag that now points elsewhere than the commit recorded for it.
    fn check_pin(&self, source: &GitSource, commit: &str) -> Result<(), String> {
        let Some(tag) = source::pinned_tag(source) else {
            return Ok(());
        };
        match self
            .pins
            .iter()
            .find(|pin| pin.url == source.url && pin.tag == tag)
        {
            Some(pin) if pin.commit != commit => Err(format!(
                "the tag {tag} of {} now points at the commit {}, not at {} as when it was installed here: its owner moved it after it was reviewed, and it is not taken",
                source.url,
                short(commit),
                short(&pin.commit)
            )),
            _ => Ok(()),
        }
    }

    /// Records the commit a tag points at, the first time it is seen.
    fn pin(&mut self, source: &GitSource, commit: &str) {
        let Some(tag) = source::pinned_tag(source) else {
            return;
        };
        if !self
            .pins
            .iter()
            .any(|pin| pin.url == source.url && pin.tag == tag)
        {
            self.pins.push(Pin {
                url: source.url.clone(),
                tag: tag.to_string(),
                commit: commit.to_string(),
            });
        }
    }
}

fn short(commit: &str) -> &str {
    commit.get(..12).unwrap_or(commit)
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

    /// `hooks:<hash>` of what its plugin runs by itself (plugin.rs): its hooks
    /// and everything else, the files they run included; `None` when it runs
    /// nothing.
    pub fn hooks_permission(&self) -> Option<String> {
        plugin::permission(&self.plugin_dir()?)
    }

    /// `commands:<hash>` of what its tabs may run on `os` (commands.rs), the
    /// files those name in the kit included; `None` without any.
    pub fn commands_permission(&self, os: &str) -> Option<String> {
        commands::grant(&self.manifest, &self.root, os)
    }
}

pub struct Store {
    root: PathBuf,
}

/// One change to the store at a time, across every window and command. Never
/// held over the network: a stalled remote would hold every other kit with it.
pub(super) fn lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One fetch, update or removal of a kit's clone at a time, held over the
/// network: a second one on the same kit waits for the first, a check of
/// another kit does not. Taken before `lock`, never after.
fn kit_lock(id: &str) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(id.to_string())
        .or_default()
        .clone()
}

fn hold(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What an update would bring, fetched and read in full, and nothing switched
/// to it yet.
struct Prepared {
    commit: String,
    version: String,
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
            commit: None,
        };
        index.kits.push(entry.clone());
        self.save(&index)?;
        Ok(entry)
    }

    /// Clones the kit a GitHub link points at. Only `https://github.com/`
    /// links are taken (source::parse_link).
    pub fn add_git(&self, link: &str, now: u64) -> Result<Entry, String> {
        self.add_git_link(source::parse_link(link)?, now)
    }

    /// A link already checked: its `/tree/` part read against the remote's refs,
    /// which is where a tag tells itself from a branch and pins the kit.
    fn add_git_link(&self, link: source::Link, now: u64) -> Result<Entry, String> {
        let refs = if link.tree.is_empty() {
            source::RemoteRefs::default()
        } else {
            source::remote_refs(&link.url)?
        };
        self.add_git_source(source::source_of(link, &refs)?, now)
    }

    /// The clone itself, from any URL git takes: the link was checked by the
    /// caller, and the tests clone from `file://`. The network part runs with no
    /// lock held; the store is locked only to move the clone in and record it.
    pub fn add_git_source(&self, source: GitSource, now: u64) -> Result<Entry, String> {
        fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        // Cloned aside first: the id is only known once the manifest is read,
        // and a clone that turns out not to be a kit leaves nothing behind.
        let staging = self
            .root
            .join(format!(".clone-{}", uuid::Uuid::new_v4().simple()));
        let checked = (|| {
            let commit = source::clone(&source, &staging)?;
            let root = kit_root(&staging, source.subdir.as_deref())?;
            let manifest = manifest::read(&root)?;
            manifest::compatible(&manifest, HANGAR_VERSION)?;
            Ok::<_, String>((manifest, commit))
        })();
        let (manifest, commit) = match checked {
            Ok(found) => found,
            Err(why) => {
                let _ = remove_tree(&staging);
                return Err(why);
            }
        };

        let kit = kit_lock(&manifest.id);
        let _kit = hold(&kit);
        let _guard = lock();
        let recorded = (|| {
            let index = self.load()?;
            refuse_taken(&index, &manifest.id)?;
            index.check_pin(&source, &commit)?;
            Ok::<_, String>(index)
        })();
        let mut index = match recorded {
            Ok(index) => index,
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
        index.pin(&source, &commit);
        let entry = Entry {
            id: manifest.id.clone(),
            source: Source::Git(source),
            dir: dir.to_string_lossy().into_owned(),
            installed_version: manifest.version,
            added_at: now,
            hooks_consent: true,
            commit: Some(commit),
        };
        index.kits.push(entry.clone());
        self.save(&index)?;
        Ok(entry)
    }

    /// Takes a kit out of the index, and deletes its folder only when Hangar
    /// cloned it: a linked folder is the person's, and stays as it is.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let kit = kit_lock(id);
        let _kit = hold(&kit);
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
    /// against the one installed. A kit pinned to a tag is only looked at on
    /// another tag the catalog names (`catalog_tag`, `source::update_target`); with
    /// none, it is as it is and nothing is fetched. A folder kit is always as it is
    /// on disk: there is nothing to check, and the answer is `None`. A new version
    /// that could not be used here — a manifest that does not hold, a newer Hangar
    /// needed, another kit — is an error that says so, not an update.
    pub fn check_update(
        &self,
        id: &str,
        now: u64,
        catalog_tag: Option<&str>,
    ) -> Result<Option<UpdateInfo>, String> {
        let kit = kit_lock(id);
        let _kit = hold(&kit);
        let entry = self.find(id)?;
        let Source::Git(source) = &entry.source else {
            return Ok(None);
        };
        let latest = match source::update_target(source, catalog_tag) {
            Some(target) => self.prepare(&entry, &target)?.version,
            None => entry.installed_version.clone(),
        };
        let info = UpdateInfo {
            available: is_later(&latest, &entry.installed_version),
            current: entry.installed_version.clone(),
            latest,
            checked_at: now,
        };
        checks().insert((self.root.clone(), id.to_string()), info.clone());
        Ok(Some(info))
    }

    /// Brings a git kit to what its remote ref holds now, and a folder kit's
    /// recorded version to what its manifest says. A kit pinned to a tag moves to
    /// the tag the catalog names, which it then is pinned to, and never back to an
    /// older version; with no other tag named, it stays as it is.
    ///
    /// The new commit is fetched and read in full aside — its manifest, this
    /// Hangar's version against it, its id — before the kit's folder moves to it,
    /// so an update that could not be used leaves the kit as it was. If recording
    /// the update fails once it moved, the folder goes back to the commit it held.
    pub fn update(&self, id: &str, catalog_tag: Option<&str>) -> Result<Entry, String> {
        let kit = kit_lock(id);
        let _kit = hold(&kit);
        let entry = self.find(id)?;
        let Source::Git(source) = &entry.source else {
            let _guard = lock();
            let mut index = self.load()?;
            let at = position(&index, id)?;
            index.kits[at].installed_version = manifest::read(Path::new(&entry.dir))?.version;
            self.save(&index)?;
            checks().remove(&(self.root.clone(), id.to_string()));
            return Ok(index.kits[at].clone());
        };
        let Some(target) = source::update_target(source, catalog_tag) else {
            checks().remove(&(self.root.clone(), id.to_string()));
            return Ok(entry);
        };
        let prepared = self.prepare(&entry, &target)?;
        if source::pinned_tag(source).is_some()
            && !is_later(&prepared.version, &entry.installed_version)
        {
            return Err(format!(
                "the catalog names the tag {}, which holds version {}: not later than the installed {}",
                source::pinned_tag(&target).unwrap_or_default(),
                prepared.version,
                entry.installed_version
            ));
        }

        let _guard = lock();
        let mut index = self.load()?;
        let at = position(&index, id)?;
        if index.kits[at] != entry {
            return Err("the kit changed while its update was fetched; try again".into());
        }
        index.check_pin(&target, &prepared.commit)?;
        let clone = self.clone_dir(id);
        let previous = source::head(&clone)?;
        let hooks_before = self
            .loaded(&entry)
            .ok()
            .and_then(|kit| kit.hooks_permission());
        source::checkout(&clone, &prepared.commit)?;
        let hooks_now = self
            .loaded(&entry)
            .ok()
            .and_then(|kit| kit.hooks_permission());
        if hooks_now != hooks_before {
            index.kits[at].hooks_consent = true;
        }
        index.pin(&target, &prepared.commit);
        index.kits[at].source = Source::Git(target);
        index.kits[at].installed_version = prepared.version;
        index.kits[at].commit = Some(prepared.commit);
        if let Err(why) = self.save(&index) {
            return Err(match source::checkout(&clone, &previous) {
                Ok(()) => format!("the update could not be recorded ({why}); the kit is as it was"),
                Err(back) => format!(
                    "the update could not be recorded ({why}), nor the kit put back as it was ({back}); remove it and add it again"
                ),
            });
        }
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

    /// Fetches the remote ref, and reads what it holds in full, written out
    /// aside: a manifest that holds, for this Hangar, of this very kit. The
    /// commit is checked against the one recorded for its tag, if any.
    fn prepare(&self, entry: &Entry, source: &GitSource) -> Result<Prepared, String> {
        let clone = self.clone_dir(&entry.id);
        let commit = source::fetch(source, &clone)?;
        self.load()?.check_pin(source, &commit)?;
        let path = match &source.subdir {
            Some(subdir) => format!("{subdir}/{}", manifest::FILE),
            None => manifest::FILE.to_string(),
        };
        let text = source::file_at(&clone, &commit, &path)
            .map_err(|_| format!("the remote holds no {path} any more"))?;
        // Its id first: a remote that has become another kit is said as such.
        if let Some((remote_id, _)) = manifest::version_of(&text) {
            if remote_id != entry.id {
                return Err(format!(
                    "the remote now holds the kit {remote_id:?}, not {:?}",
                    entry.id
                ));
            }
        }
        let aside = self
            .root
            .join(format!(".check-{}", uuid::Uuid::new_v4().simple()));
        let read = (|| {
            source::export(&clone, &commit, &aside)?;
            let root = kit_root(&aside, source.subdir.as_deref())?;
            let manifest = manifest::read(&root)
                .map_err(|why| format!("the new version cannot be used: {why}"))?;
            if manifest.id != entry.id {
                return Err(format!(
                    "the remote now holds the kit {:?}, not {:?}",
                    manifest.id, entry.id
                ));
            }
            manifest::compatible(&manifest, HANGAR_VERSION).map_err(|why| {
                format!("version {} cannot be used here: {why}", manifest.version)
            })?;
            Ok(manifest.version)
        })();
        let _ = remove_tree(&aside);
        Ok(Prepared {
            commit,
            version: read?,
        })
    }
}

fn position(index: &Index, id: &str) -> Result<usize, String> {
    index
        .kits
        .iter()
        .position(|entry| entry.id == id)
        .ok_or_else(|| format!("no kit {id:?} is installed"))
}

/// Whether `latest` is an update of `current`. A version that is not semver has
/// nothing to compare by, and any other one counts.
fn is_later(latest: &str, current: &str) -> bool {
    match (
        semver::Version::parse(latest),
        semver::Version::parse(current),
    ) {
        (Ok(latest), Ok(current)) => latest > current,
        _ => latest != current,
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
        // A git just killed on a deadline still holds its files open for a
        // moment on Windows, and the removal fails while it does: a few short
        // tries let the handles go instead of leaving the folder behind.
        #[cfg(windows)]
        Err(_) => {
            let mut last = Ok(());
            for attempt in 0..6 {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
                clear_readonly(path);
                last = match fs::remove_dir_all(path) {
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    other => other,
                };
                if last.is_ok() {
                    break;
                }
            }
            last
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
    use crate::kits::testing::{git, Folder, Remote};
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
        assert_eq!(store.check_update("studio", 1, None).unwrap(), None);
        assert_eq!(
            store.update("studio", None).unwrap().installed_version,
            "1.1.0"
        );

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
        let check = store.check_update("studio", 8, None).unwrap().unwrap();
        assert_eq!((check.latest.as_str(), check.available), ("1.0.0", false));

        remote.folder.manifest("work", "studio", "1.2.0");
        remote
            .folder
            .file("work/skills/more/SKILL.md", "---\nname: more\n---\n");
        remote.push("second");
        let check = store.check_update("studio", 9, None).unwrap().unwrap();
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

        let updated = store.update("studio", None).unwrap();
        assert_eq!(updated.installed_version, "1.2.0");
        assert!(clone.join("skills/more/SKILL.md").is_file());
        assert!(
            clone.join("installed.txt").is_file(),
            "untracked files stay"
        );
        assert_eq!(store.update_info("studio"), None, "the check is stale now");
        assert!(
            !store
                .check_update("studio", 10, None)
                .unwrap()
                .unwrap()
                .available
        );

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
        store.update("linked", None).unwrap();
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
        let same = store.update("studio", None).unwrap();
        assert!(!same.hooks_consent);
        assert_eq!(same.installed_version, "1.1.0");
        assert_eq!(
            store.loaded(&same).unwrap().hooks_permission().unwrap(),
            before
        );

        with_hooks(&remote, "two.sh");
        remote.folder.manifest("work", "studio", "1.2.0");
        remote.push("other hooks");
        let changed = store.update("studio", None).unwrap();
        assert!(changed.hooks_consent);
        assert_ne!(
            store.loaded(&changed).unwrap().hooks_permission().unwrap(),
            before
        );
    }

    #[test]
    fn an_update_that_changes_only_what_else_runs_is_a_change() {
        let (_home, store) = store("consent-running");
        let remote = Remote::new("consent-running-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        with_hooks(&remote, "one.sh");
        remote.push("first");
        cloned(&store, &remote);
        forget_consent(&store, "studio");
        let before = store
            .loaded(&store.entry("studio").unwrap())
            .unwrap()
            .hooks_permission()
            .unwrap();

        // The same hooks, another command line for its MCP server.
        remote.folder.file(
            "work/.mcp.json",
            &json!({ "mcpServers": { "studio": { "command": "node", "args": ["other.js"] } } })
                .to_string(),
        );
        remote.folder.manifest("work", "studio", "1.1.0");
        remote.push("another server");
        let changed = store.update("studio", None).unwrap();
        assert!(changed.hooks_consent);
        assert_ne!(
            store.loaded(&changed).unwrap().hooks_permission().unwrap(),
            before
        );

        // A monitor on top of that.
        let mcp_changed = store
            .loaded(&store.entry("studio").unwrap())
            .unwrap()
            .hooks_permission()
            .unwrap();
        remote.folder.file(
            "work/monitors/monitors.json",
            &json!([{ "name": "log", "command": "tail -F log", "description": "d" }]).to_string(),
        );
        remote.folder.manifest("work", "studio", "1.2.0");
        remote.push("a monitor");
        let watched = store.update("studio", None).unwrap();
        assert_ne!(
            store.loaded(&watched).unwrap().hooks_permission().unwrap(),
            mcp_changed
        );
    }

    #[test]
    fn an_update_that_brings_hooks_to_a_kit_without_any_is_a_change() {
        let (_home, store) = store("consent-gained");
        let remote = Remote::new("consent-gained-remote");
        remote.folder.manifest("work", "studio", "1.0.0");
        remote.folder.plugin("work", "studio");
        remote.push("first");
        let entry = cloned(&store, &remote);
        assert_eq!(store.loaded(&entry).unwrap().hooks_permission(), None);
        forget_consent(&store, "studio");

        with_hooks(&remote, "new.sh");
        remote.folder.manifest("work", "studio", "1.1.0");
        remote.push("hooks now");
        let updated = store.update("studio", None).unwrap();
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
        assert!(
            store
                .check_update("studio", 2, None)
                .unwrap()
                .unwrap()
                .available
        );
        assert_eq!(
            store.update("studio", None).unwrap().installed_version,
            "2.1.0"
        );
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
            .check_update("studio", 2, None)
            .unwrap_err()
            .contains("\"other\""));
        assert!(store.update("studio", None).is_err());
        assert_eq!(store.entry("studio").unwrap().installed_version, "1.0.0");
    }

    /// Tags what the working copy holds, and pushes the tag.
    fn tag(remote: &Remote, name: &str) {
        git(&remote.work(), &["tag", name]);
        git(
            &remote.work(),
            &["push", "-q", "origin", &format!("refs/tags/{name}")],
        );
    }

    /// Commits a new version of the kit `studio` and tags it.
    fn release(remote: &Remote, version: &str) {
        remote.folder.manifest("work", "studio", version);
        remote.push(version);
        tag(remote, &format!("v{version}"));
    }

    /// As the gallery installs: the link `…/tree/<tree>`, read against the remote.
    fn from_tree(store: &Store, remote: &Remote, tree: &[&str]) -> Entry {
        store
            .add_git_link(
                source::Link {
                    url: remote.url(),
                    tree: tree.iter().map(|part| part.to_string()).collect(),
                },
                1,
            )
            .unwrap()
    }

    fn git_ref(entry: &Entry) -> Option<&str> {
        match &entry.source {
            Source::Git(source) => source.git_ref.as_deref(),
            Source::Folder { .. } => None,
        }
    }

    fn in_clone(store: &Store, id: &str, args: &[&str]) -> String {
        source::git(Some(store.root().join(id).as_path()), args)
            .unwrap()
            .trim()
            .to_string()
    }

    /// The version the clone's manifest declares, on disk.
    fn on_disk(store: &Store, id: &str) -> String {
        store
            .loaded(&store.entry(id).unwrap())
            .unwrap()
            .manifest
            .version
    }

    /// `(latest, available)` of a check against the tag the catalog names.
    fn check(store: &Store, catalog_tag: Option<&str>) -> (String, bool) {
        let info = store
            .check_update("studio", 1, catalog_tag)
            .unwrap()
            .unwrap();
        (info.latest, info.available)
    }

    #[test]
    fn a_tag_link_with_no_folder_is_a_shallow_clone_pinned_to_that_tag() {
        let (_home, store) = store("tag-link");
        let remote = Remote::new("tag-link-remote");
        remote.folder.kit("work", "studio", "0.9.0");
        remote.push("an older commit");
        release(&remote, "1.0.0");
        remote.folder.manifest("work", "studio", "2.0.0");
        remote.push("the head of the branch");

        let entry = from_tree(&store, &remote, &["v1.0.0"]);
        assert_eq!(git_ref(&entry), Some("refs/tags/v1.0.0"));
        assert_eq!(entry.installed_version, "1.0.0");
        assert_eq!(on_disk(&store, "studio"), "1.0.0");
        assert!(entry.hooks_consent, "accepted like any git kit");
        assert_eq!(
            Path::new(&entry.dir),
            dunce::canonicalize(store.root().join("studio")).unwrap()
        );

        let clone = |args: &[&str]| in_clone(&store, "studio", args);
        assert_eq!(clone(&["rev-parse", "--is-shallow-repository"]), "true");
        assert_eq!(clone(&["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(clone(&["tag"]), "", "no tag comes along");
    }

    #[test]
    fn an_annotated_tag_pins_and_updates_like_a_light_one() {
        let (_home, store) = store("tag-annotated");
        let remote = Remote::new("tag-annotated-remote");
        let annotate = |name: &str| {
            git(&remote.work(), &["tag", "-a", name, "-m", name]);
            git(
                &remote.work(),
                &["push", "-q", "origin", &format!("refs/tags/{name}")],
            );
        };
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        annotate("v1");
        let entry = from_tree(&store, &remote, &["v1"]);
        assert_eq!(git_ref(&entry), Some("refs/tags/v1"));
        assert_eq!(on_disk(&store, "studio"), "1.0.0");

        remote.folder.manifest("work", "studio", "1.1.0");
        remote.push("second");
        annotate("v2");
        assert_eq!(check(&store, Some("v2")), ("1.1.0".to_string(), true));
        let updated = store.update("studio", Some("v2")).unwrap();
        assert_eq!(git_ref(&updated), Some("refs/tags/v2"));
        assert_eq!(on_disk(&store, "studio"), "1.1.0");
    }

    #[test]
    fn a_tag_link_can_name_a_folder_it_keeps_across_an_update() {
        let (_home, store) = store("tag-folder");
        let remote = Remote::new("tag-folder-remote");
        remote.folder.kit("work/kits/studio", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1");
        let entry = from_tree(&store, &remote, &["v1", "kits", "studio"]);
        assert_eq!(git_ref(&entry), Some("refs/tags/v1"));
        assert!(entry.dir.ends_with("studio"));

        remote
            .folder
            .manifest("work/kits/studio", "studio", "1.1.0");
        remote.push("second");
        tag(&remote, "v2");
        let updated = store.update("studio", Some("v2")).unwrap();
        assert_eq!(updated.dir, entry.dir);
        assert_eq!(on_disk(&store, "studio"), "1.1.0");
    }

    #[test]
    fn a_new_version_of_a_pinned_kit_appears_only_when_the_catalog_names_its_tag() {
        let (_home, store) = store("tag-follow");
        let remote = Remote::new("tag-follow-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1.0.0");
        from_tree(&store, &remote, &["v1.0.0"]);
        let same = ("1.0.0".to_string(), false);

        // The branch moves.
        remote.folder.manifest("work", "studio", "1.5.0");
        remote.push("the branch moves on");
        assert_eq!(check(&store, None), same);
        assert_eq!(check(&store, Some("v1.0.0")), same);

        // A new tag exists, which the catalog does not name yet.
        release(&remote, "1.6.0");
        assert_eq!(check(&store, None), same);
        assert_eq!(check(&store, Some("v1.0.0")), same);
        let kept = store.update("studio", Some("v1.0.0")).unwrap();
        assert_eq!(kept.installed_version, "1.0.0");
        assert_eq!(git_ref(&kept), Some("refs/tags/v1.0.0"));
        assert_eq!(on_disk(&store, "studio"), "1.0.0");

        // The catalog names it, with the branch ahead of it.
        remote.folder.manifest("work", "studio", "1.7.0");
        remote.push("the branch again");
        assert_eq!(check(&store, Some("v1.6.0")), ("1.6.0".to_string(), true));
        fs::write(store.root().join("studio/installed.txt"), "local").unwrap();
        let updated = store.update("studio", Some("v1.6.0")).unwrap();
        assert_eq!(updated.installed_version, "1.6.0");
        assert_eq!(git_ref(&updated), Some("refs/tags/v1.6.0"));
        assert_eq!(on_disk(&store, "studio"), "1.6.0");
        assert!(
            store.root().join("studio/installed.txt").is_file(),
            "untracked files stay"
        );
        assert_eq!(store.update_info("studio"), None, "the check is stale now");
        assert_eq!(
            in_clone(&store, "studio", &["rev-parse", "--is-shallow-repository"]),
            "true"
        );

        // On the disk's index too, and the kit now stays on that one.
        assert_eq!(
            git_ref(&store.entry("studio").unwrap()),
            Some("refs/tags/v1.6.0")
        );
        assert_eq!(check(&store, Some("v1.6.0")), ("1.6.0".to_string(), false));
        assert_eq!(check(&store, None), ("1.6.0".to_string(), false));
    }

    #[test]
    fn a_tag_moved_by_its_owner_reaches_nobody_until_the_catalog_names_another() {
        let (_home, store) = store("tag-moved");
        let remote = Remote::new("tag-moved-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1");
        from_tree(&store, &remote, &["v1"]);

        remote.folder.manifest("work", "studio", "9.0.0");
        remote.push("not reviewed");
        git(&remote.work(), &["tag", "-f", "v1"]);
        git(
            &remote.work(),
            &["push", "-q", "-f", "origin", "refs/tags/v1"],
        );

        assert_eq!(check(&store, Some("v1")), ("1.0.0".to_string(), false));
        let kept = store.update("studio", Some("v1")).unwrap();
        assert_eq!(kept.installed_version, "1.0.0");
        assert_eq!(on_disk(&store, "studio"), "1.0.0");
    }

    #[test]
    fn a_pinned_kit_is_not_taken_back_to_an_older_version() {
        let (_home, store) = store("tag-back");
        let remote = Remote::new("tag-back-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1.0.0");
        release(&remote, "1.1.0");
        from_tree(&store, &remote, &["v1.1.0"]);

        assert_eq!(check(&store, Some("v1.0.0")), ("1.0.0".to_string(), false));
        assert!(store
            .update("studio", Some("v1.0.0"))
            .unwrap_err()
            .contains("not later"));
        let kept = store.entry("studio").unwrap();
        assert_eq!(kept.installed_version, "1.1.0");
        assert_eq!(git_ref(&kept), Some("refs/tags/v1.1.0"));
        assert_eq!(on_disk(&store, "studio"), "1.1.0");
    }

    #[test]
    fn a_tag_the_catalog_names_that_cannot_be_used_changes_nothing() {
        let (_home, store) = store("tag-bad");
        let remote = Remote::new("tag-bad-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1");
        from_tree(&store, &remote, &["v1"]);

        assert!(store.check_update("studio", 2, Some("v404")).is_err());
        assert!(store.update("studio", Some("v404")).is_err());

        remote.folder.manifest("work", "other", "9.0.0");
        remote.push("another kit");
        tag(&remote, "v9");
        assert!(store
            .update("studio", Some("v9"))
            .unwrap_err()
            .contains("\"other\""));

        let kept = store.entry("studio").unwrap();
        assert_eq!(kept.installed_version, "1.0.0");
        assert_eq!(git_ref(&kept), Some("refs/tags/v1"));
        assert_eq!(on_disk(&store, "studio"), "1.0.0");
    }

    #[test]
    fn an_update_to_a_new_tag_goes_through_consent_like_any_other() {
        let (_home, store) = store("tag-consent");
        let remote = Remote::new("tag-consent-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        with_hooks(&remote, "one.sh");
        remote.push("first");
        tag(&remote, "v1.0.0");
        from_tree(&store, &remote, &["v1.0.0"]);
        forget_consent(&store, "studio");
        let permission = || {
            store
                .loaded(&store.entry("studio").unwrap())
                .unwrap()
                .hooks_permission()
                .unwrap()
        };
        let before = permission();

        release(&remote, "1.1.0");
        let same = store.update("studio", Some("v1.1.0")).unwrap();
        assert!(!same.hooks_consent);
        assert_eq!(permission(), before);

        with_hooks(&remote, "two.sh");
        release(&remote, "1.2.0");
        let changed = store.update("studio", Some("v1.2.0")).unwrap();
        assert!(changed.hooks_consent);
        assert_ne!(
            permission(),
            before,
            "what the workspace has to accept again"
        );
    }

    #[test]
    fn a_kit_on_a_branch_follows_it_whatever_the_catalog_names() {
        for (tag_name, tree, wanted) in [
            ("plain", vec![], None),
            ("branch", vec!["main"], Some("main")),
        ] {
            let (_home, store) = store(tag_name);
            let remote = Remote::new(tag_name);
            remote.folder.kit("work", "studio", "1.0.0");
            remote.push("first");
            tag(&remote, "v5");
            let entry = from_tree(&store, &remote, &tree);
            assert_eq!(git_ref(&entry), wanted);

            remote.folder.manifest("work", "studio", "1.1.0");
            remote.push("the branch moves on");
            assert_eq!(check(&store, Some("v5")), ("1.1.0".to_string(), true));
            let updated = store.update("studio", Some("v5")).unwrap();
            assert_eq!(updated.installed_version, "1.1.0");
            assert_eq!(git_ref(&updated), wanted);
        }
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
        assert!(store.update("..", None).is_err() && store.remove("..").is_err());
    }

    /// The commit the clone of `id` holds.
    fn head_of(store: &Store, id: &str) -> String {
        in_clone(store, id, &["rev-parse", "HEAD"])
    }

    fn no_folder_left_aside(store: &Store) {
        let left: Vec<String> = fs::read_dir(store.root())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn an_update_that_changes_only_a_script_its_hooks_run_is_a_change() {
        let (_home, store) = store("consent-script");
        let remote = Remote::new("consent-script-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        with_hooks(&remote, "node ${CLAUDE_PLUGIN_ROOT}/guard.js");
        remote.folder.file("work/guard.js", "console.log('one')");
        remote.push("first");
        cloned(&store, &remote);
        forget_consent(&store, "studio");
        let permission = || {
            store
                .loaded(&store.entry("studio").unwrap())
                .unwrap()
                .hooks_permission()
                .unwrap()
        };
        let before = permission();

        // What nothing runs: as it was.
        remote.folder.file("work/README.md", "docs");
        remote.folder.manifest("work", "studio", "1.1.0");
        remote.push("docs");
        assert!(!store.update("studio", None).unwrap().hooks_consent);
        assert_eq!(permission(), before);

        // The same command line, another script behind it.
        remote
            .folder
            .file("work/guard.js", "require('child_process')");
        remote.folder.manifest("work", "studio", "1.2.0");
        remote.push("another guard");
        let changed = store.update("studio", None).unwrap();
        assert!(changed.hooks_consent, "the workspace is asked again");
        assert_ne!(permission(), before);

        // A program in bin/, on the PATH of Claude's shell, counts as well.
        let with_guard = permission();
        remote.folder.file("work/bin/tool", "#!/bin/sh\necho two");
        remote.folder.manifest("work", "studio", "1.3.0");
        remote.push("a tool");
        store.update("studio", None).unwrap();
        assert_ne!(permission(), with_guard);
    }

    #[test]
    fn an_update_that_changes_only_a_script_a_command_runs_needs_another_grant() {
        let (_home, store) = store("consent-command");
        let remote = Remote::new("consent-command-remote");
        let manifest = |version: &str| {
            json!({
                "schema": 1, "id": "studio", "name": "studio", "version": version,
                "description": "d",
                "commands": { "audit": { "description": "d", "run": ["node", "{kitDir}/tools/audit.js"] } }
            })
            .to_string()
        };
        remote
            .folder
            .file("work/hangar-kit.json", &manifest("1.0.0"));
        remote.folder.file("work/tools/audit.js", "one");
        remote.push("first");
        let entry = cloned(&store, &remote);
        let os = manifest::current_os();
        let before = store
            .loaded(&entry)
            .unwrap()
            .commands_permission(os)
            .unwrap();

        remote
            .folder
            .file("work/hangar-kit.json", &manifest("1.1.0"));
        remote.folder.file("work/tools/audit.js", "two");
        remote.push("another audit");
        let updated = store.update("studio", None).unwrap();
        let after = store
            .loaded(&updated)
            .unwrap()
            .commands_permission(os)
            .unwrap();
        assert_ne!(
            after, before,
            "the grant for the old script does not run the new one"
        );
    }

    #[test]
    fn an_update_that_could_not_be_used_leaves_the_kit_as_it_was() {
        let (_home, store) = store("update-bad");
        let remote = Remote::new("update-bad-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        cloned(&store, &remote);
        let head = head_of(&store, "studio");

        for (why, manifest) in [
            (
                "needs Hangar",
                json!({ "schema": 1, "id": "studio", "name": "studio", "version": "1.1.0",
                        "description": "d", "hangar": ">=99.0.0" }),
            ),
            (
                "cannot be used",
                json!({ "schema": 1, "id": "studio", "version": "1.2.0", "description": "d",
                        "tabs": [{ "id": "t", "title": "T", "page": "missing.html" }] }),
            ),
        ] {
            remote
                .folder
                .file("work/hangar-kit.json", &manifest.to_string());
            remote.push(why);
            let err = store.check_update("studio", 2, None).unwrap_err();
            assert!(err.contains(why), "{err}");
            let err = store.update("studio", None).unwrap_err();
            assert!(err.contains(why), "{err}");
            assert_eq!(head_of(&store, "studio"), head, "not checked out");
            assert_eq!(on_disk(&store, "studio"), "1.0.0");
            let kept = store.entry("studio").unwrap();
            assert_eq!(kept.installed_version, "1.0.0");
            assert!(store.loaded(&kept).is_ok(), "still usable");
            no_folder_left_aside(&store);
        }

        // A good version after them goes through.
        remote.folder.manifest("work", "studio", "1.3.0");
        remote.push("fixed");
        let updated = store.update("studio", None).unwrap();
        assert_eq!(updated.installed_version, "1.3.0");
        assert_eq!(
            updated.commit.as_deref(),
            Some(head_of(&store, "studio").as_str())
        );
        assert_ne!(head_of(&store, "studio"), head);
    }

    #[test]
    fn the_commit_of_a_tag_is_recorded_and_a_tag_moved_since_is_refused() {
        let (_home, store) = store("tag-commit");
        let remote = Remote::new("tag-commit-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1");
        let entry = from_tree(&store, &remote, &["v1"]);
        let reviewed = head_of(&store, "studio");
        assert_eq!(entry.commit.as_deref(), Some(reviewed.as_str()));
        assert_eq!(
            store.entry("studio").unwrap().commit,
            entry.commit,
            "on disk"
        );

        // Removed, and its tag moved by its owner meanwhile.
        store.remove("studio").unwrap();
        remote.folder.manifest("work", "studio", "1.0.0");
        remote.folder.file("work/hooks/hooks.json", "{}");
        remote.push("not reviewed");
        git(&remote.work(), &["tag", "-f", "v1"]);
        git(
            &remote.work(),
            &["push", "-q", "-f", "origin", "refs/tags/v1"],
        );
        let err = store
            .add_git_link(
                source::Link {
                    url: remote.url(),
                    tree: vec!["v1".into()],
                },
                2,
            )
            .unwrap_err();
        assert!(
            err.contains("moved") && err.contains(&reviewed[..12]),
            "{err}"
        );
        assert!(store.entries().unwrap().is_empty());
        no_folder_left_aside(&store);
        assert!(!store.root().join("studio").exists());

        // Another tag is new to this machine: taken, and recorded in turn.
        tag(&remote, "v2");
        let other = from_tree(&store, &remote, &["v2"]);
        assert_eq!(
            other.commit.as_deref(),
            Some(head_of(&store, "studio").as_str())
        );
        let pins = store.load().unwrap().pins;
        assert_eq!(pins.len(), 2, "{pins:?}");
    }

    #[test]
    fn an_update_to_a_tag_that_moved_since_it_was_seen_is_refused() {
        let (_home, store) = store("tag-commit-update");
        let remote = Remote::new("tag-commit-update-remote");
        remote.folder.kit("work", "studio", "1.0.0");
        remote.push("first");
        tag(&remote, "v1");
        release(&remote, "1.1.0");
        from_tree(&store, &remote, &["v1.1.0"]);
        // Back to v1, by hand, then forward again: v1.1.0 is known.
        store.remove("studio").unwrap();
        from_tree(&store, &remote, &["v1"]);

        remote.folder.manifest("work", "studio", "1.1.0");
        remote.folder.file("work/extra.txt", "moved");
        remote.push("moved");
        git(&remote.work(), &["tag", "-f", "v1.1.0"]);
        git(
            &remote.work(),
            &["push", "-q", "-f", "origin", "refs/tags/v1.1.0"],
        );
        assert!(store
            .check_update("studio", 2, Some("v1.1.0"))
            .unwrap_err()
            .contains("moved"));
        assert!(store
            .update("studio", Some("v1.1.0"))
            .unwrap_err()
            .contains("moved"));
        assert_eq!(
            git_ref(&store.entry("studio").unwrap()),
            Some("refs/tags/v1")
        );
        assert_eq!(on_disk(&store, "studio"), "1.0.0");
    }

    #[test]
    fn a_stalled_network_does_not_hold_the_store() {
        let (_home, store) = store("stalled");
        let server = source::tests::stalled_server();
        let url = server.url.clone();
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let store = &store;
            scope.spawn(move || {
                source::TEST_DEADLINE.with(|d| d.set(Some(Duration::from_secs(4))));
                let result = store.add_git_source(
                    GitSource {
                        url,
                        git_ref: None,
                        subdir: None,
                    },
                    1,
                );
                done.send(result).unwrap();
            });
            std::thread::sleep(Duration::from_millis(500));
            // While the clone waits on the network, the store goes on.
            let folder = Folder::new("stalled-src");
            let dir = folder.kit(".", "linked", "1.0.0");
            store.add_folder(&dir.to_string_lossy(), 1).unwrap();
            store.remove("linked").unwrap();
            assert!(
                finished.try_recv().is_err(),
                "the clone was still waiting meanwhile"
            );
            let err = finished.recv().unwrap().unwrap_err();
            assert!(err.contains("longer than"), "{err}");
        });
        no_folder_left_aside(&store);
    }
}
