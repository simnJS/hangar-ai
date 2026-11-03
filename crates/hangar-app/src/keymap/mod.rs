//! P7: the keymap and the shortcut dispatcher (`src/lib/{keys,shortcuts,
//! useShortcuts}.ts`, inventory §1.10).
//!
//! - `chord.rs`: key presses as one-integer strokes, named without
//!   allocating; the chord algebra itself is `hangar_ui::shortcut`.
//! - `bindings.rs`: every command's default keys, the person's overrides
//!   (`settings.keybindings`) and the editing of them.
//! - `index.rs`: [`KeymapIndex`], the resolved keymap and its lookups,
//!   published as `Model.live.keymap.index`.
//! - `dispatch.rs`: the dispatcher's rules, its subscription, and the check
//!   the terminals and the fields ask before they use a key.
//!
//! # For the pane package (terminals)
//!
//! Give every `TerminalView` the keymap's filter, so that a terminal never
//! types a key that runs a live shortcut:
//!
//! ```ignore
//! terminal_view(&term, on_action).key_filter(keymap::terminal_filter(ctx))
//! ```
//!
//! # For a package with a text field
//!
//! Wrap the field in [`guard`]: a shortcut is then taken before the field
//! can type it or submit on it (Ctrl+Shift+Enter must not also send the
//! broadcast line). Without the guard the shortcut still runs, after the
//! field has used the key.
//!
//! # For the Shortcuts settings (P13)
//!
//! Read `m.live.keymap.index`: [`KeymapIndex::keymap`] for every command's
//! keys, [`KeymapIndex::overrides`] for what the person changed,
//! [`KeymapIndex::conflicts`], [`KeymapIndex::commands_for`] for the rivals
//! of a keystroke, [`KeymapIndex::label`] for a command's keys as shown.
//! Record with `hangar_ui::shortcut::Recorder` (and its field), then send:
//!
//! - `Msg::Suspend(token)` when a recorder opens and `Msg::Release(token)`
//!   when it closes, any token of the page's own; [`suspended`] says whether
//!   one is open, which is when the page must not close on Escape;
//! - `Msg::Bind`, `Msg::Unbind`, `Msg::Reset`, `Msg::ResetAll`, `Msg::Free`,
//!   or `Msg::SetOverrides` with what [`with_binding`] and the others
//!   computed. Each writes `settings.keybindings` through a `StoreAction`, in
//!   the shape `state.json` has always had.

mod bindings;
mod chord;
mod dispatch;
mod index;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use iced::advanced::subscription::from_recipe;
use iced::keyboard;
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Subscription, Task, window};

use hangar_i18n::Key;
use hangar_theme::{fonts, style, tokens};
use hangar_ui::shortcut::{Binding, format_chord_on};

use crate::commands::CommandId;
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::services::{CoreEvent, Services};
use crate::shell::Region;
use crate::state::{Model, Patch, Settings, StoreAction};

pub use bindings::{Overrides, reset_command, with_binding, without_binding};
pub use chord::Stroke;
pub use dispatch::{Dispatcher, Outcome, SEQUENCE_TIMEOUT};
pub use index::KeymapIndex;
// The Shortcuts settings (P13) are what these are for; nothing in this
// package needs them by name.
#[allow(unused_imports)]
pub use bindings::{Keymap, defaults};
#[allow(unused_imports)]
pub use index::{Conflict, ConflictKind, commands_for, find_conflicts};

/// The package's UI state; lives in `App.pkg.keymap`. Everything the
/// dispatcher keeps is in [`Live`], where the terminals' filter reads it.
#[derive(Debug, Default)]
pub struct State;

/// What other packages read; lives in `Model.live.keymap`.
#[derive(Debug, Default)]
pub struct Live {
    /// The resolved keymap. The shell hands it to every view as
    /// `Ctx::keymap`, and the terminals' key filter asks it.
    pub index: Arc<KeymapIndex>,
    /// The sequence half typed, and the recorders that suspend it.
    pub dispatcher: Dispatcher,
}

