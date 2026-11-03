//! The words a widget is styled with. A kit never names a colour, a font or a
//! size in pixels for text: it says what something *is* (dim, a danger, a
//! title), and Hangar draws it in the theme the person chose, as Hangar UI's
//! tokens do for an HTML tab (docs/kit-ui.md, "Tokens").

use serde::{Deserialize, Serialize};

/// Whether a value is its type's default, to leave it out of the JSON.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// How much room a widget takes along one axis.
///
/// In JSON: `"shrink"`, `"fill"`, `{ "portion": 2 }`, `{ "px": 120 }`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Length {
    /// As much as its content needs.
    #[default]
    Shrink,
    /// All that is left, shared with the other `fill`s of its row or column.
    Fill,
    /// A share of what is left: `portion(2)` takes twice what `fill` does.
    Portion(u16),
    /// This many logical pixels. Hangar's interface size scales the whole
    /// window, so a kit never multiplies by it.
    Px(f32),
}

impl From<f32> for Length {
    fn from(px: f32) -> Self {
        Length::Px(px)
    }
}

/// Space inside a container's edge, in logical pixels.
///
/// In JSON: `8`, `[4, 8]` (vertical, horizontal) or `[1, 2, 3, 4]` (top,
/// right, bottom, left), as CSS writes it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Padding {
    All(f32),
    Axes([f32; 2]),
    Sides([f32; 4]),
}

impl Padding {
    /// Top, right, bottom, left.
    pub fn sides(self) -> [f32; 4] {
        match self {
            Padding::All(all) => [all; 4],
            Padding::Axes([vertical, horizontal]) => [vertical, horizontal, vertical, horizontal],
            Padding::Sides(sides) => sides,
        }
    }
}

impl From<f32> for Padding {
    fn from(all: f32) -> Self {
        Padding::All(all)
    }
}

impl From<[f32; 2]> for Padding {
    fn from(axes: [f32; 2]) -> Self {
        Padding::Axes(axes)
    }
}

impl From<[f32; 4]> for Padding {
    fn from(sides: [f32; 4]) -> Self {
        Padding::Sides(sides)
    }
}

/// Where something sits across the axis of its row or column, or inside its
/// container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

/// The type scale of Hangar UI: `--kit-fs-*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextSize {
    /// 15 px: a page's title.
    Title,
    /// 13 px: running text.
    #[default]
    Body,
    /// 12.5 px: a row's title.
    Row,
    /// 12 px: what a control says.
    Control,
    /// 11.5 px: an aside, a second line.
    Small,
    /// 11.5 px: code and logs, in the terminals' face.
    Code,
    /// 11 px: metadata.
    Meta,
    /// 10.5 px: a section's title, in small capitals.
    Label,
    /// 10 px: a chip.
    Chip,
}

impl TextSize {
    /// Its size in logical pixels, at 100 % interface size.
    pub fn px(self) -> f32 {
        match self {
            TextSize::Title => 15.0,
            TextSize::Body => 13.0,
            TextSize::Row => 12.5,
            TextSize::Control => 12.0,
            TextSize::Small | TextSize::Code => 11.5,
            TextSize::Meta => 11.0,
            TextSize::Label => 10.5,
            TextSize::Chip => 10.0,
        }
    }
}

/// What a text's colour means. `default` is most of a page: colour in Hangar
/// only ever means something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    #[default]
    Default,
    /// Labels and second lines.
    Dim,
    /// Metadata.
    Muted,
    /// Placeholders, what barely matters.
    Faint,
    /// A link, what is on.
    Accent,
    /// A state, as a word.
    Ok,
    Warn,
    Danger,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Weight {
    #[default]
    Normal,
    Medium,
    Semibold,
    Bold,
}

/// How a text that does not fit its width behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Wrap {
    /// Wraps between words, and inside a word too long for the line.
    #[default]
    Word,
    /// Stays on one line, cut at the edge: give it a tooltip with the whole
    /// value (Hangar UI's `.kit-truncate`).
    Truncate,
}

/// A state, the words of `tab.setBadge` plus `waiting`: shape and colour both
/// say it, so it reads for anyone who cannot tell green from amber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Running,
    Success,
    Failure,
    Waiting,
    #[default]
    Neutral,
}

/// Hangar's own icons, drawn in the colour of the text around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Icon {
    Alert,
    Check,
    ChevronDown,
    Close,
    Copy,
    Info,
    Lock,
    More,
    Plus,
    Restart,
    Search,
    Updown,
    /// An icon a newer Hangar has: this one draws nothing in its place.
    #[serde(other)]
    Unknown,
}

/// What a container is drawn on: the surfaces of Hangar UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Surface {
    /// Nothing: the page shows through.
    #[default]
    None,
    /// A group of rows (`--kit-surface`).
    Card,
    /// A button's (`--kit-raised`).
    Raised,
    /// What floats (`--kit-float`).
    Float,
    /// Inside an input (`--kit-field`).
    Field,
    /// Behind code and logs (`--kit-well`).
    Well,
    /// The chosen row (`--kit-selected`).
    Selected,
    /// A tint behind what is on (`--kit-accent-soft`).
    Accent,
    /// A tint behind what is destructive (`--kit-danger-soft`).
    Danger,
}

/// A button's look. One primary per screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonKind {
    #[default]
    Default,
    Primary,
    Danger,
    Ghost,
}

/// A banner's, or a toast's, kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Notice {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

/// The four things a part of a page can be instead of its content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmptyKind {
    /// Nothing yet: say what will fill it.
    #[default]
    Empty,
    /// Being read: say what is being done.
    Loading,
    /// It went wrong: say what, and what to do.
    Error,
    /// A permission is missing: say which, and where it is given.
    Denied,
}

/// A progress bar's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgressKind {
    #[default]
    Normal,
    /// What it last knew, in dashes: nothing was heard since.
    Stale,
    Success,
    Failure,
}

/// A line of a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineKind {
    #[default]
    Normal,
    Error,
    Warning,
    /// A heading inside the log: a job, a step.
    Head,
}

/// Which way a split or a scroll goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Axis {
    /// Top to bottom.
    #[default]
    Vertical,
    /// Left to right.
    Horizontal,
    /// Both ways (a scroll only).
    Both,
}

/// How an image fills its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fit {
    /// Whole, as large as fits.
    #[default]
    Contain,
    /// The box filled, the image cut to it.
    Cover,
    /// The box filled, the image stretched.
    Fill,
    /// Its own size, never larger than the box.
    ScaleDown,
}
