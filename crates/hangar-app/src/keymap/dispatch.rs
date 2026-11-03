//! The dispatcher: what a key press means to the keymap (`useShortcuts.ts`).
//!
//! The web app had one listener on the window's capture phase, above every
//! widget. iced hands a key to the widget that has the keyboard first, so the
//! same decision is asked in three places here, and [`decide`] is the one
//! function all three ask:
//!
//! - the terminals' key filter (`hangar_term::TerminalView::key_filter`,
//!   wired by the pane package): a key the keymap takes is neither typed nor
//!   captured by the terminal;
//! - [`guard`](super::guard) around a text field, which takes such a key
//!   before the field can type it or submit on it;
//! - the subscription, which sees every press after the widgets, with
//!   whether one of them used it, and runs the command.
//!
//! The rules, as useShortcuts.ts has them:
//!
//! - a sequence waits [`SEQUENCE_TIMEOUT`] for its next chord;
//! - in a text field, a chord without Ctrl, Alt or Meta is the field's
//!   (unless it is a function key, which types nothing), and so are the
//!   editing chords (Ctrl+A/C/V/X/Z/Y, Cmd on a Mac). A terminal is not a
//!   field. Mid-sequence neither rule applies: the key is unambiguous then;
//! - the first command of the keymap that is live wins; a key with no live
//!   command is not taken, and reaches the terminal;
//! - the prefix of a longer sequence is taken and held;
//! - a sequence that leads nowhere swallows the key that ended it, rather
//!   than letting half a shortcut land in the terminal.
//!
//! The subscription does the cheap part off the UI thread: it lets through
//! only presses that start a binding, or any press while a sequence is half
//! typed, so the keys typed all day into a terminal never become a message.

use std::collections::BTreeSet;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

use iced::advanced::subscription::{self, EventStream, Hasher, Recipe};
use iced::futures::stream::BoxStream;
use iced::keyboard::key::Physical;
use iced::keyboard::{self, Key, Modifiers};
use iced::window;

use super::Msg;
use super::chord::Stroke;
use super::index::KeymapIndex;
use crate::commands::{self, CommandId};
use crate::message::Message;
use crate::shell::Overlay;
use crate::state::Model;

/// How long a half-typed sequence waits for the chord that would complete it.
pub const SEQUENCE_TIMEOUT: Duration = Duration::from_millis(1600);

/// What a key press comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Run this command; the key is spent.
    Run(CommandId),
    /// The start of a longer sequence: the key is spent and held.
    Hold,
    /// A half-typed sequence that this key leads nowhere: the key is spent.
    Swallow,
    /// Not the keymap's: the key goes on to whatever has the keyboard.
    Pass,
}

impl Outcome {
    /// Whether the keymap takes the key, so that no widget may use it.
    pub fn takes(self) -> bool {
        self != Outcome::Pass
    }
}

/// What `stroke` means after the strokes `pending`, typed into a text field
/// when `field` is set, with `live` saying which commands can run now.
pub fn decide(
    index: &KeymapIndex,
    pending: &[Stroke],
    stroke: Stroke,
    field: bool,
    live: &mut dyn FnMut(CommandId) -> bool,
) -> Outcome {
    let armed = !pending.is_empty();
    if !armed {
        // A chord without Ctrl/Alt/Meta is a character somewhere else; typing
        // it into a field is not asking for the command. A function key types
        // nothing, so F6 still leaves a field.
        if field && stroke.is_bare() && !stroke.is_function_key() {
            return Outcome::Pass;
        }
        // Copy, paste and select-all belong to the field the caret is in.
        if field && stroke.is_text_editing(index.platform()) {
            return Outcome::Pass;
        }
        if !index.is_head(stroke) {
            return Outcome::Pass;
        }
    }

    let mut sequence = Vec::with_capacity(pending.len() + 1);
    sequence.extend_from_slice(pending);
    sequence.push(stroke);

    // Several commands can share a keystroke (the settings flag it as a
    // conflict), and the first one that has something to do here wins.
    if let Some(id) = index.owners(&sequence).iter().copied().find(|id| live(*id)) {
        return Outcome::Run(id);
    }
    if index.is_prefix(&sequence) {
        return Outcome::Hold;
    }
    if armed {
        Outcome::Swallow
    } else {
        Outcome::Pass
    }
}

