//! The agents' view of the previews: P18's `PreviewHosts`, installed on the
//! control bus (`Hook::Preview`) once the state is read.
//!
//! The TS asked a `ModelHost` that was the window's own, or forwarded the call
//! to the window that drew the workspace; in one process every workspace's
//! host is a handle on the same store (`Shared`). What a host lists is richer
//! than `PreviewAccess` (the shell's fallback adapter): the entry in front,
//! whether the panel is drawn, a model's format and findings, a file's type
//! and size, a kit tab's params.
//!
//! The control handlers have admitted the path (the folders granted, the file
//! inspected) before they show it, so `show` only opens it.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::access::{NOT_HERE, Shared};
use super::model::{FileKind, Kind};
use super::store::{Body, Entry, KitTabToOpen, ModelInfo, ShowFile, Status, What};
use crate::control::port::{
    BoxFuture, EntryKind, EntryStatus, OpenEntry, PreviewHost, PreviewHosts, PreviewListing,
    Showing,
};
use crate::services::PreviewAccess;
use crate::services::preview::SnapshotAngle;
use crate::state::WsId;

/// Every workspace's preview, for the control bus.
pub struct Hosts {
    shared: Arc<Shared>,
}

impl Hosts {
    pub fn new(shared: Arc<Shared>) -> Hosts {
        Hosts { shared }
    }
}

impl PreviewHosts for Hosts {
    /// A workspace in a window of its own is the same store as any other:
    /// there is always a host to ask.
    fn host(&self, ws: &WsId, _detached: bool) -> Option<Arc<dyn PreviewHost>> {
        Some(Arc::new(Host {
            shared: self.shared.clone(),
            ws: ws.clone(),
        }))
    }
}

/// One workspace's preview. It never touches another's: an id it is given
/// that is not open here is not found (`NOT_HERE`).
struct Host {
    shared: Arc<Shared>,
    ws: WsId,
}

impl PreviewHost for Host {
    fn list(&self) -> BoxFuture<Result<PreviewListing, String>> {
        let listing = self.shared.read(|store| {
            let scope = store.scope(&self.ws);
            let view = store.view();
            PreviewListing {
                entries: scope.entries.iter().map(open_entry).collect(),
                active_id: scope.active_id.clone(),
                on_screen: view.on_screen(&self.ws),
                shown: view.panel_shown(&self.ws),
            }
        });
        Box::pin(std::future::ready(Ok(listing)))
    }

    fn show(&self, entry: Showing) -> BoxFuture<Result<String, String>> {
        let shown = self.open(entry);
        Box::pin(std::future::ready(shown))
    }

    fn reveal(&self, id: &str) -> BoxFuture<()> {
        self.shared.edit(|store| {
            if store.scope(&self.ws).entry(id).is_some() {
                store.activate_model(id);
                store.set_model_panel(&self.ws, Some(true), None);
            }
        });
        Box::pin(std::future::ready(()))
    }

    fn reload(&self, id: &str, modified_ms: u64) -> BoxFuture<()> {
        self.shared.edit(|store| {
            // A kit's tab has no file to read again.
            if store
                .scope(&self.ws)
                .entry(id)
                .is_some_and(|entry| !entry.is_kit_tab())
            {
                store.reload_model(id, modified_ms, None);
            }
        });
        Box::pin(std::future::ready(()))
    }

    fn when_settled(&self, id: &str, ms: u64) -> BoxFuture<Option<OpenEntry>> {
        let settled = self.shared.settle(&self.ws, id, Duration::from_millis(ms));
        Box::pin(async move { settled.await.map(|entry| open_entry(&entry)) })
    }

    fn snapshot(
        &self,
        id: &str,
        angle: SnapshotAngle,
        width: u32,
        height: u32,
    ) -> BoxFuture<Result<Vec<u8>, String>> {
        if !self.shared.owns(&self.ws, id) {
            return Box::pin(std::future::ready(Err(NOT_HERE.to_string())));
        }
        PreviewAccess::snapshot(&*self.shared, &self.ws, id, angle, (width, height))
    }

