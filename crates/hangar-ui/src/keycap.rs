//! Keyboard hints: a key drawn as a cap, and a shortcut as a row of them
//! (`.kbd-seq`, `.kbd-chord`).
//!
//! ```ignore
//! use hangar_ui::keycap;
//!
//! keycap::keycap(t, "Esc")                       // one cap
//! keycap::binding(t, "Ctrl+K Ctrl+S")            // a two-step shortcut, as the settings store it
//! keycap::binding(t, "Ctrl+Shift+Enter").conflict()   // in the danger colour
//! ```
//!
//! A shortcut is drawn in the platform's spelling (`Ctrl`, `Alt` and `Win` on
//! most, `⌃⌥⇧⌘` on a Mac), one cap per piece; the chords of a sequence are
//! spaced apart and the keys inside one are not.

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{Theme, fonts, style};
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length, Padding};

use crate::shortcut::{chord_segments, parse_binding};

/// One key, as a cap.
pub fn keycap<'a, M: 'a>(t: &'a Theme, label: impl Into<String>) -> Element<'a, M> {
    cap(t, label.into(), false, 1.0)
}

fn cap<'a, M: 'a>(t: &'a Theme, label: String, conflict: bool, opacity: f32) -> Element<'a, M> {
    let narrow = label.chars().count() <= 1;
    let words = text(label)
        .size(font_size::CAPTION)
        .font(fonts::mono())
        .center();
    let words = if opacity < 1.0 {
        words.style(move |_: &iced::Theme| text::Style {
            color: Some(t.colors.fg.scale_alpha(opacity)),
        })
    } else {
        words
    };
    let cap = container(words)
        .height(20.0)
        .padding(Padding::from([0.0, 5.0]))
        .align_y(Alignment::Center)
        .center_x(Length::Shrink);
    let cap = if narrow { cap.width(20.0) } else { cap };
    if conflict {
        cap.style(style::container::kbd_conflict(t)).into()
    } else {
        cap.style(style::container::kbd(t)).into()
    }
}

/// A shortcut as caps, from the way the settings write it. A binding that
/// does not parse is shown as the text it is.
#[must_use]
pub struct Binding<'a> {
    theme: &'a Theme,
    binding: String,
    conflict: bool,
}

pub fn binding<'a>(t: &'a Theme, binding: impl Into<String>) -> Binding<'a> {
    Binding {
        theme: t,
        binding: binding.into(),
        conflict: false,
    }
}

impl Binding<'_> {
    /// Another command answers to the same keys: the caps turn red.
    pub fn conflict(mut self) -> Self {
        self.conflict = true;
        self
    }
}

impl<'a, M: 'a> From<Binding<'a>> for Element<'a, M> {
    fn from(b: Binding<'a>) -> Self {
        let t = b.theme;
        let Some(chords) = parse_binding(&b.binding) else {
            return text(b.binding)
                .size(font_size::CAPTION)
                .font(fonts::mono())
                .style(style::text::muted(t))
                .into();
        };
        let sequence = chords.iter().map(|chord| {
            let caps = chord_segments(chord)
                .into_iter()
                .map(|segment| cap(t, segment, b.conflict, 1.0));
            Element::from(row(caps).spacing(space::XXS).align_y(Alignment::Center))
        });
        row(sequence)
            .spacing(space::SM)
            .align_y(Alignment::Center)
            .into()
    }
}

/// The caps for modifiers being held, followed by a faded "…" that says a
/// key is expected next: what the recorder shows before a chord is complete.
pub fn held<'a, M: 'a>(t: &'a Theme, modifiers: &[&'static str]) -> Element<'a, M> {
    let caps = modifiers
        .iter()
        .map(|segment| cap(t, (*segment).to_string(), false, 1.0))
        .chain([cap(t, "…".to_string(), false, 0.45)]);
    row(caps)
        .spacing(space::XXS)
        .align_y(Alignment::Center)
        .into()
}
