//! The app's side of a kit tab's message protocol (docs/kits.md,
//! `src/lib/kitApi.ts`): what a kit's tab, native or a page, may ask Hangar
//! for, and only that.
//!
//! One implementation for both kinds of tab. Every call goes through
//! hangar-kit-host's gate first (no more than 16 waiting, a method there is,
//! the permission held, the params of the right shape and size, the app's own
//! check, the rate), which a native tab's `KitTab` holds itself and a page's
//! [`PageHost`] holds here; then [`TabApi`] does it with what the app has,
//! reached through [`Deps`], which a test hands fakes. Which tab it speaks
//! for, and what it may call, is read afresh at each call from the app's own
//! record of the tab ([`TabContext`]), never from what the kit says.
//!
//! A page's messages that are not requests of its own page are dropped
//! without a word: answering them would tell a stranger something.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};

use hangar_kit_host::{
    CallFuture, ClipboardFuture, ClipboardImage, Gate, HostCall, KitApi, LoopbackCall, PickFuture,
    PickedFile,
};
use hangar_kit_ui::ImageSource;
use hangar_kit_ui::abi::{Call, CallId};
use hangar_kit_ui::api::{self, BadgeState, KitError, limits};
use hangar_webview::kit_relay::{self, Incoming};

use crate::preview::store::{KitBadge, KitBadgeState, KitTabCaller, KitTabParams};
use crate::state::live::PaneActivity;
use crate::state::types::{AgentId, Pane, Workspace};

/// A future the app's dependencies answer with.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// The event a page hears when a singleton gets new params, or an agent or a
/// hook asks for the tab again. No permission: it is about itself.
pub const KIT_TAB_PARAMS_EVENT: &str = api::event::TAB_PARAMS;

/// How a tab names who sent a message to an agent: the bridge labels it, never
/// as the person.
pub fn kit_sender(kit_id: &str) -> String {
    format!("kit:{kit_id}")
}

fn error(code: &str, message: impl Into<String>) -> KitError {
    KitError::new(code, message)
}

fn failed(message: impl Into<String>) -> KitError {
    error(KitError::FAILED, message)
}

/// A refusal from Rust keeps its code, which is what the page tells apart;
/// anything else is `failed` (`tabErrorOf`).
pub fn tab_error_of(refusal: hangar_core::kits::TabError) -> KitError {
    error(refusal.code, refusal.message)
}

/// Which tab the API speaks for, read afresh at each call
/// (`KitHostContext`).
pub trait TabContext: Send + Sync + 'static {
    /// The kit, from the app's record of the tab.
    fn kit_id(&self) -> &str;
    fn kit_name(&self) -> String;
    /// The workspace the tab is shown in, as it is now; `None` once it is
    /// gone.
    fn workspace(&self) -> Option<Workspace>;
    /// What the tab may call now (`workspace_kit_tabs`).
    fn permissions(&self) -> Vec<String>;
    /// The `commands:<hash>` the tab's commands run under now; `None` when
    /// not asked.
    fn commands_accepted(&self) -> Option<String>;
    /// What this very tab was opened with, as it is now.
    fn params(&self) -> KitTabParams;
    /// Who last asked for this very tab, from Hangar's record; `None` when
    /// only the person did.
    fn caller(&self) -> Option<KitTabCaller>;
    /// Names this very tab in the tab strip.
    fn set_title(&self, title: &str);
    /// Sets this very tab's state in the tab strip; `None` takes it away.
    fn set_badge(&self, badge: Option<KitBadge>);
}

/// A `command.run` once checked: the kit, the folder and the grant are the
/// app's own record of the tab, never the message's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCall {
    pub kit: String,
    pub name: String,
    pub args: BTreeMap<String, String>,
    /// The workspace folder of the tab, where the command starts.
    pub cwd: String,
    pub accepted: Option<String>,
}

/// What a native tab asks of the window: a toast, a question, the clipboard.
#[derive(Debug, Clone, PartialEq)]
pub enum UiCall {
    Toast(api::Toast),
    Confirm(api::Confirm),
    Copy(String),
}