/// The package's messages, routed by the shell as `Message::Keymap(Msg)`.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A key press the subscription let through: one that starts a binding,
    /// or any while a sequence is half typed. `field` says a widget used it.
    Key {
        window: window::Id,
        stroke: Stroke,
        field: bool,
    },
    /// The sequence held by hold `n` waited too long.
    Timeout(u64),
    /// `settings.keybindings` changed under the index.
    Refresh,
    /// A shortcut recorder opened (Settings › Shortcuts): the dispatcher steps
    /// aside until every token is released.
    Suspend(u64),
    Release(u64),
    /// Adds a binding to a command.
    Bind {
        command: CommandId,
        binding: Binding,
    },
    /// Takes one off.
    Unbind {
        command: CommandId,
        binding: Binding,
    },
    /// Takes a keystroke away from the commands that answer to it, to give
    /// it to another (« Free it »).
    Free {
        binding: Binding,
        commands: Vec<CommandId>,
    },
    /// Puts a command back on its shipped keys.
    Reset(CommandId),
    /// Every command back on its shipped keys.
    ResetAll,
    /// Writes these overrides as they are.
    SetOverrides(Overrides),
}

pub fn update(st: &mut State, m: &mut Model, sv: &Services, msg: Msg) -> Task<Message> {
    let _ = (st, sv);
    refresh(m);
    match msg {
        Msg::Key {
            window,
            stroke,
            field,
        } => press(m, window, stroke, field),
        Msg::Timeout(held) => {
            if m.live.keymap.dispatcher.time_out(held) {
                show_pending(m);
            }
            Task::none()
        }
        Msg::Refresh => Task::none(),
        Msg::Suspend(token) => {
            m.live.keymap.dispatcher.suspend(token);
            show_pending(m);
            Task::none()
        }
        Msg::Release(token) => {
            m.live.keymap.dispatcher.release(token);
            Task::none()
        }
        Msg::Bind { command, binding } => {
            let index = m.live.keymap.index.clone();
            let next = with_binding(
                index.overrides(),
                index.keymap(),
                command,
                &binding,
                index.platform(),
            );
            write(m, next)
        }
        Msg::Unbind { command, binding } => {
            let index = m.live.keymap.index.clone();
            let next = without_binding(
                index.overrides(),
                index.keymap(),
                command,
                &binding,
                index.platform(),
            );
            write(m, next)
        }
        Msg::Free { binding, commands } => {
            let index = m.live.keymap.index.clone();
            let mut next = index.overrides().clone();
            for command in commands {
                next = without_binding(&next, index.keymap(), command, &binding, index.platform());
            }
            write(m, next)
        }
        Msg::Reset(command) => {
            let next = reset_command(m.live.keymap.index.overrides(), command);
            write(m, next)
        }
        Msg::ResetAll => write(m, Overrides::default()),
        Msg::SetOverrides(next) => write(m, next),
    }
}

/// A key press through the dispatcher.
fn press(m: &mut Model, window: window::Id, stroke: Stroke, field: bool) -> Task<Message> {
    let index = m.live.keymap.index.clone();
    // Taken out for the press, so that what is live can be asked of the
    // model while the dispatcher changes.
    let mut dispatcher = std::mem::take(&mut m.live.keymap.dispatcher);
    let outcome = {
        let mut liveness = dispatch::Liveness::new(m, window);
        dispatcher.press(&index, stroke, field, &mut |id| liveness.is_live(id))
    };
    let held = dispatcher.held();
    m.live.keymap.dispatcher = dispatcher;
    show_pending(m);
    match outcome {
        Outcome::Run(id) => Task::done(Message::Command(id)),
        Outcome::Hold => Task::perform(tokio::time::sleep(SEQUENCE_TIMEOUT), move |()| {
            Message::Keymap(Msg::Timeout(held))
        }),
        Outcome::Swallow | Outcome::Pass => Task::none(),
    }
}

/// The chords of the sequence half typed, as the chord hint shows them
/// (`Model.ui.pending_chords`, which the shell's frame draws).
fn show_pending(m: &mut Model) {
    let keymap = &m.live.keymap;
    let shown: Vec<String> = keymap
        .dispatcher
        .pending()
        .iter()
        .map(|stroke| format_chord_on(keymap.index.platform(), &keymap.index.chord(*stroke)))
        .collect();
    if m.ui.pending_chords != shown {
        m.ui.pending_chords = shown;
    }
}

/// Rebuilds the index when the settings' keybindings are no longer what it
/// was built from: a rebinding, « Reset all settings », a file loaded.
fn refresh(m: &mut Model) {
    let keybindings = &m.settings().keybindings;
    if m.live.keymap.index.is_stale(keybindings) {
        let index = KeymapIndex::new(keybindings);
        m.live.keymap.index = Arc::new(index);
        m.live.version += 1;
    }
}

