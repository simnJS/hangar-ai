//! The package driven as the shell drives it, without a core: a model with a
//! workspace, the preview store, and the messages the tasks of `update` would
//! have ended with, fed by hand (a read's events, the folders it waits for).
//! What the views draw is drawn headless by `iced_test`; with
//! `HANGAR_MODEL3D_SNAPSHOTS=<folder>` the scenes are written there as PNGs
//! (`cargo test -p hangar-app model3d_view::tests::snapshots`).
//!
//! The viewport itself is a GPU widget: on `iced_test`'s software renderer it
//! stays empty, so these tests look at the bars, the cards and the numbers
//! around it. Pictures taken off screen are checked on a machine that has a
//! GPU, and skipped on one that has none (the crate's own tests do the same).

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::{Settings as IcedSettings, Size, window};
use iced_test::Simulator;

use hangar_i18n::{Key, Locale, Translator};
use hangar_model3d::{
    CaptureJob, LoadError, LoadProgress, Model as Mesh, ModelFormat, Phase, SnapshotAngle,
    ViewerTheme, samples,
};
use hangar_theme::{fonts, to_iced};

use super::scratch::Scratch;
use super::snapshots::{Ask, GONE, Look, Snapshots};
use super::*;
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::preview::access::Snapshotter;
use crate::preview::store::{Entry, ShowFile, Status, What};
use crate::services::inbox;
use crate::services::preview::SnapshotAngle as AppAngle;
use crate::state::{AppState, Pane, Workspace};

const MB: u64 = 1024 * 1024;

fn unique(tag: &str) -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{tag}-{}", N.fetch_add(1, Ordering::Relaxed))
}

struct Harness {
    st: State,
    model: Model,
    shared: Arc<Shared>,
    ws: WsId,
    dir: Scratch,
    inbox: Inbox,
    /// Keeps the inbox open.
    _receiver: inbox::InboxReceiver,
    /// The model whose pane is on screen: the last one shown, as the newest
    /// pane is the one the person looks at, until `look_at` says another.
    front: Option<String>,
}

impl Harness {
    /// A workspace of its own over a store of its own.
    fn new(tag: &str) -> Harness {
        Harness::over(tag, Shared::new(None))
    }

    /// The process's store, which the views read through `Model.live.preview`:
    /// the workspace is unique, so tests running side by side do not meet.
    fn global(tag: &str) -> Harness {
        Harness::over(tag, Shared::global())
    }

    fn over(tag: &str, shared: Arc<Shared>) -> Harness {
        let dir = Scratch::new(tag);
        let ws = unique(tag);
        let mut model = Model::default();
        model.hydrate(AppState {
            workspaces: vec![Workspace {
                id: ws.clone(),
                name: "demo".into(),
                cwd: dir.path().to_string_lossy().into_owned(),
                panes: vec![Pane {
                    id: "pane-1".into(),
                    name: "Ava".into(),
                    ..Pane::default()
                }],
                ..Workspace::default()
            }],
            active_workspace_id: Some(ws.clone()),
            ..AppState::default()
        });
        let (inbox, receiver) = inbox::channel();
        Harness {
            st: State::with_shared(shared.clone()),
            model,
            shared,
            ws,
            dir,
            inbox,
            _receiver: receiver,
            front: None,
        }
    }

    /// The pane of model `id` is the one on screen, the last one's no
    /// longer is.
    fn look_at(&mut self, id: &str) {
        let before = self.front.replace(id.to_string());
        self.shared.edit(|store| {
            if let Some(before) = &before {
                store.set_shown(before, false);
            }
            store.set_shown(id, true);
        });
    }

    /// No pane of this test's models is on screen.
    fn look_away(&mut self) {
        if let Some(before) = self.front.take() {
            self.shared.edit(|store| store.set_shown(&before, false));
        }
    }

    /// What an agent's `model_show` does to the store, minus core's gate on
    /// the path.
    fn show(&mut self, name: &str, bytes: &[u8]) -> String {
        let path = self.dir.write(name, bytes);
        let format = ModelFormat::from_path(&path).expect("a model's name");
        let ws = self.ws.clone();
        let id = self.shared.edit(|store| {
            store.show_file(ShowFile {
                workspace_id: ws,
                path: path.to_string_lossy().into_owned(),
                title: None,
                opened_by: None,
                modified_ms: 1,
                what: What::Model {
                    format: format.extension().into(),
                },
            })
        });
        self.look_at(&id);
        id
    }

    /// The file was rewritten.
    fn rewrite(&mut self, id: &str) {
        self.shared
            .edit(|store| store.reload_model(id, 2, Some(10)));
    }

    fn send(&mut self, msg: Msg) {
        let _ = step(&mut self.st, &mut self.model, &self.inbox, msg);
    }

    /// The package looks again at the store.
    fn tick(&mut self) {
        self.send(Msg::Store);
    }

    fn entry(&self, id: &str) -> Entry {
        self.shared
            .read(|store| store.entry(id).cloned())
            .expect("the entry is open")
    }

    fn slot(&self, id: &str) -> &Slot {
        self.st.slots.get(id).expect("the entry has a slot")
    }

    /// The folders a read waits for, answered.
    fn roots(&mut self, id: &str) {
        let revision = self.entry(id).revision;
        let roots = vec![self.dir.path().to_path_buf()];
        self.send(Msg::Roots(id.into(), revision, roots));
    }

    /// The last event of a read: the file read for real, off the harness's
    /// thread-free path.
    fn land(&mut self, id: &str) {
        let entry = self.entry(id);
        let model = hangar_model3d::load::load(&entry.path).expect("the sample loads");
        self.send(Msg::Load(
            id.into(),
            entry.revision,
            LoadEvent::Done(Ok(Arc::new(model))),
        ));
    }

    fn fail(&mut self, id: &str, error: LoadError) {
        let revision = self.entry(id).revision;
        self.send(Msg::Load(id.into(), revision, LoadEvent::Done(Err(error))));
    }

    fn ctx<'a>(
        &'a self,
        window: window::Id,
        pkg: &'a Packages,
        locale: Locale,
        theme: &str,
    ) -> Ctx<'a> {
        Ctx {
            model: &self.model,
            pkg,
            i18n: Translator::new(locale),
            theme: hangar_theme::by_id_or_default(theme),
            metrics: Metrics::default(),
            keymap: &self.model.live.keymap.index,
            window,
            now: Instant::now(),
        }
    }
}

/* ── loading ───────────────────────────────────────────────────── */

#[test]
fn a_model_that_is_shown_is_read_and_reported_to_the_store() {
    let mut h = Harness::new("land");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let revision = h.entry(&id).revision;
    // The store says it is loading; the view has begun, and has no model yet.
    assert!(h.slot(&id).loading(revision));
    assert!(!h.slot(&id).has_content());

    // The folders are known: the read starts, and can be stopped.
    h.roots(&id);
    assert!(h.slot(&id).flight.as_ref().unwrap().cancel.is_some());

    h.send(Msg::Load(
        id.clone(),
        revision,
        LoadEvent::Progress(LoadProgress {
            phase: Phase::Read,
            loaded: 5,
            total: 10,
        }),
    ));
    assert_eq!(
        h.slot(&id)
            .flight
            .as_ref()
            .unwrap()
            .progress
            .unwrap()
            .loaded,
        5
    );

    h.land(&id);
    let slot = h.slot(&id);
    assert!(slot.flight.is_none() && slot.has_content());
    assert_eq!(slot.loaded, Some(revision));
    // The store, and so an agent waiting on `model_show`, has what was found.
    let entry = h.entry(&id);
    assert_eq!(entry.status, Status::Ready);
    let info = entry.model_info().expect("the model's numbers");
    assert_eq!(info.triangles, 12);
    assert_eq!(info.vertices, 24);
    assert!(matches!(
        h.st.registry.look(&id, revision),
        Look::Ready { .. }
    ));
}

