//! An agent's state, drawn the same everywhere (`StateGlyph.tsx`), and the
//! context gauge of a pane (`PaneGauge.tsx`).
//!
//! Colour and shape both carry the state, so it reads without telling green
//! from amber: a pulsing dot while working, a diamond while waiting for an
//! answer, a solid dot when it is your turn, a ring when idle. `Starting` and
//! `Exited` are the shell's own states, before or after there is an agent to
//! report on.
//!
//! ```ignore
//! use hangar_ui::glyph::{self, GlyphState};
//!
//! row![glyph::state(t, GlyphState::Working, 8.0), text("Ben")]
//! glyph::gauge(t, 72).tip("72 % of 200k tokens")
//! ```

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use hangar_theme::tokens::{font_size, space};
use hangar_theme::{AgentState, Theme};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::svg;
use iced::advanced::widget::Tree;
use iced::advanced::{Shell, Widget};
use iced::mouse;
use iced::widget::{canvas, row, text};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Point, Radians, Rectangle, Size,
    window,
};

use crate::tip;

/// The states a glyph can show: an agent's four, and the shell's two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphState {
    Working,
    Waiting,
    Yours,
    Idle,
    /// The shell is coming up.
    Starting,
    /// The process ended.
    Exited,
}

impl From<AgentState> for GlyphState {
    fn from(state: AgentState) -> Self {
        match state {
            AgentState::Working => GlyphState::Working,
            AgentState::Waiting => GlyphState::Waiting,
            AgentState::Yours => GlyphState::Yours,
            AgentState::Idle => GlyphState::Idle,
        }
    }
}

impl GlyphState {
    /// Whether the glyph moves, and so needs the window redrawn to keep going.
    pub fn animates(self) -> bool {
        matches!(self, GlyphState::Working | GlyphState::Starting)
    }
}

/// The colours a glyph is drawn in, from the theme or from the row it sits on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint {
    /// The fill of the dot, the diamond or the ring.
    pub fill: Color,
}

impl Tint {
    pub fn of(t: &Theme, state: GlyphState) -> Tint {
        let c = &t.colors;
        let fill = match state {
            GlyphState::Working => c.state_working_fill,
            GlyphState::Waiting => c.state_waiting_fill,
            GlyphState::Yours => c.state_yours_fill,
            GlyphState::Idle | GlyphState::Exited | GlyphState::Starting => c.state_idle_fill,
        };
        Tint { fill }
    }
}

/// Where the pulse is in its turn, from 0 to 1. All glyphs on screen pulse
/// together, which reads as one rhythm and not as a swarm.
fn turn(period: Duration) -> f32 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    (start.elapsed().as_secs_f32() % period.as_secs_f32()) / period.as_secs_f32()
}

/// The pulse of a working glyph: 2.4 s, the ring growing from 0.6 to 1.35
/// times the dot and fading as it goes, then waiting out the last 30 %.
const PULSE: Duration = Duration::from_millis(2400);

/// The ring's scale and opacity at `turn` (0 to 1) of the pulse.
pub fn pulse(turn: f32) -> (f32, f32) {
    if turn >= 0.7 {
        return (1.35, 0.0);
    }
    let progress = turn / 0.7;
    (0.6 + 0.75 * progress, 0.7 * (1.0 - progress))
}

/// Whether a starting glyph is lit at `turn` of its 1.2 s blink.
pub fn blink_lit(turn: f32) -> bool {
    turn < 0.5
}

/// A rounded square turned a quarter, for the waiting state: drawn from an SVG
/// since a quad cannot be rotated.
static DIAMOND: LazyLock<svg::Handle> = LazyLock::new(|| {
    svg::Handle::from_memory(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">"##,
            r##"<rect x="1.47" y="1.47" width="7.06" height="7.06" rx="1.3" "##,
            r##"transform="rotate(45 5 5)" fill="#000"/></svg>"##
        )
        .as_bytes()
        .to_vec(),
    )
});

