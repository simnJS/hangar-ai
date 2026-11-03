//! Hangar's own title bar, on Linux.
//!
//! GNOME on Wayland leaves a window's title bar to the window, and winit's is
//! a band of its own above the app, in neither the app's colours nor the
//! desktop's (GTK's settings never reach it). So Hangar's windows open
//! without one, and the band at the top of the window is the title bar: the
//! navigator's top and the toolbar, level with each other, or a bare band
//! over a placeholder. On that band:
//!
//! - a press where no control takes it moves the window, two in a row
//!   maximise it or bring it back, and a right press opens the desktop's
//!   window menu;
//! - the window's buttons are the ones the desktop shows, on its side
//!   (`button-layout`, read once at start: GNOME's `':close'` gives a close
//!   button alone, `'close,minimize:'` puts two on the left);
//! - the window's edges resize it, and a hairline outlines it, while it is
//!   not maximised.
//!
//! "Use the system's title bar" (Appearance) gives the system's back, for a
//! desktop that draws one in its own colours (KDE's, a tiling window
//! manager's). macOS and Windows keep theirs.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use hangar_i18n::Key;
use hangar_theme::{Theme, tokens};
use hangar_ui::{Icon, icon, tip};
use iced::mouse::Interaction;
use iced::widget::{Space, button, column, container, mouse_area, row};
use iced::window::{self, Direction};
use iced::{Alignment, Background, Border, Color, Element, Length};

use crate::ctx::Ctx;
use crate::message::Message;
use crate::shell::{Action, WindowKind};
use crate::state::types::Settings;

/// A button of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Minimize,
    Maximize,
    Close,
}

/// Which buttons the desktop shows, on which side of the band.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub start: Vec<Control>,
    pub end: Vec<Control>,
}

impl Default for Layout {
    /// What a desktop that says nothing gets: the three, on the right.
    fn default() -> Self {
        Layout {
            start: Vec::new(),
            end: vec![Control::Minimize, Control::Maximize, Control::Close],
        }
    }
}

impl Layout {
    /// GNOME's `button-layout` or GTK's `gtk-decoration-layout`: the buttons
    /// on the left, a colon, the ones on the right, each a comma-separated
    /// list (`appmenu:minimize,maximize,close`). Names of things that are not
    /// buttons (`appmenu`, `icon`, `menu`, `spacer`) are left out; text
    /// without a colon is all left, as GTK reads it.
    pub fn parse(text: &str) -> Layout {
        let text = text.trim().trim_matches(|c| c == '\'' || c == '"');
        let (start, end) = text.split_once(':').unwrap_or((text, ""));
        let side = |names: &str| {
            names
                .split(',')
                .filter_map(|name| match name.trim() {
                    "minimize" => Some(Control::Minimize),
                    "maximize" => Some(Control::Maximize),
                    "close" => Some(Control::Close),
                    _ => None,
                })
                .collect()
        };
        Layout {
            start: side(start),
            end: side(end),
        }
    }
}

/// What the shell knows of its windows' title bars.
#[derive(Debug)]
pub struct Live {
    pub layout: Layout,
    /// The windows that are maximised: no edges to resize, no outline, and
    /// their middle button brings them back.
    pub maximized: HashSet<window::Id>,
    /// Whether the windows have the system's title bar now: they open without
    /// it on Linux, before the settings are read, and the setting turns it
    /// on and off for all of them at once.
    pub decorated: bool,
    /// The last press on the band, to tell a double press from a move.
    pub last_press: Option<(window::Id, Instant)>,
}

impl Default for Live {
    fn default() -> Live {
        Live {
            layout: Layout::default(),
            maximized: HashSet::new(),
            decorated: !cfg!(target_os = "linux"),
            last_press: None,
        }
    }
}

/// What a press on the band asks of the window.
#[derive(Debug, Clone, Copy)]
pub enum Press {
    /// On the band where nothing else took the press.
    Band,
    /// A right press on the band.
    Menu,
    Control(Control),
    /// On an edge or a corner, to resize from it.
    Edge(Direction),
}

/// Two presses closer than this are a double press (GTK's default).
pub const DOUBLE_PRESS: Duration = Duration::from_millis(400);

/// Whether a press on window `w`'s band at `now` is the second of a double
/// press, after `last`.
pub fn is_double(last: Option<(window::Id, Instant)>, w: window::Id, now: Instant) -> bool {
    last.is_some_and(|(at_w, at)| at_w == w && now.saturating_duration_since(at) <= DOUBLE_PRESS)
}

/// Whether Hangar draws the title bar of its windows.
pub fn drawn(settings: &Settings) -> bool {
    cfg!(target_os = "linux") && !settings.system_title_bar
}

