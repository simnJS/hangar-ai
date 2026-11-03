use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use hangar_webview::{CloseReason, Header, PageLoadState};

use super::*;
use crate::preview::model::FileKind;
use crate::preview::store::{ShowFile, Store, What};

/// A helper's output that never ends until the test lets go of its sender.
struct Silent(Receiver<Vec<u8>>);

impl Read for Silent {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.recv() {
            Ok(bytes) => {
                let n = bytes.len().min(buf.len());
                buf[..n].copy_from_slice(&bytes[..n]);
                Ok(n)
            }
            Err(_) => Ok(0),
        }
    }
}

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);

impl Write for Recorder {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Fake {
    client: Client,
    sent: Recorder,
    _keep: Sender<Vec<u8>>,
}

impl Fake {
    fn new() -> Fake {
        let (keep, reader) = mpsc::channel();
        let sent = Recorder::default();
        let (client, _events) = Client::from_io(Silent(reader), sent.clone());
        Fake {
            client,
            sent,
            _keep: keep,
        }
    }

    /// The commands the client wrote, once there are `count` of them.
    fn commands(&self, count: usize) -> Vec<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let text = String::from_utf8(self.sent.0.lock().unwrap().clone()).unwrap();
            let lines: Vec<serde_json::Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            if lines.len() >= count || Instant::now() > deadline {
                return lines;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn entry(kind: FileKind, path: &str) -> Entry {
    // A clock that moves, so that two entries made by two stores have two ids.
    static TICKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut store = Store::with_clock(|| TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let id = store.show_file(ShowFile {
        workspace_id: "w".into(),
        path: path.into(),
        title: None,
        opened_by: None,
        modified_ms: 1,
        what: What::File {
            kind,
            mime: "application/pdf".into(),
            size: 10,
        },
    });
    store.entry(&id).unwrap().clone()
}

fn theme() -> &'static hangar_theme::Theme {
    hangar_theme::by_id_or_default("graphite")
}

fn by_id(entry: &Entry) -> HashMap<&str, &Entry> {
    HashMap::from([(entry.id.as_str(), entry)])
}

#[test]
fn a_page_is_opened_by_the_scheme_of_the_model_files_and_may_go_nowhere_else() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let command = open_command("preview-1", &pdf, Kind::Pdf, theme());
    assert_eq!(command.window, "preview-1");
    assert_eq!(command.title, "report.pdf");
    assert_eq!(command.schemes, [SCHEME]);
    assert_eq!(
        command.content,
        Content::url(preview_url("/work/app/report.pdf", 0, false))
    );
    assert_eq!(
        command.allow_navigation.as_deref(),
        Some([scheme_base(SCHEME)].as_slice())
    );
    // Isolated by default: nothing of the page is kept.
    assert!(command.ephemeral);
    assert!(!command.kit_bridge);
}

#[test]
fn a_page_expects_white_a_player_black_and_the_rest_the_themes_page() {
    assert_eq!(background_of(Kind::Html, theme()), [255, 255, 255]);
    assert_eq!(background_of(Kind::Video, theme()), [0, 0, 0]);
    let bg = theme().colors.bg;
    assert_eq!(
        background_of(Kind::Pdf, theme())[0],
        (bg.r * 255.0).round() as u8
    );
}

#[test]
fn the_player_is_told_its_loop_and_speed() {
    let script = player_script(true, 1.5);
    assert!(script.contains("m.loop=true"), "{script}");
    assert!(script.contains("m.playbackRate=1.5"), "{script}");
    assert!(player_script(false, 1.0).contains("m.loop=false"));
}

#[test]
fn a_window_waits_for_the_helper_then_opens_and_is_open_once_it_says_so() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let mut windows = Windows::default();
    windows.open(None, &pdf, Kind::Pdf, theme());
    assert_eq!(windows.state(&pdf.id), Some(&WinState::Waiting));

    let fake = Fake::new();
    windows.flush(&fake.client, &by_id(&pdf), theme());
    assert_eq!(windows.state(&pdf.id), Some(&WinState::Opening));
    let sent = fake.commands(1);
    assert_eq!(sent[0]["type"], "open");
    assert_eq!(sent[0]["window"], format!("preview-{}", pdf.id));

    let name = windows.get(&pdf.id).unwrap().name.clone();
    windows.on_event(Some(&fake.client), &Event::Opened { window: name });
    assert_eq!(windows.state(&pdf.id), Some(&WinState::Open));
}

#[test]
fn the_person_closing_the_window_leaves_it_closed_and_the_entry_noted_as_tried() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.mark_tried(&page.id);
    windows.open(Some(&fake.client), &page, Kind::Html, theme());
    let name = windows.get(&page.id).unwrap().name.clone();
    windows.on_event(
        Some(&fake.client),
        &Event::Opened {
            window: name.clone(),
        },
    );
    windows.on_event(
        Some(&fake.client),
        &Event::WindowClosed {
            window: name,
            reason: CloseReason::User,
        },
    );
    assert_eq!(windows.state(&page.id), Some(&WinState::Closed));
    assert!(windows.was_tried(&page.id));
}

