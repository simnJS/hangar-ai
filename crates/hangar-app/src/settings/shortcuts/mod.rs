//! The keyboard shortcut editor of Settings › Shortcuts
//! (`ShortcutsSettings.tsx`).
//!
//! Deliberately self-contained: it reads the settings and nothing else, so it
//! drops into the page as one block among controls that are all
//! label-plus-widget rows and could not describe this one.
//!
//! Recording a shortcut takes the whole keyboard: the field that records
//! (hangar-ui's `shortcut_field`) listens to every key and spends it, and the
//! dispatcher is told to stand down for as long as one is recording
//! (`Intent::SuspendShortcuts`), since its keys are the shortcut being
//! recorded and not commands to run. Escape is the recorder's then, and not
//! the page's (`Layer::escape(false)` around the sheet, and the page's own
//! Escape ignoring it while `State::recording` holds).

use hangar_i18n::{Key, args};
use hangar_ui::matching::fold;
use hangar_ui::shortcut::{Input, Outcome, Recorder, format_binding, parse_binding, write_binding};
use hangar_ui::shortcut_field::{Labels, shortcut_field};
use hangar_ui::{badge, button, input, text as typography, tip};
use iced::widget::{column, container, mouse_area, row};
use iced::{Alignment, Element, Length, Padding, Task};

use hangar_theme::style;
use hangar_theme::tokens::space;

pub mod keymap;

use self::keymap::{
    Keymap, Overrides, bindings_of, commands_for, find_conflicts, is_customized, reset_command,
    resolve, with_binding, without_binding,
};
use crate::commands::{CommandId, id::Section};
use crate::ctx::Ctx;
use crate::message::{Intent, Message};
use crate::state::{Model, Patch, Settings, StoreAction};

#[derive(Debug, Clone)]
pub enum Msg {
    Query(String),
    ClearQuery,
    /// The search field was pressed: anything typed there must reach it, so
    /// a recording ends (focusing the search cancels any recording).
    SearchPressed,
    /// The plus of a command: record another key for it.
    Record(CommandId),
    /// A keyboard event, while recording.
    Input(Input),
    Confirm,
    Cancel,
    /// The cross on the key at this position of the command.
    Remove(CommandId, usize),
    Reset(CommandId),
    /// Take the key at this position of the command away from whoever else
    /// answers to it.
    Free(CommandId, usize),
    AskResetAll(bool),
    ResetAll,
}

#[derive(Debug, Default)]
pub struct State {
    query: String,
    /// The command being recorded for, and the recorder.
    recording: Option<(CommandId, Recorder)>,
    confirm_reset: bool,
    /// What the rows are drawn from. Held here and not made in the view
    /// because the controls borrow the keys for as long as the frame lives.
    cache: Option<Cache>,
}

#[derive(Debug)]
struct Cache {
    overrides: Overrides,
    keymap: Keymap,
}

impl State {
    /// Whether a recorder holds the keyboard. The page's Escape leaves it
    /// alone then.
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Makes the keymap the rows are drawn from match the settings. Called
    /// after every message of the page, so a change made anywhere shows.
    pub fn refresh(&mut self, settings: &Settings) {
        if self
            .cache
            .as_ref()
            .is_some_and(|cache| cache.overrides == settings.keybindings)
        {
            return;
        }
        self.cache = Some(Cache {
            keymap: resolve(&settings.keybindings),
            overrides: settings.keybindings.clone(),
        });
    }

    /// The page is left (closed, or another category shown): a recording
    /// ends and lets go of the keyboard.
    pub fn leave(&mut self) -> Task<Message> {
        let was_recording = self.recording.is_some();
        *self = State::default();
        if was_recording {
            release()
        } else {
            Task::none()
        }
    }

    /// Ends a recording, if there is one: whether the dispatcher is to be let
    /// go of.
    fn stop_recording(&mut self) -> Option<bool> {
        self.recording.take().map(|_| false)
    }
}

fn suspend(on: bool) -> Task<Message> {
    Task::done(Message::Intent(Intent::SuspendShortcuts(on)))
}

fn release() -> Task<Message> {
    suspend(false)
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Shortcuts(msg))
}

/// What a message of the editor asks of the outside: new overrides to write,
/// and the dispatcher to suspend (`true`) or let go (`false`).
#[derive(Debug, Default, PartialEq)]
pub struct Effect {
    pub write: Option<Overrides>,
    pub suspend: Option<bool>,
}