#[test]
fn a_read_whose_stream_ends_without_an_outcome_fails_instead_of_spinning() {
    let mut h = Harness::new("ended");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let revision = h.entry(&id).revision;
    h.roots(&id);
    // The reader's thread died: the stream is over, with no `Done`.
    h.send(Msg::LoadEnded(id.clone(), revision));
    assert!(h.slot(&id).flight.is_none());
    assert_eq!(h.slot(&id).failed, Some(revision));
    assert_eq!(h.entry(&id).status, Status::Error);
    assert!(matches!(h.st.registry.look(&id, revision), Look::Failed(_)));
}

#[test]
fn a_read_that_ended_with_its_outcome_is_not_failed_by_the_end_of_its_stream() {
    let mut h = Harness::new("ended-done");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let revision = h.entry(&id).revision;
    h.roots(&id);
    h.land(&id);
    h.send(Msg::LoadEnded(id.clone(), revision));
    assert_eq!(h.entry(&id).status, Status::Ready);
    assert!(h.slot(&id).failed.is_none());
}

#[test]
fn a_model_that_cannot_be_opened_says_why_and_is_not_tried_again_until_asked() {
    let mut h = Harness::new("fail");
    let id = h.show("model.usdz", b"PK usdz");
    h.tick();
    let revision = h.entry(&id).revision;
    let said = "USDZ files are not supported by this build of the preview: export the model as \
                .glb and show that file.";
    h.fail(&id, LoadError::Unsupported(said.into()));

    // The crate's sentence, word for word, in the store and for the agent.
    let entry = h.entry(&id);
    assert_eq!(entry.status, Status::Error);
    assert_eq!(entry.error.as_deref(), Some(said));
    assert!(matches!(
        h.st.registry.look(&id, revision),
        Look::Failed(ref message) if message == said
    ));

    // The store changing again does not read the broken file again.
    h.tick();
    assert!(h.slot(&id).flight.is_none());
    assert_eq!(h.slot(&id).failed, Some(revision));

    // Retry loads it again under a new revision.
    h.send(Msg::Retry(id.clone()));
    let entry = h.entry(&id);
    assert_eq!(
        (entry.revision, entry.status),
        (revision + 1, Status::Loading)
    );
    assert!(h.slot(&id).loading(revision + 1));
    assert_eq!(h.slot(&id).failed, None);
}

#[test]
fn a_new_version_that_cannot_be_read_leaves_the_previous_one_on_screen() {
    let mut h = Harness::new("reload");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let first = h.slot(&id).loaded.expect("loaded");

    h.rewrite(&id);
    h.tick();
    let entry = h.entry(&id);
    assert_eq!((entry.revision, entry.status), (first + 1, Status::Loading));
    assert!(h.slot(&id).loading(first + 1));
    assert!(h.slot(&id).has_content(), "the old version stays");

    h.fail(&id, LoadError::Read("The file was not found.".into()));
    assert_eq!(h.entry(&id).status, Status::Error);
    assert!(h.slot(&id).has_content());
}

#[test]
fn a_newer_revision_replaces_the_load_under_way() {
    let mut h = Harness::new("newer");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let old = h.entry(&id).revision;
    h.roots(&id);
    let cancel = h.slot(&id).flight.as_ref().unwrap().cancel.clone().unwrap();

    // An agent shows the same file again: its revision moves on.
    h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let new = h.entry(&id).revision;
    assert_eq!(new, old + 1);
    assert!(cancel.is_cancelled(), "the read of the old version stops");
    assert!(h.slot(&id).loading(new) && !h.slot(&id).loading(old));

    // What the old read still reports is nobody's.
    let stale = Arc::new(hangar_model3d::load::load(h.entry(&id).path).unwrap());
    h.send(Msg::Load(id.clone(), old, LoadEvent::Done(Ok(stale))));
    assert!(!h.slot(&id).has_content());
    assert_eq!(h.entry(&id).status, Status::Loading);

    h.land(&id);
    assert_eq!(h.slot(&id).loaded, Some(new));
    assert_eq!(h.entry(&id).status, Status::Ready);
}

#[test]
fn closing_a_tab_stops_its_read_and_lets_go_of_its_model() {
    let mut h = Harness::new("close");
    let loading = h.show("a.glb", &samples::cube_glb());
    let loaded = h.show("b.glb", &samples::cube_glb());
    h.tick();
    h.roots(&loading);
    let cancel = h
        .slot(&loading)
        .flight
        .as_ref()
        .unwrap()
        .cancel
        .clone()
        .unwrap();
    h.land(&loaded);
    let revision = h.entry(&loaded).revision;
    assert!(h.st.registry.holds(&loaded));

    h.shared
        .edit(|store| store.close_models(&[loading.clone(), loaded.clone()]));
    h.tick();
    assert!(h.st.slots.is_empty());
    assert!(cancel.is_cancelled());
    assert!(matches!(
        h.st.registry.look(&loaded, revision),
        Look::Missing
    ));
}

#[test]
fn the_person_can_cancel_a_load_and_try_again() {
    let mut h = Harness::new("cancel");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.roots(&id);
    let revision = h.entry(&id).revision;
    let cancel = h.slot(&id).flight.as_ref().unwrap().cancel.clone().unwrap();

    h.send(Msg::CancelLoad(id.clone()));
    assert!(cancel.is_cancelled());
    assert!(h.slot(&id).flight.is_none());
    let entry = h.entry(&id);
    assert_eq!(entry.status, Status::Error);
    assert_eq!(entry.error, Some(LoadError::Cancelled.to_string()));
    // Not started again behind their back.
    h.tick();
    assert!(h.slot(&id).flight.is_none());

    h.send(Msg::Retry(id.clone()));
    assert!(h.slot(&id).loading(revision + 1));
}

#[test]
fn a_cancel_with_nothing_loading_does_nothing() {
    let mut h = Harness::new("cancel-idle");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    h.send(Msg::CancelLoad(id.clone()));
    h.send(Msg::CancelLoad("nothing".into()));
    // The bar's other buttons with no camera drawn yet, or no model at all.
    h.send(Msg::Zoom(id.clone(), 1));
    h.send(Msg::Zoom("nothing".into(), -1));
    h.send(Msg::Recenter("nothing".into()));
    h.send(Msg::Retry("nothing".into()));
    assert_eq!(h.entry(&id).status, Status::Ready);
}

/* ── what stays in memory ──────────────────────────────────────── */

