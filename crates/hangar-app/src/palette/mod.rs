//! P7: the command palette (`CommandPalette.tsx`, inventory §2.1/5).
//!
//! The panel, the matching, the ranking, the keys and the highlight are
//! hangar-ui's `Palette<T>` and `matching` (the port of `paletteMatch.ts`,
//! with its tests); this package says what it lists (`sources.rs`) and what
//! running an entry does.
//!
//! The shell pushes and pops `Overlay::Palette` (`Intent::TogglePalette`);
//! this package follows it in `on_intent`, building the list when the
//! palette appears and dropping it when it goes. While it is open the list
//! is rebuilt on every core event and every intent, so an agent changing
//! state shows in the open list, as `useWorkspaceActivity` made it.

pub mod sources;

use iced::widget::Space;
use iced::{Element, Length, Subscription, Task, window};

use hangar_i18n::{Key, Translator, args};
use hangar_ui::palette::{self as panel, Event, Labels, Palette};

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::{Focus, Overlay, Region, View};
use crate::state::Model;

pub use sources::Target;

/// The package's UI state; lives in `App.pkg.palette`.
#[derive(Debug, Default)]
pub struct State {
    open: Option<Open>,
}

#[derive(Debug)]
struct Open {
    palette: Palette<Target>,
    labels: Labels,
    /// The window it was opened in, which is where it lists from and draws.
    window: window::Id,
    /// Where the keyboard was, to give it back when the palette is dismissed.
    opener: Option<Focus>,
}

#[derive(Debug, Default)]
pub struct Live;

/// The package's messages, routed by the shell as `Message::Palette(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    Ui(panel::Msg),
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let _ = sv;
    let Msg::Ui(msg) = msg;
    let Some(open) = &mut st.open else {
        return Task::none();
    };
    let (event, task) = open.palette.update(msg);
    let task = task.map(|msg| Message::Palette(Msg::Ui(msg)));
    match event {
        Event::None => task,
        Event::Dismiss => {
            let Some(open) = st.open.take() else {
                return task;
            };
            Task::batch([task, close(m).chain(give_back(m, open.window, open.opener))])
        }
        Event::Run(target) => {
            let window = open.window;
            st.open = None;
            Task::batch([task, close(m).chain(run(m, window, target))])
        }
    }
}

/// Closes the palette, if it is still the overlay on top: Escape may have
/// closed it already.
fn close(m: &Model) -> Task<Message> {
    if m.ui.overlays.last() == Some(&Overlay::Palette) {
        Task::done(Message::Intent(Intent::CloseOverlay))
    } else {
        Task::none()
    }
}

/// Dismissed without running anything: the keyboard goes back where it was,
/// else to the terminal of the focused pane of the workspace its window draws.
fn give_back(m: &Model, window: window::Id, opener: Option<Focus>) -> Task<Message> {
    match opener {
        Some(Focus::Terminal(pane)) => focus_terminal(m, pane),
        Some(Focus::Widget(id)) => iced::widget::operation::focus(id),
        Some(Focus::Region(region)) => Task::done(Message::Intent(Intent::FocusRegion(region))),
        None => match focused_pane(m, window) {
            Some(pane) => focus_terminal(m, pane),
            None => Task::none(),
        },
    }
}

/// The focused pane of the workspace `window` draws: a palette opened in a
/// workspace's window is about that window's panes, not the main window's.
fn focused_pane(m: &Model, window: window::Id) -> Option<String> {
    let ws = m.window_workspace(window)?;
    m.ui.focus_by_workspace.get(&ws.id).cloned()
}

fn focus_terminal(m: &Model, pane: String) -> Task<Message> {
    match sources::workspace_of(m, &pane) {
        Some(ws) => Task::done(Message::Intent(Intent::FocusPane {
            ws: ws.id.clone(),
            pane,
            terminal: true,
        })),
        None => Task::none(),
    }
}