/// What the API reaches of the app (`KitHostDeps`). Paths are relative to
/// their folder, with `/`, already held to the rules of the gate; each of
/// hangar-core's functions holds them again on disk, where links can lead
/// out.
pub trait Deps: Send + Sync + 'static {
    /// `kit_file_read`: a text file inside `root`. Refused with a sentence.
    fn read_file(&self, root: String, path: String) -> BoxFuture<Result<String, String>>;
    /// `kit_workspace_path`: the canonical path of a file inside `root`.
    fn resolve_file(&self, root: String, path: String) -> BoxFuture<Result<String, String>>;
    /// `kit_files_read`: a text file inside `root`, hidden or not.
    fn read_hidden_file(&self, root: String, path: String) -> BoxFuture<Result<String, KitError>>;
    /// `kit_files_read_binary`: an image or a PDF, `{ base64, mime }`.
    fn read_binary_file(
        &self,
        root: String,
        path: String,
        hidden: bool,
    ) -> BoxFuture<Result<Value, KitError>>;
    /// `kit_files_list`: the files under a folder of `root`.
    fn list_files(
        &self,
        root: String,
        path: Option<String>,
        pattern: Option<String>,
    ) -> BoxFuture<Result<Value, KitError>>;
    /// `kit_files_write`: atomic.
    fn write_file(
        &self,
        root: String,
        path: String,
        text: String,
        expected: Option<String>,
    ) -> BoxFuture<Result<(), KitError>>;
    /// `kit_files_write_binary`: a new image, never over a file.
    fn write_binary_file(
        &self,
        root: String,
        path: String,
        base64: String,
    ) -> BoxFuture<Result<(), KitError>>;
    /// Opens the file in the workspace's preview, as opened by `opened_by`.
    fn show_file(
        &self,
        ws: Workspace,
        path: String,
        opened_by: String,
    ) -> BoxFuture<Result<(), String>>;
    /// The hangar-bridge queue: submitted once the agent is free.
    fn enqueue(&self, pane_id: String, text: String, from: String)
    -> BoxFuture<Result<(), String>>;
    fn activity(&self, pane_id: &str) -> Option<PaneActivity>;
    fn notify(&self, title: String, body: String) -> BoxFuture<()>;
    /// `kit_data_read`: a text of the kit's own data.
    fn read_data(&self, kit: String, path: String) -> BoxFuture<Result<String, KitError>>;
    /// `kit_data_write`: atomic.
    fn write_data(
        &self,
        kit: String,
        path: String,
        text: String,
    ) -> BoxFuture<Result<(), KitError>>;
    /// `kit_loopback_fetch`, made by Rust: `{ status, text }`.
    fn loopback_fetch(&self, call: LoopbackCall) -> BoxFuture<Result<Value, KitError>>;
    /// `kit_command_run`, made by Rust.
    fn run_command(&self, call: CommandCall) -> BoxFuture<Result<Value, KitError>>;
    /// A native tab's `ui.*`: what the window answers once it is done.
    fn ui(&self, call: UiCall) -> BoxFuture<Result<Value, KitError>>;
    /// The bytes of an image a native tab's view shows.
    fn image(&self, kit: &str, source: &ImageSource) -> Result<Vec<u8>, KitError>;
    /// The bytes of an image of the workspace folder `root`, held to the
    /// rules of `files.read` (hidden ones when `hidden`). Refused by default.
    fn workspace_image(
        &self,
        root: String,
        path: String,
        hidden: bool,
    ) -> Result<Vec<u8>, KitError> {
        let _ = (root, path, hidden);
        Err(KitError::new(
            KitError::UNSUPPORTED,
            "no image of the workspace",
        ))
    }
    /// `files.pick`: the system's chooser, opened in `root` (media.rs).
    /// Refused by default.
    fn pick_file(
        &self,
        root: String,
        request: api::FilesPick,
    ) -> BoxFuture<Result<Option<PickedFile>, KitError>> {
        let _ = (root, request);
        ready(Err(KitError::new(KitError::UNSUPPORTED, "no file chooser")))
    }
    /// `ui.pasteImage`: the clipboard's picture (media.rs). Refused by
    /// default.
    fn clipboard_image(&self) -> BoxFuture<Result<Option<ClipboardImage>, KitError>> {
        ready(Err(KitError::new(KitError::UNSUPPORTED, "no clipboard")))
    }
}

/// A pane `panes.send` may reach: a Claude Code agent whose session runs
/// Hangar's bridge.
fn sendable(pane: &Pane, activity: Option<&PaneActivity>) -> bool {
    pane.agent == AgentId::Claude && activity.is_some_and(|now| now.bridged)
}

/// The workspace's panes, in their order, as a tab sees them (`kitPanes`).
pub fn kit_panes(
    ws: &Workspace,
    activity: impl Fn(&str) -> Option<PaneActivity>,
) -> Vec<api::Pane> {
    ws.panes
        .iter()
        .map(|pane| {
            let now = activity(&pane.id);
            api::Pane {
                name: pane.name.clone(),
                agent: pane.agent.as_str().to_owned(),
                role: pane.role.map(|role| {
                    serde_json::to_value(role)
                        .ok()
                        .and_then(|role| role.as_str().map(str::to_owned))
                        .unwrap_or_default()
                }),
                state: now.as_ref().map(|now| {
                    serde_json::to_value(now.activity)
                        .ok()
                        .and_then(|state| state.as_str().map(str::to_owned))
                        .unwrap_or_default()
                }),
                can_send: sendable(pane, now.as_ref()),
            }
        })
        .collect()
}