/// Which commands can run in a window now, asked of the shell's command table
/// once, and only when a key press is a binding at all.
pub struct Liveness<'a> {
    model: &'a Model,
    window: window::Id,
    available: Option<Vec<CommandId>>,
}

impl<'a> Liveness<'a> {
    pub fn new(model: &'a Model, window: window::Id) -> Liveness<'a> {
        Liveness {
            model,
            window,
            available: None,
        }
    }

    pub fn is_live(&mut self, id: CommandId) -> bool {
        // While the palette is open the keyboard is its own: a shortcut
        // acting on the window behind it would change what the list is about
        // mid-search. Only its own key stays, to close it.
        if self.model.ui.overlays.contains(&Overlay::Palette) {
            return id == CommandId::ViewPalette;
        }
        let (model, window) = (self.model, self.window);
        self.available
            .get_or_insert_with(|| commands::available(model, window))
            .contains(&id)
    }
}

/// The dispatcher's memory: the sequence half typed, and who asked it to
/// step aside.
#[derive(Debug, Default)]
pub struct Dispatcher {
    pending: Vec<Stroke>,
    /// Bumped by every hold, so that the timeout of an older one does nothing.
    held: u64,
    /// The recorders open in the Shortcuts settings. Recording a shortcut
    /// means typing keys that are themselves shortcuts, so the dispatcher
    /// steps aside while any is open. A set of tokens rather than a counter:
    /// two recorders closing in any order, or one released twice, still
    /// leave it right.
    suspensions: BTreeSet<u64>,
}

impl Dispatcher {
    /// A key press, as the subscription saw it.
    pub fn press(
        &mut self,
        index: &KeymapIndex,
        stroke: Stroke,
        field: bool,
        live: &mut dyn FnMut(CommandId) -> bool,
    ) -> Outcome {
        if self.is_suspended() {
            self.forget();
            return Outcome::Pass;
        }
        let outcome = decide(index, &self.pending, stroke, field, live);
        match outcome {
            Outcome::Run(_) | Outcome::Swallow => self.forget(),
            Outcome::Hold => {
                self.pending.push(stroke);
                self.held += 1;
            }
            Outcome::Pass => {}
        }
        outcome
    }

    /// The strokes of the sequence typed so far.
    pub fn pending(&self) -> &[Stroke] {
        &self.pending
    }

    pub fn is_armed(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The hold the next timeout is about.
    pub fn held(&self) -> u64 {
        self.held
    }

    /// The timeout of hold `held` came: forgets the sequence if no key has
    /// come since. Whether it did.
    pub fn time_out(&mut self, held: u64) -> bool {
        if held != self.held || self.pending.is_empty() {
            return false;
        }
        self.forget();
        true
    }

    pub fn forget(&mut self) {
        self.pending.clear();
    }

    pub fn suspend(&mut self, token: u64) {
        self.suspensions.insert(token);
        self.forget();
    }

    pub fn release(&mut self, token: u64) {
        self.suspensions.remove(&token);
    }

    /// True while a recorder owns the keyboard (`shortcutsSuspended`).
    /// Anything else listening to keys stands down too: Escape cancels the
    /// recording, and must not also close the settings it sits in.
    pub fn is_suspended(&self) -> bool {
        !self.suspensions.is_empty()
    }
}

/// Whether the keymap takes a key pressed in window `window`: what the
/// terminals' key filter and the fields' guard ask before they use it.
#[allow(clippy::too_many_arguments)]
pub fn takes(
    model: &Model,
    window: window::Id,
    key: &Key,
    physical: Physical,
    modifiers: Modifiers,
    text: Option<&str>,
    field: bool,
    released: bool,
) -> bool {
    let keymap = &model.live.keymap;
    if keymap.dispatcher.is_suspended() {
        return false;
    }
    let Some(stroke) = keymap.index.stroke_of_press(key, physical, modifiers, text) else {
        return false;
    };
    // A release belongs where its press went. The press of a shortcut has
    // run its command and ended the sequence by the time the release comes,
    // so the release is asked as a press with nothing pending would be.
    let pending = if released {
        &[][..]
    } else {
        keymap.dispatcher.pending()
    };
    let mut liveness = Liveness::new(model, window);
    decide(&keymap.index, pending, stroke, field, &mut |id| {
        liveness.is_live(id)
    })
    .takes()
}

/// The subscription's stream: the key presses the dispatcher may care about,
/// as messages, and nothing else.
pub struct KeyPresses {
    pub index: Arc<KeymapIndex>,
    pub armed: bool,
    pub suspended: bool,
}

impl KeyPresses {
    fn message(&self, event: subscription::Event) -> Option<Message> {
        let subscription::Event::Interaction {
            window,
            event:
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    physical_key,
                    modifiers,
                    text,
                    ..
                }),
            status,
        } = event
        else {
            return None;
        };
        if self.suspended {
            return None;
        }
        let stroke = self
            .index
            .stroke_of_press(&key, physical_key, modifiers, text.as_deref())?;
        if !self.armed && !self.index.is_head(stroke) {
            return None;
        }
        Some(Message::Keymap(Msg::Key {
            window,
            stroke,
            // A widget used the key: a field typed it, a dialog kept it, or
            // a guard took it for the keymap. Either way the field rules
            // apply; a key nothing used was pressed outside any field.
            field: status == iced::event::Status::Captured,
        }))
    }
}