impl Effect {
    fn write(next: Overrides) -> Effect {
        Effect {
            write: Some(next),
            suspend: None,
        }
    }

    fn suspend(on: Option<bool>) -> Effect {
        Effect {
            write: None,
            suspend: on,
        }
    }
}

/// What a message does to the editor, given the overrides and the keymap
/// resolved from them. Pure, so that the whole of the editor's behaviour can
/// be driven without a window.
pub fn step(st: &mut State, overrides: &Overrides, keymap: &Keymap, msg: Msg) -> Effect {
    match msg {
        Msg::Query(query) => {
            st.query = query;
            Effect::suspend(st.stop_recording())
        }
        Msg::ClearQuery => {
            st.query.clear();
            Effect::default()
        }
        Msg::SearchPressed => Effect::suspend(st.stop_recording()),
        Msg::Record(id) => {
            let was_recording = st.recording.is_some();
            st.recording = Some((id, Recorder::new()));
            // Moving the recorder from one row to another keeps the one
            // suspension it holds.
            Effect::suspend((!was_recording).then_some(true))
        }
        Msg::Input(input) => {
            let Some((id, recorder)) = &mut st.recording else {
                return Effect::default();
            };
            let id = *id;
            match recorder.input(input) {
                Outcome::Pending => Effect::default(),
                Outcome::Cancelled => Effect::suspend(st.stop_recording()),
                Outcome::Done(chords) => Effect {
                    write: Some(with_binding(overrides, keymap, id, &write_binding(&chords))),
                    suspend: st.stop_recording(),
                },
            }
        }
        Msg::Confirm => {
            let Some((id, recorder)) = &st.recording else {
                return Effect::default();
            };
            let id = *id;
            match recorder.confirm() {
                Some(chords) => Effect {
                    write: Some(with_binding(overrides, keymap, id, &write_binding(&chords))),
                    suspend: st.stop_recording(),
                },
                None => Effect::default(),
            }
        }
        Msg::Cancel => Effect::suspend(st.stop_recording()),
        Msg::Remove(id, at) => match bindings_of(keymap, id).get(at) {
            Some(binding) => Effect::write(without_binding(overrides, keymap, id, binding)),
            None => Effect::default(),
        },
        Msg::Reset(id) => Effect::write(reset_command(overrides, id)),
        Msg::Free(id, at) => {
            let Some(binding) = bindings_of(keymap, id).get(at) else {
                return Effect::default();
            };
            let Some(chords) = parse_binding(binding) else {
                return Effect::default();
            };
            let mut next = overrides.clone();
            for rival in commands_for(keymap, &chords, Some(id)) {
                next = without_binding(&next, keymap, rival, binding);
            }
            Effect::write(next)
        }
        Msg::AskResetAll(ask) => {
            st.confirm_reset = ask;
            Effect::default()
        }
        Msg::ResetAll => {
            st.confirm_reset = false;
            Effect::write(Overrides::new())
        }
    }
}

pub fn update(st: &mut State, m: &mut Model, msg: Msg) -> Task<Message> {
    st.refresh(m.settings());
    let Some(cache) = &st.cache else {
        return Task::none();
    };
    let (overrides, keymap) = (cache.overrides.clone(), cache.keymap.clone());
    let effect = step(st, &overrides, &keymap, msg);
    if let Some(next) = effect.write {
        m.apply(StoreAction::UpdateSettings(Patch::new(
            move |settings: &mut Settings| settings.keybindings = next.clone(),
        )));
        // What the rows are drawn from follows the write at once.
        st.refresh(m.settings());
    }
    match effect.suspend {
        Some(on) => suspend(on),
        None => Task::none(),
    }
}

// ---- What the rows say ----------------------------------------------------------------------------

/// The label of a command, in the interface's language. The three digit
/// families share one label with an `{n}` (`labelParams`).
pub fn command_label(ctx: Ctx<'_>, id: CommandId) -> String {
    match id.label_n() {
        Some(n) => ctx
            .i18n
            .t_with(id.label_key(), &args!("n" => n))
            .into_owned(),
        None => ctx.t(id.label_key()).into_owned(),
    }
}

