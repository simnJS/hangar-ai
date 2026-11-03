//! The application: boot, the router, the views of each window, the
//! subscriptions (inventory §8.2).
//!
//! The router is the only place that knows every package. It dispatches each
//! package's messages to that package's `update`, offers every intent and
//! every core event to each package's `on_intent` and `on_event` after its
//! own handling, and batches every package's subscription. The list of
//! packages is written once, in `packages!` below; a package never calls
//! another package's `update` (PACKAGES.md, rule 4).

use std::sync::Arc;
use std::time::Instant;

use iced::widget::Space;
use iced::{Element, Subscription, Task, keyboard, window};
use tokio::sync::watch;

use hangar_i18n::Translator;

use crate::commands;
use crate::ctx::{Ctx, Metrics};
use crate::message::{Intent, Message};
use crate::services::events::Bus;
use crate::services::inbox::InboxReceiver;
use crate::services::{self, CoreEvent, CoreHandles, Services};
use crate::shell::{self, Overlay, Region, View, WindowKind, notice, windows};
use crate::state::{Model, Patch, Settings, Snapshot, StoreAction, persist};

/// Every package, once: its directory (module and field name) and its route
/// in `Message`. The callback macro gets the whole list.
macro_rules! packages {
    ($callback:ident) => {
        $callback! {
            panes_logic: PanesLogic,
            panes: Panes,
            navigator: Navigator,
            toolbar: Toolbar,
            status_bar: StatusBar,
            needs_you: NeedsYou,
            update_banner: UpdateBanner,
            voice: Voice,
            keymap: Keymap,
            palette: Palette,
            saved_commands: SavedCommands,
            session_picker: SessionPicker,
            pane_menu: PaneMenu,
            workspace_dialog: WorkspaceDialog,
            git_flow: GitFlow,
            consent_card: Consent,
            accounts: Accounts,
            inspector: Inspector,
            git_ui: GitUi,
            memory: Memory,
            mcp: Mcp,
            orchestration_ui: OrchestrationUi,
            stats: Stats,
            board: Board,
            settings: Settings,
            kits: Kits,
            preview: Preview,
            kit_tabs: KitTabs,
            model3d_view: Model3d,
            control: Control,
            orchestration: Orchestration,
            mobile: Mobile,
        }
    };
}

macro_rules! define_packages {
    ($($field:ident: $route:ident,)*) => {
        /// Every package's UI state (`App.pkg.<name>`).
        #[derive(Debug, Default)]
        pub struct Packages {
            $(pub $field: crate::$field::State,)*
        }

        impl App {
            /// Whether `message` is a package's, for `route`.
            fn is_package_message(message: &Message) -> bool {
                matches!(message, $(Message::$route(_))|*)
            }

            /// A package's message to its `update`.
            fn route(&mut self, message: Message) -> Task<Message> {
                match message {
                    $(Message::$route(msg) => crate::$field::update(
                        &mut self.pkg.$field,
                        &mut self.model,
                        &self.services,
                        msg,
                    ),)*
                    _ => Task::none(),
                }
            }

            /// The intent, to every package after the shell.
            fn offer_intent(&mut self, intent: &Intent) -> Vec<Task<Message>> {
                let mut tasks = Vec::new();
                $(if let Some(task) = crate::$field::on_intent(
                    &mut self.pkg.$field,
                    &mut self.model,
                    &self.services,
                    intent,
                ) {
                    tasks.push(task);
                })*
                tasks
            }

            /// The core event, to every package after the shell.
            fn offer_event(&mut self, event: &CoreEvent) -> Vec<Task<Message>> {
                let mut tasks = Vec::new();
                $(if let Some(task) = crate::$field::on_event(
                    &mut self.pkg.$field,
                    &mut self.model,
                    &self.services,
                    event,
                ) {
                    tasks.push(task);
                })*
                tasks
            }

            fn package_subscriptions(&self) -> Vec<Subscription<Message>> {
                vec![$(crate::$field::subscription(&self.pkg.$field, &self.model),)*]
            }
        }
    };
}

packages!(define_packages);

/// Runs the app until its main window closes.
pub fn run() -> iced::Result {
    iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .scale_factor(App::scale_factor)
        .font(hangar_theme::fonts::INTER_VARIABLE)
        .default_font(hangar_theme::fonts::ui())
        .run()
}

