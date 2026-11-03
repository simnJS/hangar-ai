//! The column drawn headless, with a pane that has reported everything a
//! pane can.
//!
//! The tests always build and lay the column out, which catches a view that
//! panics or cannot be built. With `HANGAR_SNAPSHOTS=<folder>` set they also
//! draw it and write a PNG per case, which is how the column is looked at
//! without a window (a GNOME Wayland session lets nobody but the person take
//! a screenshot). Drawing runs one test at a time, on the software renderer:
//!
//! ```text
//! ICED_TEST_BACKEND=tiny-skia HANGAR_SNAPSHOTS=/tmp/inspector \
//!     cargo test -p hangar-app inspector::snapshot -- --test-threads=1
//! ```

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use hangar_core::bridge::Queued;
use hangar_core::git::{ChangedFile, Changes};
use hangar_i18n::{Locale, Translator};
use hangar_theme::{AgentState, Theme, by_id_or_default, fonts, to_iced};
use iced::{Element, Settings, Size, window};
use iced_test::Simulator;
use serde_json::json;

use super::sources::TaskCard;
use super::view::{Data, column_of, scene};
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::state::Model;
use crate::state::live::{ContextGauge, PaneEvent, PaneInsight, RateLimit};
use crate::state::types::{AgentId, AppState, Pane, Workspace};

fn model(agent: AgentId) -> Model {
    let mut m = Model::default();
    m.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w".into(),
            name: "storefront".into(),
            cwd: "/home/ada/dev/clients/storefront".into(),
            panes: vec![Pane {
                id: "a".into(),
                name: "Ava".into(),
                agent,
                session_id: Some("3f2a9c1e-0b7d-4c55-9a43-2f1e8d0c6b11".into()),
                cwd: Some("/home/ada/dev/clients/storefront/packages/checkout-ui".into()),
                ..Pane::default()
            }],
            ..Workspace::default()
        }],
        active_workspace_id: Some("w".into()),
        ..AppState::default()
    });
    m
}

fn ago(minutes: u64) -> Instant {
    Instant::now()
        .checked_sub(Duration::from_secs(minutes * 60))
        .unwrap_or_else(Instant::now)
}

fn insight() -> PaneInsight {
    let event = |minutes, kind: &str, params| PaneEvent {
        at: ago(minutes),
        kind: kind.into(),
        params,
    };
    PaneInsight {
        model: Some("claude-sonnet-4-5-20250929".into()),
        agent_version: Some("2.1.4".into()),
        session_since: Some(ago(80)),
        context: Some(ContextGauge {
            pct: 72.0,
            used_tokens: 144_000,
            window: 200_000,
            model: None,
        }),
        cost_usd: Some(1.84),
        queue: vec![
            Queued {
                id: "q1".into(),
                text: "Run the whole test suite and fix what breaks".into(),
                from: None,
            },
            Queued {
                id: "q2".into(),
                text: "Rebase on main".into(),
                from: Some("hangar".into()),
            },
        ],
        edited_files: vec![
            "/home/ada/dev/clients/storefront/packages/checkout-ui/src/Cart.tsx".into(),
            "/home/ada/dev/clients/storefront/packages/checkout-ui/src/pricing/discount.ts".into(),
            "/home/ada/dev/clients/storefront/packages/checkout-ui/logo.png".into(),
            "/home/ada/dev/clients/storefront/README.md".into(),
        ],
        events: vec![
            event(1, "edit", json!({"file": "Cart.tsx"})),
            event(2, "permission", json!({"tool": "Bash"})),
            event(5, "turn-start", json!({})),
            event(
                12,
                "turn-end",
                json!({"outcome": "answer", "answer": "Moved the discount rules into pricing/."}),
            ),
            event(
                40,
                "queued",
                json!({"text": "Rebase on main", "from": "hangar"}),
            ),
        ],
        turn_started_at: Some(ago(5)),
        mcp: Some(json!({"mode": "custom", "servers": ["hangar", "github"]})),
        ..PaneInsight::default()
    }
}

