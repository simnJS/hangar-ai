//! Long logs and code: what a kit gives by the thousand lines, drawn a few
//! dozen at a time, and the ids by which a kit's operations reach a widget.
//!
//! A log with an `id` has lines of one height, kept whole (it scrolls
//! sideways for a long one), so that where a line is follows from its index
//! alone: Hangar draws the lines in view and some around them between two
//! spaces as tall as the rest, and a scroll to a line is a scroll to an
//! offset. A text area with line numbers is laid out the same way, its
//! gutter beside it in one scroll.

use iced::alignment::Horizontal;
use iced::widget::scrollable::{Direction, Viewport};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{Column, Row, container, scrollable, space, text};
use iced::{Background, Length, Pixels};

use hangar_kit_ui as ui;
use hangar_kit_ui::{HandlerId, LineKind, Range, TextSize, Weight};

use crate::message::{KitMessage, Local};

use super::{Cx, El, scroll_style, surface, thin};

/// The height of a line of a long log, in logical pixels.
pub(crate) const LOG_LINE: f32 = 17.0;
/// Above the first line and below the last.
const LOG_PAD: f32 = 4.0;
/// The width of a character of the terminals' face at the size of code, near
/// enough to size a line that is not wrapped.
const CHAR: f32 = 7.0;
/// Lines drawn beyond those in view, each way.
const MARGIN: usize = 40;
/// Lines asked for beyond those in view, each way, when a log is given in
/// parts: enough that a scroll of a few screens needs no new view.
const ASK_MARGIN: u32 = 200;
/// The height of a line of a text area with line numbers.
pub(crate) const EDITOR_LINE: f32 = 16.0;
const EDITOR_PAD: f32 = 8.0;
/// How many lines above the one a scroll brings into view.
const CONTEXT: u32 = 3;

/// The id iced knows a kit's widget by: the tab's namespace and the kit's
/// id, so that two tabs never name the same field.
pub(crate) fn widget_id(ns: u64, id: &str) -> String {
    format!("kit{ns}/{id}")
}

/// The scroll of a text area with line numbers, which holds it and its
/// gutter.
pub(crate) fn area_scroller_id(ns: u64, id: &str) -> String {
    format!("kit{ns}/{id}#lines")
}

/// Where a log's line, from 0, is scrolled to: a few lines under the top.
pub(crate) fn log_line_top(line: u32) -> f32 {
    LOG_PAD + line.saturating_sub(CONTEXT) as f32 * LOG_LINE
}

/// Where a text area's line, from 0, is scrolled to.
pub(crate) fn editor_line_top(line: u32) -> f32 {
    line.saturating_sub(CONTEXT) as f32 * EDITOR_LINE
}

/// What a scroll operation reaches.
pub(crate) enum Scroller {
    Log,
    Scroll,
}

/// The widget of `id` a scroll reaches, in `root`.
pub(crate) fn find_scroller(root: &ui::Element, id: &str) -> Option<Scroller> {
    let mut found = None;
    root.for_each(&mut |element| match element {
        ui::Element::Log(log) if found.is_none() && log.id == id => found = Some(Scroller::Log),
        ui::Element::Scroll(scroll) if found.is_none() && scroll.id == id => {
            found = Some(Scroller::Scroll)
        }
        _ => {}
    });
    found
}

/// Whether `root` has a field of `id` the keyboard can go to.
pub(crate) fn find_field(root: &ui::Element, id: &str) -> bool {
    let mut found = false;
    root.for_each(&mut |element| match element {
        ui::Element::TextInput(input) if input.id == id => found = true,
        ui::Element::Search(search) if search.id == id => found = true,
        ui::Element::TextArea(area) if area.id == id => found = true,
        _ => {}
    });
    found
}

/// The text area of `id` in `root`, and whether it has line numbers.
pub(crate) fn find_area(root: &ui::Element, id: &str) -> Option<bool> {
    let mut found = None;
    root.for_each(&mut |element| {
        if let ui::Element::TextArea(area) = element
            && area.id == id
        {
            found = Some(area.line_numbers);
        }
    });
    found
}

