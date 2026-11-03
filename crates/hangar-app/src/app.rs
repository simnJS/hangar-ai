//! The application: boot, the router, the views of each window, the
//! subscriptions (inventory §8.2).
//!
//! The router is the only place that knows every package. It dispatches each
//! package's messages to that package's `update`, offers every intent and
//! every core event to each package's `on_intent` and `on_event` after its
//! own handling, and batches every package's subscription. The list of
//! packages is written once, in `packages!` below; a package never calls
//! another package's `update` (PACKAGES.md, rule 4).
//!
//! The shell's own behaviour (App.tsx's, inventory §1.6–§1.11) is in
//! `shell.rs` next to this file: what each intent does to the frame, the
//! windows, the drawers and the focus.

mod hooks;
mod shell;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use iced::widget::Space;
use iced::{Element, Point, Size, Subscription, Task, keyboard, window};
use tokio::sync::watch;

use hangar_i18n::Translator;

use crate::ctx::{Ctx, Metrics};
use crate::message::{Intent, Message};
use crate::services::events::Bus;
use crate::services::inbox::InboxReceiver;
use crate::services::{self, CoreEvent, CoreHandles, Services};
use crate::shell::{WindowKind, windows};
use crate::state::{Model, Snapshot, persist};

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

/// What an agent asking before the state is in is told, word for word as
/// control.ts and remoteControl.ts said it.
const STARTING: &str = "Hangar.AI is still starting. Try again in a moment.";

/// What the snapshot was last published for: a change to any of these
/// republishes it.
#[derive(Debug, Clone, Default, PartialEq)]
struct Published {
    revision: u64,
    version: u64,
    opened: Vec<String>,
}

pub struct App {
    pub model: Model,
    pub pkg: Packages,
    services: Services,
    /// The themes iced draws its own widgets in, one per theme in use, built
    /// once each (`to_iced` allocates).
    iced_themes: HashMap<&'static str, iced::Theme>,
    snapshot: watch::Sender<Arc<Snapshot>>,
    published: Published,
    /// The model revision last written to `state.json`.
    saved: u64,
    writer: persist::Writer,
    bus: Bus,
    inbox: InboxReceiver,
    main_window: window::Id,
    /// Each window's size and position as the system reports them, in
    /// logical pixels before the interface size (iced reports them divided
    /// by it).
    window_sizes: HashMap<window::Id, Size>,
    window_positions: HashMap<window::Id, Point>,
    /// The last move or resize of each workspace window, for its debounced
    /// bounds.
    bounds_seq: HashMap<window::Id, u64>,
    /// Workspaces whose window is asked for and not made yet, and why.
    opening: HashMap<String, windows::Opening>,
    /// Workspace windows made and not reported open yet: unless they are by
    /// `windows::OPEN_TIMEOUT`, the detach is undone.
    unopened: HashMap<window::Id, (String, windows::Opening)>,
    /// Workspace windows given up on, closed if they still come.
    abandoned: HashSet<window::Id>,
    /// `state.json` could neither be used nor moved aside at boot: core
    /// refuses every save until it can be (store.rs), and a refusal is told
    /// as such.
    save_held: bool,
    /// The active workspace and the focused window's view as the last update
    /// left them, for what follows a change of either.
    last_active: Option<String>,
    last_view: crate::shell::View,
    /// A kit tab was opened by the person: once the panel is placed, a
    /// preview without room opens as a drawer (App.tsx's `kitOpened`).
    kit_opened: bool,
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
        let (main_window, open) = windows::open_main(model.live.title_bar.decorated);
        model.live.windows.insert(main_window, WindowKind::Main);

