//! The page in each of its states can be laid out, a code with its QR image
//! among them (the one part the page draws from a string the PC made).

use std::borrow::Cow;

use iced::{Settings as WindowSettings, Size};
use iced_test::Simulator;

use super::*;
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::settings::mobile_model::{Mapping, ShownApp, TailscaleStatus};

fn lay_out(st: &State) {
    lay_out_as(st, None);
}

/// Laid out, and drawn to `<folder>/<stem>-<locale>` when the folder is named
/// by `HANGAR_SETTINGS_SNAPSHOTS`, to look at:
///
/// ```text
/// HANGAR_SETTINGS_SNAPSHOTS=/tmp/settings cargo test -p hangar-app settings::mobile
/// ```
fn lay_out_as(st: &State, stem: Option<&str>) {
    // Several simulators drawing this page at once (one per test thread)
    // crashed the test process now and then with SIGSEGV after every test
    // had passed, and never one at a time: they take turns.
    static DRAWING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = DRAWING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w1".into(),
            name: "Shop".into(),
            mobile_ports: Some(vec![5173]),
            ..Workspace::default()
        }],
        ..AppState::default()
    });
    let pkg = Packages::default();
    for locale in hangar_i18n::Locale::ALL {
        let ctx = Ctx {
            model: &model,
            pkg: &pkg,
            i18n: hangar_i18n::Translator::new(locale),
            theme: hangar_theme::default_theme(),
            metrics: Metrics::default(),
            keymap: &model.live.keymap.index,
            window: iced::window::Id::unique(),
            now: std::time::Instant::now(),
        };
        let window = WindowSettings {
            fonts: vec![Cow::Borrowed(hangar_theme::fonts::INTER_VARIABLE)],
            default_font: hangar_theme::fonts::ui(),
            ..WindowSettings::default()
        };
        let element: iced::Element<'_, Message> = view(st, ctx);
        let mut ui = Simulator::with_size(window, Size::new(700.0, 1500.0), element);
        if let (Some(stem), Some(folder)) = (stem, std::env::var_os("HANGAR_SETTINGS_SNAPSHOTS")) {
            let folder = std::path::PathBuf::from(folder);
            std::fs::create_dir_all(&folder).expect("the folder");
            let picture = ui
                .snapshot(&hangar_theme::to_iced(ctx.theme))
                .expect("the page can be drawn");
            picture
                .matches_image(folder.join(format!("{stem}-{}", locale.id())))
                .expect("the picture can be written");
        }
    }
}

fn on() -> RemoteStatus {
    RemoteStatus {
        enabled: true,
        running: true,
        port: 47821,
        error: None,
    }
}

fn ready() -> TailscaleStatus {
    TailscaleStatus {
        installed: true,
        running: true,
        dns_name: Some("pc.tail1234.ts.net".into()),
        serving: true,
    }
}

#[test]
fn before_the_pc_has_answered() {
    lay_out(&State::default());
}

#[test]
fn with_hangar_mobile_off_and_with_a_store_that_could_not_be_opened() {
    lay_out(&State {
        remote: Some(RemoteStatus {
            enabled: false,
            running: false,
            ..on()
        }),
        ..State::default()
    });
    lay_out(&State {
        unavailable: true,
        ..State::default()
    });
}

#[test]
fn with_every_block_and_a_code_on_show() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 21"><rect width="21" height="21" fill="#fff"/><rect x="2" y="2" width="5" height="5" fill="#000"/><rect x="14" y="2" width="5" height="5" fill="#000"/><rect x="2" y="14" width="5" height="5" fill="#000"/></svg>"##;
    let start = PairStart {
        code: "123456".into(),
        url: "https://pc.tail1234.ts.net/?code=123456".into(),
        qr_svg: svg.into(),
        expires_at: now_ms() + 270_000,
    };
    let st = State {
        remote: Some(on()),
        tailscale: Some(ready()),
        devices: vec![device("a", "iPhone de Sam", 1), device("b", "Pixel", 2)],
        revoking: Some("a".into()),
        options: Some(MobileOptions {
            keep_awake: true,
            keep_awake_supported: true,
            autostart: false,
        }),
        slots: Some(vec![
            AppSlot {
                slot: 0,
                https_port: 8443,
                local_port: 47822,
                mapping: Some(Mapping::Mapped),
                app: Some(ShownApp {
                    workspace_id: "w1".into(),
                    workspace: "Shop".into(),
                    port: 5173,
                    last_used: now_ms(),
                }),
            },
            AppSlot {
                slot: 1,
                https_port: 8444,
                local_port: 47823,
                mapping: Some(Mapping::Taken),
                app: None,
            },
        ]),
        just_paired: Some(("Pixel".into(), 1)),
        now: now_ms(),
        pairing: Some(Pairing {
            qr: svg::Handle::from_memory(start.qr_svg.clone().into_bytes()),
            start,
        }),
        ..State::default()
    };
    lay_out_as(&st, Some("mobile-code"));
    let expired = State {
        now: now_ms() + 600_000,
        ..st
    };
    lay_out(&expired);
}

#[test]
fn at_each_step_of_the_tailscale_setup() {
    for tailscale in [
        TailscaleStatus {
            installed: false,
            running: false,
            dns_name: None,
            serving: false,
        },
        TailscaleStatus {
            installed: true,
            running: false,
            dns_name: None,
            serving: false,
        },
        TailscaleStatus {
            serving: false,
            ..ready()
        },
    ] {
        lay_out(&State {
            remote: Some(on()),
            tailscale: Some(tailscale),
            ..State::default()
        });
    }
}
