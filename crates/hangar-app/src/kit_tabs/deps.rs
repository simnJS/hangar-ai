//! The kit API over the app itself (`liveDeps` of KitTabView.tsx): what a
//! tab is, read from the model's snapshot, the installed kits and the preview
//! store at each call; and what it asks, done with hangar-core's kit
//! functions, the preview, the bridge's queue, the notifier, and the window
//! for a native tab's `ui.*`.
//!
//! Everything here runs on Tokio's threads as well as the window's: it reads
//! the model only through the snapshot the shell republishes after every
//! change, and writes the preview only through its lock.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde_json::Value;
use tokio::sync::watch;

use hangar_core::Host;
use hangar_core::kits::KitInfo;
use hangar_kit_host::LoopbackCall;
use hangar_kit_ui::ImageSource;
use hangar_kit_ui::api::{HttpMethod, KitError, limits};

use super::api::{BoxFuture, CommandCall, Deps, TabContext, UiCall, tab_error_of};
use super::tabs::{KitTabState, tab_state_on};
use crate::message::Message;
use crate::preview::access::Shared;
use crate::preview::store::{KitBadge, KitTabCaller, KitTabParams};
use crate::services::PreviewRequest;
use crate::services::inbox::{Answer, Inbox};
use crate::state::live::PaneActivity;
use crate::state::types::Workspace;
use crate::state::{Snapshot, WsId};

/// The installed kits as the kit tabs last saw them in `kits::Live`, shared
/// with the tabs' APIs, which read it off the window's thread. `None` until
/// the first read.
#[derive(Default)]
pub struct Installed(RwLock<Held>);

/// The kits, and what tells a list from another: `KitInfo` has no equality,
/// and its JSON is what changes when anything of a kit does.
#[derive(Default)]
struct Held {
    kits: Option<Arc<Vec<KitInfo>>>,
    print: Option<String>,
}

fn print(kits: Option<&[KitInfo]>) -> Option<String> {
    kits.map(|kits| serde_json::to_string(kits).unwrap_or_default())
}

impl std::fmt::Debug for Installed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Installed(..)")
    }
}

impl Installed {
    pub fn get(&self) -> Option<Arc<Vec<KitInfo>>> {
        self.0
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .kits
            .clone()
    }

    /// Takes `kits` when they are not what is held: a kit added, updated,
    /// removed, or its manifest edited.
    pub fn sync(&self, kits: Option<&[KitInfo]>) -> bool {
        let print = print(kits);
        let mut held = self.0.write().unwrap_or_else(|poison| poison.into_inner());
        if held.print == print {
            return false;
        }
        held.kits = kits.map(|kits| Arc::new(kits.to_vec()));
        held.print = print;
        true
    }

    fn kit(&self, id: &str) -> Option<KitInfo> {
        self.get()?.iter().find(|kit| kit.id == id).cloned()
    }
}

/// Who a tab is, from the app's own record of it.
#[derive(Debug, Clone)]
pub struct TabRecord {
    /// Its entry in the preview store.
    pub entry: String,
    pub kit_id: String,
    pub tab_id: String,
    pub workspace_id: WsId,
}

/// `TabContext` over the model's snapshot, the installed kits and the
/// preview store.
pub struct LiveContext {
    record: TabRecord,
    shared: Arc<Shared>,
    snapshots: watch::Receiver<Arc<Snapshot>>,
    installed: Arc<Installed>,
}

impl LiveContext {
    pub fn new(
        record: TabRecord,
        shared: Arc<Shared>,
        snapshots: watch::Receiver<Arc<Snapshot>>,
        installed: Arc<Installed>,
    ) -> LiveContext {
        LiveContext {
            record,
            shared,
            snapshots,
            installed,
        }
    }

    /// The tab as the workspace offers it now, if it does.
    fn state(&self) -> KitTabState {
        let snapshot = self.snapshots.borrow().clone();
        let Some(ws) = snapshot
            .state
            .workspaces
            .iter()
            .find(|ws| ws.id == self.record.workspace_id)
        else {
            return KitTabState::Disabled;
        };
        let installed = self.installed.get();
        tab_state_on(
            ws,
            &snapshot.state.settings,
            installed.as_deref().map(Vec::as_slice),
            &self.record.kit_id,
            &self.record.tab_id,
        )
    }
}

impl TabContext for LiveContext {
    fn kit_id(&self) -> &str {
        &self.record.kit_id
    }

