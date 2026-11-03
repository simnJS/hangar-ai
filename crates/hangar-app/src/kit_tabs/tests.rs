//! The package driven as the shell drives it, without a core: a model with a
//! workspace that enables a kit, the preview's store, and the messages the
//! shell would send; what the tab's body draws is drawn headless by
//! `iced_test`. These hold what the pure modules cannot: that a module which
//! does not load falls back to the tab's page, what each state of a tab
//! says, that a page which wanders off is said to have left, and when the
//! running tabs hear that the panes changed.
//!
//! The preview's store is the process's (`Model.live.preview` can be made
//! over no other), shared with every test that opens a tab: each test has a
//! workspace of its own, and those that look at what is on screen take turns
//! (`SCREEN`), since `note_screens` replaces what every window shows.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use iced::{Settings as IcedSettings, Size, window};
use iced_test::Simulator;
use tokio::sync::watch;

use hangar_core::kits::KitInfo;
use hangar_i18n::{Key, Locale, Translator};
use hangar_webview::{CloseReason, Event, PageLoadState};

use super::bridge::PageTab;
use super::testing::{entry, kit, tab};
use super::*;
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::services::inbox;
use crate::shell::WindowKind;
use crate::state::types::{AppState, Pane};

/// Whoever looks at what is on screen takes this first: the store keeps one
/// list of the screens for every workspace.
static SCREEN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const KIT: &str = "demo-kit";
const TAB: &str = "board";

fn unique(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{tag}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// A kit folder on disk, removed with the test.
struct Folder(PathBuf);

impl Folder {
    fn new(tag: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!(
            "hangar-kit-tabs-{tag}-{}-{}",
            std::process::id(),
            unique("f")
        ));
        std::fs::create_dir_all(dir.join("tabs")).expect("a folder");
        Folder(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).expect("a file");
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Harness {
    pkg: Packages,
    model: Model,
    env: Env,
    ws: WsId,
    folder: Folder,
    /// Keeps the channels open.
    _snapshots: watch::Sender<Arc<Snapshot>>,
    _inbox: inbox::InboxReceiver,
}

/// The tab the harness opens: a page, and the module it prefers.
fn native_tab(module: Option<&str>) -> hangar_core::kits::Tab {
    hangar_core::kits::Tab {
        native: module.map(str::to_owned),
        ..tab(TAB, "Board", "tabs/board.html", &["panes.read"])
    }
}

impl Harness {
    /// A workspace enabling the kit with `panes.read` granted, whose folder
    /// holds `tabs/board.wasm` as given.
    fn new(tag: &str, tab: hangar_core::kits::Tab, module: Option<&[u8]>) -> Harness {
        let folder = Folder::new(tag);
        folder.write("tabs/board.html", b"<p>board</p>");
        if let Some(bytes) = module {
            folder.write("tabs/board.wasm", bytes);
        }
        let ws = unique(tag);
        let mut model = Model::default();
        model
            .live
            .windows
            .insert(window::Id::unique(), WindowKind::Main);
        model.hydrate(AppState {
            workspaces: vec![Workspace {
                id: ws.clone(),
                name: "demo".into(),
                cwd: folder.0.to_string_lossy().into_owned(),
                kits: Some(vec![entry(KIT, &["panes.read"])]),
                panes: vec![Pane {
                    id: format!("{ws}-ava"),
                    name: "Ava".into(),
                    ..Pane::default()
                }],
                ..Workspace::default()
            }],
            active_workspace_id: Some(ws.clone()),
            ..AppState::default()
        });
        let installed = vec![KitInfo {
            dir: folder.0.to_string_lossy().into_owned(),
            ..kit(KIT, vec![tab])
        }];
        model.live.kits.installed = Some(installed);
        let (snapshots, receiver) = watch::channel(Arc::new(Snapshot::of(&model)));
        let (inbox, inbox_receiver) = inbox::channel();
        let env = Env {
            host: Host::with_paths(hangar_core::Paths::rooted(&folder.0)),
            snapshots: receiver,
            inbox,
        };
        Harness {
            pkg: Packages::default(),
            model,
            env,
            ws,
            folder,
            _snapshots: snapshots,
            _inbox: inbox_receiver,
        }
    }

    fn st(&mut self) -> &mut State {
        &mut self.pkg.kit_tabs
    }

    /// The person opens the tab: it is in front, in a panel on screen.
    ///
    /// The 3D view's tests also say what is on screen in the process's store,
    /// and what one says another unsays: the screen is put back, a few times,
    /// until the page the tab shows has opened (a tab with no page never
    /// does, and costs the few steps).
    fn open(&mut self) -> String {
        let ws = self.ws.clone();
        let shared = self.pkg.kit_tabs.shared.clone();
        shared.edit(|store| {
            store.open_kit_tab(KitTabToOpen::new(ws.clone(), KIT, TAB, "Board"));
        });
        let id = {
            let _ = self.step(Msg::Changed);
            self.entry_id()
        };
        for _ in 0..20 {
            shared.edit(|store| store.note_screens([(ws.clone(), true)]));
            let _ = self.step(Msg::Changed);
            if self.pkg.kit_tabs.pages.contains_key(&id) {
                break;
            }
        }
        id
    }

    fn entry_id(&self) -> String {
        self.model
            .live
            .preview
            .scope(&self.ws)
            .and_then(|scope| scope.active_id.clone())
            .expect("a tab in front")
    }

    fn revision(&self, id: &str) -> u64 {
        self.model
            .live
            .preview
            .scope(&self.ws)
            .and_then(|scope| scope.entry(id))
            .map(|entry| entry.revision)
            .expect("the tab")
    }

    fn step(&mut self, msg: Msg) -> Task<Message> {
        step(&mut self.pkg.kit_tabs, &mut self.model, &self.env, msg)
    }

    /// The body of the tab, as the panel draws it.
    fn body<'a>(&'a self, locale: Locale) -> Element<'a, Message> {
        let id = self.entry_id_static();
        let ctx = Ctx {
            model: &self.model,
            pkg: &self.pkg,
            i18n: Translator::new(locale),
            theme: hangar_theme::by_id_or_default("graphite"),
            metrics: Metrics::default(),
            keymap: &self.model.live.keymap.index,
            window: window::Id::unique(),
            now: Instant::now(),
        };
        view::view(&self.pkg.kit_tabs, ctx, &self.ws, id)
    }

    /// The id of the tab in front, leaked for the length of the test: a view
    /// borrows it for as long as it is drawn.
    fn entry_id_static(&self) -> &'static str {
        Box::leak(self.entry_id().into_boxed_str())
    }

    fn page_of(&mut self, id: &str) -> Option<&mut PageTab> {
        self.pkg.kit_tabs.pages.get_mut(id)
    }
}

