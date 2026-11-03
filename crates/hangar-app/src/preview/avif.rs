//! AVIF, which the old app's webview drew and iced's `image` feature cannot:
//! it has no AV1 decoder. The container is read by `avif-parse` and the
//! pictures by `re_rav1d`, a Rust port of dav1d, built without its assembly so
//! that the app needs no `nasm`; a still picture takes a moment longer for it.
//!
//! What this reads: the primary image, in 8, 10 or 12 bits, in every chroma
//! layout, with an alpha plane when the file has one. What it leaves out: the
//! frames of an animated AVIF (the primary image is shown), a rotation or a
//! mirror the container asks for (`irot`, `imir`: `avif-parse` does not
//! report them), and HDR (the samples are shown as sRGB, not tone-mapped).
//! None of the three matters much for the files agents write, which are
//! screenshots and renders.

use re_rav1d::dav1d::pixel::{MatrixCoefficients, YUVRange};
use re_rav1d::dav1d::{Decoder, Error as Av1Error, PixelLayout, PlanarImageComponent, Settings};

/// More pixels than this are refused before anything is decoded: the output
/// is four bytes each.
pub const MAX_PIXELS: u64 = 120_000_000;

/// A picture as bytes of RGBA, row after row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Whether these are the first bytes of an AVIF file: an `ftyp` box whose
/// major or compatible brands name AVIF (`avif`, or `avis` for a sequence).
pub fn is_avif(head: &[u8]) -> bool {
    if head.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let size = head.get(0..4).map_or(0, |size| {
        u32::from_be_bytes([size[0], size[1], size[2], size[3]]) as usize
    });
    // A box that says it is longer than what was given is read as far as it
    // goes. After the box's header come the major brand and the minor version
    // (four bytes each), then the compatible brands.
    let end = if size >= 16 {
        size.min(head.len())
    } else {
        head.len()
    };
    let brands = head.get(8..end).unwrap_or_default();
    let is_brand = |brand: &[u8]| brand == b"avif" || brand == b"avis";
    brands.get(..4).is_some_and(is_brand)
        || brands
            .get(8..)
            .unwrap_or_default()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|brand| is_brand(brand))
}

/// A file decoded to RGBA.
///
/// `avif-parse` asserts on some damaged files instead of failing, so a panic
/// in the reading is a damaged file here.
pub fn decode(file: &[u8]) -> Result<Pixels, String> {
    std::panic::catch_unwind(|| read(file))
        .unwrap_or_else(|_| Err("AVIF: the file is damaged".to_string()))
}

fn read(file: &[u8]) -> Result<Pixels, String> {
    let avif = avif_parse::read_avif(&mut &*file).map_err(|err| format!("AVIF: {err}"))?;
    let meta = avif
        .primary_item_metadata()
        .map_err(|err| format!("AVIF: {err}"))?;
    let (width, height) = (meta.max_frame_width.get(), meta.max_frame_height.get());
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "AVIF: {width}×{height} pixels is too large to open"
        ));
    }
    let colour = decode_av1(avif.primary_item.to_vec())?;
    let mut pixels = with_yuv(&colour, |yuv| yuv.to_rgba())?;
    if let Some(alpha) = &avif.alpha_item {
        let plane = decode_av1(alpha.to_vec())?;
        if (plane.width(), plane.height()) != (pixels.width, pixels.height) {
            return Err("AVIF: the alpha plane is not the size of the picture".into());
        }
        let opacity = with_yuv(&plane, |yuv| yuv.opacity())?;
        apply_alpha(&mut pixels, &opacity, avif.premultiplied_alpha);
    }
    Ok(pixels)
}

/// One AV1 picture, the last one the data makes: a still item has one.
fn decode_av1(data: Vec<u8>) -> Result<re_rav1d::dav1d::Picture, String> {
    let mut settings = Settings::new();
    // A few threads decode tiles side by side; one frame in flight, so that
    // the picture is out when the data is in.
    settings
        .set_n_threads(std::thread::available_parallelism().map_or(2, |n| n.get().min(4)) as u32);
    settings.set_max_frame_delay(1);
    settings.set_apply_grain(true);
    settings.set_frame_size_limit(MAX_PIXELS as u32);
    let mut decoder = Decoder::with_settings(&settings).map_err(|err| format!("AV1: {err}"))?;
    let mut sent = decoder.send_data(data, None, None, None);
    let mut last = None;
    // Bounded: a decoder that keeps asking for more would never end.
    for _ in 0..64 {
        loop {
            match decoder.get_picture() {
                Ok(picture) => last = Some(picture),
                Err(Av1Error::Again) => break,
                Err(err) => return Err(format!("AV1: {err}")),
            }
        }
        match sent {
            Ok(()) => break,
            Err(Av1Error::Again) => sent = decoder.send_pending_data(),
            Err(err) => return last.ok_or_else(|| format!("AV1: {err}")),
        }
    }
    last.ok_or_else(|| "AV1: the file holds no picture".to_string())
}

