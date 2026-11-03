//! Pictures for agents: `model_snapshot` of the control bus lands here through
//! the preview store's `Snapshotter` (`registerSnapshotter` / `useSnapshotter`).
//!
//! A snapshot is taken of any model that is open, from a canonical angle, at
//! the size the agent asked, whatever the person is looking at: another tab,
//! another workspace, the panel hidden. The person's camera and toggles are
//! never touched, because hangar-model3d draws it with a device of its own
//! from the model in memory (`CaptureJob::snapshot`).
//!
//! The view's models live on the window's thread, and a snapshot runs on
//! tokio, so what it needs crosses in a [`Registry`]: the models that are in
//! memory (a shared `Arc`, not a copy) and the theme of each workspace. When
//! the model asked for is not in memory (dropped to make room for others, or
//! never loaded because its workspace is asleep) the snapshot asks the view
//! to load it with `Msg::Ensure` and waits for the answer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::watch;

use hangar_model3d::{CaptureJob, Model, ViewerTheme};

use super::Msg;
use crate::message::Message;
use crate::preview::access::{Shared, Snapshotter};
use crate::services::Inbox;
use crate::services::preview::{BoxFuture, SnapshotAngle};
use crate::state::WsId;

/// How long a snapshot waits for a model to be loaded for it. The control
/// handler has already waited for the entry to settle (`SNAPSHOT_SETTLE_MS`);
/// this covers a model that was dropped from memory since, which takes the
/// same time to read again as it did the first time.
const LOAD_WAIT: Duration = Duration::from_secs(40);

/// Said when the model was closed while the agent waited
/// (`That model is no longer open in Hangar.`).
pub const GONE: &str = "That model is no longer open in Hangar.";

struct Held {
    revision: u64,
    ws: WsId,
    model: Arc<Model>,
}

#[derive(Default)]
struct Inner {
    held: HashMap<String, Held>,
    failed: HashMap<String, (u64, String)>,
    themes: HashMap<WsId, ViewerTheme>,
}

/// The models the view has in memory, for the tasks that picture them.
pub struct Registry {
    inner: Mutex<Inner>,
    /// Counts the changes, for a snapshot waiting on a load.
    changes: watch::Sender<u64>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Registry(..)")
    }
}

/// What the registry holds for an entry at a revision.
#[derive(Debug, Clone)]
pub enum Look {
    /// The model of that revision is in memory.
    Ready {
        model: Arc<Model>,
        theme: ViewerTheme,
    },
    /// That revision could not be loaded, and why.
    Failed(String),
    /// Neither: it is not loaded, or an older version is.
    Missing,
}

impl Default for Registry {
    fn default() -> Self {
        Registry {
            inner: Mutex::new(Inner::default()),
            changes: watch::channel(0).0,
        }
    }
}

impl Registry {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn changed(&self) {
        self.changes.send_modify(|count| *count += 1);
    }

    /// Records a model that has been loaded.
    pub fn put(&self, id: &str, revision: u64, ws: &str, model: Arc<Model>) {
        {
            let mut inner = self.lock();
            inner.failed.remove(id);
            inner.held.insert(
                id.to_string(),
                Held {
                    revision,
                    ws: ws.to_string(),
                    model,
                },
            );
        }
        self.changed();
    }

    /// Records that a revision could not be loaded.
    pub fn fail(&self, id: &str, revision: u64, message: String) {
        self.lock()
            .failed
            .insert(id.to_string(), (revision, message));
        self.changed();
    }

    /// Forgets a model that was closed or dropped from memory.
    pub fn remove(&self, id: &str) {
        let mut inner = self.lock();
        let had = inner.held.remove(id).is_some() | inner.failed.remove(id).is_some();
        drop(inner);
        if had {
            self.changed();
        }
    }

    /// The theme each workspace is drawn in, as of now.
    pub fn set_themes(&self, themes: HashMap<WsId, ViewerTheme>) {
        self.lock().themes = themes;
    }

    /// Whether anything is held for the entry.
    pub fn holds(&self, id: &str) -> bool {
        self.lock().held.contains_key(id)
    }