pub struct App {
    pub model: Model,
    pub pkg: Packages,
    services: Services,
    /// The theme iced draws its own widgets in, rebuilt when the theme
    /// changes (`to_iced` allocates).
    iced_theme: (&'static str, iced::Theme),
    snapshot: watch::Sender<Arc<Snapshot>>,
    /// The model revision the snapshot and the save follow.
    published: u64,
    saved: u64,
    bus: Bus,
    inbox: InboxReceiver,
    main_window: window::Id,
}

impl App {
    /// Inventory §1.6, "Native order": the core started, the main window
    /// opened, the state loaded; the boot screen shows until it is in.
    fn boot() -> (App, Task<Message>) {
        let (core, bus) = CoreHandles::start();
        let (inbox, inbox_receiver) = services::inbox::channel();
        let (snapshot, snapshot_receiver) = watch::channel(Arc::new(Snapshot::default()));

        let pkg = Packages::default();
        let services = Services::new(
            core.clone(),
            crate::panes::terminal_access(&pkg.panes),
            crate::preview::preview_access(&pkg.preview),
            inbox,
            snapshot_receiver,
        );

        let mut model = Model::default();
        let (main_window, open) = windows::open_main();
        model.live.windows.insert(main_window, WindowKind::Main);

        let theme = model.theme();
        let app = App {
            model,
            pkg,
            services,
            iced_theme: (theme.id, hangar_theme::to_iced(theme)),
            snapshot,
            published: 0,
            saved: 0,
            bus,
            inbox: inbox_receiver,
            main_window,
        };
        let host = core.host().clone();
        let load = Task::perform(services::core::load_state(host), Message::Booted);
        let detect = Task::perform(services::core::detect(), |(agents, shells)| {
            Message::Detected(agents, shells)
        });
        let screenshot = match shell::dev_screenshot::requested() {
            Some(_) => shell::dev_screenshot::capture(main_window),
            None => Task::none(),
        };
        (app, Task::batch([open, load, detect, screenshot]))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.handle(message);
        let after = self.after_update();
        Task::batch([task, after])
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        // Before hydration the shell answers control requests itself, so an
        // agent asking early is told why rather than left to time out
        // (inventory §8.2, `ControlRequest`).
        if let Message::Control(crate::control::Msg::Request(request)) = &message
            && !self.model.live.hydrated
        {
            request
                .reply
                .send(Err("Hangar.AI is still starting…".into()));
            return Task::none();
        }
        if Self::is_package_message(&message) {
            return self.route(message);
        }
        match message {
            Message::Booted(loaded) => self.booted(loaded),
            Message::Detected(agents, shells) => {
                self.model.ui.available_agents = agents;
                self.model.ui.shells = shells;
                Task::none()
            }
            Message::SaveDue(revision) => self.save_if_due(revision),
            Message::Saved(revision, result) => match result {
                Ok(()) => {
                    self.saved = self.saved.max(revision);
                    Task::none()
                }
                Err(error) => notice::show(&mut self.model, shell::Notice::Problem(error)),
            },
            Message::Core(event) => self.core_event(event),
            Message::Window(id, event) => self.window_event(id, event),
            Message::Key(id, event) => self.key(id, event),
            Message::Command(command) => {
                let window = self.model.ui.focused_window.unwrap_or(self.main_window);
                commands::run(&self.model, window, command)
            }
            Message::Intent(intent) => self.intent(intent),
            Message::Store(action) => {
                self.model.apply(action);
                Task::none()
            }
            Message::NoticeExpired(id) => {
                notice::expire(&mut self.model, id);
                Task::none()
            }
            Message::DevScreenshot(shot) => {
                if let Some(path) = shell::dev_screenshot::requested() {
                    match shell::dev_screenshot::write(&path, &shot) {
                        Ok(()) => eprintln!("screenshot written to {}", path.display()),
                        Err(error) => eprintln!("screenshot failed: {error}"),
                    }
                }
                self.quit()
            }
            Message::Tick(_) | Message::Noop => Task::none(),
            // Routed above.
            _ => Task::none(),
        }
    }

    /// The state is in: hydrate, and the gated work can start.
    ///
    /// TODO(P0b): reopen detached workspaces' windows, start the engines and
    /// the subscriptions gated on hydration (§1.6 steps 7–8).
    fn booted(&mut self, loaded: Result<Option<serde_json::Value>, String>) -> Task<Message> {
        let (state, problem) = match loaded {
            Ok(Some(value)) => (crate::state::read_state(value), None),
            Ok(None) => (Default::default(), None),
            // Core has already moved an unreadable file aside; start empty.
            Err(error) => (Default::default(), Some(error)),
        };
        self.model.hydrate(state);
        // Hydrating is not a change: nothing to save until something changes.
        self.published = self.model.revision();
        self.saved = self.model.revision();
        self.publish_snapshot();
        match problem {
            Some(error) => notice::show(&mut self.model, shell::Notice::Problem(error)),
            None => Task::none(),
        }
    }

