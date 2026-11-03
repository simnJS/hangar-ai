//! Inputs: the controls a form is made of.
//!
//! ```ignore
//! use hangar_ui::input;
//!
//! // A field with its label, hint and error:
//! input::text_field(t, &self.name)
//!     .label("Name")
//!     .hint("Shown in the navigator.")
//!     .error(self.name.is_empty().then_some("A name is required."))
//!     .on_input(Msg::Name)
//!
//! input::search_field(t, &self.query, "Search settings").on_input(Msg::Query)
//! input::switch(t, self.on).on_toggle(Msg::Toggled)
//! input::slider(t, 90.0..=130.0, self.scale, Msg::Scale).step(5.0).display("100 %")
//! input::segmented(t, &[("dark", "Dark"), ("light", "Light")], &self.mode, Msg::Mode)
//! input::select(t, &self.shells, Some(self.shell.clone()), Msg::Shell)
//! ```
//!
//! Every control is stateless, as iced's are: the value is the view's, and a
//! change comes back as a message. The two that need a little state of their
//! own for what is half typed ([`NumberField`], the text editor's `Content`)
//! hold it in a struct the view keeps.
//!
//! The control that records a shortcut is in [`crate::form`].

use hangar_theme::tokens::{font_size, size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{
    Id, button, checkbox as iced_checkbox, column, container, pick_list, row,
    slider as iced_slider, space as iced_space, stack, text, text_editor, text_input, toggler,
};
use iced::{Alignment, Element, Length, Padding, Pixels};

use crate::button::icon_button;
use crate::icon::{Icon, icon};
use crate::text as typography;

/// A field's text is 12 px on a 16 px line, in a box 26 px tall.
const FIELD_PADDING: Padding = Padding {
    top: 5.0,
    right: space::MD,
    bottom: 5.0,
    left: space::MD,
};
const FIELD_LINE: f32 = 16.0;

fn line_height() -> text::LineHeight {
    text::LineHeight::Absolute(Pixels(FIELD_LINE))
}

// ---- Text -----------------------------------------------------------------------------------

/// A one-line field, with the label, the hint and the error that go with it.
#[must_use]
pub struct TextField<'a, M> {
    theme: &'a Theme,
    value: &'a str,
    placeholder: &'a str,
    label: Option<String>,
    hint: Option<String>,
    error: Option<String>,
    on_input: Option<Box<dyn Fn(String) -> M + 'a>>,
    on_submit: Option<M>,
    id: Option<Id>,
    mono: bool,
    secure: bool,
    width: Length,
}

/// A text field holding `value`. Without [`on_input`](TextField::on_input) it
/// is disabled.
pub fn text_field<'a, M>(t: &'a Theme, value: &'a str) -> TextField<'a, M> {
    TextField {
        theme: t,
        value,
        placeholder: "",
        label: None,
        hint: None,
        error: None,
        on_input: None,
        on_submit: None,
        id: None,
        mono: false,
        secure: false,
        width: Length::Fill,
    }
}

impl<'a, M> TextField<'a, M> {
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// A label above the field.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A line under the field saying what it is for.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Why the value is refused. The field turns red and the message replaces
    /// the hint.
    pub fn error(mut self, error: Option<impl Into<String>>) -> Self {
        self.error = error.map(Into::into);
        self
    }

