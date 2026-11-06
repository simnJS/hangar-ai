//! Loading the models the store says are open (`useModelLoads`, and the
//! effects of `ModelView` that call it).
//!
//! A model is read off the window's thread by hangar-model3d, one load per
//! entry at a time: a newer revision replaces the one under way, and every
//! outcome is reported to the store with `settle_model`, which is what an
//! agent waiting on `model_show` hears.
//!
//! Which models are loaded:
//!
//! - the ones the store marks as loading (an agent just showed them, or their
//!   file was rewritten), in any workspace, so the agent hears what the viewer
//!   found and the person finds the model ready;
//! - those whose pane is on screen, when not in memory (dropped to make room
//!   for others, or their workspace was asleep);
//! - any a snapshot asks for (`Msg::Ensure`), even in a workspace asleep: it
//!   is held open (`State::ensured`) until a moment after its load is over.
//!
//! Closed tabs let go of their models, and so does a workspace put to sleep:
//! its models stay listed and load again once it is back on screen. Of the
//! models nobody is looking at, the last [`KEEP_LOADED`] used stay in memory.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::Task;

use hangar_model3d::{LoadError, LoadEvent, LoadOptions, Model};

use super::slot::{Flight, Slot};
use super::theme::viewer_theme_of;
use super::{Msg, State};
use crate::message::Message;
use crate::preview::access::Shared;
use crate::preview::roots;
use crate::preview::store::{ModelInfo, Status};
use crate::state::{Model as AppModel, Workspace, WsId};

/// Models kept in memory beside the ones on screen (`KEEP_LOADED` of the
/// engine): a model is up to hundreds of megabytes once decoded, and a person
/// who has opened a dozen should not pay for all of them.
pub const KEEP_LOADED: usize = 3;

/// How long a model a snapshot asked for is held open, from the ask: the
/// snapshot's own wait, and a margin.
pub const ENSURE_HOLD: Duration = Duration::from_secs(60);

/// How long it stays held once its load has landed or failed: long enough for
/// the snapshot waiting on the registry to take the model (it then holds its
/// own `Arc`), short enough for a sleeping workspace's model to go soon after.
pub const ENSURE_GRACE: Duration = Duration::from_secs(10);

/// An open model, as the store has it now.
#[derive(Debug, Clone)]
pub struct Want {
    pub id: String,
    pub ws: WsId,
    pub path: String,
    pub revision: u64,
    pub status: Status,
    /// Its pane is on screen.
    pub front: bool,
    /// And a window shows the workspace.
    pub on_screen: bool,
}

/// Every open 3D model, of every workspace.
pub fn wants(shared: &Shared) -> Vec<Want> {
    shared.read(|store| {
        store
            .models()
            .into_iter()
            .map(|entry| Want {
                id: entry.id.clone(),
                ws: entry.workspace_id.clone(),
                path: entry.path.clone(),
                revision: entry.revision,
                status: entry.status,
                front: store.view().shows(&entry.id),
                on_screen: store.view().on_screen(&entry.workspace_id),
            })
            .collect()
    })
}

/// What the store is told a model contains (`settleModel({info})`).
pub fn info_of(model: &Model) -> ModelInfo {
    let info = &model.info;
    ModelInfo {
        triangles: info.triangles as u64,
        vertices: info.vertices as u64,
        meshes: info.meshes as u64,
        materials: info.materials as u64,
        animations: info.animations.clone(),
        size: info.size,
        center: info.center,
    }
}

/// Brings the slots in line with the store: loads what is to be loaded, lets
/// go of what is closed or asleep, keeps the registry the snapshots read in
/// step.
pub fn reconcile(st: &mut State, m: &AppModel) -> Task<Message> {
    let wants = wants(&st.shared);
    let asleep: HashSet<&str> = wants
        .iter()
        .filter(|want| m.live.panes_logic.is_asleep(&want.ws))
        .map(|want| want.ws.as_str())
        .collect();
    settle(st, m, &wants, &asleep)
}