fn simulate<'a>(element: Element<'a, Message>) -> Simulator<'a, Message> {
    let settings = IcedSettings {
        fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
        default_font: hangar_theme::fonts::ui(),
        ..IcedSettings::default()
    };
    Simulator::with_size(settings, Size::new(900.0, 600.0), element)
}

fn said(locale: Locale, key: Key) -> String {
    Translator::new(locale).t(key).into_owned()
}

fn finished(window: &str) -> Event {
    Event::PageLoad {
        window: window.into(),
        state: PageLoadState::Finished,
        url: "hangar-kit://localhost/demo-kit/tabs/board.html".into(),
    }
}

/* ── a module that does not load ───────────────────────────────── */

#[test]
fn a_module_that_does_not_load_gives_way_to_the_tabs_page_and_says_so() {
    let mut h = Harness::new(
        "fallback",
        native_tab(Some("tabs/board.wasm")),
        Some(b"not a module"),
    );
    let _screen = SCREEN.blocking_lock();
    let id = h.open();

    // The module was tried at the revision the tab is at, and refused in the
    // runtime's words; no native tab runs, and the page is on its way.
    let revision = h.revision(&id);
    let st = h.st();
    let (tried, reason) = st.unloadable.get(&id).expect("the module was refused");
    assert_eq!(*tried, revision);
    assert!(reason.contains("not a WebAssembly module"), "{reason}");
    assert!(st.natives.is_empty());
    assert!(
        st.pages.contains_key(&id),
        "the tab has a page, shown in front of a panel on screen"
    );

    // The bar says it is a page that is shown, and why on hover; the body is
    // the page's own card (its window is starting).
    let body = h.body(Locale::En);
    let mut drawn = simulate(body);
    drawn
        .find(said(Locale::En, Key::KitsTabFallback))
        .expect("the bar says the page is a fallback");
    drawn
        .find(said(Locale::En, Key::PreviewWindowStarting))
        .expect("the page's window is starting");
    drop(drawn);

    // And in French.
    let mut drawn = simulate(h.body(Locale::Fr));
    drawn
        .find(said(Locale::Fr, Key::KitsTabFallback))
        .expect("the same, in French");
}

