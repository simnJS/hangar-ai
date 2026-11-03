//! The sheet drawn. The simulator of `iced_test` lays a view out as a window
//! would; nothing is compared. A category that cannot be laid out (a `Fill` in
//! a place that has no size to fill, a widget that panics on an empty list)
//! fails here and not on the first person who opens it.

use std::borrow::Cow;

use iced::{Settings as WindowSettings, Size};
use iced_test::Simulator;

use super::*;
use crate::app::Packages;
use crate::ctx::{Ctx, Metrics};
use crate::state::types::{AppState, Workspace};

fn model() -> Model {
    let mut model = Model::default();
    model.hydrate(AppState {
        workspaces: vec![Workspace {
            id: "w1".into(),
            name: "Shop".into(),
            mobile_ports: Some(vec![5173]),
            ..Workspace::default()
        }],
        active_workspace_id: Some("w1".into()),
        ..AppState::default()
    });
    model.ui.shells = vec![hangar_core::shells::ShellInfo {
        id: "bash".into(),
        label: "Bash".into(),
        program: "bash".into(),
        args: Vec::new(),
    }];
    model
}

fn sheet(
    category: SettingsCategory,
    locale: hangar_i18n::Locale,
    theme: &str,
    query: &str,
    size: Size,
) {
    let mut model = model();
    model.ui.overlays = vec![Overlay::Settings(category)];
    let pkg = Packages::default();
    let st = State {
        query: query.into(),
        ..State::default()
    };
    let ctx = Ctx {
        model: &model,
        pkg: &pkg,
        i18n: Translator::new(locale),
        theme: hangar_theme::by_id_or_default(theme),
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
    let _ui = Simulator::with_size(window, size, view(&st, ctx, category));
}

/// The window the shell opens.
const MAIN: Size = Size::new(1360.0, 860.0);

#[test]
fn every_category_can_be_laid_out_in_both_languages_and_a_dark_and_a_light_theme() {
    for category in schema::ORDER {
        for locale in hangar_i18n::Locale::ALL {
            for theme in ["graphite", "catppuccin-latte"] {
                sheet(category, locale, theme, "", MAIN);
            }
        }
    }
}

#[test]
fn a_search_that_finds_something_and_one_that_finds_nothing_can_be_laid_out() {
    use hangar_i18n::Locale;
    sheet(SettingsCategory::General, Locale::En, "graphite", "theme", MAIN);
    sheet(SettingsCategory::General, Locale::Fr, "graphite", "thème", MAIN);
    sheet(
        SettingsCategory::General,
        Locale::En,
        "graphite",
        "no such setting anywhere",
        MAIN,
    );
}

#[test]
fn the_small_window_the_shell_allows_can_hold_the_sheet() {
    // The main window's minimum is 900 × 560, smaller than the sheet's own
    // 980 × 700, which gives way.
    sheet(
        SettingsCategory::Appearance,
        hangar_i18n::Locale::En,
        "graphite",
        "",
        Size::new(900.0, 560.0),
    );
}
