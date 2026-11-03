//! Box drawing, block elements and braille, drawn instead of typeset.
//!
//! A font draws `│` to fit its own line height. A terminal's rows are taller
//! than that whenever the line height is above 1, and the glyphs then stop
//! short of the row's edges: the frames of a TUI come out dashed. Drawing
//! these characters as rectangles that fill the cell, snapped to whole
//! pixels, makes lines in neighbouring cells meet exactly at any size, which
//! is the only way to make them line up.
//!
//! Everything here is geometry in a cell's own coordinates (origin at its top
//! left, in logical pixels). The widget moves the rectangles to where the
//! cell is and fills them.

/// How a stroke is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stroke {
    None,
    Light,
    Heavy,
    Double,
}

impl Stroke {
    fn from_digit(d: u8) -> Self {
        match d {
            b'1' => Stroke::Light,
            b'2' => Stroke::Heavy,
            b'3' => Stroke::Double,
            _ => Stroke::None,
        }
    }
}

/// The four arms of a box-drawing character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Arms {
    pub left: Stroke,
    pub right: Stroke,
    pub up: Stroke,
    pub down: Stroke,
}

/// A rectangle in cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// What the cell is drawn as.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Shape {
    /// Filled rectangles, fully opaque in the foreground colour.
    Rects(Vec<Rect>),
    /// A rectangle in the foreground colour at a fraction of its opacity:
    /// the shades.
    Shade(f32),
    /// A quarter circle joining two arms: the rounded corners `╭╮╯╰`.
    Arc(Arc),
    /// Round dots of the given diameter, centred at the points: braille.
    Dots { diameter: f32, centres: Vec<(f32, f32)> },
}

/// A rounded corner, as the stroked quarter of a circle plus the straight
/// stubs that continue it to the cell's edges.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arc {
    /// The square that holds the whole circle's ring, outer edge included.
    pub ring: Rect,
    /// The border width of the ring.
    pub thickness: f32,
    /// The part of the ring to show: the quadrant that is the corner.
    pub clip: Rect,
    pub stubs: Vec<Rect>,
}

/// The pixel metrics lines are drawn with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Metrics {
    pub width: f32,
    pub height: f32,
    /// One physical pixel in logical ones, to snap to.
    pub pixel: f32,
    /// Thickness of a light line.
    pub light: f32,
    /// The font size, which decides where underlines sit.
    pub size: f32,
}

impl Metrics {
    /// Metrics for a font size: a light line is about a twelfth of the size,
    /// and never thinner than one physical pixel.
    pub(crate) fn new(width: f32, height: f32, size: f32, scale: f32) -> Self {
        let pixel = 1.0 / scale.max(0.5);
        let light = ((size / 12.0 / pixel).round().max(1.0)) * pixel;
        Self { width, height, pixel, light, size }
    }

    fn heavy(&self) -> f32 {
        self.light * 2.0
    }

    fn snap(&self, v: f32) -> f32 {
        (v / self.pixel).round() * self.pixel
    }

    /// Snaps a horizontal coordinate, except the cell's own right edge.
    ///
    /// A cell is a fraction of a pixel wider than a whole number of them, and
    /// the next cell starts exactly there. A line that ran to the snapped
    /// edge instead would stop short of its neighbour by the difference, and
    /// once the renderer rounds both to pixels, a pixel-wide gap would show
    /// in the line at every cell where the roundings disagree.
    fn snap_x(&self, v: f32) -> f32 {
        if (v - self.width).abs() < 1e-3 { self.width } else { self.snap(v) }
    }

    /// A rectangle given by its edges, snapped.
    fn span(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        let (x0, y0, x1, y1) = (self.snap_x(x0), self.snap(y0), self.snap_x(x1), self.snap(y1));
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(self.pixel),
            h: (y1 - y0).max(self.pixel),
        }
    }
}

