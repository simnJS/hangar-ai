//! The keyboard shortcut editor of Settings › Shortcuts
//! (`ShortcutsSettings.tsx`).
//!
//! The keymap is P7's (`crate::keymap`): the editor reads the resolved
//! `KeymapIndex` the shell publishes (every command's keys, what the person
//! changed, the conflicts, the rivals of a keystroke) and asks the keymap to
//! write, with its `Bind`, `Unbind`, `Free`, `Reset` and `ResetAll` messages.
//! What is left here is the editor itself: the search, the recorder, and the
//! rows.
//!
//! Recording a shortcut takes the whole keyboard. The field that records
//! (hangar-ui's `shortcut_field`) listens to every key and spends it, and the
//! dispatcher is told to stand down for as long as one is recording
//! (`Msg::Suspend`), since the keys are the shortcut being recorded and not
//! commands to run. Escape is the recorder's then, and not the page's
//! (`Layer::escape(false)` around the sheet, and the page's own Escape
//! standing down while `State::is_recording` holds).

use hangar_i18n::{Key, args};
use hangar_ui::matching::fold;
use hangar_ui::shortcut::{
    Input, Outcome, Recorder, format_binding_on, parse_binding_on, write_binding,
};
use hangar_ui::shortcut_field::{Labels, shortcut_field};
use hangar_ui::{badge, button, input, text as typography, tip};
use iced::widget::{column, container, mouse_area, row};
use iced::{Alignment, Element, Length, Padding, Task};

use hangar_theme::style;
use hangar_theme::tokens::space;

use crate::commands::{CommandId, id::Section};
use crate::ctx::Ctx;
use crate::keymap::{self, KeymapIndex};
use crate::message::Message;

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

/// The token the page holds the dispatcher's suspension by: any, as long as
/// it is the same one for the suspension and its release.
const RECORDING: u64 = 0x5e77_1465;

#[derive(Debug, Default)]
pub struct State {
    query: String,
    /// The command being recorded for, and the recorder.
    recording: Option<(CommandId, Recorder)>,
    confirm_reset: bool,
}

impl State {
    /// Whether a recorder holds the keyboard. The page's Escape leaves it
    /// alone then.
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// The page is left (closed, or another category shown): a recording
    /// ends and lets go of the keyboard.
    pub fn leave(&mut self) -> Task<Message> {
        let was_recording = self.recording.is_some();
        *self = State::default();
        if was_recording {
            send(keymap::Msg::Release(RECORDING))
        } else {
            Task::none()
        }
    }

    /// Ends a recording, if there is one: the dispatcher is to be let go of.
    fn stop_recording(&mut self) -> Vec<keymap::Msg> {
        self.recording
            .take()
            .map(|_| keymap::Msg::Release(RECORDING))
            .into_iter()
            .collect()
    }
}

fn send(msg: keymap::Msg) -> Task<Message> {
    Task::done(Message::Keymap(msg))
}

fn wrap(msg: Msg) -> Message {
    Message::Settings(super::Msg::Shortcuts(msg))
}

/// What a message of the editor asks of the keymap, in order.
pub type Effect = Vec<keymap::Msg>;

/// What a message does to the editor, given the keymap as it stands. Pure
/// apart from the editor's own state, so that the whole of its behaviour can
/// be driven without a window.
pub fn step(st: &mut State, index: &KeymapIndex, msg: Msg) -> Effect {
    match msg {
        Msg::Query(query) => {
            st.query = query;
            st.stop_recording()
        }
        Msg::ClearQuery => {
            st.query.clear();
            Vec::new()
        }
        Msg::SearchPressed => st.stop_recording(),
        Msg::Record(id) => {
            let was_recording = st.recording.is_some();
            st.recording = Some((id, Recorder::new()));
            // Moving the recorder from one row to another keeps the one
            // suspension it holds.
            if was_recording {
                Vec::new()
            } else {
                vec![keymap::Msg::Suspend(RECORDING)]
            }
        }
        Msg::Input(input) => {
            let Some((id, recorder)) = &mut st.recording else {
                return Vec::new();
            };
            let id = *id;
            match recorder.input(input) {
                Outcome::Pending => Vec::new(),
                Outcome::Cancelled => st.stop_recording(),
                Outcome::Done(chords) => bind(st, id, write_binding(&chords)),
            }
        }
        Msg::Confirm => {
            let Some((id, recorder)) = &st.recording else {
                return Vec::new();
            };
            let id = *id;
            match recorder.confirm() {
                Some(chords) => bind(st, id, write_binding(&chords)),
                None => Vec::new(),
            }
        }
        Msg::Cancel => st.stop_recording(),
        Msg::Remove(id, at) => match index.bindings(id).get(at) {
            Some(binding) => vec![keymap::Msg::Unbind {
                command: id,
                binding: binding.clone(),
            }],
            None => Vec::new(),
        },
        Msg::Reset(id) => vec![keymap::Msg::Reset(id)],
        Msg::Free(id, at) => {
            let Some(binding) = index.bindings(id).get(at) else {
                return Vec::new();
            };
            let Some(chords) = parse_binding_on(index.platform(), binding) else {
                return Vec::new();
            };
            vec![keymap::Msg::Free {
                binding: binding.clone(),
                commands: index.commands_for(&chords, Some(id)),
            }]
        }
        Msg::AskResetAll(ask) => {
            st.confirm_reset = ask;
            Vec::new()
        }
        Msg::ResetAll => {
            st.confirm_reset = false;
            vec![keymap::Msg::ResetAll]
        }
    }
}