    fn close(&self, ids: Vec<String>) -> BoxFuture<()> {
        self.shared.edit(|store| {
            let mine: Vec<String> = ids
                .into_iter()
                .filter(|id| store.scope(&self.ws).entry(id).is_some())
                .collect();
            store.close_models(&mine);
        });
        Box::pin(std::future::ready(()))
    }
}

impl Host {
    fn open(&self, entry: Showing) -> Result<String, String> {
        let workspace_id = self.ws.clone();
        match entry {
            Showing::Model {
                path,
                title,
                format,
                opened_by,
                modified_ms,
            } => Ok(self.shared.edit(|store| {
                store.show_file(ShowFile {
                    workspace_id,
                    path,
                    title: Some(title),
                    opened_by,
                    modified_ms,
                    what: What::Model { format },
                })
            })),
            Showing::File {
                kind,
                path,
                title,
                mime,
                size,
                opened_by,
                modified_ms,
            } => {
                let kind = Kind::parse(&kind)
                    .and_then(FileKind::of)
                    .ok_or_else(|| "The preview has no viewer for that file.".to_string())?;
                Ok(self.shared.edit(|store| {
                    store.show_file(ShowFile {
                        workspace_id,
                        path,
                        title: Some(title),
                        opened_by,
                        modified_ms,
                        what: What::File { kind, mime, size },
                    })
                }))
            }
            Showing::Kit {
                kit_id,
                tab_id,
                title,
                params,
                singleton,
                opened_by,
                reveal,
                ..
            } => Ok(self.shared.edit(|store| {
                store.open_kit_tab(KitTabToOpen {
                    workspace_id,
                    kit_id,
                    tab_id,
                    title,
                    params,
                    singleton,
                    opened_by,
                    reveal,
                })
            })),
        }
    }
}

/// An entry as the control handlers list it (`OpenPreview`).
pub fn open_entry(entry: &Entry) -> OpenEntry {
    let what = match &entry.body {
        Body::Model { format, info } => EntryKind::Model {
            format: format.clone(),
            info: info.as_ref().map(findings),
        },
        Body::File { kind, mime, size } => EntryKind::File {
            kind: kind.as_str().to_string(),
            mime: mime.clone(),
            size: *size,
        },
        Body::Kit(tab) => EntryKind::Kit {
            kit_id: tab.kit_id.clone(),
            tab_id: tab.tab_id.clone(),
            params: tab.params.clone(),
        },
    };
    OpenEntry {
        id: entry.id.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
        opened_by: entry.opened_by.clone(),
        modified_ms: entry.modified_ms,
        status: match entry.status {
            Status::Loading => EntryStatus::Loading,
            Status::Ready => EntryStatus::Ready,
            Status::Error => EntryStatus::Error,
        },
        error: entry.error.clone(),
        what,
    }
}

