//! Hangar's own title bar, on Linux and Windows.
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
//! - on Linux the window's buttons are the ones the desktop shows, on its
//!   side (`button-layout`, read once at start: GNOME's `':close'` gives a
//!   close button alone, `'close,minimize:'` puts two on the left), round on
//!   a faint disc;
//! - on Windows they are Windows 11's: minimise, maximise or restore, close,
//!   always at the right end, rectangular, 46 px wide and the band's height,
//!   flush with the window's top-right corner, close turning red under the
//!   pointer ([`windows_style`]). No Snap Layouts flyout for now: that needs
//!   the window's hit-testing, which winit keeps;
//! - the window's edges resize it while it is not maximised (winit leaves
//!   an undecorated window's borders to the app on both systems: its
//!   `WM_NCCALCSIZE` gives the whole window to the client area, so Windows
//!   finds no border to resize from);
//! - on Linux a hairline outlines it and its corners are rounded as GNOME's
//!   own windows are, while it is neither maximised nor full screen. The
//!   window is transparent on Linux for that: what draws at a corner (the
//!   window's page, the navigator, the toolbar, the status bar) rounds that
//!   corner of its own background ([`corners`]). A dialog's veil stays
//!   square, so a window's corners are dimmed with it. On Windows the system
//!   draws the corners, the border and the shadow (the window opens with
//!   `undecorated_shadow` and rounded `corner_preference`, not transparent),
//!   so Hangar adds none; maximised, winit keeps the window to the work area,
//!   clear of the taskbar.
//!
//! "Use the system's title bar" (Appearance) gives the system's back, for a
//! desktop that draws one in its own colours (KDE's, a tiling window
//! manager's), or on Windows the plain one. macOS keeps its own.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use hangar_i18n::Key;
use hangar_theme::{Theme, tokens};
use hangar_ui::{Icon, icon, tip};
use iced::border::Radius;
use iced::mouse::Interaction;
use iced::widget::{Space, button, column, container, mouse_area, row};
use iced::window::{self, Direction};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding};

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

impl Layout {
    /// Windows' buttons: always the three, at the end of the band.
    pub fn windows() -> Layout {
        Layout::default()
    }
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
    /// The windows that are maximised or full screen: square corners, no
    /// edges to resize, no outline, and the middle button brings them back.
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
            decorated: !has_own_bar(),
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

/// Whether this system's windows may carry Hangar's title bar: Linux and
/// Windows. macOS keeps its own.
pub const fn has_own_bar() -> bool {
    cfg!(target_os = "linux") || cfg!(target_os = "windows")
}

/// Whether the buttons are Windows 11's rectangles rather than discs.
const fn windows_style() -> bool {
    cfg!(target_os = "windows")
}

/// Whether Hangar draws the title bar of its windows.
pub fn drawn(settings: &Settings) -> bool {
    has_own_bar() && !settings.system_title_bar
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

/// A round button's side, and the gap between two.
const SIDE: f32 = 24.0;
const GAP: f32 = tokens::space::MD;
/// A Windows button's width: the full band tall, none between two.
const WINDOWS_WIDTH: f32 = 46.0;
/// Windows' close button under the pointer, and pressed.
const WINDOWS_CLOSE: Color = Color::from_rgb(196.0 / 255.0, 43.0 / 255.0, 28.0 / 255.0);
const WINDOWS_CLOSE_PRESSED: Color = Color {
    a: 0.8,
    ..WINDOWS_CLOSE
};

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
    room(windows_style(), toolbar_controls(ctx, Side::Start).len())
        + room(windows_style(), toolbar_controls(ctx, Side::End).len())
}

/// The room `n` buttons take, gap to the rest included.
fn room(windows: bool, n: usize) -> f32 {
    match (windows, n) {
        (_, 0) => 0.0,
        (true, n) => n as f32 * WINDOWS_WIDTH,
        (false, n) => n as f32 * (SIDE + GAP),
    }
}

/// Whether the toolbar's end is the window's top-right corner, with
/// Windows' buttons flush against it: no padding after them.
pub fn flush_end(ctx: Ctx<'_>) -> bool {
    windows_style() && !toolbar_controls(ctx, Side::End).is_empty()
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
    let gap = if windows_style() { 0.0 } else { GAP };
    let mut buttons = row![].spacing(gap).align_y(Alignment::Center);
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
    let (width, height) = if windows_style() {
        (WINDOWS_WIDTH, tokens::size::TOOLBAR_HEIGHT)
    } else {
        (SIDE, SIDE)
    };
    // Windows' minimise glyph is a 10 px hairline, drawn here: the icon set's
    // `Minus` is a stroke with no height, which the renderer draws as nothing.
    let glyph: Element<'a, Message> = if windows_style() && control == Control::Minimize {
        let line = t.colors.fg;
        container(Space::new())
            .width(10)
            .height(1)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(Background::Color(line)),
                ..container::Style::default()
            })
            .into()
    } else {
        icon(glyph, 13.0).into()
    };
    let built = button(container(glyph).center(Length::Fill))
        .padding(0)
        .width(width)
        .height(height)
        .style(look(t, control))
        .on_press(press(ctx.window, Press::Control(control)));
    tip::bottom(t, built, ctx.t(words).into_owned())
}