/// `reconcile`, given which workspaces are asleep.
pub(super) fn settle(
    st: &mut State,
    m: &AppModel,
    wants: &[Want],
    asleep: &HashSet<&str>,
) -> Task<Message> {
    // A model a snapshot asked for stays, asleep or not, until its hold ends.
    let now = Instant::now();
    st.ensured.retain(|_, until| *until > now);
    let ensured = &st.ensured;
    let awake: Vec<&Want> = wants
        .iter()
        .filter(|want| !asleep.contains(want.ws.as_str()) || ensured.contains_key(&want.id))
        .collect();

    // Closed tabs let go of their models, and so does a workspace put to sleep.
    let open: HashSet<&str> = awake.iter().map(|want| want.id.as_str()).collect();
    let gone: Vec<String> = st
        .slots
        .keys()
        .filter(|id| !open.contains(id.as_str()))
        .cloned()
        .collect();
    let closed = !gone.is_empty();
    for id in gone {
        if let Some(mut slot) = st.slots.remove(&id) {
            slot.abandon();
        }
        st.registry.remove(&id);
    }
    // A model's meshes and textures, hundreds of megabytes for a large one,
    // go back to the system with it (`preview::heap`).
    if closed {
        crate::preview::heap::give_back_soon();
    }

    // The registry knows the theme each workspace is drawn in, so a snapshot
    // is painted in the colours of the workspace it is of.
    let themes: HashMap<WsId, _> = awake
        .iter()
        .map(|want| (want.ws.clone(), viewer_theme_of(m, &want.ws)))
        .collect();
    st.registry.set_themes(themes);

    let mut tasks = Vec::new();
    for want in &awake {
        let slot = st
            .slots
            .entry(want.id.clone())
            .or_insert_with(|| Slot::new(want.ws.clone(), st.display, st.reduced_motion));
        slot.ws = want.ws.clone();
        if want.front && want.on_screen {
            st.clock += 1;
            slot.used = st.clock;
        }
        // A newer version of the file replaces the load under way.
        if slot
            .flight
            .as_ref()
            .is_some_and(|flight| flight.revision != want.revision)
        {
            slot.abandon();
        }
        let wanted = !slot.loading(want.revision)
            && slot.failed != Some(want.revision)
            && (want.status == Status::Loading || (want.front && !slot.has_content()));
        if wanted {
            tasks.push(begin(st, want));
        }
    }
    evict(st, &awake);
    Task::batch(tasks)
}

/// Starts the load of an entry's current revision: first the folders its
/// side files may come from are worked out, off the window's thread, then
/// `Msg::Roots` starts the read.
pub fn begin(st: &mut State, want: &Want) -> Task<Message> {
    let Some(slot) = st.slots.get_mut(&want.id) else {
        return Task::none();
    };
    slot.abandon();
    slot.flight = Some(Flight::new(want.revision));
    let workspace = st.shared.workspace(&want.ws);
    let (id, revision, path) = (want.id.clone(), want.revision, want.path.clone());
    Task::perform(roots_for(workspace, path), move |roots| {
        Message::Model3d(Msg::Roots(id.clone(), revision, roots))
    })
}

/// The read, once its folders are known.
pub fn read(st: &mut State, id: &str, revision: u64, roots: Vec<PathBuf>) -> Task<Message> {
    let Some(slot) = st.slots.get_mut(id) else {
        return Task::none();
    };
    let Some(flight) = slot
        .flight
        .as_mut()
        .filter(|flight| flight.revision == revision && flight.cancel.is_none())
    else {
        // Replaced or abandoned while the folders were worked out.
        return Task::none();
    };
    let Some(path) = st
        .shared
        .read(|store| store.entry(id).map(|entry| entry.path.clone()))
    else {
        return Task::none();
    };
    let (task, cancel) =
        hangar_model3d::load_task(PathBuf::from(path), LoadOptions { roots: Some(roots) });
    flight.cancel = Some(cancel);
    let id = id.to_string();
    let ended = Task::done(Message::Model3d(Msg::LoadEnded(id.clone(), revision)));
    task.map(move |event| Message::Model3d(Msg::Load(id.clone(), revision, event)))
        .chain(ended)
}