/// `LRUD` strokes of U+2500 to U+257F: 0 none, 1 light, 2 heavy, 3 double.
const TABLE: [&[u8; 4]; 128] = [
    b"1100", b"2200", b"0011", b"0022", b"1100", b"2200", b"0011", b"0022", // 2500
    b"1100", b"2200", b"0011", b"0022", b"0101", b"0201", b"0102", b"0202", // 2508
    b"1001", b"2001", b"1002", b"2002", b"0110", b"0210", b"0120", b"0220", // 2510
    b"1010", b"2010", b"1020", b"2020", b"0111", b"0211", b"0121", b"0112", // 2518
    b"0122", b"0221", b"0212", b"0222", b"1011", b"2011", b"1021", b"1012", // 2520
    b"1022", b"2021", b"2012", b"2022", b"1101", b"2101", b"1201", b"2201", // 2528
    b"1102", b"2102", b"1202", b"2202", b"1110", b"2110", b"1210", b"2210", // 2530
    b"1120", b"2120", b"1220", b"2220", b"1111", b"2111", b"1211", b"2211", // 2538
    b"1121", b"1112", b"1122", b"2121", b"1221", b"2112", b"1212", b"2221", // 2540
    b"2212", b"2122", b"1222", b"2222", b"1100", b"2200", b"0011", b"0022", // 2548
    b"3300", b"0033", b"0301", b"0103", b"0303", b"3001", b"1003", b"3003", // 2550
    b"0310", b"0130", b"0330", b"3010", b"1030", b"3030", b"0311", b"0133", // 2558
    b"0333", b"3011", b"1033", b"3033", b"3301", b"1103", b"3303", b"3310", // 2560
    b"1130", b"3330", b"3311", b"1133", b"3333", b"0101", b"1001", b"1010", // 2568
    b"0110", b"0000", b"0000", b"0000", b"1000", b"0010", b"0100", b"0001", // 2570
    b"2000", b"0020", b"0200", b"0002", b"1200", b"0012", b"2100", b"0021", // 2578
];

/// The arms of a box-drawing character, `None` if it is not one drawn here
/// (the diagonals are left to the font).
pub(crate) fn arms(c: char) -> Option<Arms> {
    let code = c as u32;
    if !(0x2500..=0x257f).contains(&code) {
        return None;
    }
    let entry = TABLE[(code - 0x2500) as usize];
    if entry == b"0000" {
        return None;
    }
    Some(Arms {
        left: Stroke::from_digit(entry[0]),
        right: Stroke::from_digit(entry[1]),
        up: Stroke::from_digit(entry[2]),
        down: Stroke::from_digit(entry[3]),
    })
}

/// How many dashes a dashed line is cut into, if `c` is one.
fn dashes(c: char) -> Option<u32> {
    Some(match c as u32 {
        0x2504..=0x2507 => 3,
        0x2508..=0x250b => 4,
        0x254c..=0x254f => 2,
        _ => return None,
    })
}

/// Which corner a rounded one is: (towards the right, towards the bottom).
fn arc_corner(c: char) -> Option<(bool, bool)> {
    match c {
        '╭' => Some((true, true)),
        '╮' => Some((false, true)),
        '╯' => Some((false, false)),
        '╰' => Some((true, false)),
        _ => None,
    }
}

/// Whether `c` is drawn here rather than by the font. The diagonals are not.
pub(crate) fn is_shape(c: char) -> bool {
    matches!(c as u32, 0x2500..=0x2570 | 0x2574..=0x259f | 0x2800..=0x28ff)
}

/// The drawing of a character that is drawn rather than typeset, or `None`
/// if the font should draw it.
pub(crate) fn shape(c: char, m: Metrics) -> Option<Shape> {
    let code = c as u32;
    match code {
        0x2500..=0x257f => {
            if let Some(corner) = arc_corner(c) {
                return Some(Shape::Arc(arc(corner, m)));
            }
            let arms = arms(c)?;
            if let Some(count) = dashes(c) {
                return Some(Shape::Rects(dashed(arms, count, m)));
            }
            Some(Shape::Rects(lines(arms, m)))
        }
        0x2580..=0x259f => block(code, m),
        0x2800..=0x28ff => Some(braille(code, m)),
        _ => None,
    }
}

fn thickness(stroke: Stroke, m: Metrics) -> f32 {
    match stroke {
        Stroke::None => 0.0,
        Stroke::Light | Stroke::Double => m.light,
        Stroke::Heavy => m.heavy(),
    }
}

