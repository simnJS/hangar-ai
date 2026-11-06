//! The package driven as the shell drives it, without a core: a model with a
//! workspace, a store, and the messages the tasks of `update` would have
//! ended with, fed by hand. What the views draw is drawn headless by
//! `iced_test`; with `HANGAR_PREVIEW_SNAPSHOTS=<folder>` the scenes are
//! written there as PNGs (`cargo test -p hangar-app preview::tests::snapshots`).

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use iced::{Settings as IcedSettings, Size, window};
use iced_test::Simulator;
use tokio::sync::watch;

use hangar_i18n::{Key, Locale, Translator};
use hangar_theme::{fonts, to_iced};

use super::access::Shared;
use super::highlight;
use super::image_stage::Measured;
use super::model::{self, FileKind, Size as FrameSize};
use super::store::Status;
use super::text::Load;
use super::viewer::{Coloured, Loaded, ViewerMsg, ViewerState};
use super::webview::{StartError, WebMsg, WinState};
use super::{Env, Live, Msg, State, link, reconcile, text, viewer_update};
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::{Intent, Message};
use crate::services::inbox;
use crate::services::preview::PreviewRequest;
use crate::shell::{View, WindowKind};
use crate::state::{AppState, Model, Pane, Workspace, WsId};

/// hangar-core keeps one list of the folders it serves for the whole process:
/// the tests that grant folders take turns.
pub(crate) static ROOTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const WORKSPACE: &str = "w1";

struct Harness {
    st: State,
    model: Model,
    env: Env,
    shared: Arc<Shared>,
    dir: PathBuf,
    /// Keeps the snapshot channel open.
    _snapshots: watch::Sender<Arc<crate::state::Snapshot>>,
    _inbox: inbox::InboxReceiver,
}

fn scratch(tag: &str) -> PathBuf {
    let dir = crate::kit_tabs::testdir::scratch(&format!("preview-{tag}"));
    dir.canonicalize().expect("a canonical folder")
}

impl Harness {
    fn new(tag: &str) -> Harness {
        Harness::with_saved(tag, None)
    }

    /// A harness whose store keeps its kit tabs in `saved`, as the app's
    /// does in `kit-tabs.json`.
    fn with_saved(tag: &str, saved: Option<Box<dyn super::store::SavedTabs>>) -> Harness {
        let dir = scratch(tag);
        let shared = Shared::new(saved);
        let mut model = Model::default();
        // The main window: the shell decides, from it, which panels are on
        // screen.
        model
            .live
            .windows
            .insert(window::Id::unique(), WindowKind::Main);
        model.hydrate(AppState {
            workspaces: vec![Workspace {
                id: WORKSPACE.into(),
                name: "demo".into(),
                cwd: dir.to_string_lossy().into_owned(),
                panes: vec![Pane {
                    id: "pane-1".into(),
                    name: "Ava".into(),
                    ..Pane::default()
                }],
                ..Workspace::default()
            }],
            active_workspace_id: Some(WORKSPACE.into()),
            ..AppState::default()
        });
        model.live.preview = Live {
            shared: shared.clone(),
            scopes: Arc::default(),
            revision: u64::MAX,
        };
        let (snapshots, receiver) = watch::channel(Arc::new(crate::state::Snapshot::of(&model)));
        let (inbox, inbox_receiver) = inbox::channel();
        let st = State {
            shared: shared.clone(),
            ..State::default()
        };
        let mut harness = Harness {
            st,
            model,
            env: Env {
                snapshots: receiver,
                inbox,
            },
            shared,
            dir,
            _snapshots: snapshots,
            _inbox: inbox_receiver,
        };
        link(&mut harness.st, &harness.env);
        harness
    }

    fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, contents).expect("a file");
        path.to_string_lossy().into_owned()
    }

    /// What an agent's `preview_show` does.
    async fn show(&mut self, path: &str) -> String {
        let _turn = ROOTS.lock().await;
        let id = self
            .shared
            .show_request(PreviewRequest {
                ws: WORKSPACE.into(),
                path: path.into(),
                title: None,
                opened_by: Some("pane-1".into()),
                reveal: true,
            })
            .await
            .expect("the file is admitted");
        drop(_turn);
        self.tick();
        id
    }

    /// A message of the package, and what follows it.
    fn tick(&mut self) {
        link(&mut self.st, &self.env);
        let _ = reconcile(&mut self.st, &mut self.model, &self.env);
    }

    fn send(&mut self, msg: Msg) {
        let _ = super::step(&mut self.st, &mut self.model, &self.env, msg);
    }

    fn viewer(&mut self, id: &str, msg: ViewerMsg) {
        let revision = self
            .model
            .live
            .preview
            .entry(id)
            .map_or(0, |entry| entry.revision);
        let _ = viewer_update(&mut self.st, &self.model, &self.env, id, revision, msg);
        self.tick();
    }

    /// What the reading and colouring tasks of a text file end with.
    fn read_text(&mut self, id: &str) {
        let path = self
            .model
            .live
            .preview
            .entry(id)
            .expect("open")
            .path
            .clone();
        let head = text::read_head(Path::new(&path)).expect("readable");
        self.viewer(id, ViewerMsg::Text(Ok(Loaded::new(head))));
        let (generation, body, language) = {
            let viewer = self.st.viewers.get(id).expect("mounted");
            let ready = viewer.text().expect("read");
            let entry = self.model.live.preview.entry(id).expect("open");
            (
                ready.generation,
                ready.text.clone(),
                super::viewer::language_for(entry, viewer.kind),
            )
        };
        let lines = highlight::highlight_file(&body, &path, language);
        self.viewer(
            id,
            ViewerMsg::Coloured(generation, lines.map(|lines| Coloured(Arc::new(lines)))),
        );
    }

    fn read_image(&mut self, id: &str, stage: (f32, f32)) {
        let path = self
            .model
            .live
            .preview
            .entry(id)
            .expect("open")
            .path
            .clone();
        let bytes =
            super::image::read(Path::new(&path), super::decode::Size::Shown).expect("readable");
        self.viewer(id, ViewerMsg::Bytes(Ok(bytes)));
        // What the stage tells once the renderer has the picture.
        let natural = match image_size(&path) {
            Some((width, height)) => FrameSize::new(width as f32, height as f32),
            None => FrameSize::new(200.0, 120.0),
        };
        self.viewer(
            id,
            ViewerMsg::Image(super::image::Msg::Measured(Measured::Size(natural))),
        );
        self.viewer(
            id,
            ViewerMsg::Image(super::image::Msg::Stage(FrameSize::new(stage.0, stage.1))),
        );
    }

    /// The body of entry `id`'s view pane, as the grid draws it.
    fn body<'a>(&'a self, ctx: Ctx<'a>, id: &'a str) -> iced::Element<'a, Message> {
        static WS: std::sync::LazyLock<WsId> = std::sync::LazyLock::new(|| WORKSPACE.into());
        super::view_entry(&self.st, ctx, &WS, id)
    }

    fn ctx<'a>(
        &'a self,
        window: window::Id,
        pkg: &'a Packages,
        locale: Locale,
        theme: &str,
    ) -> Ctx<'a> {
        Ctx {
            model: &self.model,
            pkg,
            i18n: Translator::new(locale),
            theme: hangar_theme::by_id_or_default(theme),
            metrics: Metrics::default(),
            keymap: &self.model.live.keymap.index,
            window,
            now: Instant::now(),
        }
    }
}

/// The size of a PNG, from its header.
fn image_size(path: &str) -> Option<(u32, u32)> {
    let bytes = std::fs::read(path).ok()?;
    (bytes.starts_with(b"\x89PNG")).then(|| {
        let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        (word(16), word(20))
    })
}

fn only_entry(h: &Harness) -> String {
    let scope = h.model.live.preview.scope(WORKSPACE).expect("a preview");
    scope.active_id.clone().expect("a tab in front")
}

/* ── what an agent opens ───────────────────────────────────────── */

#[tokio::test]
async fn a_text_file_an_agent_shows_is_read_coloured_and_found_again_when_rewritten() {
    let mut h = Harness::new("text");
    let path = h.write("main.rs", "fn main() {\n    let answer = 42; // note\n}\n");
    let id = h.show(&path).await;
    // Mounted at once, reading.
    assert!(matches!(
        h.st.viewers.get(&id).and_then(|v| v.load.as_ref()),
        Some(Load::Loading)
    ));
    h.read_text(&id);
    let viewer = h.st.viewers.get(&id).unwrap();
    let ready = viewer.text().unwrap();
    assert_eq!(ready.text, "fn main() {\n    let answer = 42; // note\n}\n");
    assert!(
        ready
            .lines
            .iter()
            .flatten()
            .any(|run| run.class == highlight::Class::Keyword)
    );
    assert!(
        ready
            .lines
            .iter()
            .flatten()
            .any(|run| run.class == highlight::Class::Comment)
    );

    // The file is rewritten: the store hears of it, the viewer is built again.
    h.write("main.rs", "// new\n");
    h.shared.edit(|store| store.reload_model(&id, 99, Some(7)));
    h.tick();
    let viewer = h.st.viewers.get(&id).unwrap();
    assert_eq!(viewer.revision, 1);
    assert!(matches!(viewer.load, Some(Load::Loading)));
}

#[tokio::test]
async fn a_markdown_file_is_rendered_and_its_source_is_a_toggle_away() {
    let mut h = Harness::new("markdown");
    let path = h.write(
        "NOTES.md",
        "# Title\n\nText with `code`.\n\n## Setting up\n\nSee [it](#setting-up).\n",
    );
    let id = h.show(&path).await;
    h.read_text(&id);
    let viewer = h.st.viewers.get(&id).unwrap();
    let rendered = viewer.rendered.as_ref().expect("parsed");
    assert_eq!(
        super::markdown::heading_ids(
            &rendered.content,
            hangar_theme::by_id_or_default("graphite")
        ),
        ["title", "setting-up"]
    );
    assert!(!viewer.source);
    h.viewer(&id, ViewerMsg::ToggleSource);
    assert!(h.st.viewers.get(&id).unwrap().source);
}