#[test]
fn models_nobody_looks_at_are_dropped_past_the_budget_and_come_back_when_wanted() {
    let mut h = Harness::new("budget");
    let ws = h.ws.clone();
    h.shared
        .edit(|store| store.note_screens([(ws.clone(), true)]));
    let ids: Vec<String> = (0..5)
        .map(|n| h.show(&format!("m{n}.glb"), &samples::cube_glb()))
        .collect();
    for id in &ids {
        h.tick();
        h.land(id);
    }
    // The one on screen is kept, and the last three used of the others.
    let kept: Vec<bool> = ids.iter().map(|id| h.slot(id).has_content()).collect();
    assert_eq!(kept, [false, true, true, true, true], "the oldest goes");
    let revision = h.entry(&ids[0]).revision;
    assert!(matches!(
        h.st.registry.look(&ids[0], revision),
        Look::Missing
    ));
    // It stays open, and ready, for the store.
    assert_eq!(h.entry(&ids[0]).status, Status::Ready);

    // Its pane on screen again, it is read again without a word to the store.
    h.look_at(&ids[0]);
    h.tick();
    assert!(h.slot(&ids[0]).loading(revision));
    assert_eq!(h.entry(&ids[0]).status, Status::Ready);
    h.land(&ids[0]);
    assert!(h.slot(&ids[0]).has_content());
    // And the one that had been used longest ago makes room.
    assert!(!h.slot(&ids[1]).has_content());
}

#[test]
fn a_snapshots_request_loads_a_model_that_was_dropped_without_telling_the_store() {
    let mut h = Harness::new("ensure");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let revision = h.entry(&id).revision;

    // In memory, but the registry has let it go: it is handed back as it is.
    h.st.registry.remove(&id);
    h.send(Msg::Ensure(id.clone()));
    assert!(matches!(
        h.st.registry.look(&id, revision),
        Look::Ready { .. }
    ));
    assert!(h.slot(&id).flight.is_none(), "nothing is read again");

    // Dropped from memory to make room: read again, whoever is looking, and
    // the store is not told it is loading (the entry is still ready).
    h.st.slots.get_mut(&id).unwrap().drop_model();
    h.st.registry.remove(&id);
    h.send(Msg::Ensure(id.clone()));
    assert!(h.slot(&id).loading(revision));
    assert_eq!(h.entry(&id).status, Status::Ready);
    // Asked twice, it is read once.
    h.send(Msg::Ensure(id.clone()));
    assert!(h.slot(&id).loading(revision));
    h.land(&id);
    assert!(matches!(
        h.st.registry.look(&id, revision),
        Look::Ready { .. }
    ));
}

/// A message handled as `step` does, but with the workspaces given as
/// asleep (the harness's model cannot put one to sleep).
fn send_asleep(h: &mut Harness, msg: Msg, asleep: &HashSet<&str>) {
    let _ = handle(&mut h.st, &mut h.model, msg);
    let wants = loads::wants(&h.shared);
    let _ = loads::settle(&mut h.st, &h.model, &wants, asleep);
}

#[test]
fn a_snapshots_request_loads_a_model_whose_workspace_is_asleep() {
    let mut h = Harness::new("ensure-asleep");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let revision = h.entry(&id).revision;
    let ws = h.ws.clone();
    let asleep: HashSet<&str> = [ws.as_str()].into();

    // Put to sleep, the model goes.
    send_asleep(&mut h, Msg::Store, &asleep);
    assert!(h.st.slots.is_empty());
    assert!(matches!(h.st.registry.look(&id, revision), Look::Missing));

    // A snapshot asks: the load starts and is not let go of by the sleep.
    send_asleep(&mut h, Msg::Ensure(id.clone()), &asleep);
    assert!(h.slot(&id).loading(revision));
    send_asleep(&mut h, Msg::Store, &asleep);
    assert!(h.slot(&id).loading(revision), "still loading while asleep");
    let roots = vec![h.dir.path().to_path_buf()];
    send_asleep(&mut h, Msg::Roots(id.clone(), revision, roots), &asleep);
    assert!(h.slot(&id).flight.as_ref().unwrap().cancel.is_some());

    let model = hangar_model3d::load::load(&h.entry(&id).path).unwrap();
    send_asleep(
        &mut h,
        Msg::Load(id.clone(), revision, LoadEvent::Done(Ok(Arc::new(model)))),
        &asleep,
    );
    // The snapshot finds it, even after the store is looked at again.
    send_asleep(&mut h, Msg::Store, &asleep);
    assert!(matches!(
        h.st.registry.look(&id, revision),
        Look::Ready { .. }
    ));

    // Once the hold is over, the sleeping workspace lets it go again.
    h.st.ensured
        .insert(id.clone(), Instant::now() - Duration::from_millis(1));
    send_asleep(&mut h, Msg::Store, &asleep);
    assert!(h.st.slots.is_empty());
    assert!(h.st.ensured.is_empty());
}

#[test]
fn a_snapshots_request_for_what_is_closed_or_broken_does_nothing() {
    let mut h = Harness::new("ensure-none");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.fail(&id, LoadError::Read("The file was not found.".into()));
    // Broken: the registry already says why, and the read is not repeated.
    h.send(Msg::Ensure(id.clone()));
    assert!(h.slot(&id).flight.is_none());
    // Not open at all.
    h.send(Msg::Ensure("nothing".into()));
    assert!(!h.st.slots.contains_key("nothing"));
}

#[test]
fn a_workspace_put_to_sleep_lets_its_models_go_and_gets_them_back_on_waking() {
    let mut h = Harness::new("sleep");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let ws = h.ws.clone();

    let wants = loads::wants(&h.shared);
    let asleep: HashSet<&str> = [ws.as_str()].into();
    let _ = loads::settle(&mut h.st, &h.model, &wants, &asleep);
    assert!(h.st.slots.is_empty(), "the workspace sleeps");
    // Its model is still listed.
    assert_eq!(h.entry(&id).status, Status::Ready);

    // Awake again, the model on screen comes back.
    h.tick();
    assert!(h.slot(&id).loading(h.entry(&id).revision));
}

#[test]
fn the_toggles_of_the_bar_apply_to_every_model_and_to_those_opened_later() {
    let mut h = Harness::new("toggles");
    let first = h.show("a.glb", &samples::cube_glb());
    h.tick();
    assert!(h.slot(&first).viewer.display().grid);
    assert!(!h.slot(&first).viewer.display().wireframe);

    h.send(Msg::Toggle(Toggle::Wireframe));
    h.send(Msg::Toggle(Toggle::Grid));
    h.send(Msg::Toggle(Toggle::Axes));
    let display = h.slot(&first).viewer.display();
    assert!(display.wireframe && !display.grid && display.axes && !display.normals);

    let second = h.show("b.glb", &samples::cube_glb());
    h.tick();
    assert_eq!(h.slot(&second).viewer.display(), display);

    h.send(Msg::Toggle(Toggle::Normals));
    assert!(h.slot(&first).viewer.display().normals);
    assert!(h.slot(&second).viewer.display().normals);
}