/// Reads the desktop's button layout: GNOME's setting, then GTK's
/// settings.ini. Nothing when neither says.
pub async fn read_layout() -> Option<Layout> {
    let gsettings = tokio::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .kill_on_drop(true)
        .output();
    if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(2), gsettings).await
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        if !text.trim().is_empty() {
            return Some(Layout::parse(&text));
        }
    }
    let ini = dirs::config_dir()?.join("gtk-3.0").join("settings.ini");
    let text = std::fs::read_to_string(ini).ok()?;
    text.lines()
        .filter_map(|line| line.split_once('='))
        .find(|(key, _)| key.trim() == "gtk-decoration-layout")
        .map(|(_, value)| Layout::parse(value))
}

fn press(w: window::Id, press: Press) -> Message {
    Message::Shell(Action::TitleBar(w, press))
}

fn maximized(ctx: Ctx<'_>) -> bool {
    ctx.model.live.title_bar.maximized.contains(&ctx.window)
}

/// Whether the navigator is drawn in this window, at the band's left end.
fn navigator_drawn(ctx: Ctx<'_>) -> bool {
    ctx.model.live.windows.get(&ctx.window) == Some(&WindowKind::Main) && ctx.panels().navigator
}

/// Where a part of the band is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The navigator's top, or the toolbar's lead in a window without it.
    Start,
    /// The toolbar's trail.
    End,
}

/// A button's side, and the gap between two.
const SIDE: f32 = 24.0;
const GAP: f32 = tokens::space::MD;

/// The buttons the toolbar holds on `side`: the start ones only where the
/// navigator is not drawn (it holds them then).
fn toolbar_controls(ctx: Ctx<'_>, side: Side) -> &[Control] {
    if !drawn(ctx.model.settings()) {
        return &[];
    }
    let layout = &ctx.model.live.title_bar.layout;
    match side {
        Side::Start if navigator_drawn(ctx) => &[],
        Side::Start => &layout.start,
        Side::End => &layout.end,
    }
}

/// The room the toolbar's buttons take, gap to the rest included: the
/// toolbar lays itself out for the width left.
pub fn toolbar_room(ctx: Ctx<'_>) -> f32 {
    let room = |n: usize| {
        if n == 0 { 0.0 } else { n as f32 * (SIDE + GAP) }
    };
    room(toolbar_controls(ctx, Side::Start).len()) + room(toolbar_controls(ctx, Side::End).len())
}

/// The window's buttons at the toolbar's `side`, if the desktop puts any
/// there.
pub fn toolbar<'a>(ctx: Ctx<'a>, side: Side) -> Option<Element<'a, Message>> {
    controls(ctx, toolbar_controls(ctx, side))
}

/// The window's buttons at the start of the navigator's top.
pub fn navigator<'a>(ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    if !drawn(ctx.model.settings()) {
        return None;
    }
    controls(ctx, &ctx.model.live.title_bar.layout.start)
}

fn controls<'a>(ctx: Ctx<'a>, which: &[Control]) -> Option<Element<'a, Message>> {
    if which.is_empty() {
        return None;
    }
    let mut buttons = row![].spacing(GAP).align_y(Alignment::Center);
    for control in which {
        buttons = buttons.push(control_button(ctx, *control));
    }
    Some(buttons.into())
}

fn control_button<'a>(ctx: Ctx<'a>, control: Control) -> Element<'a, Message> {
    let t = ctx.theme;
    let (glyph, words) = match control {
        Control::Minimize => (Icon::Minus, Key::TitleBarMinimize),
        Control::Maximize if maximized(ctx) => (Icon::Unmaximize, Key::TitleBarRestore),
        Control::Maximize => (Icon::Maximize, Key::TitleBarMaximize),
        Control::Close => (Icon::Close, Key::TitleBarClose),
    };
    let built = button(container(icon(glyph, 13.0)).center(Length::Fill))
        .padding(0)
        .width(SIDE)
        .height(SIDE)
        .style(look(t))
        .on_press(press(ctx.window, Press::Control(control)));
    tip::bottom(t, built, ctx.t(words).into_owned())
}

/// Round, on a faint disc of the text colour, as the desktop's own are.
fn look(t: &Theme) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        let disc = match status {
            button::Status::Hovered => 0.14,
            button::Status::Pressed => 0.24,
            _ => 0.08,
        };
        button::Style {
            background: Some(Background::Color(Color { a: disc, ..c.fg })),
            text_color: c.fg,
            border: Border {
                radius: (SIDE / 2.0).into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

/// `content`, a part of the band, moving the window where nothing in it
/// takes a press. Nothing changes with the system's title bar.
pub fn band<'a>(ctx: Ctx<'a>, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let content = content.into();
    if !drawn(ctx.model.settings()) {
        return content;
    }
    mouse_area(content)
        .on_press(press(ctx.window, Press::Band))
        .on_right_press(press(ctx.window, Press::Menu))
        .into()
}

/// The band over a placeholder, which has no toolbar: the buttons, and room
/// to move the window by. Nothing with the system's title bar.
pub fn bare_band<'a>(ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    if !drawn(ctx.model.settings()) {
        return None;
    }
    let mut strip = row![].align_y(Alignment::Center).spacing(GAP);
    if let Some(start) = toolbar(ctx, Side::Start) {
        strip = strip.push(start);
    }
    strip = strip.push(Space::new().width(Length::Fill));
    if let Some(end) = toolbar(ctx, Side::End) {
        strip = strip.push(end);
    }
    let strip = container(strip)
        .width(Length::Fill)
        .height(tokens::size::TOOLBAR_HEIGHT)
        .padding([0.0, tokens::space::MD])
        .style(hangar_theme::style::container::toolbar(ctx.theme));
    Some(band(ctx, strip))
}

