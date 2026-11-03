//! What a theme is: an identity, a terminal palette and every colour token of
//! the window.

use iced::Color;

use crate::data::THEMES;

/// The theme the app starts with, and falls back on for an id it does not know.
pub const DEFAULT_ID: &str = "graphite";

/// Whether a theme sits on a dark or a light background. It decides which way
/// the chrome's surfaces step away from the terminal background, and the
/// scheme kits are told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Appearance {
    Dark,
    Light,
}

impl Appearance {
    pub fn is_dark(self) -> bool {
        self == Appearance::Dark
    }

    /// `"dark"` or `"light"`, the `scheme` of the kits' theme event.
    pub fn as_str(self) -> &'static str {
        match self {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        }
    }
}

/// A `#rrggbb` colour from its channels packed as `0xrrggbb`.
pub(crate) const fn rgb(packed: u32) -> Color {
    Color::from_rgb8((packed >> 16) as u8, (packed >> 8) as u8, packed as u8)
}

/// A colour of the given 8-bit channels and an alpha from 0 to 1.
pub(crate) const fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color {
    Color::from_rgba8(r, g, b, alpha)
}

/// One theme: how the window and its terminals are coloured.
///
/// A theme restyles the whole window, not only the terminals. The web app
/// derived the window's colours from the terminal palette and the accent when
/// the theme was applied; here the result is stored, so drawing never
/// computes a colour. [`crate::derive_colors`] is that derivation, kept to
/// check the tables against and to make themes the app does not bundle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Stable and unique, what settings and workspaces store (`"tokyo-night"`).
    pub id: &'static str,
    /// What the theme picker shows (`"Tokyo Night"`).
    pub name: &'static str,
    pub appearance: Appearance,
    /// The colours terminals are drawn with.
    pub terminal: TerminalColors,
    /// The colours of everything else.
    pub colors: Colors,
}

impl Theme {
    /// The accent, which marks links, focus and the selection.
    pub fn accent(&self) -> Color {
        self.colors.accent
    }

    pub fn is_dark(&self) -> bool {
        self.appearance.is_dark()
    }

    /// Matches and the current match of a search in a terminal: the theme's
    /// own yellow, the current one in its accent, both laid over the
    /// background so the text on top stays legible in light themes too.
    pub fn search_colors(&self) -> SearchColors {
        let bg = crate::Rgb::from(self.terminal.background);
        let match_color = crate::Rgb::from(self.terminal.ansi[3]);
        let accent = crate::Rgb::from(self.colors.accent);
        SearchColors {
            match_background: crate::mix(bg, match_color, 0.3).into(),
            match_border: crate::mix(bg, match_color, 0.65).into(),
            match_marker: match_color.into(),
            active_background: crate::mix(bg, accent, 0.45).into(),
            active_border: accent.into(),
        }
    }
}

/// How a terminal search marks what it found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchColors {
    pub match_background: Color,
    pub match_border: Color,
    /// On the scrollbar, where the matches show.
    pub match_marker: Color,
    pub active_background: Color,
    pub active_border: Color,
}

/// What a terminal is drawn with, in the terms of a terminal: a plain value,
/// so the crate that renders terminals does not have to depend on this one
/// and the app maps it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalColors {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    /// The colour of the character under a block cursor.
    pub cursor_text: Color,
    /// The background of selected text. Opaque: the text keeps its colour.
    pub selection: Color,
    /// The 16 ANSI colours in the order of the escape codes: black, red,
    /// green, yellow, blue, magenta, cyan, white, then the same eight bright.
    pub ansi: [Color; 16],
}

impl TerminalColors {
    /// Names of the 16 colours in the order of [`TerminalColors::ansi`].
    pub const ANSI_NAMES: [&'static str; 16] = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright black",
        "bright red",
        "bright green",
        "bright yellow",
        "bright blue",
        "bright magenta",
        "bright cyan",
        "bright white",
    ];
}

/// The four states of an agent. The chrome always shows them in a colour and
/// a shape, never in a colour alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentState {
    /// Busy on its own.
    Working,
    /// Blocked until the person answers.
    Waiting,
    /// Done, and the turn is the person's.
    Yours,
    /// Nothing going on.
    Idle,
}

impl AgentState {
    pub const ALL: [AgentState; 4] = [
        AgentState::Working,
        AgentState::Waiting,
        AgentState::Yours,
        AgentState::Idle,
    ];
}