#[test]
fn a_capture_waits_for_a_viewport_that_has_been_drawn() {
    // Without a window the camera is never made, and so there is no view to
    // capture: the button does nothing rather than failing.
    let mut h = Harness::new("no-camera");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    h.send(Msg::CaptureMenu(id.clone(), true));
    assert!(h.slot(&id).menu);
    h.send(Msg::Capture(id.clone(), Destination::Clipboard));
    assert!(!h.slot(&id).menu, "the menu closes");
    assert!(h.slot(&id).notice.is_none());
}

#[test]
fn a_word_after_a_capture_goes_when_its_own_timer_ends_it() {
    let mut h = Harness::new("notice");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.send(Msg::Captured(id.clone(), Ok(Done::Copied)));
    let (first, notice) = h.slot(&id).notice.clone().expect("a word");
    assert_eq!(
        notice.text,
        hangar_i18n::t(h.model.locale(), Key::ModelCopied)
    );
    assert!(!notice.error);

    h.send(Msg::Captured(id.clone(), Err("no clipboard".into())));
    let (second, notice) = h.slot(&id).notice.clone().expect("another word");
    assert!(
        notice.error && notice.text.contains("no clipboard"),
        "{notice:?}"
    );
    // The first one's timer ends and finds the second there.
    h.send(Msg::NoticeGone(id.clone(), first));
    assert!(h.slot(&id).notice.is_some());
    h.send(Msg::NoticeGone(id.clone(), second));
    assert!(h.slot(&id).notice.is_none());

    // A dialog the person closed says nothing.
    h.send(Msg::Captured(id.clone(), Ok(Done::Cancelled)));
    assert!(h.slot(&id).notice.is_none());
}

/* ── snapshots for agents ──────────────────────────────────────── */

type Asked = Arc<Mutex<Vec<String>>>;

fn recording() -> (Ask, Asked) {
    let asked: Asked = Arc::default();
    let record = asked.clone();
    (Arc::new(move |id| record.lock().unwrap().push(id)), asked)
}

/// A PNG's size from its header.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(b"\x89PNG"), "a PNG");
    let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    (word(16), word(20))
}

/// The picture, or `None` on a machine with no GPU.
fn picture(result: Result<Vec<u8>, String>) -> Option<Vec<u8>> {
    match result {
        Ok(png) => Some(png),
        Err(error) if error.contains("No graphics adapter") => {
            eprintln!("skipped, no GPU: {error}");
            None
        }
        Err(error) => panic!("{error}"),
    }
}

#[tokio::test]
async fn a_snapshot_of_a_model_that_is_not_open_says_so() {
    let h = Harness::new("snap-gone");
    let (ask, asked) = recording();
    let snapshots = Snapshots::new(h.st.registry.clone(), h.shared.clone(), ask);
    let said = snapshots.snapshot("nothing", AppAngle::Iso, (64, 64)).await;
    assert_eq!(said.unwrap_err(), GONE);
    assert!(asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_snapshot_of_a_model_that_failed_to_load_says_why() {
    let mut h = Harness::new("snap-failed");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.fail(&id, LoadError::Read("The file was not found.".into()));
    let (ask, _) = recording();
    let snapshots = Snapshots::new(h.st.registry.clone(), h.shared.clone(), ask);
    let said = snapshots.snapshot(&id, AppAngle::Iso, (64, 64)).await;
    assert_eq!(said.unwrap_err(), "The file was not found.");
}

#[tokio::test]
async fn a_snapshot_asks_for_a_model_that_is_not_in_memory_once_and_gives_up_in_time() {
    let mut h = Harness::new("snap-wait");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let (ask, asked) = recording();
    let snapshots = Snapshots::new(h.st.registry.clone(), h.shared.clone(), ask)
        .waiting(Duration::from_millis(250));
    let said = snapshots.snapshot(&id, AppAngle::Iso, (64, 64)).await;
    let said = said.unwrap_err();
    assert!(
        said.contains("cube.glb is still loading") && said.contains("model_snapshot again"),
        "{said}"
    );
    assert_eq!(*asked.lock().unwrap(), [id]);
}

#[tokio::test]
async fn a_snapshot_waits_for_the_view_to_load_the_model_and_takes_it_at_the_size_asked() {
    let mut h = Harness::new("snap-load");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let (ask, asked) = recording();
    let snapshots = Snapshots::new(h.st.registry.clone(), h.shared.clone(), ask);
    let pending = tokio::spawn(snapshots.snapshot(&id, AppAngle::Front, (96, 64)));

    // It asks the view, and the view loads the model.
    while asked.lock().unwrap().is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.land(&id);
    let result = tokio::time::timeout(Duration::from_secs(60), pending)
        .await
        .expect("it ends")
        .expect("it is not cancelled");
    if let Some(png) = picture(result) {
        assert_eq!(png_size(&png), (96, 64));
    }
}

#[tokio::test]
async fn a_snapshot_of_a_model_in_memory_is_taken_from_every_angle_off_screen() {
    let mut h = Harness::new("snap-angles");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let (ask, asked) = recording();
    let snapshots = Snapshots::new(h.st.registry.clone(), h.shared.clone(), ask);
    for angle in [
        AppAngle::Front,
        AppAngle::Back,
        AppAngle::Left,
        AppAngle::Right,
        AppAngle::Top,
        AppAngle::Bottom,
        AppAngle::Iso,
    ] {
        let result = snapshots.snapshot(&id, angle, (80, 60)).await;
        let Some(png) = picture(result) else { return };
        assert_eq!(png_size(&png), (80, 60), "{angle:?}");
    }
    assert!(asked.lock().unwrap().is_empty(), "it was in memory");
}

#[tokio::test]
async fn a_snapshot_is_painted_in_the_theme_of_the_models_workspace() {
    let mut h = Harness::new("snap-theme");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let light = hangar_theme::themes()
        .iter()
        .find(|theme| !theme.is_dark())
        .expect("a light theme");
    let ws = h.ws.clone();
    h.st.registry
        .set_themes([(ws, theme::viewer_theme(light))].into_iter().collect());
    let (ask, _) = recording();
    let snapshots = Snapshots::new(h.st.registry.clone(), h.shared.clone(), ask);
    let result = snapshots.snapshot(&id, AppAngle::Front, (64, 48)).await;
    let Some(png) = picture(result) else { return };
    let corner = pixel(&png, 1, 1);
    let background = light.colors.bg.into_rgba8();
    for channel in 0..3 {
        assert!(
            corner[channel].abs_diff(background[channel]) <= 3,
            "the corner {corner:?} is the light theme's page {background:?}"
        );
    }
}

/// One pixel of a PNG as RGBA (the pictures this package takes are 8-bit
/// RGBA).
fn pixel(png: &[u8], x: usize, y: usize) -> [u8; 4] {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .expect("a PNG");
    let (width, height) = (reader.info().width as usize, reader.info().height as usize);
    let mut pixels = vec![0u8; width * height * 4];
    reader.next_frame(&mut pixels).expect("its pixels");
    let at = (y * width + x) * 4;
    pixels[at..at + 4].try_into().unwrap()
}

/// Both angle enums name the same seven views, one for one.
#[test]
fn the_apps_angles_are_the_crates() {
    use super::snapshots::angle_of;
    let pairs = [
        (AppAngle::Front, SnapshotAngle::Front),
        (AppAngle::Back, SnapshotAngle::Back),
        (AppAngle::Left, SnapshotAngle::Left),
        (AppAngle::Right, SnapshotAngle::Right),
        (AppAngle::Top, SnapshotAngle::Top),
        (AppAngle::Bottom, SnapshotAngle::Bottom),
        (AppAngle::Iso, SnapshotAngle::Iso),
    ];
    for (app, crate_) in pairs {
        assert_eq!(angle_of(app), crate_);
    }
    assert_eq!(pairs.len(), SnapshotAngle::ALL.len());
}

#[tokio::test]
async fn a_capture_is_written_where_asked_as_a_png() {
    let dir = Scratch::new("capture-file");
    let cube = dir.write("cube.glb", samples::cube_glb());
    let model = Arc::new(hangar_model3d::load::load(&cube).expect("the cube loads"));
    let path = dir.join("shot.png");
    let job = CaptureJob::snapshot(model, SnapshotAngle::Iso, 64, 48, ViewerTheme::dark());
    match capture::deliver(job, Destination::File(path.clone()), "unused.png".into()).await {
        Ok(Done::Saved(saved)) => {
            assert_eq!(saved, path);
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(png_size(&bytes), (64, 48));
        }
        Err(error) if error.contains("No graphics adapter") => eprintln!("skipped, no GPU"),
        other => panic!("{other:?}"),
    }
}

/* ── the numbers ───────────────────────────────────────────────── */

fn sample() -> Mesh {
    hangar_model3d::load::load(sample_path()).expect("the repository's sample model loads")
}

#[test]
fn the_info_strip_lists_triangles_size_materials_and_vertices_in_that_order() {
    let h = Harness::new("facts");
    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
    let model = sample();
    let info = loads::info_of(&model);
    let facts = info_strip::facts(ctx, "glb", Some(&info));
    assert_eq!(facts.format, "glb");
    let words: Vec<String> = facts
        .items
        .iter()
        .map(|item| format!("{}{}", item.value, item.rest))
        .collect();
    assert_eq!(
        words,
        [
            "4,432 triangles",
            "13 × 5.21 × 11.5 m",
            "32 materials",
            "12,180 vertices"
        ]
    );
    // What a narrow panel drops first.
    let hide: Vec<f32> = facts.items.iter().map(|item| item.hide_below).collect();
    assert_eq!(hide, [0.0, 0.0, 360.0, 460.0]);
    // The full number is in the tip, and a glTF's size says it is in metres.
    assert_eq!(facts.items[0].tip, "4,432");
    assert_eq!(
        facts.items[1].tip,
        hangar_i18n::t(Locale::En, Key::ModelSize)
    );
}

#[test]
fn a_format_with_no_unit_says_the_size_is_in_the_files_own_units() {
    let h = Harness::new("facts-unitless");
    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::Fr, "graphite");
    let info = loads::info_of(&sample());
    let facts = info_strip::facts(ctx, "stl", Some(&info));
    assert_eq!(facts.items[1].rest, "");
    assert!(
        facts.items[1]
            .tip
            .ends_with(&hangar_i18n::t(Locale::Fr, Key::ModelUnitless).to_string()),
        "{:?}",
        facts.items[1].tip
    );
    // French groups with a narrow space and counts the singular for one.
    assert_eq!(facts.items[0].value, "4\u{202f}432");
    let mut one = info;
    one.triangles = 1;
    let facts = info_strip::facts(ctx, "stl", Some(&one));
    assert_eq!(facts.items[0].rest, " triangle");
}

#[test]
fn the_strip_says_the_format_alone_until_there_is_a_model() {
    let h = Harness::new("facts-empty");
    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
    let facts = info_strip::facts(ctx, "obj", None);
    assert_eq!(facts.format, "obj");
    assert!(facts.items.is_empty());
}

/* ── the views ─────────────────────────────────────────────────── */

fn simulate<'a>(element: iced::Element<'a, Message>, size: Size) -> Simulator<'a, Message> {
    let settings = IcedSettings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..IcedSettings::default()
    };
    Simulator::with_size(settings, size, element)
}