    fn kit_name(&self) -> String {
        self.installed
            .kit(&self.record.kit_id)
            .and_then(|kit| kit.manifest.map(|manifest| manifest.name))
            .unwrap_or_else(|| self.record.kit_id.clone())
    }

    fn workspace(&self) -> Option<Workspace> {
        self.snapshots
            .borrow()
            .state
            .workspaces
            .iter()
            .find(|ws| ws.id == self.record.workspace_id)
            .cloned()
    }

    fn permissions(&self) -> Vec<String> {
        match self.state() {
            KitTabState::Ready { choice, .. } => choice.permissions,
            _ => Vec::new(),
        }
    }

    fn commands_accepted(&self) -> Option<String> {
        match self.state() {
            KitTabState::Ready { choice, .. } => choice.commands_accepted,
            _ => None,
        }
    }

    fn params(&self) -> KitTabParams {
        self.shared
            .read(|store| store.kit_tab_params(&self.record.entry))
    }

    fn caller(&self) -> Option<KitTabCaller> {
        self.shared
            .read(|store| store.kit_tab_caller(&self.record.entry))
    }

    fn set_title(&self, title: &str) {
        self.shared
            .edit(|store| store.set_kit_tab_title(&self.record.entry, title));
    }

    fn set_badge(&self, badge: Option<KitBadge>) {
        self.shared
            .edit(|store| store.set_kit_tab_badge(&self.record.entry, badge));
    }
}

/// What a native tab's `ui.*` asks of the window, answered once it is done
/// (`Msg::Ui`).
#[derive(Debug, Clone)]
pub struct UiAsk {
    pub call: UiCall,
    pub answer: Answer<Result<Value, KitError>>,
}

/// `Deps` over hangar-core and the app.
pub struct LiveDeps {
    host: Host,
    shared: Arc<Shared>,
    snapshots: watch::Receiver<Arc<Snapshot>>,
    inbox: Inbox,
    installed: Arc<Installed>,
    /// The tab's entry, which a `ui.*` names to the window.
    entry: String,
}

impl LiveDeps {
    pub fn new(
        host: Host,
        shared: Arc<Shared>,
        snapshots: watch::Receiver<Arc<Snapshot>>,
        inbox: Inbox,
        installed: Arc<Installed>,
        entry: String,
    ) -> LiveDeps {
        LiveDeps {
            host,
            shared,
            snapshots,
            inbox,
            installed,
            entry,
        }
    }
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, KitError> {
    serde_json::to_value(value).map_err(|why| KitError::new(KitError::FAILED, why.to_string()))
}

impl Deps for LiveDeps {
    fn read_file(&self, root: String, path: String) -> BoxFuture<Result<String, String>> {
        Box::pin(hangar_core::kit_scheme::files::kit_file_read(root, path))
    }

    fn resolve_file(&self, root: String, path: String) -> BoxFuture<Result<String, String>> {
        Box::pin(hangar_core::kit_scheme::files::kit_workspace_path(
            root, path,
        ))
    }

    fn read_hidden_file(&self, root: String, path: String) -> BoxFuture<Result<String, KitError>> {
        Box::pin(async move {
            hangar_core::kits::files::kit_files_read(root, path)
                .await
                .map_err(tab_error_of)
        })
    }

    fn read_binary_file(
        &self,
        root: String,
        path: String,
        hidden: bool,
    ) -> BoxFuture<Result<Value, KitError>> {
        Box::pin(async move {
            to_value(
                hangar_core::kits::files::kit_files_read_binary(root, path, hidden)
                    .await
                    .map_err(tab_error_of)?,
            )
        })
    }

    fn list_files(
        &self,
        root: String,
        path: Option<String>,
        pattern: Option<String>,
    ) -> BoxFuture<Result<Value, KitError>> {
        Box::pin(async move {
            to_value(
                hangar_core::kits::files::kit_files_list(root, path, pattern)
                    .await
                    .map_err(tab_error_of)?,
            )
        })
    }

    fn write_file(
        &self,
        root: String,
        path: String,
        text: String,
        expected: Option<String>,
    ) -> BoxFuture<Result<(), KitError>> {
        Box::pin(async move {
            hangar_core::kits::files::kit_files_write(root, path, text, expected)
                .await
                .map_err(tab_error_of)
        })
    }

    fn write_binary_file(
        &self,
        root: String,
        path: String,
        base64: String,
    ) -> BoxFuture<Result<(), KitError>> {
        Box::pin(async move {
            hangar_core::kits::files::kit_files_write_binary(root, path, base64)
                .await
                .map_err(tab_error_of)
        })
    }