/// A recorded key added to a command, and the recorder let go of.
fn bind(st: &mut State, command: CommandId, binding: String) -> Effect {
    let mut asked = vec![keymap::Msg::Bind { command, binding }];
    asked.extend(st.stop_recording());
    asked
}

pub fn update(st: &mut State, m: &crate::state::Model, msg: Msg) -> Task<Message> {
    let index = m.live.keymap.index.clone();
    Task::batch(step(st, &index, msg).into_iter().map(send))
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
pub fn matches(ctx: Ctx<'_>, index: &KeymapIndex, id: CommandId, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let haystack = [
        command_label(ctx, id),
        index
            .bindings(id)
            .iter()
            .map(|binding| format_binding_on(index.platform(), binding))
            .collect::<Vec<_>>()
            .join(" "),
        ctx.t(id.section().label_key()).into_owned(),
    ]
    .join(" ");
    fold(&haystack).contains(needle)
}

/// The commands the editor lists for a query, by section in the order of the
/// page (`groups`).
pub fn groups(ctx: Ctx<'_>, index: &KeymapIndex, query: &str) -> Vec<(Section, Vec<CommandId>)> {
    let needle = fold(query.trim());
    Section::ALL
        .iter()
        .map(|section| {
            let commands: Vec<CommandId> = CommandId::ALL
                .iter()
                .copied()
                .filter(|id| id.section() == *section && matches(ctx, index, *id, &needle))
                .collect();
            (*section, commands)
        })
        .filter(|(_, commands)| !commands.is_empty())
        .collect()
}

/// Other commands answering to the same keystrokes as this one, by the
/// position of the key in its list.
pub fn rivals(index: &KeymapIndex, id: CommandId) -> Vec<(usize, Vec<CommandId>)> {
    index
        .bindings(id)
        .iter()
        .enumerate()
        .filter_map(|(at, binding)| {
            let chords = parse_binding_on(index.platform(), binding)?;
            let others = index.commands_for(&chords, Some(id));
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
    // The keymap the shell publishes: its keys live as long as the frame, so
    // the controls can borrow them.
    let index = ctx.keymap;
    let conflicts = index.conflicts();

    let search = mouse_area(crate::keymap::guard(
        ctx,
        input::search_field(t, &st.query, placeholder(ctx, Key::KeymapSearch))
            .clear_tip(ctx.t(Key::KeymapClear).into_owned())
            .on_input(|text| wrap(Msg::Query(text)))
            .on_clear(wrap(Msg::ClearQuery)),
    ))
    .on_press(wrap(Msg::SearchPressed));

    let state: Element<'a, Message> = if conflicts.is_empty() {
        typography::muted(t, ctx.t(Key::KeymapNoConflict).into_owned()).into()
    } else {
        let all: Vec<String> = conflicts
            .iter()
            .map(|conflict| format_binding_on(index.platform(), &conflict.binding))
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

    let customized = !index.overrides().is_empty();
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
    let groups = groups(ctx, index, &st.query);
    if groups.is_empty() {
        stack = stack.push(
            container(typography::label(t, ctx.t(Key::KeymapEmpty).into_owned()))
                .padding(Padding::from([space::XXL, space::XL])),
        );
    }
    for (section, commands) in groups {
        let rows = commands
            .into_iter()
            .map(|id| command_row(st, ctx, id))
            .collect();
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

fn command_row<'a>(st: &'a State, ctx: Ctx<'a>, id: CommandId) -> Element<'a, Message> {
    let t = ctx.theme;
    let index = ctx.keymap;
    let bindings = index.bindings(id);
    let custom = index.overrides().is_customized(id);
    let rival_list = rivals(index, id);
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

    let conflicted = !rival_list.is_empty();
    for (at, others) in rival_list {
        let keys = format_binding_on(index.platform(), &bindings[at]);
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

    let body = container(stack)
        .padding(Padding::from([space::LG, space::XL]))
        .width(Length::Fill);
    if conflicted {
        body.style(style::container::row_conflict(t)).into()
    } else {
        body.into()
    }
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