/// A harness over the process's store with a model shown and waiting for its
/// read: the first thing a person sees.
fn loading_scene(tag: &str) -> (Harness, String) {
    let mut h = Harness::global(tag);
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.roots(&id);
    (h, id)
}

fn read_progress(h: &mut Harness, id: &str, progress: LoadProgress) {
    let revision = h.entry(id).revision;
    h.send(Msg::Load(
        id.into(),
        revision,
        LoadEvent::Progress(progress),
    ));
}

fn has_text(ui: &mut Simulator<'_, Message>, words: &str) -> bool {
    ui.find(words).is_ok()
}

fn t(locale: Locale, key: Key) -> String {
    hangar_i18n::t(locale, key).into_owned()
}

fn scene_view<'a>(
    h: &'a mut Harness,
    pkg: &'a Packages,
    id: &'a str,
    locale: Locale,
    theme: &str,
) -> Simulator<'a, Message> {
    h.model.live.preview.sync();
    let ctx = h.ctx(window::Id::unique(), pkg, locale, theme);
    simulate(super::view(&h.st, ctx, &h.ws, id), Size::new(440.0, 600.0))
}

#[test]
fn a_model_being_read_shows_its_name_the_size_read_and_a_way_to_stop() {
    for locale in [Locale::En, Locale::Fr] {
        let (mut h, id) = loading_scene("view-read");
        read_progress(
            &mut h,
            &id,
            LoadProgress {
                phase: Phase::Read,
                loaded: 12 * MB,
                total: 40 * MB,
            },
        );
        let pkg = Packages::default();
        let mut ui = scene_view(&mut h, &pkg, &id, locale, "graphite");
        assert!(has_text(&mut ui, "cube.glb"));
        assert!(has_text(&mut ui, &t(locale, Key::ModelRead)));
        let of = hangar_i18n::t_with(
            locale,
            Key::ModelDownloadBytes,
            &hangar_i18n::args!(
                "loaded" => hangar_i18n::t_with(locale, Key::ModelMegabytes, &hangar_i18n::args!("size" => "12")),
                "total" => hangar_i18n::t_with(locale, Key::ModelMegabytes, &hangar_i18n::args!("size" => "40"))
            ),
        );
        assert!(has_text(&mut ui, &of), "{of}");
        // The tools are there and none of them can be used yet; the strip
        // has the format and nothing else.
        assert!(has_text(&mut ui, "GLB"));
        ui.click(t(locale, Key::ModelCancel).as_str())
            .expect("the cancel button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.iter().any(|message| matches!(
                message,
                Message::Model3d(Msg::CancelLoad(found)) if *found == id
            )),
            "{messages:?}"
        );
    }
}

#[test]
fn the_phase_after_the_read_is_named() {
    let (mut h, id) = loading_scene("view-phase");
    let pkg = Packages::default();
    for (phase, key) in [
        (Phase::Parse, Key::ModelParse),
        (Phase::Prepare, Key::ModelPrepare),
    ] {
        read_progress(
            &mut h,
            &id,
            LoadProgress {
                phase,
                loaded: 0,
                total: 0,
            },
        );
        let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
        assert!(has_text(&mut ui, &t(Locale::En, key)));
        drop(ui);
    }
}