    /// The file in the workspace's preview, as an agent's `preview_show`
    /// opens one: its folders granted, then what core makes of it.
    fn show_file(
        &self,
        ws: Workspace,
        path: String,
        opened_by: String,
    ) -> BoxFuture<Result<(), String>> {
        let shared = self.shared.clone();
        Box::pin(async move { show_in_preview(&shared, &ws, &path, opened_by).await })
    }

    fn enqueue(
        &self,
        pane_id: String,
        text: String,
        from: String,
    ) -> BoxFuture<Result<(), String>> {
        let snapshot = self.snapshots.borrow().clone();
        let queued = queue_for_pane(&self.host, &snapshot, pane_id, text, from).map(drop);
        Box::pin(std::future::ready(queued))
    }

    fn activity(&self, pane_id: &str) -> Option<PaneActivity> {
        self.snapshots.borrow().activity.get(pane_id).cloned()
    }

    fn notify(&self, title: String, body: String) -> BoxFuture<()> {
        hangar_core::notify::show(title, body);
        Box::pin(std::future::ready(()))
    }

    fn read_data(&self, kit: String, path: String) -> BoxFuture<Result<String, KitError>> {
        Box::pin(async move {
            hangar_core::kits::data::kit_data_read(kit, path)
                .await
                .map_err(tab_error_of)
        })
    }

    fn write_data(
        &self,
        kit: String,
        path: String,
        text: String,
    ) -> BoxFuture<Result<(), KitError>> {
        Box::pin(async move {
            hangar_core::kits::data::kit_data_write(kit, path, text)
                .await
                .map_err(tab_error_of)
        })
    }

    fn loopback_fetch(&self, call: LoopbackCall) -> BoxFuture<Result<Value, KitError>> {
        Box::pin(async move {
            let method = match call.method {
                HttpMethod::Get => "GET",
                HttpMethod::Post => "POST",
            };
            to_value(
                hangar_core::kits::loopback::kit_loopback_fetch(
                    call.port,
                    call.path,
                    Some(method.to_owned()),
                    call.body,
                    Some(u64::from(call.timeout_ms)),
                )
                .await
                .map_err(tab_error_of)?,
            )
        })
    }

    fn run_command(&self, call: CommandCall) -> BoxFuture<Result<Value, KitError>> {
        Box::pin(async move {
            to_value(
                hangar_core::kits::commands::kit_command_run(
                    call.kit,
                    call.name,
                    call.args.into_iter().collect(),
                    call.cwd,
                    call.accepted,
                )
                .await
                .map_err(tab_error_of)?,
            )
        })
    }

    fn ui(&self, call: UiCall) -> BoxFuture<Result<Value, KitError>> {
        let (answer, waiting) = Answer::channel();
        let asked = self.inbox.send(Message::KitTabs(super::Msg::Ui(
            self.entry.clone(),
            UiAsk { call, answer },
        )));
        Box::pin(async move {
            let gone = || KitError::new(KitError::UNAVAILABLE, "the tab is no longer open");
            if !asked {
                return Err(gone());
            }
            waiting.await.unwrap_or_else(|_| Err(gone()))
        })
    }