    /// After every message: a changed model republishes the snapshot and
    /// schedules its save; a changed theme rebuilds iced's.
    fn after_update(&mut self) -> Task<Message> {
        let theme = self.model.theme();
        if theme.id != self.iced_theme.0 {
            self.iced_theme = (theme.id, hangar_theme::to_iced(theme));
        }
        let revision = self.model.revision();
        if revision == self.published || !self.model.live.hydrated {
            return Task::none();
        }
        self.published = revision;
        self.publish_snapshot();
        persist::schedule(revision)
    }

    fn publish_snapshot(&self) {
        self.snapshot
            .send_replace(Arc::new(Snapshot::of(&self.model)));
    }

    fn save_if_due(&mut self, revision: u64) -> Task<Message> {
        if !self.model.live.hydrated
            || !persist::is_due(revision, self.model.revision(), self.saved)
        {
            return Task::none();
        }
        let host = self.services.core().host().clone();
        let state = self.model.state_arc();
        Task::perform(services::core::save_state(host, state), move |result| {
            Message::Saved(revision, result)
        })
    }

    fn core_event(&mut self, event: CoreEvent) -> Task<Message> {
        let own = match &event {
            CoreEvent::NotificationActivated { workspace, pane } => {
                self.intent(Intent::FocusPane {
                    ws: workspace.clone(),
                    pane: pane.clone(),
                    terminal: true,
                })
            }
            _ => Task::none(),
        };
        let mut tasks = self.offer_event(&event);
        tasks.push(own);
        Task::batch(tasks)
    }