/// Draws the glyph of `state` in the square `bounds`.
///
/// `animate` is off for a glyph that is part of a static drawing (a menu
/// line), where a pulse would only be noise.
pub fn draw<R: iced::advanced::Renderer + svg::Renderer>(
    renderer: &mut R,
    state: GlyphState,
    bounds: Rectangle,
    tint: Tint,
    animate: bool,
) {
    let side = bounds.width.min(bounds.height);
    let centre = bounds.center();
    let dot = Rectangle::new(
        Point::new(centre.x - side / 2.0, centre.y - side / 2.0),
        Size::new(side, side),
    );
    let disc = |renderer: &mut R, rect: Rectangle, fill: Color| {
        renderer.fill_quad(
            Quad {
                bounds: rect,
                border: Border {
                    radius: (rect.width / 2.0).into(),
                    ..Border::default()
                },
                ..Quad::default()
            },
            Background::Color(fill),
        );
    };
    let ring = |renderer: &mut R, rect: Rectangle, width: f32, color: Color| {
        renderer.fill_quad(
            Quad {
                bounds: rect,
                border: Border {
                    radius: (rect.width / 2.0).into(),
                    width,
                    color,
                },
                ..Quad::default()
            },
            Background::Color(Color::TRANSPARENT),
        );
    };

    match state {
        GlyphState::Working => {
            disc(renderer, dot, tint.fill);
            if animate {
                // A ring that grows and fades: something is happening, quietly.
                let (scale, opacity) = pulse(turn(PULSE));
                let grown = (side + 6.0) * scale;
                let rect = Rectangle::new(
                    Point::new(centre.x - grown / 2.0, centre.y - grown / 2.0),
                    Size::new(grown, grown),
                );
                if opacity > 0.01 {
                    ring(renderer, rect, 1.5, tint.fill.scale_alpha(opacity));
                }
            }
        }
        GlyphState::Yours => disc(renderer, dot, tint.fill),
        GlyphState::Waiting => {
            // 0.86 of the dot's box turned a quarter is 1.22 of it across.
            let across = side * 1.216;
            let rect = Rectangle::new(
                Point::new(centre.x - across / 2.0, centre.y - across / 2.0),
                Size::new(across, across),
            );
            renderer.draw_svg(
                svg::Svg {
                    handle: DIAMOND.clone(),
                    color: Some(Color {
                        a: 1.0,
                        ..tint.fill
                    }),
                    rotation: Radians(0.0),
                    opacity: tint.fill.a,
                },
                rect,
                rect,
            );
        }
        GlyphState::Idle | GlyphState::Exited => ring(renderer, dot, 1.5, tint.fill),
        GlyphState::Starting => {
            let lit = !animate || blink_lit(turn(Duration::from_millis(1200)));
            ring(
                renderer,
                dot,
                1.5,
                if lit {
                    tint.fill
                } else {
                    tint.fill.scale_alpha(0.25)
                },
            );
        }
    }
}

/// The widget [`state`] makes.
pub struct StateGlyph {
    state: GlyphState,
    side: f32,
    tint: Tint,
}

impl<Message, Theme> Widget<Message, Theme, iced::Renderer> for StateGlyph {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.side), Length::Fixed(self.side))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.side, self.side)
    }

    fn update(
        &mut self,
        _tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        // The motion is drawn from the clock, so the window only has to be
        // asked to redraw: about 25 times a second, which is smooth enough
        // for a ring that takes 1.7 s to fade and cheap enough for a window
        // of sixteen panes.
        if self.state.animates()
            && let Event::Window(window::Event::RedrawRequested(now)) = event
        {
            shell.request_redraw_at(*now + Duration::from_millis(40));
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        draw(renderer, self.state, layout.bounds(), self.tint, true);
    }
}

impl<'a, Message: 'a, Theme: 'a> From<StateGlyph> for Element<'a, Message, Theme, iced::Renderer> {
    fn from(glyph: StateGlyph) -> Self {
        Element::new(glyph)
    }
}

impl StateGlyph {
    /// Draws in this colour instead of the state's own: on the accent of a
    /// chosen row, the glyph keeps its shape in the colour that reads there.
    pub fn tint(mut self, color: Color) -> Self {
        self.tint = Tint { fill: color };
        self
    }
}

/// The glyph of `state`, `side` pixels across (8 in the pane header, 7 in the
/// navigator and the menus).
pub fn state(t: &Theme, state: GlyphState, side: f32) -> StateGlyph {
    StateGlyph {
        state,
        side,
        tint: Tint::of(t, state),
    }
}