    pub fn on_input(mut self, on_input: impl Fn(String) -> M + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    /// Enter pressed in the field.
    pub fn on_submit(mut self, message: M) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// The identity to focus the field by, with `iced::widget::operation::focus`.
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The mono face, for a command or a path.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// Dots instead of the characters.
    pub fn secure(mut self) -> Self {
        self.secure = true;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<TextField<'a, M>> for Element<'a, M> {
    fn from(field: TextField<'a, M>) -> Self {
        let t = field.theme;
        let invalid = field.error.is_some();
        let mut input = text_input(field.placeholder, field.value)
            .size(font_size::UI)
            .line_height(line_height())
            .padding(FIELD_PADDING)
            .secure(field.secure)
            .width(Length::Fill);
        if field.mono {
            input = input.font(fonts::mono());
        }
        if let Some(id) = field.id {
            input = input.id(id);
        }
        if let Some(on_input) = field.on_input {
            input = input.on_input(on_input);
        }
        if let Some(message) = field.on_submit {
            input = input.on_submit(message);
        }
        let input = input.style(move |theme: &iced::Theme, status| {
            if invalid {
                style::text_input::field_invalid(t)(theme, status)
            } else {
                style::text_input::field(t)(theme, status)
            }
        });

        let mut stack = column![].spacing(space::SM).width(field.width);
        if let Some(label) = field.label {
            stack = stack.push(typography::label(t, label).size(font_size::UI));
        }
        stack = stack.push(input);
        if let Some(error) = field.error {
            stack = stack.push(typography::danger(t, error));
        } else if let Some(hint) = field.hint {
            stack = stack.push(typography::hint(t, hint));
        }
        stack.into()
    }
}

/// A field with a magnifier before it and a clear button after it, shown
/// while there is something to clear (`.settings__search`).
#[must_use]
pub struct SearchField<'a, M> {
    theme: &'a Theme,
    value: &'a str,
    placeholder: &'a str,
    on_input: Option<Box<dyn Fn(String) -> M + 'a>>,
    on_clear: Option<M>,
    on_submit: Option<M>,
    id: Option<Id>,
    clear_tip: String,
    width: Length,
}

pub fn search_field<'a, M>(
    t: &'a Theme,
    value: &'a str,
    placeholder: &'a str,
) -> SearchField<'a, M> {
    SearchField {
        theme: t,
        value,
        placeholder,
        on_input: None,
        on_clear: None,
        on_submit: None,
        id: None,
        clear_tip: String::from("Clear"),
        width: Length::Fill,
    }
}

impl<'a, M> SearchField<'a, M> {
    pub fn on_input(mut self, on_input: impl Fn(String) -> M + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    /// The message of the clear button, which is only there while the field
    /// holds text. Without it, clearing sends `on_input` an empty string.
    pub fn on_clear(mut self, message: M) -> Self {
        self.on_clear = Some(message);
        self
    }

    pub fn on_submit(mut self, message: M) -> Self {
        self.on_submit = Some(message);
        self
    }

    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// What the clear button's tooltip says, in the interface's language.
    pub fn clear_tip(mut self, words: impl Into<String>) -> Self {
        self.clear_tip = words.into();
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

impl<'a, M: Clone + 'a> From<SearchField<'a, M>> for Element<'a, M> {
    fn from(field: SearchField<'a, M>) -> Self {
        let t = field.theme;
        // Emptying the field is a message the caller already handles, when it
        // has not named one for the button.
        let clear = if field.value.is_empty() {
            None
        } else {
            field.on_clear.clone().or_else(|| {
                field
                    .on_input
                    .as_ref()
                    .map(|on_input| on_input(String::new()))
            })
        };
        let mut input = text_input(field.placeholder, field.value)
            .size(font_size::UI)
            .line_height(line_height())
            .padding(Padding {
                left: 28.0,
                right: 28.0,
                ..FIELD_PADDING
            })
            .width(Length::Fill)
            .style(style::text_input::field(t));
        if let Some(id) = field.id {
            input = input.id(id);
        }
        if let Some(on_input) = field.on_input {
            input = input.on_input(on_input);
        }
        if let Some(message) = field.on_submit {
            input = input.on_submit(message);
        }

        // The magnifier and the clear button sit on the field, in a layer of
        // their own that lets everything but the button through.
        let magnifier = container(icon(Icon::Search, size::ICON).color(t.colors.fg_muted))
            .padding(Padding::from([0.0, space::LG]))
            .height(Length::Fill)
            .align_y(Alignment::Center);
        let mut overlay = row![magnifier, iced_space::horizontal()].align_y(Alignment::Center);
        if let Some(message) = clear {
            overlay = overlay.push(
                container(
                    icon_button(t, Icon::Close)
                        .small()
                        .icon_size(10.0)
                        .tip(field.clear_tip)
                        .on_press(message),
                )
                .padding(Padding::from([0.0, 2.0])),
            );
        }
        let overlay = container(overlay.height(Length::Fill)).height(Length::Fill);
        stack![input, overlay].width(field.width).into()
    }
}

// ---- Numbers --------------------------------------------------------------------------------

/// What a [`number_input`] says to the view.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberMsg {
    /// The text of the field changed.
    Input(String),
    /// A step button: one step up (1) or down (-1).
    Step(i32),
    /// Enter, or the field losing the keyboard: settle the text on the value.
    Commit,
}

/// The state of a number field: the number and the text it is being typed as.
///
/// A number cannot be edited as a number: "1." is on its way to "1.5", and an
/// empty field is on its way to anything. So the text is kept apart, the value
/// follows it whenever it reads as one inside the bounds, and
/// [`NumberMsg::Commit`] puts the text back in line with the value.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberField {
    value: f64,
    text: String,
    min: f64,
    max: f64,
    step: f64,
    decimals: usize,
}

impl NumberField {
    pub fn new(value: f64, min: f64, max: f64, step: f64) -> NumberField {
        let mut field = NumberField {
            value: value.clamp(min, max),
            text: String::new(),
            min,
            max,
            step,
            decimals: 0,
        };
        field.text = field.format(field.value);
        field
    }

