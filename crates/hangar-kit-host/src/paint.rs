//! A canvas's marks, laid out once for both of the places they are drawn:
//! the window (render/canvas.rs, iced's canvas) and the picture
//! `canvas.export` writes (here, with tiny-skia). Each mark becomes a few
//! primitives in the canvas's units, a text's box placed by its measured
//! width, so the picture a kit hands an agent is the one the person drew.
//!
//! Text is Inter, the interface's face, measured and drawn with ab_glyph
//! from the same file iced is given (hangar-theme's fonts), so a label's
//! box fits its words on screen and in the file alike.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, VariableFont, point};
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Rect,
    Stroke, Transform,
};

use hangar_kit_ui::{Align, Ink, Mark, Point, Shape};

/// A part of a mark, in the canvas's units, with its colour as RGBA.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Prim {
    /// A line through points, its ends and corners round.
    Polyline {
        points: Vec<Point>,
        width: f32,
        rgb: [u8; 3],
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        stroke: Option<(f32, [u8; 3])>,
        fill: Option<[u8; 3]>,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        stroke: Option<(f32, [u8; 3])>,
        fill: Option<[u8; 3]>,
    },
    /// A line of text whose left end is at `x` and middle at `y`.
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        bold: bool,
        rgb: [u8; 3],
    },
}

/// The padding of a text's box, as a share of its size: kit-annotate's.
const TEXT_PAD: f32 = 0.35;

fn fonts() -> &'static (FontRef<'static>, FontRef<'static>) {
    static FONTS: OnceLock<(FontRef<'static>, FontRef<'static>)> = OnceLock::new();
    FONTS.get_or_init(|| {
        let regular = FontRef::try_from_slice(hangar_theme::fonts::INTER_VARIABLE)
            .expect("Inter, which hangar-theme ships, reads");
        let mut bold = regular.clone();
        bold.set_variation(b"wght", 700.0);
        (regular, bold)
    })
}

fn font(bold: bool) -> &'static FontRef<'static> {
    let (regular, heavy) = fonts();
    if bold { heavy } else { regular }
}

/// The scale of a text `size` pixels to the em, as iced and a canvas size
/// one: ab_glyph's scale is the height from the ascent to the descent,
/// which for Inter is 1.21 em.
fn em(font: &FontRef<'static>, size: f32) -> PxScale {
    let units = font.units_per_em().unwrap_or(1000.0);
    PxScale::from(size * font.height_unscaled() / units)
}

/// The glyphs of a line of text and where each starts, from 0.
fn shape_line(text: &str, size: f32, bold: bool) -> (Vec<(GlyphId, f32)>, f32) {
    let scaled = font(bold).as_scaled(em(font(bold), size));
    let mut caret = 0.0;
    let mut previous: Option<GlyphId> = None;
    let mut glyphs = Vec::new();
    for c in text.chars().filter(|c| !c.is_control()) {
        let id = scaled.glyph_id(c);
        if let Some(previous) = previous {
            caret += scaled.kern(previous, id);
        }
        glyphs.push((id, caret));
        caret += scaled.h_advance(id);
        previous = Some(id);
    }
    (glyphs, caret)
}

/// How wide a line of text is, in the units of its size.
pub(crate) fn measure(text: &str, size: f32, bold: bool) -> f32 {
    shape_line(text, size, bold).1
}

