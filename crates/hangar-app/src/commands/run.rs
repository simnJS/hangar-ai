//! Which commands are live, and what each one does (App.tsx `buildHandlers`,
//! the "Live when" column of inventory §1.10).
//!
//! A command that is not live is not a shortcut at all at that moment: the
//! keymap does not consume its key (it reaches the terminal), the palette does
//! not list it, and running it does nothing. That is why the pane commands
//! are only live when there is a pane to act on: Ctrl+Shift+X with no
//! workspace open should do nothing, not be eaten.
//!
//! Running one turns it into an `Intent`, a `StoreAction` or a shell action;
//! the palette (P7) lists `available()` and runs through `run()`, like a
//! shortcut. While the palette is open the dispatcher's only command is
//! `view.palette` (P7's rule); `available()` still answers the whole set, which
//! is what the palette lists.

use std::collections::HashSet;

use iced::{Task, window};

use super::CommandId;
use crate::message::{Intent, Message, TerminalAction};
use crate::orchestration_ui::actions::{can_create_orchestrator, pin_refusal};
use crate::shell::{Action, Overlay, SettingsCategory, View, WindowKind};
use crate::state::PaneId;
use crate::state::layout::{Direction, grid_tree, leaf_ids, neighbor_of};
use crate::state::roles::{orchestrator_of, workers_of};
use crate::state::types::DropZone;
use crate::state::types::{AgentId, SplitDir, SplitNode};
use crate::state::{Model, Patch, Settings, StoreAction, Workspace, folders::sidebar_order};

/// The bounds of the terminals' font size (the settings' slider).
pub const FONT_MIN: f64 = 9.0;
pub const FONT_MAX: f64 = 24.0;

/// What the commands of window `w` act on, worked out once.
struct Scope<'a> {
    main: bool,
    /// The workspaces in navigator order; none in a workspace window, which
    /// shows its own alone.
    workspaces: Vec<&'a Workspace>,
    /// The workspace the window shows.
    ws: Option<&'a Workspace>,
    view: View,
    /// The arrangement of its panes on screen (App.tsx's `tree`), terminals
    /// and views, which every pane command acts on. A workspace without a
    /// pane has none: `normalize_tree` answers it a lone empty leaf, a pane
    /// that does not exist for the commands to focus, restart or split.
    tree: Option<SplitNode>,
    /// The window's panes in reading order, terminals and views: Ctrl+1…9
    /// count every pane on screen.
    order: Vec<PaneId>,
    /// The focused pane, a terminal or a view.
    focused: Option<PaneId>,
    /// The focused pane is a view: what acts on a terminal, its agent or its
    /// shell is not about it.
    view_focused: bool,
}

