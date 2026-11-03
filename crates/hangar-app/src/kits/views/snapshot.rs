//! Pictures of the screens, to look at.
//!
//! The simulator of `iced_test` lays a view out and draws it with whichever
//! renderer the machine has, without a window. This is not a check (nothing is
//! compared): it writes one PNG per screen, theme and language to the folder
//! named by `HANGAR_KITS_SNAPSHOTS`, so that whoever changes these screens can
//! see them.
//!
//! ```text
//! HANGAR_KITS_SNAPSHOTS=/tmp/kits cargo test -p hangar-app kits::views::snapshot -- --ignored
//! ```

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Instant;

use iced::widget::container;
use iced::{Element, Length, Settings, Size, window};
use iced_test::Simulator;

use hangar_core::kits::commands::CommandInfo;
use hangar_core::kits::{CommandArg, Hook, Inspection, StepStatus, Tab};
use hangar_i18n::{Locale, Translator};
use hangar_theme::{Theme, by_id, fonts, to_iced};

use super::activation_model::StepRun;
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::kits::icon::IconBytes;
use crate::kits::testing::{HOOKS, entry, git, git_kit, installed, kit, manifest, native_tab, tab};
use crate::kits::{Activation, State, Target};
use crate::message::Message;
use crate::state::types::{AppState, Pane, Workspace};
use crate::state::{Model, types::AgentId};

fn out_dir() -> Option<PathBuf> {
    std::env::var_os("HANGAR_KITS_SNAPSHOTS").map(PathBuf::from)
}

const ICON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round"><path d="M3 8l3 3 7-7"/><circle cx="8" cy="8" r="6.2"/></svg>"##;

fn model() -> Model {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w1".into(),
            name: "Studio".into(),
            panes: vec![Pane {
                id: "p1".into(),
                name: "Ava".into(),
                agent: AgentId::Claude,
                ..Pane::default()
            }],
            kits: Some(vec![entry("hello-kit", &["panes.read", HOOKS])]),
            ..Workspace::default()
        }],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });

    let mut hello = git_kit(Some(HOOKS), true);
    let found = hello.manifest.as_mut().expect("manifest");
    found.description = "Watches your CI runs and tells your agents.".into();
    found.icon = Some("icon.svg".into());
    found.tabs = vec![native_tab(&["panes.read", "command:ci-run"]), {
        let mut page = tab(&["notify"]);
        page.id = "page".into();
        page.title = "Dashboard".into();
        page
    }];
    hello.dir = std::env::temp_dir().to_string_lossy().into_owned();
    hello.source = git(
        "https://github.com/hangar-ai/hello-kit",
        Some("refs/tags/v1.0.0"),
    );

    let mut roblox = kit("0.3.0", vec![tab(&["files.read", "files.write"])]);
    roblox.id = "roblox".into();
    roblox.manifest = Some({
        let mut m = manifest("roblox", "0.3.0", vec![tab(&["files.read", "files.write"])]);
        m.name = "Roblox".into();
        m.description = "Build places with agents.".into();
        m
    });
    let mut broken = installed("bad-kit");
    broken.error = Some("tabs[0].permissions: unknown permission \"files.delete\"".into());

    let kits = vec![hello.clone(), roblox, broken];
    m.live.kits.installed = Some(kits);
    // The one icon, as it is read from disk.
    m.live.kits.icons.loaded(
        "hello-kit",
        crate::kits::icon::icon_key(&hello).expect("an icon"),
        Ok(IconBytes::Svg(ICON.to_vec())),
    );
    m
}

fn hooks() -> Vec<Hook> {
    vec![
        Hook {
            event: "PostToolUse".into(),
            matcher: "Bash|PowerShell|Write|Edit|MultiEdit|NotebookEdit|Task".into(),
            command: "echo \"edited\" >> \"$HANGAR_KITS_DATA/hello-kit/log\"".into(),
        },
        Hook {
            event: "SessionStart".into(),
            matcher: String::new(),
            command: "node ${CLAUDE_PLUGIN_ROOT}/start.js".into(),
        },
    ]
}