/// The answer of a call that answers nothing in particular: `{}`.
fn done() -> Value {
    json!({})
}

/// The kit API of one tab: [`KitApi`] for hangar-kit-host, over the app.
pub struct TabApi {
    cx: Arc<dyn TabContext>,
    deps: Arc<dyn Deps>,
}

impl TabApi {
    pub fn new(cx: Arc<dyn TabContext>, deps: Arc<dyn Deps>) -> TabApi {
        TabApi { cx, deps }
    }

    /// The workspace, still there.
    fn workspace_now(&self) -> Result<Workspace, KitError> {
        self.cx.workspace().ok_or_else(|| {
            error(
                KitError::UNAVAILABLE,
                "the workspace this tab was shown in is gone",
            )
        })
    }

    /// The pane `panes.send` names, or why it may not be reached.
    fn send_target(&self, ws: &Workspace, name: &str) -> Result<Pane, KitError> {
        let Some(pane) = ws
            .panes
            .iter()
            .find(|pane| pane.name.to_lowercase() == name.to_lowercase())
        else {
            return Err(error(
                KitError::NOT_FOUND,
                format!(
                    "no pane named {} in this workspace",
                    serde_json::to_string(name).unwrap_or_default()
                ),
            ));
        };
        if pane.agent != AgentId::Claude {
            return Err(error(
                KitError::DENIED,
                format!(
                    "panes.send only reaches Claude Code agents, and {} runs {}",
                    pane.name,
                    pane.agent.as_str()
                ),
            ));
        }
        if !sendable(pane, self.deps.activity(&pane.id).as_ref()) {
            return Err(error(
                KitError::UNAVAILABLE,
                format!(
                    "{}'s Claude Code is not running with Hangar's bridge, so nothing can wait for it to be free",
                    pane.name
                ),
            ));
        }
        Ok(pane.clone())
    }
}

fn ready<T: Send + 'static>(answer: T) -> BoxFuture<T> {
    Box::pin(std::future::ready(answer))
}

impl KitApi for TabApi {
    fn granted(&self) -> Vec<String> {
        self.cx.permissions()
    }

    /// What kitApi.ts refused before it took from a rate: a workspace gone,
    /// and a pane `panes.send` cannot reach. The other calls that need the
    /// workspace are refused the same way when they run.
    fn check(&self, call: &HostCall) -> Result<(), KitError> {
        match call {
            HostCall::PanesSend { pane, .. } => {
                let ws = self.workspace_now()?;
                self.send_target(&ws, pane).map(drop)
            }
            HostCall::FilesWrite { .. } | HostCall::FilesWriteBinary { .. } => {
                self.workspace_now().map(drop)
            }
            _ => Ok(()),
        }
    }