/// The lines of a log that a view starting `top` and `height` tall shows,
/// from the first to before the last.
fn in_view(top: f32, height: f32, total: usize) -> (usize, usize) {
    let start = ((top - LOG_PAD) / LOG_LINE).floor().max(0.0) as usize;
    let end = ((top + height - LOG_PAD) / LOG_LINE).ceil().max(0.0) as usize + 1;
    (start.min(total), end.min(total))
}

/// For the log `id` of `root` given in parts, the lines to ask for when a
/// view starting `top` and `height` tall shows lines it was not given; none
/// when it was given them all, or is not given in parts.
pub(crate) fn range_wanted(
    root: &ui::Element,
    id: &str,
    top: f32,
    height: f32,
) -> Option<(Range, HandlerId)> {
    let mut found = None;
    root.for_each(&mut |element| {
        if let ui::Element::Log(log) = element
            && log.id == id
            && let (Some(total), Some(handler)) = (log.total, log.on_range)
        {
            found = Some((total, log.first, log.lines.len() as u32, handler));
        }
    });
    let (total, first, given, handler) = found?;
    let (start, end) = in_view(top, height, total as usize);
    let (start, end) = (start as u32, end as u32);
    if start >= first && end <= first + given {
        return None;
    }
    Some((
        Range {
            start: start.saturating_sub(ASK_MARGIN),
            end: (end + ASK_MARGIN).min(total),
        },
        handler,
    ))
}

/// A log with an id: its lines in view drawn, the others the room they take.
pub(crate) fn long_log<'a>(cx: &Cx<'a>, w: &'a ui::Log) -> El<'a> {
    let style = cx.style;
    let p = style.palette;
    let windowed = w.total.is_some();
    let total = w.total.unwrap_or(w.lines.len() as u32) as usize;
    let first = if windowed { w.first as usize } else { 0 };
    let room = w.height.unwrap_or(240.0);
    let full = total as f32 * LOG_LINE + 2.0 * LOG_PAD;
    let height = full.min(room);
    let (top, seen) = match cx.widgets.scrolls.get(&w.id) {
        Some(&(top, seen)) => (top, seen),
        // Not scrolled yet: at its start, or at its end for one that opens
        // there.
        None if w.at_end => ((full - height).max(0.0), height),
        None => (0.0, height),
    };
    let (start, end) = in_view(top, seen, total);
    let draw_start = start.saturating_sub(MARGIN).max(first);
    let draw_end = (end + MARGIN).min(first + w.lines.len()).min(total);
    let numbers = w.lines.iter().any(|line| line.number.is_some());
    let digits = w
        .lines
        .iter()
        .filter_map(|line| line.number)
        .max()
        .map(|largest| largest.to_string().len())
        .unwrap_or(0);
    let gutter = digits as f32 * CHAR + 8.0;
    let drawn = if draw_start < draw_end {
        &w.lines[draw_start - first..draw_end - first]
    } else {
        &[][..]
    };
    let widest = drawn
        .iter()
        .map(|line| line.text.chars().count())
        .max()
        .unwrap_or(0);
    let width = widest as f32 * CHAR + if numbers { gutter + 10.0 } else { 0.0 } + 16.0;
    let rows = drawn.iter().map(|given| {
        let (color, tint) = match given.kind {
            LineKind::Error => (p.danger, None),
            LineKind::Warning => (p.warn, None),
            LineKind::Head => (p.text, None),
            LineKind::Normal => (p.text_dim, None),
            LineKind::Added => (
                p.text,
                Some(iced::Color {
                    a: 0.16,
                    ..p.ok_fill
                }),
            ),
            LineKind::Removed => (p.text, Some(p.danger_soft)),
        };
        let weight = if given.kind == LineKind::Head {
            Weight::Semibold
        } else {
            Weight::Normal
        };
        let mut parts: Vec<El<'a>> = Vec::new();
        if numbers {
            parts.push(
                text(given.number.map(|n| n.to_string()).unwrap_or_default())
                    .size(TextSize::Code.px())
                    .font(style.mono)
                    .color(p.text_faint)
                    .align_x(Horizontal::Right)
                    .wrapping(Wrapping::None)
                    .width(gutter)
                    .into(),
            );
        }
        parts.push(
            text(given.text.as_str())
                .size(TextSize::Code.px())
                .font(style.weighted(weight, true))
                .color(color)
                .wrapping(Wrapping::None)
                .into(),
        );
        let background = if given.current {
            Some(p.selected)
        } else {
            tint
        };
        container(Row::with_children(parts).spacing(10))
            .padding([0, 8])
            .height(LOG_LINE)
            .width(width)
            .clip(true)
            .style(move |_| container::Style {
                background: background.map(Background::Color),
                ..Default::default()
            })
            .into()
    });
    let mut lines: Vec<El<'a>> = vec![
        space()
            .height(LOG_PAD + draw_start as f32 * LOG_LINE)
            .into(),
    ];
    lines.extend(rows);
    lines.push(
        space()
            .height(LOG_PAD + total.saturating_sub(draw_end.max(draw_start)) as f32 * LOG_LINE)
            .into(),
    );
    let id = w.id.clone();
    let scroll = scrollable(Column::with_children(lines))
        .id(widget_id(cx.ns, &w.id))
        .direction(Direction::Both {
            vertical: thin(),
            horizontal: thin(),
        })
        .height(height)
        .width(Length::Fill)
        .on_scroll(move |viewport: Viewport| {
            KitMessage::Local(Local::Scrolled {
                log: id.clone(),
                top: viewport.absolute_offset().y,
                height: viewport.bounds().height,
            })
        })
        .style(scroll_style(style));
    let scroll: El<'a> = if w.at_end {
        scroll.anchor_bottom().into()
    } else {
        scroll.into()
    };
    container(scroll)
        .width(Length::Fill)
        .style(surface(style, Some(p.well), Some(p.line), 6.0))
        .into()
}

