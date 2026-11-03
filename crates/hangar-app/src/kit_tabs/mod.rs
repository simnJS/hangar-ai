//! P16: kit tabs and the kit API.
//!
//! Ports `preview/KitTabView.tsx`, `lib/{kitApi,kitTabs}.ts` and
//! `strings/kitsTab.ts` (inventory-frontend.md §2.4/1.4 and 1.8). A kit's tab
//! lives in its workspace's preview (P15's store, `Body::Kit`) and is one of
//! two kinds:
//!
//! - **native**, a WebAssembly module (`Tab::module`), run by
//!   hangar-kit-host and drawn here with Hangar's own widgets (`native.rs`);
//!   when its module cannot be loaded and the tab has a page, the page opens
//!   instead, and the bar says so;
//! - **HTML**, a page (`Tab::page`) in a window of the webview helper, served
//!   by hangar-core's `kit_scheme`, whose `window.hangar` calls are relayed
//!   back here (`bridge.rs`).
//!
//! Both answer through one API (`api.rs`, `TabApi` over the app's `Deps`)
//! behind one gate (hangar-kit-host's `Gate`), with the permissions the
//! workspace granted the kit, read afresh at each call (`tabs.rs`,
//! `workspace_kit_tabs`).
//!
//! # With the other packages
//!
//! - The preview (P15) draws the panel and calls [`view`] for a kit tab's
//!   body; its store keeps the tabs, their params, titles and badges, and
//!   `kit-tabs.json`. Tabs are opened by the person through
//!   `Intent::OpenKitTab` (the toolbar's Kits menu, the palette), and by
//!   agents through the control bus.
//! - The control bus (P18) gets [`catalog::Catalog`] once the state is read
//!   (`Hook::KitTabs`), for `kit_tab_list` and `kit_tab_open`.
//! - The palette (P7) and the toolbar (P6) list [`choices`].
//!
//! # When a tab runs
//!
//! A tab's page or module runs while its workspace is mounted (opened in
//! this run and not asleep), from the moment it would load (`kit_tab_loads`:
//! at once, unless brought back at startup and not shown yet), until it is
//! closed, its kit stops being usable on the workspace, or it is reloaded
//! (a new instance, a new window). Each change of the store and a tick every
//! second bring the running tabs in line (`reconcile`): what changed of the
//! panes (`panes.changed`), of the theme, of what is in front.

pub mod api;
mod bridge;
pub mod catalog;
pub mod deps;
mod dev;
mod native;
pub mod tabs;
mod view;

#[cfg(test)]
mod listed;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::futures::stream::{self, StreamExt};
use iced::{Element, Subscription, Task};
use serde_json::{Value, json};
use tokio::sync::watch;

use hangar_core::Host;

use hangar_kit_host::KitMessage;
use hangar_kit_ui::abi::CallId;
use hangar_kit_ui::api::KitError;

use self::api::{PageHost, TabApi, kit_panes};
use self::bridge::{Handled, PageTab};
use self::deps::{Installed, LiveContext, LiveDeps, TabRecord, UiAsk};
use self::native::{NativeTab, Started};
use self::tabs::{KitTabChoice, KitTabState, kit_tab_loads, kit_theme_event, kit_ui_scale};
use crate::control::{self, Hook};
use crate::ctx::Ctx;
use crate::kits::model::effective_kits;
use crate::message::{Intent, Message};
use crate::preview::access::Shared;
use crate::preview::store::{Entry, KitTabToOpen};
use crate::preview::webview::{StartError, WinState};
use crate::services::{CoreEvent, Inbox, Services};
use crate::shell::Region;
use crate::state::{Model, Snapshot, Workspace, WsId};

/// How often the running tabs are brought in line with what nothing tells
/// this package of: an agent's state, the installed kits, the helper.
const TICK: Duration = Duration::from_secs(1);
/// How often the package looks for the state to have been read.
const HYDRATION: Duration = Duration::from_millis(250);
/// How often a toast's time is looked at.
const TOAST_TICK: Duration = Duration::from_millis(250);