    fn call(&self, call: HostCall) -> CallFuture {
        let deps = self.deps.clone();
        let kit_id = self.cx.kit_id().to_owned();
        // What a call reads of the tab is read now, as the TS read it in the
        // handler, before anything is awaited.
        let workspace = || self.workspace_now();
        let answer = move |future: BoxFuture<Result<Value, KitError>>| -> CallFuture { future };
        match call {
            HostCall::WorkspaceInfo => hangar_kit_host::ready(
                workspace().map(|ws| json!({ "id": ws.id, "name": ws.name, "folder": ws.cwd })),
            ),
            HostCall::PanesList => hangar_kit_host::ready(workspace().map(|ws| {
                serde_json::to_value(kit_panes(&ws, |pane| deps.activity(pane)))
                    .unwrap_or(Value::Null)
            })),
            HostCall::PanesSend { pane, text } => {
                let target = workspace().and_then(|ws| self.send_target(&ws, &pane));
                answer(Box::pin(async move {
                    let pane = target?;
                    deps.enqueue(pane.id.clone(), text, kit_sender(&kit_id))
                        .await
                        .map_err(failed)?;
                    Ok(json!({ "queued": true, "pane": pane.name }))
                }))
            }
            // A tab holding `files.write` as well reads hidden files, which the
            // person accepted in those words; `files.read` alone keeps the
            // rules it always had, and its refusals are `failed`.
            HostCall::FilesRead { path, hidden } => {
                let ws = workspace();
                Box::pin(async move {
                    let ws = ws?;
                    if hidden {
                        deps.read_hidden_file(ws.cwd, path).await.map(Value::String)
                    } else {
                        deps.read_file(ws.cwd, path)
                            .await
                            .map(Value::String)
                            .map_err(failed)
                    }
                })
            }
            HostCall::FilesReadBinary { path, hidden } => {
                let ws = workspace();
                Box::pin(async move { deps.read_binary_file(ws?.cwd, path, hidden).await })
            }
            HostCall::FilesList { path, pattern } => {
                let ws = workspace();
                Box::pin(async move { deps.list_files(ws?.cwd, path, pattern).await })
            }
            HostCall::FilesWrite {
                path,
                text,
                expected,
            } => {
                let ws = workspace();
                Box::pin(async move {
                    deps.write_file(ws?.cwd, path, text, expected).await?;
                    Ok(done())
                })
            }
            HostCall::FilesWriteBinary { path, base64 } => {
                let ws = workspace();
                Box::pin(async move {
                    deps.write_binary_file(ws?.cwd, path, base64).await?;
                    Ok(done())
                })
            }
            HostCall::PreviewShow { path } => {
                let ws = workspace();
                let kit_name = self.cx.kit_name();
                Box::pin(async move {
                    let ws = ws?;
                    let real = deps
                        .resolve_file(ws.cwd.clone(), path.clone())
                        .await
                        .map_err(failed)?;
                    deps.show_file(ws, real, kit_name).await.map_err(failed)?;
                    Ok(json!({ "opened": true, "path": path }))
                })
            }
            // The kit is the app's own record of the tab, whatever the
            // message says.
            HostCall::DataRead { path } => Box::pin(async move {
                let text = deps.read_data(kit_id, path).await?;
                Ok(json!({ "text": text }))
            }),
            HostCall::DataWrite { path, text } => Box::pin(async move {
                deps.write_data(kit_id, path, text).await?;
                Ok(done())
            }),
            HostCall::LoopbackFetch(call) => {
                Box::pin(async move { deps.loopback_fetch(call).await })
            }
            // `name` was checked with the permission. No limit of its own:
            // tabs poll, and Rust holds a kit to four at once.
            HostCall::CommandRun { name, args } => {
                let ws = workspace();
                let accepted = self.cx.commands_accepted();
                Box::pin(async move {
                    let ws = ws?;
                    deps.run_command(CommandCall {
                        kit: kit_id,
                        name,
                        args,
                        cwd: ws.cwd,
                        accepted,
                    })
                    .await
                })
            }
            // Always under the kit's name: a kit does not speak as Hangar.
            HostCall::Notify { title, body } => {
                let titled = format!("{} · {title}", self.cx.kit_name());
                Box::pin(async move {
                    deps.notify(titled, body).await;
                    Ok(Value::Null)
                })
            }
            HostCall::TabParams => {
                let params = self.cx.params();
                let caller = self
                    .cx
                    .caller()
                    .map(|caller| json!({ "pane": caller.pane }));
                hangar_kit_host::ready(Ok(json!({ "params": params, "caller": caller })))
            }
            HostCall::TabSetTitle { title } => {
                self.cx.set_title(&title);
                hangar_kit_host::ready(Ok(done()))
            }
            HostCall::TabSetBadge { badge } => {
                self.cx.set_badge(badge.map(|(state, text)| KitBadge {
                    state: match state {
                        BadgeState::Running => KitBadgeState::Running,
                        BadgeState::Success => KitBadgeState::Success,
                        BadgeState::Failure => KitBadgeState::Failure,
                        BadgeState::Neutral => KitBadgeState::Neutral,
                    },
                    text,
                }));
                hangar_kit_host::ready(Ok(done()))
            }
            HostCall::Toast(toast) => deps.ui(UiCall::Toast(toast)),
            HostCall::Confirm(confirm) => deps.ui(UiCall::Confirm(confirm)),
            HostCall::Copy { text } => deps.ui(UiCall::Copy(text)),
            // A native tab does these itself, through `pick_file`,
            // `clipboard_image`, `image` and `files.writeBinary`; a page has
            // them in its own document and is answered `unknown` before the
            // gate.
            native @ (HostCall::FilesPick(_)
            | HostCall::PasteImage
            | HostCall::CanvasExport(_)
            | HostCall::ImageSize(_)) => hangar_kit_host::ready(Err(error(
                KitError::UNKNOWN,
                format!("no method {:?}", native.method()),
            ))),
        }
    }