#[test]
fn a_model_that_could_not_be_opened_says_why_and_offers_to_try_again() {
    for locale in [Locale::En, Locale::Fr] {
        let (mut h, id) = loading_scene("view-error");
        let said = "Draco compression is not supported: export the model without it.";
        h.fail(&id, LoadError::Unsupported(said.into()));
        let pkg = Packages::default();
        let mut ui = scene_view(&mut h, &pkg, &id, locale, "graphite");
        assert!(has_text(&mut ui, &t(locale, Key::ModelErrorTitle)));
        assert!(has_text(&mut ui, said));
        ui.click(t(locale, Key::ModelRetry).as_str())
            .expect("the retry button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.iter().any(|message| matches!(
                message,
                Message::Model3d(Msg::Retry(found)) if *found == id
            )),
            "{messages:?}"
        );
    }
}

#[test]
fn a_model_on_screen_has_its_numbers_and_a_failed_reload_says_so_over_it() {
    for locale in [Locale::En, Locale::Fr] {
        let mut h = Harness::global("view-ready");
        let id = h.show("sample.glb", &std::fs::read(sample_path()).unwrap());
        h.tick();
        h.land(&id);
        let pkg = Packages::default();
        {
            let mut ui = scene_view(&mut h, &pkg, &id, locale, "graphite");
            assert!(has_text(&mut ui, "GLB"));
            assert!(has_text(
                &mut ui,
                "4,432".replace(',', &group(locale)).as_str()
            ));
            // The animation bar lists the clip, and cannot play it yet.
            assert!(has_text(&mut ui, "Flight loop"));
            let total = hangar_i18n::t_with(
                locale,
                Key::ModelTimeOf,
                &hangar_i18n::args!("time" => zero(locale), "total" => eight(locale)),
            );
            assert!(has_text(&mut ui, &total), "{total}");
            assert!(!has_text(&mut ui, &t(locale, Key::ModelErrorTitle)));
        }

        // The file is rewritten and cannot be read: the old version stays.
        h.rewrite(&id);
        h.tick();
        h.fail(&id, LoadError::Read("The file was not found.".into()));
        let mut ui = scene_view(&mut h, &pkg, &id, locale, "graphite");
        assert!(has_text(&mut ui, &t(locale, Key::ModelReloadFailed)));
        assert!(has_text(&mut ui, "The file was not found."));
        assert!(has_text(&mut ui, "Flight loop"), "the old version stays");
        assert!(!has_text(&mut ui, &t(locale, Key::ModelErrorTitle)));
    }
}

/// The repository's sample model: a hangar and a drone flying a loop around
/// it, made from primitives with no third-party asset (by
/// `scripts/demo-model.mjs` at tag v1.12.1, for the browser demo it was first
/// drawn for).
fn sample_path() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/sample.glb")
}

fn group(locale: Locale) -> String {
    match locale {
        Locale::En => ",".into(),
        Locale::Fr => "\u{202f}".into(),
    }
}

fn zero(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "0.0",
        Locale::Fr => "0,0",
    }
}

fn eight(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "8.0",
        Locale::Fr => "8,0",
    }
}

#[test]
fn a_file_the_watch_found_gone_is_said_over_the_model() {
    let mut h = Harness::global("view-missing");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    let mut pkg = Packages::default();
    pkg.preview
        .missing
        .insert(id.clone(), "The file was not found.".into());
    let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
    assert!(has_text(&mut ui, &t(Locale::En, Key::ModelMissing)));
    assert!(has_text(&mut ui, "The file was not found."));
}

#[test]
fn what_the_loader_could_not_use_is_said_over_the_model_in_its_own_words() {
    let mut h = Harness::global("view-warnings");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    let mut cube = hangar_model3d::load::load(h.entry(&id).path).expect("the cube loads");
    let said =
        "The texture grey could not be read: KTX2 textures are not supported by the preview.";
    cube.warnings.push(said.into());
    let revision = h.entry(&id).revision;
    h.send(Msg::Load(
        id.clone(),
        revision,
        LoadEvent::Done(Ok(Arc::new(cube))),
    ));
    let pkg = Packages::default();
    let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
    assert!(has_text(&mut ui, said));
}

#[test]
fn every_state_draws_in_both_languages_and_a_dark_and_a_light_theme() {
    for theme in ["graphite", "github-light"] {
        for locale in [Locale::En, Locale::Fr] {
            let (mut h, id) = loading_scene("view-all");
            let pkg = Packages::default();
            // Loading, then loaded, then failing over what is loaded.
            for stage in 0..3 {
                match stage {
                    1 => h.land(&id),
                    2 => {
                        h.rewrite(&id);
                        h.tick();
                        h.fail(&id, LoadError::Read("gone".into()));
                    }
                    _ => {}
                }
                let mut ui = scene_view(&mut h, &pkg, &id, locale, theme);
                let colours = to_iced(hangar_theme::by_id_or_default(theme));
                let _ = ui.snapshot(&colours).expect("it draws");
            }

            // An animated model, and a viewport that cannot draw it.
            let (mut h, id) = animated_scene("view-all-anim");
            for open in [false, true] {
                h.send(Msg::ClipsMenu(id.clone(), open));
                let mut ui = scene_view(&mut h, &pkg, &id, locale, theme);
                let colours = to_iced(hangar_theme::by_id_or_default(theme));
                let _ = ui.snapshot(&colours).expect("it draws");
            }
            for rendering in trouble_states() {
                h.slot(&id).viewer.pretend_rendering(rendering);
                let mut ui = scene_view(&mut h, &pkg, &id, locale, theme);
                let colours = to_iced(hangar_theme::by_id_or_default(theme));
                let _ = ui.snapshot(&colours).expect("it draws");
            }
        }
    }
}

/// Each way the viewport can be unable to draw, with the device's words.
fn trouble_states() -> [hangar_model3d::Rendering; 4] {
    use hangar_model3d::Rendering;
    [
        Rendering::Software,
        Rendering::Unavailable("no graphics adapter was found".into()),
        Rendering::Failed("a texture of 70000 by 70000 pixels".into()),
        Rendering::Lost("Unknown: the card was reset".into()),
    ]
}

#[test]
fn an_entry_the_store_no_longer_has_draws_the_loading_stand_in() {
    let h = Harness::global("view-gone");
    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
    let mut ui = simulate(
        super::view(&h.st, ctx, &h.ws, "not-an-entry"),
        Size::new(440.0, 600.0),
    );
    assert!(has_text(&mut ui, &t(Locale::En, Key::PreviewLoading)));
}

/* ── animation ─────────────────────────────────────────────────── */

/// A model that slides for two seconds ("Slide") and spins for one ("Spin"),
/// loaded and on show.
fn animated_scene(tag: &str) -> (Harness, String) {
    let mut h = Harness::global(tag);
    let id = h.show("slide.glb", &samples::animated_cube_glb());
    h.tick();
    h.land(&id);
    (h, id)
}

fn playback_of(h: &Harness, id: &str) -> hangar_model3d::Playback {
    h.slot(id).viewer.playback().expect("the model has clips")
}