/// Whether a command is shown for a query: its label, its keys and its
/// section's name are searched, ignoring case and accents.
pub fn matches(ctx: Ctx<'_>, keymap: &Keymap, id: CommandId, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let haystack = [
        command_label(ctx, id),
        bindings_of(keymap, id)
            .iter()
            .map(|binding| format_binding(binding))
            .collect::<Vec<_>>()
            .join(" "),
        ctx.t(id.section().label_key()).into_owned(),
    ]
    .join(" ");
    fold(&haystack).contains(needle)
}

/// The commands the editor lists for a query, by section in the order of the
/// page (`groups`).
pub fn groups(ctx: Ctx<'_>, keymap: &Keymap, query: &str) -> Vec<(Section, Vec<CommandId>)> {
    let needle = fold(query.trim());
    Section::ALL
        .iter()
        .map(|section| {
            let commands: Vec<CommandId> = CommandId::ALL
                .iter()
                .copied()
                .filter(|id| id.section() == *section && matches(ctx, keymap, *id, &needle))
                .collect();
            (*section, commands)
        })
        .filter(|(_, commands)| !commands.is_empty())
        .collect()
}

/// Other commands answering to the same keystrokes as this one, by the
/// position of the key in its list.
pub fn rivals(keymap: &Keymap, id: CommandId) -> Vec<(usize, Vec<CommandId>)> {
    bindings_of(keymap, id)
        .iter()
        .enumerate()
        .filter_map(|(at, binding)| {
            let chords = parse_binding(binding)?;
            let others = commands_for(keymap, &chords, Some(id));
            (!others.is_empty()).then_some((at, others))
        })
        .collect()
}

// ---- Drawing ------------------------------------------------------------------------------------

fn labels(ctx: Ctx<'_>) -> Labels {
    Labels {
        prompt: ctx.t(Key::KeymapRecording).into_owned(),
        hint: ctx.t(Key::KeymapRecordingHint).into_owned(),
        confirm: ctx.t(Key::KeymapConfirm).into_owned(),
        cancel: ctx.t(Key::KeymapCancel).into_owned(),
        add: ctx.t(Key::KeymapAdd).into_owned(),
        // The field fills `{keys}` in with the key it names.
        remove: ctx.t(Key::KeymapRemove).into_owned(),
        reset: ctx.t(Key::KeymapReset).into_owned(),
        unbound: ctx.t(Key::KeymapUnbound).into_owned(),
    }
}

pub fn view<'a>(st: &'a State, ctx: Ctx<'a>) -> Element<'a, Message> {
    let t = ctx.theme;
    // The cache is made by the page's update, before anything is drawn; a
    // frame before the first update shows the editor empty rather than wrong.
    let Some(cache) = &st.cache else {
        return column![].into();
    };
    let keymap = &cache.keymap;
    let conflicts = find_conflicts(keymap);

    let search = mouse_area(
        input::search_field(t, &st.query, placeholder(ctx, Key::KeymapSearch))
            .clear_tip(ctx.t(Key::KeymapClear).into_owned())
            .on_input(|text| wrap(Msg::Query(text)))
            .on_clear(wrap(Msg::ClearQuery)),
    )
    .on_press(wrap(Msg::SearchPressed));

    let state: Element<'a, Message> = if conflicts.is_empty() {
        typography::muted(t, ctx.t(Key::KeymapNoConflict).into_owned()).into()
    } else {
        let all: Vec<String> = conflicts
            .iter()
            .map(|conflict| format_binding(&conflict.binding))
            .collect();
        tip::over(
            t,
            typography::danger(
                t,
                ctx.i18n
                    .tn(Key::KeymapConflicts, conflicts.len(), &[])
                    .into_owned(),
            ),
            all.join(" · "),
        )
    };

    let customized = CommandId::ALL
        .iter()
        .any(|id| is_customized(&cache.overrides, *id));
    let reset: Element<'a, Message> = if st.confirm_reset {
        row![
            button::ghost(t, ctx.t(Key::CreateCancel).into_owned())
                .on_press(wrap(Msg::AskResetAll(false))),
            button::danger(t, ctx.t(Key::KeymapResetAllConfirm).into_owned())
                .on_press(wrap(Msg::ResetAll)),
        ]
        .spacing(space::LG)
        .into()
    } else {
        tip::over(
            t,
            button::secondary(t, ctx.t(Key::KeymapResetAll).into_owned())
                .on_press_maybe(customized.then(|| wrap(Msg::AskResetAll(true)))),
            ctx.t(Key::KeymapResetAllHint).into_owned(),
        )
    };

    let bar = row![container(search).width(Length::Fill), state, reset]
        .spacing(space::XL)
        .align_y(Alignment::Center);

    let mut stack = column![bar].spacing(space::XXL);
    let groups = groups(ctx, keymap, &st.query);
    if groups.is_empty() {
        stack = stack.push(
            container(typography::label(t, ctx.t(Key::KeymapEmpty).into_owned()))
                .padding(Padding::from([space::XXL, space::XL])),
        );
    }
    for (section, commands) in groups {
        let mut rows = Vec::new();
        for id in commands {
            rows.push(command_row(st, ctx, cache, id));
        }
        stack = stack.push(
            column![
                container(typography::heading(
                    t,
                    ctx.t(section.label_key()).into_owned()
                ))
                .padding(Padding::from([0.0, space::XS])),
                hangar_ui::form::group(t, rows),
            ]
            .spacing(space::MD),
        );
    }
    stack.into()
}