impl<'a> Scope<'a> {
    fn of(m: &'a Model, w: window::Id) -> Scope<'a> {
        let main = !matches!(m.live.windows.get(&w), Some(WindowKind::Workspace(_)));
        let state = m.state();
        let ws = m.window_workspace(w);
        let tree = ws.filter(|ws| !ws.panes.is_empty()).map(grid_tree);
        let order = tree.as_ref().map_or_else(Vec::new, leaf_ids);
        let focused = ws.and_then(|ws| m.focused_leaf(ws)).cloned();
        let view_focused = ws
            .zip(focused.as_deref())
            .is_some_and(|(ws, focused)| ws.has_view(focused));
        Scope {
            main,
            workspaces: if main {
                sidebar_order(&state.workspaces, &state.folders)
            } else {
                Vec::new()
            },
            ws,
            view: m.view_in(w),
            tree,
            order,
            focused,
            view_focused,
        }
    }
}

/// The commands live in window `w` now, in keymap order.
pub fn available(m: &Model, w: window::Id) -> Vec<CommandId> {
    let live = live(m, w);
    CommandId::ALL
        .iter()
        .copied()
        .filter(|command| live.contains(command))
        .collect()
}

/// Whether `id` is live in window `w` now: what the dispatcher asks before
/// consuming a key.
pub fn is_available(m: &Model, w: window::Id, id: CommandId) -> bool {
    live(m, w).contains(&id)
}

/// `buildHandlers`, as the set of commands it would have built a handler for.
fn live(m: &Model, w: window::Id) -> HashSet<CommandId> {
    use CommandId::*;
    let scope = Scope::of(m, w);
    let settings = m.settings();
    let mut live: HashSet<CommandId> = [
        ViewSettings,
        ViewShortcuts,
        WorkspaceNew,
        ViewPalette,
        ViewInspector,
        ViewNextRegion,
        ViewPrevRegion,
        AppZoomIn,
        AppZoomOut,
        AppZoomReset,
        AppFullscreen,
    ]
    .into_iter()
    .collect();

    // Left out while dictation is off, so its key reaches the terminal
    // instead of being swallowed by a feature nobody enabled.
    if settings.voice_enabled {
        live.insert(VoiceDictate);
    }
    // A workspace window shows one workspace and no settings: those keys are
    // the main window's.
    if scope.main {
        live.insert(ViewNavigator);
        live.insert(ViewFocusNavigator);
    } else {
        live.remove(&ViewSettings);
        live.remove(&ViewShortcuts);
        live.remove(&WorkspaceNew);
    }

    live.extend(GO.iter().take(scope.workspaces.len()));
    if scope.workspaces.len() > 1 && scope.ws.is_some() {
        live.extend([WorkspaceNext, WorkspacePrev, WorkspaceSleep]);
    }

    let Some(ws) = scope.ws else {
        return live;
    };
    live.extend([
        ViewFocusInspector,
        ViewTerminals,
        ViewBoard,
        ViewBoardDock,
        ViewMemory,
        ViewStats,
        ViewMcp,
        WorkspaceMcpServers,
        WorkspaceOrchestration,
    ]);
    // Only while there is something to show or close: otherwise the keys
    // fall through, and the palette does not offer them.
    if !ws.view_panes.is_empty() {
        live.insert(ViewCloseModels);
    }
    if can_create_orchestrator(ws) {
        live.insert(WorkspaceNewOrchestrator);
    }
    if !workers_of(ws).is_empty() {
        live.insert(WorkspaceStopWorkers);
    }
    if m.live.signals.needs_you > 0 {
        live.insert(ViewNeedsYou);
    }

    // Everything below acts on panes, which are only on screen, and only
    // what a key can be about, in the terminals view, and only where there
    // are some (App.tsx: `view !== "terminals" || !tree`).
    if scope.view != View::Terminals || scope.tree.is_none() {
        return live;
    }
    live.extend([
        ViewBroadcast,
        PaneSplit,
        PaneSplitRight,
        PaneSplitDown,
        PaneRestartAll,
    ]);
    if ws.panes.iter().any(|pane| pane.agent != AgentId::Shell) {
        live.insert(PaneResetAll);
    }
    if ws.pane_count() > 1 {
        live.insert(PaneTidy);
    }
    live.extend(FOCUS.iter().take(scope.order.len()));
    if scope.order.len() > 1 {
        live.extend([PaneNext, PanePrev]);
    }

    let Some(focused) = scope.focused.as_deref() else {
        return live;
    };
    // A view moves, swaps, zooms and closes like any pane; nothing else of
    // a pane is about it, and its keys reach what it shows.
    let moves = [
        PaneFocusLeft,
        PaneFocusRight,
        PaneFocusUp,
        PaneFocusDown,
        PaneSwapLeft,
        PaneSwapRight,
        PaneSwapUp,
        PaneSwapDown,
    ];
    if scope.view_focused {
        live.insert(PaneClose);
        if ws.pane_count() > 1 {
            live.insert(PaneZoom);
        }
        live.extend(moves);
        return live;
    }
    live.insert(PaneRestart);
    if ws.panes.len() > 1 {
        live.insert(PaneClose);
    }
    if ws.pane_count() > 1 {
        live.insert(PaneZoom);
    }
    if ws
        .panes
        .iter()
        .find(|pane| pane.id == focused)
        .is_some_and(|pane| pane.agent.info().resumable)
    {
        live.insert(PaneSessions);
    }
    if pin_refusal(ws, focused).is_none() {
        live.insert(PanePinOrchestrator);
    }
    if orchestrator_of(ws).is_some_and(|pane| pane.id == focused) {
        live.insert(PaneUnpinOrchestrator);
    }
    let saved = ws.saved_commands.len() + settings.saved_commands.len();
    live.extend(RUN.iter().take(saved));
    live.extend(moves);
    live.extend([
        TerminalClear,
        TerminalSelectAll,
        TerminalScrollTop,
        TerminalScrollBottom,
        TerminalFind,
        TerminalCopy,
        TerminalPaste,
    ]);
    live
}

/// `pane.focus1`…`pane.focus9`: the pane at that place in reading order.
pub(crate) const FOCUS: [CommandId; 9] = [
    CommandId::PaneFocus1,
    CommandId::PaneFocus2,
    CommandId::PaneFocus3,
    CommandId::PaneFocus4,
    CommandId::PaneFocus5,
    CommandId::PaneFocus6,
    CommandId::PaneFocus7,
    CommandId::PaneFocus8,
    CommandId::PaneFocus9,
];

const RUN: [CommandId; 9] = [
    CommandId::CommandRun1,
    CommandId::CommandRun2,
    CommandId::CommandRun3,
    CommandId::CommandRun4,
    CommandId::CommandRun5,
    CommandId::CommandRun6,
    CommandId::CommandRun7,
    CommandId::CommandRun8,
    CommandId::CommandRun9,
];

/// What running `id` in window `w` does; nothing when it is not live.
pub fn run(m: &Model, w: window::Id, id: CommandId) -> Task<Message> {
    if !is_available(m, w, id) {
        return Task::none();
    }
    match message(m, w, id) {
        Some(message) => Task::done(message),
        None => Task::none(),
    }
}

/// The message `id` becomes, when it has something to act on.
fn message(m: &Model, w: window::Id, id: CommandId) -> Option<Message> {
    use CommandId::*;
    let scope = Scope::of(m, w);
    let intent = |intent| Some(Message::Intent(intent));
    let store = |action| Some(Message::Store(action));
    let zoom = |size: f64| {
        let size = size.clamp(FONT_MIN, FONT_MAX);
        store(StoreAction::UpdateSettings(Patch::new(
            move |s: &mut Settings| s.font_size = size,
        )))
    };
    let font = m.settings().font_size;

    // The commands that need no workspace.
    match id {
        // Through the intents every opening and closing of the sheet goes
        // through, so the settings package syncs with it: a sheet opened any
        // other way would not know it is open, and its first category click
        // would land back on General.
        ViewSettings => {
            let open = m
                .overlays_in(w)
                .iter()
                .any(|overlay| matches!(overlay, Overlay::Settings(_)));
            return if open {
                Some(Message::Shell(Action::CloseSettings))
            } else {
                intent(Intent::OpenSettings(None))
            };
        }
        ViewShortcuts => return intent(Intent::OpenSettings(Some(SettingsCategory::Shortcuts))),
        WorkspaceNew => return intent(Intent::OpenOverlay(Overlay::WorkspaceDialog)),
        // The same key closes it again, as in any editor.
        ViewPalette => return intent(Intent::TogglePalette),
        ViewInspector => return intent(Intent::ToggleInspector),
        ViewNextRegion => return intent(Intent::CycleRegion(1)),
        ViewPrevRegion => return intent(Intent::CycleRegion(-1)),
        AppZoomIn => return zoom(font + 1.0),
        AppZoomOut => return zoom(font - 1.0),
        AppZoomReset => return zoom(Settings::default().font_size),
        AppFullscreen => return Some(Message::Shell(Action::ToggleFullscreen(w))),
        VoiceDictate => return intent(Intent::Dictate),
        ViewNavigator => return intent(Intent::ToggleNavigator),
        ViewFocusNavigator => return intent(Intent::FocusNavigator),
        _ => {}
    }
    if let Some(n) = GO.iter().position(|go| *go == id) {
        return scope
            .workspaces
            .get(n)
            .and_then(|target| intent(Intent::ActivateWorkspace(target.id.clone())));
    }

    let ws = scope.ws?;
    let wid = ws.id.clone();
    let step_workspace = |delta: i64| {
        let list = &scope.workspaces;
        let at = list.iter().position(|entry| entry.id == ws.id).unwrap_or(0) as i64;
        let next = (at + delta).rem_euclid(list.len() as i64) as usize;
        intent(Intent::ActivateWorkspace(list[next].id.clone()))
    };
    match id {
        WorkspaceNext => return step_workspace(1),
        WorkspacePrev => return step_workspace(-1),
        WorkspaceSleep => return intent(Intent::Sleep(wid)),
        ViewFocusInspector => return intent(Intent::FocusInspector),
        ViewTerminals => return intent(Intent::SetView(View::Terminals)),
        ViewBoard => return intent(Intent::SetView(View::Board)),
        ViewBoardDock => return intent(Intent::ToggleDock),
        ViewCloseModels => return intent(Intent::ClosePreviews(wid)),
        ViewMemory => return intent(Intent::SetView(View::Memory)),
        ViewStats => return intent(Intent::SetView(View::Stats)),
        ViewMcp => return intent(Intent::OpenMcpInstall),
        WorkspaceMcpServers => return intent(Intent::OpenMcpServers(wid)),
        WorkspaceOrchestration => return intent(Intent::OpenOrchestration(wid)),
        WorkspaceNewOrchestrator => return intent(Intent::CreateOrchestrator(wid)),
        // Only arms the confirmation, like the reset.
        WorkspaceStopWorkers => return intent(Intent::ArmStopWorkers(true)),
        ViewNeedsYou => return Some(Message::Shell(Action::OpenNeedsYou)),
        ViewBroadcast => return intent(Intent::FocusRegion(crate::shell::Region::Status)),
        PaneSplit => {
            return intent(Intent::SplitPane {
                ws: wid,
                near: None,
                dir: None,
            });
        }
        PaneSplitRight => {
            return intent(Intent::SplitPane {
                ws: wid,
                near: None,
                dir: Some(SplitDir::Row),
            });
        }
        PaneSplitDown => {
            return intent(Intent::SplitPane {
                ws: wid,
                near: None,
                dir: Some(SplitDir::Col),
            });
        }
        PaneRestartAll => {
            return intent(Intent::RestartPanes {
                ws: wid,
                panes: ws.panes.iter().map(|pane| pane.id.clone()).collect(),
            });
        }
        // Only arms the confirmation: the key never throws conversations
        // away by itself.
        PaneResetAll => return intent(Intent::ArmResetAgents(true)),
        PaneTidy => return intent(Intent::Tidy(wid)),
        _ => {}
    }
    // Moving between the window's own panes is App.tsx's `focusPane` alone:
    // not the landing of `Intent::FocusPane`, which closes the overlays.
    let focus = |pane: &PaneId| {
        intent(Intent::SelectPane {
            ws: wid.clone(),
            pane: pane.clone(),
        })
    };
    if let Some(n) = FOCUS.iter().position(|command| *command == id) {
        return scope.order.get(n).and_then(focus);
    }
    if matches!(id, PaneNext | PanePrev) {
        let len = scope.order.len() as i64;
        let at = scope
            .focused
            .as_ref()
            .and_then(|focused| scope.order.iter().position(|pane| pane == focused))
            .unwrap_or(0) as i64;
        let delta = if id == PaneNext { 1 } else { -1 };
        return focus(&scope.order[(at + delta).rem_euclid(len) as usize]);
    }

    let focused = scope.focused.clone()?;
    let tree = scope.tree.as_ref()?;
    let toward = |dir: Direction| neighbor_of(tree, &focused, dir);
    let terminal = |action| {
        intent(Intent::TerminalAction {
            pane: focused.clone(),
            action,
        })
    };
    if let Some(n) = RUN.iter().position(|command| *command == id) {
        return intent(Intent::RunSavedCommand {
            ws: wid,
            from: focused.clone(),
            index: n,
        });
    }
    match id {
        PaneRestart => intent(Intent::ReplacePane {
            ws: wid,
            pane: focused.clone(),
            patch: Patch::none(),
        }),
        PaneClose if scope.view_focused => intent(Intent::CloseView {
            ws: wid,
            pane: focused.clone(),
        }),
        PaneClose => store(StoreAction::ClosePane {
            ws: wid,
            pane: focused.clone(),
        }),

        PaneZoom => intent(Intent::SetZoom {
            on: !m.ui.zoom_by_workspace.get(&wid).copied().unwrap_or(false),
            ws: wid,
        }),
        PaneSessions => intent(Intent::OpenSessions {
            ws: wid,
            pane: focused.clone(),
        }),
        PanePinOrchestrator | PaneUnpinOrchestrator => intent(Intent::PinOrchestrator {
            ws: wid,
            pane: focused.clone(),
            pinned: id == PanePinOrchestrator,
        }),
        PaneFocusLeft => toward(Direction::Left).as_ref().and_then(focus),
        PaneFocusRight => toward(Direction::Right).as_ref().and_then(focus),
        PaneFocusUp => toward(Direction::Up).as_ref().and_then(focus),
        PaneFocusDown => toward(Direction::Down).as_ref().and_then(focus),
        // Swapping is the drop-in-the-middle gesture, spelled with keys.
        PaneSwapLeft | PaneSwapRight | PaneSwapUp | PaneSwapDown => {
            let dir = match id {
                PaneSwapLeft => Direction::Left,
                PaneSwapRight => Direction::Right,
                PaneSwapUp => Direction::Up,
                _ => Direction::Down,
            };
            toward(dir).and_then(|target| {
                store(StoreAction::MovePane {
                    ws: wid,
                    drag: focused.clone(),
                    target,
                    zone: DropZone::Center,
                })
            })
        }
        TerminalClear => terminal(TerminalAction::Clear),
        TerminalSelectAll => terminal(TerminalAction::SelectAll),
        TerminalScrollTop => terminal(TerminalAction::ScrollTop),
        TerminalScrollBottom => terminal(TerminalAction::ScrollBottom),
        TerminalCopy => terminal(TerminalAction::Copy),
        TerminalPaste => terminal(TerminalAction::Paste),
        TerminalFind => intent(Intent::OpenFinder(focused.clone())),
        _ => None,
    }
}

const GO: [CommandId; 9] = [
    CommandId::WorkspaceGo1,
    CommandId::WorkspaceGo2,
    CommandId::WorkspaceGo3,
    CommandId::WorkspaceGo4,
    CommandId::WorkspaceGo5,
    CommandId::WorkspaceGo6,
    CommandId::WorkspaceGo7,
    CommandId::WorkspaceGo8,
    CommandId::WorkspaceGo9,
];

#[cfg(test)]
mod tests;