/// What the viewer found in a model, as the agents read it (`ModelInfo`).
fn findings(info: &ModelInfo) -> Value {
    json!({
        "triangles": info.triangles,
        "vertices": info.vertices,
        "meshes": info.meshes,
        "materials": info.materials,
        "animations": info.animations,
        "size": info.size,
        "center": info.center,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::store::KitTabParams;

    fn host(shared: &Arc<Shared>, ws: &str) -> Arc<dyn PreviewHost> {
        Hosts::new(shared.clone())
            .host(&ws.to_string(), false)
            .expect("always a host")
    }

    fn model(path: &str) -> Showing {
        Showing::Model {
            path: path.into(),
            title: "Chair".into(),
            format: "glb".into(),
            opened_by: Some("Ava".into()),
            modified_ms: 7,
        }
    }

    fn page(path: &str) -> Showing {
        Showing::File {
            kind: "html".into(),
            path: path.into(),
            title: "Report".into(),
            mime: "text/html".into(),
            size: 12,
            opened_by: None,
            modified_ms: 3,
        }
    }

    fn kit(reveal: bool) -> Showing {
        Showing::Kit {
            path: "kit:ci/runs".into(),
            kit_id: "ci".into(),
            tab_id: "runs".into(),
            title: "Runs".into(),
            params: KitTabParams::from([("branch".to_string(), "main".to_string())]),
            singleton: false,
            opened_by: Some("Ava".into()),
            reveal,
        }
    }

    #[tokio::test]
    async fn lists_every_tab_with_what_is_in_front_and_what_each_is() {
        let shared = Shared::new(None);
        let me = host(&shared, "w1");
        let chair = me.show(model("/dev/chair.glb")).await.unwrap();
        let report = me.show(page("/dev/report.html")).await.unwrap();
        let runs = me.show(kit(false)).await.unwrap();
        shared.edit(|store| store.settle_model(&chair, info()));

        let listing = me.list().await.unwrap();
        assert_eq!(
            listing
                .entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            [chair.as_str(), report.as_str(), runs.as_str()]
        );
        // The file shown last is in front; the kit tab was added quietly.
        assert_eq!(listing.active_id.as_deref(), Some(report.as_str()));
        assert!(!listing.on_screen);
        assert!(!listing.shown);

        let first = &listing.entries[0];
        assert_eq!(first.path, "/dev/chair.glb");
        assert_eq!(first.title, "Chair");
        assert_eq!(first.opened_by.as_deref(), Some("Ava"));
        assert_eq!(first.modified_ms, 7);
        assert_eq!(first.status, EntryStatus::Ready);
        assert!(first.is_model());
        let found = first.info().expect("the findings");
        assert_eq!(found["triangles"], 1284);
        assert_eq!(found["animations"], json!(["Idle"]));
        assert_eq!(found["size"], json!([2.0, 1.0, 3.0]));

        assert_eq!(
            listing.entries[1].what,
            EntryKind::File {
                kind: "html".into(),
                mime: "text/html".into(),
                size: 12
            }
        );
        let EntryKind::Kit {
            kit_id,
            tab_id,
            params,
        } = &listing.entries[2].what
        else {
            panic!("a kit's tab");
        };
        assert_eq!((kit_id.as_str(), tab_id.as_str()), ("ci", "runs"));
        assert_eq!(params.get("branch").map(String::as_str), Some("main"));
    }

    fn info() -> Result<ModelInfo, String> {
        Ok(ModelInfo {
            triangles: 1284,
            vertices: 702,
            meshes: 3,
            materials: 2,
            animations: vec!["Idle".into()],
            size: [2.0, 1.0, 3.0],
            center: [0.0, 0.5, 0.0],
        })
    }

    #[tokio::test]
    async fn says_where_the_workspace_stands_on_screen() {
        let shared = Shared::new(None);
        let me = host(&shared, "w1");
        me.show(page("/dev/report.html")).await.unwrap();
        shared.edit(|store| store.note_screens([("w1".into(), true)]));
        let listing = me.list().await.unwrap();
        assert!(listing.on_screen && listing.shown);
        shared.edit(|store| store.note_screens([("w1".into(), false)]));
        let listing = me.list().await.unwrap();
        assert!(listing.on_screen && !listing.shown);
    }

    #[tokio::test]
    async fn keeps_to_its_own_workspace() {
        let shared = Shared::new(None);
        let mine = host(&shared, "w1");
        let theirs = host(&shared, "w2");
        let a = mine.show(page("/dev/a.html")).await.unwrap();
        let b = theirs.show(page("/dev/b.html")).await.unwrap();
        assert_eq!(mine.list().await.unwrap().entries.len(), 1);

        // Another workspace's id is not found: not closed, not reloaded, not
        // waited for, not photographed.
        mine.close(vec![b.clone()]).await;
        mine.reload(&b, 99).await;
        assert!(mine.when_settled(&b, 5).await.is_none());
        assert_eq!(
            mine.snapshot(&b, SnapshotAngle::Iso, 64, 64)
                .await
                .unwrap_err(),
            NOT_HERE
        );
        assert_eq!(theirs.list().await.unwrap().entries.len(), 1);
        assert_eq!(
            theirs.list().await.unwrap().entries[0].modified_ms,
            3,
            "not reloaded"
        );

        mine.close(vec![a, b]).await;
        assert!(mine.list().await.unwrap().entries.is_empty());
        assert_eq!(theirs.list().await.unwrap().entries.len(), 1);
    }

    #[tokio::test]
    async fn opens_what_the_preview_has_a_viewer_for_and_refuses_the_rest() {
        let shared = Shared::new(None);
        let me = host(&shared, "w1");
        let refused = me
            .show(Showing::File {
                kind: "spreadsheet".into(),
                path: "/dev/a.xlsx".into(),
                title: "a".into(),
                mime: String::new(),
                size: 0,
                opened_by: None,
                modified_ms: 0,
            })
            .await;
        assert_eq!(
            refused.unwrap_err(),
            "The preview has no viewer for that file."
        );
        // A model is not a file kind: it is shown as a model.
        let refused = me
            .show(Showing::File {
                kind: "model".into(),
                path: "/dev/a.glb".into(),
                title: "a".into(),
                mime: String::new(),
                size: 0,
                opened_by: None,
                modified_ms: 0,
            })
            .await;
        assert!(refused.is_err());
        assert!(me.list().await.unwrap().entries.is_empty());
    }

    #[tokio::test]
    async fn showing_a_file_again_brings_it_forward_and_reveal_opens_the_panel() {
        let shared = Shared::new(None);
        let me = host(&shared, "w1");
        let a = me.show(page("/dev/a.html")).await.unwrap();
        let b = me.show(page("/dev/b.html")).await.unwrap();
        assert_eq!(me.show(page("/dev/a.html")).await.unwrap(), a);
        assert_eq!(
            me.list().await.unwrap().active_id.as_deref(),
            Some(a.as_str())
        );
        shared.edit(|store| store.set_model_panel("w1", Some(false), None));
        me.reveal(&b).await;
        assert_eq!(
            me.list().await.unwrap().active_id.as_deref(),
            Some(b.as_str())
        );
        assert!(shared.read(|store| store.scope("w1").panel_open));
    }

    #[tokio::test]
    async fn waits_for_a_model_to_load_or_fail_and_gives_up() {
        let shared = Shared::new(None);
        let me = host(&shared, "w1");
        let id = me.show(model("/dev/chair.glb")).await.unwrap();
        assert!(me.when_settled(&id, 20).await.is_none(), "still loading");
        let waiting = {
            let me = me.clone();
            let id = id.clone();
            tokio::spawn(async move { me.when_settled(&id, 5_000).await })
        };
        tokio::task::yield_now().await;
        shared.edit(|store| store.settle_model(&id, Err("not a glTF".into())));
        let settled = waiting.await.unwrap().expect("settled");
        assert_eq!(settled.status, EntryStatus::Error);
        assert_eq!(settled.error.as_deref(), Some("not a glTF"));
    }

    #[tokio::test]
    async fn reloads_a_rewritten_file_with_the_modification_time_it_is_given() {
        let shared = Shared::new(None);
        let me = host(&shared, "w1");
        let id = me.show(model("/dev/chair.glb")).await.unwrap();
        shared.edit(|store| store.settle_model(&id, info()));
        me.reload(&id, 4242).await;
        let entry = me.when_settled(&id, 5).await;
        assert!(entry.is_none(), "loading again");
        let listing = me.list().await.unwrap();
        assert_eq!(listing.entries[0].modified_ms, 4242);
        assert_eq!(listing.entries[0].status, EntryStatus::Loading);
    }
}