fn inspection() -> Inspection {
    let tabs: Vec<Tab> = vec![
        {
            let mut native = native_tab(&["panes.read", "command:ci-run", "loopback:8765"]);
            native.title = "CI".into();
            native
        },
        {
            let mut page = tab(&["files.read", "files.write", "notify"]);
            page.id = "page".into();
            page.title = "Dashboard".into();
            page
        },
    ];
    Inspection {
        skills: vec!["ci-status".into(), "ci-logs".into()],
        commands: vec!["ci".into()],
        mcp_servers: vec!["github".into()],
        hooks: hooks(),
        hooks_permission: Some(HOOKS.into()),
        declared_commands: vec![CommandInfo {
            name: "ci-run".into(),
            description: "The state of a CI run.".into(),
            argv: ["gh", "run", "view", "{run}", "--repo={repo}"]
                .map(String::from)
                .to_vec(),
            args: [
                (
                    "repo".to_owned(),
                    CommandArg {
                        pattern: "[\\w.-]+/[\\w.-]+".into(),
                        description: None,
                    },
                ),
                (
                    "run".to_owned(),
                    CommandArg {
                        pattern: "[0-9]+".into(),
                        description: Some("The run's number".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
            timeout_seconds: 30,
        }],
        commands_permission: Some("commands:0123456789abcdef".into()),
        ..crate::kits::testing::inspection(tabs)
    }
}

fn steps() -> Vec<StepStatus> {
    vec![StepStatus {
        id: "deps".into(),
        title: "Install the server's dependencies".into(),
        description: Some("Needs Node 20 or later.".into()),
        command: "npm install --prefix server".into(),
        sha256: "x".into(),
        done: false,
    }]
}

fn activation(ran: bool) -> State {
    State {
        activation: Some(Activation {
            kit: "hello-kit".into(),
            target: Target::Workspace("w1".into()),
            ws: Some("w1".into()),
            inspection: Some(inspection()),
            steps: Some(steps()),
            running: None,
            ran: ran.then(|| StepRun {
                step: "deps".into(),
                pane_name: "Bea".into(),
            }),
            error: None,
            seen: (None, None),
        }),
        ..State::default()
    }
}

struct Shot {
    name: &'static str,
    size: Size,
}

fn write(
    out: &std::path::Path,
    stem: &str,
    size: Size,
    theme: &'static Theme,
    view: Element<'_, Message>,
) {
    let settings = Settings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..Settings::default()
    };
    if let Ok(entries) = std::fs::read_dir(out) {
        for found in entries.flatten() {
            let name = found.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{stem}-")) && name.ends_with(".png") {
                let _ = std::fs::remove_file(found.path());
            }
        }
    }
    let mut ui = Simulator::with_size(settings, size, view);
    let snapshot = ui.snapshot(&to_iced(theme)).expect("the view can be drawn");
    snapshot
        .matches_image(out.join(stem))
        .expect("the picture can be written");
    println!("{}", out.join(stem).display());
}

#[test]
#[ignore = "writes pictures; run with HANGAR_KITS_SNAPSHOTS=<folder>"]
fn write_the_screens() {
    let Some(out) = out_dir() else {
        eprintln!("HANGAR_KITS_SNAPSHOTS names no folder: nothing written");
        return;
    };
    std::fs::create_dir_all(&out).expect("the folder");
    // The icon's file is read from disk in the app; the cache is filled here.
    let m = model();
    let pkg = Packages::default();
    let w1 = "w1".to_string();
    let screens = [
        Shot {
            name: "activation",
            size: Size::new(900.0, 800.0),
        },
        Shot {
            name: "peek",
            size: Size::new(900.0, 700.0),
        },
        Shot {
            name: "body",
            size: Size::new(640.0, 2600.0),
        },
        Shot {
            name: "section",
            size: Size::new(640.0, 360.0),
        },
        Shot {
            name: "settings",
            size: Size::new(780.0, 1500.0),
        },
    ];
    for (theme_id, locale) in [
        ("graphite", Locale::En),
        ("github-light", Locale::En),
        ("graphite", Locale::Fr),
    ] {
        let theme = by_id(theme_id).expect("a theme");
        let ctx = Ctx {
            model: &m,
            pkg: &pkg,
            i18n: Translator::new(locale),
            theme,
            metrics: Metrics::default(),
            keymap: &m.live.keymap.index,
            window: window::Id::unique(),
            now: Instant::now(),
        };
        for shot in &screens {
            let stem = format!("{}-{}-{:?}", shot.name, theme_id, locale);
            let mut st = State::default();
            let view: Element<'_, Message> = match shot.name {
                "activation" => {
                    st = activation(false);
                    crate::kits::activation_view(&st, ctx, "hello-kit", None)
                }
                "peek" => {
                    st = activation(true);
                    crate::kits::activation_view(&st, ctx, "hello-kit", None)
                }
                // The whole body, not scrolled inside its dialog.
                "body" => {
                    st = activation(false);
                    let (found, model, _) =
                        super::activation::model_of(&st, ctx, "hello-kit").expect("open");
                    container(super::activation::body(ctx, found, &model))
                        .padding(20)
                        .width(Length::Fill)
                        .into()
                }
                "section" => container(crate::kits::workspace_section(&st, ctx, Some(&w1)))
                    .padding(20)
                    .width(Length::Fill)
                    .into(),
                _ => crate::kits::settings_view(&st, ctx),
            };
            // `st` outlives the view in this arm only by being declared first.
            write(&out, &stem, shot.size, theme, view);
            drop(st);
        }
    }
}