/// Closed first, then run: whatever the command opens must not open under
/// the palette, or be closed with it. A command that does not take the
/// keyboard somewhere of its own hands it back to the terminal first, so
/// that one which focuses another pane still wins.
fn run(m: &Model, window: window::Id, target: Target) -> Task<Message> {
    let intent = |intent| Task::done(Message::Intent(intent));
    match target {
        Target::Command(id) => {
            let back = match focused_pane(m, window) {
                Some(pane) if !sources::owns_focus(id) => focus_terminal(m, pane),
                _ => Task::none(),
            };
            back.chain(Task::done(Message::Command(id)))
        }
        Target::Pane { ws, pane } => intent(Intent::FocusPane {
            ws,
            pane,
            terminal: true,
        }),
        Target::Workspace(ws) => {
            intent(Intent::ActivateWorkspace(ws)).chain(intent(Intent::SetView(View::Terminals)))
        }
        Target::Manager => intent(Intent::OpenManager),
    }
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = (st, m);
    Subscription::none()
}

/// The palette owns no region, so the shell never calls this.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

/// Follows the overlay stack: the palette appears, goes, or stays with a list
/// brought up to date.
pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (sv, intent);
    follow(st, m);
    None
}

pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (sv, event);
    if st.open.is_some() {
        follow(st, m);
    }
    None
}

fn follow(st: &mut State, m: &Model) {
    let shown = m.ui.overlays.contains(&Overlay::Palette);
    match (&mut st.open, shown) {
        (None, true) => st.open = Some(open(m)),
        (Some(_), false) => st.open = None,
        (Some(open), true) => {
            let items = sources::items(m, open.window, translator(m), &m.live.keymap.index);
            open.palette.set_items(items);
        }
        (None, false) => {}
    }
}

fn translator(m: &Model) -> Translator {
    Translator::new(m.locale())
}

fn open(m: &Model) -> Open {
    let i18n = translator(m);
    let index = &m.live.keymap.index;
    // Where commands run: the window with the system's focus, else the main
    // one.
    let window = m.focused_window().unwrap_or_else(window::Id::unique);
    let keys = index.label(CommandId::ViewPalette);
    Open {
        palette: Palette::new(sources::items(m, window, i18n, index)),
        labels: Labels {
            placeholder: i18n.t(Key::PalettePlaceholder).into_owned(),
            navigate: i18n.t(Key::PaletteNavigate).into_owned(),
            run: i18n.t(Key::PaletteRun).into_owned(),
            close: i18n.t(Key::PaletteClose).into_owned(),
            esc: i18n.t(Key::PaletteEscKey).into_owned(),
            // The palette fills `{query}` itself, as the user types.
            empty: i18n.t(Key::PaletteEmpty).into_owned(),
            anywhere: (!keys.is_empty()).then(|| {
                i18n.t_with(Key::PaletteAnywhere, &args!("keys" => keys))
                    .into_owned()
            }),
        },
        window,
        opener: m.ui.focus.clone(),
    }
}

/// `Overlay::Palette`: the panel a way down from the top, over a veil that
/// closes it when pressed and released on.
pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let Some(open) = &st.open else {
        return Space::new().into();
    };
    // The shell draws the overlays in every window; the palette belongs to
    // the one it was opened in.
    if open.window != ctx.window && ctx.model.live.windows.contains_key(&open.window) {
        return Space::new().into();
    }
    let t = ctx.theme;
    // The keymap goes first: a shortcut typed in the search field acts, and is
    // not also typed there.
    let content = crate::keymap::guard(
        ctx,
        open.palette
            .view(t, &open.labels)
            .map(|msg| Message::Palette(Msg::Ui(msg))),
    );
    panel::over(
        t,
        Space::new().width(Length::Fill).height(Length::Fill),
        Some(content),
        Message::Palette(Msg::Ui(panel::Msg::Dismiss)),
    )
    .into()
}