#[tokio::test]
async fn a_markdown_pages_source_is_coloured_only_once_it_is_shown() {
    let mut h = Harness::new("markdown-colour");
    let path = h.write("NOTES.md", "# Title\n\n```rust\nfn main() {}\n```\n");
    let id = h.show(&path).await;
    let head = text::read_head(Path::new(&path)).expect("readable");
    h.viewer(&id, ViewerMsg::Text(Ok(Loaded::new(head))));
    // The page is rendered: its source is not given to the highlighter.
    assert!(!h.st.viewers.get(&id).unwrap().coloured);
    h.viewer(&id, ViewerMsg::ToggleSource);
    assert!(h.st.viewers.get(&id).unwrap().coloured);
    // Back to the page and to the source again: coloured once.
    h.viewer(&id, ViewerMsg::ToggleSource);
    h.viewer(&id, ViewerMsg::ToggleSource);
    assert!(h.st.viewers.get(&id).unwrap().coloured);
}

#[tokio::test]
async fn a_markdown_page_parsed_where_it_was_read_is_not_parsed_again() {
    let mut h = Harness::new("markdown-parsed");
    let path = h.write("NOTES.md", "# Title\n");
    let id = h.show(&path).await;
    let head = text::read_head(Path::new(&path)).expect("readable");
    // What the reading task parsed (here, on purpose, another page) is what
    // the viewer shows: the window's thread did not parse the file itself.
    let parsed =
        super::viewer::Parsed::new(super::markdown::Rendered::parse("# Elsewhere\n", &path));
    h.viewer(
        &id,
        ViewerMsg::Text(Ok(Loaded(Arc::new(head), parsed.clone()))),
    );
    let rendered = h.st.viewers.get(&id).unwrap().rendered.as_ref().unwrap();
    assert_eq!(
        super::markdown::heading_ids(
            &rendered.content,
            hangar_theme::by_id_or_default("graphite")
        ),
        ["elsewhere"]
    );
    // Taken once.
    assert!(parsed.take().is_none());
}

#[tokio::test]
async fn a_code_file_is_coloured_as_soon_as_it_is_read() {
    let mut h = Harness::new("code-colour");
    let path = h.write("main.rs", "fn main() {}\n");
    let id = h.show(&path).await;
    let head = text::read_head(Path::new(&path)).expect("readable");
    h.viewer(&id, ViewerMsg::Text(Ok(Loaded::new(head))));
    assert!(h.st.viewers.get(&id).unwrap().coloured);
}

#[tokio::test]
async fn an_image_is_framed_to_fit_and_the_wheel_takes_it_over() {
    let mut h = Harness::new("image");
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 64, 32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![255u8; 64 * 32 * 4]).unwrap();
    }
    let path = h.write("dot.png", &png);
    let id = h.show(&path).await;
    h.read_image(&id, (400.0, 300.0));
    let picture = &h.st.viewers.get(&id).unwrap().picture;
    assert!(picture.pic.is_some());
    assert_eq!(picture.natural, Some(FrameSize::new(64.0, 32.0)));
    // An icon shows at its size, sharp, rather than blown up.
    assert_eq!(picture.frame().zoom, 1.0);
    assert!(picture.chosen.is_none());
    h.viewer(
        &id,
        ViewerMsg::Image(super::image::Msg::Gesture(
            super::image_stage::Gesture::Zoom {
                factor: 2.0,
                at: (0.0, 0.0),
            },
        )),
    );
    let picture = &h.st.viewers.get(&id).unwrap().picture;
    assert_eq!(picture.frame().zoom, 2.0);
}

#[tokio::test]
async fn a_page_opens_a_window_once_and_says_when_the_helper_is_not_installed() {
    let mut h = Harness::new("page");
    let path = h.write("page.html", "<!doctype html><p>hi</p>");
    let id = h.show(&path).await;
    // In front of an open panel: its window is asked for, once.
    assert_eq!(h.st.windows.state(&id), Some(&WinState::Waiting));
    assert!(h.st.windows.was_tried(&id));
    h.tick();
    assert_eq!(h.st.windows.state(&id), Some(&WinState::Waiting));
    h.send(Msg::Web(WebMsg::Started(Err(StartError::Missing))));
    assert_eq!(h.st.windows.state(&id), Some(&WinState::Missing));
    // Asking again goes through the same door.
    h.viewer(&id, ViewerMsg::Window(super::windowed::Action::Open));
    assert_eq!(h.st.windows.state(&id), Some(&WinState::Waiting));
}

#[tokio::test]
async fn a_file_shown_quietly_is_a_pane_and_opens_no_window_until_it_is_on_screen() {
    let mut h = Harness::new("quiet");
    // The board is in front of the grid.
    h.model.ui.view = View::Board;
    let path = h.write("doc.pdf", b"%PDF-1.4\n");
    let _turn = ROOTS.lock().await;
    let id = h
        .shared
        .show_request(PreviewRequest {
            ws: WORKSPACE.into(),
            path,
            title: None,
            opened_by: None,
            reveal: false,
        })
        .await
        .unwrap();
    drop(_turn);
    h.tick();
    assert!(h.ws().has_view(&id), "it is a pane of the grid");
    assert!(!h.shown(&id));
    assert!(h.st.windows.get(&id).is_none(), "nobody looks at it yet");
    // Back to the terminals: its pane is on screen, and its window opens.
    h.model.ui.view = View::Terminals;
    h.tick();
    assert!(h.shown(&id));
    assert!(h.st.windows.get(&id).is_some());
}

