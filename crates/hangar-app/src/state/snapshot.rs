//! What the engines read: the model as it stood after the last update.
//!
//! The TS called `snapshot()` from ~30 modules, off any render (inventory
//! §1.3). Here the control, orchestration and mobile engines run on tokio,
//! away from `Model`, and read an `Arc<Snapshot>` the shell republishes after
//! every update that changed something (`Services::snapshot`, rule 6 of
//! PACKAGES.md). Cheap to take: the persisted state is itself an `Arc`
//! shared with the model until the model next writes it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::Model;
use super::live::{Delivering, PaneActivity};
use super::types::{AppState, PaneId, WsId};

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub state: Arc<AppState>,
    /// Workspaces opened in this run (their panes are alive).
    pub opened: HashSet<WsId>,
    /// Workspaces put to sleep (P4's hibernation).
    pub asleep: HashSet<WsId>,
    pub activity: HashMap<PaneId, PaneActivity>,
    pub delivering: Delivering,
    /// `Model.live.hydrated`: engines answer nothing before.
    pub hydrated: bool,
}

impl Snapshot {
    /// The snapshot of `model` now. `asleep` and `activity` are what P4
    /// writes in `live.signals`, `delivering` what P18 writes there.
    pub fn of(model: &Model) -> Snapshot {
        let signals = &model.live.signals;
        Snapshot {
            state: model.state_arc(),
            opened: model.ui.opened_ids.iter().cloned().collect(),
            asleep: signals.asleep.clone(),
            activity: signals.activity.clone(),
            delivering: signals.delivering.clone(),
            hydrated: model.live.hydrated,
        }
    }
}