/// The stream of a read is over. It ends with `Done`, which takes the load out
/// of its slot; a load still under way here is one whose thread died without
/// saying how, and would otherwise spin until the file changes.
pub fn ended(st: &mut State, id: &str, revision: u64) {
    let Some(slot) = st.slots.get_mut(id) else {
        return;
    };
    let reading = slot
        .flight
        .as_ref()
        .is_some_and(|flight| flight.revision == revision && flight.cancel.is_some());
    if !reading {
        return;
    }
    slot.flight = None;
    failed(
        st,
        id,
        revision,
        "The model's reader stopped without saying why: show the file again to retry.".into(),
    );
}

/// What a running load reported.
pub fn progressed(st: &mut State, id: &str, revision: u64, event: LoadEvent) {
    let Some(slot) = st.slots.get_mut(id) else {
        return;
    };
    // An event of a load that was replaced, or that the person cancelled.
    if !slot.loading(revision) {
        return;
    }
    match event {
        LoadEvent::Progress(progress) => {
            if let Some(flight) = slot.flight.as_mut() {
                flight.progress = Some(progress);
            }
        }
        LoadEvent::Done(result) => {
            slot.flight = None;
            match result {
                Ok(model) => landed(st, id, revision, model),
                // Nobody cancels without taking the load out of the slot
                // first, so this is not one we are waiting for.
                Err(LoadError::Cancelled) => {}
                Err(error) => failed(st, id, revision, error.to_string()),
            }
        }
    }
}

/// A model has been read: it goes on show (keeping the person's camera if it
/// is a newer version of the same file), the snapshots can reach it, and the
/// store learns what is in it.
fn landed(st: &mut State, id: &str, revision: u64, model: std::sync::Arc<Model>) {
    let Some(slot) = st.slots.get_mut(id) else {
        return;
    };
    st.clock += 1;
    slot.used = st.clock;
    slot.viewer.set_model(model.clone());
    slot.loaded = Some(revision);
    slot.failed = None;
    st.registry.put(id, revision, &slot.ws, model.clone());
    release_hold(st, id);
    let info = info_of(&model);
    st.shared.edit(|store| store.settle_model(id, Ok(info)));
}

/// A model could not be read: the old version, if any, stays on show, and the
/// store (and the agent waiting on it) is told why.
pub fn failed(st: &mut State, id: &str, revision: u64, message: String) {
    if let Some(slot) = st.slots.get_mut(id) {
        slot.failed = Some(revision);
    }
    st.registry.fail(id, revision, message.clone());
    release_hold(st, id);
    st.shared.edit(|store| store.settle_model(id, Err(message)));
}

/// A load a snapshot asked for is over: the hold ends after a short grace.
pub(super) fn release_hold(st: &mut State, id: &str) {
    if let Some(until) = st.ensured.get_mut(id) {
        *until = (*until).min(Instant::now() + ENSURE_GRACE);
    }
}

/// The person stops a load: the entry shows why, with a way to try again.
pub fn cancel(st: &mut State, id: &str) {
    let Some(slot) = st.slots.get_mut(id) else {
        return;
    };
    let Some(revision) = slot.flight.as_ref().map(|flight| flight.revision) else {
        return;
    };
    slot.abandon();
    failed(st, id, revision, LoadError::Cancelled.to_string());
}

/// Lets go of the models nobody is looking at beyond [`KEEP_LOADED`], the
/// least recently used first. The one in front on a window's screen and any
/// being loaded are never dropped.
fn evict(st: &mut State, awake: &[&Want]) {
    let shown: HashSet<&str> = awake
        .iter()
        .filter(|want| want.front && want.on_screen)
        .map(|want| want.id.as_str())
        .collect();
    let mut resident: Vec<(&String, u64)> = st
        .slots
        .iter()
        .filter(|(id, slot)| {
            slot.has_content() && slot.flight.is_none() && !shown.contains(id.as_str())
        })
        .map(|(id, slot)| (id, slot.used))
        .collect();
    if resident.len() <= KEEP_LOADED {
        return;
    }
    resident.sort_by_key(|(_, used)| *used);
    let surplus = resident.len() - KEEP_LOADED;
    let dropped: Vec<String> = resident
        .into_iter()
        .take(surplus)
        .map(|(id, _)| id.clone())
        .collect();
    for id in dropped {
        if let Some(slot) = st.slots.get_mut(&id) {
            slot.drop_model();
        }
        st.registry.remove(&id);
    }
    crate::preview::heap::give_back_soon();
}