    fn window_event(&mut self, id: window::Id, event: window::Event) -> Task<Message> {
        match event {
            window::Event::CloseRequested if id == self.main_window => self.quit(),
            // TODO(P0b): a workspace window's close is a reattach.
            window::Event::CloseRequested => window::close(id),
            window::Event::Closed => {
                self.model.live.windows.remove(&id);
                Task::none()
            }
            window::Event::Opened { size, .. } | window::Event::Resized(size) => {
                self.model.ui.body_width.insert(id, size.width);
                Task::none()
            }
            window::Event::Focused => {
                self.model.ui.focused_window = Some(id);
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// The main window's end is the app's (inventory §1.6, "On main-window
    /// close"): the last change saved, the core shut down, then exit.
    fn quit(&mut self) -> Task<Message> {
        if self.model.live.hydrated && self.model.revision() != self.saved {
            let host = self.services.core().host();
            if let Err(error) = services::core::save_state_now(host, self.model.state()) {
                eprintln!("the last change could not be saved: {error}");
            }
        }
        self.services.core().shutdown();
        iced::exit()
    }

    /// Keys the shell answers itself. Shortcuts are the keymap's (P7, its own
    /// subscription).
    fn key(&mut self, window: window::Id, event: keyboard::Event) -> Task<Message> {
        // TODO(P0b): Escape closes the top overlay only when the overlay did
        // not use it (a popover inside it, a field's own Escape).
        if let keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        } = event
            && !self.model.ui.overlays.is_empty()
        {
            return self.intent(Intent::CloseOverlay);
        }
        Task::none()
    }

    /// The shell's part of an intent, then every package's.
    fn intent(&mut self, intent: Intent) -> Task<Message> {
        let own = self.own_intent(&intent);
        let mut tasks = self.offer_intent(&intent);
        tasks.push(own);
        Task::batch(tasks)
    }

    fn own_intent(&mut self, intent: &Intent) -> Task<Message> {
        let ui = &mut self.model.ui;
        let open = |ui: &mut crate::state::Ui, overlay: Overlay| {
            if ui.overlays.last() != Some(&overlay) {
                ui.overlays.push(overlay);
            }
        };
        match intent.clone() {
            Intent::OpenOverlay(overlay) => open(ui, overlay),
            Intent::CloseOverlay => {
                ui.overlays.pop();
            }
            Intent::TogglePalette => {
                if ui.overlays.last() == Some(&Overlay::Palette) {
                    ui.overlays.pop();
                } else {
                    open(ui, Overlay::Palette);
                }
            }
            Intent::OpenSettings(category) => {
                open(ui, Overlay::Settings(category.unwrap_or_default()))
            }
            Intent::OpenMcpInstall => open(ui, Overlay::McpInstall),
            Intent::OpenMcpServers(ws) => open(ui, Overlay::McpServers(ws)),
            Intent::OpenOrchestration(ws) => open(ui, Overlay::Orchestration(ws)),
            Intent::OpenWorktrees(ws) => open(ui, Overlay::Worktrees(ws)),
            Intent::OpenKitsDialog(ws) => open(ui, Overlay::KitsDialog(ws)),
            Intent::OpenDiff { ws, pane } => open(ui, Overlay::Diff { ws, pane }),
            Intent::OpenSessions { ws, pane } => open(ui, Overlay::SessionPicker { ws, pane }),
            Intent::OpenTask { cwd, task } => open(ui, Overlay::Task { cwd, task }),
            Intent::SetView(view) => ui.view = view,
            Intent::OpenBoard(ws) => {
                ui.view = View::Board;
                return self.intent(Intent::ActivateWorkspace(ws));
            }
            Intent::ActivateWorkspace(ws) => {
                // TODO(P0b): `activateRef` semantics (App.tsx:438).
                if !ui.opened_ids.contains(&ws) {
                    ui.opened_ids.push(ws.clone());
                }
                self.model.apply(StoreAction::SetActiveWorkspace(Some(ws)));
            }
            Intent::ToggleNavigator => {
                self.model.apply(StoreAction::UpdateSettings(Patch::new(
                    |s: &mut Settings| s.navigator_open = !s.navigator_open,
                )));
            }
            Intent::ToggleInspector => {
                self.model.apply(StoreAction::UpdateSettings(Patch::new(
                    |s: &mut Settings| s.inspector_open = !s.inspector_open,
                )));
            }
            Intent::ToggleDock => {
                self.model.apply(StoreAction::UpdateSettings(Patch::new(
                    |s: &mut Settings| s.board_dock_open = !s.board_dock_open,
                )));
            }
            Intent::CycleRegion(step) => return self.cycle_region(step),
            Intent::FocusRegion(region) => return self.focus_region(region),
            Intent::Notice(text) => return notice::show(&mut self.model, text),
            Intent::Notify(notification) => {
                return services::notify::send(&self.services, notification);
            }
            // TODO(P0b): the shell's part of the others (focus, zoom, detach,
            // reattach, presets…). Packages already get them below.
            _ => {}
        }
        Task::none()
    }

    /// F6 and Shift+F6. A dialog keeps the keyboard.
    ///
    /// TODO(P0b): the regions shown in the focused window, and the region the
    /// focus is in now.
    fn cycle_region(&mut self, step: i8) -> Task<Message> {
        if !self.model.ui.overlays.is_empty() {
            return Task::none();
        }
        let from = match self.model.ui.focus {
            Some(shell::Focus::Region(region)) => Some(region),
            _ => None,
        };
        match shell::regions::next_region(&shell::regions::ORDER, from, step) {
            Some(region) => self.focus_region(region),
            None => Task::none(),
        }
    }

    /// The focus to region `region`, as its owner says.
    fn focus_region(&mut self, region: Region) -> Task<Message> {
        self.model.ui.focus = Some(shell::Focus::Region(region));
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
        entry.unwrap_or_else(Task::none)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        let ctx = self.ctx(window);
        match self.model.live.windows.get(&window) {
            Some(WindowKind::Main) => shell::frame::main(ctx),
            Some(WindowKind::Workspace(id)) => match self.model.workspace(id) {
                Some(ws) => shell::frame::workspace_window(ctx, ws),
                None => Space::new().into(),
            },
            None => Space::new().into(),
        }
    }

    fn ctx(&self, window: window::Id) -> Ctx<'_> {
        Ctx {
            model: &self.model,
            pkg: &self.pkg,
            i18n: Translator::new(self.model.locale()),
            theme: self.model.theme(),
            metrics: Metrics {
                scale: self.model.settings().scale_factor(),
                body_width: self
                    .model
                    .ui
                    .body_width
                    .get(&window)
                    .copied()
                    .unwrap_or(windows::MAIN_SIZE.width),
            },
            keymap: &self.model.live.keymap.index,
            window,
            now: Instant::now(),
        }
    }

    fn title(&self, window: window::Id) -> String {
        match self.model.live.windows.get(&window) {
            Some(WindowKind::Workspace(id)) => match self.model.workspace(id) {
                Some(ws) => format!("{} — Hangar.AI", ws.name),
                None => "Hangar.AI".into(),
            },
            _ => "Hangar.AI".into(),
        }
    }

    fn theme(&self, _window: window::Id) -> iced::Theme {
        self.iced_theme.1.clone()
    }

    fn scale_factor(&self, _window: window::Id) -> f32 {
        self.model.settings().scale_factor()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            services::events::subscription(&self.bus),
            services::inbox::subscription(&self.inbox),
            window::events().map(|(id, event)| Message::Window(id, event)),
            iced::event::listen_with(|event, _status, window| match event {
                iced::Event::Keyboard(event) => Some(Message::Key(window, event)),
                _ => None,
            }),
        ];
        subscriptions.extend(self.package_subscriptions());
        Subscription::batch(subscriptions)
    }
}