#[tokio::test]
async fn closing_a_view_pane_closes_its_entry_its_viewer_and_its_window() {
    let mut h = Harness::new("close");
    let notes = h.write("notes.txt", "plain\n");
    let page = h.write("page.html", "<p>hi</p>");
    let first = h.show(&notes).await;
    let second = h.show(&page).await;
    h.read_text(&first);
    assert!(h.st.viewers.contains_key(&first));
    assert!(h.st.windows.get(&second).is_some());
    for pane in [first.clone(), second.clone()] {
        let intent = Intent::CloseView {
            ws: WORKSPACE.into(),
            pane,
        };
        assert!(super::heard(&mut h.st, &mut h.model, &h.env, &intent).is_some());
    }
    // The panes go at once; the rest follows the store.
    assert!(h.ws().view_panes.is_empty());
    h.tick();
    assert!(h.st.viewers.is_empty());
    assert!(h.st.windows.get(&second).is_none(), "its window closed");
    assert!(!h.model.live.preview.has_preview(WORKSPACE));
}

/* ── the previews as panes of the grid ─────────────────────────── */

impl Harness {
    fn ws(&self) -> &Workspace {
        self.model.workspace(WORKSPACE).expect("the workspace")
    }

    /// The entry's pane is on screen, as the store was told.
    fn shown(&self, id: &str) -> bool {
        self.shared.read(|store| store.view().shows(id))
    }
}

#[tokio::test]
async fn every_entry_is_a_pane_beside_the_focused_one_and_goes_with_it() {
    let mut h = Harness::new("panes");
    let one = h.show(&h.write("one.txt", "1")).await;
    let two = h.show(&h.write("two.txt", "2")).await;
    let ws = h.ws();
    assert_eq!(ws.panes.len(), 1, "the terminals are still the terminals");
    let order = crate::state::layout::grid_order(ws);
    assert_eq!(order, ["pane-1".to_string(), one.clone(), two.clone()]);
    assert!(h.shown(&one) && h.shown(&two));
    // With the focus on the first view, the next opens beside it.
    h.model
        .ui
        .focus_by_workspace
        .insert(WORKSPACE.into(), one.clone());
    let three = h.show(&h.write("three.txt", "3")).await;
    let order = crate::state::layout::grid_order(h.ws());
    assert_eq!(
        order,
        ["pane-1".to_string(), one.clone(), three, two.clone()]
    );
    // An entry closed by anyone takes its pane.
    h.shared.edit(|store| store.close_model(&one));
    h.tick();
    assert!(!h.ws().has_view(&one));
    assert!(h.ws().has_view(&two));
}

#[tokio::test]
async fn a_full_grid_has_no_room_for_another_and_says_so() {
    let mut h = Harness::new("full");
    for n in 1..crate::state::layout::MAX_PANES {
        let id = format!("v{n}");
        h.model.apply(crate::state::StoreAction::AddView {
            ws: WORKSPACE.into(),
            id,
            near: None,
            dir: None,
        });
    }
    // Views the store has not read back yet stay: the workspace's tabs are
    // still on their way.
    assert_eq!(h.ws().pane_count(), crate::state::layout::MAX_PANES);
    let path = h.write("one.txt", "1");
    let _turn = ROOTS.lock().await;
    let id = h
        .shared
        .show_request(PreviewRequest {
            ws: WORKSPACE.into(),
            path,
            title: Some("Notes".into()),
            opened_by: Some("pane-1".into()),
            reveal: true,
        })
        .await
        .unwrap();
    drop(_turn);
    let synced = super::panes::sync(&mut h.model);
    assert_eq!(synced.refused, ["Notes"]);
    assert!(
        synced
            .notice(&h.model)
            .is_some_and(|words| words.contains("16"))
    );
    assert!(
        h.shared.read(|store| store.entry(&id).is_none()),
        "closed again"
    );
    assert!(!h.ws().has_view(&id));
}

#[tokio::test]
async fn once_its_tabs_are_read_back_a_view_without_an_entry_goes() {
    let mut h = Harness::new("restored");
    // A view saved for a file, which the previews never kept.
    h.model.apply(crate::state::StoreAction::AddView {
        ws: WORKSPACE.into(),
        id: "p-old-1".into(),
        near: None,
        dir: None,
    });
    assert!(!h.shared.read(|store| store.is_restored(WORKSPACE)));
    super::panes::sync(&mut h.model);
    assert!(h.ws().has_view("p-old-1"), "its entry may be on its way");
    h.model.mount(WORKSPACE);
    h.tick();
    assert!(h.shared.read(|store| store.is_restored(WORKSPACE)));
    assert!(!h.ws().has_view("p-old-1"));
}

#[tokio::test]
async fn a_pane_zoomed_over_the_others_is_the_only_one_on_screen() {
    let mut h = Harness::new("zoom");
    let one = h.show(&h.write("one.txt", "1")).await;
    let two = h.show(&h.write("two.txt", "2")).await;
    h.model
        .ui
        .focus_by_workspace
        .insert(WORKSPACE.into(), two.clone());
    h.model.ui.zoom_by_workspace.insert(WORKSPACE.into(), true);
    h.tick();
    assert!(h.shown(&two));
    assert!(!h.shown(&one));
    // And the terminal zoomed hides them all.
    h.model
        .ui
        .focus_by_workspace
        .insert(WORKSPACE.into(), "pane-1".into());
    h.tick();
    assert!(!h.shown(&one) && !h.shown(&two));
}

