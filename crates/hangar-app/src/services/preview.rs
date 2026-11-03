//! What the engines may do with a workspace's preview panel (the
//! `PreviewHost` of control.ts, inventory A6): list, show, reveal, reload,
//! close, wait for an entry to settle, render a model off screen, list kit
//! tabs. P15 implements it over its preview store (with P16's kit tabs and
//! P17's snapshots); until it does, every workspace's preview is empty and
//! showing anything fails.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::state::{PaneId, WsId};

/// A future an engine can await from any tokio task.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Opening a file in a workspace's preview (`PreviewToShow` of modelStore.ts;
/// kit tabs go through `Intent::OpenKitTab`). The kind is the preview's to
/// work out (`preview_inspect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    pub ws: WsId,
    /// Absolute, or relative to the workspace's folder.
    pub path: String,
    pub title: Option<String>,
    /// The pane of the agent that asks; `None` when the person does.
    pub opened_by: Option<PaneId>,
    /// Opens the panel with the entry in front; false adds it quietly.
    pub reveal: bool,
}

/// One open entry, as an engine sees it.
/// TODO(P15): the fields `model_list`/`preview_list` answer with.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewEntry {
    pub id: String,
    /// `model`, `kit`, or a file kind (`image`, `text`, `pdf`…).
    pub kind: String,
    pub path: String,
    pub title: String,
    pub opened_by: Option<PaneId>,
    /// Still loading.
    pub loading: bool,
    pub error: Option<String>,
}

/// From where a model snapshot looks (`SnapshotAngle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotAngle {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Iso,
}

const UNAVAILABLE: &str = "the preview is not available yet";

pub trait PreviewAccess: Send + Sync {
    fn list(&self, ws: &WsId) -> Vec<PreviewEntry> {
        let _ = ws;
        Vec::new()
    }

    /// Opens a file; answers the entry's id.
    fn show(&self, request: PreviewRequest) -> Result<String, String> {
        let _ = request;
        Err(UNAVAILABLE.into())
    }

    /// Brings an entry to the front, opening the panel. Whether it exists.
    fn reveal(&self, ws: &WsId, id: &str) -> bool {
        let _ = (ws, id);
        false
    }

    fn reload(&self, ws: &WsId, id: &str) -> bool {
        let _ = (ws, id);
        false
    }

    /// Closes one entry, or every entry of the workspace; answers how many.
    fn close(&self, ws: &WsId, id: Option<&str>) -> usize {
        let _ = (ws, id);
        0
    }

    /// The entry once it has loaded or failed, or `None` past `timeout`
    /// (`whenSettled`).
    fn when_settled(
        &self,
        ws: &WsId,
        id: &str,
        timeout: Duration,
    ) -> BoxFuture<Option<PreviewEntry>> {
        let _ = (ws, id, timeout);
        Box::pin(std::future::ready(None))
    }

    /// A model rendered off screen, as PNG bytes.
    fn snapshot(
        &self,
        ws: &WsId,
        id: &str,
        angle: SnapshotAngle,
        size: (u32, u32),
    ) -> BoxFuture<Result<Vec<u8>, String>> {
        let _ = (ws, id, angle, size);
        Box::pin(std::future::ready(Err(UNAVAILABLE.to_string())))
    }

    /// The kit tabs open in the workspace (P16).
    fn tabs(&self, ws: &WsId) -> Vec<PreviewEntry> {
        let _ = ws;
        Vec::new()
    }
}

/// No preview: the default until P15 installs its store.
#[derive(Debug, Default)]
pub struct NoPreview;

impl PreviewAccess for NoPreview {}