    /// Whole numbers only (the default). With `decimals` places the field
    /// takes fractions and shows its value with that many.
    pub fn decimals(mut self, decimals: usize) -> NumberField {
        self.decimals = decimals;
        self.text = self.format(self.value);
        self
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn min(&self) -> f64 {
        self.min
    }

    pub fn max(&self) -> f64 {
        self.max
    }

    /// Sets the value from outside, as when a setting is reset.
    pub fn set(&mut self, value: f64) {
        self.value = value.clamp(self.min, self.max);
        self.text = self.format(self.value);
    }

    /// Whether what is typed is a number inside the bounds.
    pub fn is_valid(&self) -> bool {
        self.parse(&self.text).is_some()
    }

    fn format(&self, value: f64) -> String {
        format!("{value:.*}", self.decimals)
    }

    fn parse(&self, text: &str) -> Option<f64> {
        let value: f64 = text.trim().parse().ok()?;
        (value.is_finite() && value >= self.min && value <= self.max).then_some(value)
    }

    /// Whether `text` can still become a number by typing more.
    fn is_prefix(&self, text: &str) -> bool {
        let mut dots = 0;
        for (at, c) in text.chars().enumerate() {
            match c {
                '0'..='9' => {}
                '.' if self.decimals > 0 => dots += 1,
                '-' if at == 0 && self.min < 0.0 => {}
                _ => return false,
            }
        }
        dots <= 1
    }

    /// Applies a message. Returns the new value when it changed, for the view
    /// to store.
    pub fn update(&mut self, message: NumberMsg) -> Option<f64> {
        let before = self.value;
        match message {
            NumberMsg::Input(text) => {
                if !self.is_prefix(&text) {
                    return None;
                }
                self.text = text;
                if let Some(value) = self.parse(&self.text) {
                    self.value = value;
                }
            }
            NumberMsg::Step(direction) => {
                let stepped = self.value + self.step * f64::from(direction);
                self.value = stepped.clamp(self.min, self.max);
                self.text = self.format(self.value);
            }
            NumberMsg::Commit => {
                // What cannot be read, or lies outside, goes back to the last
                // value that could.
                self.text = self.format(self.value);
            }
        }
        (self.value != before).then_some(self.value)
    }
}

/// A number field with a step button on each side of its value.
pub fn number_input<'a, M: Clone + 'a>(
    t: &'a Theme,
    field: &'a NumberField,
    on_message: impl Fn(NumberMsg) -> M + Clone + 'a,
) -> Element<'a, M> {
    let invalid = !field.is_valid();
    let on_input = on_message.clone();
    let input = text_input("", field.text())
        .size(font_size::UI)
        .line_height(line_height())
        .padding(FIELD_PADDING)
        .font(fonts::mono())
        .width(Length::Fill)
        .on_input(move |text| on_input(NumberMsg::Input(text)))
        .on_submit(on_message(NumberMsg::Commit))
        .style(move |theme: &iced::Theme, status| {
            if invalid {
                style::text_input::field_invalid(t)(theme, status)
            } else {
                style::text_input::field(t)(theme, status)
            }
        });
    let down = icon_button(t, Icon::Minus)
        .on_press_maybe((field.value() > field.min()).then(|| on_message(NumberMsg::Step(-1))));
    let up = icon_button(t, Icon::Plus)
        .on_press_maybe((field.value() < field.max()).then(|| on_message(NumberMsg::Step(1))));
    row![input, down, up]
        .spacing(space::XXS)
        .align_y(Alignment::Center)
        .into()
}

// ---- Several lines --------------------------------------------------------------------------

/// A multi-line field over the state iced's `text_editor` keeps in a
/// `Content`. The view owns the `Content` and feeds the actions back with
/// `content.perform(action)`.
pub fn textarea<'a, M: Clone + 'a>(
    t: &'a Theme,
    content: &'a text_editor::Content,
    on_action: impl Fn(text_editor::Action) -> M + 'a,
) -> Element<'a, M> {
    text_editor(content)
        .on_action(on_action)
        .size(font_size::UI)
        .padding(FIELD_PADDING)
        .style(style::text_editor::field(t))
        .into()
}