/// The folders a model's own files — a glTF's textures, an OBJ's `.mtl` — may
/// be read from, in the canonical form hangar-model3d compares them in.
///
/// hangar-core holds the list of folders it serves for the whole process, but
/// does not say what it holds; the same folders are worked out here the way
/// the preview store grants them (`Shared::admit`): the workspace's folders
/// and every worktree of their repositories, plus the folder the model is in,
/// which core has already let through.
pub async fn roots_for(workspace: Option<Workspace>, model: String) -> Vec<PathBuf> {
    let mut folders: Vec<String> = Vec::new();
    if let Some(workspace) = &workspace {
        folders.extend(roots::workspace_roots(workspace));
        let repos: Vec<String> = std::iter::once(workspace.cwd.clone())
            .chain(workspace.extra_roots.iter().cloned())
            .collect();
        for repo in repos {
            // A folder that is not a repository, or a machine without git,
            // just adds none.
            if let Ok(listed) = hangar_core::git::git_worktree_list(repo).await {
                folders.extend(
                    listed
                        .into_iter()
                        .filter(|tree| !tree.bare && tree.prunable.is_none())
                        .map(|tree| tree.path),
                );
            }
        }
    }
    if let Some(dir) = Path::new(&model).parent() {
        folders.push(dir.to_string_lossy().into_owned());
    }
    // Canonicalizing touches the disk, a network share's included.
    tokio::task::spawn_blocking(move || canonical(folders))
        .await
        .unwrap_or_default()
}

/// The folders that exist, canonical, each once. A relative folder is dropped:
/// it would resolve against the app's own working directory.
fn canonical(folders: Vec<String>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for folder in folders {
        let path = PathBuf::from(folder.trim());
        if !path.is_absolute() {
            continue;
        }
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        if path.is_dir() && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model3d_view::scratch::Scratch;

    #[test]
    fn folders_are_canonical_each_once_and_only_if_they_exist() {
        let dir = Scratch::new("canonical");
        std::fs::create_dir(dir.join("a")).unwrap();
        let here = dir.path().to_string_lossy().into_owned();
        let listed = canonical(vec![
            here.clone(),
            format!("{here}/a/.."),
            format!("{here}/missing"),
            "relative/folder".into(),
            "  ".into(),
        ]);
        assert_eq!(listed, vec![dir.path().to_path_buf()]);
    }

    #[tokio::test]
    async fn a_models_roots_are_its_workspaces_folders_and_its_own() {
        let work = Scratch::new("roots-work");
        let other = Scratch::new("roots-other");
        std::fs::create_dir(work.join("lib")).unwrap();
        let workspace = Workspace {
            id: "w1".into(),
            cwd: work.path().to_string_lossy().into_owned(),
            extra_roots: vec![work.join("lib").to_string_lossy().into_owned()],
            ..Workspace::default()
        };
        let model = other.join("chair.glb").to_string_lossy().into_owned();
        let roots = roots_for(Some(workspace), model).await;
        assert!(roots.contains(&work.path().to_path_buf()), "{roots:?}");
        assert!(roots.contains(&work.join("lib")), "{roots:?}");
        // Core let the model through, so its own folder is readable too.
        assert!(roots.contains(&other.path().to_path_buf()), "{roots:?}");
    }

    #[tokio::test]
    async fn a_model_with_no_workspace_may_read_its_own_folder_only() {
        let other = Scratch::new("roots-alone");
        let model = other.join("chair.glb").to_string_lossy().into_owned();
        assert_eq!(
            roots_for(None, model).await,
            vec![other.path().to_path_buf()]
        );
    }
}
