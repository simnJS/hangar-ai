use std::path::Path;
use std::time::{Duration, Instant};

use hangar_webview::{CloseReason, Header, PageLoadState};

use super::testing::Fake;
use super::*;
use crate::preview::model::{FileKind, model_url};
use crate::preview::store::{ShowFile, Store, What};

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
    windows.open(None, &pdf, Kind::Pdf, theme(), true);
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
    windows.open(Some(&fake.client), &page, Kind::Html, theme(), true);
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
    windows.open(Some(&fake.client), &pdf, Kind::Pdf, theme(), true);
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
    windows.open(Some(&fake.client), &pdf, Kind::Pdf, theme(), true);
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
    windows.open(Some(&fake.client), &page, Kind::Html, theme(), true);
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
    windows.open(Some(&fake.client), &video, Kind::Video, theme(), true);
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

/// The address a player's page goes to when its media fails or plays, as
/// `MEDIA_WATCH` writes it: the reason written like `encodeURIComponent`.
fn media_report(state: &str, reason: &str) -> String {
    let mut encoded = String::new();
    for byte in reason.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    format!("hangar-media:{state}?{encoded}")
}

/// A window of `kind` opened and told so; its name.
fn opened(windows: &mut Windows, fake: &Fake, entry: &Entry, kind: Kind, keyboard: bool) -> String {
    windows.open(Some(&fake.client), entry, kind, theme(), keyboard);
    let name = windows.get(&entry.id).unwrap().name.clone();
    windows.on_event(
        Some(&fake.client),
        &Event::Opened {
            window: name.clone(),
        },
    );
    name
}

/// The helper telling that a window's page went somewhere it may not.
fn refused(window: &str, url: &str) -> Event {
    Event::NavigationBlocked {
        window: window.into(),
        url: url.into(),
        new_window: false,
    }
}

fn agent(entry: &Entry) -> Entry {
    Entry {
        opened_by: Some("Ava".into()),
        ..entry.clone()
    }
}

#[test]
fn a_player_whose_page_reports_an_error_says_so_and_one_that_plays_does_not() {
    let video = entry(FileKind::Video, "/work/app/clip.mp4");
    let fake = Fake::new();
    let mut windows = Windows::default();
    let name = opened(&mut windows, &fake, &video, Kind::Video, true);

    let said = |windows: &mut Windows, url: &str| {
        let handled = windows.on_event(Some(&fake.client), &refused(&name, url));
        (windows.get(&video.id).unwrap().media_error.clone(), handled)
    };
    // An engine without the codec raises an error with no reason to give.
    let (error, handled) = said(&mut windows, &media_report("error", ""));
    assert_eq!(error, Some(String::new()));
    assert_eq!(handled, Handled::default(), "it is no link for the browser");
    assert_eq!(
        said(
            &mut windows,
            &media_report("error", "MEDIA_ERR_SRC_NOT_SUPPORTED")
        )
        .0,
        Some("MEDIA_ERR_SRC_NOT_SUPPORTED".into())
    );
    // The file plays after all (a reload that works).
    assert_eq!(said(&mut windows, &media_report("ready", "")).0, None);

    // What is not a report of the page's player changes nothing, and a
    // page that goes elsewhere is a link for the browser as before.
    assert_eq!(
        said(&mut windows, &media_report("error", "late")).0,
        Some("late".into())
    );
    for not_a_report in [
        "https://example.com/",
        "hangar-media:",
        "hangar-media:exploded?x",
        "hangar-media",
        &format!("hangar-media:error?{}", "x".repeat(5000)),
    ] {
        let (error, handled) = said(&mut windows, not_a_report);
        assert_eq!(error, Some("late".into()), "{not_a_report}");
        assert_eq!(handled.link.as_deref(), Some(not_a_report));
    }

    // A reason is kept to a line's length, and read as UTF-8 text.
    let (long, _) = said(&mut windows, &media_report("error", &"é".repeat(300)));
    assert_eq!(long.unwrap(), "é".repeat(200));
    assert_eq!(
        said(&mut windows, "hangar-media:error?100%25%20sure%").0,
        Some("100% sure%".into())
    );
    assert_eq!(
        said(&mut windows, "hangar-media:error?%FF%zz").0,
        Some("\u{fffd}%zz".into()),
        "bytes that are no text, and an escape that is none"
    );
}

#[test]
fn only_a_players_page_is_listened_to_for_its_media() {
    // A page of an agent's can go to any address it likes.
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::default();
    let name = opened(&mut windows, &fake, &page, Kind::Html, true);
    let forged = media_report("error", "forged");
    let handled = windows.on_event(Some(&fake.client), &refused(&name, &forged));
    assert_eq!(windows.get(&page.id).unwrap().media_error, None);
    assert_eq!(handled.link, Some(forged), "it is only a link refused");
    // A player's window that is sent to a window of its own is not a report
    // either.
    let video = entry(FileKind::Video, "/work/app/clip.mp4");
    let player = opened(&mut windows, &fake, &video, Kind::Video, true);
    let popup = Event::NavigationBlocked {
        window: player,
        url: media_report("error", "popup"),
        new_window: true,
    };
    windows.on_event(Some(&fake.client), &popup);
    assert_eq!(windows.get(&video.id).unwrap().media_error, None);

    // And no one is asked after a load: a player's page speaks for itself.
    let handled = windows.on_event(
        Some(&fake.client),
        &Event::PageLoad {
            window: name,
            state: PageLoadState::Finished,
            url: "x".into(),
        },
    );
    assert_eq!(handled, Handled::default());
    assert_eq!(fake.commands(2).len(), 2, "only the two opens were sent");
}