#[test]
fn a_module_that_does_not_load_and_a_tab_with_no_page_says_it_did_not_start() {
    // The entry is the module itself: there is no page to fall back to.
    let only_module = hangar_core::kits::Tab {
        entry: "tabs/board.wasm".into(),
        ..tab(TAB, "Board", "tabs/board.wasm", &["panes.read"])
    };
    let mut h = Harness::new("no-page", only_module, Some(b"\0asm but not quite"));
    let _screen = SCREEN.blocking_lock();
    let id = h.open();
    let st = h.st();
    assert!(st.unloadable.contains_key(&id));
    assert!(st.natives.is_empty() && st.pages.is_empty());

    let mut drawn = simulate(h.body(Locale::En));
    drawn
        .find(said(Locale::En, Key::KitsTabNativeNotStarted))
        .expect("the tab says it did not start");
    // The reason is shown, and the reload is offered in the bar and under it.
    let reason = h.pkg.kit_tabs.unloadable.get(&id).unwrap().1.clone();
    drawn.find(reason).expect("the reason");
}

#[test]
fn a_reload_tries_the_module_again_at_the_new_revision() {
    let mut h = Harness::new(
        "reload",
        native_tab(Some("tabs/board.wasm")),
        Some(b"not a module"),
    );
    let _screen = SCREEN.blocking_lock();
    let id = h.open();
    let first = h.st().unloadable.get(&id).unwrap().0;

    // Nothing asks for another try while the revision is the same.
    let _ = h.step(Msg::Tick);
    assert_eq!(h.st().unloadable.get(&id).unwrap().0, first);

    // The bar's reload: the verdict is forgotten, the store bumps the tab's
    // revision, and the next look tries the file again.
    let _ = h.step(Msg::Reload(id.clone()));
    let _ = h.step(Msg::Changed);
    let second = h.st().unloadable.get(&id).expect("refused again").0;
    assert!(second > first, "{second} after {first}");
    assert_eq!(second, h.revision(&id));
}

#[test]
fn a_module_that_appears_later_is_read_without_a_reload() {
    // The verdict on a file is kept for as long as the file is not written:
    // a linked kit that is built after its tab was opened is read when it is.
    let mut runtime = native::Runtime::default();
    let folder = Folder::new("appears");
    let file = folder.0.join("tabs/board.wasm");
    let absent = runtime.module(&file).err().expect("there is no file");
    assert!(absent.contains("board.wasm"), "{absent}");
    assert!(!absent.contains("not a WebAssembly module"), "{absent}");
    std::fs::write(&file, b"now there, but not a module").unwrap();
    let written = runtime.module(&file).err().expect("refused");
    assert!(written.contains("not a WebAssembly module"), "{written}");
}

/* ── what each state of a tab says ─────────────────────────────── */

#[test]
fn each_state_a_tab_cannot_run_in_is_said_in_words() {
    let _screen = SCREEN.blocking_lock();
    let en = |key| said(Locale::En, key);

    // The kit was not read yet.
    let mut h = Harness::new("states-loading", native_tab(None), None);
    let id = h.open();
    h.model.live.kits.installed = None;
    let mut drawn = simulate(h.body(Locale::En));
    drawn.find(en(Key::KitsTabLoading)).expect("loading");
    drop(drawn);

    // The kit was taken off the workspace.
    h.model.live.kits.installed = Some(vec![kit(KIT, vec![native_tab(None)])]);
    let ws = h.ws.clone();
    h.model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: ws.clone(),
            name: "demo".into(),
            kits: Some(Vec::new()),
            ..Workspace::default()
        }],
        active_workspace_id: Some(ws),
        ..AppState::default()
    });
    let mut drawn = simulate(h.body(Locale::En));
    drawn.find(en(Key::KitsTabDisabled)).expect("disabled");
    drawn.find(en(Key::KitsTabDisabledHint)).expect("its hint");
    drop(drawn);

    // The kit is enabled, and no longer installed.
    h.model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: h.ws.clone(),
            name: "demo".into(),
            kits: Some(vec![entry(KIT, &["panes.read"])]),
            ..Workspace::default()
        }],
        active_workspace_id: Some(h.ws.clone()),
        ..AppState::default()
    });
    h.model.live.kits.installed = Some(Vec::new());
    let mut drawn = simulate(h.body(Locale::En));
    drawn.find(en(Key::KitsTabMissing)).expect("missing");
    drawn.find(en(Key::KitsTabMissingHint)).expect("its hint");
    drop(drawn);

    // Installed and broken: its own words are the detail.
    h.model.live.kits.installed = Some(vec![KitInfo {
        error: Some("its manifest is invalid".into()),
        manifest: None,
        ..kit(KIT, Vec::new())
    }]);
    let mut drawn = simulate(h.body(Locale::En));
    drawn.find(en(Key::KitsTabBroken)).expect("broken");
    drawn.find("its manifest is invalid").expect("why");
    let _ = id;
}