#[test]
fn a_model_with_clips_starts_playing_the_first_and_the_bar_says_where_it_is() {
    for locale in [Locale::En, Locale::Fr] {
        let (mut h, id) = animated_scene("anim-view");
        let p = playback_of(&h, &id);
        assert_eq!((p.clip, p.time, p.duration, p.playing), (0, 0.0, 2.0, true));

        h.send(Msg::Seek(id.clone(), 1.25));
        let pkg = Packages::default();
        let mut ui = scene_view(&mut h, &pkg, &id, locale, "graphite");
        // The clip that plays, and the time of it.
        assert!(has_text(&mut ui, "Slide"));
        let of = hangar_i18n::t_with(
            locale,
            Key::ModelTimeOf,
            &hangar_i18n::args!(
                "time" => format::seconds(1.25, locale),
                "total" => format::seconds(2.0, locale)
            ),
        );
        assert!(has_text(&mut ui, &of), "{of}");
    }
}

#[test]
fn a_model_that_stands_still_has_no_bar() {
    let mut h = Harness::global("anim-none");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    assert!(h.slot(&id).viewer.playback().is_none());
    let pkg = Packages::default();
    let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
    assert!(!has_text(&mut ui, "0.0 / 0.0\u{a0}s"));
    assert!(has_text(&mut ui, "GLB"), "the rest of the view is there");
}

#[test]
fn the_bar_plays_pauses_picks_a_clip_and_scrubs_through_the_viewer() {
    let (mut h, id) = animated_scene("anim-msgs");
    h.send(Msg::PlayPause(id.clone()));
    assert!(!playback_of(&h, &id).playing);
    h.send(Msg::PlayPause(id.clone()));
    assert!(playback_of(&h, &id).playing);

    h.send(Msg::Seek(id.clone(), 1.5));
    assert_eq!(playback_of(&h, &id).time, 1.5);
    h.send(Msg::Seek(id.clone(), 99.0));
    assert_eq!(playback_of(&h, &id).time, 2.0, "held to the clip");

    h.send(Msg::ClipsMenu(id.clone(), true));
    h.send(Msg::Clip(id.clone(), 1));
    let p = playback_of(&h, &id);
    assert_eq!((p.clip, p.time, p.duration, p.playing), (1, 0.0, 1.0, true));
    assert!(!h.slot(&id).clips_menu, "picking a clip closes the menu");
    h.send(Msg::Clip(id.clone(), 7));
    assert_eq!(playback_of(&h, &id).clip, 1, "there is no eighth");

    // A bar for a model that is gone does nothing.
    h.send(Msg::PlayPause("not-open".into()));
    h.send(Msg::Seek("not-open".into(), 1.0));
    h.send(Msg::Clip("not-open".into(), 0));
}

#[test]
fn the_clip_menu_opens_from_the_name_of_the_clip_that_plays() {
    let (mut h, id) = animated_scene("anim-menu");
    let pkg = Packages::default();
    // Closed: the playing clip's name opens it, and the others are not shown.
    {
        let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
        assert!(!has_text(&mut ui, "Spin"), "the other clip is in the menu");
        ui.click("Slide").expect("the clip's name");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages.iter().any(|message| matches!(
                message,
                Message::Model3d(Msg::ClipsMenu(found, true)) if *found == id
            )),
            "{messages:?}"
        );
    }
    // Open, the bar still draws (the lines are in an overlay, whose
    // mechanics are hangar-ui's menu tests') and the clip stays the same
    // until a line is picked.
    h.send(Msg::ClipsMenu(id.clone(), true));
    let pkg = Packages::default();
    let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
    let colours = to_iced(hangar_theme::by_id_or_default("graphite"));
    let _ = ui.snapshot(&colours).expect("it draws");
    drop(ui);
    assert_eq!(playback_of(&h, &id).clip, 0);
}

#[test]
fn a_reload_of_the_file_keeps_the_clip_and_the_time_the_person_was_at() {
    let (mut h, id) = animated_scene("anim-reload");
    h.send(Msg::Clip(id.clone(), 1));
    h.send(Msg::Seek(id.clone(), 0.5));
    h.send(Msg::PlayPause(id.clone()));
    h.rewrite(&id);
    h.tick();
    h.land(&id);
    let p = playback_of(&h, &id);
    assert_eq!((p.clip, p.time, p.playing), (1, 0.5, false));
}

#[test]
fn the_view_is_built_again_for_the_slider_only_while_a_clip_plays_in_front() {
    let (mut h, id) = animated_scene("anim-tick");
    // The store does not know its pane is on screen yet.
    h.look_away();
    assert!(!animating_in_front(&h.st));
    h.look_at(&id);
    assert!(animating_in_front(&h.st), "playing, its pane on screen");
    // A viewport that cannot draw has no slider to move.
    h.slot(&id)
        .viewer
        .pretend_rendering(hangar_model3d::Rendering::Software);
    assert!(!animating_in_front(&h.st));
    h.slot(&id)
        .viewer
        .pretend_rendering(hangar_model3d::Rendering::Drawing);
    assert!(animating_in_front(&h.st));

    h.look_away();
    assert!(!animating_in_front(&h.st), "its pane is hidden");
    h.look_at(&id);

    h.send(Msg::PlayPause(id.clone()));
    assert!(!animating_in_front(&h.st), "paused");
    h.send(Msg::PlayPause(id.clone()));
    assert!(animating_in_front(&h.st));

    // Another pane is zoomed over it: this one is no longer watched.
    let other = h.show("other.glb", &samples::cube_glb());
    assert_ne!(other, id);
    assert!(!animating_in_front(&h.st));
    // Leave the shared store as it was found.
    h.look_away();
}

#[test]
fn a_tick_only_makes_the_shell_build_the_view_again() {
    let mut h = Harness::new("anim-tick-msg");
    let id = h.show("slide.glb", &samples::animated_cube_glb());
    // A look at the store would start the read; a tick does not look.
    h.send(Msg::Tick);
    assert!(h.st.slots.is_empty());
    h.tick();
    assert!(h.slot(&id).loading(h.entry(&id).revision));
}

/* ── reduced motion ────────────────────────────────────────────── */

#[test]
fn the_systems_wish_for_less_motion_is_asked_once_and_reaches_every_viewer() {
    let mut h = Harness::new("motion");
    assert!(!h.st.motion_asked);
    h.tick();
    assert!(h.st.motion_asked, "asked at the first look");
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.send(Msg::ReducedMotion(true));
    assert!(h.st.reduced_motion);
    // A viewer that is made afterwards starts with it too.
    let second = h.show("second.glb", &samples::cube_glb());
    h.tick();
    assert!(h.st.slots.contains_key(&id) && h.st.slots.contains_key(&second));
    h.send(Msg::ReducedMotion(false));
    assert!(!h.st.reduced_motion);
}

/* ── a viewport that cannot draw ───────────────────────────────── */

fn cube_on_show(tag: &str) -> (Harness, String) {
    let mut h = Harness::global(tag);
    let id = h.show("cube.glb", &samples::cube_glb());
    h.tick();
    h.land(&id);
    (h, id)
}