/// Declares [`Colors`] from the CSS custom property each field was in
/// `deriveChromeTokens`, so that the names kits and the stylesheets use can
/// be looked up ([`Colors::get`]) and the tests can walk every token.
macro_rules! colors {
    ($($(#[$doc:meta])* $field:ident = $css:literal,)*) => {
        /// Every colour token of the window, by the name it had as a CSS
        /// custom property in the web app (`--chrome-bar` is `chrome_bar`).
        /// Some are translucent; those say so.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Colors {
            $($(#[$doc])* pub $field: Color,)*
        }

        impl Colors {
            /// The CSS names of all tokens, in the order of the fields.
            pub const CSS_NAMES: &'static [&'static str] = &[$($css),*];

            /// A token by its CSS name (`"--chrome-bar"`).
            pub fn get(&self, css_name: &str) -> Option<Color> {
                match css_name {
                    $($css => Some(self.$field),)*
                    _ => None,
                }
            }

            /// Every token with its CSS name.
            pub fn iter(&self) -> impl Iterator<Item = (&'static str, Color)> {
                [$(($css, self.$field)),*].into_iter()
            }
        }
    };
}

colors! {
    /// The navigator, inspector and status bar sit on this: a step away from
    /// the terminal background.
    chrome = "--chrome",
    /// The toolbar, a step above the chrome; floating panels too.
    chrome_bar = "--chrome-bar",
    /// A pane's header, just off the terminal itself.
    pane_head = "--pane-head",
    /// A pane's header while the pane has the focus.
    pane_head_focus = "--pane-head-focus",
    /// A raised group, a button at rest.
    raised = "--raised",
    /// A card or a settings group: lifted off the page on a dark theme, the
    /// page's own colour on a light one (where the page is the grey).
    card = "--card",
    /// What sits between panes and columns: darker than anything around it.
    sep = "--sep",
    /// A hairline, translucent.
    line = "--line",
    /// A stronger hairline: the edge of a field, a panel or a button.
    /// Translucent.
    line_strong = "--line-strong",
    /// The accent as text or an icon: held to 4.5:1 on every surface.
    accent_hi = "--accent-hi",
    /// The word for a working agent, 4.5:1 on every surface.
    state_working = "--state-working",
    /// A working agent's glyph, bar or frame, held to 3:1.
    state_working_fill = "--state-working-fill",
    state_waiting = "--state-waiting",
    state_waiting_fill = "--state-waiting-fill",
    /// "Your turn" has a hue of its own, so it never reads as a link or as the
    /// focus.
    state_yours = "--state-yours",
    state_yours_fill = "--state-yours-fill",
    state_idle = "--state-idle",
    state_idle_fill = "--state-idle-fill",
    /// The page: the terminal background, and what inputs are filled with.
    bg = "--bg",
    /// Darker than the page on a dark theme, white on a light one.
    bg_sunken = "--bg-sunken",
    surface = "--surface",
    /// Under the pointer.
    surface_hover = "--surface-hover",
    /// The selected row.
    surface_active = "--surface-active",
    border = "--border",
    border_strong = "--border-strong",
    /// Body text, the terminal foreground unless that is too faint for the
    /// chrome.
    fg = "--fg",
    /// Labels.
    fg_secondary = "--fg-secondary",
    /// Metadata and icons.
    fg_muted = "--fg-muted",
    /// The quietest hints, placeholders, paths and counts.
    fg_subtle = "--fg-subtle",
    /// The theme's accent as it is.
    accent = "--accent",
    /// A tint of the accent behind something selected or a focused field's
    /// halo. Translucent.
    accent_soft = "--accent-soft",
    /// The focus ring. Translucent.
    accent_ring = "--accent-ring",
    /// Text and icons written on the accent.
    accent_fg = "--accent-fg",
    danger = "--danger",
    /// Behind something destructive under the pointer, or a badge that
    /// warns. Translucent.
    danger_soft = "--danger-soft",
    success = "--success",
    warning = "--warning",
    /// The colour of a drop shadow. Translucent.
    shadow = "--shadow",
    /// A dark outer edge for what floats, under its light hairline.
    /// Translucent.
    float_ring = "--float-ring",
    /// The recessed track of a segmented control.
    seg_track = "--seg-track",
    /// The chosen segment, raised out of the track.
    seg_thumb = "--seg-thumb",
    /// The statistics' four ordered data colours, cheapest nearest the card.
    viz_1 = "--viz-1",
    viz_2 = "--viz-2",
    viz_3 = "--viz-3",
    viz_4 = "--viz-4",
    /// The activity heatmap: an empty cell, then five steps up.
    viz_heat_0 = "--viz-heat-0",
    viz_heat_1 = "--viz-heat-1",
    viz_heat_2 = "--viz-heat-2",
    viz_heat_3 = "--viz-heat-3",
    viz_heat_4 = "--viz-heat-4",
    viz_heat_5 = "--viz-heat-5",
    /// What steps back while something else is picked out.
    viz_mute = "--viz-mute",
    /// The line of a sparkline.
    viz_line = "--viz-line",
}

impl Colors {
    /// An agent state's word and its fill: the word at 4.5:1, the fill (a
    /// glyph, a bar, a frame) at 3:1 and so closer to the theme's own colour.
    pub fn state(&self, state: AgentState) -> StateColors {
        match state {
            AgentState::Working => StateColors {
                text: self.state_working,
                fill: self.state_working_fill,
            },
            AgentState::Waiting => StateColors {
                text: self.state_waiting,
                fill: self.state_waiting_fill,
            },
            AgentState::Yours => StateColors {
                text: self.state_yours,
                fill: self.state_yours_fill,
            },
            AgentState::Idle => StateColors {
                text: self.state_idle,
                fill: self.state_idle_fill,
            },
        }
    }
}

/// The two colours of one agent state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateColors {
    pub text: Color,
    pub fill: Color,
}

/// Every bundled theme, Graphite first, the dark ones then the light ones, in
/// the order the picker shows them.
pub fn themes() -> &'static [Theme] {
    &THEMES
}

/// The theme with this id.
pub fn by_id(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|theme| theme.id == id)
}

/// The theme with this id, or the default one for an id that is not (or no
/// longer) bundled, as `getTheme` does.
pub fn by_id_or_default(id: &str) -> &'static Theme {
    by_id(id).unwrap_or_else(default_theme)
}

/// Graphite, the default.
pub fn default_theme() -> &'static Theme {
    &THEMES[0]
}