/// The rectangles of a box-drawing character.
///
/// Each arm runs from the edge of the cell to where it meets the others. A
/// light or heavy arm is one line through the centre. A double arm is two
/// lines a line's width apart, and how far they run depends on what crosses
/// them, which follows the glyphs of the Unicode chart:
///
/// - Where no perpendicular arm is present they run to the centre.
/// - Where the perpendicular arms are single lines, they run to the centre
///   too, covering the half thickness of the line they meet.
/// - Where a perpendicular arm is double, each line stops at the line it
///   meets. With one such arm, the line on its side (the inner one) meets the
///   inner line and the other (the outer one) meets the outer line; with
///   both, every line meets an inner one, which is what leaves the middle of
///   a double cross empty.
fn lines(arms: Arms, m: Metrics) -> Vec<Rect> {
    let (cx, cy) = (m.width / 2.0, m.height / 2.0);
    let t = |s: Stroke| thickness(s, m);
    let mut out = Vec::new();

    // The thickness of what is at the centre along each axis, used to cover the
    // joint when a single line meets another.
    let th = t(arms.left).max(t(arms.right));
    let tv = t(arms.up).max(t(arms.down));
    let d = m.light; // distance of a double line's lines from the axis

    // A straight line is one rectangle across the cell, not two halves that
    // meet in the middle.
    let straight = |stroke: Stroke, horizontal: bool| -> Vec<Rect> {
        let offsets: &[f32] = if stroke == Stroke::Double { &[-d, d] } else { &[0.0] };
        let thick = t(stroke);
        offsets
            .iter()
            .map(|&s| {
                if horizontal {
                    m.span(0.0, cy + s - thick / 2.0, m.width, cy + s + thick / 2.0)
                } else {
                    m.span(cx + s - thick / 2.0, 0.0, cx + s + thick / 2.0, m.height)
                }
            })
            .collect()
    };
    if arms.up == Stroke::None && arms.down == Stroke::None && arms.left == arms.right && arms.left != Stroke::None {
        return straight(arms.left, true);
    }
    if arms.left == Stroke::None && arms.right == Stroke::None && arms.up == arms.down && arms.up != Stroke::None {
        return straight(arms.up, false);
    }

    // An arm along the horizontal axis. `towards` is +1 for right, -1 for left.
    let horizontal = |stroke: Stroke, towards: f32, out: &mut Vec<Rect>| {
        if stroke == Stroke::None {
            return;
        }
        let up = arms.up != Stroke::None;
        let down = arms.down != Stroke::None;
        let perpendicular_double = arms.up == Stroke::Double || arms.down == Stroke::Double;
        let edge = if towards > 0.0 { m.width } else { 0.0 };
        let offsets: &[f32] = if stroke == Stroke::Double { &[-d, d] } else { &[0.0] };
        for &s in offsets {
            let thick = t(stroke);
            // Where the line ends near the centre, as an offset from `cx`.
            let end = if !(up || down) {
                0.0
            } else if !perpendicular_double {
                -towards * tv / 2.0
            } else if stroke == Stroke::Double {
                let inner = (up && down) || (down && s > 0.0) || (up && s < 0.0);
                let line = if inner { towards * d } else { -towards * d };
                line - towards * m.light / 2.0
            } else {
                towards * d - towards * m.light / 2.0
            };
            let (x0, x1) = if towards > 0.0 { (cx + end, edge) } else { (edge, cx + end) };
            out.push(m.span(x0, cy + s - thick / 2.0, x1, cy + s + thick / 2.0));
        }
    };
    horizontal(arms.left, -1.0, &mut out);
    horizontal(arms.right, 1.0, &mut out);

    let vertical = |stroke: Stroke, towards: f32, out: &mut Vec<Rect>| {
        if stroke == Stroke::None {
            return;
        }
        let left = arms.left != Stroke::None;
        let right = arms.right != Stroke::None;
        let perpendicular_double = arms.left == Stroke::Double || arms.right == Stroke::Double;
        let edge = if towards > 0.0 { m.height } else { 0.0 };
        let offsets: &[f32] = if stroke == Stroke::Double { &[-d, d] } else { &[0.0] };
        for &s in offsets {
            let thick = t(stroke);
            let end = if !(left || right) {
                0.0
            } else if !perpendicular_double {
                -towards * th / 2.0
            } else if stroke == Stroke::Double {
                let inner = (left && right) || (right && s > 0.0) || (left && s < 0.0);
                let line = if inner { towards * d } else { -towards * d };
                line - towards * m.light / 2.0
            } else {
                towards * d - towards * m.light / 2.0
            };
            let (y0, y1) = if towards > 0.0 { (cy + end, edge) } else { (edge, cy + end) };
            out.push(m.span(cx + s - thick / 2.0, y0, cx + s + thick / 2.0, y1));
        }
    };
    vertical(arms.up, -1.0, &mut out);
    vertical(arms.down, 1.0, &mut out);

    out
}