/// On Linux round, on a faint disc of the text colour, as the desktop's own
/// are; on Windows a rectangle, bare until the pointer is over it.
fn look(
    t: &Theme,
    control: Control,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let c = &t.colors;
        if windows_style() {
            let (background, text_color) = match (control, status) {
                (Control::Close, button::Status::Hovered) => (Some(WINDOWS_CLOSE), Color::WHITE),
                (Control::Close, button::Status::Pressed) => {
                    (Some(WINDOWS_CLOSE_PRESSED), Color::WHITE)
                }
                (_, button::Status::Hovered) => (Some(Color { a: 0.08, ..c.fg }), c.fg),
                (_, button::Status::Pressed) => (Some(Color { a: 0.05, ..c.fg }), c.fg),
                _ => (None, c.fg),
            };
            return button::Style {
                background: background.map(Background::Color),
                text_color,
                border: Border::default(),
                ..button::Style::default()
            };
        }
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
    let mut strip = row![]
        .align_y(Alignment::Center)
        .spacing(if windows_style() { 0.0 } else { GAP });
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
        .padding(Padding {
            top: 0.0,
            right: if flush_end(ctx) {
                0.0
            } else {
                tokens::space::MD
            },
            bottom: 0.0,
            left: tokens::space::MD,
        })
        .style(rounded_style(
            ctx,
            Corners::top(ctx),
            hangar_theme::style::container::toolbar(ctx.theme),
        ));
    Some(band(ctx, strip))
}

/// How far in from the window's edge a press resizes it, and how far along
/// it a corner reaches.
const EDGE: f32 = 4.0;
const CORNER: f32 = 12.0;

/// The radius of a window's corners: libadwaita's, what GNOME's own
/// windows have.
pub const RADIUS: f32 = 12.0;

/// Which corners of a region are the window's corners.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Corners {
    pub top_left: bool,
    pub top_right: bool,
    pub bottom_right: bool,
    pub bottom_left: bool,
}

impl Corners {
    pub const ALL: Corners = Corners {
        top_left: true,
        top_right: true,
        bottom_right: true,
        bottom_left: true,
    };
    pub const LEFT: Corners = Corners {
        top_left: true,
        top_right: false,
        bottom_right: false,
        bottom_left: true,
    };

    /// The top two, or the top right one when the navigator holds the left.
    pub fn top(ctx: Ctx<'_>) -> Corners {
        Corners {
            top_left: !navigator_drawn(ctx),
            top_right: true,
            ..Corners::default()
        }
    }

    /// The bottom two, or the bottom right one when the navigator holds the
    /// left.
    pub fn bottom(ctx: Ctx<'_>) -> Corners {
        Corners {
            bottom_right: true,
            bottom_left: !navigator_drawn(ctx),
            ..Corners::default()
        }
    }
}

/// Whether this window's corners are rounded now: by Hangar, on Linux only.
/// Windows rounds its own.
fn rounded(ctx: Ctx<'_>) -> bool {
    cfg!(target_os = "linux") && drawn(ctx.model.settings()) && !maximized(ctx)
}

/// The radius a region's background takes at `which` of its corners: the
/// window's, less the outline's hairline, while the corners are rounded.
pub fn corners(ctx: Ctx<'_>, which: Corners) -> Radius {
    if !rounded(ctx) {
        return Radius::default();
    }
    let r = |on: bool| {
        if on {
            RADIUS - tokens::BORDER_WIDTH
        } else {
            0.0
        }
    };
    Radius {
        top_left: r(which.top_left),
        top_right: r(which.top_right),
        bottom_right: r(which.bottom_right),
        bottom_left: r(which.bottom_left),
    }
}

/// A container's style with its background rounded at `which` corners of
/// the window.
pub fn rounded_style<'a>(
    ctx: Ctx<'a>,
    which: Corners,
    style: impl Fn(&iced::Theme) -> container::Style + 'a,
) -> impl Fn(&iced::Theme) -> container::Style + 'a {
    let radius = corners(ctx, which);
    move |theme| {
        let mut look = style(theme);
        look.border.radius = radius;
        look
    }
}

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

/// `frame` outlined by a hairline, rounded at the window's corners, which
/// tells the window from what is behind it now that the system draws no
/// edge. Not while it is maximised, nor on Windows, which draws its own.
pub fn outline<'a>(ctx: Ctx<'a>, frame: Element<'a, Message>) -> Element<'a, Message> {
    if !rounded(ctx) {
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
                radius: RADIUS.into(),
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
    fn windows_buttons_are_the_three_at_the_end() {
        let layout = Layout::windows();
        assert!(layout.start.is_empty());
        assert_eq!(
            layout.end,
            [Control::Minimize, Control::Maximize, Control::Close]
        );
    }

    #[test]
    fn windows_buttons_take_their_width_and_no_gap() {
        assert_eq!(room(true, 0), 0.0);
        assert_eq!(room(true, 3), 3.0 * 46.0);
        assert_eq!(room(false, 3), 3.0 * (SIDE + GAP));
    }

    #[test]
    fn linux_and_windows_draw_their_bar_and_the_setting_gives_the_system_back() {
        let mut settings = Settings::default();
        assert_eq!(drawn(&settings), has_own_bar());
        settings.system_title_bar = true;
        assert!(!drawn(&settings));
        assert_eq!(
            has_own_bar(),
            cfg!(any(target_os = "linux", target_os = "windows"))
        );
        // Windows opens undecorated, like Linux; macOS keeps its bar.
        assert_eq!(Live::default().decorated, cfg!(target_os = "macos"));
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