#[tokio::test]
async fn the_person_opening_a_file_lands_on_its_pane() {
    let mut h = Harness::new("person");
    let path = h.write("one.txt", "1");
    let _turn = ROOTS.lock().await;
    let id = h
        .shared
        .show_request(PreviewRequest {
            ws: WORKSPACE.into(),
            path,
            title: None,
            opened_by: None,
            reveal: true,
        })
        .await
        .unwrap();
    drop(_turn);
    let task = super::handle(
        &mut h.st,
        &mut h.model,
        &h.env,
        Msg::Opened(WORKSPACE.into(), id.clone()),
    );
    assert!(h.ws().has_view(&id), "placed before the focus is asked");
    assert_eq!(task.units(), 1, "the focus is asked for");
}

/// Kit tabs kept across a restart, in memory.
#[derive(Clone, Default)]
struct Saved(Arc<std::sync::Mutex<std::collections::HashMap<String, serde_json::Value>>>);

impl super::store::SavedTabs for Saved {
    fn read(&self, workspace: &str) -> Option<serde_json::Value> {
        self.0.lock().unwrap().get(workspace).cloned()
    }

    fn write(&mut self, workspace: &str, tabs: Option<&[super::store::SavedKitTab]>) {
        let mut map = self.0.lock().unwrap();
        match tabs {
            Some(tabs) => {
                map.insert(workspace.into(), serde_json::to_value(tabs).unwrap());
            }
            None => {
                map.remove(workspace);
            }
        }
    }

    fn workspaces(&self) -> Vec<String> {
        self.0.lock().unwrap().keys().cloned().collect()
    }
}

#[tokio::test]
async fn kit_tabs_come_back_as_the_panes_they_were_and_older_ones_as_new_panes() {
    use crate::state::{SplitDir, StoreAction};
    let saved = Saved::default();
    saved.0.lock().unwrap().insert(
        WORKSPACE.into(),
        serde_json::json!([
            {"id": "p-kit-1", "kitId": "ci", "tabId": "run", "path": "kit:ci/run",
             "title": "Runs", "titled": false, "params": {}, "openedBy": null},
            // Saved by a version whose tabs were in a side panel: no id.
            {"kitId": "ci", "tabId": "logs", "path": "kit:ci/logs",
             "title": "Logs", "titled": false, "params": {}, "openedBy": null}
        ]),
    );
    let mut h = Harness::with_saved("restart", Some(Box::new(saved.clone())));
    // state.json as the last run left it: the tab's pane right of the
    // terminal, at the width the person dragged it to.
    h.model.apply(StoreAction::AddView {
        ws: WORKSPACE.into(),
        id: "p-kit-1".into(),
        near: Some("pane-1".into()),
        dir: Some(SplitDir::Row),
    });
    let tree = crate::state::layout::with_ratio(&crate::state::layout::grid_tree(h.ws()), &[], 0.3);
    h.model
        .apply(StoreAction::SetTree(WORKSPACE.into(), tree.clone()));
    h.model.mount(WORKSPACE);
    h.tick();

    let ws = h.ws();
    let ids: Vec<&str> = h
        .model
        .live
        .preview
        .scope(WORKSPACE)
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert!(ids.contains(&"p-kit-1"), "under the id its pane has");
    assert_eq!(ws.view_panes.len(), 2, "the older tab is a pane too");
    let logs = ids.iter().find(|id| **id != "p-kit-1").unwrap();
    assert!(ws.has_view(logs));
    // The person's layout is where it was: the old tab's pane is split for
    // the new one, the boundary they dragged has not moved.
    assert_eq!(
        crate::state::layout::ratio_at(ws.tree.as_ref().unwrap(), &[]),
        0.3
    );
    assert_eq!(crate::state::layout::grid_order(ws)[0], "pane-1");
    // And from now on both are saved under their panes' ids.
    let kept = super::store::SavedTabs::read(&saved, WORKSPACE).unwrap();
    let kept: Vec<&str> = kept
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| tab["id"].as_str().unwrap())
        .collect();
    assert!(kept.contains(&"p-kit-1") && kept.contains(logs));
}

#[tokio::test]
async fn closing_the_previews_of_a_workspace_closes_every_pane() {
    let mut h = Harness::new("close-all");
    let page = h.write("page.html", "<p>hi</p>");
    let first = h.show(&h.write("one.txt", "1")).await;
    let second = h.show(&page).await;
    h.read_text(&first);
    assert!(h.st.windows.get(&second).is_some());

    let intent = Intent::ClosePreviews(WORKSPACE.into());
    assert!(super::heard(&mut h.st, &mut h.model, &h.env, &intent).is_some());
    // The panes go at once; the rest follows the store.
    assert!(h.ws().view_panes.is_empty());
    h.tick();
    assert!(!h.model.live.preview.has_preview(WORKSPACE));
    assert!(h.st.viewers.is_empty());
    assert!(h.st.windows.get(&second).is_none());
}