/// The package's UI state; lives in `App.pkg.kit_tabs`.
#[derive(Debug)]
pub struct State {
    shared: Arc<Shared>,
    installed: Arc<Installed>,
    runtime: native::Runtime,
    /// The running native tabs, by entry.
    pub(crate) natives: HashMap<String, NativeTab>,
    /// The open HTML tabs, by entry.
    pub(crate) pages: HashMap<String, PageTab>,
    /// Why a tab's module did not load, by entry, with the revision it was
    /// tried at: a reload tries again.
    pub(crate) unloadable: HashMap<String, (u64, String)>,
    /// Each workspace's panes as its tabs last saw them (`kitPanes`).
    panes: HashMap<WsId, String>,
    /// The control bus's hook was sent.
    hosted: bool,
    /// The helper sends this package its windows' events.
    routed: bool,
    helper_starting: bool,
    /// Numbers the windows, each open being a window of its own.
    windows: u64,
    /// `HANGAR_DEV_KIT_TABS` has been looked at (dev.rs).
    dev_done: bool,
    /// Tabs whose window the person asked for with the panel's buttons (open
    /// it, reload it): the next window of each is theirs, and may take the
    /// keyboard whoever opened the tab.
    asked: HashSet<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            shared: Shared::global(),
            installed: Arc::default(),
            runtime: native::Runtime::default(),
            natives: HashMap::new(),
            pages: HashMap::new(),
            unloadable: HashMap::new(),
            panes: HashMap::new(),
            hosted: false,
            routed: false,
            helper_starting: false,
            windows: 0,
            dev_done: false,
            asked: HashSet::new(),
        }
    }
}

/// The package's shared read-model. Other packages read the tabs a workspace
/// offers through [`choices`], from the model itself.
#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::KitTabs(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// The preview store changed.
    Changed,
    Tick,
    /// What a native tab's drawing sent, or one of its calls answered.
    Native(String, KitMessage),
    /// The bar's reload: the tab starts again, as a new instance or a new
    /// window.
    Reload(String),
    /// A tab got new params, or an agent or a hook asked for it again.
    Params(String),
    /// An event of the helper about one of this package's windows.
    Web(hangar_webview::Event),
    /// The helper started, or could not.
    Helper(Result<(), StartError>),
    /// A page's call answered: the entry, its window, the call.
    Answered(String, String, CallId, Result<Value, KitError>),
    OpenWindow(String),
    FocusWindow(String),
    /// A native tab's `ui.*`.
    Ui(String, UiAsk),
    ToastAction(String, u64),
    ToastDismiss(String, u64),
    ToastTick,
    Confirmed(String, bool),
}

fn send(msg: Msg) -> Message {
    Message::KitTabs(msg)
}

/// The tabs a workspace's kits bring (its own and those enabled everywhere),
/// for the palette and the toolbar's Kits menu (`kitChoices` of App.tsx).
pub fn choices(m: &Model, ws: &Workspace) -> Vec<KitTabChoice> {
    tabs::workspace_kit_tabs(
        &effective_kits(Some(ws), Some(m.settings())),
        m.live.kits.installed.as_deref(),
    )
}

/// Whether any kit is enabled on the workspace, its own or everywhere
/// (`hasKits`).
pub fn has_kits(m: &Model, ws: &Workspace) -> bool {
    !effective_kits(Some(ws), Some(m.settings())).is_empty()
}

/// What this package takes of the services: the core's host (its folders and
/// events), the model's snapshots, which a tab's API reads off the window's
/// thread, and the way back into `update`. Nothing else of them is needed,
/// which is what lets the tests drive the package without a running core.
pub(crate) struct Env {
    host: Host,
    snapshots: watch::Receiver<Arc<Snapshot>>,
    inbox: Inbox,
}

impl Env {
    fn of(sv: &Services) -> Env {
        Env {
            host: sv.core().host().clone(),
            snapshots: sv.snapshots(),
            inbox: sv.inbox().clone(),
        }
    }
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    step(st, m, &Env::of(sv), msg)
}

fn step(st: &mut State, m: &mut Model, env: &Env, msg: Msg) -> Task<Message> {
    let first = handle(st, m, msg);
    let then = reconcile(st, m, env);
    Task::batch([first, then])
}

