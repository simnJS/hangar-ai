//! Which commands are live, and what each one does (App.tsx `buildHandlers`,
//! the "Live when" column of inventory §1.10).
//!
//! A command that is not live is not consumed by the keymap (the key reaches
//! the terminal) and the palette does not list it. Running one turns it into
//! an `Intent`, a `StoreAction` or a package message; the palette (P7) lists
//! `available()` and runs through `run()`, like a shortcut.
//!
//! TODO(P0b): the whole table. What is here maps the commands whose effect is
//! a bare intent, so the routes can be tried; the rest do nothing yet.

use iced::{Task, window};

use super::CommandId;
use crate::message::{Intent, Message};
use crate::shell::{Overlay, SettingsCategory, View};
use crate::state::Model;

/// The commands live in window `w` now, in keymap order.
pub fn available(m: &Model, w: window::Id) -> Vec<CommandId> {
    let _ = (m, w);
    // TODO(P0b): every "Live when" condition.
    vec![
        CommandId::ViewPalette,
        CommandId::ViewInspector,
        CommandId::ViewNextRegion,
        CommandId::ViewPrevRegion,
    ]
}

/// What running `id` in window `w` does.
pub fn run(m: &Model, w: window::Id, id: CommandId) -> Task<Message> {
    let _ = w;
    let intent = |intent| Task::done(Message::Intent(intent));
    let active = m.active_workspace().map(|ws| ws.id.clone());
    match id {
        CommandId::ViewPalette => intent(Intent::TogglePalette),
        CommandId::ViewNavigator => intent(Intent::ToggleNavigator),
        CommandId::ViewInspector => intent(Intent::ToggleInspector),
        CommandId::ViewFocusNavigator => intent(Intent::FocusNavigator),
        CommandId::ViewFocusInspector => intent(Intent::FocusInspector),
        CommandId::ViewNextRegion => intent(Intent::CycleRegion(1)),
        CommandId::ViewPrevRegion => intent(Intent::CycleRegion(-1)),
        CommandId::ViewBoardDock => intent(Intent::ToggleDock),
        CommandId::ViewModelPreview => intent(Intent::TogglePreview),
        CommandId::ViewTerminals => intent(Intent::SetView(View::Terminals)),
        CommandId::ViewBoard => intent(Intent::SetView(View::Board)),
        CommandId::ViewMemory => intent(Intent::SetView(View::Memory)),
        CommandId::ViewStats => intent(Intent::SetView(View::Stats)),
        CommandId::ViewSettings => intent(Intent::OpenSettings(None)),
        CommandId::ViewShortcuts => intent(Intent::OpenSettings(Some(SettingsCategory::Shortcuts))),
        CommandId::ViewMcp => intent(Intent::OpenMcpInstall),
        CommandId::WorkspaceNew => intent(Intent::OpenOverlay(Overlay::WorkspaceDialog)),
        CommandId::WorkspaceMcpServers => match active {
            Some(ws) => intent(Intent::OpenMcpServers(ws)),
            None => Task::none(),
        },
        CommandId::WorkspaceOrchestration => match active {
            Some(ws) => intent(Intent::OpenOrchestration(ws)),
            None => Task::none(),
        },
        // TODO(P0b): the rest of `buildHandlers`.
        _ => Task::none(),
    }
}
