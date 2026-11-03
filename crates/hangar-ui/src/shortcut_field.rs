//! The control of the Shortcuts settings: a command's keys, and the field that
//! records a new one (`ShortcutsSettings.tsx`).
//!
//! At rest it shows the keys the command answers to, each with a cross that
//! takes it away, and a plus that starts recording. While recording, it shows
//! what has been pressed so far, or the modifiers held, and listens to the
//! whole keyboard: everything pressed is the shortcut and none of it reaches
//! the rest of the window.
//!
//! ```ignore
//! use hangar_ui::shortcut::{Outcome, Recorder};
//! use hangar_ui::shortcut_field::shortcut_field;
//!
//! // The app keeps a `Recorder` while one is recording:
//! shortcut_field(t, &keys, self.recording.as_ref().filter(|(id, _)| *id == command).map(|(_, r)| r))
//!     .on_record(Msg::Record(command))
//!     .on_input(Msg::RecorderInput)
//!     .on_confirm(Msg::RecorderConfirm)
//!     .on_cancel(Msg::RecorderCancel)
//!     .on_remove(move |at| Msg::Remove(command, at))
//!
//! // In `update`, the recorder is fed what the field reports:
//! Msg::RecorderInput(input) => match recorder.input(input) {
//!     Outcome::Done(chords) => { self.bind(command, write_binding(&chords)); self.recording = None }
//!     Outcome::Cancelled => self.recording = None,
//!     Outcome::Pending => {}
//! }
//! ```
//!
//! Escape means "cancel" to the recorder, and to a dialog it means "close": a
//! settings dialog around the field must not close under it, so its layer
//! turns Escape off while something is recording (`Layer::escape(false)`).

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, style};
use iced::widget::{container, row, space as iced_space, text};
use iced::{Alignment, Element, Length, Padding};

use crate::button::icon_button;
use crate::icon::Icon;
use crate::keycap;
use crate::keys::on_keyboard;
use crate::shortcut::{Input, Recorder, input_from_event};
use crate::text as typography;

/// What the field says, so that the app can translate it. The defaults are
/// English.
#[derive(Debug, Clone)]
pub struct Labels {
    /// Shown while recording and nothing is pressed.
    pub prompt: String,
    /// After the keys being recorded.
    pub hint: String,
    pub confirm: String,
    pub cancel: String,
    pub add: String,
    /// The tip of the cross on a key; `{keys}` is replaced.
    pub remove: String,
    pub reset: String,
    /// Shown for a command with no keys.
    pub unbound: String,
}

impl Default for Labels {
    fn default() -> Self {
        Labels {
            prompt: String::from("Press the keys…"),
            hint: String::from("Enter to confirm, Esc to cancel"),
            confirm: String::from("Confirm"),
            cancel: String::from("Cancel"),
            add: String::from("Add a shortcut"),
            remove: String::from("Remove {keys}"),
            reset: String::from("Restore the default"),
            unbound: String::from("Not set"),
        }
    }
}

#[must_use]
pub struct ShortcutField<'a, M> {
    theme: &'a Theme,
    bindings: &'a [String],
    recording: Option<&'a Recorder>,
    labels: Labels,
    conflicts: Vec<usize>,
    customized: bool,
    on_record: Option<M>,
    on_input: Option<Box<dyn Fn(Input) -> M + 'a>>,
    on_confirm: Option<M>,
    on_cancel: Option<M>,
    on_remove: Option<Box<dyn Fn(usize) -> M + 'a>>,
    on_reset: Option<M>,
}

/// The keys of one command, each as the settings write it, and the recorder if
/// one is recording for it.
pub fn shortcut_field<'a, M>(
    t: &'a Theme,
    bindings: &'a [String],
    recording: Option<&'a Recorder>,
) -> ShortcutField<'a, M> {
    ShortcutField {
        theme: t,
        bindings,
        recording,
        labels: Labels::default(),
        conflicts: Vec::new(),
        customized: false,
        on_record: None,
        on_input: None,
        on_confirm: None,
        on_cancel: None,
        on_remove: None,
        on_reset: None,
    }
}

impl<'a, M> ShortcutField<'a, M> {
    pub fn labels(mut self, labels: Labels) -> Self {
        self.labels = labels;
        self
    }

    /// The keys (by position) that another command answers to as well: they
    /// turn red.
    pub fn conflicts(mut self, conflicts: Vec<usize>) -> Self {
        self.conflicts = conflicts;
        self
    }

    /// The command's keys differ from the defaults: a reset button shows.
    pub fn customized(mut self, customized: bool) -> Self {
        self.customized = customized;
        self
    }