/// A native tab's effects, as the app's tasks.
fn effects(id: &str, effects: hangar_kit_host::Effects) -> Task<Message> {
    if effects.is_empty() {
        return Task::none();
    }
    let id = id.to_owned();
    effects
        .task()
        .map(move |message| send(Msg::Native(id.clone(), message)))
}

fn post(st: &State, window: &str, messages: impl IntoIterator<Item = Value>) {
    let Some(client) = st.shared.helper().client() else {
        return;
    };
    for message in messages {
        // A helper that is gone has no page to tell.
        let _ = client.post_message(window, message);
    }
}

fn handle(st: &mut State, m: &Model, msg: Msg) -> Task<Message> {
    match msg {
        Msg::Changed | Msg::Tick => Task::none(),
        Msg::Native(id, message) => match st.natives.get_mut(&id) {
            Some(native) => {
                let done = native.tab.update(message);
                effects(&id, done)
            }
            None => Task::none(),
        },
        Msg::Reload(id) => {
            st.unloadable.remove(&id);
            st.asked.insert(id.clone());
            st.shared.edit(|store| store.reload_kit_tab(&id));
            Task::none()
        }
        Msg::Params(id) => {
            let (params, caller) = st
                .shared
                .read(|store| (store.kit_tab_params(&id), store.kit_tab_caller(&id)));
            if let Some(native) = st.natives.get_mut(&id) {
                let caller = caller.map(|caller| hangar_kit_ui::api::Caller { pane: caller.pane });
                let done = native.tab.set_params(params, caller);
                return effects(&id, done);
            }
            if let Some(page) = st.pages.get(&id) {
                let caller = caller.map(|caller| json!({ "pane": caller.pane }));
                let event = page.host.emit(
                    api::KIT_TAB_PARAMS_EVENT,
                    json!({ "params": params, "caller": caller }),
                );
                post(st, page.window(), event);
            }
            Task::none()
        }
        Msg::Web(event) => web_event(st, &event),
        Msg::Helper(started) => {
            st.helper_starting = false;
            match started {
                Ok(()) => {
                    if let Some(client) = st.shared.helper().client() {
                        let theme = m.theme();
                        for page in st.pages.values_mut() {
                            if page.state == WinState::Waiting {
                                page.open(&client, bridge::background_of(theme));
                            }
                        }
                    }
                }
                Err(error) => {
                    for page in st.pages.values_mut() {
                        if page.state == WinState::Waiting {
                            page.state = match &error {
                                StartError::Missing => WinState::Missing,
                                StartError::Failed(why) => WinState::Failed(why.clone()),
                            };
                        }
                    }
                }
            }
            Task::none()
        }
        Msg::Answered(id, window, call, answer) => {
            let Some(page) = st.pages.get_mut(&id).filter(|page| page.window() == window) else {
                return Task::none();
            };
            let answer = page.host.settle(call, answer);
            post(st, &window, answer);
            Task::none()
        }
        Msg::OpenWindow(id) => {
            // A new window and a new host, the next time the tab is looked at,
            // which is now: it is in front. It is the person's.
            st.asked.insert(id.clone());
            if let Some(page) = st.pages.remove(&id) {
                drop_page(st, &id, page);
            }
            Task::none()
        }
        Msg::FocusWindow(id) => {
            if let (Some(page), Some(client)) = (st.pages.get_mut(&id), st.shared.helper().client())
            {
                page.bring_forward(&client);
            }
            Task::none()
        }
        Msg::Ui(id, ask) => {
            if let api::UiCall::Copy(text) = &ask.call {
                ask.answer.send(Ok(json!({})));
                return iced::clipboard::write(text.clone());
            }
            match st.natives.get_mut(&id) {
                Some(native) => native.asked.ask(&id, ask, Instant::now()),
                None => {
                    ask.answer.send(Err(KitError::new(
                        KitError::UNAVAILABLE,
                        "the tab is no longer open",
                    )));
                }
            }
            Task::none()
        }
        Msg::ToastAction(id, seq) => {
            if let Some(native) = st.natives.get_mut(&id) {
                native.asked.toast_action(seq);
            }
            Task::none()
        }
        Msg::ToastDismiss(id, shown) => {
            if let Some(native) = st.natives.get_mut(&id) {
                native.asked.dismiss_toast(shown);
            }
            Task::none()
        }
        Msg::ToastTick => {
            let now = Instant::now();
            for native in st.natives.values_mut() {
                native.asked.tick(now);
            }
            Task::none()
        }
        Msg::Confirmed(id, yes) => {
            if let Some(native) = st.natives.get_mut(&id) {
                native.asked.confirmed(yes);
            }
            Task::none()
        }
    }
}

