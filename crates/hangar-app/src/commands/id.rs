//! Every command a shortcut or the palette can run: the 88 of `COMMANDS` in
//! `src/lib/shortcuts.ts` (inventory §1.10), in its order.
//!
//! The order matters: the dispatcher runs the first command of the keymap
//! with a live handler (P7), and the settings list them in it. The string ids
//! are the keys of `settings.keybindings` in `state.json`, so they never
//! change. Default bindings are the keymap's (P7, `keymap/`), not here.

use std::fmt;
use std::str::FromStr;

use hangar_i18n::Key;

/// The groups of Settings › Shortcuts, in their order (`SECTIONS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Panes,
    Navigation,
    Workspaces,
    View,
    Terminal,
    Commands,
    Voice,
    App,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Panes,
        Section::Navigation,
        Section::Workspaces,
        Section::View,
        Section::Terminal,
        Section::Commands,
        Section::Voice,
        Section::App,
    ];

    pub fn label_key(self) -> Key {
        match self {
            Section::Panes => Key::KeymapSectionPanes,
            Section::Navigation => Key::KeymapSectionNavigation,
            Section::Workspaces => Key::KeymapSectionWorkspaces,
            Section::View => Key::KeymapSectionView,
            Section::Terminal => Key::KeymapSectionTerminal,
            Section::Commands => Key::KeymapSectionCommands,
            Section::Voice => Key::KeymapSectionVoice,
            Section::App => Key::KeymapSectionApp,
        }
    }
}

macro_rules! commands {
    ($($variant:ident = $id:literal, $section:ident, $label:ident $(, $n:literal)?;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum CommandId {
            $($variant,)*
        }

        impl CommandId {
            /// Every command, in `COMMANDS` order.
            pub const ALL: &'static [CommandId] = &[$(CommandId::$variant,)*];

            /// The id `state.json` and the TS use (`"pane.split"`).
            pub fn as_str(self) -> &'static str {
                match self {
                    $(CommandId::$variant => $id,)*
                }
            }

            pub fn section(self) -> Section {
                match self {
                    $(CommandId::$variant => Section::$section,)*
                }
            }

            /// The label's key. The digit families share one label with an
            /// `{n}` placeholder, filled from `label_n`.
            pub fn label_key(self) -> Key {
                match self {
                    $(CommandId::$variant => Key::$label,)*
                }
            }

            /// The `n` of the label of a digit family (`labelParams`).
            pub fn label_n(self) -> Option<u8> {
                match self {
                    $(CommandId::$variant => commands!(@n $($n)?),)*
                }
            }
        }

        impl FromStr for CommandId {
            type Err = UnknownCommand;

            fn from_str(id: &str) -> Result<Self, Self::Err> {
                match id {
                    $($id => Ok(CommandId::$variant),)*
                    _ => Err(UnknownCommand(id.to_string())),
                }
            }
        }
    };
    (@n $n:literal) => { Some($n) };
    (@n) => { None };
}