    fn image(&self, kit: &str, source: &ImageSource) -> Result<Vec<u8>, KitError> {
        let (root, raw, hidden) = match source {
            ImageSource::Kit(path) => {
                let dir = self
                    .installed
                    .kit(kit)
                    .map(|kit| PathBuf::from(kit.dir))
                    .ok_or_else(|| {
                        KitError::new(KitError::NOT_FOUND, "the kit is not installed")
                    })?;
                (dir, path, false)
            }
            ImageSource::Data(path) => {
                let root = hangar_core::kits::data::root()
                    .ok_or_else(|| KitError::new(KitError::FAILED, "no home directory"))?;
                (root.join(kit), path, true)
            }
        };
        read_image(&root, raw, hidden)
    }
}

/// The file in the workspace's preview, as an agent's `preview_show` opens
/// one: its folders granted, then what core makes of it.
///
/// The size is core's to judge, in `admit`: a model or a PDF above 256 MB, an
/// image or a page above 64 MB, a text above 10 MB are refused there with the
/// size and the limit in the message, before anything opens. The TypeScript
/// checked the 256 MB of models here by hand, because its inspection did not.
async fn show_in_preview(
    shared: &Shared,
    ws: &Workspace,
    path: &str,
    opened_by: String,
) -> Result<(), String> {
    let file = shared.admit(ws, path).await?;
    let request = PreviewRequest {
        ws: ws.id.clone(),
        path: file.path.clone(),
        title: None,
        opened_by: Some(opened_by),
        reveal: true,
    };
    shared.open_file(&request, &file).map(drop)
}

/// Queues `text` for the pane's hangar-bridge mod, unless the pane has gone
/// since the call was checked: a prompt queued for a pane that is no longer
/// there would wait for a mod that never comes, and the tab would be told it
/// was sent. `bridge_enqueue` itself cannot fail; this is the one way a send
/// does (the TypeScript awaited the command, which could throw).
fn queue_for_pane(
    host: &Host,
    snapshot: &Snapshot,
    pane_id: String,
    text: String,
    from: String,
) -> Result<hangar_core::bridge::Queued, String> {
    let there = snapshot
        .state
        .workspaces
        .iter()
        .any(|ws| ws.panes.iter().any(|pane| pane.id == pane_id));
    if !there {
        return Err("that pane was closed before the text could be queued for it".into());
    }
    Ok(hangar_core::bridge::bridge_enqueue(
        host,
        pane_id,
        text,
        Some(from),
    ))
}

/// An image of a kit's folder or of its data folder: a relative path inside,
/// links followed and held inside, at most 10 MB. The kit scheme's rules for a
/// kit's own files: nothing hidden there; in its data, its own names.
fn read_image(root: &Path, raw: &str, hidden: bool) -> Result<Vec<u8>, KitError> {
    let invalid = |why: &str| KitError::new(KitError::INVALID, format!("{raw}: {why}"));
    let mut relative = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.contains('\0') {
            return Err(invalid("must be a path inside its folder"));
        }
        if !hidden && part.starts_with('.') {
            return Err(invalid("must not name a hidden file"));
        }
        relative.push(part);
    }
    if relative.as_os_str().is_empty() || raw.starts_with(['/', '\\']) {
        return Err(invalid("must name a file inside its folder"));
    }
    let not_found = || KitError::new(KitError::NOT_FOUND, format!("{raw} was not found"));
    let root = std::fs::canonicalize(root).map_err(|_| not_found())?;
    let real = std::fs::canonicalize(root.join(&relative)).map_err(|_| not_found())?;
    if !real.starts_with(&root) {
        return Err(invalid("leads outside its folder"));
    }
    let size = std::fs::metadata(&real).map_err(|_| not_found())?.len();
    if size > limits::BINARY_MAX_BYTES as u64 {
        return Err(KitError::new(
            KitError::INVALID,
            format!("{raw} is larger than 10 MB"),
        ));
    }
    std::fs::read(&real).map_err(|why| KitError::new(KitError::FAILED, why.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::PreviewAccess;
    use crate::state::types::Pane;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hangar-kit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn workspace_in(dir: &Path) -> Workspace {
        Workspace {
            id: "w-kit-show".into(),
            name: "demo".into(),
            cwd: dir.to_string_lossy().into_owned(),
            ..Workspace::default()
        }
    }

    /// `preview.show` of `name`, as the kit API calls it; hangar-core keeps one
    /// list of the folders it serves for the whole process, and a test of the
    /// preview may replace it between the grant and the inspection, so a
    /// refusal for being outside is tried again a few times.
    async fn shown(shared: &Shared, ws: &Workspace, name: &str) -> Result<(), String> {
        let path = Path::new(&ws.cwd).join(name).to_string_lossy().into_owned();
        let mut answer = show_in_preview(shared, ws, &path, "Hello kit".into()).await;
        for _ in 0..20 {
            match &answer {
                Err(why) if why.contains("outside") => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    answer = show_in_preview(shared, ws, &path, "Hello kit".into()).await;
                }
                _ => break,
            }
        }
        answer
    }

    #[tokio::test]
    async fn a_model_above_256_megabytes_is_not_shown_and_the_message_says_how_big() {
        let dir = scratch("show-big");
        let ws = workspace_in(&dir);
        let shared = Shared::new(None);
        // glTF binary header, then nothing: sparse, never read.
        let big = std::fs::File::create(dir.join("big.glb")).unwrap();
        big.set_len(256 * 1024 * 1024 + 1).unwrap();
        let refused = shown(&shared, &ws, "big.glb").await.unwrap_err();
        assert!(refused.contains("big.glb"), "{refused}");
        assert!(refused.contains("257 MB"), "{refused}");
        assert!(refused.contains("256 MB"), "{refused}");
        assert!(shared.list(&ws.id).is_empty(), "nothing was opened");

        // At the cap itself, a model opens.
        let small = std::fs::File::create(dir.join("small.glb")).unwrap();
        small.set_len(1024).unwrap();
        shown(&shared, &ws, "small.glb").await.expect("it opens");
        let listed = shared.list(&ws.id);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].opened_by.as_deref(), Some("Hello kit"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_file_that_is_not_there_is_refused_and_opens_nothing() {
        let dir = scratch("show-none");
        let ws = workspace_in(&dir);
        let shared = Shared::new(None);
        let refused = shown(&shared, &ws, "gone.glb").await.unwrap_err();
        assert!(refused.contains("gone.glb"), "{refused}");
        assert!(shared.list(&ws.id).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn snapshot_with_pane(pane: &str) -> Snapshot {
        Snapshot {
            state: Arc::new(crate::state::AppState {
                workspaces: vec![Workspace {
                    id: "w1".into(),
                    panes: vec![Pane {
                        id: pane.into(),
                        name: "Ava".into(),
                        ..Pane::default()
                    }],
                    ..Workspace::default()
                }],
                ..crate::state::AppState::default()
            }),
            ..Snapshot::default()
        }
    }

    fn host() -> Host {
        Host::with_paths(hangar_core::Paths::rooted(&std::env::temp_dir()))
    }

    #[test]
    fn a_send_is_queued_for_its_pane_under_the_kits_name() {
        let pane = "p-kit-queue-live";
        let queued = queue_for_pane(
            &host(),
            &snapshot_with_pane(pane),
            pane.into(),
            "Build it".into(),
            "kit:hello-kit".into(),
        )
        .expect("queued");
        let waiting = hangar_core::bridge::bridge_queue(pane.into());
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].id, queued.id);
        assert_eq!(waiting[0].text, "Build it");
        assert_eq!(waiting[0].from.as_deref(), Some("kit:hello-kit"));
        hangar_core::bridge::bridge_cancel(&host(), pane.into(), None);
    }

    #[test]
    fn a_send_to_a_pane_that_has_gone_is_reported_and_queues_nothing() {
        let pane = "p-kit-queue-gone";
        let refused = queue_for_pane(
            &host(),
            &snapshot_with_pane("another-pane"),
            pane.into(),
            "Build it".into(),
            "kit:hello-kit".into(),
        )
        .unwrap_err();
        assert!(refused.contains("closed"), "{refused}");
        assert!(hangar_core::bridge::bridge_queue(pane.into()).is_empty());
        // No workspace at all, as after the last one closed.
        assert!(
            queue_for_pane(
                &host(),
                &Snapshot::default(),
                pane.into(),
                "x".into(),
                "kit:hello-kit".into()
            )
            .is_err()
        );
        assert!(hangar_core::bridge::bridge_queue(pane.into()).is_empty());
    }

    #[test]
    fn an_image_is_read_from_inside_its_folder_only() {
        let dir = std::env::temp_dir().join(format!("hangar-kit-image-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/logo.png"), b"png").unwrap();
        std::fs::write(dir.join(".secret.png"), b"no").unwrap();
        assert_eq!(read_image(&dir, "assets\\logo.png", false).unwrap(), b"png");
        for raw in ["../x.png", "/etc/x.png", "C:x.png", "", "."] {
            assert_eq!(
                read_image(&dir, raw, false).unwrap_err().code,
                "invalid",
                "{raw}"
            );
        }
        assert_eq!(
            read_image(&dir, ".secret.png", false).unwrap_err().code,
            "invalid"
        );
        assert_eq!(read_image(&dir, ".secret.png", true).unwrap(), b"no");
        assert_eq!(
            read_image(&dir, "assets/gone.png", false).unwrap_err().code,
            "not_found"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_installed_kits_are_taken_only_when_they_changed() {
        let installed = Installed::default();
        assert!(installed.get().is_none());
        let kits = vec![crate::kit_tabs::testing::kit("hello-kit", Vec::new())];
        assert!(installed.sync(Some(&kits)));
        assert!(!installed.sync(Some(&kits)));
        assert_eq!(
            installed.kit("hello-kit").map(|kit| kit.id),
            Some("hello-kit".into())
        );
        assert!(installed.sync(None));
    }
}