/// One plane of samples: bytes, or little-endian pairs of them for pictures
/// of more than 8 bits.
struct Plane<'a> {
    bytes: &'a [u8],
    /// Samples (not bytes) from one row to the next.
    stride: usize,
    width: usize,
    height: usize,
    wide: bool,
}

impl<'a> Plane<'a> {
    /// A plane over `bytes`, or why they cannot be one: a decoder that hands
    /// over less than its own geometry says is not trusted with an index.
    fn new(
        bytes: &'a [u8],
        stride_bytes: usize,
        width: usize,
        height: usize,
        wide: bool,
    ) -> Result<Plane<'a>, String> {
        let size = if wide { 2 } else { 1 };
        let stride = stride_bytes / size;
        if width == 0 || height == 0 || stride < width || bytes.len() < stride * height * size {
            return Err("AV1: a plane is smaller than the picture".into());
        }
        Ok(Plane {
            bytes,
            stride,
            width,
            height,
            wide,
        })
    }

    /// The sample at a position, the nearest one when it is past an edge.
    fn at(&self, x: usize, y: usize) -> f32 {
        let (x, y) = (x.min(self.width - 1), y.min(self.height - 1));
        let at = y * self.stride + x;
        if self.wide {
            f32::from(u16::from_le_bytes([
                self.bytes[at * 2],
                self.bytes[at * 2 + 1],
            ]))
        } else {
            f32::from(self.bytes[at])
        }
    }

    /// The plane read at a fractional position, between its four nearest
    /// samples: how a subsampled chroma plane is stretched to the picture.
    fn linear(&self, x: f32, y: f32) -> f32 {
        let (x, y) = (x.max(0.0), y.max(0.0));
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as usize, y0 as usize);
        let top = self.at(x0, y0) * (1.0 - fx) + self.at(x0 + 1, y0) * fx;
        let bottom = self.at(x0, y0 + 1) * (1.0 - fx) + self.at(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// How the chroma planes are smaller than the luma plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sampling {
    /// No chroma planes: a grey picture, or an alpha plane.
    None,
    /// Half as wide and half as high.
    Half,
    /// Half as wide.
    Wide,
    /// As large as the luma.
    Full,
}

/// A decoded picture apart from the decoder that made it: its planes and how
/// to read them.
struct Yuv<'a> {
    width: usize,
    height: usize,
    sampling: Sampling,
    /// The bits a sample uses (8, 10 or 12), which its range is scaled by.
    bits: u32,
    /// Whether the samples span the whole range, or 16 to 235 (luma) and 16
    /// to 240 (chroma) as video does.
    full: bool,
    /// The luma weights of the colour matrix, `None` for the identity matrix.
    matrix: Option<(f32, f32)>,
    luma: Plane<'a>,
    chroma: Option<(Plane<'a>, Plane<'a>)>,
}

/// The luma weights of a colour matrix: (kr, kb). `None` for the identity
/// matrix, where the planes are already green, blue and red.
fn weights(matrix: MatrixCoefficients) -> Option<(f32, f32)> {
    Some(match matrix {
        MatrixCoefficients::Identity => return None,
        MatrixCoefficients::BT709 => (0.2126, 0.0722),
        MatrixCoefficients::ST240M => (0.212, 0.087),
        MatrixCoefficients::BT2020NonConstantLuminance
        | MatrixCoefficients::BT2020ConstantLuminance => (0.2627, 0.0593),
        // BT.601, which is also what an unspecified matrix means for the
        // encoders that write none.
        _ => (0.299, 0.114),
    })
}