#[tokio::test]
async fn the_hook_for_the_control_bus_goes_out_once_the_state_is_read() {
    use crate::control::Hook;

    let mut h = Harness::new("hook");
    h.model.live.hydrated = false;
    assert!(super::hook_to_install(&mut h.st, &h.model).is_none());
    h.model.live.hydrated = true;
    let Some(Hook::Preview(hosts)) = super::hook_to_install(&mut h.st, &h.model) else {
        panic!("the previews' hosts");
    };
    assert!(
        super::hook_to_install(&mut h.st, &h.model).is_none(),
        "once"
    );

    // What an agent shows through it is what the person has in front of them.
    let id = h.show(&h.write("one.txt", "1")).await;
    let host = hosts.host(&WORKSPACE.to_string(), false).expect("a host");
    let listing = host.list().await.expect("listed");
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].id, id);
    assert_eq!(listing.active_id.as_deref(), Some(id.as_str()));
    // And its grid is drawn there.
    assert!(listing.on_screen);
    assert!(listing.shown);
    h.model.ui.view = View::Board;
    h.tick();
    let listing = host.list().await.expect("listed");
    assert!(listing.on_screen);
    assert!(
        !listing.shown,
        "the board is in front: nobody sees the file"
    );
}

#[tokio::test]
async fn a_file_that_changed_is_noticed_a_moment_after_it_stops_changing() {
    let mut h = Harness::new("watch");
    let path = h.write("run.log", "one\n");
    let id = h.show(&path).await;
    let before = h.model.live.preview.entry(&id).unwrap().modified_ms;
    // What the two looks of the watch end with.
    let seen = |modified_ms, size| model::PreviewFile {
        path: path.clone(),
        kind: model::Kind::Text,
        mime: "text/plain".into(),
        size,
        name: "run.log".into(),
        modified_ms,
    };
    h.st.watching.insert(id.clone());
    h.send(Msg::Watched(id.clone(), 0, Ok(seen(before + 1000, 9))));
    h.send(Msg::Settled(
        id.clone(),
        0,
        Box::new(seen(before + 1000, 9)),
        Ok(seen(before + 1000, 9)),
    ));
    let entry = h.model.live.preview.entry(&id).unwrap();
    assert_eq!((entry.revision, entry.modified_ms), (1, before + 1000));

    // Written in several goes: the second look differs, nothing is loaded.
    h.st.watching.insert(id.clone());
    h.send(Msg::Settled(
        id.clone(),
        1,
        Box::new(seen(before + 2000, 9)),
        Ok(seen(before + 2500, 20)),
    ));
    assert_eq!(h.model.live.preview.entry(&id).unwrap().revision, 1);
    // And a file that is gone says so, keeping what was there.
    h.st.watching.insert(id.clone());
    h.send(Msg::Watched(id.clone(), 1, Err("gone".into())));
    assert_eq!(h.st.missing.get(&id).map(String::as_str), Some("gone"));
    h.st.watching.insert(id.clone());
    h.send(Msg::Watched(id.clone(), 1, Ok(seen(before + 1000, 9))));
    assert!(h.st.missing.is_empty());
}

#[tokio::test]
async fn copying_the_text_says_so_and_stops_saying_so() {
    let mut h = Harness::new("copy");
    let path = h.write("notes.txt", "plain\n");
    let id = h.show(&path).await;
    h.read_text(&id);
    h.viewer(&id, ViewerMsg::Copy);
    let (_, notice) = h.st.notices.get(&id).expect("a word");
    assert_eq!(notice.text, hangar_i18n::t(Locale::En, Key::PreviewCopied));
    assert!(!notice.error);
    let seq = h.st.notices.get(&id).unwrap().0;
    h.send(Msg::NoticeGone(id.clone(), seq - 1));
    assert!(h.st.notices.contains_key(&id), "a newer word stays");
    h.send(Msg::NoticeGone(id.clone(), seq));
    assert!(!h.st.notices.contains_key(&id));
}

#[tokio::test]
async fn wrapping_the_lines_keeps_the_place_the_view_was_scrolled_to() {
    let mut h = Harness::new("wrap-keeps");
    let long: String = (1..=300).map(|n| format!("line {n}\n")).collect();
    let id = h.show(&h.write("long.txt", long)).await;
    h.read_text(&id);
    let place = super::code_lines::Scroll {
        offset: 1234.0,
        height: 500.0,
    };
    h.viewer(&id, ViewerMsg::CodeScrolled(place));
    assert!(!h.st.wrap);
    h.viewer(&id, ViewerMsg::ToggleWrap);
    assert!(h.st.wrap);
    assert_eq!(h.st.viewers.get(&id).map(|v| v.scroll), Some(place));
    h.viewer(&id, ViewerMsg::ToggleWrap);
    assert!(!h.st.wrap);
    assert_eq!(h.st.viewers.get(&id).map(|v| v.scroll), Some(place));
}

#[tokio::test]
async fn the_status_of_a_file_is_ready_at_once_and_a_model_waits_for_its_viewer() {
    let mut h = Harness::new("status");
    let path = h.write("notes.txt", "plain\n");
    let id = h.show(&path).await;
    assert_eq!(
        h.model.live.preview.entry(&id).unwrap().status,
        Status::Ready
    );
    assert_eq!(h.st.viewers.get(&id).map(|v| v.kind), Some(FileKind::Text));
}

/* ── selecting text ────────────────────────────────────────────── */