/// A text area with line numbers: a gutter of numbers, its marks on them,
/// beside the lines kept whole, both in one scroll.
pub(crate) fn numbered_area<'a>(
    cx: &Cx<'a>,
    w: &'a ui::TextArea,
    editor: El<'a>,
    lines: usize,
) -> El<'a> {
    let style = cx.style;
    let p = style.palette;
    let digits = lines.max(1).to_string().len().max(2);
    let numbers = (1..=lines.max(1)).map(|number| {
        let mark = w.marks.iter().find(|mark| mark.line as usize == number);
        let (background, color) = match mark.map(|mark| mark.kind) {
            Some(ui::Notice::Error) => (Some(p.danger_soft), p.danger),
            Some(ui::Notice::Warning) => (
                Some(iced::Color {
                    a: 0.18,
                    ..p.warn_fill
                }),
                p.warn,
            ),
            Some(ui::Notice::Success) => (
                Some(iced::Color {
                    a: 0.16,
                    ..p.ok_fill
                }),
                p.text,
            ),
            Some(ui::Notice::Info) => (Some(p.accent_soft), p.text),
            None => (None, p.text_faint),
        };
        let number: El<'a> = container(
            text(number.to_string())
                .size(TextSize::Control.px())
                .font(style.mono)
                .color(color)
                .line_height(LineHeight::Absolute(Pixels(EDITOR_LINE)))
                .align_x(Horizontal::Right)
                .width(Length::Fill),
        )
        .height(EDITOR_LINE)
        .padding([0, 6])
        .style(move |_| container::Style {
            background: background.map(Background::Color),
            ..Default::default()
        })
        .into();
        match mark.and_then(|mark| mark.label.as_deref()) {
            Some(label) => super::with_tooltip(style, number, Some(label)),
            None => number,
        }
    });
    let gutter = container(Column::with_children(numbers))
        .padding([EDITOR_PAD, 0.0])
        .width(digits as f32 * CHAR + 14.0)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.well)),
            ..Default::default()
        });
    let height = if w.fill {
        Length::Fill
    } else {
        Length::Fixed(w.height.unwrap_or(240.0))
    };
    container(
        scrollable(Row::with_children([gutter.into(), editor]))
            .id(area_scroller_id(cx.ns, &w.id))
            .direction(Direction::Vertical(thin()))
            .height(height)
            .style(scroll_style(style)),
    )
    .width(Length::Fill)
    .height(height)
    .style(surface(style, Some(p.field), Some(p.border), 6.0))
    .into()
}