        let mut app = App {
            model,
            pkg,
            services,
            iced_themes: HashMap::new(),
            snapshot,
            published: Published::default(),
            saved: 0,
            writer: persist::Writer::default(),
            bus,
            inbox: inbox_receiver,
            main_window,
            window_sizes: HashMap::new(),
            window_positions: HashMap::new(),
            bounds_seq: HashMap::new(),
            opening: HashMap::new(),
            unopened: HashMap::new(),
            abandoned: HashSet::new(),
            save_held: false,
            last_active: None,
            last_view: crate::shell::View::default(),
            kit_opened: false,
        };
        app.cache_themes();
        let host = core.host().clone();
        let load = Task::perform(services::core::load_state(host), Message::Booted);
        let detect = Task::perform(services::core::detect(), |(claude, shells)| {
            Message::Detected(claude, shells)
        });
        let screenshot = match crate::shell::dev_screenshot::requested() {
            Some(_) => crate::shell::dev_screenshot::capture(main_window),
            None => Task::none(),
        };
        let hooks = hooks::install(&app.services);
        let layout = if cfg!(target_os = "linux") {
            Task::perform(crate::shell::title_bar::read_layout(), |layout| {
                Message::Shell(crate::shell::Action::TitleBarLayout(layout))
            })
        } else {
            Task::none()
        };
        (
            app,
            Task::batch([open, load, detect, screenshot, hooks, layout]),
        )
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
            request.reply.send(Err(STARTING.into()));
            return Task::none();
        }
        if Self::is_package_message(&message) {
            return self.route(message);
        }
        match message {
            Message::Booted(loaded) => self.booted(loaded),
            Message::Detected(claude, shells) => {
                self.model.ui.claude_installed = claude;
                self.model.ui.shells = shells;
                Task::none()
            }
            Message::SaveDue(revision) => self.save_if_due(revision),
            Message::Saved(revision, result) => match result {
                Ok(()) => {
                    self.saved = self.saved.max(revision);
                    // A held file is only ever written over once core could
                    // move it aside: the hold is over.
                    self.save_held = false;
                    Task::none()
                }
                Err(error) => {
                    let notice =
                        save_notice(Translator::new(self.model.locale()), self.save_held, error);
                    crate::shell::notice::show(&mut self.model, notice)
                }
            },
            Message::Core(event) => self.core_event(event),
            Message::Window(id, event) => self.window_event(id, event),
            Message::Key(id, event) => self.key(id, event),
            Message::Command(command) => {
                let window = self.focused_window();
                crate::commands::run(&self.model, window, command)
            }
            Message::Intent(intent) => self.intent(intent),
            Message::Store(action) => {
                self.model.apply(action);
                Task::none()
            }
            Message::NoticeExpired(id) => {
                crate::shell::notice::expire(&mut self.model, id);
                Task::none()
            }
            Message::DevScreenshot(due) => {
                if let Some(path) = crate::shell::dev_screenshot::requested() {
                    let main = self.main_window;
                    let taken = crate::shell::dev_screenshot::take(
                        &path,
                        self.view(main),
                        due,
                        self.scale_factor(main),
                        &self.theme(main),
                    );
                    match taken {
                        Ok(()) => eprintln!("screenshot written to {}", path.display()),
                        Err(error) => eprintln!("screenshot failed: {error}"),
                    }
                }
                self.quit()
            }
            Message::Shell(action) => self.shell_action(action),
            Message::Tick(_) | Message::Noop => Task::none(),
            // Routed above.
            _ => Task::none(),
        }
    }

    /// The state is in (§1.6 steps 5–7): hydrate, then the windows of the
    /// workspaces that were detached when the app last closed open again.
    /// What is gated on hydration (control, engines, hibernation, notices)
    /// reads `live.hydrated`, which is now set.
    fn booted(
        &mut self,
        loaded: Result<Option<Box<crate::state::Loaded>>, hangar_core::store::LoadError>,
    ) -> Task<Message> {
        // A file that could not be used starts the app empty. It has been
        // moved out of the way of the first save already, or core holds it:
        // no save goes over it until it can be moved (store.rs), and the
        // person is told each time one is refused (`Message::Saved`).
        let (loaded, problem) = match loaded {
            Ok(loaded) => (loaded.map(|loaded| *loaded).unwrap_or_default(), None),
            Err(error) => (crate::state::Loaded::default(), Some(error)),
        };
        self.model.hydrate_read(loaded.state, loaded.kept);
        // Hydrating is not a change: nothing to save until something changes.
        self.saved = self.model.revision();
        self.last_active = self.model.state().active_workspace_id.clone();
        self.publish_snapshot();

        let mut tasks: Vec<Task<Message>> = self
            .model
            .detached_workspaces()
            .into_iter()
            .map(|ws| self.ask_workspace_window(ws, windows::Opening::Boot))
            .collect();
        if let Some(error) = problem {
            self.save_held = error.rescue == hangar_core::store::Rescue::Held;
            let notice = load_notice(Translator::new(self.model.locale()), &error);
            tasks.push(crate::shell::notice::show(&mut self.model, notice));
        }
        Task::batch(tasks)
    }

    /// After every message: what follows a change (App.tsx's effects), the
    /// snapshot republished, the save scheduled, the themes in use built.
    fn after_update(&mut self) -> Task<Message> {
        let mut tasks = self.follow_changes();
        self.cache_themes();
        let now = Published {
            revision: self.model.revision(),
            version: self.model.live.version,
            opened: self.model.ui.opened_ids.clone(),
        };
        if now != self.published {
            let saved_revision = self.published.revision;
            self.published = now;
            self.publish_snapshot();
            if self.model.live.hydrated && self.model.revision() != saved_revision {
                tasks.push(persist::schedule(self.model.revision()));
            }
        }
        // Nothing to save, but what the engines and hibernation read beside
        // the state (deliveries, activity, sleep) may have moved (P18).
        if self.model.live.hydrated && self.snapshot.borrow().is_stale(&self.model) {
            self.publish_snapshot();
        }
        Task::batch(tasks)
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
        let value = self.model.to_json();
        let writer = self.writer.clone();
        Task::perform(
            services::core::save_state(host, value, writer, revision),
            move |result| Message::Saved(revision, result),
        )
    }

    fn core_event(&mut self, event: CoreEvent) -> Task<Message> {
        let own = match &event {
            // The window the toast was about comes forward, then the pane
            // (core-api.md, "A notification's click").
            CoreEvent::NotificationActivated { workspace, pane } => {
                let raise = match self.model.window_of(workspace) {
                    Some(w) => window::gain_focus(w),
                    None => Task::none(),
                };
                let land = self.intent(Intent::FocusPane {
                    ws: workspace.clone(),
                    pane: pane.clone(),
                    terminal: true,
                });
                Task::batch([raise, land])
            }
            _ => Task::none(),
        };
        let mut tasks = self.offer_event(&event);
        tasks.push(own);
        Task::batch(tasks)
    }

    /// The main window's end is the app's (inventory §1.6, "On main-window
    /// close"): the last change saved, the core shut down, then exit.
    fn quit(&mut self) -> Task<Message> {
        if self.model.live.hydrated && self.model.revision() != self.saved {
            let host = self.services.core().host();
            let value = self.model.to_json();
            let revision = self.model.revision();
            match self
                .writer
                .write(revision, || hangar_core::store::save_state(host, value))
            {
                Ok(_) => self.saved = revision,
                Err(error) => eprintln!("the last change could not be saved: {error}"),
            }
        }
        self.services.core().shutdown();
        iced::exit()
    }

    /// The intent's shell part, then every package's.
    fn intent(&mut self, intent: Intent) -> Task<Message> {
        let own = self.own_intent(&intent);
        let mut tasks = self.offer_intent(&intent);
        tasks.push(own);
        Task::batch(tasks)
    }

    /// The window the person is in: where commands run and intents land.
    fn focused_window(&self) -> window::Id {
        self.model.focused_window().unwrap_or(self.main_window)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        let ctx = self.ctx(window);
        match self.model.live.windows.get(&window) {
            Some(WindowKind::Main) => crate::shell::frame::main(ctx),
            Some(WindowKind::Workspace(id)) => match self.model.workspace(id) {
                Some(ws) => crate::shell::frame::workspace_window(ctx, ws),
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
            theme: self.model.theme_in(window),
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

    /// Each window in its workspace's theme (inventory §1.11).
    fn theme(&self, window: window::Id) -> iced::Theme {
        let theme = self.model.theme_in(window);
        self.iced_themes
            .get(theme.id)
            .cloned()
            .unwrap_or_else(|| hangar_theme::to_iced(theme))
    }

    /// The themes the windows are drawn in, built when first needed.
    fn cache_themes(&mut self) {
        let mut ids: Vec<window::Id> = self.model.live.windows.keys().copied().collect();
        ids.push(self.main_window);
        for w in ids {
            let theme = self.model.theme_in(w);
            self.iced_themes
                .entry(theme.id)
                .or_insert_with(|| hangar_theme::to_iced(theme));
        }
    }

    /// `uiScale` as iced's scale factor: the whole window, the terminals
    /// included, so views lay out in unscaled units.
    fn scale_factor(&self, _window: window::Id) -> f32 {
        self.model.settings().scale_factor()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            services::events::subscription(&self.bus),
            services::inbox::subscription(&self.inbox),
            Subscription::run(quit_signals),
            window::events().map(|(id, event)| Message::Window(id, event)),
            // Escape alone, and only when nothing used it: one a field or a
            // hangar-ui layer took (a layer dismisses on it and captures it)
            // is not the shell's, so a layer and the shell never both close
            // an overlay on one press. Shortcuts are the keymap's, in its own
            // subscription (P7); a message per key here would only double
            // the work of every keystroke.
            iced::event::listen_with(|event, status, window| match event {
                iced::Event::Keyboard(
                    event @ keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(keyboard::key::Named::Escape),
                        ..
                    },
                ) if status == iced::event::Status::Ignored => Some(Message::Key(window, event)),
                _ => None,
            }),
        ];
        // A drawer is open somewhere: a press outside it closes it.
        let drawer_open = self.model.ui.drawer.is_some()
            || self
                .model
                .ui
                .parked
                .values()
                .any(|parked| parked.drawer.is_some());
        if drawer_open {
            subscriptions.push(iced::event::listen_with(
                |event, _status, window| match event {
                    iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => {
                        Some(Message::Shell(crate::shell::Action::Pressed(window)))
                    }
                    _ => None,
                },
            ));
        }
        subscriptions.extend(self.package_subscriptions());
        Subscription::batch(subscriptions)
    }

    /// Keys the shell answers itself: Escape, when nothing used it, closes
    /// the focused window's top overlay, else its drawer.
    fn key(&mut self, window: window::Id, event: keyboard::Event) -> Task<Message> {
        let keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            ..
        } = event
        else {
            return Task::none();
        };
        if !self.model.is_focused_window(window) {
            return Task::none();
        }
        if !self.model.ui.overlays.is_empty() {
            return self.intent(Intent::CloseOverlay);
        }
        self.close_drawer(crate::shell::DrawerCloseReason::Escape)
    }
}