/// The primitives of `marks` on a canvas of `width` by `height`, a mark
/// without a width of its own drawn `stroke` wide.
pub(crate) fn layout(marks: &[Mark], width: f32, height: f32, stroke: f32) -> Vec<Prim> {
    let mut prims = Vec::new();
    for mark in marks {
        let rgb = mark.ink.rgb();
        let line = mark.width.unwrap_or(stroke).max(0.1);
        let fill = mark.fill.map(Ink::rgb);
        match &mark.shape {
            Shape::Line { from, to, arrow } => {
                prims.push(Prim::Polyline {
                    points: vec![*from, *to],
                    width: line,
                    rgb,
                });
                if *arrow {
                    let head = (line * 5.0).max(14.0);
                    let angle = (to.y - from.y).atan2(to.x - from.x);
                    for side in [-1.0f32, 1.0] {
                        let turned = angle + side * 0.5;
                        prims.push(Prim::Polyline {
                            points: vec![
                                *to,
                                Point::new(to.x - head * turned.cos(), to.y - head * turned.sin()),
                            ],
                            width: line,
                            rgb,
                        });
                    }
                }
            }
            Shape::Rect { x, y, w, h } => prims.push(Prim::Rect {
                x: *x,
                y: *y,
                w: *w,
                h: *h,
                stroke: Some((line, rgb)),
                fill,
            }),
            Shape::Ellipse { x, y, w, h } => prims.push(Prim::Ellipse {
                cx: x + w / 2.0,
                cy: y + h / 2.0,
                rx: (w / 2.0).abs(),
                ry: (h / 2.0).abs(),
                stroke: Some((line, rgb)),
                fill,
            }),
            Shape::Path { points } if points.len() >= 2 => prims.push(Prim::Polyline {
                points: points.clone(),
                width: line,
                rgb,
            }),
            Shape::Path { .. } => {}
            Shape::Circle { center, r } => prims.push(Prim::Ellipse {
                cx: center.x,
                cy: center.y,
                rx: *r,
                ry: *r,
                // A pin: its edge as thin as half a line, as kit-annotate's.
                stroke: mark.width.map(|width| (width, rgb)).or(if fill.is_some() {
                    Some(((stroke / 2.0).max(1.0), rgb))
                } else {
                    Some((line, rgb))
                }),
                fill,
            }),
            Shape::Text {
                at,
                text,
                size,
                bold,
                align,
                fallback,
            } => {
                let size = size.max(1.0);
                let pad = if fill.is_some() { size * TEXT_PAD } else { 0.0 };
                let text_width = measure(text, size, *bold);
                let box_width = text_width + 2.0 * pad;
                let left_of = |point: Point, align: Align| match align {
                    Align::Start => point.x,
                    Align::Center => point.x - box_width / 2.0,
                    Align::End => point.x - box_width,
                };
                let inside = |left: f32| left >= 0.0 && left + box_width <= width;
                let mut chosen = (*at, *align);
                if !inside(left_of(*at, *align))
                    && let Some(other) = fallback
                {
                    let turned = match align {
                        Align::Start => Align::End,
                        Align::End => Align::Start,
                        Align::Center => Align::Center,
                    };
                    chosen = (*other, turned);
                }
                let left = left_of(chosen.0, chosen.1);
                let y = chosen.0.y;
                if let Some(fill) = fill {
                    let box_height = size + 2.0 * pad;
                    prims.push(Prim::Rect {
                        x: left,
                        y: y - box_height / 2.0,
                        w: box_width,
                        h: box_height,
                        stroke: None,
                        fill: Some(fill),
                    });
                }
                prims.push(Prim::Text {
                    x: left + pad,
                    y,
                    text: text.clone(),
                    size,
                    bold: *bold,
                    rgb,
                });
            }
            Shape::Unsupported => {}
        }
    }
    let _ = height;
    prims
}

/// The picture `canvas.export` writes, and how it came out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rendered {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

/// The scales a picture is tried at, until its PNG fits: kit-annotate's.
const SCALES: [f32; 5] = [1.0, 0.75, 0.5, 0.35, 0.25];

/// Draws a canvas as a PNG of at most `max_bytes`: its background, given as
/// the bytes of a raster image, stretched to it, and its marks; white where
/// there is no background. The largest of [`SCALES`] that fits wins.
pub(crate) fn render_png(
    width: f32,
    height: f32,
    background: Option<&[u8]>,
    marks: &[Mark],
    stroke: f32,
    max_bytes: usize,
) -> Result<Rendered, String> {
    let picture = match background {
        Some(bytes) => Some(
            image::load_from_memory(bytes)
                .map_err(|why| format!("the background is not an image Hangar reads: {why}"))?
                .to_rgba8(),
        ),
        None => None,
    };
    let picture = match picture {
        Some(rgba) => {
            let (w, h) = rgba.dimensions();
            let mut data = rgba.into_raw();
            premultiply(&mut data);
            Some(
                Pixmap::from_vec(
                    data,
                    tiny_skia::IntSize::from_wh(w, h).ok_or("the background is empty")?,
                )
                .ok_or("the background could not be drawn")?,
            )
        }
        None => None,
    };
    let prims = layout(marks, width, height, stroke);
    for scale in SCALES {
        let (w, h) = (
            (width * scale).round().max(1.0) as u32,
            (height * scale).round().max(1.0) as u32,
        );
        let mut pixmap =
            Pixmap::new(w, h).ok_or_else(|| format!("a picture of {w} by {h} is too large"))?;
        pixmap.fill(tiny_skia::Color::WHITE);
        if let Some(picture) = &picture {
            let transform = Transform::from_scale(
                w as f32 / picture.width() as f32,
                h as f32 / picture.height() as f32,
            );
            pixmap.draw_pixmap(
                0,
                0,
                picture.as_ref(),
                &PixmapPaint {
                    quality: FilterQuality::Bilinear,
                    ..PixmapPaint::default()
                },
                transform,
                None,
            );
        }
        draw(&mut pixmap, &prims, scale);
        let png = encode(&pixmap)?;
        if png.len() <= max_bytes {
            return Ok(Rendered {
                png,
                width: w,
                height: h,
                scale,
            });
        }
    }
    Err(format!(
        "the picture is larger than {} MB even at a quarter of its size",
        max_bytes / (1024 * 1024)
    ))
}

