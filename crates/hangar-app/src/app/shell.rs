//! The shell's own part of each intent, its window events and its actions
//! (App.tsx's handlers and effects, inventory §1.5–§1.11).
//!
//! Each window keeps its own view, overlays, drawer and focus, as each Tauri
//! window had its own App: the focused window's are `ui.view`, `ui.overlays`,
//! `ui.drawer` and `ui.focus`, the others wait in `ui.parked` and trade places
//! when the focus moves (`swap_focus`).

use iced::{Point, Size, Task, window};

use super::App;
use crate::message::{Intent, Message};
use crate::shell::drawer::{self, DrawerCloseReason};
use crate::shell::{
    Action, Drawer, Focus, Overlay, Panels, Region, View, WindowKind, frame, notice, regions,
    windows,
};
use crate::state::folders::sidebar_order;
use crate::state::layout::{compute_layout, normalize_tree, preferred_dir};
use crate::state::types::{AgentId, WindowBounds};
use crate::state::{PaneId, PanePatch, Patch, Settings, StoreAction, Workspace, WsId};

impl App {
    /// The shell's part of an intent. Packages get the intent after this.
    pub(super) fn own_intent(&mut self, intent: &Intent) -> Task<Message> {
        let w = self.focused_window();
        match intent.clone() {
            Intent::OpenOverlay(overlay) => self.open_overlay(overlay),
            Intent::CloseOverlay => return self.close_overlay(),
            Intent::TogglePalette => {
                if self.model.ui.overlays.last() == Some(&Overlay::Palette) {
                    return self.close_overlay();
                }
                self.open_overlay(Overlay::Palette);
            }
            // Opening always names a category, so a shortcut lands where it
            // promised; none is General (App.tsx `openSettings`).
            Intent::OpenSettings(category) => {
                self.open_overlay(Overlay::Settings(category.unwrap_or_default()))
            }
            Intent::OpenMcpInstall => self.open_overlay(Overlay::McpInstall),
            Intent::OpenMcpServers(ws) => self.open_overlay(Overlay::McpServers(ws)),
            Intent::OpenOrchestration(ws) => self.open_overlay(Overlay::Orchestration(ws)),
            Intent::OpenWorktrees(ws) => self.open_overlay(Overlay::Worktrees(ws)),
            Intent::OpenKitsDialog(ws) => self.open_overlay(Overlay::KitsDialog(ws)),
            Intent::OpenDiff { ws, pane } => self.open_overlay(Overlay::Diff { ws, pane }),
            Intent::OpenSessions { ws, pane } => {
                self.open_overlay(Overlay::SessionPicker { ws, pane })
            }
            Intent::OpenTask { cwd, task } => self.open_overlay(Overlay::Task { cwd, task }),
            Intent::SetView(view) => self.set_view(w, view),
            Intent::OpenBoard(ws) => {
                // A workspace window never moves the main window's selection.
                if self.model.live.windows.get(&w) != Some(&WindowKind::Workspace(ws.clone())) {
                    self.model.apply(StoreAction::SetActiveWorkspace(Some(ws)));
                }
                self.set_view(w, View::Board);
            }
            Intent::ActivateWorkspace(ws) => return self.activate_workspace(ws),
            Intent::FocusPane { ws, pane, terminal } => return self.focus_pane(ws, pane, terminal),
            Intent::ToggleNavigator => {
                self.update_settings(|s| s.navigator_open = !s.navigator_open)
            }
            Intent::FocusNavigator => {
                // Shown first when hidden: asking for its keys is asking to
                // see it.
                if !self.model.settings().navigator_open {
                    self.update_settings(|s| s.navigator_open = true);
                }
                return self.focus_region(w, Region::Navigator);
            }
            Intent::ToggleInspector => return self.toggle_inspector(w),
            Intent::FocusInspector => return self.focus_inspector(w),
            Intent::ToggleDock => return self.toggle_dock(w),
            Intent::CloseDock => return self.close_panel(w, Drawer::Dock),
            Intent::TogglePreview => return self.toggle_preview(w),
            Intent::ClosePreviewPanel => return self.close_panel(w, Drawer::Model),
            Intent::TogglePreviewExpand => {
                if let Some(ws) = self.window_ws(w) {
                    let panel = self.model.live.signals.preview.entry(ws).or_default();
                    panel.open = true;
                    panel.expanded = !panel.expanded;
                }
            }
            Intent::CycleRegion(step) => return self.cycle_region(w, step),
            Intent::FocusRegion(region) => return self.focus_region(w, region),
            Intent::SplitPane { ws, near, dir } => self.split_pane(w, ws, near, dir),
            Intent::ReplacePane { ws, pane, patch } => self.replace_pane(&ws, &pane, patch),
            Intent::RestartPanes { ws, panes } => {
                for pane in panes {
                    self.replace_pane(&ws, &pane, Patch::none());
                }
            }
            Intent::ArmResetAgents(armed) => self.model.ui.confirm_reset = armed,
            // Every agent on a brand new conversation: the respawn of a
            // restart, minus the session, so the launch has nothing to
            // resume (the transcripts stay, the picker can reopen them).
            Intent::ResetAgents(ws) => {
                let agents: Vec<PaneId> = self
                    .model
                    .workspace(&ws)
                    .map(|w| {
                        w.panes
                            .iter()
                            .filter(|pane| pane.agent != AgentId::Shell)
                            .map(|pane| pane.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for pane in agents {
                    self.replace_pane(
                        &ws,
                        &pane,
                        Patch::new(|p: &mut crate::state::Pane| p.session_id = None),
                    );
                }
                self.model.ui.confirm_reset = false;
            }
            // The focused pane is the model for whatever the preset makes.
            Intent::ApplyPreset { ws, size } => {
                let model_pane = self.model.ui.focus_by_workspace.get(&ws).cloned();
                self.model.apply(StoreAction::ApplyPreset {
                    ws,
                    count: usize::from(size),
                    model_pane,
                });
            }
            Intent::Tidy(ws) => {
                self.model.apply(StoreAction::TidyLayout(ws));
            }
            Intent::SetZoom { ws, on } => {
                self.model.ui.zoom_by_workspace.insert(ws, on);
            }
            Intent::Detach(ws) => return self.detach(ws),
            Intent::Reattach(ws) => return self.reattach(ws),
            Intent::ArmStopWorkers(armed) => {
                if armed {
                    self.model.ui.confirm_reset = false;
                }
                self.model.ui.confirm_stop_workers = armed;
            }
            Intent::StopWorkers(_) => self.model.ui.confirm_stop_workers = false,
            // A kit tab the person asked for goes to the terminals view, and
            // the preview comes over them as a drawer when there is no room
            // beside them, which an agent's file never does.
            Intent::OpenKitTab { .. } => {
                self.set_view(w, View::Terminals);
                self.kit_opened = true;
            }
            Intent::Notice(text) => return notice::show(&mut self.model, text),
            Intent::Notify(notification) => {
                return crate::services::notify::send(&self.services, notification);
            }
            // The packages' alone: ClosePopovers, Sleep, the orchestration
            // ones, Land, ShowPreview, OpenFinder, RunSavedCommand, Broadcast,
            // InsertTranscript, CreateWorkspaceFromGit, TerminalAction,
            // Dictate, ClosePreviews, PaneRespawned and what packages append.
            _ => {}
        }
        Task::none()
    }

    /// The shell's own messages.
    pub(super) fn shell_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::PointerOverDrawer(over) => self.model.ui.pointer.over_drawer = over,
            Action::PointerOverToolbar(over) => self.model.ui.pointer.over_toolbar = over,
            Action::Pressed(w) => {
                if self.model.is_focused_window(w) && drawer::press_closes(&self.model.ui) {
                    return self.close_drawer(DrawerCloseReason::Outside);
                }
            }
            Action::OpenNeedsYou => self.model.ui.needs_you_open = true,
            Action::ToggleSettings => {
                let ui = &mut self.model.ui;
                if let Some(at) = ui
                    .overlays
                    .iter()
                    .rposition(|overlay| matches!(overlay, Overlay::Settings(_)))
                {
                    ui.overlays.remove(at);
                    if at < ui.overlay_openers.len() {
                        ui.overlay_openers.remove(at);
                    }
                } else {
                    self.open_overlay(Overlay::Settings(Default::default()));
                }
            }
            Action::ToggleFullscreen(w) => {
                return window::mode(w).then(move |mode| {
                    window::set_mode(
                        w,
                        if mode == window::Mode::Fullscreen {
                            window::Mode::Windowed
                        } else {
                            window::Mode::Fullscreen
                        },
                    )
                });
            }
            Action::OpenWorkspaceWindow { ws, screen } => {
                return self.open_workspace_window(ws, screen);
            }
            Action::BoundsSettled(w, seq) => {
                if self.bounds_seq.get(&w) == Some(&seq) {
                    self.remember_bounds(w);
                }
            }
        }
        Task::none()
    }

    /// The window's events the shell answers.
    pub(super) fn window_event(&mut self, id: window::Id, event: window::Event) -> Task<Message> {
        let scale = self.model.settings().scale_factor();
        match event {
            window::Event::CloseRequested if id == self.main_window => self.quit(),
            // A workspace window's close button brings the workspace back to
            // the main window rather than closing anything.
            window::Event::CloseRequested => match self.model.live.windows.get(&id).cloned() {
                Some(WindowKind::Workspace(ws)) => self.intent(Intent::Reattach(ws)),
                _ => window::close(id),
            },
            window::Event::Closed => {
                let kind = self.model.live.windows.remove(&id);
                self.model.ui.parked.remove(&id);
                self.model.ui.body_width.remove(&id);
                self.window_sizes.remove(&id);
                self.window_positions.remove(&id);
                self.bounds_seq.remove(&id);
                if self.model.ui.focused_window == Some(id) {
                    self.model.ui.focused_window = None;
                    self.load_window_ui(self.main_window);
                }
                // Closed some other way than by its close button (the system
                // ending it): its workspace is not left drawn nowhere.
                if let Some(WindowKind::Workspace(ws)) = kind
                    && self
                        .model
                        .workspace(&ws)
                        .is_some_and(|w| w.detached == Some(true))
                {
                    self.model.apply(StoreAction::UpdateWorkspace(
                        ws,
                        Patch::new(|w: &mut Workspace| w.detached = Some(false)),
                    ));
                }
                Task::none()
            }
            window::Event::Opened { size, position } => {
                self.window_sizes.insert(id, size * scale);
                if let Some(at) = position {
                    self.window_positions
                        .insert(id, Point::new(at.x * scale, at.y * scale));
                }
                Task::none()
            }
            window::Event::Resized(size) => {
                self.window_sizes.insert(id, size * scale);
                self.bounds_moved(id)
            }
            window::Event::Moved(at) => {
                self.window_positions
                    .insert(id, Point::new(at.x * scale, at.y * scale));
                self.bounds_moved(id)
            }
            window::Event::Focused => {
                self.swap_focus(id);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The focus moves to window `to`: the window it leaves parks its view,
    /// overlays, drawer and focus, and `to` takes its own back.
    fn swap_focus(&mut self, to: window::Id) {
        let from = self.focused_window();
        if from == to {
            self.model.ui.focused_window = Some(to);
            return;
        }
        let ui = &mut self.model.ui;
        let leaving = crate::state::WindowUi {
            view: ui.view,
            overlays: std::mem::take(&mut ui.overlays),
            overlay_openers: std::mem::take(&mut ui.overlay_openers),
            drawer: ui.drawer.take(),
            drawer_opener: ui.drawer_opener.take(),
            focus: ui.focus.take(),
        };
        ui.parked.insert(from, leaving);
        ui.pointer = Default::default();
        ui.focused_window = Some(to);
        self.load_window_ui(to);
    }

    fn load_window_ui(&mut self, w: window::Id) {
        let ui = &mut self.model.ui;
        let mine = ui.parked.remove(&w).unwrap_or_default();
        ui.view = mine.view;
        ui.overlays = mine.overlays;
        ui.overlay_openers = mine.overlay_openers;
        ui.drawer = mine.drawer;
        ui.drawer_opener = mine.drawer_opener;
        ui.focus = mine.focus;
    }

    /// What follows a change (App.tsx's effects), run after every message.
    pub(super) fn follow_changes(&mut self) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();

        // Every respawn, whoever applied it, is announced.
        for respawn in self.model.take_respawns() {
            tasks.push(Task::done(Message::Intent(Intent::PaneRespawned {
                ws: respawn.ws,
                from: respawn.from,
                to: respawn.to,
                keeps_session: respawn.keeps_session,
            })));
        }

        // Workspaces stay opened once visited, so their panes live on while
        // hidden; nothing spawns for one never opened.
        let active = self.model.state().active_workspace_id.clone();
        if let Some(id) = &active
            && !self.model.ui.opened_ids.contains(id)
        {
            self.model.ui.opened_ids.push(id.clone());
        }
        if active != self.last_active {
            // A preview drawer of one workspace does not come over the next
            // one's terminals.
            if self.model.ui.drawer == Some(Drawer::Model) {
                self.model.ui.drawer = None;
            }
            // Picking a detached workspace brings its window forward.
            if let Some(ws) = active.as_ref().and_then(|id| self.model.workspace(id))
                && ws.detached == Some(true)
                && let Some(w) = self.model.window_of(&ws.id)
                && w != self.main_window
            {
                tasks.push(window::gain_focus(w));
            }
        }
        // An armed confirmation is about the workspace and the view it was
        // armed in.
        if active != self.last_active || self.model.ui.view != self.last_view {
            self.model.ui.confirm_reset = false;
            self.model.ui.confirm_stop_workers = false;
        }
        self.last_active = active;
        self.last_view = self.model.ui.view;

        // Keep the focus on a pane that still exists after a layout change.
        let shown: Vec<(WsId, PaneId)> = self
            .model
            .live
            .windows
            .keys()
            .filter_map(|w| self.model.window_workspace(*w))
            .filter(|ws| self.model.focused_pane(ws).is_none())
            .filter_map(|ws| {
                ws.panes
                    .first()
                    .map(|pane| (ws.id.clone(), pane.id.clone()))
            })
            .collect();
        for (ws, pane) in shown {
            self.model.ui.focus_by_workspace.insert(ws, pane);
        }

        // Nothing left in Needs-you: its list has nothing to show.
        if self.model.live.signals.needs_you == 0 {
            self.model.ui.needs_you_open = false;
        }

        self.measure_windows();
        self.settle_drawers();

        // A workspace window whose workspace was deleted closes; its panes
        // went with the workspace.
        let orphans: Vec<window::Id> = self
            .model
            .live
            .windows
            .iter()
            .filter(|(_, kind)| match kind {
                WindowKind::Workspace(ws) => self.model.workspace(ws).is_none(),
                WindowKind::Main => false,
            })
            .map(|(id, _)| *id)
            .collect();
        tasks.extend(orphans.into_iter().map(window::close));
        tasks
    }

    /// Each window's body width, for the side panels (`body_width` in layout
    /// units, measured from the window less the navigator).
    fn measure_windows(&mut self) {
        let scale = self.model.settings().scale_factor();
        let navigator = self.model.settings().navigator_open;
        let widths: Vec<(window::Id, f32)> = self
            .model
            .live
            .windows
            .iter()
            .filter_map(|(id, kind)| {
                let size = self.window_sizes.get(id)?;
                Some((
                    *id,
                    frame::body_width(size.width / scale, navigator && *kind == WindowKind::Main),
                ))
            })
            .collect();
        for (id, width) in widths {
            self.model.ui.body_width.insert(id, width);
        }
    }

    /// Drawers the windows' changes ended (`drawer::settle`), and what a
    /// shown preview has seen.
    fn settle_drawers(&mut self) {
        let windows: Vec<window::Id> = self.model.live.windows.keys().copied().collect();
        for w in windows {
            let panels = Panels::of(&self.model, w);
            let view = self.model.view_in(w);
            if self.model.is_focused_window(w) {
                // The person asked for a kit tab: without room, the preview
                // comes as a drawer, once the tab is there.
                if self.kit_opened && panels.models_open {
                    self.kit_opened = false;
                    if panels.model.drawer && !panels.model_expanded && view == View::Terminals {
                        drawer::open(&mut self.model.ui, Drawer::Model);
                    }
                }
                let panels = Panels::of(&self.model, w);
                let settled = drawer::settle(self.model.ui.drawer, &panels, view);
                if settled != self.model.ui.drawer {
                    self.model.ui.drawer = settled;
                    self.model.ui.pointer.over_drawer = false;
                }
            } else if let Some(parked) = self.model.ui.parked.get_mut(&w) {
                parked.drawer = drawer::settle(parked.drawer, &panels, view);
            }
            let panels = Panels::of(&self.model, w);
            if panels.model.shown
                && let Some(ws) = self.model.window_workspace(w).map(|ws| ws.id.clone())
                && let Some(panel) = self.model.live.signals.preview.get_mut(&ws)
            {
                panel.unseen = false;
            }
        }
    }

    // Overlays

    fn open_overlay(&mut self, overlay: Overlay) {
        let ui = &mut self.model.ui;
        ui.overlay_openers.resize(ui.overlays.len(), None);
        if ui.overlays.last() == Some(&overlay) {
            return;
        }
        let opener = ui.focus.clone();
        ui.overlays.push(overlay);
        ui.overlay_openers.push(opener);
    }

    /// Closes the top overlay, and gives the keyboard back to what had it.
    fn close_overlay(&mut self) -> Task<Message> {
        let ui = &mut self.model.ui;
        ui.overlay_openers.resize(ui.overlays.len(), None);
        if ui.overlays.pop().is_none() {
            return Task::none();
        }
        match ui.overlay_openers.pop().flatten() {
            Some(focus) => self.restore_focus(focus),
            None => Task::none(),
        }
    }

    // Workspaces and panes

    fn activate_workspace(&mut self, ws: WsId) -> Task<Message> {
        self.model
            .apply(StoreAction::SetActiveWorkspace(Some(ws.clone())));
        // Shown in a window of its own: that window comes forward.
        match self.model.workspace(&ws) {
            Some(w) if w.detached == Some(true) => match self.model.window_of(&ws) {
                Some(id) if id != self.main_window => window::gain_focus(id),
                _ => Task::none(),
            },
            _ => Task::none(),
        }
    }

    /// App.tsx's `activateRef`: lands on a pane, as a notification's click,
    /// the board's assignee links or Needs-you do.
    fn focus_pane(&mut self, ws: WsId, pane: PaneId, terminal: bool) -> Task<Message> {
        // The model, not a render: an agent that just made the pane asks for
        // it before anything has drawn it.
        let Some(workspace) = self.model.workspace(&ws) else {
            // Deleted since: the raised window is all it gets.
            return Task::none();
        };
        let exists = workspace.panes.iter().any(|p| p.id == pane);
        let detached = workspace.detached == Some(true);
        let w = if detached {
            self.model.window_of(&ws).unwrap_or(self.main_window)
        } else {
            // Anything drawn over the terminals goes: the settings are a full
            // sheet, and a click that promised a pane must not land on one.
            if self.model.is_focused_window(self.main_window) {
                self.model.ui.overlays.clear();
                self.model.ui.overlay_openers.clear();
            } else if let Some(parked) = self.model.ui.parked.get_mut(&self.main_window) {
                parked.overlays.clear();
                parked.overlay_openers.clear();
            }
            self.model.ui.needs_you_open = false;
            self.model
                .apply(StoreAction::SetActiveWorkspace(Some(ws.clone())));
            self.main_window
        };
        self.set_view(w, View::Terminals);
        // A pane that is gone leaves the focus alone: the next update moves
        // it onto one that still exists.
        if !exists {
            return Task::none();
        }
        self.model.ui.focus_by_workspace.insert(ws, pane.clone());
        let mut tasks = Vec::new();
        if terminal {
            let focus = Focus::Terminal(pane);
            if self.model.is_focused_window(w) {
                tasks.push(self.leave_drawer_for(Region::Main));
                self.model.ui.focus = Some(focus);
            } else if let Some(parked) = self.model.ui.parked.get_mut(&w) {
                parked.focus = Some(focus);
            }
        }
        if detached && w != self.main_window && !self.model.is_focused_window(w) {
            tasks.push(window::gain_focus(w));
        }
        Task::batch(tasks)
    }

    /// Splits a pane (`splitPane`): `near` or the focused one, along `dir` or
    /// the pane's longer side, and focuses the new pane.
    fn split_pane(
        &mut self,
        w: window::Id,
        ws: WsId,
        near: Option<PaneId>,
        dir: Option<crate::state::SplitDir>,
    ) {
        let near = near.or_else(|| self.model.ui.focus_by_workspace.get(&ws).cloned());
        let dir = dir.unwrap_or_else(|| preferred_dir(self.pane_size(w, &ws, near.as_deref())));
        let created = self
            .model
            .apply(StoreAction::AddPane {
                ws: ws.clone(),
                agent: None,
                near,
                dir: Some(dir),
                cwd: None,
                name: None,
                init: None,
            })
            .pane;
        if let Some(created) = created {
            self.model.ui.focus_by_workspace.insert(ws, created.clone());
            if self.model.is_focused_window(w) {
                self.model.ui.focus = Some(Focus::Terminal(created));
            }
        }
    }

    /// A pane's size on screen, from its share of the grid and the work
    /// area's size: what `preferredDir` measured in the DOM.
    fn pane_size(&self, w: window::Id, ws: &str, pane: Option<&str>) -> Option<(f32, f32)> {
        let workspace = self.model.workspace(ws)?;
        let tree = normalize_tree(&workspace.panes, workspace.tree.as_ref());
        let rect = *compute_layout(&tree).rects.get(pane?)?;
        let scale = self.model.settings().scale_factor();
        let window = self.window_sizes.get(&w)?;
        let panels = Panels::of(&self.model, w);
        let docked = |slot: crate::shell::side_panels::Slot| {
            if slot.shown && !slot.drawer {
                slot.width
            } else {
                0.0
            }
        };
        let width = self
            .model
            .ui
            .body_width
            .get(&w)
            .copied()
            .unwrap_or(window.width / scale)
            - docked(panels.inspector)
            - docked(panels.dock)
            - docked(panels.model);
        let height = window.height / scale
            - hangar_theme::tokens::size::TOOLBAR_HEIGHT
            - hangar_theme::tokens::size::STATUS_BAR_HEIGHT;
        Some((rect.width as f32 * width, rect.height as f32 * height))
    }

    /// A respawn that keeps the focus on the pane (`replacePane`).
    fn replace_pane(&mut self, ws: &str, pane: &str, patch: PanePatch) {
        let next = self
            .model
            .apply(StoreAction::RespawnPane {
                ws: ws.into(),
                pane: pane.into(),
                patch,
            })
            .pane;
        if let Some(next) = next
            && self.model.ui.focus_by_workspace.get(ws).map(String::as_str) == Some(pane)
        {
            self.model
                .ui
                .focus_by_workspace
                .insert(ws.into(), next.clone());
            if self.model.ui.focus == Some(Focus::Terminal(pane.into())) {
                self.model.ui.focus = Some(Focus::Terminal(next));
            }
        }
    }

    fn set_view(&mut self, w: window::Id, view: View) {
        if self.model.is_focused_window(w) {
            self.model.ui.view = view;
        } else {
            self.model.ui.parked.entry(w).or_default().view = view;
        }
    }

    fn update_settings(&mut self, edit: impl Fn(&mut Settings) + Send + Sync + 'static) {
        self.model
            .apply(StoreAction::UpdateSettings(Patch::new(edit)));
    }

    /// The workspace window `w` shows, by id.
    fn window_ws(&self, w: window::Id) -> Option<WsId> {
        self.model.window_workspace(w).map(|ws| ws.id.clone())
    }

    // Side panels and drawers (App.tsx:541–686)

    /// Without room, the inspector's toggle opens or closes its drawer and
    /// leaves the setting alone.
    fn toggle_inspector(&mut self, w: window::Id) -> Task<Message> {
        let panels = Panels::of(&self.model, w);
        if panels.inspector.drawer {
            return self.toggle_drawer(w, Drawer::Inspector);
        }
        let closing = self.model.settings().inspector_open;
        self.update_settings(|s| s.inspector_open = !s.inspector_open);
        if closing {
            return self.after_docked_close(Region::Inspector);
        }
        Task::none()
    }

    /// Shown first when hidden; a drawer takes the keyboard as it opens.
    fn focus_inspector(&mut self, w: window::Id) -> Task<Message> {
        let panels = Panels::of(&self.model, w);
        if panels.inspector.drawer && self.model.drawer_in(w) != Some(Drawer::Inspector) {
            return self.open_drawer(w, Drawer::Inspector);
        }
        if !panels.inspector.shown {
            self.update_settings(|s| s.inspector_open = true);
        }
        self.focus_region(w, Region::Inspector)
    }

    fn toggle_dock(&mut self, w: window::Id) -> Task<Message> {
        let panels = Panels::of(&self.model, w);
        if self.model.view_in(w) != View::Terminals {
            self.set_view(w, View::Terminals);
            self.update_settings(|s| s.board_dock_open = true);
            if Panels::of(&self.model, w).dock.drawer {
                return self.open_drawer(w, Drawer::Dock);
            }
            return Task::none();
        }
        if panels.dock.drawer {
            return self.toggle_drawer(w, Drawer::Dock);
        }
        let closing = self.model.settings().board_dock_open;
        self.update_settings(|s| s.board_dock_open = !s.board_dock_open);
        if closing {
            return self.after_docked_close(Region::Dock);
        }
        Task::none()
    }

    /// The preview's toolbar button: docked when there is room and it is
    /// open, a drawer the person opens otherwise. An agent showing something
    /// never throws a drawer over the terminals someone is typing in.
    fn toggle_preview(&mut self, w: window::Id) -> Task<Message> {
        let Some(ws) = self.window_ws(w) else {
            return Task::none();
        };
        if self.model.view_in(w) != View::Terminals {
            self.set_view(w, View::Terminals);
            self.model.live.signals.preview.entry(ws).or_default().open = true;
            if Panels::of(&self.model, w).model.drawer {
                return self.open_drawer(w, Drawer::Model);
            }
            return Task::none();
        }
        let panels = Panels::of(&self.model, w);
        let panel = self.model.live.signals.preview.entry(ws).or_default();
        if panels.model_expanded {
            panel.open = false;
            return Task::none();
        }
        if panels.model.drawer {
            return self.toggle_drawer(w, Drawer::Model);
        }
        panel.open = !panel.open;
        if !panel.open {
            return self.after_docked_close(Region::Model);
        }
        Task::none()
    }

    /// A panel's own close: its drawer, else its docked setting.
    fn close_panel(&mut self, w: window::Id, which: Drawer) -> Task<Message> {
        let panels = Panels::of(&self.model, w);
        let slot = panels.slot(which);
        if slot.drawer
            && self.model.drawer_in(w) == Some(which)
            && !(which == Drawer::Model && panels.model_expanded)
        {
            return self.close_drawer(DrawerCloseReason::Toggle);
        }
        match which {
            Drawer::Inspector => self.update_settings(|s| s.inspector_open = false),
            Drawer::Dock => self.update_settings(|s| s.board_dock_open = false),
            Drawer::Model => {
                if let Some(ws) = self.window_ws(w) {
                    self.model.live.signals.preview.entry(ws).or_default().open = false;
                }
            }
        }
        self.after_docked_close(which.region())
    }

    fn toggle_drawer(&mut self, w: window::Id, which: Drawer) -> Task<Message> {
        if self.model.drawer_in(w) == Some(which) {
            self.close_drawer(DrawerCloseReason::Toggle)
        } else {
            self.open_drawer(w, which)
        }
    }

    fn open_drawer(&mut self, w: window::Id, which: Drawer) -> Task<Message> {
        if !self.model.is_focused_window(w) {
            return Task::none();
        }
        let region = drawer::open(&mut self.model.ui, which);
        self.enter_region(region).unwrap_or_else(Task::none)
    }

    /// Closes the focused window's drawer for `reason`, giving the keyboard
    /// back as `drawer::close` says.
    pub(super) fn close_drawer(&mut self, reason: DrawerCloseReason) -> Task<Message> {
        match drawer::close(&mut self.model.ui, reason) {
            Some(focus) => self.restore_focus(focus),
            None => Task::none(),
        }
    }

    /// A docked panel closed with the keyboard in it: the keyboard goes to
    /// the panes, not nowhere.
    fn after_docked_close(&mut self, region: Region) -> Task<Message> {
        if self.model.ui.focus == Some(Focus::Region(region)) {
            return self.restore_focus(Focus::Region(Region::Main));
        }
        Task::none()
    }

    /// The keyboard moving into `region` while a drawer is open elsewhere is
    /// leaving the drawer.
    fn leave_drawer_for(&mut self, region: Region) -> Task<Message> {
        match self.model.ui.drawer {
            Some(open) if open.region() != region => {
                drawer::close(&mut self.model.ui, DrawerCloseReason::Outside);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    // Regions and focus (inventory §1.8)

    /// F6 and Shift+F6 in window `w`: a dialog keeps the keyboard; a region
    /// with nothing to focus is stepped over.
    fn cycle_region(&mut self, w: window::Id, step: i8) -> Task<Message> {
        if !self.model.overlays_in(w).is_empty() || !self.model.is_focused_window(w) {
            return Task::none();
        }
        let panels = Panels::of(&self.model, w);
        let shown = regions::shown(&panels, self.model.window_workspace(w).is_some());
        let mut current = self.model.ui.focus.as_ref().and_then(Focus::region);
        for _ in 0..shown.len() {
            let Some(next) = regions::next_region(&shown, current, step) else {
                return Task::none();
            };
            let leave = self.leave_drawer_for(next);
            if let Some(task) = self.enter_region(next) {
                return Task::batch([leave, task]);
            }
            current = Some(next);
        }
        Task::none()
    }

    /// The keyboard to `region` in window `w`, as its owner says.
    fn focus_region(&mut self, w: window::Id, region: Region) -> Task<Message> {
        if !self.model.is_focused_window(w) {
            return Task::none();
        }
        let leave = self.leave_drawer_for(region);
        let enter = self.enter_region(region).unwrap_or_else(Task::none);
        Task::batch([leave, enter])
    }

    /// Asks the region's owner for its entry. `None` when the region has
    /// nothing to focus; a side panel always takes the keyboard as a whole,
    /// as the TS focused the panel itself.
    fn enter_region(&mut self, region: Region) -> Option<Task<Message>> {
        let p = &self.pkg;
        let m = &self.model;
        let entry = match region {
            Region::Navigator => crate::navigator::focus_entry(&p.navigator, m, region),
            Region::Toolbar => crate::toolbar::focus_entry(&p.toolbar, m, region),
            Region::Main => match m.ui.view {
                View::Terminals => crate::panes::focus_entry(&p.panes, m, region),
                View::Board => crate::board::focus_entry(&p.board, m, region),
                View::Memory => crate::memory::focus_entry(&p.memory, m, region),
                View::Stats => crate::stats::focus_entry(&p.stats, m, region),
            },
            Region::Model => crate::preview::focus_entry(&p.preview, m, region),
            Region::Dock => crate::board::focus_entry(&p.board, m, region),
            Region::Inspector => crate::inspector::focus_entry(&p.inspector, m, region),
            Region::Status => crate::status_bar::focus_entry(&p.status_bar, m, region),
        };
        if entry.is_none() && !region.is_panel() {
            return None;
        }
        self.model.ui.focus = Some(match (region, self.model.ui.view) {
            (Region::Main, View::Terminals) => {
                match self
                    .model
                    .focused_window()
                    .and_then(|w| self.model.window_workspace(w))
                {
                    Some(ws) => match self.model.focused_pane(ws) {
                        Some(pane) => Focus::Terminal(pane.id.clone()),
                        None => Focus::Region(region),
                    },
                    None => Focus::Region(region),
                }
            }
            _ => Focus::Region(region),
        });
        Some(entry.unwrap_or_else(Task::none))
    }

    /// Gives the keyboard back to `focus`: a terminal through the panes'
    /// entry (the focused pane's terminal), a region through its owner, a
    /// widget by its id.
    fn restore_focus(&mut self, focus: Focus) -> Task<Message> {
        match focus {
            Focus::Terminal(_) => self.enter_region(Region::Main).unwrap_or_else(Task::none),
            Focus::Region(region) => self.enter_region(region).unwrap_or_else(Task::none),
            Focus::Widget(id) => {
                self.model.ui.focus = Some(Focus::Widget(id.clone()));
                iced::widget::operation::focus(id)
            }
        }
    }

    // Workspace windows (inventory §1.7)

    /// Main window: sends a workspace to a window of its own. Its panes keep
    /// running: they live in the model, not in a window.
    fn detach(&mut self, ws: WsId) -> Task<Message> {
        let Some(workspace) = self.model.workspace(&ws) else {
            return Task::none();
        };
        if workspace.detached == Some(true) {
            return Task::none();
        }
        self.model.apply(StoreAction::UpdateWorkspace(
            ws.clone(),
            Patch::new(|w: &mut Workspace| w.detached = Some(true)),
        ));
        // The main window moves on to a workspace it can still show.
        let state = self.model.state();
        let next = sidebar_order(&state.workspaces, &state.folders)
            .into_iter()
            .find(|entry| entry.id != ws && entry.detached != Some(true))
            .map(|entry| entry.id.clone());
        if let Some(next) = next {
            self.model
                .apply(StoreAction::SetActiveWorkspace(Some(next)));
        }
        self.ask_workspace_window(ws)
    }

    /// Either window: brings a workspace back into the main window, and
    /// closes its own.
    fn reattach(&mut self, ws: WsId) -> Task<Message> {
        if self.model.workspace(&ws).is_some() {
            self.model.apply(StoreAction::UpdateWorkspace(
                ws.clone(),
                Patch::new(|w: &mut Workspace| w.detached = Some(false)),
            ));
            self.model
                .apply(StoreAction::SetActiveWorkspace(Some(ws.clone())));
        }
        self.opening.remove(&ws);
        let own = self
            .model
            .live
            .windows
            .iter()
            .find(|(_, kind)| **kind == WindowKind::Workspace(ws.clone()))
            .map(|(id, _)| *id);
        match own {
            Some(id) => Task::batch([window::close(id), window::gain_focus(self.main_window)]),
            None => Task::none(),
        }
    }

    /// Workspace `ws`'s window: brought forward when it is open, else opened
    /// once the main window's screen is known (`windows::workspace_settings_at`).
    pub(super) fn ask_workspace_window(&mut self, ws: WsId) -> Task<Message> {
        if let Some(id) = self
            .model
            .live
            .windows
            .iter()
            .find(|(_, kind)| **kind == WindowKind::Workspace(ws.clone()))
            .map(|(id, _)| *id)
        {
            return window::gain_focus(id);
        }
        if !self.opening.insert(ws.clone()) {
            return Task::none();
        }
        window::monitor_size(self.main_window).map(move |screen| {
            Message::Shell(Action::OpenWorkspaceWindow {
                ws: ws.clone(),
                screen,
            })
        })
    }

    fn open_workspace_window(&mut self, ws: WsId, screen: Option<Size>) -> Task<Message> {
        if !self.opening.remove(&ws) {
            return Task::none();
        }
        // Brought back, or gone, while the screen was being asked about.
        let Some(workspace) = self
            .model
            .workspace(&ws)
            .filter(|w| w.detached == Some(true))
        else {
            return Task::none();
        };
        let settings = windows::workspace_settings_at(
            workspace.window_bounds,
            screen,
            self.model.settings().scale_factor(),
        );
        let (id, open) = windows::open_workspace(settings);
        self.model
            .live
            .windows
            .insert(id, WindowKind::Workspace(ws));
        Task::batch([open, window::gain_focus(id)])
    }

    /// A workspace window moved or was resized: its bounds are written once
    /// it has been still for `BOUNDS_SETTLE`.
    fn bounds_moved(&mut self, w: window::Id) -> Task<Message> {
        if !matches!(
            self.model.live.windows.get(&w),
            Some(WindowKind::Workspace(_))
        ) {
            return Task::none();
        }
        let seq = self.bounds_seq.get(&w).map_or(1, |seq| seq + 1);
        self.bounds_seq.insert(w, seq);
        Task::perform(tokio::time::sleep(windows::BOUNDS_SETTLE), move |()| {
            Message::Shell(Action::BoundsSettled(w, seq))
        })
    }

    fn remember_bounds(&mut self, w: window::Id) {
        let Some(WindowKind::Workspace(ws)) = self.model.live.windows.get(&w).cloned() else {
            return;
        };
        let Some(size) = self.window_sizes.get(&w).copied() else {
            return;
        };
        let before = self.model.workspace(&ws).and_then(|w| w.window_bounds);
        let bounds: WindowBounds =
            windows::bounds_of(self.window_positions.get(&w).copied(), size, 1.0, before);
        if before != Some(bounds) {
            self.model.apply(StoreAction::UpdateWorkspace(
                ws,
                Patch::new(move |w: &mut Workspace| w.window_bounds = Some(bounds)),
            ));
        }
    }
}
