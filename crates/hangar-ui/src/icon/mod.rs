//! The icons: the web app's one icon set, as SVG, drawn in the colour of the
//! text around them.
//!
//! Every glyph sits on the same 16-unit grid with the same 1.35-unit stroke,
//! round caps and round joins, as the web app's `src/components/Icon.tsx` drew
//! it (tag v1.12.1). The table in `data.rs` was lifted out of that file and is
//! the source now; the HTML kits' stylesheet takes a few of its drawings from
//! it too (`scripts/gen-hangar-ui.mjs`).
//!
//! # Adding an icon
//!
//! In `data.rs`, give it a variant of [`Icon`] at the end of the enum, the
//! same at the end of `ALL` (one more in its length and in the count of
//! `every_icon_of_the_web_app_is_there`), its name in `name()` (camelCase:
//! the layouts and settings store it), and its drawing in `elements()`. The
//! drawing is the SVG elements inside a `viewBox="0 0 16 16"` box (`<path>`,
//! `<rect>`, `<circle>`), stroked by the shared style; a filled part says
//! `fill="#000"`, which the tint replaces. Keep to the grid and the stroke,
//! so it sits with the others.
//!
//! ```ignore
//! use hangar_ui::{icon, Icon};
//!
//! // Takes the colour of whatever it sits in: a button's text, a menu item's
//! // highlight, a muted label. That is `currentColor`, which iced's own `svg`
//! // widget cannot do (it wants the colour in its style).
//! row![icon(Icon::Gear, 14.0), text("Settings")]
//!
//! // Or its own colour, from the theme:
//! icon(Icon::Alert, 14.0).color(t.colors.warning)
//! ```
//!
//! An icon is decoration: the control around it carries the label.

mod data;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use iced::advanced::Widget;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::svg;
use iced::advanced::widget::Tree;
use iced::mouse;
use iced::{Color, Element, Length, Radians, Rectangle, Size};

pub use data::Icon;

/// A glyph as a whole SVG document: the shared stroke around its elements.
fn document(icon: Icon) -> String {
    format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" "##,
            r##"stroke="#000" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">"##,
            "{}</svg>"
        ),
        icon.elements()
    )
}

/// One handle per icon, made once: a handle hashes its bytes when it is
/// created, and the renderer keys its rasterised copies on that hash.
static HANDLES: LazyLock<Vec<svg::Handle>> = LazyLock::new(|| {
    Icon::ALL
        .iter()
        .map(|icon| svg::Handle::from_memory(document(*icon).into_bytes()))
        .collect()
});

impl Icon {
    /// The SVG document of the icon, in black, for anything that wants the
    /// drawing itself (an export, a test).
    pub fn svg(self) -> String {
        document(self)
    }

    /// The handle to give a renderer or iced's `svg` widget.
    pub fn handle(self) -> svg::Handle {
        HANDLES[self as usize].clone()
    }

    /// The icon with this name in the web app (`"splitRight"`), for what the
    /// settings and saved layouts store as text.
    pub fn from_name(name: &str) -> Option<Icon> {
        Icon::ALL.iter().copied().find(|icon| icon.name() == name)
    }
}

/// An icon of this side in pixels, drawn in the text colour it inherits.
///
/// Sizes in the stylesheets are 10 to 18 and mostly 14 ([`size::ICON`]).
pub fn icon(icon: Icon, side: f32) -> IconView {
    IconView {
        icon,
        side,
        color: None,
        rotation: Radians(0.0),
    }
}

/// The widget [`icon`] makes.
#[derive(Debug, Clone, Copy)]
pub struct IconView {
    icon: Icon,
    side: f32,
    color: Option<Color>,
    rotation: Radians,
}

impl IconView {
    /// Draws in this colour instead of the one it inherits. Its alpha counts:
    /// a faded colour fades the glyph.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Turns the glyph by this many radians about its centre: the section
    /// chevron folds this way.
    pub fn rotate(mut self, rotation: impl Into<Radians>) -> Self {
        self.rotation = rotation.into();
        self
    }
}

/// Draws `icon` into the square `bounds` in `color`. For widgets that draw
/// their own glyphs (the menus) rather than holding an [`IconView`] element.
pub fn draw<R: svg::Renderer>(
    renderer: &mut R,
    icon: Icon,
    bounds: Rectangle,
    color: Color,
    rotation: Radians,
) {
    // The renderer replaces the colour of every pixel of the glyph with the
    // tint's red, green and blue and ignores its alpha, which is carried by
    // the opacity instead. The turn is in the drawing and not asked of the
    // renderer: the software renderer that iced falls back on without a GPU
    // loses a glyph it is asked to rotate.
    renderer.draw_svg(
        svg::Svg {
            handle: turned_handle(icon, rotation),
            color: Some(Color { a: 1.0, ..color }),
            rotation: Radians(0.0),
            opacity: color.a,
        },
        bounds,
        bounds,
    );
}

/// The handle of an icon turned by `rotation`, to the nearest degree. Made on
/// first use and kept: the few turns an interface asks for (a folded
/// chevron) are drawn every frame.
fn turned_handle(icon: Icon, rotation: Radians) -> svg::Handle {
    static TURNED: LazyLock<Mutex<HashMap<(Icon, u16), svg::Handle>>> =
        LazyLock::new(Mutex::default);
    let degrees = rotation.0.to_degrees().rem_euclid(360.0).round() as u16 % 360;
    if degrees == 0 {
        return icon.handle();
    }
    let mut turned = TURNED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    turned
        .entry((icon, degrees))
        .or_insert_with(|| {
            let document = format!(
                concat!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" "##,
                    r##"stroke="#000" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">"##,
                    r##"<g transform="rotate({} 8 8)">{}</g></svg>"##
                ),
                degrees,
                icon.elements()
            );
            svg::Handle::from_memory(document.into_bytes())
        })
        .clone()
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for IconView
where
    Renderer: svg::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.side), Length::Fixed(self.side))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.side, self.side)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let color = self.color.unwrap_or(style.text_color);
        draw(renderer, self.icon, layout.bounds(), color, self.rotation);
    }
}

impl<'a, Message, Theme, Renderer> From<IconView> for Element<'a, Message, Theme, Renderer>
where
    Renderer: svg::Renderer + 'a,
    Theme: 'a,
    Message: 'a,
{
    fn from(view: IconView) -> Self {
        Element::new(view)
    }
}

#[cfg(test)]
mod tests;