#[test]
fn a_players_window_carries_the_script_that_reports_its_media_and_no_other_does() {
    for (file, kind, watched) in [
        (FileKind::Video, Kind::Video, true),
        (FileKind::Audio, Kind::Audio, true),
        (FileKind::Pdf, Kind::Pdf, false),
        (FileKind::Html, Kind::Html, false),
    ] {
        let shown = entry(file, "/work/app/file");
        let command = open_command("preview-1", &shown, kind, theme());
        assert_eq!(
            command.init_script.as_deref(),
            watched.then_some(MEDIA_WATCH),
            "{kind:?}"
        );
    }
    // It reads the element's own events, caught on the way down, and goes to
    // the address `read_media_report` reads (a media document has no `ipc`).
    assert!(MEDIA_WATCH.contains("addEventListener('error'"));
    assert!(MEDIA_WATCH.contains("HTMLMediaElement"));
    assert!(MEDIA_WATCH.contains(&format!("location.href='{MEDIA_REPORT}'")));
    assert!(!MEDIA_WATCH.contains("ipc"));
}

/* ── a window follows what the person sees ─────────────────────── */

#[test]
fn the_entries_seen_are_the_ones_in_front_of_a_panel_on_screen_of_a_workspace_awake() {
    let first = entry(FileKind::Pdf, "/work/app/a.pdf");
    let second = entry(FileKind::Html, "/work/app/b.html");
    let third = entry(FileKind::Video, "/work/app/c.mp4");
    let front = [
        ("w1", &first, true),
        ("w2", &second, false),
        ("w3", &third, true),
    ];
    let seen = seen_entries(front, |ws| ws == "w3");
    // A panel that is not drawn, and a workspace asleep, show nothing.
    assert_eq!(
        seen.keys().copied().collect::<Vec<_>>(),
        [first.id.as_str()]
    );
    assert!(seen_entries([], |_| false).is_empty());
}

#[test]
fn a_window_goes_off_screen_when_its_entry_is_not_seen_and_comes_back_when_it_is() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    let name = opened(&mut windows, &fake, &page, Kind::Html, false);
    assert!(windows.get(&page.id).unwrap().is_shown());

    // The panel is hidden, or another tab is in front, or the workspace left:
    // nothing of the entry is seen.
    windows.set_seen(Some(&fake.client), &HashMap::new());
    assert!(!windows.get(&page.id).unwrap().is_shown());
    // Again, so nothing more is said.
    windows.set_seen(Some(&fake.client), &HashMap::new());

    // It is hidden, not closed: the page keeps its place.
    windows.set_seen(Some(&fake.client), &by_id(&page));
    assert!(windows.get(&page.id).unwrap().is_shown());
    assert_eq!(windows.state(&page.id), Some(&WinState::Open));

    let sent = fake.commands(4);
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert_eq!(sent[0]["type"], "open");
    assert_eq!(sent[1]["script"], PAUSE_MEDIA, "what plays stops first");
    assert_eq!(sent[2]["type"], "set_visible");
    assert_eq!(sent[2]["window"], name);
    assert_eq!(sent[2]["visible"], false);
    assert_eq!(sent[3]["type"], "set_visible");
    assert_eq!(sent[3]["visible"], true);
    // It comes back without asking for the keyboard.
    assert!(sent[3].get("focus").is_none(), "{:?}", sent[3]);
}

#[test]
fn a_hidden_panel_takes_its_windows_off_the_screen() {
    // The shell's word that a panel is not drawn (hidden, or the workspace
    // left the screen) reaches the windows as an empty `seen`.
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let video = entry(FileKind::Video, "/work/app/clip.mp4");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    for (shown, kind) in [(&pdf, Kind::Pdf), (&video, Kind::Video)] {
        let name = opened(&mut windows, &fake, shown, kind, false);
        if kind == Kind::Pdf {
            windows.on_event(
                Some(&fake.client),
                &Event::EvalResult {
                    window: name,
                    token: "pdf:x".into(),
                    result: Value::Bool(true),
                },
            );
        }
    }
    assert!(windows.get(&pdf.id).unwrap().is_shown());
    assert!(windows.get(&video.id).unwrap().is_shown());

    let front = [("w", &pdf, false), ("w", &video, false)];
    windows.set_seen(Some(&fake.client), &seen_entries(front, |_| false));
    assert!(!windows.get(&pdf.id).unwrap().is_shown());
    assert!(!windows.get(&video.id).unwrap().is_shown());
    assert_eq!(windows.state(&pdf.id), Some(&WinState::Open));
    assert_eq!(windows.state(&video.id), Some(&WinState::Open));

    // Each was hidden after its media was told to stop.
    let sent = fake.commands(7);
    let hidden = sent
        .iter()
        .filter(|command| command["type"] == "set_visible" && command["visible"] == false)
        .count();
    assert_eq!(hidden, 2, "{sent:?}");
    let pauses = sent
        .iter()
        .filter(|command| command["script"] == PAUSE_MEDIA)
        .count();
    assert_eq!(pauses, 2, "{sent:?}");
}