fn dashed(arms: Arms, count: u32, m: Metrics) -> Vec<Rect> {
    let (cx, cy) = (m.width / 2.0, m.height / 2.0);
    let mut out = Vec::new();
    let n = count as f32;
    if arms.left != Stroke::None {
        let t = thickness(arms.left, m);
        let period = m.width / n;
        for i in 0..count {
            let start = i as f32 * period + period / 4.0;
            out.push(m.span(start, cy - t / 2.0, start + period / 2.0, cy + t / 2.0));
        }
    } else if arms.up != Stroke::None {
        let t = thickness(arms.up, m);
        let period = m.height / n;
        for i in 0..count {
            let start = i as f32 * period + period / 4.0;
            out.push(m.span(cx - t / 2.0, start, cx + t / 2.0, start + period / 2.0));
        }
    }
    out
}

fn arc((right, down): (bool, bool), m: Metrics) -> Arc {
    let (cx, cy) = (m.snap(m.width / 2.0), m.snap(m.height / 2.0));
    let t = m.light;
    let r = m.width.min(m.height) / 2.0;
    // The ring's centre is `r` away from the crossing in the direction of the
    // arms, and its bounds are the circle of radius `r + t/2`.
    let (ccx, ccy) = (cx + if right { r } else { -r }, cy + if down { r } else { -r });
    let outer = r + t / 2.0;
    let ring = Rect { x: ccx - outer, y: ccy - outer, w: outer * 2.0, h: outer * 2.0 };
    // The corner is the quadrant of the ring that faces the crossing.
    let clip = Rect {
        x: if right { cx - t / 2.0 } else { cx - r },
        y: if down { cy - t / 2.0 } else { cy - r },
        w: r + t / 2.0,
        h: r + t / 2.0,
    };
    let mut stubs = Vec::new();
    // The straight parts from the end of the arc to the cell's edge.
    if right {
        stubs.push(m.span(cx + r, cy - t / 2.0, m.width, cy + t / 2.0));
    } else {
        stubs.push(m.span(0.0, cy - t / 2.0, cx - r, cy + t / 2.0));
    }
    if down {
        stubs.push(m.span(cx - t / 2.0, cy + r, cx + t / 2.0, m.height));
    } else {
        stubs.push(m.span(cx - t / 2.0, 0.0, cx + t / 2.0, cy - r));
    }
    Arc { ring, thickness: t, clip, stubs }
}

fn block(code: u32, m: Metrics) -> Option<Shape> {
    let (w, h) = (m.width, m.height);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| m.span(x0, y0, x1, y1);
    let rects = |list: Vec<Rect>| Some(Shape::Rects(list));
    match code {
        0x2580 => rects(vec![rect(0.0, 0.0, w, h / 2.0)]),
        0x2581..=0x2588 => {
            let eighths = (code - 0x2580) as f32;
            rects(vec![rect(0.0, h - h * eighths / 8.0, w, h)])
        }
        0x2589..=0x258f => {
            let eighths = (0x2590 - code) as f32;
            rects(vec![rect(0.0, 0.0, w * eighths / 8.0, h)])
        }
        0x2590 => rects(vec![rect(w / 2.0, 0.0, w, h)]),
        0x2591 => Some(Shape::Shade(0.25)),
        0x2592 => Some(Shape::Shade(0.5)),
        0x2593 => Some(Shape::Shade(0.75)),
        0x2594 => rects(vec![rect(0.0, 0.0, w, h / 8.0)]),
        0x2595 => rects(vec![rect(w - w / 8.0, 0.0, w, h)]),
        0x2596..=0x259f => {
            // Bits: upper left, upper right, lower left, lower right.
            const QUADRANTS: [u8; 10] = [
                0b0010, 0b0001, 0b1000, 0b1011, 0b1001, 0b1110, 0b1101, 0b0100, 0b0110, 0b0111,
            ];
            // The chart lists them in the order ▖ ▗ ▘ ▙ ▚ ▛ ▜ ▝ ▞ ▟, and the
            // table follows it with bit 3 = upper left, 2 = upper right,
            // 1 = lower left, 0 = lower right.
            let bits = QUADRANTS[(code - 0x2596) as usize];
            let mut out = Vec::new();
            let (hw, hh) = (w / 2.0, h / 2.0);
            if bits & 0b1000 != 0 {
                out.push(rect(0.0, 0.0, hw, hh));
            }
            if bits & 0b0100 != 0 {
                out.push(rect(hw, 0.0, w, hh));
            }
            if bits & 0b0010 != 0 {
                out.push(rect(0.0, hh, hw, h));
            }
            if bits & 0b0001 != 0 {
                out.push(rect(hw, hh, w, h));
            }
            rects(out)
        }
        _ => None,
    }
}