/// An event of the helper about one of this package's windows.
fn web_event(st: &mut State, event: &hangar_webview::Event) -> Task<Message> {
    let Some(window) = crate::preview::webview::event_window(event) else {
        return Task::none();
    };
    let Some((id, page)) = st
        .pages
        .iter_mut()
        .find(|(_, page)| page.window() == window)
    else {
        return Task::none();
    };
    let id = id.clone();
    let was_left = page.left;
    let handled = page.on_event(event, Instant::now());
    let window = page.window().to_owned();
    let left = !was_left && page.left;
    match handled {
        Handled::Nothing => {}
        Handled::Post(messages) => post(st, &window, messages),
        Handled::Call(call, future) => {
            return Task::perform(future, move |answer| {
                send(Msg::Answered(id.clone(), window.clone(), call, answer))
            });
        }
    }
    if left && let Some(client) = st.shared.helper().client() {
        let _ = client.close(&window);
    }
    Task::none()
}

/// What a tab is now: its entry, whether it is in front of a panel on
/// screen, its workspace's theme.
struct Seen {
    entry: Entry,
    shown: bool,
    theme: &'static hangar_theme::Theme,
}

/// Every kit tab of every workspace as the preview last synced it, and
/// whether it is in front of a panel on screen.
fn seen_tabs(st: &State, m: &Model) -> Vec<Seen> {
    let view = st.shared.read(|store| store.view().clone());
    let mut seen = Vec::new();
    for ws in &m.state().workspaces {
        let Some(scope) = m.live.preview.scope(&ws.id) else {
            continue;
        };
        for entry in scope.entries.iter().filter(|entry| entry.is_kit_tab()) {
            let shown =
                view.panel_shown(&ws.id) && scope.active_id.as_deref() == Some(entry.id.as_str());
            seen.push(Seen {
                entry: entry.clone(),
                shown,
                theme: theme_of(m, ws),
            });
        }
    }
    seen
}

fn theme_of(m: &Model, ws: &Workspace) -> &'static hangar_theme::Theme {
    hangar_theme::by_id_or_default(
        ws.theme_id
            .as_deref()
            .unwrap_or(m.settings().theme_id.as_str()),
    )
}