/// A triple click on the text `target` of the only entry's pane: the
/// messages the viewer was told, in order.
///
/// Three clicks are a triple click only within the double-click delay
/// (300 ms between presses), and each press lays the text out again: on a
/// machine busy with the other tests the delay can pass, and the clicks read
/// as a double one. The gesture is tried again, a few times, until the
/// viewer is told `target` was selected; the last try is answered whatever
/// it gave.
fn click_thrice(h: &Harness, target: &str, size: Size) -> Vec<ViewerMsg> {
    let pkg = Packages::default();
    let id = only_entry(h);
    let mut told = Vec::new();
    for _ in 0..5 {
        let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
        let mut ui = simulate(h.body(ctx, &id), size);
        ui.click(target).expect("the text is on screen");
        let _ = ui.simulate(iced_test::simulator::click().chain(iced_test::simulator::click()));
        told = ui
            .into_messages()
            .filter_map(|message| match message {
                Message::Preview(Msg::Viewer(_, _, msg)) => Some(msg),
                _ => None,
            })
            .collect();
        let whole = told.iter().any(|msg| {
            matches!(msg, ViewerMsg::Selected(text) if text == target)
                || matches!(msg,
                    ViewerMsg::Markdown(super::markdown::Msg::Selected(text)) if text == target)
        });
        if whole {
            break;
        }
    }
    told
}

#[tokio::test]
async fn a_line_of_code_is_selected_with_the_pointer_and_copy_takes_only_that() {
    let mut h = Harness::new("select-code");
    let path = h.write("notes.txt", "first line\nsecond line\nthird line\n");
    let id = h.show(&path).await;
    h.read_text(&id);
    let told = click_thrice(&h, "second line", Size::new(900.0, 600.0));
    let selected = told
        .iter()
        .rev()
        .find_map(|msg| match msg {
            ViewerMsg::Selected(text) => Some(text.clone()),
            _ => None,
        })
        .expect("the viewer is told what was selected");
    assert_eq!(selected, "second line");
    h.viewer(&id, ViewerMsg::Selected(selected));
    assert_eq!(
        h.st.viewers.get(&id).and_then(ViewerState::copy_text),
        Some("second line".into())
    );
    // With nothing selected, Copy takes the whole file.
    h.viewer(&id, ViewerMsg::Selected(String::new()));
    assert_eq!(
        h.st.viewers.get(&id).and_then(ViewerState::copy_text),
        Some("first line\nsecond line\nthird line\n".into())
    );
}

#[tokio::test]
async fn a_link_on_a_page_is_still_followed_and_selects_nothing() {
    let mut h = Harness::new("select-link");
    let path = h.write(
        "README.md",
        "# Top\n\n[jump to the code](#code)\n\n## Code\n\ntext\n",
    );
    let id = h.show(&path).await;
    h.read_text(&id);
    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
    let mut ui = simulate(h.body(ctx, &id), Size::new(900.0, 600.0));
    ui.click("jump to the code").expect("the link is on screen");
    let told: Vec<ViewerMsg> = ui
        .into_messages()
        .filter_map(|message| match message {
            Message::Preview(Msg::Viewer(_, _, msg)) => Some(msg),
            _ => None,
        })
        .collect();
    assert!(
        told.iter().any(|msg| matches!(msg,
            ViewerMsg::Markdown(super::markdown::Msg::Link(href)) if href == "#code")),
        "{told:?}"
    );
    assert!(
        !told
            .iter()
            .any(|msg| matches!(msg, ViewerMsg::Markdown(super::markdown::Msg::Selected(_)))),
        "a click on a link is not the start of a selection: {told:?}"
    );
}

#[tokio::test]
async fn a_selection_is_let_go_when_the_source_is_toggled() {
    let mut h = Harness::new("select-toggle");
    let path = h.write("NOTES.md", "# Title\n\ntext\n");
    let id = h.show(&path).await;
    h.read_text(&id);
    h.viewer(&id, ViewerMsg::Selected("text".into()));
    h.viewer(&id, ViewerMsg::ToggleSource);
    let viewer = h.st.viewers.get(&id).expect("mounted");
    assert_eq!(viewer.selection, "", "the lines are not the page's text");
}

#[tokio::test]
async fn a_paragraph_of_a_page_is_selected_with_the_pointer_and_link_text_is_text() {
    let mut h = Harness::new("select-page");
    let path = h.write(
        "README.md",
        "# Title\n\nSome *emphasised* words and [a link](https://example.com) here.\n\nSecond paragraph.\n",
    );
    let id = h.show(&path).await;
    h.read_text(&id);
    let told = click_thrice(&h, "Second paragraph.", Size::new(900.0, 600.0));
    let selected = told.iter().rev().find_map(|msg| match msg {
        ViewerMsg::Markdown(super::markdown::Msg::Selected(text)) => Some(text.clone()),
        _ => None,
    });
    assert_eq!(selected.as_deref(), Some("Second paragraph."), "{told:?}");
    // The text of a paragraph is what its spans say, links and emphasis
    // included, in one piece.
    let told = click_thrice(
        &h,
        "Some emphasised words and a link here.",
        Size::new(900.0, 600.0),
    );
    assert!(
        told.iter().any(|msg| matches!(msg,
            ViewerMsg::Markdown(super::markdown::Msg::Selected(text))
                if text == "Some emphasised words and a link here.")),
        "{told:?}"
    );
}