fn premultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = pixel[3] as u16;
        if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((*channel as u16 * alpha + 127) / 255) as u8;
            }
        }
    }
}

fn paint(rgb: [u8; 3]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 255);
    paint.anti_alias = true;
    paint
}

fn round_stroke(width: f32) -> Stroke {
    Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    }
}

fn draw(pixmap: &mut Pixmap, prims: &[Prim], scale: f32) {
    let transform = Transform::from_scale(scale, scale);
    for prim in prims {
        match prim {
            Prim::Polyline { points, width, rgb } => {
                let mut path = PathBuilder::new();
                path.move_to(points[0].x, points[0].y);
                for point in &points[1..] {
                    path.line_to(point.x, point.y);
                }
                if let Some(path) = path.finish() {
                    pixmap.stroke_path(&path, &paint(*rgb), &round_stroke(*width), transform, None);
                }
            }
            Prim::Rect {
                x,
                y,
                w,
                h,
                stroke,
                fill,
            } => {
                let (x, w) = if *w < 0.0 { (x + w, -w) } else { (*x, *w) };
                let (y, h) = if *h < 0.0 { (y + h, -h) } else { (*y, *h) };
                let Some(rect) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) else {
                    continue;
                };
                let path = PathBuilder::from_rect(rect);
                if let Some(fill) = fill {
                    pixmap.fill_path(&path, &paint(*fill), FillRule::Winding, transform, None);
                }
                if let Some((width, rgb)) = stroke {
                    let mut stroke = round_stroke(*width);
                    stroke.line_cap = LineCap::Square;
                    stroke.line_join = LineJoin::Miter;
                    pixmap.stroke_path(&path, &paint(*rgb), &stroke, transform, None);
                }
            }
            Prim::Ellipse {
                cx,
                cy,
                rx,
                ry,
                stroke,
                fill,
            } => {
                let Some(oval) =
                    Rect::from_xywh(cx - rx, cy - ry, (2.0 * rx).max(0.01), (2.0 * ry).max(0.01))
                else {
                    continue;
                };
                let Some(path) = PathBuilder::from_oval(oval) else {
                    continue;
                };
                if let Some(fill) = fill {
                    pixmap.fill_path(&path, &paint(*fill), FillRule::Winding, transform, None);
                }
                if let Some((width, rgb)) = stroke {
                    pixmap.stroke_path(&path, &paint(*rgb), &round_stroke(*width), transform, None);
                }
            }
            Prim::Text {
                x,
                y,
                text,
                size,
                bold,
                rgb,
            } => draw_text(
                pixmap,
                *x * scale,
                *y * scale,
                text,
                size * scale,
                *bold,
                *rgb,
            ),
        }
    }
}

/// A line of text, its left end at `x` and its middle at `y`, in pixels:
/// each glyph's outline filled by its coverage, over what is there.
fn draw_text(pixmap: &mut Pixmap, x: f32, y: f32, text: &str, size: f32, bold: bool, rgb: [u8; 3]) {
    let font = font(bold);
    let scale = em(font, size);
    let scaled = font.as_scaled(scale);
    // The middle of the line is halfway between the ascent and the descent,
    // as a canvas's `textBaseline = "middle"` puts it.
    let baseline = y + (scaled.ascent() + scaled.descent()) / 2.0;
    let (glyphs, _) = shape_line(text, size, bold);
    let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
    let pixels = pixmap.pixels_mut();
    for (id, caret) in glyphs {
        let glyph = id.with_scale_and_position(scale, point(x + caret, baseline));
        let Some(outline) = font.outline_glyph(glyph) else {
            continue;
        };
        let bounds = outline.px_bounds();
        outline.draw(|gx, gy, coverage| {
            let (px, py) = (
                bounds.min.x as i32 + gx as i32,
                bounds.min.y as i32 + gy as i32,
            );
            if px < 0 || py < 0 || px >= width || py >= height || coverage <= 0.0 {
                return;
            }
            let at = (py * width + px) as usize;
            let under = pixels[at];
            let blend = |over: u8, under: u8| {
                (over as f32 * coverage + under as f32 * (1.0 - coverage)).round() as u8
            };
            // The picture is opaque (white under a background), so its
            // premultiplied channels are its colours.
            if let Some(blended) = tiny_skia::PremultipliedColorU8::from_rgba(
                blend(rgb[0], under.red()),
                blend(rgb[1], under.green()),
                blend(rgb[2], under.blue()),
                255,
            ) {
                pixels[at] = blended;
            }
        });
    }
}