// ---- The context gauge -----------------------------------------------------------------------

/// Amber from here, in percent: the agent compacts on its own near the limit,
/// and these say how near that is.
pub const GAUGE_WARN: u8 = 70;
/// Red from here.
pub const GAUGE_HOT: u8 = 90;

/// How worrying a gauge's reading is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaugeLevel {
    Ok,
    Warn,
    Hot,
}

/// The level of a context window `pct` percent full (`gaugeLevel`).
pub fn gauge_level(pct: u8) -> GaugeLevel {
    if pct >= GAUGE_HOT {
        GaugeLevel::Hot
    } else if pct >= GAUGE_WARN {
        GaugeLevel::Warn
    } else {
        GaugeLevel::Ok
    }
}

/// The ring on its 12-unit box: the radius, and the thickness of its stroke.
const RING_RADIUS: f32 = 4.6 / 12.0;
const RING_STROKE: f32 = 2.2 / 12.0;

/// The ring of a gauge, drawn on a canvas.
struct Ring {
    pct: u8,
    track: Color,
    value: Color,
}

impl<Message> canvas::Program<Message> for Ring {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let centre = frame.center();
        let radius = bounds.width * RING_RADIUS;
        let width = bounds.width * RING_STROKE;
        frame.stroke(
            &canvas::Path::circle(centre, radius),
            canvas::Stroke::default()
                .with_width(width)
                .with_color(self.track),
        );
        let fraction = f32::from(self.pct.min(100)) / 100.0;
        if fraction >= 1.0 {
            frame.stroke(
                &canvas::Path::circle(centre, radius),
                canvas::Stroke::default()
                    .with_width(width)
                    .with_color(self.value),
            );
        } else if fraction > 0.0 {
            // The ring starts at twelve o'clock and fills clockwise.
            let start = -std::f32::consts::FRAC_PI_2;
            let arc = canvas::Path::new(|path| {
                path.arc(canvas::path::Arc {
                    center: centre,
                    radius,
                    start_angle: Radians(start),
                    end_angle: Radians(start + std::f32::consts::TAU * fraction),
                });
            });
            frame.stroke(
                &arc,
                canvas::Stroke::default()
                    .with_width(width)
                    .with_color(self.value)
                    .with_line_cap(canvas::LineCap::Round),
            );
        }
        vec![frame.into_geometry()]
    }
}

/// A pane's context gauge.
#[must_use]
pub struct Gauge<'a> {
    theme: &'a Theme,
    pct: u8,
    tip: Option<String>,
}

/// How full the agent's context window is: a ring and a percentage, sized to
/// survive an 8-pane layout. Amber from 70 %, red from 90 %.
pub fn gauge(t: &Theme, pct: u8) -> Gauge<'_> {
    Gauge {
        theme: t,
        pct,
        tip: None,
    }
}

impl Gauge<'_> {
    /// The numbers behind the ring, in a tooltip.
    pub fn tip(mut self, words: impl Into<String>) -> Self {
        self.tip = Some(words.into());
        self
    }
}

impl<'a, M: 'a> From<Gauge<'a>> for Element<'a, M> {
    fn from(g: Gauge<'a>) -> Self {
        let c = &g.theme.colors;
        let (ink, ring) = match gauge_level(g.pct) {
            GaugeLevel::Ok => (c.fg_muted, c.fg_muted),
            GaugeLevel::Warn => (c.warning, c.warning),
            GaugeLevel::Hot => (c.danger, c.danger),
        };
        let ring = canvas(Ring {
            pct: g.pct,
            track: c.line_strong,
            value: ring,
        })
        .width(12.0)
        .height(12.0);
        let words = text(format!("{} %", g.pct))
            .size(font_size::CAPTION_LG)
            .style(move |_: &iced::Theme| text::Style { color: Some(ink) });
        let content = row![ring, words]
            .spacing(space::XS)
            .align_y(Alignment::Center);
        match g.tip {
            Some(words) => tip::bottom(g.theme, content, words),
            None => content.into(),
        }
    }
}

#[cfg(test)]
mod tests;