#[test]
fn a_page_opens_only_in_front_of_a_panel_on_screen() {
    let mut h = Harness::new(
        "quiet",
        native_tab(Some("tabs/board.wasm")),
        Some(b"not a module"),
    );
    let _screen = SCREEN.blocking_lock();
    // Opened quietly and out of sight, as an agent adds a tab.
    let ws = h.ws.clone();
    let shared = h.pkg.kit_tabs.shared.clone();
    shared.edit(|store| {
        store.open_kit_tab(KitTabToOpen::new(ws.clone(), KIT, TAB, "Board").reveal(false));
        store.note_screens([(ws, false)]);
    });
    let _ = h.step(Msg::Changed);
    let id = h.model.live.preview.scope(&h.ws).expect("a tab").entries[0]
        .id
        .clone();
    // The module is tried at once, shown or not.
    assert!(h.st().unloadable.contains_key(&id));
    // The page it falls back to would take a window, and so the keyboard:
    // it waits for the tab to be in front of a panel on screen.
    assert!(!h.st().pages.contains_key(&id));
}

/* ── a page that wandered off ──────────────────────────────────── */

#[test]
fn a_page_that_goes_to_another_document_is_closed_and_the_tab_says_it_left() {
    let mut h = Harness::new("left", native_tab(None), None);
    let _screen = SCREEN.blocking_lock();
    let id = h.open();
    let window = h.st().pages.get(&id).expect("a page").window().to_owned();

    // The first document to finish loading is the tab's page: nothing is said.
    let page = h.page_of(&id).unwrap();
    assert!(matches!(
        page.on_event(&Event::Opened { window: window.clone() }, Instant::now()),
        bridge::Handled::Nothing
    ));
    assert_eq!(page.state, WinState::Open);
    let handled = page.on_event(&finished(&window), Instant::now());
    assert!(matches!(handled, bridge::Handled::Post(_)));
    assert!(!page.left);

    let mut drawn = simulate(h.body(Locale::En));
    drawn
        .find(said(Locale::En, Key::PreviewWindowTitle))
        .expect("the page is shown in its own window");
    drop(drawn);

    // A second one is somewhere the page went: its host stops answering, and
    // the tab says so, with the way back.
    let page = h.page_of(&id).unwrap();
    let handled = page.on_event(&finished(&window), Instant::now());
    assert!(matches!(handled, bridge::Handled::Nothing));
    assert!(page.left);

    let mut drawn = simulate(h.body(Locale::En));
    drawn.find(said(Locale::En, Key::KitsTabLeft)).expect("left");
    drawn
        .find(said(Locale::En, Key::KitsTabLeftHint))
        .expect("how to come back");
    drop(drawn);

    // The window closing after that is not "closed by the person": the tab
    // still says it left.
    let page = h.page_of(&id).unwrap();
    page.on_event(
        &Event::WindowClosed {
            window,
            reason: CloseReason::Command,
        },
        Instant::now(),
    );
    assert!(page.left);
    assert_ne!(page.state, WinState::Closed);
}

#[test]
fn a_window_the_person_closed_is_offered_again_and_a_failed_one_says_why() {
    let mut h = Harness::new("window-states", native_tab(None), None);
    let _screen = SCREEN.blocking_lock();
    let id = h.open();
    let window = h.st().pages.get(&id).unwrap().window().to_owned();

    h.page_of(&id).unwrap().on_event(
        &Event::WindowClosed {
            window: window.clone(),
            reason: CloseReason::User,
        },
        Instant::now(),
    );
    assert_eq!(h.st().pages.get(&id).unwrap().state, WinState::Closed);
    let mut drawn = simulate(h.body(Locale::En));
    drawn
        .find(said(Locale::En, Key::PreviewWindowClosed))
        .expect("closed");
    drawn
        .find(said(Locale::En, Key::PreviewWindowOpen))
        .expect("the way to open it again");
    drop(drawn);

    h.page_of(&id).unwrap().on_event(
        &Event::Error {
            window: Some(window),
            code: hangar_webview::ErrorCode::OpenFailed,
            message: "no display".into(),
        },
        Instant::now(),
    );
    let mut drawn = simulate(h.body(Locale::En));
    drawn
        .find(said(Locale::En, Key::PreviewWindowFailed))
        .expect("failed");
    drawn.find("no display").expect("why");
}

