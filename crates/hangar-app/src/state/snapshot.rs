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
    /// The snapshot of `model` now: `asleep` and `activity` from P4's
    /// read-model, `delivering` from P18's.
    pub fn of(model: &Model) -> Snapshot {
        Snapshot {
            state: model.state_arc(),
            opened: model.ui.opened_ids.iter().cloned().collect(),
            asleep: model.live.panes_logic.asleep_ids(),
            activity: model.live.panes_logic.activity_map().clone(),
            delivering: model.live.control.delivering(),
            hydrated: model.live.hydrated,
        }
    }

    /// Whether the model's live parts moved on since this snapshot was taken,
    /// though nothing to save did: a delivery began or ended, an agent
    /// started waiting, a workspace went to sleep or was opened. The engines
    /// and hibernation read those here, so a stale one is published again.
    /// Run after every message: compares in place, cheapest first, and
    /// allocates nothing while no delivery runs.
    pub fn is_stale(&self, model: &Model) -> bool {
        let opened = &model.ui.opened_ids;
        let asleep = model.live.panes_logic.asleep();
        self.opened.len() != opened.len()
            || opened.iter().any(|id| !self.opened.contains(id))
            || self.asleep.len() != asleep.len()
            || asleep.keys().any(|id| !self.asleep.contains(id))
            || self.activity != *model.live.panes_logic.activity_map()
            || self.delivering != model.live.control.delivering()
    }
}