/// Brings the running tabs in line with the store and the model: starts what
/// should run, stops what should not, and tells each what changed.
fn reconcile(st: &mut State, m: &mut Model, env: &Env) -> Task<Message> {
    if !m.live.hydrated {
        return Task::none();
    }
    let mut tasks = Vec::new();
    if !st.hosted {
        st.hosted = true;
        tasks.push(Task::done(Message::Control(control::Msg::Install(
            Hook::KitTabs(Arc::new(catalog::Catalog)),
        ))));
    }
    st.installed.sync(m.live.kits.installed.as_deref());
    // A debug build asked to open tabs, once the kits are read.
    if !st.dev_done && m.live.kits.installed.is_some() {
        st.dev_done = true;
        if let Some(ws) = m.active_workspace().cloned() {
            for (kit, tab) in dev::requested() {
                let Some(choice) = choices(m, &ws)
                    .into_iter()
                    .find(|choice| choice.kit_id == kit && choice.tab.id == tab)
                else {
                    continue;
                };
                let open = KitTabToOpen::new(ws.id.clone(), kit, tab, choice.tab.title);
                st.shared.edit(|store| store.open_kit_tab(open));
            }
        }
    }
    m.live.preview.sync();
    let installed = st.installed.get();

    // Every kit tab of every workspace, and whether it is in front of a
    // panel on screen; one shown is no longer waiting for its first look.
    let seen = seen_tabs(st, m);
    for tab in &seen {
        if tab.shown && tab.entry.kit().is_some_and(|kit| kit.restored) {
            st.shared
                .edit(|store| store.mark_kit_tab_shown(&tab.entry.id));
        }
    }

    let mut keep_native = HashSet::new();
    let mut keep_page = HashSet::new();
    let locale = m.locale();
    for tab in &seen {
        let entry = &tab.entry;
        let Some(kit) = entry.kit() else { continue };
        let Some(ws) = m.workspace(&entry.workspace_id) else {
            continue;
        };
        let mounted = m.ui.opened_ids.contains(&entry.workspace_id)
            && !m.live.panes_logic.is_asleep(&entry.workspace_id);
        if !mounted {
            continue;
        }
        let state = tabs::tab_state_on(
            ws,
            m.settings(),
            installed.as_deref().map(Vec::as_slice),
            &kit.kit_id,
            &kit.tab_id,
        );
        let KitTabState::Ready { choice, url } = state else {
            continue;
        };
        let loads = kit_tab_loads(kit.restored, tab.shown);
        let dir = installed
            .as_deref()
            .and_then(|kits| kits.iter().find(|candidate| candidate.id == kit.kit_id))
            .map(|kit| PathBuf::from(&kit.dir));

        // The module, while it loads; the page, when it does not and the tab
        // has one.
        let tried = st
            .unloadable
            .get(&entry.id)
            .is_some_and(|(revision, _)| *revision == entry.revision);
        let module = choice
            .tab
            .module()
            .zip(dir.as_ref())
            .map(|(module, dir)| dir.join(module))
            .filter(|_| !tried);
        if let Some(path) = module {
            let current = st.natives.get(&entry.id).is_some_and(|native| {
                native.revision == entry.revision
                    && native.module == path
                    && native.modified == native::modified(&path)
            });
            if current {
                keep_native.insert(entry.id.clone());
                continue;
            }
            if !loads {
                continue;
            }
            st.runtime.cache_in(native::cache_dir(env.host.paths()));
            match st.runtime.module(&path) {
                Ok(module) => {
                    let runtime = match st.runtime.runtime() {
                        Ok(runtime) => runtime,
                        Err(reason) => {
                            st.unloadable
                                .insert(entry.id.clone(), (entry.revision, reason));
                            continue;
                        }
                    };
                    if let Some(mut old) = st.natives.remove(&entry.id) {
                        old.close();
                        unwatch(st, &entry.id, old.watch);
                    }
                    let api = tab_api(st, env, entry, &kit.kit_id, &kit.tab_id);
                    let version = installed
                        .as_deref()
                        .and_then(|kits| kits.iter().find(|candidate| candidate.id == kit.kit_id))
                        .and_then(|kit| kit.manifest.as_ref())
                        .map(|manifest| manifest.version.clone())
                        .unwrap_or_default();
                    let (mut native, first) = NativeTab::start(
                        &runtime,
                        &module,
                        Started {
                            kit_id: &kit.kit_id,
                            kit_name: &choice.kit_name,
                            kit_version: &version,
                            tab_id: &kit.tab_id,
                            params: kit.params.clone(),
                            caller: entry.opened_by.clone(),
                            theme: native::theme_of(tab.theme, locale),
                            visible: tab.shown,
                        },
                        api,
                        entry.revision,
                        path,
                    );
                    native.watch = Some(watch(st, env, &entry.id));
                    tasks.push(effects(&entry.id, first));
                    st.natives.insert(entry.id.clone(), native);
                    keep_native.insert(entry.id.clone());
                    continue;
                }
                Err(reason) => {
                    st.unloadable
                        .insert(entry.id.clone(), (entry.revision, reason));
                }
            }
        }
        let Some(url) = url else { continue };
        let open = st
            .pages
            .get(&entry.id)
            .is_some_and(|page| page.revision == entry.revision);
        if open {
            keep_page.insert(entry.id.clone());
            // The window is on screen while its tab is in front of a panel
            // that is, and hidden otherwise.
            let client = st.shared.helper().client();
            if let Some(page) = st.pages.get_mut(&entry.id) {
                page.set_seen(client.as_deref(), tab.shown);
            }
            continue;
        }
        if !tab.shown {
            continue;
        }
        if let Some(old) = st.pages.remove(&entry.id) {
            drop_page(st, &entry.id, old);
        }
        st.windows += 1;
        let window = format!("{}{}-{}", bridge::PREFIX, entry.id, st.windows);
        let api = tab_api(st, env, entry, &kit.kit_id, &kit.tab_id);
        // A window the person asked for is theirs; one for a tab an agent or a
        // hook opened does not take the keyboard from whoever is typing, and
        // where the window system would give it to any window it maps, is not
        // opened until the person asks (`bridge::PageTab::arriving`).
        let by_agent = entry.opened_by.is_some() && !st.asked.remove(&entry.id);
        let mut page = PageTab::new(
            PageHost::new(window, api),
            &kit.kit_id,
            url,
            entry.revision,
            &entry.title,
        )
        .arriving(by_agent, hangar_webview::opens_unfocused());
        page.watch = Some(watch(st, env, &entry.id));
        route(st, env);
        if !page.held {
            match st.shared.helper().client() {
                Some(client) => page.open(&client, bridge::background_of(tab.theme)),
                None => tasks.extend(start_helper(st, env)),
            }
        }
        st.pages.insert(entry.id.clone(), page);
        keep_page.insert(entry.id.clone());
    }

    // What should not run any more stops.
    let gone: Vec<String> = st
        .natives
        .keys()
        .filter(|id| !keep_native.contains(*id))
        .cloned()
        .collect();
    for id in gone {
        if let Some(mut native) = st.natives.remove(&id) {
            native.close();
            unwatch(st, &id, native.watch);
        }
    }
    let gone: Vec<String> = st
        .pages
        .keys()
        .filter(|id| !keep_page.contains(*id))
        .cloned()
        .collect();
    for id in gone {
        if let Some(page) = st.pages.remove(&id) {
            drop_page(st, &id, page);
        }
    }
    let live: HashSet<&str> = seen.iter().map(|tab| tab.entry.id.as_str()).collect();
    st.unloadable.retain(|id, _| live.contains(id.as_str()));
    st.asked.retain(|id| live.contains(id.as_str()));

    tasks.push(tell(st, m, &seen));
    Task::batch(tasks)
}