// ---- Choosing -------------------------------------------------------------------------------

/// A drop-down list of options, closed to the chosen one (`.set-select`).
///
/// The list that opens is iced's; it takes the field's width and the
/// popover's look.
pub fn select<'a, T, M>(
    t: &'a Theme,
    options: impl std::borrow::Borrow<[T]> + 'a,
    selected: Option<T>,
    on_select: impl Fn(T) -> M + 'a,
) -> Element<'a, M>
where
    T: ToString + PartialEq + Clone + 'a,
    M: Clone + 'a,
{
    pick_list(options, selected, on_select)
        .text_size(font_size::UI)
        .padding(FIELD_PADDING)
        .width(Length::Fill)
        .style(style::pick_list::field(t))
        .menu_style(style::pick_list::menu(t))
        .into()
}

/// A tick box with its label.
#[must_use]
pub struct Checkbox<'a, M> {
    theme: &'a Theme,
    checked: bool,
    label: String,
    on_toggle: Option<Box<dyn Fn(bool) -> M + 'a>>,
}

pub fn checkbox<'a, M>(t: &'a Theme, label: impl Into<String>, checked: bool) -> Checkbox<'a, M> {
    Checkbox {
        theme: t,
        checked,
        label: label.into(),
        on_toggle: None,
    }
}

impl<'a, M> Checkbox<'a, M> {
    /// Without it the box is disabled.
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> M + 'a) -> Self {
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }
}

impl<'a, M: Clone + 'a> From<Checkbox<'a, M>> for Element<'a, M> {
    fn from(c: Checkbox<'a, M>) -> Self {
        let mut built = iced_checkbox(c.checked)
            .label(c.label)
            .size(16)
            .text_size(font_size::BODY)
            .spacing(space::MD)
            .style(style::checkbox::default(c.theme));
        if let Some(on_toggle) = c.on_toggle {
            built = built.on_toggle(on_toggle);
        }
        built.into()
    }
}

/// An on/off switch (`.switch`), with a label beside it if one is given.
#[must_use]
pub struct Switch<'a, M> {
    theme: &'a Theme,
    on: bool,
    label: Option<String>,
    on_toggle: Option<Box<dyn Fn(bool) -> M + 'a>>,
}