    /// The plus was pressed: start recording.
    pub fn on_record(mut self, message: M) -> Self {
        self.on_record = Some(message);
        self
    }

    /// A keyboard event while recording, as the recorder takes it.
    pub fn on_input(mut self, on_input: impl Fn(Input) -> M + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    /// The check was pressed. Only sent once a chord has been recorded.
    pub fn on_confirm(mut self, message: M) -> Self {
        self.on_confirm = Some(message);
        self
    }

    /// The cross was pressed.
    pub fn on_cancel(mut self, message: M) -> Self {
        self.on_cancel = Some(message);
        self
    }

    /// The cross on the key at this position was pressed.
    pub fn on_remove(mut self, on_remove: impl Fn(usize) -> M + 'a) -> Self {
        self.on_remove = Some(Box::new(on_remove));
        self
    }

    /// The reset button was pressed.
    pub fn on_reset(mut self, message: M) -> Self {
        self.on_reset = Some(message);
        self
    }
}

impl<'a, M: Clone + 'a> From<ShortcutField<'a, M>> for Element<'a, M> {
    fn from(f: ShortcutField<'a, M>) -> Self {
        match f.recording {
            Some(recorder) => recording(f, recorder),
            None => at_rest(f),
        }
    }
}

fn at_rest<'a, M: Clone + 'a>(f: ShortcutField<'a, M>) -> Element<'a, M> {
    let t = f.theme;
    let mut line = row![].spacing(space::SM).align_y(Alignment::Center);
    for (at, binding) in f.bindings.iter().enumerate() {
        let mut caps = keycap::binding(t, binding);
        if f.conflicts.contains(&at) {
            caps = caps.conflict();
        }
        let mut one = row![Element::from(caps)]
            .spacing(space::XXS)
            .align_y(Alignment::Center);
        if let Some(on_remove) = &f.on_remove {
            let tip = f
                .labels
                .remove
                .replace("{keys}", &crate::shortcut::format_binding(binding));
            one = one.push(
                icon_button(t, Icon::Close)
                    .side(16.0)
                    .icon_size(10.0)
                    .tip(tip)
                    .danger()
                    .on_press(on_remove(at)),
            );
        }
        line = line.push(one);
    }
    if f.bindings.is_empty() {
        line = line.push(typography::hint(t, f.labels.unbound.clone()));
    }
    if let Some(message) = f.on_record {
        line = line.push(
            icon_button(t, Icon::Plus)
                .small()
                .icon_size(13.0)
                .tip(f.labels.add.clone())
                .on_press(message),
        );
    }
    if let (true, Some(message)) = (f.customized, f.on_reset) {
        line = line.push(
            icon_button(t, Icon::Reset)
                .small()
                .icon_size(13.0)
                .tip(f.labels.reset.clone())
                .on_press(message),
        );
    }
    line.into()
}

fn recording<'a, M: Clone + 'a>(f: ShortcutField<'a, M>, recorder: &'a Recorder) -> Element<'a, M> {
    let t = f.theme;
    let c = &t.colors;

    let dot = container(iced_space::horizontal())
        .width(7.0)
        .height(7.0)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(c.danger)),
            border: iced::Border {
                radius: 3.5.into(),
                ..iced::Border::default()
            },
            ..container::Style::default()
        });

    let shown: Element<'a, M> = if let Some(binding) = recorder.binding() {
        keycap::binding(t, binding).into()
    } else if let Some(held) = recorder.held() {
        keycap::held(t, &held.segments())
    } else {
        text(f.labels.prompt.clone())
            .size(font_size::CAPTION_LG)
            .style(style::text::primary(t))
            .into()
    };

    let confirm = icon_button(t, Icon::Check)
        .small()
        .icon_size(13.0)
        .tip(f.labels.confirm.clone())
        .on_press_maybe(recorder.binding().and_then(|_| f.on_confirm.clone()));
    let cancel = icon_button(t, Icon::Close)
        .small()
        .icon_size(13.0)
        .tip(f.labels.cancel.clone())
        .on_press_maybe(f.on_cancel.clone());

    let line = row![
        dot,
        shown,
        typography::hint(t, f.labels.hint.clone()).size(font_size::CAPTION),
        confirm,
        cancel,
    ]
    .spacing(space::MD)
    .align_y(Alignment::Center);
    let boxed = container(line)
        .padding(Padding {
            top: 2.0,
            right: 2.0,
            bottom: 2.0,
            left: 9.0,
        })
        .height(Length::Shrink)
        .style(style::container::recorder(t));

    match f.on_input {
        Some(on_input) => on_keyboard(boxed, move |event| input_from_event(event).map(&on_input)),
        None => boxed.into(),
    }
}

#[cfg(test)]
mod tests;