/// Reads a decoded picture as `Yuv` and hands it to `use_it`; the planes
/// belong to the picture, so they cannot outlive this call.
fn with_yuv<R>(
    picture: &re_rav1d::dav1d::Picture,
    use_it: impl FnOnce(&Yuv<'_>) -> R,
) -> Result<R, String> {
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let wide = picture.bit_depth() > 8;
    let bits = picture.bits_per_component().map_or(8, |bits| bits.0 as u32);
    let sampling = match picture.pixel_layout() {
        PixelLayout::I400 => Sampling::None,
        PixelLayout::I420 => Sampling::Half,
        PixelLayout::I422 => Sampling::Wide,
        PixelLayout::I444 => Sampling::Full,
    };
    let (chroma_width, chroma_height) = match sampling {
        Sampling::Half => (width.div_ceil(2), height.div_ceil(2)),
        Sampling::Wide => (width.div_ceil(2), height),
        Sampling::None | Sampling::Full => (width, height),
    };
    let luma_bytes = picture.plane(PlanarImageComponent::Y);
    let luma = Plane::new(
        &luma_bytes,
        picture.stride(PlanarImageComponent::Y) as usize,
        width,
        height,
        wide,
    )?;
    let chroma_bytes = (sampling != Sampling::None).then(|| {
        (
            picture.plane(PlanarImageComponent::U),
            picture.plane(PlanarImageComponent::V),
        )
    });
    let chroma = match &chroma_bytes {
        Some((u, v)) => {
            let stride = picture.stride(PlanarImageComponent::U) as usize;
            Some((
                Plane::new(u, stride, chroma_width, chroma_height, wide)?,
                Plane::new(v, stride, chroma_width, chroma_height, wide)?,
            ))
        }
        None => None,
    };
    Ok(use_it(&Yuv {
        width,
        height,
        sampling,
        bits,
        full: matches!(picture.color_range(), YUVRange::Full),
        matrix: weights(picture.matrix_coefficients()),
        luma,
        chroma,
    }))
}

impl Yuv<'_> {
    /// A luma sample as 0 to 1 (also how an alpha plane and the planes of an
    /// identity matrix are read).
    fn unit(&self, sample: f32) -> f32 {
        let scale = (1u32 << (self.bits - 8)) as f32;
        if self.full {
            sample / ((1u32 << self.bits) - 1) as f32
        } else {
            (sample - 16.0 * scale) / (219.0 * scale)
        }
    }

    /// A chroma sample as -0.5 to 0.5.
    fn centred(&self, sample: f32) -> f32 {
        let scale = (1u32 << (self.bits - 8)) as f32;
        let middle = 128.0 * scale;
        if self.full {
            (sample - middle) / ((1u32 << self.bits) - 1) as f32
        } else {
            (sample - middle) / (224.0 * scale)
        }
    }

    /// The picture as RGBA, every pixel opaque.
    fn to_rgba(&self) -> Pixels {
        let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let mut rgba = vec![255u8; self.width * self.height * 4];
        for y in 0..self.height {
            for x in 0..self.width {
                let l = self.unit(self.luma.at(x, y));
                let (r, g, b) = match &self.chroma {
                    None => (l, l, l),
                    Some((u, v)) => {
                        // Where the chroma sample of a pixel is, in the plane
                        // that was made smaller: centred between the pixels
                        // it stands for.
                        let (cx, cy) = match self.sampling {
                            Sampling::Half => {
                                ((x as f32 + 0.5) / 2.0 - 0.5, (y as f32 + 0.5) / 2.0 - 0.5)
                            }
                            Sampling::Wide => ((x as f32 + 0.5) / 2.0 - 0.5, y as f32),
                            Sampling::None | Sampling::Full => (x as f32, y as f32),
                        };
                        match self.matrix {
                            // The planes are green, blue and red, each read
                            // as a luma plane is.
                            None => (self.unit(v.linear(cx, cy)), l, self.unit(u.linear(cx, cy))),
                            Some((kr, kb)) => {
                                let cb = self.centred(u.linear(cx, cy));
                                let cr = self.centred(v.linear(cx, cy));
                                let r = l + 2.0 * (1.0 - kr) * cr;
                                let b = l + 2.0 * (1.0 - kb) * cb;
                                let g = (l - kr * r - kb * b) / (1.0 - kr - kb);
                                (r, g, b)
                            }
                        }
                    }
                };
                let at = (y * self.width + x) * 4;
                rgba[at] = byte(r);
                rgba[at + 1] = byte(g);
                rgba[at + 2] = byte(b);
            }
        }
        Pixels {
            width: self.width as u32,
            height: self.height as u32,
            rgba,
        }
    }

    /// The luma plane as opacity, 0 to 255 for each pixel.
    fn opacity(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                let opacity = self.unit(self.luma.at(x, y)).clamp(0.0, 1.0);
                out.push((opacity * 255.0 + 0.5) as u8);
            }
        }
        out
    }
}

/// Writes opacity into the fourth byte of each pixel, taking it out of the
/// colours first when the file says they were multiplied by it.
fn apply_alpha(pixels: &mut Pixels, opacity: &[u8], premultiplied: bool) {
    for (pixel, alpha) in pixels.rgba.as_chunks_mut::<4>().0.iter_mut().zip(opacity) {
        if premultiplied && *alpha > 0 {
            for channel in &mut pixel[..3] {
                *channel = (u32::from(*channel) * 255 / u32::from(*alpha)).min(255) as u8;
            }
        }
        pixel[3] = *alpha;
    }
}

#[cfg(test)]
mod tests;