/* ── when the running tabs hear that the panes changed ──────────── */

fn pane(id: &str, name: &str) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        ..Pane::default()
    }
}

fn model_with_panes(ws: &str, panes: Vec<Pane>) -> Model {
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: ws.into(),
            name: "demo".into(),
            panes,
            ..Workspace::default()
        }],
        active_workspace_id: Some(ws.into()),
        ..AppState::default()
    });
    model
}

#[test]
fn the_panes_changed_are_those_whose_list_a_tab_would_read_differently() {
    let ws: WsId = unique("panes");
    let other: WsId = unique("panes-other");
    let mut last = HashMap::new();
    fn watched<'a>(ids: &[&'a WsId]) -> HashSet<&'a WsId> {
        ids.iter().copied().collect()
    }

    // The first reading of a workspace is what the tab was started with.
    let model = model_with_panes(&ws, vec![pane("a", "Ava")]);
    assert!(panes_changed(&mut last, &model, watched(&[&ws])).is_empty());
    assert!(panes_changed(&mut last, &model, watched(&[&ws])).is_empty());

    // A pane renamed, added, or taken away is a change, once.
    let renamed = model_with_panes(&ws, vec![pane("a", "Zoe")]);
    assert_eq!(
        panes_changed(&mut last, &renamed, watched(&[&ws])),
        HashSet::from([ws.clone()])
    );
    assert!(panes_changed(&mut last, &renamed, watched(&[&ws])).is_empty());
    let added = model_with_panes(&ws, vec![pane("a", "Zoe"), pane("b", "Ben")]);
    assert_eq!(panes_changed(&mut last, &added, watched(&[&ws])).len(), 1);
    let removed = model_with_panes(&ws, Vec::new());
    assert_eq!(panes_changed(&mut last, &removed, watched(&[&ws])).len(), 1);

    // A workspace nobody watches is not read, and one that is gone is
    // skipped.
    let model = model_with_panes(&ws, vec![pane("a", "Ava")]);
    assert!(panes_changed(&mut last, &model, watched(&[&other])).is_empty());
    assert!(!last.contains_key(&other));
}

#[test]
fn only_the_events_that_move_a_panes_state_tell_the_tabs() {
    use crate::services::CoreEvent;
    let moved = |event: CoreEvent| moves_panes(&event);
    assert!(moved(CoreEvent::Bridge {
        pane: "p".into(),
        event: json!({ "kind": "turn-start" }),
    }));
    assert!(moved(CoreEvent::PtyExit { pane: "p".into() }));
    assert!(!moved(CoreEvent::BridgeQueue {
        pane: "p".into(),
        pending: Vec::new(),
    }));
    assert!(!moved(CoreEvent::MemoryChanged));
    assert!(!moved(CoreEvent::KitsChanged));
}

#[test]
fn an_event_that_moves_a_pane_is_told_to_the_running_tabs_at_once() {
    use crate::services::CoreEvent;
    let mut h = Harness::new("event", native_tab(None), None);
    let _screen = SCREEN.blocking_lock();
    let id = h.open();
    assert!(h.st().pages.contains_key(&id));
    let event = CoreEvent::PtyExit {
        pane: format!("{}-ava", h.ws),
    };

    // The tab has been told the panes it starts with.
    let ws = h.ws.clone();
    assert!(h.st().panes.contains_key(&ws));
    let before = h.st().panes.get(&ws).cloned();

    // The pane is renamed in the model, and its shell ends: the tab hears of
    // it with the event, not a second later with the tick.
    h.model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: ws.clone(),
            name: "demo".into(),
            cwd: h.folder.0.to_string_lossy().into_owned(),
            kits: Some(vec![entry(KIT, &["panes.read"])]),
            panes: vec![pane(&format!("{ws}-ava"), "Zoe")],
            ..Workspace::default()
        }],
        active_workspace_id: Some(ws.clone()),
        ..AppState::default()
    });
    let (st, model) = (&mut h.pkg.kit_tabs, &h.model);
    assert!(tell_of_event(st, model, &event).is_some());
    assert_ne!(h.st().panes.get(&ws).cloned(), before, "it was read again");

    // With no tab running there is nobody to tell, and nothing is done.
    h.st().pages.clear();
    let (st, model) = (&mut h.pkg.kit_tabs, &h.model);
    assert!(tell_of_event(st, model, &event).is_none());
}