/// Writes `next` as `settings.keybindings`.
fn write(m: &mut Model, next: Overrides) -> Task<Message> {
    if next.as_map() == &m.settings().keybindings {
        return Task::none();
    }
    let map = next.into_map();
    m.apply(StoreAction::UpdateSettings(Patch::new(
        move |s: &mut Settings| {
            s.keybindings = map.clone();
        },
    )));
    refresh(m);
    Task::none()
}

pub fn subscription(st: &State, m: &Model) -> Subscription<Message> {
    let _ = st;
    let keymap = &m.live.keymap;
    let presses = from_recipe(dispatch::KeyPresses {
        index: keymap.index.clone(),
        armed: keymap.dispatcher.is_armed(),
        suspended: keymap.dispatcher.is_suspended(),
    });
    let keybindings = &m.settings().keybindings;
    if !keymap.index.is_stale(keybindings) {
        return presses;
    }
    // The settings changed under the index: one message rebuilds it. Named
    // after what it rebuilds for, so a second change while the first is on
    // its way is a new subscription, and an index rebuilt makes it go.
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(keybindings)
        .unwrap_or_default()
        .hash(&mut hasher);
    let changed = Subscription::run_with(hasher.finish(), |_| {
        use iced::futures::StreamExt;
        iced::futures::stream::once(async { Message::Keymap(Msg::Refresh) })
            .chain(iced::futures::stream::pending())
    });
    Subscription::batch([presses, changed])
}

/// The keymap owns no region, so the shell never calls this.
pub fn focus_entry(st: &State, m: &Model, region: Region) -> Option<Task<Message>> {
    let _ = (st, m, region);
    None
}

pub fn on_intent(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    intent: &Intent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, intent);
    None
}

pub fn on_event(
    st: &mut State,
    m: &mut Model,
    sv: &Services,
    event: &CoreEvent,
) -> Option<Task<Message>> {
    let _ = (st, m, sv, event);
    None
}

/// Whether a shortcut recorder has the keyboard (`shortcutsSuspended`):
/// whatever else listens to keys stands down meanwhile.
pub fn suspended(m: &Model) -> bool {
    m.live.keymap.dispatcher.is_suspended()
}

/// The filter for `hangar_term::TerminalView::key_filter`: true for a key the
/// keymap takes, which the terminal then neither types nor captures, so it
/// reaches the dispatcher. A terminal is not a text field: a bare chord bound
/// by the person is a shortcut there.
pub fn terminal_filter<'a>(ctx: Ctx<'a>) -> impl Fn(&hangar_term::KeyPress) -> bool + 'a {
    move |press| {
        dispatch::takes(
            ctx.model,
            ctx.window,
            &press.key,
            press.physical_key,
            press.modifiers,
            press.text.as_deref(),
            false,
            press.kind == hangar_term::KeyEventKind::Release,
        )
    }
}

/// `field` with the keys the keymap takes spent before it sees them, so a
/// shortcut never also types or submits in it. The field rules apply: what
/// the field owns (plain characters, Ctrl+C and the like) still reaches it.
pub fn guard<'a>(ctx: Ctx<'a>, field: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    hangar_ui::keys::on_keyboard(field, move |event| match event {
        keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            text,
            ..
        } => dispatch::takes(
            ctx.model,
            ctx.window,
            key,
            *physical_key,
            *modifiers,
            text.as_deref(),
            true,
            false,
        )
        // Spent here; the subscription, which sees it as used by a widget,
        // runs the command.
        .then_some(Message::Noop),
        _ => None,
    })
}

/// The chord hint: the chords of a sequence half typed and « waiting for the
/// next key », or nothing.
pub fn chord_hint<'a>(ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    let pending = &ctx.model.ui.pending_chords;
    if pending.is_empty() {
        return None;
    }
    let t = ctx.theme;
    let line = row![
        text(pending.join(" "))
            .size(tokens::font_size::UI)
            .font(fonts::mono())
            .style(style::text::primary(t)),
        text(ctx.t(Key::KeymapPending))
            .size(tokens::font_size::UI)
            .style(style::text::secondary(t)),
    ]
    .spacing(tokens::space::MD)
    .align_y(Alignment::Center);
    Some(
        container(line)
            .padding([tokens::space::XS, tokens::space::MD])
            .style(style::container::tooltip(t))
            .into(),
    )
}