fn command_row<'a>(
    st: &'a State,
    ctx: Ctx<'a>,
    cache: &'a Cache,
    id: CommandId,
) -> Element<'a, Message> {
    let t = ctx.theme;
    let bindings = bindings_of(&cache.keymap, id);
    let custom = is_customized(&cache.overrides, id);
    let rival_list = rivals(&cache.keymap, id);
    let recording = st
        .recording
        .as_ref()
        .filter(|(recording_id, _)| *recording_id == id)
        .map(|(_, recorder)| recorder);

    let field = shortcut_field::<Message>(t, bindings, recording)
        .labels(labels(ctx))
        .conflicts(rival_list.iter().map(|(at, _)| *at).collect())
        .customized(custom)
        .on_record(wrap(Msg::Record(id)))
        .on_input(|input| wrap(Msg::Input(input)))
        .on_confirm(wrap(Msg::Confirm))
        .on_cancel(wrap(Msg::Cancel))
        .on_remove(move |at| wrap(Msg::Remove(id, at)))
        .on_reset(wrap(Msg::Reset(id)));

    let mut label = row![typography::body(t, command_label(ctx, id))]
        .spacing(space::LG)
        .align_y(Alignment::Center);
    if custom {
        label = label.push(badge::tag(t, ctx.t(Key::KeymapCustom).into_owned()));
    }

    let mut stack = column![
        row![
            container(label).width(Length::FillPortion(2)),
            container(Element::from(field)).width(Length::FillPortion(3)),
        ]
        .spacing(space::XL)
        .align_y(Alignment::Center)
    ]
    .spacing(space::SM);

    for (at, others) in rival_list {
        let keys = format_binding(&bindings[at]);
        let names: Vec<String> = others
            .iter()
            .map(|other| command_label(ctx, *other))
            .collect();
        stack = stack.push(
            row![
                typography::danger(
                    t,
                    ctx.i18n
                        .t_with(
                            Key::KeymapAlsoBound,
                            &args!("keys" => keys, "commands" => names.join(", ")),
                        )
                        .into_owned(),
                ),
                button::link(
                    t,
                    ctx.t(Key::KeymapFree).into_owned(),
                    wrap(Msg::Free(id, at)),
                ),
            ]
            .spacing(space::LG)
            .align_y(Alignment::Center),
        );
    }

    let conflicted = !rivals_empty(&cache.keymap, id);
    let body = container(stack)
        .padding(Padding::from([space::LG, space::XL]))
        .width(Length::Fill);
    if conflicted {
        body.style(style::container::row_conflict(t)).into()
    } else {
        body.into()
    }
}

fn rivals_empty(keymap: &Keymap, id: CommandId) -> bool {
    rivals(keymap, id).is_empty()
}

/// A plain entry of the dictionary, which the translator lends for the whole
/// program: what a search field wants as its placeholder.
fn placeholder(ctx: Ctx<'_>, key: Key) -> &'static str {
    match ctx.i18n.t(key) {
        std::borrow::Cow::Borrowed(words) => words,
        std::borrow::Cow::Owned(_) => "",
    }
}

#[cfg(test)]
mod tests;