impl Recipe for KeyPresses {
    type Output = Message;

    fn hash(&self, state: &mut Hasher) {
        std::any::TypeId::of::<KeyPresses>().hash(state);
        (self.index.generation(), self.armed, self.suspended).hash(state);
    }

    fn stream(self: Box<Self>, input: EventStream) -> BoxStream<'static, Message> {
        use iced::futures::StreamExt;
        let gate = *self;
        input
            .filter_map(move |event| std::future::ready(gate.message(event)))
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_ui::shortcut::Platform;
    use serde_json::{Map, Value, json};

    fn index(value: Value) -> KeymapIndex {
        let Value::Object(map) = value else {
            panic!("an object")
        };
        KeymapIndex::on(Platform::Other, &map)
    }

    fn stroke(index: &KeymapIndex, written: &str) -> Stroke {
        let strokes = index.strokes(written).unwrap();
        assert_eq!(strokes.len(), 1);
        strokes[0]
    }

    fn all_live(_: CommandId) -> bool {
        true
    }

    #[test]
    fn a_bound_chord_runs_its_command() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        let outcome =
            dispatcher.press(&index, stroke(&index, "Ctrl+Shift+P"), false, &mut all_live);
        assert_eq!(outcome, Outcome::Run(CommandId::ViewPalette));
        assert!(!dispatcher.is_armed());
    }

    #[test]
    fn a_chord_whose_command_is_not_live_is_not_taken() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        let outcome = dispatcher.press(&index, stroke(&index, "Ctrl+Shift+X"), false, &mut |_| {
            false
        });
        assert_eq!(outcome, Outcome::Pass);
    }

    #[test]
    fn the_first_live_command_of_a_shared_keystroke_wins() {
        let index = index(json!({"pane.split": ["F9"], "pane.close": ["F9"]}));
        let f9 = stroke(&index, "F9");
        let mut dispatcher = Dispatcher::default();
        assert_eq!(
            dispatcher.press(&index, f9, false, &mut all_live),
            Outcome::Run(CommandId::PaneSplit)
        );
        assert_eq!(
            dispatcher.press(&index, f9, false, &mut |id| id == CommandId::PaneClose),
            Outcome::Run(CommandId::PaneClose)
        );
    }

    #[test]
    fn a_sequence_is_held_then_run() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+K"), false, &mut all_live),
            Outcome::Hold
        );
        assert!(dispatcher.is_armed());
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+S"), false, &mut all_live),
            Outcome::Run(CommandId::ViewShortcuts)
        );
        assert!(!dispatcher.is_armed());
    }

    #[test]
    fn a_sequence_that_leads_nowhere_swallows_the_key_that_ended_it() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        dispatcher.press(&index, stroke(&index, "Ctrl+K"), false, &mut all_live);
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "A"), false, &mut all_live),
            Outcome::Swallow
        );
        assert!(!dispatcher.is_armed());
        // And the next one is ordinary again.
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "A"), false, &mut all_live),
            Outcome::Pass
        );
    }

    #[test]
    fn a_sequence_whose_command_is_not_live_is_swallowed_too() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        dispatcher.press(&index, stroke(&index, "Ctrl+K"), false, &mut all_live);
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+S"), false, &mut |_| false),
            Outcome::Swallow
        );
    }

    #[test]
    fn the_timeout_forgets_only_the_hold_it_was_set_for() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        dispatcher.press(&index, stroke(&index, "Ctrl+K"), false, &mut all_live);
        let first = dispatcher.held();
        assert!(dispatcher.time_out(first));
        assert!(!dispatcher.is_armed());

        dispatcher.press(&index, stroke(&index, "Ctrl+K"), false, &mut all_live);
        assert!(!dispatcher.time_out(first));
        assert!(dispatcher.is_armed());
    }

    #[test]
    fn a_field_keeps_bare_chords_and_its_editing_chords() {
        let index =
            index(json!({"pane.split": ["Q"], "pane.close": ["Ctrl+C"], "view.board": ["F9"]}));
        let mut dispatcher = Dispatcher::default();
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Q"), true, &mut all_live),
            Outcome::Pass
        );
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+C"), true, &mut all_live),
            Outcome::Pass
        );
        // A function key types nothing: F6 leaves a field.
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "F9"), true, &mut all_live),
            Outcome::Run(CommandId::ViewBoard)
        );
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "F6"), true, &mut all_live),
            Outcome::Run(CommandId::ViewNextRegion)
        );
        // Out of a field, and in a terminal, the same keys are shortcuts.
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Q"), false, &mut all_live),
            Outcome::Run(CommandId::PaneSplit)
        );
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+C"), false, &mut all_live),
            Outcome::Run(CommandId::PaneClose)
        );
    }

    #[test]
    fn mid_sequence_the_field_rules_do_not_apply() {
        let index = index(json!({"pane.split": ["Ctrl+K X"]}));
        let mut dispatcher = Dispatcher::default();
        dispatcher.press(&index, stroke(&index, "Ctrl+K"), true, &mut all_live);
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "X"), true, &mut all_live),
            Outcome::Run(CommandId::PaneSplit)
        );
    }

    #[test]
    fn a_suspended_dispatcher_takes_nothing_and_forgets_what_it_held() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut dispatcher = Dispatcher::default();
        dispatcher.press(&index, stroke(&index, "Ctrl+K"), false, &mut all_live);
        dispatcher.suspend(7);
        assert!(!dispatcher.is_armed());
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+Shift+P"), false, &mut all_live),
            Outcome::Pass
        );
        dispatcher.suspend(8);
        dispatcher.release(7);
        dispatcher.release(7);
        assert!(dispatcher.is_suspended());
        dispatcher.release(8);
        assert!(!dispatcher.is_suspended());
        assert_eq!(
            dispatcher.press(&index, stroke(&index, "Ctrl+Shift+P"), false, &mut all_live),
            Outcome::Run(CommandId::ViewPalette)
        );
    }

    #[test]
    fn an_unbound_key_is_answered_without_asking_what_is_live() {
        let index = KeymapIndex::on(Platform::Other, &Map::new());
        let mut asked = false;
        let outcome = decide(&index, &[], stroke(&index, "A"), false, &mut |_| {
            asked = true;
            true
        });
        assert_eq!(outcome, Outcome::Pass);
        assert!(!asked);
    }

    mod as_the_window_asks {
        use super::*;
        use iced::keyboard::key::{Code, Named};

        /// What `Mod` is held as on the platform the tests run on.
        fn platform_mod() -> Modifiers {
            match Platform::current() {
                Platform::Mac => Modifiers::LOGO,
                Platform::Other => Modifiers::CTRL,
            }
        }

        fn pressed(
            key: Key,
            code: Code,
            modifiers: Modifiers,
            status: iced::event::Status,
        ) -> subscription::Event {
            subscription::Event::Interaction {
                window: window::Id::unique(),
                event: iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    modified_key: key.clone(),
                    key,
                    physical_key: Physical::Code(code),
                    location: keyboard::Location::Standard,
                    modifiers,
                    text: None,
                    repeat: false,
                }),
                status,
            }
        }

        fn letter(c: &str, code: Code, modifiers: Modifiers) -> subscription::Event {
            pressed(
                Key::Character(c.into()),
                code,
                modifiers,
                iced::event::Status::Ignored,
            )
        }

        fn gate(armed: bool, suspended: bool) -> KeyPresses {
            KeyPresses {
                index: Arc::new(KeymapIndex::default()),
                armed,
                suspended,
            }
        }

        #[test]
        fn typing_never_becomes_a_message() {
            let gate = gate(false, false);
            assert!(
                gate.message(letter("a", Code::KeyA, Modifiers::empty()))
                    .is_none()
            );
            assert!(
                gate.message(letter("a", Code::KeyA, Modifiers::SHIFT))
                    .is_none()
            );
            assert!(
                gate.message(pressed(
                    Key::Named(Named::Enter),
                    Code::Enter,
                    Modifiers::empty(),
                    iced::event::Status::Captured
                ))
                .is_none()
            );
            // Ctrl+S starts no binding: the shell's XOFF stays the shell's.
            assert!(
                gate.message(letter("s", Code::KeyS, platform_mod()))
                    .is_none()
            );
        }

        #[test]
        fn the_start_of_a_binding_does_and_says_whether_a_widget_used_it() {
            let gate = gate(false, false);
            let palette = letter("p", Code::KeyP, platform_mod() | Modifiers::SHIFT);
            assert!(matches!(
                gate.message(palette),
                Some(Message::Keymap(Msg::Key { field: false, .. }))
            ));
            let in_a_field = pressed(
                Key::Named(Named::F6),
                Code::F6,
                Modifiers::empty(),
                iced::event::Status::Captured,
            );
            assert!(matches!(
                gate.message(in_a_field),
                Some(Message::Keymap(Msg::Key { field: true, .. }))
            ));
        }

        #[test]
        fn a_half_typed_sequence_hears_every_key_and_a_recorder_none() {
            assert!(
                gate(true, false)
                    .message(letter("s", Code::KeyS, platform_mod()))
                    .is_some()
            );
            assert!(
                gate(true, false)
                    .message(letter("a", Code::KeyA, Modifiers::empty()))
                    .is_some()
            );
            assert!(
                gate(false, true)
                    .message(letter("p", Code::KeyP, platform_mod() | Modifiers::SHIFT))
                    .is_none()
            );
        }

        fn takes_press(
            model: &Model,
            key: Key,
            code: Code,
            modifiers: Modifiers,
            field: bool,
        ) -> bool {
            takes(
                model,
                window::Id::unique(),
                &key,
                Physical::Code(code),
                modifiers,
                None,
                field,
                false,
            )
        }

        #[test]
        fn a_terminal_lets_the_live_shortcuts_through_and_keeps_the_rest() {
            let model = Model::default();
            let p = || Key::Character("p".into());
            assert!(takes_press(
                &model,
                p(),
                Code::KeyP,
                platform_mod() | Modifiers::SHIFT,
                false
            ));
            assert!(takes_press(
                &model,
                Key::Named(Named::F6),
                Code::F6,
                Modifiers::empty(),
                false
            ));
            assert!(!takes_press(
                &model,
                p(),
                Code::KeyP,
                Modifiers::empty(),
                false
            ));
            assert!(!takes_press(
                &model,
                Key::Character("c".into()),
                Code::KeyC,
                platform_mod(),
                false
            ));
        }

        #[test]
        fn while_the_palette_is_open_only_its_own_key_is_taken() {
            let mut model = Model::default();
            model.ui.overlays.push(Overlay::Palette);
            let p = Key::Character("p".into());
            assert!(takes_press(
                &model,
                p,
                Code::KeyP,
                platform_mod() | Modifiers::SHIFT,
                true
            ));
            assert!(!takes_press(
                &model,
                Key::Named(Named::F6),
                Code::F6,
                Modifiers::empty(),
                true
            ));
        }

        #[test]
        fn a_recorder_has_every_key() {
            let mut model = Model::default();
            model.live.keymap.dispatcher.suspend(1);
            let p = Key::Character("p".into());
            assert!(!takes_press(
                &model,
                p,
                Code::KeyP,
                platform_mod() | Modifiers::SHIFT,
                false
            ));
        }
    }
}