/// The workspaces among `watched` whose panes (their names, roles, states or
/// whether a send reaches them, `kitPanes`) differ from what `last` held
/// for them, which is brought up to date. A workspace not seen before only
/// takes its first reading: a tab has just been told the panes it starts
/// with.
fn panes_changed(
    last: &mut HashMap<WsId, String>,
    m: &Model,
    watched: HashSet<&WsId>,
) -> HashSet<WsId> {
    let mut changed = HashSet::new();
    for ws_id in watched {
        let Some(ws) = m.workspace(ws_id) else {
            continue;
        };
        let panes = serde_json::to_string(&kit_panes(ws, |pane| {
            m.live.panes_logic.pane_activity(pane).cloned()
        }))
        .unwrap_or_default();
        match last.insert(ws_id.clone(), panes.clone()) {
            Some(before) if before != panes => {
                changed.insert(ws_id.clone());
            }
            _ => {}
        }
    }
    changed
}

/// Tells the running tabs what changed: the panes of their workspace, their
/// theme, whether they are in front; a page's window takes its tab's name.
fn tell(st: &mut State, m: &Model, seen: &[Seen]) -> Task<Message> {
    let mut tasks = Vec::new();
    let locale = m.locale();
    let settings = m.settings();
    let scale = kit_ui_scale(settings.ui_scale);
    let client = st.shared.helper().client();

    // A helper that went away took the windows with it.
    if client.is_none() {
        for page in st.pages.values_mut() {
            if matches!(page.state, WinState::Open | WinState::Opening) {
                page.host.close();
                page.state = WinState::Closed;
            }
        }
    }

    // `panes.changed`: the panes, their names, roles or states, changed.
    let running: HashSet<&str> = st
        .natives
        .keys()
        .chain(st.pages.keys())
        .map(String::as_str)
        .collect();
    let watched: HashSet<&WsId> = seen
        .iter()
        .filter(|tab| running.contains(tab.entry.id.as_str()))
        .map(|tab| &tab.entry.workspace_id)
        .collect();
    let changed = panes_changed(&mut st.panes, m, watched);

    for tab in seen {
        let id = tab.entry.id.as_str();
        let ws_changed = changed.contains(&tab.entry.workspace_id);
        if let Some(native) = st.natives.get_mut(id) {
            let theme = native::theme_of(tab.theme, locale);
            let mut done = vec![
                native.tab.set_theme(theme),
                native.tab.set_visible(tab.shown),
            ];
            if ws_changed {
                done.push(native.tab.event("panes.changed", json!({})));
            }
            for effect in done {
                tasks.push(effects(id, effect));
            }
        }
        if let Some(page) = st.pages.get_mut(id) {
            let mut posted: Vec<Value> = Vec::new();
            let event = kit_theme_event(tab.theme, scale, &settings.font_family, locale);
            posted.extend(page.theme.set(event));
            if ws_changed {
                posted.extend(page.host.emit("panes.changed", json!({})));
            }
            if let Some(client) = &client {
                page.retitle(client, &tab.entry.title);
                for message in posted {
                    let _ = client.post_message(page.window(), message);
                }
            }
        }
    }
    Task::batch(tasks)
}