    fn image(&self, source: &ImageSource) -> Result<Vec<u8>, KitError> {
        match source {
            ImageSource::File(path) => {
                let ws = self.workspace_now()?;
                let hidden = api::reads_hidden_files(&self.cx.permissions());
                self.deps.workspace_image(ws.cwd, path.clone(), hidden)
            }
            _ => self.deps.image(self.cx.kit_id(), source),
        }
    }

    fn pick_file(&self, request: &api::FilesPick) -> PickFuture {
        let (ws, deps, request) = (self.workspace_now(), self.deps.clone(), request.clone());
        Box::pin(async move { deps.pick_file(ws?.cwd, request).await })
    }

    fn clipboard_image(&self) -> ClipboardFuture {
        self.deps.clipboard_image()
    }
}

/// The methods of a native tab that stand for what a page has in its own
/// document (`hangar.ui`): no method of a page's protocol.
fn page_has_its_own(method: &str) -> bool {
    api::NATIVE_ONLY.contains(&method)
}

/// What to post to a page for a message it sent: an answer at once (a
/// refusal), or a call to run, whose answer [`PageHost::settle`] makes into
/// the message.
pub enum Post {
    Now(Value),
    Later(CallId, CallFuture),
}

impl std::fmt::Debug for Post {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Post::Now(answer) => write!(f, "Now({answer})"),
            Post::Later(id, _) => write!(f, "Later({id})"),
        }
    }
}

/// The host of one page, bound to its window (`createKitHost`): it answers
/// only what that window's page of the kit posts, through the gate every tab
/// goes through, and stops answering once closed.
pub struct PageHost {
    window: String,
    api: Arc<TabApi>,
    gate: Gate,
    /// The page's ids, by the gate's own.
    ids: HashMap<CallId, Value>,
    next: CallId,
    closed: bool,
}

impl PageHost {
    pub fn new(window: impl Into<String>, api: Arc<TabApi>) -> PageHost {
        PageHost {
            window: window.into(),
            api,
            gate: Gate::new(),
            ids: HashMap::new(),
            next: 0,
            closed: false,
        }
    }

    pub fn window(&self) -> &str {
        &self.window
    }

    /// An ipc of the helper: answered when it is a request of this window's
    /// page of the kit. `now` is when it came, for the rates.
    pub fn receive(&mut self, window: &str, uri: &str, body: &str, now: Instant) -> Option<Post> {
        if self.closed || window != self.window {
            return None;
        }
        // A native tab's message is held to 4 MB; so is a page's, before
        // anything parses it.
        if body.len() > hangar_webview::MAX_IPC_BODY_BYTES {
            return None;
        }
        if !kit_relay::is_kit_document(uri, self.api.cx.kit_id()) {
            return None;
        }
        let (id, method, params) = match kit_relay::read(body)? {
            Incoming::NoMethod { id } => {
                return Some(Post::Now(kit_relay::error(
                    &id,
                    KitError::INVALID,
                    "method must be a string",
                )));
            }
            Incoming::Call { id, method, params } => (id, method, params),
        };
        if self.gate.waiting() < limits::MAX_PENDING && page_has_its_own(&method) {
            return Some(Post::Now(kit_relay::error(
                &id,
                KitError::UNKNOWN,
                &format!(
                    "no method {}",
                    serde_json::to_string(&method).unwrap_or_default()
                ),
            )));
        }
        self.next += 1;
        let call = Call {
            id: self.next,
            method,
            params,
        };
        let granted = self.api.granted();
        let api = &self.api;
        match self
            .gate
            .admit(&call, &granted, false, now, |call| api.check(call))
        {
            Ok(admitted) => {
                self.ids.insert(call.id, id);
                Some(Post::Later(call.id, self.api.call(admitted)))
            }
            Err(refusal) => Some(Post::Now(kit_relay::error(
                &id,
                &refusal.code,
                &refusal.message,
            ))),
        }
    }

    /// A call answered: the message for the page, unless the page is gone.
    pub fn settle(&mut self, call: CallId, answer: Result<Value, KitError>) -> Option<Value> {
        self.gate.settle(call);
        let id = self.ids.remove(&call)?;
        if self.closed {
            return None;
        }
        Some(match answer {
            Ok(result) => kit_relay::result(&id, result),
            Err(refusal) => kit_relay::error(&id, &refusal.code, &refusal.message),
        })
    }

    /// One of Hangar's events, for the page when it may hear it.
    pub fn emit(&self, name: &str, data: Value) -> Option<Value> {
        if self.closed || !api::may_hear(name, &self.api.granted()) {
            return None;
        }
        Some(kit_relay::event(name, data))
    }

    /// The tab closed, or its page left: nothing is answered any more.
    pub fn close(&mut self) {
        self.closed = true;
    }
}

#[cfg(test)]
mod tests;