fn braille(code: u32, m: Metrics) -> Shape {
    let bits = code - 0x2800;
    // Dot n of the pattern: (column, row). The two columns hold dots 1 2 3 7
    // and 4 5 6 8 top to bottom.
    const DOTS: [(usize, usize); 8] = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)];
    let diameter = (m.width / 4.0).min(m.height / 8.0 * 1.4).max(m.pixel);
    let mut centres = Vec::new();
    for (i, (column, row)) in DOTS.iter().enumerate() {
        if bits & (1 << i) != 0 {
            let x = m.width * (0.3 + 0.4 * *column as f32);
            let y = m.height * (0.125 + 0.25 * *row as f32);
            centres.push((x, y));
        }
    }
    Shape::Dots { diameter, centres }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        // 8x16 cell, 13 px text, 1x scale: a light line is 1 px.
        Metrics::new(8.0, 16.0, 13.0, 1.0)
    }

    fn rects(c: char) -> Vec<Rect> {
        match shape(c, metrics()) {
            Some(Shape::Rects(r)) => r,
            other => panic!("{c}: {other:?}"),
        }
    }

    #[test]
    fn a_vertical_line_spans_the_whole_height_of_the_cell() {
        let r = rects('│');
        let top = r.iter().map(|r| r.y).fold(f32::MAX, f32::min);
        let bottom = r.iter().map(|r| r.y + r.h).fold(f32::MIN, f32::max);
        assert_eq!((top, bottom), (0.0, 16.0));
    }

    #[test]
    fn a_horizontal_line_spans_the_whole_width() {
        let r = rects('─');
        let left = r.iter().map(|r| r.x).fold(f32::MAX, f32::min);
        let right = r.iter().map(|r| r.x + r.w).fold(f32::MIN, f32::max);
        assert_eq!((left, right), (0.0, 8.0));
    }

    #[test]
    fn a_line_runs_to_the_exact_edge_of_a_cell_that_is_not_a_whole_number_of_pixels_wide() {
        // The advance of a real font: the next cell starts at 8.43, so a line
        // that stopped at 8 would leave a gap that rounding turns into a
        // missing pixel in some cells.
        let m = Metrics::new(8.43, 18.0, 14.0, 1.0);
        let Some(Shape::Rects(r)) = shape('─', m) else { panic!("not rects") };
        let right = r.iter().map(|r| r.x + r.w).fold(f32::MIN, f32::max);
        assert_eq!(right, 8.43);
        let Some(Shape::Rects(r)) = shape('█', m) else { panic!("not rects") };
        assert_eq!(r[0].x + r[0].w, 8.43);
    }

    #[test]
    fn a_light_line_is_one_whole_pixel_on_the_pixel_grid() {
        let r = rects('─')[0];
        assert_eq!(r.h, 1.0);
        assert_eq!(r.y, r.y.round());
        let r = rects('│')[0];
        assert_eq!(r.w, 1.0);
        assert_eq!(r.x, r.x.round());
    }

    #[test]
    fn a_corner_covers_the_joint() {
        let r = rects('┌');
        assert_eq!(r.len(), 2);
        let (h, v) = (r[0], r[1]);
        // The horizontal arm starts no later than the vertical line's left
        // edge, and the vertical arm no later than the horizontal line's top,
        // so the corner is solid.
        assert!(h.x <= v.x);
        assert!(v.y <= h.y);
    }

    #[test]
    fn heavy_lines_are_twice_as_thick() {
        let light = rects('─')[0];
        let heavy = rects('━')[0];
        assert_eq!(heavy.h, light.h * 2.0);
    }

    #[test]
    fn a_double_line_is_two_lines() {
        assert_eq!(rects('═').len(), 2);
        assert_eq!(rects('║').len(), 2);
    }

    #[test]
    fn a_double_corner_leaves_its_inner_channel_open() {
        // ╔: the right arm's two lines start at different places, outer
        // before inner.
        let r = rects('╔');
        let horizontals: Vec<_> = r.iter().filter(|r| r.w > r.h).collect();
        assert_eq!(horizontals.len(), 2);
        let (upper, lower) = if horizontals[0].y < horizontals[1].y {
            (horizontals[0], horizontals[1])
        } else {
            (horizontals[1], horizontals[0])
        };
        assert!(upper.x < lower.x, "outer line starts before the inner one");
    }

    #[test]
    fn a_double_cross_has_an_empty_centre() {
        let m = metrics();
        let r = rects('╬');
        let (cx, cy) = (m.width / 2.0, m.height / 2.0);
        assert!(r.iter().all(|r| !(r.x < cx && r.x + r.w > cx && r.y < cy && r.y + r.h > cy)));
    }

    #[test]
    fn dashes_are_separate_pieces() {
        assert_eq!(rects('┄').len(), 3);
        assert_eq!(rects('┈').len(), 4);
        assert_eq!(rects('╎').len(), 2);
    }

    #[test]
    fn diagonals_are_left_to_the_font() {
        assert!(shape('╱', metrics()).is_none());
        assert!(shape('a', metrics()).is_none());
    }

    #[test]
    fn rounded_corners_are_arcs_with_stubs() {
        for c in ['╭', '╮', '╯', '╰'] {
            match shape(c, metrics()) {
                Some(Shape::Arc(arc)) => {
                    assert_eq!(arc.stubs.len(), 2, "{c}");
                    assert!(arc.ring.w > 0.0);
                }
                other => panic!("{c}: {other:?}"),
            }
        }
    }

    #[test]
    fn half_blocks_fill_half_the_cell() {
        let r = rects('▀');
        assert_eq!((r[0].y, r[0].h), (0.0, 8.0));
        let r = rects('▄');
        assert_eq!((r[0].y, r[0].h), (8.0, 8.0));
        let r = rects('▌');
        assert_eq!((r[0].x, r[0].w), (0.0, 4.0));
        let r = rects('█');
        assert_eq!((r[0].w, r[0].h), (8.0, 16.0));
    }

    #[test]
    fn quadrants_match_their_names() {
        let at = |c| rects(c).iter().map(|r| (r.x, r.y)).collect::<Vec<_>>();
        assert_eq!(at('▘'), [(0.0, 0.0)]);
        assert_eq!(at('▝'), [(4.0, 0.0)]);
        assert_eq!(at('▖'), [(0.0, 8.0)]);
        assert_eq!(at('▗'), [(4.0, 8.0)]);
        assert_eq!(rects('▙').len(), 3);
        assert_eq!(rects('▚').len(), 2);
    }

    #[test]
    fn shades_are_translucent() {
        assert_eq!(shape('░', metrics()), Some(Shape::Shade(0.25)));
        assert_eq!(shape('▓', metrics()), Some(Shape::Shade(0.75)));
    }

    #[test]
    fn braille_draws_one_dot_per_bit() {
        match shape('\u{2800}', metrics()) {
            Some(Shape::Dots { centres, .. }) => assert!(centres.is_empty()),
            other => panic!("{other:?}"),
        }
        match shape('\u{28ff}', metrics()) {
            Some(Shape::Dots { centres, .. }) => assert_eq!(centres.len(), 8),
            other => panic!("{other:?}"),
        }
        match shape('\u{2801}', metrics()) {
            Some(Shape::Dots { centres, .. }) => assert_eq!(centres.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_table_agrees_with_the_chart_on_a_few_known_characters() {
        let a = arms('┼').unwrap();
        assert_eq!((a.left, a.right, a.up, a.down), (Stroke::Light, Stroke::Light, Stroke::Light, Stroke::Light));
        let a = arms('╋').unwrap();
        assert_eq!(a.up, Stroke::Heavy);
        let a = arms('╧').unwrap();
        assert_eq!((a.left, a.right, a.up, a.down), (Stroke::Double, Stroke::Double, Stroke::Light, Stroke::None));
        let a = arms('╼').unwrap();
        assert_eq!((a.left, a.right), (Stroke::Light, Stroke::Heavy));
        let a = arms('┞').unwrap();
        assert_eq!((a.up, a.down, a.right, a.left), (Stroke::Heavy, Stroke::Light, Stroke::Light, Stroke::None));
    }
}