#[test]
fn a_window_made_while_its_entry_is_not_seen_opens_hidden() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    // It waited for the helper, and the tab was left in the meantime.
    windows.open(None, &page, Kind::Html, theme(), false);
    windows.set_seen(None, &HashMap::new());
    windows.flush(&fake.client, &by_id(&page), theme());
    let sent = fake.commands(1);
    assert_eq!(sent[0]["visible"], false);
    assert!(!windows.get(&page.id).unwrap().is_shown());

    // Its tab comes back before the window says it is open: it is shown then.
    let name = windows.get(&page.id).unwrap().name.clone();
    windows.set_seen(Some(&fake.client), &by_id(&page));
    assert_eq!(
        fake.commands(2).len(),
        1,
        "nothing is said to a window that is not open yet"
    );
    windows.on_event(Some(&fake.client), &Event::Opened { window: name });
    let sent = fake.commands(2);
    assert_eq!(sent[1]["type"], "set_visible");
    assert_eq!(sent[1]["visible"], true);
}

#[test]
fn a_player_that_goes_out_of_sight_is_paused_and_one_that_starts_unseen_is_too() {
    let video = entry(FileKind::Video, "/work/app/clip.mp4");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    let name = opened(&mut windows, &fake, &video, Kind::Video, false);
    windows.set_seen(Some(&fake.client), &HashMap::new());
    let sent = fake.commands(3);
    assert_eq!(sent[1]["script"], PAUSE_MEDIA);
    assert_eq!(sent[2]["visible"], false);

    // A media document may start playing as it loads, seen or not.
    windows.on_event(
        Some(&fake.client),
        &Event::PageLoad {
            window: name,
            state: PageLoadState::Finished,
            url: "x".into(),
        },
    );
    let sent = fake.commands(5);
    assert!(
        sent[3]["script"].as_str().unwrap().contains("m.loop="),
        "its loop and speed are applied"
    );
    assert_eq!(sent[4]["script"], PAUSE_MEDIA);
}

#[test]
fn an_error_of_a_command_that_raced_the_window_is_not_a_failure_of_the_window() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::default();
    let name = opened(&mut windows, &fake, &page, Kind::Html, true);
    let error = |code| Event::Error {
        window: Some(name.clone()),
        code,
        message: "m".into(),
    };
    windows.on_event(
        Some(&fake.client),
        &error(hangar_webview::ErrorCode::UnknownWindow),
    );
    windows.on_event(
        Some(&fake.client),
        &error(hangar_webview::ErrorCode::EvalFailed),
    );
    assert_eq!(windows.state(&page.id), Some(&WinState::Open));
    windows.on_event(
        Some(&fake.client),
        &error(hangar_webview::ErrorCode::OpenFailed),
    );
    assert_eq!(windows.state(&page.id), Some(&WinState::Failed("m".into())));
}

/* ── the keyboard ──────────────────────────────────────────────── */

#[test]
fn a_window_the_person_asked_for_asks_for_the_keyboard_and_one_an_agent_showed_does_not() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);

    assert_eq!(windows.arrival(&page), Arrival::Focused);
    assert_eq!(windows.arrival(&agent(&page)), Arrival::Unfocused);
    let due = windows.to_open(&by_id(&page));
    assert_eq!(due.len(), 1);
    assert!(due[0].1, "the person's entry may take the keyboard");
    let shown_by_agent = agent(&page);
    let due = windows.to_open(&by_id(&shown_by_agent));
    assert_eq!(due.len(), 1);
    assert!(!due[0].1, "an agent's may not");

    windows.open(Some(&fake.client), &page, Kind::Html, theme(), true);
    let sent = fake.commands(1);
    assert!(sent[0].get("focus").is_none(), "{:?}", sent[0]);
    assert!(sent[0].get("visible").is_none(), "{:?}", sent[0]);

    windows.open(
        Some(&fake.client),
        &shown_by_agent,
        Kind::Html,
        theme(),
        false,
    );
    let sent = fake.commands(2);
    assert_eq!(sent[1]["focus"], false);
    assert!(
        sent[1].get("visible").is_none(),
        "it is in front: {:?}",
        sent[1]
    );
}

#[test]
fn an_agents_file_is_not_opened_by_itself_where_a_mapped_window_takes_the_keyboard() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let by_agent = agent(&page);
    let wayland = Windows::with_unfocused_open(false);
    assert_eq!(wayland.arrival(&by_agent), Arrival::Held);
    assert!(
        wayland.to_open(&by_id(&by_agent)).is_empty(),
        "the panel offers the window, the person opens it"
    );
    // What the person opened is theirs, wherever they are.
    assert_eq!(wayland.arrival(&page), Arrival::Focused);
    assert_eq!(wayland.to_open(&by_id(&page)).len(), 1);

    for (agent_showed, unfocused_open, arrival) in [
        (false, true, Arrival::Focused),
        (false, false, Arrival::Focused),
        (true, true, Arrival::Unfocused),
        (true, false, Arrival::Held),
    ] {
        assert_eq!(Arrival::of(agent_showed, unfocused_open), arrival);
    }
}

