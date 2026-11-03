//! Test support for the packages of the field guards (settings, workspace
//! dialog, memory, kits, worktrees): the proof that a text field leaves the
//! keymap its shortcuts.
//!
//! A shortcut typed in a field must run, and must not also act in the field:
//! Mod+Shift+Enter splits the pane behind the dialog, and must neither type
//! nor submit the line it is pressed in (`keymap::guard`). The proof is a
//! view, built over a model in which that chord is live, that is handed the
//! chord: a guarded field spends it (`Message::Noop`, and the field never
//! sees it), an unguarded one lets it go by and says nothing.

use iced::keyboard::{self, Key, Location, Modifiers, key::Named};
use iced::{Element, Event, Settings, Size, window};
use iced_test::Simulator;

use hangar_i18n::{Locale, Translator};

use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::message::Message;
use crate::shell::WindowKind;
use crate::state::types::{AgentId, Pane, Workspace};
use crate::state::{AppState, Model};

/// A model whose main window shows a workspace with a shell pane, where
/// Mod+Shift+Enter (split the pane) is a live shortcut.
pub struct Fixture {
    pub model: Model,
    pub pkg: Packages,
    pub window: window::Id,
}

impl Default for Fixture {
    fn default() -> Self {
        Fixture::new()
    }
}

impl Fixture {
    pub fn new() -> Fixture {
        let mut model = Model::default();
        let window = window::Id::unique();
        model.live.windows.insert(window, WindowKind::Main);
        model.hydrate(AppState {
            workspaces: vec![Workspace {
                id: "w1".into(),
                name: "Shop".into(),
                cwd: "/home/me/shop".into(),
                panes: vec![Pane {
                    id: "p1".into(),
                    name: "Ava".into(),
                    agent: AgentId::Shell,
                    ..Pane::default()
                }],
                ..Workspace::default()
            }],
            active_workspace_id: Some("w1".into()),
            ..AppState::default()
        });
        model.ui.focus_by_workspace.insert("w1".into(), "p1".into());
        Fixture {
            model,
            pkg: Packages::default(),
            window,
        }
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            model: &self.model,
            pkg: &self.pkg,
            i18n: Translator::new(Locale::En),
            theme: hangar_theme::default_theme(),
            metrics: Metrics::default(),
            keymap: &self.model.live.keymap.index,
            window: self.window,
            now: std::time::Instant::now(),
        }
    }

    /// What `element` says when Mod+Shift+Enter is pressed over it, as the
    /// words of the messages: one `Noop` for each guard that spent the key.
    pub fn split_chord<'a>(&self, element: impl Into<Element<'a, Message>>) -> Vec<String> {
        let settings = Settings {
            fonts: vec![std::borrow::Cow::Borrowed(
                hangar_theme::fonts::INTER_VARIABLE,
            )],
            default_font: hangar_theme::fonts::ui(),
            ..Settings::default()
        };
        let mut ui = Simulator::with_size(settings, Size::new(1000.0, 800.0), element);
        let key = Key::Named(Named::Enter);
        ui.simulate([Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Enter),
            location: Location::Standard,
            modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
            repeat: false,
            text: None,
        })]);
        ui.into_messages()
            .map(|message| format!("{message:?}"))
            .collect()
    }

    /// The same, for a view that is more than a field: every message must be
    /// a spent key, and at least one must be.
    pub fn assert_spends_the_split_chord<'a>(&self, element: impl Into<Element<'a, Message>>) {
        let said = self.split_chord(element);
        assert!(
            !said.is_empty() && said.iter().all(|word| word == "Noop"),
            "the shortcut was not spent by the field: {said:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_input;

    #[test]
    fn the_split_chord_is_live_in_the_fixture() {
        let fixture = Fixture::new();
        assert!(
            crate::commands::available(&fixture.model, fixture.window)
                .contains(&crate::commands::CommandId::PaneSplit)
        );
    }

    #[test]
    fn a_field_without_a_guard_does_not_spend_the_chord() {
        let fixture = Fixture::new();
        let field: Element<'_, Message> = text_input("", "").into();
        assert!(fixture.split_chord(field).is_empty());
    }

    #[test]
    fn a_guarded_field_spends_it() {
        let fixture = Fixture::new();
        let field = crate::keymap::guard(fixture.ctx(), text_input("", ""));
        assert_eq!(fixture.split_chord(field), ["Noop"]);
    }
}