/// The picture drawn, as a PNG: it is opaque (white under a background),
/// so its premultiplied pixels are its colours.
fn encode(pixmap: &Pixmap) -> Result<Vec<u8>, String> {
    encode_rgba(pixmap.width(), pixmap.height(), pixmap.data())
}

/// Pixels of four bytes, not premultiplied, as a PNG: the clipboard's
/// picture (`ui.pasteImage`), and a canvas drawn.
pub(crate) fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
        return Err("the picture has no pixels, or not as many as its size says".into());
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|why| format!("the picture could not be written: {why}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|why| format!("the picture could not be written: {why}"))?;
    }
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_turns_to_its_fallback_at_the_edge() {
        let label = |x: f32| {
            Mark::text((x, 50.0), "Login button", 20.0)
                .fill(Ink::Red)
                .ink(Ink::White)
                .fallback((x - 40.0, 50.0))
        };
        let width = measure("Login button", 20.0, false) + 2.0 * 20.0 * TEXT_PAD;
        let boxes = |x: f32| -> Vec<f32> {
            layout(&[label(x)], 400.0, 200.0, 4.0)
                .into_iter()
                .filter_map(|prim| match prim {
                    Prim::Rect { x, .. } => Some(x),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(boxes(100.0), vec![100.0], "it fits where it was put");
        let turned = boxes(390.0)[0];
        assert!(
            (turned - (350.0 - width)).abs() < 0.01,
            "it ends at its fallback: {turned}"
        );
        assert!(width > 60.0 && width < 200.0, "{width}");
    }

    #[test]
    fn an_arrow_has_a_head_of_two_strokes() {
        let prims = layout(
            &[Mark::arrow((0.0, 0.0), (100.0, 0.0)).ink(Ink::Blue)],
            200.0,
            200.0,
            4.0,
        );
        assert_eq!(prims.len(), 3);
        let Prim::Polyline { points, width, .. } = &prims[1] else {
            panic!("{prims:?}");
        };
        assert_eq!(*width, 4.0);
        // 20 units back from the tip (five lines), half a radian off.
        assert!((points[1].x - (100.0 - 20.0 * 0.5f32.cos())).abs() < 0.001);
    }

    #[test]
    fn a_picture_is_written_at_its_size_or_smaller_to_fit() {
        let marks = [
            Mark::rect(10.0, 10.0, 60.0, 30.0).ink(Ink::Green),
            Mark::circle((100.0, 50.0), 12.0).fill(Ink::Red),
            Mark::text((100.0, 50.0), "1", 15.0)
                .bold()
                .align(Align::Center)
                .ink(Ink::White),
        ];
        let at_size = render_png(200.0, 100.0, None, &marks, 4.0, 10 << 20).unwrap();
        assert_eq!(
            (at_size.width, at_size.height, at_size.scale),
            (200, 100, 1.0)
        );
        let decoded = image::load_from_memory(&at_size.png).unwrap().to_rgba8();
        assert_eq!(
            decoded.get_pixel(5, 90).0,
            [255, 255, 255, 255],
            "white under no background"
        );
        assert_eq!(&decoded.get_pixel(40, 10).0[..3], &Ink::Green.rgb());
        // On a background, and in a budget only a smaller picture fits.
        let background = Some(at_size.png.as_slice());
        let whole = render_png(200.0, 100.0, background, &marks, 4.0, 10 << 20).unwrap();
        assert_eq!(whole.scale, 1.0);
        let smaller =
            render_png(200.0, 100.0, background, &marks, 4.0, whole.png.len() - 1).unwrap();
        assert!(smaller.scale < 1.0);
        assert_eq!(smaller.width, (200.0 * smaller.scale).round() as u32);
        assert!(render_png(200.0, 100.0, None, &marks, 4.0, 10).is_err());
    }
}