/// How far in from the window's edge a press resizes it, and how far along
/// it a corner reaches.
const EDGE: f32 = 4.0;
const CORNER: f32 = 12.0;

/// The window's edges and corners, over everything else: what resizes a
/// window without the system's title bar. Nothing while it is maximised.
pub fn edges<'a>(ctx: Ctx<'a>) -> Option<Element<'a, Message>> {
    if !drawn(ctx.model.settings()) || maximized(ctx) {
        return None;
    }
    let w = ctx.window;
    let handle = |direction: Direction, width: Length, height: Length| -> Element<'a, Message> {
        let cursor = match direction {
            Direction::North | Direction::South => Interaction::ResizingVertically,
            Direction::East | Direction::West => Interaction::ResizingHorizontally,
            Direction::NorthWest | Direction::SouthEast => Interaction::ResizingDiagonallyDown,
            Direction::NorthEast | Direction::SouthWest => Interaction::ResizingDiagonallyUp,
        };
        mouse_area(Space::new().width(width).height(height))
            .interaction(cursor)
            .on_press(press(w, Press::Edge(direction)))
            .into()
    };
    let (edge, corner, fill) = (Length::Fixed(EDGE), Length::Fixed(CORNER), Length::Fill);
    let along = Length::Fixed(CORNER - EDGE);
    let side = |top: Direction, middle: Direction, bottom: Direction| {
        column![
            handle(top, edge, along),
            handle(middle, edge, fill),
            handle(bottom, edge, along),
        ]
        .height(fill)
    };
    Some(
        column![
            row![
                handle(Direction::NorthWest, corner, edge),
                handle(Direction::North, fill, edge),
                handle(Direction::NorthEast, corner, edge),
            ],
            row![
                side(Direction::NorthWest, Direction::West, Direction::SouthWest),
                Space::new().width(fill).height(fill),
                side(Direction::NorthEast, Direction::East, Direction::SouthEast),
            ]
            .height(fill),
            row![
                handle(Direction::SouthWest, corner, edge),
                handle(Direction::South, fill, edge),
                handle(Direction::SouthEast, corner, edge),
            ],
        ]
        .width(fill)
        .height(fill)
        .into(),
    )
}

/// `frame` outlined by a hairline, which tells the window from what is
/// behind it now that the system draws no edge. Not while it is maximised.
pub fn outline<'a>(ctx: Ctx<'a>, frame: Element<'a, Message>) -> Element<'a, Message> {
    if !drawn(ctx.model.settings()) || maximized(ctx) {
        return frame;
    }
    let t = ctx.theme;
    container(frame)
        .padding(tokens::BORDER_WIDTH)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            border: Border {
                color: t.colors.line_strong,
                width: tokens::BORDER_WIDTH,
                radius: 0.0.into(),
            },
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktops_layout_puts_its_buttons_on_its_side() {
        assert_eq!(
            Layout::parse("'appmenu:minimize,maximize,close'\n"),
            Layout {
                start: vec![],
                end: vec![Control::Minimize, Control::Maximize, Control::Close],
            }
        );
        assert_eq!(
            Layout::parse("'appmenu:close'"),
            Layout {
                start: vec![],
                end: vec![Control::Close],
            }
        );
        assert_eq!(
            Layout::parse("close,minimize,maximize:icon"),
            Layout {
                start: vec![Control::Close, Control::Minimize, Control::Maximize],
                end: vec![],
            }
        );
    }

    #[test]
    fn a_layout_without_a_colon_is_all_on_the_left() {
        assert_eq!(
            Layout::parse("close"),
            Layout {
                start: vec![Control::Close],
                end: vec![],
            }
        );
        assert_eq!(
            Layout::parse(""),
            Layout {
                start: vec![],
                end: vec![]
            }
        );
    }

    #[test]
    fn two_presses_on_the_same_band_in_a_short_while_are_a_double_press() {
        let (a, b) = (window::Id::unique(), window::Id::unique());
        let then = Instant::now();
        let soon = then + Duration::from_millis(250);
        let late = then + Duration::from_millis(900);
        assert!(is_double(Some((a, then)), a, soon));
        assert!(!is_double(Some((a, then)), a, late));
        assert!(!is_double(Some((a, then)), b, soon));
        assert!(!is_double(None, a, soon));
    }
}
