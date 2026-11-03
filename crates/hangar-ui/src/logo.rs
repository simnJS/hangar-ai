//! The app mark (`Logo.tsx`): a workspace cut into panes, one of them active,
//! the same shape the interface makes. Drawn as the bare glyph so it takes the
//! theme's foreground and accent like everything else; the tile-backed version
//! used for the OS icon is a bundled asset.
//!
//! ```ignore
//! row![hangar_ui::logo::logo(t, 15.0), text("Hangar.AI")]
//! ```
//!
//! Decorative wherever it ships: the name is always written beside it.

use hangar_theme::Theme;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::Tree;
use iced::advanced::{Renderer as _, Widget};
use iced::{Background, Color, Element, Length, Point, Rectangle, Size, mouse};

/// The mark's drawing, on a 32-unit square: the active pane (the tall one)
/// first, then the two others.
const ACTIVE: (f32, f32, f32, f32) = (4.0, 4.0, 10.0, 24.0);
const OTHERS: [(f32, f32, f32, f32); 2] = [(17.0, 4.0, 11.0, 10.0), (17.0, 17.0, 11.0, 11.0)];
const GRID: f32 = 32.0;

/// The mark, `side` pixels across.
pub fn logo<'a, M: 'a>(t: &Theme, side: f32) -> Element<'a, M> {
    Element::new(Logo {
        side,
        active: t.colors.accent,
        others: t.colors.fg,
    })
}

struct Logo {
    side: f32,
    active: Color,
    others: Color,
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Logo {
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

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let unit = bounds.width.min(bounds.height) / GRID;
        let paint = |renderer: &mut iced::Renderer, (x, y, w, h): (f32, f32, f32, f32), c| {
            renderer.fill_quad(
                Quad {
                    bounds: Rectangle::new(
                        Point::new(bounds.x + x * unit, bounds.y + y * unit),
                        Size::new(w * unit, h * unit),
                    ),
                    ..Quad::default()
                },
                Background::Color(c),
            );
        };
        paint(renderer, ACTIVE, self.active);
        for rect in OTHERS {
            paint(renderer, rect, self.others);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_panes_stay_inside_the_grid_and_apart() {
        let all = [ACTIVE, OTHERS[0], OTHERS[1]];
        for (x, y, w, h) in all {
            assert!(x >= 0.0 && y >= 0.0 && x + w <= GRID && y + h <= GRID);
        }
        // The active pane ends before the others begin.
        assert!(ACTIVE.0 + ACTIVE.2 < OTHERS[0].0);
        assert!(OTHERS[0].1 + OTHERS[0].3 < OTHERS[1].1);
    }

    #[test]
    fn the_mark_takes_the_themes_accent_and_foreground() {
        let t = &hangar_theme::themes()[0];
        let mark = Logo {
            side: 15.0,
            active: t.colors.accent,
            others: t.colors.fg,
        };
        assert_ne!(mark.active, mark.others);
    }
}
