//! The delivery_* control operations (deliveryControl.ts) and the
//! deliveries running (deliveryRuns.ts).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::state::WsId;

use super::types::{DeliveryOutcomeKind, DeliveryState};

/// A delivery under way (`InFlight`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    pub workspace_id: WsId,
    pub worker: String,
    pub since: u64,
    /// The person landing a delivery that waited, rather than a worker
    /// submitting.
    pub landing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Settled {
    outcome: DeliveryOutcomeKind,
    detail: String,
    at: u64,
}

/// deliveryControl's module state, held by the port: one delivery at a time
/// per task, and the last outcome of each while the app runs.
#[derive(Debug, Default)]
pub struct Deliveries {
    book: Mutex<Book>,
}

#[derive(Debug, Default)]
struct Book {
    in_flight: HashMap<String, InFlight>,
    settled: HashMap<String, Settled>,
}

impl Deliveries {
    fn book(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Takes the task for one delivery, unless one is under way: then
    /// answers that one.
    pub fn take(&self, task: &str, entry: InFlight) -> Result<(), InFlight> {
        let mut book = self.book();
        if let Some(busy) = book.in_flight.get(task) {
            return Err(busy.clone());
        }
        book.in_flight.insert(task.to_string(), entry);
        Ok(())
    }

    pub fn release(&self, task: &str) {
        self.book().in_flight.remove(task);
    }

    pub fn in_flight(&self, task: &str) -> Option<InFlight> {
        self.book().in_flight.get(task).cloned()
    }

    pub fn settle(&self, task: &str, outcome: DeliveryOutcomeKind, detail: String, at: u64) {
        self.book().settled.insert(
            task.to_string(),
            Settled {
                outcome,
                detail,
                at,
            },
        );
    }

    /// Where a task's delivery stands (`deliveryState`).
    pub fn state(&self, task: &str) -> Option<DeliveryState> {
        let book = self.book();
        if let Some(running) = book.in_flight.get(task) {
            return Some(DeliveryState {
                outcome: None,
                since: running.since,
                detail: None,
            });
        }
        book.settled.get(task).map(|last| DeliveryState {
            outcome: Some(last.outcome),
            since: last.at,
            detail: Some(last.detail.clone()),
        })
    }

    /// Whether a delivery of the workspace is being tested or landed
    /// (`deliveringIn`): minutes during which it must not sleep.
    pub fn delivering_in(&self, ws: &str) -> bool {
        self.book()
            .in_flight
            .values()
            .any(|entry| entry.workspace_id == ws)
    }
}