commands! {
    PaneSplit = "pane.split", Panes, CmdPaneSplit;
    PaneSplitRight = "pane.splitRight", Panes, CmdPaneSplitRight;
    PaneSplitDown = "pane.splitDown", Panes, CmdPaneSplitDown;
    PaneClose = "pane.close", Panes, CmdPaneClose;
    PaneRestart = "pane.restart", Panes, CmdPaneRestart;
    PaneRestartAll = "pane.restartAll", Panes, CmdPaneRestartAll;
    PaneResetAll = "pane.resetAll", Panes, CmdPaneResetAll;
    PaneTidy = "pane.tidy", Panes, CmdPaneTidy;
    PaneSessions = "pane.sessions", Panes, CmdPaneSessions;
    PaneZoom = "pane.zoom", Panes, CmdPaneZoom;
    PanePinOrchestrator = "pane.pinOrchestrator", Panes, CmdPanePinOrchestrator;
    PaneUnpinOrchestrator = "pane.unpinOrchestrator", Panes, CmdPaneUnpinOrchestrator;
    PaneNext = "pane.next", Navigation, CmdPaneNext;
    PanePrev = "pane.prev", Navigation, CmdPanePrev;
    PaneFocusLeft = "pane.focusLeft", Navigation, CmdPaneFocusLeft;
    PaneFocusRight = "pane.focusRight", Navigation, CmdPaneFocusRight;
    PaneFocusUp = "pane.focusUp", Navigation, CmdPaneFocusUp;
    PaneFocusDown = "pane.focusDown", Navigation, CmdPaneFocusDown;
    PaneFocus1 = "pane.focus1", Navigation, CmdPaneFocusN, 1;
    PaneFocus2 = "pane.focus2", Navigation, CmdPaneFocusN, 2;
    PaneFocus3 = "pane.focus3", Navigation, CmdPaneFocusN, 3;
    PaneFocus4 = "pane.focus4", Navigation, CmdPaneFocusN, 4;
    PaneFocus5 = "pane.focus5", Navigation, CmdPaneFocusN, 5;
    PaneFocus6 = "pane.focus6", Navigation, CmdPaneFocusN, 6;
    PaneFocus7 = "pane.focus7", Navigation, CmdPaneFocusN, 7;
    PaneFocus8 = "pane.focus8", Navigation, CmdPaneFocusN, 8;
    PaneFocus9 = "pane.focus9", Navigation, CmdPaneFocusN, 9;
    PaneSwapLeft = "pane.swapLeft", Navigation, CmdPaneSwapLeft;
    PaneSwapRight = "pane.swapRight", Navigation, CmdPaneSwapRight;
    PaneSwapUp = "pane.swapUp", Navigation, CmdPaneSwapUp;
    PaneSwapDown = "pane.swapDown", Navigation, CmdPaneSwapDown;
    WorkspaceNext = "workspace.next", Workspaces, CmdWorkspaceNext;
    WorkspacePrev = "workspace.prev", Workspaces, CmdWorkspacePrev;
    WorkspaceNew = "workspace.new", Workspaces, CmdWorkspaceNew;
    WorkspaceMcpServers = "workspace.mcpServers", Workspaces, CmdWorkspaceMcpServers;
    WorkspaceNewOrchestrator = "workspace.newOrchestrator", Workspaces, CmdWorkspaceNewOrchestrator;
    WorkspaceOrchestration = "workspace.orchestration", Workspaces, CmdWorkspaceOrchestration;
    WorkspaceStopWorkers = "workspace.stopWorkers", Workspaces, CmdWorkspaceStopWorkers;
    WorkspaceSleep = "workspace.sleep", Workspaces, CmdWorkspaceSleep;
    WorkspaceGo1 = "workspace.go1", Workspaces, CmdWorkspaceGoN, 1;
    WorkspaceGo2 = "workspace.go2", Workspaces, CmdWorkspaceGoN, 2;
    WorkspaceGo3 = "workspace.go3", Workspaces, CmdWorkspaceGoN, 3;
    WorkspaceGo4 = "workspace.go4", Workspaces, CmdWorkspaceGoN, 4;
    WorkspaceGo5 = "workspace.go5", Workspaces, CmdWorkspaceGoN, 5;
    WorkspaceGo6 = "workspace.go6", Workspaces, CmdWorkspaceGoN, 6;
    WorkspaceGo7 = "workspace.go7", Workspaces, CmdWorkspaceGoN, 7;
    WorkspaceGo8 = "workspace.go8", Workspaces, CmdWorkspaceGoN, 8;
    WorkspaceGo9 = "workspace.go9", Workspaces, CmdWorkspaceGoN, 9;
    ViewSettings = "view.settings", View, CmdViewSettings;
    ViewShortcuts = "view.shortcuts", View, CmdViewShortcuts;
    ViewPalette = "view.palette", View, CmdViewPalette;
    ViewNavigator = "view.navigator", View, CmdViewNavigator;
    ViewInspector = "view.inspector", View, CmdViewInspector;
    ViewFocusNavigator = "view.focusNavigator", View, CmdViewFocusNavigator;
    ViewFocusInspector = "view.focusInspector", View, CmdViewFocusInspector;
    ViewNextRegion = "view.nextRegion", View, CmdViewNextRegion;
    ViewPrevRegion = "view.prevRegion", View, CmdViewPrevRegion;
    ViewTerminals = "view.terminals", View, CmdViewTerminals;
    ViewBoard = "view.board", View, CmdViewBoard;
    ViewBoardDock = "view.boardDock", View, CmdViewBoardDock;
    ViewModelPreview = "view.modelPreview", View, CmdViewModelPreview;
    ViewCloseModels = "view.closeModels", View, CmdViewCloseModels;
    ViewMemory = "view.memory", View, CmdViewMemory;
    ViewStats = "view.stats", View, CmdViewStats;
    ViewMcp = "view.mcp", View, CmdViewMcp;
    ViewNeedsYou = "view.needsYou", View, CmdViewNeedsYou;
    ViewBroadcast = "view.broadcast", View, CmdViewBroadcast;
    TerminalClear = "terminal.clear", Terminal, CmdTerminalClear;
    TerminalCopy = "terminal.copy", Terminal, CmdTerminalCopy;
    TerminalPaste = "terminal.paste", Terminal, CmdTerminalPaste;
    TerminalSelectAll = "terminal.selectAll", Terminal, CmdTerminalSelectAll;
    TerminalScrollTop = "terminal.scrollTop", Terminal, CmdTerminalScrollTop;
    TerminalScrollBottom = "terminal.scrollBottom", Terminal, CmdTerminalScrollBottom;
    TerminalFind = "terminal.find", Terminal, CmdTerminalFind;
    CommandRun1 = "command.run1", Commands, CmdCommandRunN, 1;
    CommandRun2 = "command.run2", Commands, CmdCommandRunN, 2;
    CommandRun3 = "command.run3", Commands, CmdCommandRunN, 3;
    CommandRun4 = "command.run4", Commands, CmdCommandRunN, 4;
    CommandRun5 = "command.run5", Commands, CmdCommandRunN, 5;
    CommandRun6 = "command.run6", Commands, CmdCommandRunN, 6;
    CommandRun7 = "command.run7", Commands, CmdCommandRunN, 7;
    CommandRun8 = "command.run8", Commands, CmdCommandRunN, 8;
    CommandRun9 = "command.run9", Commands, CmdCommandRunN, 9;
    AppZoomIn = "app.zoomIn", App, CmdAppZoomIn;
    AppZoomOut = "app.zoomOut", App, CmdAppZoomOut;
    AppZoomReset = "app.zoomReset", App, CmdAppZoomReset;
    AppFullscreen = "app.fullscreen", App, CmdAppFullscreen;
    VoiceDictate = "voice.dictate", Voice, CmdVoiceDictate;
}

