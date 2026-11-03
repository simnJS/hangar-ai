//! A headless window for the views of this package (P8): a model with a
//! workspace or two, a `Ctx` over it, and `iced_test`'s simulator to draw and
//! drive what a view returns.
//!
//! `HANGAR_P8_SNAPSHOTS=<folder>` makes the tests that call [`Harness::shot`]
//! write their picture there as `<name>-<renderer>.png`, which is how the
//! surfaces are looked at in both languages and a dark and a light theme.

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Instant;

use hangar_core::accounts::Account;
use hangar_i18n::{Locale, Translator};
use hangar_theme::{fonts, to_iced};
use iced::{Element, Settings, Size, window};
use iced_test::Simulator;

use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::shell::WindowKind;
use crate::state::types::{AgentId, Pane, ShellInfo, Workspace};
use crate::state::{AppState, Model};

/// The window the tests draw in: the dialog's width plus air.
pub const WINDOW: Size = Size::new(900.0, 760.0);

pub struct Harness {
    pub model: Model,
    pub pkg: Packages,
    pub window: window::Id,
}

pub fn account(id: &str, label: &str, logged_in: bool) -> Account {
    Account {
        id: id.into(),
        label: label.into(),
        is_main: id == "main",
        logged_in,
        email: (id != "main").then(|| format!("{id}@example.com")),
        plan: (id == "main").then(|| "Max".to_string()),
    }
}

fn pane(id: &str, name: &str, agent: AgentId) -> Pane {
    Pane {
        id: id.into(),
        name: name.into(),
        agent,
        ..Pane::default()
    }
}

impl Harness {
    /// Two workspaces on a machine with two shells, two agents installed and
    /// two Claude accounts besides the main one.
    pub fn new(locale: Locale, theme: &str) -> Harness {
        let mut state = AppState::default();
        state.settings.locale = Some(locale);
        state.settings.theme_id = theme.into();
        state.workspaces = vec![
            Workspace {
                id: "w1".into(),
                name: "hangar".into(),
                cwd: "/home/me/dev/hangar".into(),
                panes: vec![
                    pane("p1", "Ava", AgentId::Claude),
                    pane("p2", "Max", AgentId::Shell),
                ],
                ..Workspace::default()
            },
            Workspace {
                id: "w2".into(),
                name: "shop".into(),
                cwd: "/home/me/dev/shop".into(),
                claude_account: Some("perso".into()),
                panes: vec![pane("p3", "Leo", AgentId::Claude)],
                ..Workspace::default()
            },
        ];
        state.active_workspace_id = Some("w1".into());
        let mut model = Model::default();
        model.hydrate(state);
        let window = window::Id::unique();
        model.live.windows.insert(window, WindowKind::Main);
        model.ui.shells = vec![
            ShellInfo {
                id: "bash".into(),
                label: "Bash".into(),
                program: "/bin/bash".into(),
                args: vec![],
            },
            ShellInfo {
                id: "zsh".into(),
                label: "Zsh".into(),
                program: "/bin/zsh".into(),
                args: vec![],
            },
        ];
        model.ui.available_agents = vec!["claude".into(), "codex".into()];
        model.live.accounts.accounts = Some(vec![
            account("main", "Principal", true),
            account("perso", "Perso", true),
            account("pro", "Travail", false),
        ]);
        Harness {
            model,
            pkg: Packages::default(),
            window,
        }
    }

    /// The persisted state changed by `edit` (the store's own actions are
    /// P0b's, so a test that needs a different state hydrates it).
    pub fn edit(mut self, edit: impl FnOnce(&mut AppState)) -> Harness {
        let mut state = self.model.state().clone();
        edit(&mut state);
        self.model.hydrate(state);
        self
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            model: &self.model,
            pkg: &self.pkg,
            i18n: Translator::new(self.model.locale()),
            theme: self.model.theme(),
            metrics: Metrics::default(),
            keymap: &self.model.live.keymap.index,
            window: self.window,
            now: Instant::now(),
        }
    }

    /// A simulator showing `element` in a window of [`WINDOW`].
    pub fn show<'a>(&self, element: impl Into<Element<'a, Message>>) -> Simulator<'a, Message> {
        let settings = Settings {
            fonts: vec![Cow::Borrowed(fonts::INTER_VARIABLE)],
            default_font: fonts::ui(),
            ..Settings::default()
        };
        Simulator::with_size(settings, WINDOW, element)
    }

    /// Writes the picture of what the simulator shows, when asked to.
    pub fn shot(&self, ui: &mut Simulator<'_, Message>, name: &str) {
        let Some(folder) = std::env::var_os("HANGAR_P8_SNAPSHOTS").map(PathBuf::from) else {
            return;
        };
        let path = folder.join(name);
        // The simulator keeps a picture that exists already, to compare
        // against; here the picture is wanted afresh.
        if let Some(parent) = path.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let file = entry.file_name().to_string_lossy().into_owned();
                    if file.starts_with(&format!("{name}-")) && file.ends_with(".png") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
        let theme = to_iced(self.model.theme());
        let snapshot = ui.snapshot(&theme).expect("the interface can be drawn");
        snapshot
            .matches_image(path)
            .expect("the picture can be written");
    }
}

/// The messages a simulator collected, for the checks to look through.
pub fn messages(ui: Simulator<'_, Message>) -> Vec<Message> {
    ui.into_messages().collect()
}
