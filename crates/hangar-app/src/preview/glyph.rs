//! The preview's own glyphs (`components/preview/Glyph.tsx`): a mark for each
//! kind of file a tab can hold, and the few tools the app's icon set has no
//! need for elsewhere. Drawn as `hangar-ui`'s icons are, on the same 16-unit
//! grid with the same 1.35-unit stroke, round caps and joins, so a tab of an
//! image sits beside a tab of a model and a toolbar of both lines up.

use std::sync::LazyLock;

use iced::widget::svg;
use iced::{Color, Element};

use super::model::{FileKind, Kind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Image,
    Video,
    Audio,
    Text,
    Markdown,
    Pdf,
    Html,
    Code,
    Wrap,
    Loop,
}

impl Glyph {
    const ALL: [Glyph; 10] = [
        Glyph::Image,
        Glyph::Video,
        Glyph::Audio,
        Glyph::Text,
        Glyph::Markdown,
        Glyph::Pdf,
        Glyph::Html,
        Glyph::Code,
        Glyph::Wrap,
        Glyph::Loop,
    ];

    /// The mark of a kind of file.
    pub fn of_kind(kind: FileKind) -> Glyph {
        match kind {
            FileKind::Image => Glyph::Image,
            FileKind::Video => Glyph::Video,
            FileKind::Audio => Glyph::Audio,
            FileKind::Text => Glyph::Text,
            FileKind::Markdown => Glyph::Markdown,
            FileKind::Pdf => Glyph::Pdf,
            FileKind::Html => Glyph::Html,
        }
    }

    /// The shapes, without the frame they share.
    fn elements(self) -> &'static str {
        match self {
            Glyph::Image => concat!(
                r#"<rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2"/>"#,
                r#"<circle cx="5.6" cy="6.3" r="1.2"/>"#,
                r#"<path d="M1.9 11.6 5.5 8.2l2.7 2.5 2.3-2.1 3.6 3.3"/>"#,
            ),
            Glyph::Video => concat!(
                r#"<rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2"/>"#,
                r#"<path d="M6.6 5.7v4.6L10.4 8z"/>"#,
            ),
            Glyph::Audio => concat!(
                r#"<path d="M6.1 12.1V3.7l6.6-1.5v8.3"/>"#,
                r#"<circle cx="4.4" cy="12.1" r="1.75"/>"#,
                r#"<circle cx="11" cy="10.5" r="1.75"/>"#,
            ),
            Glyph::Text => concat!(
                r#"<path d="M4.4 1.75h4.9l3.3 3.3v7.7a1.5 1.5 0 0 1-1.5 1.5H4.4a1.5 1.5 0 0 1-1.5-1.5V3.25a1.5 1.5 0 0 1 1.5-1.5z"/>"#,
                r#"<path d="M9.3 1.75v3.3h3.3M5.5 8.3h5M5.5 11h3.4"/>"#,
            ),
            Glyph::Markdown => concat!(
                r#"<rect x="1.75" y="2.75" width="12.5" height="10.5" rx="2.2"/>"#,
                r#"<path d="M4.3 10.4V5.6l1.9 2.3 1.9-2.3v4.8M11.4 5.6v4.6M9.9 8.8l1.5 1.5 1.5-1.5"/>"#,
            ),
            Glyph::Pdf => concat!(
                r#"<path d="M4.4 1.75h4.9l3.3 3.3v7.7a1.5 1.5 0 0 1-1.5 1.5H4.4a1.5 1.5 0 0 1-1.5-1.5V3.25a1.5 1.5 0 0 1 1.5-1.5z"/>"#,
                r#"<path d="M9.3 1.75v3.3h3.3"/>"#,
                r#"<rect x="1.6" y="8.2" width="8.4" height="3.6" rx="1"/>"#,
            ),
            Glyph::Html => concat!(
                r#"<circle cx="8" cy="8" r="6.1"/>"#,
                r#"<path d="M1.9 8h12.2M8 1.9c1.6 1.7 2.5 3.8 2.5 6.1S9.6 12.4 8 14.1C6.4 12.4 5.5 10.3 5.5 8S6.4 3.6 8 1.9z"/>"#,
            ),
            Glyph::Code => {
                r#"<path d="M5.4 4.6 2 8l3.4 3.4M10.6 4.6 14 8l-3.4 3.4M9.1 3.1 6.9 12.9"/>"#
            }
            Glyph::Wrap => {
                r#"<path d="M2.2 4h11.6M2.2 8h9a2.25 2.25 0 0 1 0 4.5H8.4M10 10.9l-1.6 1.6 1.6 1.6M2.2 12.5h3.4"/>"#
            }
            Glyph::Loop => {
                r#"<path d="M2.6 7.3v-.8A2.5 2.5 0 0 1 5.1 4h7.6M10.9 2.2 12.7 4l-1.8 1.8M13.4 8.7v.8a2.5 2.5 0 0 1-2.5 2.5H3.3M5.1 13.8 3.3 12l1.8-1.8"/>"#
            }
        }
    }

    /// The glyph as a whole SVG document, in black.
    pub fn svg(self) -> String {
        format!(
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" "##,
                r##"stroke="#000" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">"##,
                "{}</svg>"
            ),
            self.elements()
        )
    }

    fn handle(self) -> svg::Handle {
        // One handle per glyph, made once: a handle hashes its bytes when it
        // is created, and the renderer keys its rasterised copies on that.
        static HANDLES: LazyLock<Vec<svg::Handle>> = LazyLock::new(|| {
            Glyph::ALL
                .iter()
                .map(|glyph| svg::Handle::from_memory(glyph.svg().into_bytes()))
                .collect()
        });
        let at = Glyph::ALL
            .iter()
            .position(|glyph| *glyph == self)
            .unwrap_or_default();
        HANDLES[at].clone()
    }
}

/// A glyph of this side in pixels, in `color`.
pub fn glyph<'a, M: 'a>(which: Glyph, side: f32, color: Color) -> Element<'a, M> {
    svg(which.handle())
        .width(side)
        .height(side)
        .style(move |_theme, _status| svg::Style { color: Some(color) })
        .into()
}

/// The glyph of a kind of preview, `None` for a 3D model (the app's cube).
pub fn of(kind: Kind) -> Option<Glyph> {
    FileKind::of(kind).map(Glyph::of_kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_an_svg_a_renderer_can_read() {
        for glyph in Glyph::ALL {
            let document = glyph.svg();
            assert!(
                usvg::Tree::from_str(&document, &usvg::Options::default()).is_ok(),
                "{glyph:?}: {document}"
            );
        }
    }

    #[test]
    fn every_file_kind_has_its_own_mark() {
        let marks: Vec<Glyph> = [
            FileKind::Image,
            FileKind::Video,
            FileKind::Audio,
            FileKind::Text,
            FileKind::Markdown,
            FileKind::Pdf,
            FileKind::Html,
        ]
        .into_iter()
        .map(Glyph::of_kind)
        .collect();
        for (at, mark) in marks.iter().enumerate() {
            assert!(!marks[..at].contains(mark), "{mark:?} twice");
        }
        assert_eq!(of(Kind::Model), None);
    }
}