/// The signals that ask the app to end: `kill` and a session's logout
/// (SIGTERM), Ctrl+C in the terminal it was started from (SIGINT), that
/// terminal closing (SIGHUP). Each becomes the same quit as the main
/// window's close, so the last change is saved and every pane's processes
/// are killed instead of being left to the system's default, which ended the
/// app on the spot and left behind whatever ignored the hangup.
///
/// Once heard, a signal no longer ends the process by itself. A second one,
/// while the first quit is under way, does: whoever sends it again means it.
fn quit_signals() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(1, async |mut output| {
        use iced::futures::SinkExt;
        // The listeners are made once, before the first wait, so that no
        // signal falls between two of them.
        let Some(mut signals) = QuitSignals::listen() else {
            // They cannot be listened to: the system's default stays, which
            // is how the app was before.
            return;
        };
        signals.next().await;
        let _ = output
            .send(Message::Shell(crate::shell::Action::Quit))
            .await;
        signals.next().await;
        std::process::exit(130);
    })
}

#[cfg(unix)]
struct QuitSignals([tokio::signal::unix::Signal; 3]);

#[cfg(unix)]
impl QuitSignals {
    fn listen() -> Option<QuitSignals> {
        use tokio::signal::unix::{SignalKind, signal};
        Some(QuitSignals([
            signal(SignalKind::terminate()).ok()?,
            signal(SignalKind::interrupt()).ok()?,
            signal(SignalKind::hangup()).ok()?,
        ]))
    }

