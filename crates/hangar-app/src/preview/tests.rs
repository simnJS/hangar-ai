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
use super::image::Bytes;
use super::image_stage::Measured;
use super::model::{self, FileKind, Size as FrameSize};
use super::store::Status;
use super::text::Load;
use super::viewer::{Coloured, Loaded, ViewerMsg};
use super::webview::{StartError, WebMsg, WinState};
use super::{Env, Live, Msg, State, link, reconcile, text, viewer_update};
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::services::inbox;
use crate::services::preview::PreviewRequest;
use crate::shell::PanelMode;
use crate::state::{AppState, Model, Pane, Workspace};

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
    let dir = std::env::temp_dir().join(format!(
        "hangar-preview-{tag}-{}-{}",
        std::process::id(),
        uuid_like()
    ));
    std::fs::create_dir_all(&dir).expect("a folder");
    dir.canonicalize().expect("a canonical folder")
}

fn uuid_like() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

impl Harness {
    fn new(tag: &str) -> Harness {
        let dir = scratch(tag);
        let shared = Shared::new(None);
        let mut model = Model::default();
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
        self.viewer(id, ViewerMsg::Text(Ok(Loaded(Arc::new(head)))));
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
        let bytes: Bytes = super::image::read(Path::new(&path)).expect("readable");
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
async fn a_file_shown_quietly_waits_without_opening_a_window_or_the_panel() {
    let mut h = Harness::new("quiet");
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
    let scope = h.model.live.preview.scope(WORKSPACE).unwrap();
    assert!(!scope.panel_open);
    assert!(scope.unseen);
    assert!(h.st.windows.get(&id).is_none(), "nobody looks at it yet");
}

#[tokio::test]
async fn closing_a_tab_lets_go_of_its_viewer_and_its_window() {
    let mut h = Harness::new("close");
    let notes = h.write("notes.txt", "plain\n");
    let page = h.write("page.html", "<p>hi</p>");
    let first = h.show(&notes).await;
    let second = h.show(&page).await;
    h.read_text(&first);
    assert!(h.st.viewers.contains_key(&first));
    assert!(h.st.windows.get(&second).is_some());
    h.send(Msg::Close(WORKSPACE.into(), first.clone()));
    h.send(Msg::Close(WORKSPACE.into(), second.clone()));
    assert!(h.st.viewers.is_empty());
    assert!(h.st.windows.get(&second).is_none());
    assert!(!h.model.live.preview.has_preview(WORKSPACE));
}

#[tokio::test]
async fn the_unseen_dot_goes_out_once_the_panel_is_on_screen() {
    let mut h = Harness::new("seen");
    let path = h.write("notes.txt", "plain\n");
    h.show(&path).await;
    h.tick();
    // The panel is open in the terminals view: it is on screen.
    assert!(!h.model.live.preview.unseen(WORKSPACE));
    h.send(Msg::SetPanel(WORKSPACE.into(), Some(false), None));
    h.shared
        .edit(|store| store.reload_model(&only_entry(&h), 5, None));
    h.tick();
    assert!(h.model.live.preview.unseen(WORKSPACE), "closed, it is news");
    h.send(Msg::SetPanel(WORKSPACE.into(), Some(true), None));
    assert!(!h.model.live.preview.unseen(WORKSPACE));
}

#[tokio::test]
async fn the_keys_of_the_strip_move_between_tabs() {
    let mut h = Harness::new("keys");
    let one = h.show(&h.write("one.txt", "1")).await;
    let two = h.show(&h.write("two.txt", "2")).await;
    assert_eq!(only_entry(&h), two);
    h.send(Msg::Tab(super::tabs::TabKey::Next));
    assert_eq!(only_entry(&h), one);
    h.send(Msg::Tab(super::tabs::TabKey::Last));
    assert_eq!(only_entry(&h), two);
    h.send(Msg::Tab(super::tabs::TabKey::CloseActive));
    assert_eq!(only_entry(&h), one);
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
async fn the_panel_draws_every_viewer_and_a_tab_press_asks_to_bring_it_forward() {
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
        for mode in [
            PanelMode::Docked { width: 440.0 },
            PanelMode::Drawer { width: 440.0 },
            PanelMode::Expanded,
        ] {
            let ctx = h.ctx(window, &pkg, locale, "graphite");
            let view = super::view_panel(&h.st, ctx, mode);
            let mut ui = simulate(view, Size::new(900.0, 600.0));
            let _ = ui.snapshot(&to_iced(ctx.theme)).expect("it draws");
        }
    }
    // Pressing the tab of the first file asks for it.
    let ctx = h.ctx(window, &pkg, Locale::En, "graphite");
    let mut ui = simulate(
        super::view_panel(&h.st, ctx, PanelMode::Docked { width: 440.0 }),
        Size::new(900.0, 600.0),
    );
    ui.click("notes.txt").expect("the tab is there to press");
    let messages: Vec<Message> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|message| matches!(
            message,
            Message::Preview(Msg::Activate(ws, id)) if ws == WORKSPACE && *id == first
        )),
        "{messages:?}"
    );
    let _ = second;
}

#[tokio::test]
async fn the_panel_of_a_workspace_with_nothing_open_says_so() {
    let h = Harness::new("empty");
    let pkg = Packages::default();
    let ctx = h.ctx(window::Id::unique(), &pkg, Locale::En, "graphite");
    let view = super::view_panel(&h.st, ctx, PanelMode::Docked { width: 440.0 });
    let mut ui = simulate(view, Size::new(900.0, 600.0));
    assert!(
        ui.find(hangar_i18n::t(Locale::En, Key::ModelEmpty).as_ref())
            .is_ok()
    );
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
    let view = super::view_panel(&h.st, ctx, PanelMode::Docked { width: 480.0 });
    let mut ui = simulate(view, Size::new(480.0, 620.0));
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
                let view = super::view_panel(&h.st, ctx, PanelMode::Docked { width: 480.0 });
                let mut ui = simulate(view, Size::new(480.0, 620.0));
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