/// The API of a tab, over the app, from the app's own record of it.
fn tab_api(st: &State, env: &Env, entry: &Entry, kit_id: &str, tab_id: &str) -> Arc<TabApi> {
    let record = TabRecord {
        entry: entry.id.clone(),
        kit_id: kit_id.to_owned(),
        tab_id: tab_id.to_owned(),
        workspace_id: entry.workspace_id.clone(),
    };
    let cx = LiveContext::new(
        record,
        st.shared.clone(),
        env.snapshots.clone(),
        st.installed.clone(),
    );
    let deps = LiveDeps::new(
        env.host.clone(),
        st.shared.clone(),
        env.snapshots.clone(),
        env.inbox.clone(),
        st.installed.clone(),
        entry.id.clone(),
    );
    Arc::new(TabApi::new(Arc::new(cx), Arc::new(deps)))
}

/// Hears a tab's new params, and an agent or a hook asking for it again: the
/// store tells it under its lock, so this only queues a message.
fn watch(st: &State, env: &Env, id: &str) -> crate::preview::store::WatchId {
    let inbox = env.inbox.clone();
    let owner = id.to_owned();
    st.shared.edit(|store| {
        store.watch_kit_tab_params(
            id,
            Box::new(move |_, _| {
                inbox.send(send(Msg::Params(owner.clone())));
            }),
        )
    })
}

fn unwatch(st: &State, id: &str, watch: Option<crate::preview::store::WatchId>) {
    if let Some(watch) = watch {
        st.shared
            .edit(|store| store.unwatch_kit_tab_params(id, watch));
    }
}

/// A page goes: its host answers nothing more, its window closes.
fn drop_page(st: &State, id: &str, mut page: PageTab) {
    page.host.close();
    if let Some(client) = st.shared.helper().client() {
        let _ = client.close(page.window());
    }
    unwatch(st, id, page.watch);
}

/// The helper sends this package its windows' events and has it answer
/// their pages' requests, once.
fn route(st: &mut State, env: &Env) {
    if st.routed {
        return;
    }
    st.routed = true;
    let helper = st.shared.helper();
    let inbox = env.inbox.clone();
    helper.route(bridge::PREFIX, move |event| {
        inbox.send(send(Msg::Web(event.clone())));
    });
    helper.serve(bridge::PREFIX, bridge::answer);
}

/// Starts the helper, off the window's thread, unless it runs or is
/// starting.
fn start_helper(st: &mut State, env: &Env) -> Option<Task<Message>> {
    if st.helper_starting || st.shared.helper().client().is_some() {
        return None;
    }
    st.helper_starting = true;
    let helper = st.shared.helper().clone();
    let inbox = env.inbox.clone();
    Some(Task::perform(
        async move {
            match tokio::task::spawn_blocking(move || helper.start(inbox).map(drop)).await {
                Ok(started) => started,
                Err(why) => Err(StartError::Failed(why.to_string())),
            }
        },
        |started| send(Msg::Helper(started)),
    ))
}

/* ── subscriptions ─────────────────────────────────────────────── */