    pub fn look(&self, id: &str, revision: u64) -> Look {
        let inner = self.lock();
        if let Some(held) = inner.held.get(id).filter(|held| held.revision == revision) {
            let theme = inner.themes.get(&held.ws).copied().unwrap_or_default();
            return Look::Ready {
                model: held.model.clone(),
                theme,
            };
        }
        match inner.failed.get(id) {
            Some((failed, message)) if *failed == revision => Look::Failed(message.clone()),
            _ => Look::Missing,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}

/// What `model_snapshot` renders with, registered with the preview store.
pub struct Snapshots {
    registry: Arc<Registry>,
    shared: Arc<Shared>,
    inbox: Inbox,
    wait: Duration,
}

impl Snapshots {
    pub fn new(registry: Arc<Registry>, shared: Arc<Shared>, inbox: Inbox) -> Snapshots {
        Snapshots {
            registry,
            shared,
            inbox,
            wait: LOAD_WAIT,
        }
    }

    #[cfg(test)]
    pub fn waiting(mut self, wait: Duration) -> Snapshots {
        self.wait = wait;
        self
    }

    /// The model of the entry's latest revision, loaded for it if it has to
    /// be. The file may be rewritten while it loads: whatever revision the
    /// store holds once a load lands is the one pictured.
    async fn model_of(&self, id: &str) -> Result<(Arc<Model>, ViewerTheme), String> {
        let mut changes = self.registry.subscribe();
        let deadline = tokio::time::Instant::now() + self.wait;
        let mut asked: Option<u64> = None;
        loop {
            let entry = self.shared.read(|store| {
                store
                    .entry(id)
                    .filter(|entry| entry.is_model())
                    .map(|entry| (entry.revision, entry.title.clone()))
            });
            let Some((revision, title)) = entry else {
                return Err(GONE.into());
            };
            match self.registry.look(id, revision) {
                Look::Ready { model, theme } => return Ok((model, theme)),
                Look::Failed(message) => return Err(message),
                Look::Missing => {
                    // Asked once for each revision: the view loads it, and
                    // tells the registry when it has.
                    if asked != Some(revision) {
                        asked = Some(revision);
                        self.inbox
                            .send(Message::Model3d(Msg::Ensure(id.to_string())));
                    }
                }
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => return Err("the 3D preview is closing".into()),
                Err(_) => {
                    return Err(format!(
                        "{title} is still loading after {} s. Try model_snapshot again in a moment.",
                        self.wait.as_secs()
                    ));
                }
            }
        }
    }
}

/// The crate's angle for the app's.
fn angle_of(angle: SnapshotAngle) -> hangar_model3d::SnapshotAngle {
    match angle {
        SnapshotAngle::Front => hangar_model3d::SnapshotAngle::Front,
        SnapshotAngle::Back => hangar_model3d::SnapshotAngle::Back,
        SnapshotAngle::Left => hangar_model3d::SnapshotAngle::Left,
        SnapshotAngle::Right => hangar_model3d::SnapshotAngle::Right,
        SnapshotAngle::Top => hangar_model3d::SnapshotAngle::Top,
        SnapshotAngle::Bottom => hangar_model3d::SnapshotAngle::Bottom,
        SnapshotAngle::Iso => hangar_model3d::SnapshotAngle::Iso,
    }
}

impl Snapshotter for Snapshots {
    fn snapshot(
        &self,
        id: &str,
        angle: SnapshotAngle,
        size: (u32, u32),
    ) -> BoxFuture<Result<Vec<u8>, String>> {
        let me = Snapshots {
            registry: self.registry.clone(),
            shared: self.shared.clone(),
            inbox: self.inbox.clone(),
            wait: self.wait,
        };
        let id = id.to_string();
        Box::pin(async move {
            let (model, theme) = me.model_of(&id).await?;
            CaptureJob::snapshot(model, angle_of(angle), size.0, size.1, theme)
                .run_async()
                .await
                .map_err(|error| error.to_string())
        })
    }
}