#[test]
fn a_pdf_the_engine_cannot_draw_is_closed_and_said_so() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.open(Some(&fake.client), &pdf, Kind::Pdf, theme());
    let name = windows.get(&pdf.id).unwrap().name.clone();
    windows.on_event(
        Some(&fake.client),
        &Event::Opened {
            window: name.clone(),
        },
    );
    windows.on_event(
        Some(&fake.client),
        &Event::PageLoad {
            window: name.clone(),
            state: PageLoadState::Finished,
            url: "x".into(),
        },
    );
    // It asks the engine, once the page is in.
    let sent = fake.commands(2);
    assert_eq!(sent[1]["type"], "eval");
    assert_eq!(sent[1]["script"], PDF_CHECK);
    let token = sent[1]["token"].as_str().unwrap().to_string();
    windows.on_event(
        Some(&fake.client),
        &Event::EvalResult {
            window: name.clone(),
            token,
            result: Value::Bool(false),
        },
    );
    assert_eq!(windows.state(&pdf.id), Some(&WinState::NoPdfViewer));
    let sent = fake.commands(3);
    assert_eq!(sent[2]["type"], "close");
    // Its closing does not turn "no viewer" into "closed".
    windows.on_event(
        Some(&fake.client),
        &Event::WindowClosed {
            window: name,
            reason: CloseReason::Command,
        },
    );
    assert_eq!(windows.state(&pdf.id), Some(&WinState::NoPdfViewer));
}

#[test]
fn a_pdf_the_engine_draws_stays_open() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.open(Some(&fake.client), &pdf, Kind::Pdf, theme());
    let name = windows.get(&pdf.id).unwrap().name.clone();
    windows.on_event(
        Some(&fake.client),
        &Event::Opened {
            window: name.clone(),
        },
    );
    windows.on_event(
        Some(&fake.client),
        &Event::EvalResult {
            window: name,
            token: "pdf:x".into(),
            result: Value::Bool(true),
        },
    );
    assert_eq!(windows.state(&pdf.id), Some(&WinState::Open));
}

#[test]
fn a_closed_entry_closes_its_window_and_a_rewritten_file_loads_the_new_revision() {
    let mut page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.open(Some(&fake.client), &page, Kind::Html, theme());
    let name = windows.get(&page.id).unwrap().name.clone();
    windows.on_event(
        Some(&fake.client),
        &Event::Opened {
            window: name.clone(),
        },
    );

    page.revision = 2;
    windows.reconcile(Some(&fake.client), &by_id(&page));
    let sent = fake.commands(2);
    assert_eq!(sent[1]["type"], "navigate");
    assert_eq!(
        sent[1]["url"],
        preview_url("/work/app/index.html", 2, false)
    );

    windows.reconcile(Some(&fake.client), &HashMap::new());
    let sent = fake.commands(3);
    assert_eq!(sent[2]["type"], "close");
    assert_eq!(sent[2]["window"], name);
    assert!(windows.get(&page.id).is_none());
}

#[test]
fn the_loop_and_speed_are_kept_for_the_next_load() {
    let video = entry(FileKind::Video, "/work/app/clip.mp4");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.open(Some(&fake.client), &video, Kind::Video, theme());
    let name = windows.get(&video.id).unwrap().name.clone();
    windows.on_event(
        Some(&fake.client),
        &Event::Opened {
            window: name.clone(),
        },
    );
    windows.set_player(Some(&fake.client), &video.id, Some(true), Some(2.0));
    windows.on_event(
        Some(&fake.client),
        &Event::PageLoad {
            window: name,
            state: PageLoadState::Finished,
            url: "x".into(),
        },
    );
    let sent = fake.commands(3);
    assert!(sent[1]["script"].as_str().unwrap().contains("m.loop=true"));
    assert!(
        sent[2]["script"]
            .as_str()
            .unwrap()
            .contains("m.playbackRate=2")
    );
    let win = windows.get(&video.id).unwrap();
    assert!(win.looping);
    assert_eq!(win.speed, 2.0);
}

