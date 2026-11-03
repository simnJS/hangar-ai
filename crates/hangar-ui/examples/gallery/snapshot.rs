//! `--snapshot`: draws the gallery without a window, one PNG per page.
//!
//! The simulator of `iced_test` lays the interface out and draws it with
//! whichever renderer the machine has (a GPU, or tiny-skia without one;
//! `ICED_TEST_BACKEND=tiny-skia` forces the second), at twice the size, and
//! writes the picture next to the others as `<page>-<renderer>.png`.

use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use hangar_theme::{by_id, fonts, to_iced};
use hangar_ui::toast::Toast;
use iced::{Settings, Size, Vector};
use iced_test::Simulator;

use crate::{Gallery, Message, Page};

pub fn run(args: &[String]) -> iced::Result {
    let mut out = PathBuf::from("gallery-snapshots");
    let mut theme = None;
    let mut page = None;
    let mut open = None;
    let mut size = Size::new(1180.0, 800.0);

    let mut args = args.iter();
    let mut first = true;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--theme" => theme = args.next().cloned(),
            "--page" => page = args.next().cloned(),
            "--open" => open = args.next().cloned(),
            "--size" => {
                if let Some((w, h)) = args.next().and_then(|s| s.split_once('x')) {
                    size = Size::new(w.parse().unwrap_or(1180.0), h.parse().unwrap_or(800.0));
                }
            }
            other if first && !other.starts_with("--") => out = PathBuf::from(other),
            other => eprintln!("ignoring {other}"),
        }
        first = false;
    }

    fs::create_dir_all(&out).expect("the output folder can be made");
    let pages: Vec<Page> = Page::ALL
        .into_iter()
        .filter(|candidate| {
            page.as_deref()
                .is_none_or(|wanted| candidate.key() == wanted)
        })
        .collect();

    for page in pages {
        let mut gallery = Gallery::new();
        if let Some(id) = &theme {
            match by_id(id) {
                Some(found) => gallery.theme = found,
                None => eprintln!("no theme called {id}"),
            }
        }
        gallery.page = page;
        let stem = match &open {
            Some(what) => {
                prepare(&mut gallery, what);
                format!("{}-{}-{}", gallery.page.key(), what, gallery.theme.id)
            }
            None => format!("{}-{}", page.key(), gallery.theme.id),
        };
        write(&gallery, size, &out, &stem);
    }
    Ok(())
}

/// Puts the gallery in a state that is otherwise one click away.
fn prepare(gallery: &mut Gallery, what: &str) {
    match what {
        "menu" => {
            gallery.page = Page::Overlays;
            gallery.menu_open = true;
        }
        "context" => {
            gallery.page = Page::Overlays;
            gallery.context = Some(Vector::new(150.0, 30.0));
        }
        "dialog" => gallery.dialog = true,
        "palette" => gallery.open_palette(),
        "toasts" => {
            let now = Instant::now();
            gallery.page = Page::Overlays;
            gallery
                .toasts
                .push(Toast::info("Pane restarted").detail("Ben is back."), now);
            gallery.toasts.push(Toast::success("Delivery merged"), now);
            gallery
                .toasts
                .push(Toast::warning("The context is 92 % full"), now);
            gallery.toasts.push(
                Toast::error("Could not start the agent")
                    .detail("claude: command not found")
                    .action("Details", Message::Noop),
                now,
            );
        }
        "recording" => {
            gallery.page = Page::Form;
            gallery.recorder = Some(hangar_ui::shortcut::Recorder::new());
        }
        other => eprintln!("nothing to open called {other}"),
    }
}

fn write(gallery: &Gallery, size: Size, out: &std::path::Path, stem: &str) {
    // The simulator keeps a picture that exists already, to compare against;
    // here the picture is wanted afresh.
    if let Ok(entries) = fs::read_dir(out) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{stem}-")) && name.ends_with(".png") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let settings = Settings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..Settings::default()
    };
    let mut ui = Simulator::with_size(settings, size, gallery.view());
    let theme = to_iced(gallery.theme);
    let snapshot = ui.snapshot(&theme).expect("the interface can be drawn");
    snapshot
        .matches_image(out.join(stem))
        .expect("the picture can be written");
    println!("{}", out.join(stem).display());
}