impl CommandId {
    /// Whether the palette lists it: never the digit families, dictation
    /// (push to talk) or the region steps (§2.1/5).
    pub fn in_palette(self) -> bool {
        !(self
            .label_n()
            .is_some_and(|_| self.section() != Section::Commands)
            || matches!(
                self,
                CommandId::VoiceDictate | CommandId::ViewNextRegion | CommandId::ViewPrevRegion
            ))
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An id `settings.keybindings` names that this version does not know (kept
/// in the file, never run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownCommand(pub String);

impl fmt::Display for UnknownCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no command is called {}", self.0)
    }
}

impl std::error::Error for UnknownCommand {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn there_are_88_commands_each_with_its_own_id() {
        assert_eq!(CommandId::ALL.len(), 88);
        let ids: HashSet<&str> = CommandId::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(ids.len(), 88);
    }

    #[test]
    fn ids_read_back() {
        for command in CommandId::ALL {
            assert_eq!(command.as_str().parse::<CommandId>(), Ok(*command));
        }
        assert!("pane.explode".parse::<CommandId>().is_err());
    }

    #[test]
    fn sections_follow_the_settings_order() {
        // `COMMANDS` lists each section's commands together, in `SECTIONS`
        // order, except that dictation comes after the window's own keys.
        let order: Vec<Section> =
            CommandId::ALL
                .iter()
                .map(|c| c.section())
                .fold(Vec::new(), |mut seen, section| {
                    if seen.last() != Some(&section) {
                        seen.push(section);
                    }
                    seen
                });
        assert_eq!(
            order,
            [
                Section::Panes,
                Section::Navigation,
                Section::Workspaces,
                Section::View,
                Section::Terminal,
                Section::Commands,
                Section::App,
                Section::Voice
            ]
        );
    }

    #[test]
    fn the_palette_hides_the_digit_families_but_not_saved_commands() {
        assert!(!CommandId::PaneFocus3.in_palette());
        assert!(!CommandId::WorkspaceGo1.in_palette());
        assert!(!CommandId::VoiceDictate.in_palette());
        assert!(!CommandId::ViewNextRegion.in_palette());
        assert!(CommandId::CommandRun2.in_palette());
        assert!(CommandId::PaneSplit.in_palette());
    }
}