#[test]
fn a_viewport_that_cannot_draw_says_why_over_it_in_both_languages() {
    use hangar_model3d::Rendering;
    // Rendering, what is said, and whether there is a button to try again.
    let cases: [(Rendering, Key, &str, bool); 4] = [
        (Rendering::Software, Key::ModelUnavailable, "", false),
        (
            Rendering::Unavailable("no adapter was found".into()),
            Key::ModelUnavailable,
            "no adapter was found",
            true,
        ),
        (
            Rendering::Failed("a texture of 70000 pixels".into()),
            Key::ModelDrawFailed,
            "a texture of 70000 pixels",
            true,
        ),
        (
            Rendering::Lost("Unknown: the card was reset".into()),
            Key::ModelLost,
            "Unknown: the card was reset",
            true,
        ),
    ];
    for locale in [Locale::En, Locale::Fr] {
        for (rendering, title, detail, restart) in &cases {
            let (mut h, id) = cube_on_show("trouble");
            h.slot(&id).viewer.pretend_rendering(rendering.clone());
            let pkg = Packages::default();
            let mut ui = scene_view(&mut h, &pkg, &id, locale, "graphite");
            assert!(has_text(&mut ui, &t(locale, *title)), "{rendering:?}");
            let detail = if *rendering == Rendering::Software {
                t(locale, Key::ModelSoftware)
            } else {
                detail.to_string()
            };
            assert!(has_text(&mut ui, &detail), "{detail}");
            // The model's own bar is still there: a capture still works.
            assert!(has_text(&mut ui, "GLB"));
            let button = t(locale, Key::ModelRestart);
            assert_eq!(has_text(&mut ui, &button), *restart, "{rendering:?}");
            if *restart {
                ui.click(button.as_str()).expect("the restart button");
                let messages: Vec<Message> = ui.into_messages().collect();
                assert!(
                    messages.iter().any(|message| matches!(
                        message,
                        Message::Model3d(Msg::Restart(found)) if *found == id
                    )),
                    "{messages:?}"
                );
            }
        }
    }
}

#[test]
fn a_viewport_that_draws_says_nothing_and_a_restart_takes_the_message_away() {
    use hangar_model3d::Rendering;
    let (mut h, id) = cube_on_show("trouble-gone");
    let pkg = Packages::default();
    for quiet in [Rendering::Starting, Rendering::Drawing] {
        h.slot(&id).viewer.pretend_rendering(quiet);
        let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
        assert!(!has_text(&mut ui, &t(Locale::En, Key::ModelUnavailable)));
        assert!(!has_text(&mut ui, &t(Locale::En, Key::ModelLost)));
    }
    h.slot(&id)
        .viewer
        .pretend_rendering(Rendering::Lost("reset".into()));
    h.send(Msg::Restart(id.clone()));
    assert_eq!(h.slot(&id).viewer.rendering(), Rendering::Starting);
    let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
    assert!(!has_text(&mut ui, &t(Locale::En, Key::ModelLost)));
    // And a restart for a model that is gone does nothing.
    drop(ui);
    h.send(Msg::Restart("not-open".into()));
}

#[test]
fn nothing_is_said_over_a_model_that_has_not_loaded_even_if_the_renderer_is_known_bad() {
    // The loading card is the message while there is nothing to draw.
    let (mut h, id) = loading_scene("trouble-loading");
    h.slot(&id)
        .viewer
        .pretend_rendering(hangar_model3d::Rendering::Software);
    let pkg = Packages::default();
    let mut ui = scene_view(&mut h, &pkg, &id, Locale::En, "graphite");
    assert!(!has_text(&mut ui, &t(Locale::En, Key::ModelUnavailable)));
    assert!(has_text(&mut ui, "cube.glb"), "the loading card is shown");
}

/// The scenes, as pictures, when `HANGAR_MODEL3D_SNAPSHOTS` names a folder.
#[test]
fn snapshots() {
    let Some(out) = std::env::var_os("HANGAR_MODEL3D_SNAPSHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    for theme_id in ["graphite", "github-light"] {
        for locale in [Locale::En, Locale::Fr] {
            let lang = if locale == Locale::En { "en" } else { "fr" };
            let pkg = Packages::default();
            let colours = to_iced(hangar_theme::by_id_or_default(theme_id));
            let save = |scene: &str, mut ui: Simulator<'_, Message>| {
                let stem = format!("{scene}-{theme_id}-{lang}");
                for old in std::fs::read_dir(&out).unwrap().flatten() {
                    let name = old.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&format!("{stem}-")) && name.ends_with(".png") {
                        let _ = std::fs::remove_file(old.path());
                    }
                }
                ui.snapshot(&colours)
                    .expect("it draws")
                    .matches_image(out.join(&stem))
                    .expect("written");
            };

            let (mut h, id) = loading_scene("snap-read");
            read_progress(
                &mut h,
                &id,
                LoadProgress {
                    phase: Phase::Read,
                    loaded: 12 * MB,
                    total: 40 * MB,
                },
            );
            save("reading", scene_view(&mut h, &pkg, &id, locale, theme_id));
            read_progress(
                &mut h,
                &id,
                LoadProgress {
                    phase: Phase::Parse,
                    loaded: 0,
                    total: 0,
                },
            );
            save("decoding", scene_view(&mut h, &pkg, &id, locale, theme_id));
            h.fail(
                &id,
                LoadError::Unsupported(
                    "This glTF needs KHR_draco_mesh_compression, which the preview does not \
                     decode: export it without Draco and show that file."
                        .into(),
                ),
            );
            save("error", scene_view(&mut h, &pkg, &id, locale, theme_id));

            let mut h = Harness::global("snap-ready");
            let id = h.show("sample.glb", &std::fs::read(sample_path()).unwrap());
            h.tick();
            h.land(&id);
            save("ready", scene_view(&mut h, &pkg, &id, locale, theme_id));
            h.send(Msg::CaptureMenu(id.clone(), true));
            save("menu", scene_view(&mut h, &pkg, &id, locale, theme_id));
            h.send(Msg::CaptureMenu(id.clone(), false));
            h.send(Msg::Captured(
                id.clone(),
                Ok(Done::Saved("/tmp/a.png".into())),
            ));
            save("saved", scene_view(&mut h, &pkg, &id, locale, theme_id));
            h.rewrite(&id);
            h.tick();
            save("reloading", scene_view(&mut h, &pkg, &id, locale, theme_id));
            h.fail(&id, LoadError::Read("The file was not found.".into()));
            save(
                "reload-failed",
                scene_view(&mut h, &pkg, &id, locale, theme_id),
            );

            // A viewport that cannot draw: one picture for each way.
            let (mut h, id) = cube_on_show("snap-trouble");
            let names = ["software", "unavailable", "refused", "lost"];
            for (name, rendering) in names.into_iter().zip(trouble_states()) {
                h.slot(&id).viewer.pretend_rendering(rendering);
                save(name, scene_view(&mut h, &pkg, &id, locale, theme_id));
            }
            // And the bar of an animated model with its clips menu open.
            let (mut h, id) = animated_scene("snap-anim");
            h.send(Msg::Seek(id.clone(), 1.25));
            save("playing", scene_view(&mut h, &pkg, &id, locale, theme_id));
            h.send(Msg::PlayPause(id.clone()));
            h.send(Msg::ClipsMenu(id.clone(), true));
            save(
                "paused-menu",
                scene_view(&mut h, &pkg, &id, locale, theme_id),
            );
        }
    }
}