/// Follows the store's changes.
#[derive(Clone)]
struct Follow(Arc<Shared>);

impl std::hash::Hash for Follow {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.0) as usize).hash(state);
    }
}

fn changes(shared: &Arc<Shared>) -> Subscription<Message> {
    Subscription::run_with(Follow(shared.clone()), |follow| {
        let receiver = follow.0.follow();
        stream::unfold(receiver, |mut receiver| async move {
            receiver.changed().await.ok()?;
            Some((send(Msg::Changed), receiver))
        })
        .boxed()
    })
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let mut subscriptions = vec![changes(&st.shared)];
    if !m.live.hydrated {
        subscriptions.push(iced::time::every(HYDRATION).map(|_| send(Msg::Changed)));
    }
    let any = !st.natives.is_empty()
        || !st.pages.is_empty()
        || m.live.preview.any_open()
            && m.state().workspaces.iter().any(|ws| {
                m.live
                    .preview
                    .scope(&ws.id)
                    .is_some_and(|scope| scope.entries.iter().any(Entry::is_kit_tab))
            });
    if any {
        subscriptions.push(iced::time::every(TICK).map(|_| send(Msg::Tick)));
    }
    for (id, native) in &st.natives {
        subscriptions.push(
            native
                .tab
                .subscription()
                .with(id.clone())
                .map(|(id, message)| Message::KitTabs(Msg::Native(id, message))),
        );
    }
    if st
        .natives
        .values()
        .any(|native| native.asked.toasts.next_deadline().is_some())
    {
        subscriptions.push(iced::time::every(TOAST_TICK).map(|_| send(Msg::ToastTick)));
    }
    Subscription::batch(subscriptions)
}

/// What takes the focus when F6 reaches a region this package owns. It owns
/// none, so the shell never calls it.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

/// Every intent, after the shell's own handling of it: `Some` when this
/// package acted on it. `OpenKitTab` is the person's (the toolbar's Kits
/// menu, the palette): the tab opens in its workspace's preview, in front.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let Intent::OpenKitTab { ws, kit, tab } = intent else {
        return None;
    };
    let workspace = m.workspace(ws)?.clone();
    let choice = choices(m, &workspace)
        .into_iter()
        .find(|choice| choice.kit_id == *kit && choice.tab.id == *tab)?;
    let mut open = KitTabToOpen::new(ws.clone(), kit.clone(), tab.clone(), choice.tab.title);
    if choice.tab.singleton {
        open = open.singleton();
    }
    st.shared.edit(|store| store.open_kit_tab(open));
    Some(reconcile(st, m, &Env::of(sv)))
}

/// Whether a core event is one that changes what `panes.list` tells: a
/// report of a pane's hangar-bridge mod (its agent started or finished a turn,
/// asks for a permission), or its shell ending. The reducers of the panes
/// package have applied it to the model by the time it is offered here: that
/// package is offered every event first (`packages!` in `app.rs` lists
/// `panes_logic` first).
fn moves_panes(event: &CoreEvent) -> bool {
    matches!(event, CoreEvent::Bridge { .. } | CoreEvent::PtyExit { .. })
}

/// Every core event, after the shell's own handling of it. The installed
/// kits are P14's to read again; the next look here takes them.
///
/// A pane's state is the one thing a tab hears about that no store change and
/// no message of this package announces, so the events that move it tell the
/// running tabs at once (`panes.changed`), where the TypeScript followed the
/// activity live. What no event announces, a state read from a terminal's
/// output, still reaches them on the next tick, within a second.
pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = sv;
    tell_of_event(st, m, event)
}

fn tell_of_event(st: &mut State, m: &Model, event: &CoreEvent) -> Option<Task<Message>> {
    if !moves_panes(event) || (st.natives.is_empty() && st.pages.is_empty()) {
        return None;
    }
    let seen = seen_tabs(st, m);
    Some(tell(st, m, &seen))
}

/// A kit tab in the preview panel's body (called by `preview::view_entry`):
/// a native tab drawn by hangar-kit-host, or the card of an HTML one's
/// window.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>, ws: &'a WsId, tab: &'a str) -> Element<'a, Message> {
    view::view(st, ctx, ws, tab)
}