/* ── the views ─────────────────────────────────────────────────── */

fn simulate<'a>(element: iced::Element<'a, Message>, size: Size) -> Simulator<'a, Message> {
    let settings = IcedSettings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..IcedSettings::default()
    };
    Simulator::with_size(settings, size, element)
}

#[tokio::test]
async fn every_viewer_draws_in_its_pane_in_both_languages() {
    let mut h = Harness::new("view");
    let notes = h.write("notes.txt", "plain notes\nsecond line\n");
    let page = h.write("page.html", "<p>hi</p>");
    let first = h.show(&notes).await;
    let second = h.show(&page).await;
    h.read_text(&first);
    h.send(Msg::Web(WebMsg::Started(Err(StartError::Missing))));
    let pkg = Packages::default();
    let window = window::Id::unique();
    for locale in [Locale::En, Locale::Fr] {
        for id in [&first, &second] {
            let ctx = h.ctx(window, &pkg, locale, "graphite");
            let mut ui = simulate(h.body(ctx, id), Size::new(600.0, 500.0));
            let _ = ui.snapshot(&to_iced(ctx.theme)).expect("it draws");
        }
    }
    // The text is there to read.
    let ctx = h.ctx(window, &pkg, Locale::En, "graphite");
    let mut ui = simulate(h.body(ctx, &first), Size::new(600.0, 500.0));
    assert!(ui.find("second line").is_ok());
}

/// How long a file of the longest the viewer draws takes: read, coloured, laid
/// out and drawn. Printed with `--nocapture`, for whoever tunes it.
#[tokio::test]
async fn a_long_file_is_drawn_in_a_moment() {
    let mut h = Harness::new("long");
    let source: String = (0..super::model::TEXT_HEAD_LINES + 1000)
        .map(|i| format!("    let v{i} = f({i}); // {i}\n"))
        .collect();
    let path = h.write("big.rs", &source);
    let id = h.show(&path).await;
    let started = Instant::now();
    h.read_text(&id);
    let read = started.elapsed();
    let ready = h.st.viewers.get(&id).unwrap().text().unwrap();
    assert_eq!(ready.lines.len(), super::model::TEXT_HEAD_LINES);
    assert!(ready.truncated);

    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
    let started = Instant::now();
    let mut ui = simulate(h.body(ctx, &id), Size::new(480.0, 620.0));
    let _ = ui.snapshot(&to_iced(ctx.theme)).expect("it draws");
    let drawn = started.elapsed();
    eprintln!("read and coloured in {read:?}, laid out and drawn in {drawn:?}");
}

/// The scenes, as pictures, when `HANGAR_PREVIEW_SNAPSHOTS` names a folder.
#[tokio::test]
async fn snapshots() {
    let Some(out) = std::env::var_os("HANGAR_PREVIEW_SNAPSHOTS").map(PathBuf::from) else {
        return;
    };
    let source = std::env::var_os("HANGAR_PREVIEW_SAMPLES")
        .map(PathBuf::from)
        .expect("HANGAR_PREVIEW_SAMPLES names a folder of sample files");
    std::fs::create_dir_all(&out).unwrap();
    let theme_ids: Vec<String> = std::env::var("HANGAR_PREVIEW_THEMES")
        .unwrap_or_else(|_| "graphite,paper".into())
        .split(',')
        .map(str::to_string)
        .collect();
    let locales = [Locale::En, Locale::Fr];
    let scenes = [
        "main.rs",
        "data.json",
        "NOTES.md",
        "shot.png",
        "logo.svg",
        "page.html",
        "doc.pdf",
        "clip.mp4",
        "notes.txt",
    ];
    for scene in scenes {
        let mut h = Harness::new("snap");
        let copied = h.dir.join(scene);
        std::fs::copy(source.join(scene), &copied).expect("a sample");
        let id = h.show(&copied.to_string_lossy()).await;
        let kind = h
            .model
            .live
            .preview
            .entry(&id)
            .and_then(|entry| entry.file_kind())
            .expect("a file");
        match kind {
            FileKind::Text | FileKind::Markdown => h.read_text(&id),
            FileKind::Image => h.read_image(&id, (440.0, 520.0)),
            _ => h.send(Msg::Web(WebMsg::Started(Err(StartError::Missing)))),
        }
        let pkg = Packages::default();
        let window = window::Id::unique();
        for theme_id in &theme_ids {
            for locale in locales {
                let ctx = h.ctx(window, &pkg, locale, theme_id);
                let mut ui = simulate(h.body(ctx, &id), Size::new(480.0, 620.0));
                let snapshot = ui.snapshot(&to_iced(ctx.theme)).expect("it draws");
                let stem = format!(
                    "{}-{theme_id}-{}",
                    scene.replace('.', "_"),
                    if locale == Locale::En { "en" } else { "fr" }
                );
                for old in std::fs::read_dir(&out).unwrap().flatten() {
                    let name = old.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&format!("{stem}-")) && name.ends_with(".png") {
                        let _ = std::fs::remove_file(old.path());
                    }
                }
                snapshot.matches_image(out.join(&stem)).expect("written");
            }
        }
        let _ = std::fs::remove_dir_all(&h.dir);
    }
}