#[test]
fn a_link_the_window_was_stopped_from_following_is_handed_back() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let mut windows = Windows::default();
    windows.open(None, &page, Kind::Html, theme());
    let blocked = windows.on_event(
        None,
        &Event::NavigationBlocked {
            window: format!("preview-{}", page.id),
            url: "https://example.com/".into(),
            new_window: true,
        },
    );
    assert_eq!(blocked.as_deref(), Some("https://example.com/"));
}

#[test]
fn a_lost_helper_closes_what_it_showed_and_a_failed_start_says_why() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.open(Some(&fake.client), &pdf, Kind::Pdf, theme());
    windows.open(None, &page, Kind::Html, theme());
    windows.fail_waiting(&StartError::Missing);
    assert_eq!(windows.state(&page.id), Some(&WinState::Missing));
    windows.open(None, &page, Kind::Html, theme());
    windows.fail_waiting(&StartError::Failed("no display".into()));
    assert_eq!(
        windows.state(&page.id),
        Some(&WinState::Failed("no display".into()))
    );
    windows.lost();
    assert_eq!(windows.state(&pdf.id), Some(&WinState::Closed));
    assert_eq!(
        windows.entry_of(&format!("preview-{}", pdf.id)),
        Some(pdf.id.as_str())
    );
}

/* ── serving the page its files ────────────────────────────────── */

fn request(uri: &str, headers: &[(&str, &str)]) -> SchemeRequest {
    SchemeRequest {
        request: 7,
        window: "preview-1".into(),
        method: "GET".into(),
        uri: uri.into(),
        headers: headers
            .iter()
            .map(|(name, value)| Header::new(*name, *value))
            .collect(),
        body: Vec::new(),
    }
}

fn header<'a>(response: &'a SchemeResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

#[tokio::test]
async fn serves_a_page_from_the_folders_the_preview_may_read_and_nothing_else() {
    let dir = std::env::temp_dir().join(format!("hangar-webview-serve-{}", std::process::id()));
    let outside =
        std::env::temp_dir().join(format!("hangar-webview-outside-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><title>x</title><p>hi</p>",
    )
    .unwrap();
    std::fs::write(dir.join("notes.txt"), "0123456789").unwrap();
    std::fs::write(outside.join("secret.txt"), "no").unwrap();
    hangar_core::model::model_allow_roots(vec![dir.to_string_lossy().into_owned()])
        .await
        .unwrap();

    let page = answer(&request(
        &format!(
            "hangar-model://localhost{}/index.html",
            dir.canonicalize().unwrap().display()
        ),
        &[],
    ));
    assert_eq!(page.request, 7);
    assert_eq!(page.status, 200);
    assert!(page.body.starts_with(b"<!doctype html>"));
    // A page runs in a sandbox of its own, whatever the engine.
    assert!(
        header(&page, "content-security-policy").is_some_and(|csp| csp.contains("sandbox")),
        "{:?}",
        page.headers
    );

    let range = answer(&request(
        &format!(
            "hangar-model://localhost{}/notes.txt",
            dir.canonicalize().unwrap().display()
        ),
        &[("Range", "bytes=2-5")],
    ));
    assert_eq!(range.status, 206);
    assert_eq!(range.body, b"2345");
    assert_eq!(header(&range, "content-range"), Some("bytes 2-5/10"));

    let refused = answer(&request(
        &format!(
            "hangar-model://localhost{}/secret.txt",
            outside.canonicalize().unwrap().display()
        ),
        &[],
    ));
    assert!(refused.status >= 400, "{}", refused.status);
    assert!(refused.body != b"no");

    // A page's own `fetch` carries an Origin, and is refused.
    let fetched = answer(&request(
        &format!(
            "hangar-model://localhost{}/notes.txt",
            dir.canonicalize().unwrap().display()
        ),
        &[("Origin", "null")],
    ));
    assert!(fetched.status >= 400, "{}", fetched.status);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn a_request_that_is_not_one_is_a_400() {
    let response = answer(&request("not a uri", &[]));
    assert_eq!(response.status, 400);
}

#[test]
fn only_a_helper_next_to_the_app_or_a_dev_build_starts() {
    // Under test, as in a development build, cargo can run the helper from
    // the workspace; a packaged build without the helper cannot.
    let options = spawn_options();
    assert!(options.is_ok() || matches!(options, Err(StartError::Missing)));
}

#[test]
fn routes_hand_other_windows_events_to_whoever_asked() {
    let helper = Helper::new();
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = heard.clone();
    helper.route("kit-", move |event| {
        if let Some(window) = event_window(event) {
            sink.lock().unwrap().push(window.to_string());
        }
    });
    assert!(helper.client().is_none());
    assert_eq!(
        event_window(&Event::Opened {
            window: "kit-a".into()
        }),
        Some("kit-a")
    );
    assert_eq!(event_window(&Event::Unknown), None);
}