#[test]
fn an_entry_with_a_window_or_one_tried_is_not_opened_again() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let mut windows = Windows::with_unfocused_open(true);
    windows.mark_tried(&page.id);
    assert!(windows.to_open(&by_id(&page)).is_empty());
    let other = entry(FileKind::Pdf, "/work/app/b.pdf");
    windows.open(None, &other, Kind::Pdf, theme(), true);
    assert!(windows.to_open(&by_id(&other)).is_empty());
}

#[test]
fn a_window_the_person_asked_for_on_a_session_that_would_give_it_the_keyboard_is_theirs() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let by_agent = agent(&page);
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(false);
    // Nothing opens by itself for the agent's file; the person pressed "Open
    // in a window" on its card, and the window asks for the keyboard.
    assert!(windows.to_open(&by_id(&by_agent)).is_empty());
    let name = opened(&mut windows, &fake, &by_agent, Kind::Html, true);
    assert!(windows.get(&page.id).unwrap().is_shown());
    let sent = fake.commands(1);
    assert!(sent[0].get("focus").is_none(), "{:?}", sent[0]);

    // It follows its tab like any window: off the screen with it, and back.
    windows.set_seen(Some(&fake.client), &HashMap::new());
    assert!(!windows.get(&page.id).unwrap().is_shown());
    // The card's button brings it forward, with the keyboard, hidden or not.
    windows.focus(&fake.client, &page.id);
    let sent = fake.commands(4);
    assert_eq!(sent[3]["type"], "set_visible");
    assert_eq!(sent[3]["visible"], true);
    assert_eq!(sent[3]["focus"], true);
    assert!(windows.get(&page.id).unwrap().is_shown());
    windows.focus(&fake.client, &page.id);
    let sent = fake.commands(5);
    assert_eq!(sent[4]["type"], "focus");
    assert_eq!(sent[4]["window"], name);
}

#[test]
fn a_pdf_the_person_opens_by_the_button_is_shown_once_the_engine_has_a_viewer() {
    // The agent's file waits for the click; the click is the person's, so the
    // window that waited hidden for the engine comes up with the keyboard.
    let pdf = agent(&entry(FileKind::Pdf, "/work/app/report.pdf"));
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(false);
    assert!(windows.to_open(&by_id(&pdf)).is_empty());
    let name = opened(&mut windows, &fake, &pdf, Kind::Pdf, true);
    assert!(
        !windows.get(&pdf.id).unwrap().is_shown(),
        "hidden until known"
    );
    windows.on_event(
        Some(&fake.client),
        &Event::EvalResult {
            window: name,
            token: "pdf:x".into(),
            result: Value::Bool(true),
        },
    );
    assert!(windows.get(&pdf.id).unwrap().is_shown());
    let sent = fake.commands(2);
    assert_eq!(sent[1]["type"], "set_visible");
    assert_eq!(sent[1]["focus"], true);
}

/// The window of a kit tab's page is put up by the same `Presence`: what it
/// opens with and where it stands afterwards.
#[test]
fn a_presence_opens_a_window_as_the_person_asked_and_keeps_it_where_the_tab_is() {
    let fake = Fake::new();
    let blank = || OpenWindow::new("kit-a", Content::html("<p>"));
    let flags = |presence: &Presence| {
        let line = serde_json::to_value(presence.opening(blank())).unwrap();
        (
            line.get("focus").and_then(Value::as_bool).unwrap_or(true),
            line.get("visible").and_then(Value::as_bool).unwrap_or(true),
        )
    };

    // (focus, visible) at the opening, for a tab the person asked for, for one
    // an agent opened, and for tabs that are behind or wait to be drawable.
    assert_eq!(flags(&Presence::new(true)), (true, true));
    assert_eq!(flags(&Presence::new(false)), (false, true));
    let mut behind = Presence::new(true);
    behind.set_seen(false);
    assert_eq!(flags(&behind), (false, false), "behind a tab, hidden");
    let waiting = Presence::new(true).until_drawable(false);
    assert_eq!(
        flags(&waiting),
        (false, false),
        "not known drawable, hidden"
    );

    // The window is on screen as it is opened, and the person's wish for the
    // keyboard is spent on that first showing.
    let mut presence = Presence::new(true);
    presence.opened();
    assert!(presence.is_shown());
    presence.set_seen(false);
    presence.place(&fake.client, "kit-a");
    assert!(!presence.is_shown());
    presence.set_seen(true);
    presence.place(&fake.client, "kit-a");
    let sent = fake.commands(4);
    assert_eq!(sent[0]["script"], PAUSE_MEDIA);
    assert_eq!(sent[1]["visible"], false);
    assert_eq!(sent[2]["visible"], true);
    assert!(
        sent[2].get("focus").is_none(),
        "it comes back for the tab, not for the person: {:?}",
        sent[2]
    );
    assert!(presence.is_shown());
    assert_eq!(sent.len(), 3, "{sent:?}");
}