    async fn next(&mut self) {
        let [term, int, hup] = &mut self.0;
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
            _ = hup.recv() => {}
        }
    }
}

/// Ctrl+C in the console a development build runs in; a release build on
/// Windows has none.
#[cfg(not(unix))]
struct QuitSignals;

#[cfg(not(unix))]
impl QuitSignals {
    fn listen() -> Option<QuitSignals> {
        Some(QuitSignals)
    }

    async fn next(&mut self) {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// What the person is told when `state.json` could not be used: where it was
/// kept, or that it stays and nothing will be saved over it.
fn load_notice(t: Translator, error: &hangar_core::store::LoadError) -> crate::shell::Notice {
    use hangar_core::store::Rescue;
    use hangar_i18n::{Key, args};
    let reason = error.reason.as_str();
    crate::shell::Notice::Problem(
        match &error.rescue {
            Rescue::KeptAs(path) => t.t_with(
                Key::StateLoadKept,
                &args!("error" => reason, "path" => path.display().to_string()),
            ),
            Rescue::Held => t.t_with(Key::StateLoadHeld, &args!("error" => reason)),
            Rescue::NoFile => t.t_with(Key::StateLoadFailed, &args!("error" => reason)),
        }
        .into_owned(),
    )
}

/// A save that failed; while the file is held, a refusal that keeps it.
fn save_notice(t: Translator, held: bool, error: String) -> crate::shell::Notice {
    crate::shell::Notice::Problem(if held {
        t.t_with(
            hangar_i18n::Key::StateSaveHeld,
            &hangar_i18n::args!("error" => error),
        )
        .into_owned()
    } else {
        error
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_core::store::{LoadError, Rescue};
    use hangar_i18n::Locale;

    #[test]
    fn a_load_problem_says_what_became_of_the_file() {
        let kept = LoadError {
            reason: "it is not UTF-8 text".into(),
            rescue: Rescue::KeptAs("/d/state.json.corrupt.1".into()),
        };
        let text = load_notice(Translator::new(Locale::En), &kept);
        assert_eq!(
            text.text(),
            "Could not read state.json (it is not UTF-8 text). It is kept as \
             /d/state.json.corrupt.1, and Hangar.AI starts empty."
        );
        let held = LoadError {
            reason: "permission denied".into(),
            rescue: Rescue::Held,
        };
        assert!(
            load_notice(Translator::new(Locale::Fr), &held)
                .text()
                .starts_with(
                    "Impossible de lire state.json (permission denied). Il reste tel quel"
                )
        );
        assert_eq!(
            save_notice(Translator::new(Locale::En), true, "still there".into()).text(),
            "Not saved, so that the unreadable state.json stays as it is: still there"
        );
        assert_eq!(
            save_notice(Translator::new(Locale::En), false, "disk full".into()).text(),
            "disk full"
        );
    }
}