pub fn switch<'a, M>(t: &'a Theme, on: bool) -> Switch<'a, M> {
    Switch {
        theme: t,
        on,
        label: None,
        on_toggle: None,
    }
}

impl<'a, M> Switch<'a, M> {
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Without it the switch is disabled.
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> M + 'a) -> Self {
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }
}

impl<'a, M: Clone + 'a> From<Switch<'a, M>> for Element<'a, M> {
    fn from(s: Switch<'a, M>) -> Self {
        let mut built = toggler(s.on)
            .size(size::SWITCH_HEIGHT)
            .spacing(space::MD)
            .text_size(font_size::BODY)
            .style(style::toggler::default(s.theme));
        if let Some(label) = s.label {
            built = built.label(label);
        }
        if let Some(on_toggle) = s.on_toggle {
            built = built.on_toggle(on_toggle);
        }
        built.into()
    }
}

/// A slider with the value it stands at written beside it
/// (`.set-range` and `.set-value`).
#[must_use]
pub struct Slider<'a, M> {
    theme: &'a Theme,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    on_change: Box<dyn Fn(f32) -> M + 'a>,
    on_release: Option<M>,
    step: f32,
    display: Option<String>,
}

pub fn slider<'a, M>(
    t: &'a Theme,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    on_change: impl Fn(f32) -> M + 'a,
) -> Slider<'a, M> {
    Slider {
        theme: t,
        range,
        value,
        on_change: Box::new(on_change),
        on_release: None,
        step: 1.0,
        display: None,
    }
}

impl<'a, M> Slider<'a, M> {
    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    /// What the value is called beside the slider (`"100 %"`). Left out, the
    /// slider stands alone.
    pub fn display(mut self, display: impl Into<String>) -> Self {
        self.display = Some(display.into());
        self
    }

    /// Sent when the knob is let go: the moment to save, where `on_change`
    /// is sent for every step of the drag.
    pub fn on_release(mut self, message: M) -> Self {
        self.on_release = Some(message);
        self
    }
}

impl<'a, M: Clone + 'a> From<Slider<'a, M>> for Element<'a, M> {
    fn from(s: Slider<'a, M>) -> Self {
        let t = s.theme;
        let mut track = iced_slider(s.range, s.value, s.on_change)
            .step(s.step)
            .height(style::slider::HEIGHT)
            .style(style::slider::default(t));
        if let Some(message) = s.on_release {
            track = track.on_release(message);
        }
        let mut line = row![track].spacing(space::LG).align_y(Alignment::Center);
        if let Some(display) = s.display {
            line = line.push(
                container(
                    text(display)
                        .size(font_size::CAPTION)
                        .font(fonts::mono())
                        .style(style::text::muted(t))
                        .align_x(iced::alignment::Horizontal::Right),
                )
                .width(52.0),
            );
        }
        line.into()
    }
}

/// A row of segments, one of which is chosen (`.seg`). The chosen one is
/// raised out of the track.
pub fn segmented<'a, V, M>(
    t: &'a Theme,
    options: &[(V, &'a str)],
    selected: &V,
    on_select: impl Fn(V) -> M,
) -> Element<'a, M>
where
    V: Clone + PartialEq,
    M: Clone + 'a,
{
    let segments = options.iter().map(|(value, label)| {
        let chosen = value == selected;
        let segment = button(
            container(text(*label).size(font_size::UI).font(fonts::ui_medium()))
                .center_y(Length::Fill),
        )
        .padding(Padding::from([0.0, space::LG]))
        .height(Length::Fill)
        .style(style::button::tab(t, chosen))
        .on_press(on_select(value.clone()));
        Element::from(segment)
    });
    container(row(segments).spacing(space::XXS))
        .padding(space::XXS)
        .height(size::CONTROL_HEIGHT)
        .style(style::container::segmented(t))
        .into()
}

#[cfg(test)]
mod tests;