#[test]
fn a_reload_loads_an_open_window_again_remakes_one_that_was_closed_and_reads_the_source() {
    let reload = |source, window: Option<WinState>| Reload::of(source, window.as_ref());
    assert_eq!(
        reload(false, Some(WinState::Open)),
        Reload {
            navigate: true,
            reopen: false,
            reread: false
        }
    );
    // In source mode the file is read again as well: the window behind is
    // loaded again too, it costs nothing and is what it shows next.
    assert_eq!(
        reload(true, Some(WinState::Open)),
        Reload {
            navigate: true,
            reopen: false,
            reread: true
        }
    );
    // A window the person closed, or that failed, is made again.
    for window in [WinState::Closed, WinState::Failed("x".into())] {
        assert_eq!(
            reload(false, Some(window)),
            Reload {
                navigate: false,
                reopen: true,
                reread: false
            }
        );
    }
    // Nothing is made for a window that is still on its way, that has no
    // helper to open in, or that was never opened (the source is still read).
    for window in [
        Some(WinState::Waiting),
        Some(WinState::Opening),
        Some(WinState::Missing),
        None,
    ] {
        let plan = reload(true, window);
        assert!(!plan.navigate && !plan.reopen && plan.reread);
    }
}

/* ── a PDF waits to be known drawable ──────────────────────────── */

#[test]
fn a_pdf_window_stays_hidden_until_the_engine_says_it_has_a_viewer() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    let name = opened(&mut windows, &fake, &pdf, Kind::Pdf, true);
    let sent = fake.commands(1);
    assert_eq!(sent[0]["visible"], false, "nothing flashes up first");
    assert!(!windows.get(&pdf.id).unwrap().is_shown());

    // The page loads, hidden, and the engine is asked.
    windows.on_event(
        Some(&fake.client),
        &Event::PageLoad {
            window: name.clone(),
            state: PageLoadState::Finished,
            url: "x".into(),
        },
    );
    let sent = fake.commands(2);
    assert_eq!(sent[1]["script"], PDF_CHECK);
    let token = sent[1]["token"].as_str().unwrap().to_string();
    windows.on_event(
        Some(&fake.client),
        &Event::EvalResult {
            window: name.clone(),
            token,
            result: Value::Bool(true),
        },
    );
    // Now it is shown, with the keyboard it was opened for.
    let sent = fake.commands(3);
    assert_eq!(sent[2]["type"], "set_visible");
    assert_eq!(sent[2]["visible"], true);
    assert_eq!(sent[2]["focus"], true);
    assert!(windows.get(&pdf.id).unwrap().is_shown());

    // The next PDF is not asked about: the engine is the same.
    let second = entry(FileKind::Pdf, "/work/app/other.pdf");
    let other = opened(&mut windows, &fake, &second, Kind::Pdf, false);
    let sent = fake.commands(4);
    assert!(
        sent[3].get("visible").is_none(),
        "shown at once: {:?}",
        sent[3]
    );
    windows.on_event(
        Some(&fake.client),
        &Event::PageLoad {
            window: other,
            state: PageLoadState::Finished,
            url: "x".into(),
        },
    );
    assert_eq!(fake.commands(5).len(), 4, "no second question");
}

#[test]
fn once_the_engine_is_known_not_to_draw_a_pdf_no_window_is_made_for_one() {
    let first = entry(FileKind::Pdf, "/work/app/report.pdf");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    let name = opened(&mut windows, &fake, &first, Kind::Pdf, true);
    windows.on_event(
        Some(&fake.client),
        &Event::EvalResult {
            window: name,
            token: "pdf:x".into(),
            result: Value::Bool(false),
        },
    );
    assert_eq!(windows.state(&first.id), Some(&WinState::NoPdfViewer));
    let sent = fake.commands(2);
    assert_eq!(sent[1]["type"], "close");

    let second = entry(FileKind::Pdf, "/work/app/other.pdf");
    windows.open(Some(&fake.client), &second, Kind::Pdf, theme(), true);
    assert_eq!(windows.state(&second.id), Some(&WinState::NoPdfViewer));
    assert_eq!(
        fake.commands(3).len(),
        2,
        "nothing is opened, so nothing flashes"
    );
    // Any other kind is still opened.
    let page = entry(FileKind::Html, "/work/app/index.html");
    windows.open(Some(&fake.client), &page, Kind::Html, theme(), true);
    assert_eq!(fake.commands(3).len(), 3);
}

#[test]
fn a_pdf_window_that_waited_is_shown_once_another_has_found_the_engine_drawing() {
    let first = entry(FileKind::Pdf, "/work/app/a.pdf");
    let second = entry(FileKind::Pdf, "/work/app/b.pdf");
    let fake = Fake::new();
    let mut windows = Windows::with_unfocused_open(true);
    let a = opened(&mut windows, &fake, &first, Kind::Pdf, false);
    opened(&mut windows, &fake, &second, Kind::Pdf, false);
    windows.on_event(
        Some(&fake.client),
        &Event::EvalResult {
            window: a,
            token: "pdf:x".into(),
            result: Value::Bool(true),
        },
    );
    assert!(windows.get(&first.id).unwrap().is_shown());
    assert!(windows.get(&second.id).unwrap().is_shown());
}

