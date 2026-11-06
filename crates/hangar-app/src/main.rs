//! Hangar's window, natively.
//!
//! See docs/native/PLAN.md, "hangar-app", and crates/hangar-app/PACKAGES.md
//! for how this crate is split between packages.

// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod ctx;
mod message;
mod services;
mod shell;
mod state;

// One directory per package (PACKAGES.md), each owned by its package.
mod accounts;
mod board;
mod chat_view;
mod consent_card;
mod control;
mod git_flow;
mod git_ui;
mod inspector;
mod keymap;
mod kit_tabs;
mod kits;
mod mcp;
mod memory;
mod mobile;
mod model3d_view;
mod navigator;
mod needs_you;
mod orchestration;
mod orchestration_ui;
mod palette;
mod pane_menu;
mod panes;
mod panes_logic;
mod preview;
mod saved_commands;
mod session_picker;
mod settings;
mod stats;
mod status_bar;
mod toolbar;
mod update_banner;
mod voice;
mod workspace_dialog;

fn main() -> iced::Result {
    // Same executable, two roles: `--mcp` turns it into a stdio MCP server
    // for agent CLIs, checked first as the old main did. Shipping one binary
    // means agent configs only ever need this path.
    if std::env::args().any(|arg| arg == "--mcp") {
        services::core::run_mcp();
        return Ok(());
    }
    if let Err(refusal) = dev_guard() {
        eprintln!("{refusal}");
        std::process::exit(2);
    }
    app::run()
}

/// A development build refuses to start on the installed Hangar's data.
///
/// The native app is written on machines where the released Hangar runs, and
/// both use the same folders: `state.json`, the endpoint file agents find the
/// app by, the Claude Code mods. A debug build started plainly would answer
/// agents in the real app's place and could rewrite the person's workspaces.
/// `scripts/native-dev.sh` sets both variables to a folder of its own;
/// `HANGAR_DEV_USE_REAL_DATA=1` is the deliberate way past this. A release
/// build is the real app and never checks.
fn dev_guard() -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    if set("HANGAR_DEV_USE_REAL_DATA") || (set("HANGAR_DATA_ROOT") && set("HANGAR_ENDPOINT_FILE")) {
        return Ok(());
    }
    Err(
        "This development build would use the installed Hangar's data \
         (state.json, the agents' endpoint file).\n\
         Run it with scripts/native-dev.sh, which keeps it under target/dev-home/, \
         or set HANGAR_DATA_ROOT and HANGAR_ENDPOINT_FILE yourself \
         (HANGAR_DEV_USE_REAL_DATA=1 to use the real data on purpose)."
            .into(),
    )
}