fn changes() -> Changes {
    let file = |path: &str, status: &str, add, del| ChangedFile {
        path: path.into(),
        status: status.into(),
        additions: Some(add),
        deletions: Some(del),
        ..ChangedFile::default()
    };
    Changes {
        root: "/home/ada/dev/clients/storefront".into(),
        base: Some("a1b2c3d".into()),
        files: vec![
            file("packages/checkout-ui/src/Cart.tsx", "M", 38, 12),
            file("packages/checkout-ui/src/pricing/discount.ts", "?", 64, 0),
            ChangedFile {
                path: "packages/checkout-ui/logo.png".into(),
                status: "M".into(),
                binary: true,
                ..ChangedFile::default()
            },
        ],
        truncated: false,
    }
}

fn limits() -> Vec<RateLimit> {
    vec![
        RateLimit {
            kind: "five_hour".into(),
            percent_used: 34.0,
            resets_at: Some("2099-01-01T20:01:00Z".into()),
        },
        RateLimit {
            kind: "seven_day".into(),
            percent_used: 91.0,
            resets_at: None,
        },
    ]
}

fn render(
    name: &str,
    theme: &'static Theme,
    locale: Locale,
    width: f32,
    element: Element<'_, Message>,
) {
    let settings = Settings {
        fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
        default_font: fonts::ui(),
        ..Settings::default()
    };
    // Building the simulator lays the column out, which is what a plain test
    // run checks. Drawing it reads pixels back from the renderer, and
    // several of those at once crash a GPU driver: only a person asking for
    // pictures does that, one test at a time.
    let mut ui = Simulator::with_size(settings, Size::new(width, 900.0), element);
    let Some(folder) = std::env::var_os("HANGAR_SNAPSHOTS").map(PathBuf::from) else {
        return;
    };
    let snapshot = ui
        .snapshot(&to_iced(theme))
        .expect("the column can be drawn");
    std::fs::create_dir_all(&folder).expect("the folder can be made");
    let stem = format!("{name}-{}-{}", theme.id, locale.id());
    // The simulator compares with a picture that exists; a fresh one is wanted.
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.starts_with(&format!("{stem}-")) && file.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    snapshot
        .matches_image(folder.join(&stem))
        .expect("the picture can be written");
}

/// Draws the column for `agent` as `locale` reads it, in `theme`.
fn draw(name: &str, agent: AgentId, theme_id: &str, locale: Locale, width: f64) {
    let mut m = model(agent);
    let mut state = m.state().clone();
    state.settings.inspector_width = width;
    state.settings.locale = Some(locale);
    state.settings.theme_id = theme_id.into();
    m.hydrate(state);

    let packages = Packages::default();
    let theme = by_id_or_default(theme_id);
    let ctx = Ctx {
        model: &m,
        pkg: &packages,
        i18n: Translator::new(locale),
        theme,
        metrics: Metrics::default(),
        keymap: &m.live.keymap.index,
        window: window::Id::unique(),
        now: Instant::now(),
    };
    let ws = &m.state().workspaces[0];
    let pane = &ws.panes[0];
    let scene = scene(ctx, ws, pane);
    let insight = insight();
    let changes = changes();
    let limits = limits();
    let data = Data {
        insight: &insight,
        state: AgentState::Working,
        bridged: true,
        limits: Some(&limits),
        task: Some(TaskCard {
            id: "9d1f7a2e-5b3c-4e08-8a6d-1c2b3a4f5e60".into(),
            title: "Rework the checkout discount rules so that stacking coupons works".into(),
            priority: 1,
            column: "doing".into(),
            doing_since: Some(super::sources::now_ms() - 47 * 60_000),
            comments: 3,
        }),
        shell: Some("zsh".into()),
        changes: Some(&changes),
        index: 1,
    };
    render(name, theme, locale, width as f32, column_of(&scene, data));
}

#[test]
fn an_agent_pane_with_everything_reported_is_drawn_in_both_languages() {
    for locale in [Locale::En, Locale::Fr] {
        draw("claude", AgentId::Claude, "graphite", locale, 296.0);
    }
}

#[test]
fn a_shell_pane_is_drawn() {
    draw("shell", AgentId::Shell, "graphite", Locale::En, 296.0);
}

#[test]
fn a_light_theme_and_a_narrow_column_are_drawn() {
    let light = hangar_theme::themes()
        .iter()
        .find(|theme| !theme.is_dark())
        .expect("a light theme exists")
        .id;
    draw("claude", AgentId::Claude, light, Locale::En, 296.0);
    draw(
        "claude-narrow",
        AgentId::Claude,
        "graphite",
        Locale::En,
        240.0,
    );
}