#[test]
fn a_link_the_window_was_stopped_from_following_is_handed_back() {
    let page = entry(FileKind::Html, "/work/app/index.html");
    let mut windows = Windows::default();
    windows.open(None, &page, Kind::Html, theme(), true);
    let blocked = windows.on_event(
        None,
        &Event::NavigationBlocked {
            window: format!("preview-{}", page.id),
            url: "https://example.com/".into(),
            new_window: true,
        },
    );
    assert_eq!(blocked.link.as_deref(), Some("https://example.com/"));
}

#[test]
fn a_lost_helper_closes_what_it_showed_and_a_failed_start_says_why() {
    let pdf = entry(FileKind::Pdf, "/work/app/report.pdf");
    let page = entry(FileKind::Html, "/work/app/index.html");
    let fake = Fake::new();
    let mut windows = Windows::default();
    windows.open(Some(&fake.client), &pdf, Kind::Pdf, theme(), true);
    windows.open(None, &page, Kind::Html, theme(), true);
    windows.fail_waiting(&StartError::Missing);
    assert_eq!(windows.state(&page.id), Some(&WinState::Missing));
    windows.open(None, &page, Kind::Html, theme(), true);
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

/// Where the helper asks for `file` of the folder `dir`: the address the
/// platform builds for the folder's real path (`model_url`), as it does for
/// every file the preview shows. Windows' `canonicalize` puts `\\?\` before
/// the path, which no address can carry; hangar-core hands the app its paths
/// without it, so it is taken off here too. Pasting the path into a
/// `hangar-model://localhost…` address by hand would be no address at all on
/// Windows, where the path has a drive and backslashes.
fn address(dir: &Path, file: &str) -> String {
    let real = dir.canonicalize().unwrap().to_string_lossy().into_owned();
    let plain = real.strip_prefix(r"\\?\").unwrap_or(&real);
    model_url(&format!("{plain}/{file}"))
}

#[tokio::test]
async fn serves_a_page_from_the_folders_the_preview_may_read_and_nothing_else() {
    let _turn = crate::preview::tests::ROOTS.lock().await;
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

    let page = answer(&request(&address(&dir, "index.html"), &[]));
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
        &address(&dir, "notes.txt"),
        &[("Range", "bytes=2-5")],
    ));
    assert_eq!(range.status, 206);
    assert_eq!(range.body, b"2345");
    assert_eq!(header(&range, "content-range"), Some("bytes 2-5/10"));

    let refused = answer(&request(&address(&outside, "secret.txt"), &[]));
    assert!(refused.status >= 400, "{}", refused.status);
    assert!(refused.body != b"no");

    // A page's own `fetch` carries an Origin, and is refused.
    let fetched = answer(&request(&address(&dir, "notes.txt"), &[("Origin", "null")]));
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

/// The whole way a file takes to the helper's window and back, with the real
/// helper: `cargo build -p hangar-webview --features engine`, then
/// `HANGAR_WEBVIEW=target/debug/hangar-webview cargo test -p hangar-app
/// an_html_page_runs -- --ignored --nocapture` on a machine with a display.
#[tokio::test]
#[ignore = "needs the helper built with --features engine, and a display"]
async fn an_html_page_a_pdf_and_a_video_load_through_the_scheme_in_the_real_helper() {
    let _turn = crate::preview::tests::ROOTS.lock().await;
    let dir = std::env::temp_dir().join(format!("hangar-webview-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let page = dir.join("page.html");
    std::fs::write(
        &page,
        "<!doctype html><title>start</title><body><script>document.title='ran'</script>",
    )
    .unwrap();
    let pdf = dir.join("doc.pdf");
    std::fs::write(
        &pdf,
        std::fs::read(std::env::var("HANGAR_TEST_PDF").unwrap()).unwrap(),
    )
    .unwrap();
    hangar_core::model::model_allow_roots(vec![dir.to_string_lossy().into_owned()])
        .await
        .unwrap();

    let helper = hangar_webview::find_helper().expect("HANGAR_WEBVIEW names the helper");
    let (client, events) = Client::spawn(helper).expect("the helper starts");
    let client = Arc::new(client);
    let html = entry(FileKind::Html, &page.to_string_lossy());
    let document = entry(FileKind::Pdf, &pdf.to_string_lossy());
    client
        .open(open_command("preview-html", &html, Kind::Html, theme()))
        .unwrap();
    client
        .open(open_command("preview-pdf", &document, Kind::Pdf, theme()))
        .unwrap();

    let mut title = None;
    let mut viewer = None;
    let mut served = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(40);
    while (title.is_none() || viewer.is_none()) && Instant::now() < deadline {
        let Ok(event) = events.recv_timeout(Duration::from_secs(2)) else {
            continue;
        };
        match &event {
            Event::SchemeRequest(request) => {
                let response = answer(request);
                served.push((request.uri.clone(), response.status));
                client.respond(response).unwrap();
            }
            Event::PageLoad {
                window,
                state: PageLoadState::Finished,
                ..
            } if window == "preview-html" => {
                client
                    .eval_with_token(window, "document.title", "html")
                    .unwrap();
            }
            Event::PageLoad {
                window,
                state: PageLoadState::Finished,
                ..
            } if window == "preview-pdf" => {
                client
                    .eval_with_token(window, "navigator.pdfViewerEnabled", "pdf")
                    .unwrap();
            }
            Event::EvalResult { token, result, .. } if token == "html" => {
                title = Some(result.clone())
            }
            Event::EvalResult { token, result, .. } if token == "pdf" => {
                viewer = Some(result.clone())
            }
            other => eprintln!("event: {other:?}"),
        }
    }
    eprintln!("served: {served:#?}");
    eprintln!("title: {title:?}, pdf viewer: {viewer:?}");
    assert_eq!(
        title,
        Some(Value::String("ran".into())),
        "the page's script ran"
    );
    assert!(served.iter().all(|(_, status)| *status < 400), "{served:?}");
    assert!(viewer.is_some(), "the engine answered about its PDF viewer");
    let _ = std::fs::remove_dir_all(&dir);
}

/// And a video: the player asks for byte ranges, which `model::serve` answers
/// in chunks (`HANGAR_TEST_VIDEO` names an MP4).
#[tokio::test]
#[ignore = "needs the helper built with --features engine, and a display"]
async fn a_video_is_played_from_ranges_in_the_real_helper() {
    let _turn = crate::preview::tests::ROOTS.lock().await;
    let dir = std::env::temp_dir().join(format!("hangar-webview-video-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let clip = dir.join("clip.mp4");
    std::fs::write(
        &clip,
        std::fs::read(std::env::var("HANGAR_TEST_VIDEO").unwrap()).unwrap(),
    )
    .unwrap();
    hangar_core::model::model_allow_roots(vec![dir.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let helper = hangar_webview::find_helper().expect("HANGAR_WEBVIEW names the helper");
    let (client, events) = Client::spawn(helper).expect("the helper starts");
    let video = entry(FileKind::Video, &clip.to_string_lossy());
    client
        .open(open_command("preview-video", &video, Kind::Video, theme()))
        .unwrap();
    let mut served = Vec::new();
    let mut state = None;
    let started = Instant::now();
    let mut asked = false;
    while state.is_none() && started.elapsed() < Duration::from_secs(30) {
        let Ok(event) = events.recv_timeout(Duration::from_millis(500)) else {
            if started.elapsed() > Duration::from_secs(3) && !asked {
                asked = true;
                let script = "(()=>{const m=document.querySelector('video');\
                              return m?[m.readyState,m.duration,m.error?m.error.message:null]:null})()";
                client
                    .eval_with_token("preview-video", script, "video")
                    .unwrap();
            }
            continue;
        };
        match &event {
            Event::SchemeRequest(request) => {
                let response = answer(request);
                let range = request
                    .headers
                    .iter()
                    .find(|header| header.name.eq_ignore_ascii_case("range"))
                    .map(|header| header.value.clone());
                served.push((range, response.status, response.body.len()));
                client.respond(response).unwrap();
            }
            Event::EvalResult { token, result, .. } if token == "video" => {
                state = Some(result.clone())
            }
            other => eprintln!("event: {other:?}"),
        }
    }
    eprintln!("served: {served:?}");
    eprintln!("video: {state:?}");
    assert!(!served.is_empty());
    assert!(
        served.iter().all(|(_, status, _)| *status < 400),
        "{served:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the windows do with the real helper, end to end through `Windows`:
/// a PDF opens hidden and is shown once the engine says it has a viewer, a
/// broken video reports its error from the page's own event, a sound that
/// plays is paused when its entry leaves the screen, and the window comes back.
///
/// `cargo build -p hangar-webview --features engine`, then
/// `HANGAR_WEBVIEW=target/debug/hangar-webview HANGAR_TEST_PDF=<a pdf>
/// HANGAR_TEST_AUDIO=<a wav> HANGAR_TEST_VIDEO=<an mp4> cargo test -p
/// hangar-app windows_follow_ -- --ignored --nocapture` on a machine with a
/// display (`HANGAR_TEST_TRACE=1` prints the helper's events).
#[tokio::test]
#[ignore = "needs the helper built with --features engine, and a display"]
async fn windows_follow_what_is_seen_in_the_real_helper() {
    let _turn = crate::preview::tests::ROOTS.lock().await;
    let dir = std::env::temp_dir().join(format!("hangar-webview-follow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let copy = |variable: &str, name: &str| {
        let to = dir.join(name);
        std::fs::write(
            &to,
            std::fs::read(std::env::var(variable).unwrap()).unwrap(),
        )
        .unwrap();
        to.to_string_lossy().into_owned()
    };
    let pdf = entry(FileKind::Pdf, &copy("HANGAR_TEST_PDF", "doc.pdf"));
    let sound = entry(FileKind::Audio, &copy("HANGAR_TEST_AUDIO", "tone.wav"));
    // A video the engine cannot play: the head of a real one, so that it is
    // still served as a video (its header is checked), and then runs out.
    let broken = dir.join("broken.mp4");
    let video = std::fs::read(std::env::var("HANGAR_TEST_VIDEO").unwrap()).unwrap();
    std::fs::write(&broken, &video[..video.len().min(600)]).unwrap();
    let broken = entry(FileKind::Video, &broken.to_string_lossy());
    hangar_core::model::model_allow_roots(vec![dir.to_string_lossy().into_owned()])
        .await
        .unwrap();

    let helper = hangar_webview::find_helper().expect("HANGAR_WEBVIEW names the helper");
    let (client, events) = Client::spawn(helper).expect("the helper starts");
    let mut windows = Windows::with_unfocused_open(true);

    // Reads the helper's events into `windows` until `done` holds.
    let said: std::cell::RefCell<HashMap<String, Value>> = Default::default();
    let drive = |windows: &mut Windows,
                 done: &dyn Fn(&Windows, &HashMap<String, Value>) -> bool| {
        let until = Instant::now() + Duration::from_secs(30);
        while !done(windows, &said.borrow()) {
            assert!(Instant::now() < until, "gave up: {windows:?}");
            let Ok(event) = events.recv_timeout(Duration::from_millis(500)) else {
                continue;
            };
            match &event {
                Event::SchemeRequest(request) => {
                    let response = answer(request);
                    if std::env::var_os("HANGAR_TEST_TRACE").is_some() {
                        eprintln!(
                            "served {} -> {} ({} bytes)",
                            request.uri,
                            response.status,
                            response.body.len()
                        );
                    }
                    client.respond(response).unwrap();
                }
                Event::EvalResult { token, result, .. } if !token.starts_with("pdf:") => {
                    said.borrow_mut().insert(token.clone(), result.clone());
                }
                _ => {}
            }
            if std::env::var_os("HANGAR_TEST_TRACE").is_some() {
                eprintln!("event: {event:?}");
            }
            windows.on_event(Some(&client), &event);
        }
    };

    // The PDF: made hidden, shown when the engine has said it draws PDFs.
    windows.open(Some(&client), &pdf, Kind::Pdf, theme(), false);
    assert!(!windows.get(&pdf.id).unwrap().is_shown(), "it waits hidden");
    drive(&mut windows, &|windows, _| {
        windows.get(&pdf.id).is_some_and(|win| win.is_shown())
            || windows.state(&pdf.id) == Some(&WinState::NoPdfViewer)
    });
    eprintln!("pdf: {:?}", windows.state(&pdf.id));
    assert!(windows.get(&pdf.id).unwrap().is_shown());

    // A video that is not one says so from the page.
    windows.open(Some(&client), &broken, Kind::Video, theme(), false);
    drive(&mut windows, &|windows, _| {
        windows
            .get(&broken.id)
            .is_some_and(|win| win.media_error.is_some())
    });
    eprintln!("broken: {:?}", windows.get(&broken.id).unwrap().media_error);

    // The sound: out of sight, its window is hidden and still answers; and
    // where the engine plays what the scheme serves (WebKitGTK 2.52.6 does
    // not: it says so, from the page, as the video above did), it is paused.
    windows.open(Some(&client), &sound, Kind::Audio, theme(), false);
    let name = windows.get(&sound.id).unwrap().name.clone();
    drive(&mut windows, &|windows, _| {
        windows.state(&sound.id) == Some(&WinState::Open)
    });
    // Long enough for an engine that cannot play it to say so.
    let settled = Instant::now() + Duration::from_millis(2500);
    drive(&mut windows, &|_, _| Instant::now() >= settled);
    let playable = windows.get(&sound.id).unwrap().media_error.is_none();
    eprintln!("sound: playable through the scheme: {playable}");
    let paused = "document.querySelector('video,audio').paused";
    if playable {
        // Muted, so that no policy of the engine refuses to play it unasked.
        let play = "(()=>{const m=document.querySelector('video,audio');m.muted=true;m.play();return 1})()";
        client.eval_with_token(&name, play, "play").unwrap();
        drive(&mut windows, &|_, said| said.contains_key("play"));
        std::thread::sleep(Duration::from_millis(800));
        client.eval_with_token(&name, paused, "before").unwrap();
        drive(&mut windows, &|_, said| said.contains_key("before"));
        assert_eq!(said.borrow()["before"], Value::Bool(false), "it plays");
    }

    windows.set_seen(Some(&client), &HashMap::new());
    assert!(!windows.get(&sound.id).unwrap().is_shown());
    if playable {
        client.eval_with_token(&name, paused, "after").unwrap();
        drive(&mut windows, &|_, said| said.contains_key("after"));
        assert_eq!(
            said.borrow()["after"],
            Value::Bool(true),
            "hidden, it was paused"
        );
    }

    // And it comes back, the same page, still answering.
    windows.set_seen(Some(&client), &by_id(&sound));
    assert!(windows.get(&sound.id).unwrap().is_shown());
    client
        .eval_with_token(&name, "document.readyState", "back")
        .unwrap();
    drive(&mut windows, &|_, said| said.contains_key("back"));
    assert_eq!(windows.state(&sound.id), Some(&WinState::Open));
    if playable {
        client.eval_with_token(&name, paused, "kept").unwrap();
        drive(&mut windows, &|_, said| said.contains_key("kept"));
        assert_eq!(said.borrow()["kept"], Value::Bool(true), "it stays paused");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
