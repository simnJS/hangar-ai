//! What a picture file is, and what a viewer is given to draw for it.
//!
//! Every picture is decoded here, off the UI thread, to pixels: iced would
//! otherwise decode a file handed to it as bytes inside `layout`, on the UI
//! thread, and a 50 MB PNG would freeze the window for seconds.
//!
//! - **An animated GIF or WebP.** Its frames are decoded here, composed onto
//!   the whole canvas, each with the time it stays (`Animation`); the stage
//!   plays them (`image_stage.rs`). The webview did this by itself.
//! - **An AVIF.** iced cannot decode it; `avif.rs` does, to pixels.
//! - **Anything else** (PNG, JPEG, BMP, a still GIF or WebP, the first frame
//!   of an animation too large to play): its header is read first, a picture
//!   of more than [`MAX_PIXELS`] is refused, and the rest decoded to RGBA.
//!
//! Only an SVG is handed on as its source (`Loaded::Encoded`), for the stage
//! to rasterise.
//!
//! A still picture larger than [`SHOWN_EDGE`] on a side is reduced to it
//! once decoded ([`Size::Shown`]): a pane shows an 8000 × 6000 photo a few
//! hundred pixels wide, and the whole of it was 192 MB of pixels held on the
//! heap and in the GPU's atlas, its first frame holding the window for two
//! seconds while it was uploaded. The viewer asks for every pixel
//! ([`Size::Full`]) only once it is zoomed past what the reduced copy holds.
//!
//! A file is told by its first bytes, not by its name: a `.png` that is a GIF
//! plays as one.

use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ::image::{AnimationDecoder, ImageDecoder};
use iced::widget::image::Handle;

use super::avif;

/// The bytes of a picture, read off the UI thread.
#[derive(Debug, Clone)]
pub struct Bytes(pub Arc<Vec<u8>>);

/// The largest picture file the viewer reads (`Kind::Image` in
/// `hangar-core/src/preview.rs`, which checks it when a file is shown; it is
/// checked again here because the file may have been rewritten since).
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// The most memory the decoded frames of one animation may take. An animation
/// that would take more is shown as its first frame, which is what was shown
/// before frames were played at all.
pub const FRAME_BUDGET: usize = 256 * 1024 * 1024;

/// A frame that asks to be shown for less than this is shown for
/// [`FALLBACK`] instead: the delay of 0 or 1 hundredth of a second that
/// encoders write for "as fast as possible", which browsers also play at
/// 10 frames a second.
const SHORTEST: Duration = Duration::from_millis(20);
const FALLBACK: Duration = Duration::from_millis(100);

/// More pixels than this are refused before anything is decoded: the output
/// is four bytes each (the same cap as an AVIF's, and a Markdown picture's).
pub const MAX_PIXELS: u64 = avif::MAX_PIXELS;

/// The longest side a still picture is decoded to for the viewer's first
/// look, and for a picture in the flow of a Markdown page: as wide as a 4K
/// screen, so that a picture fitted to any pane is drawn from enough pixels.
pub const SHOWN_EDGE: u32 = 4096;

/// How many of a still picture's pixels are wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// At most [`SHOWN_EDGE`] on its longest side.
    Shown,
    /// Every pixel of it.
    Full,
}

/// What the viewer is given for a picture file.
#[derive(Debug, Clone)]
pub enum Loaded {
    /// The file as it is: an SVG's source, for the stage to rasterise.
    Encoded(Bytes),
    /// A picture decoded here, as pixels, and the size the picture itself
    /// has: the handle holds fewer pixels when it was reduced
    /// ([`Size::Shown`]).
    Pixels(Handle, u32, u32),
    /// A picture of several frames.
    Animated(Arc<Animation>),
}

/// One frame of an animation.
#[derive(Debug, Clone)]
pub struct Frame {
    pub handle: Handle,
    pub delay: Duration,
}

/// The frames of an animated picture, each as large as the canvas, and the
/// clock that tells which one is showing when.
#[derive(Debug)]
pub struct Animation {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Frame>,
    pub timeline: Timeline,
}

/// Two animations are the same when they are the same decoding: every
/// handle is new for each one.
impl PartialEq for Animation {
    fn eq(&self, other: &Self) -> bool {
        self.frames.first().map(|frame| frame.handle.id())
            == other.frames.first().map(|frame| frame.handle.id())
    }
}

impl Eq for Animation {}

/// When each frame of an animation ends, from its start. The animation loops
/// for as long as it is looked at (a GIF's own loop count is not read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    ends: Vec<Duration>,
}

impl Timeline {
    pub fn new(delays: impl IntoIterator<Item = Duration>) -> Timeline {
        let mut total = Duration::ZERO;
        let ends = delays
            .into_iter()
            .map(|delay| {
                total += if delay < SHORTEST { FALLBACK } else { delay };
                total
            })
            .collect();
        Timeline { ends }
    }

    /// The time one round takes.
    pub fn total(&self) -> Duration {
        self.ends.last().copied().unwrap_or(Duration::ZERO)
    }

    /// The frame showing `elapsed` after the start, and how long it has left
    /// to show.
    pub fn at(&self, elapsed: Duration) -> (usize, Duration) {
        let total = self.total().as_nanos();
        if total == 0 {
            return (0, Duration::MAX);
        }
        let into = Duration::from_nanos((elapsed.as_nanos() % total) as u64);
        let frame = self
            .ends
            .partition_point(|end| *end <= into)
            .min(self.ends.len() - 1);
        (frame, self.ends[frame] - into)
    }
}

/// Where an animation is on the clock of the redraws, kept by whichever
/// widget draws it. It starts when the widget is first drawn and runs for as
/// long as the widget is in the window: a widget that is not drawn is not
/// told the time, so an animation out of sight does not run.
#[derive(Debug, Default)]
pub struct Clock {
    started: Option<Instant>,
    /// The frame showing.
    pub frame: usize,
}

impl Clock {
    /// Begins again, for another animation.
    pub fn reset(&mut self) {
        *self = Clock::default();
    }

    /// Moves to the frame `now` has reached, and says when to be drawn next:
    /// when that frame ends.
    pub fn tick(&mut self, now: Instant, timeline: &Timeline) -> Instant {
        let start = *self.started.get_or_insert(now);
        let (frame, left) = timeline.at(now.saturating_duration_since(start));
        self.frame = frame;
        // An hour is as good as never for a frame, and keeps the sum from
        // overflowing the clock.
        now + left.min(Duration::from_secs(3600))
    }
}

/// Reads a picture file whole, up to [`MAX_BYTES`].
pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let failed = |err: std::io::Error| format!("{}: {err}", path.display());
    let file = std::fs::File::open(path).map_err(failed)?;
    let size = file.metadata().map_err(failed)?.len();
    if size > MAX_BYTES {
        return Err(format!(
            "{}: larger than the {} MB the preview reads",
            path.display(),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    // One byte more than the cap, so that a file that grew meanwhile is seen.
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!(
            "{}: larger than the {} MB the preview reads",
            path.display(),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

/// What the viewer is given for a picture file's bytes. An SVG is not told
/// here: the stage rasterises it from its source.
///
/// The decoders read files an agent or a repository wrote; one that panics on
/// a damaged file (a parser's assertion) must not take the task with it, so a
/// panic is a failed picture like any other.
pub fn load(bytes: Vec<u8>, size: Size) -> Result<Loaded, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        match decode(&bytes, size)? {
            Some(loaded) => Ok(loaded),
            // A picture of one frame, or an animation that would take too
            // much memory: its first frame.
            None => still(&bytes, size),
        }
    }))
    .unwrap_or_else(|_| Err("the file is damaged".to_string()))
}

/// Whether the handle of a decoded picture holds fewer pixels than the
/// picture has (`image::detail_of` says by how much).
#[cfg(test)]
pub fn reduced(loaded: &Loaded) -> bool {
    match loaded {
        Loaded::Pixels(Handle::Rgba { width, .. }, natural, _) => width < natural,
        _ => false,
    }
}

/// By how much a picture of `width` × `height` is reduced on each side: 1
/// for none, else the least whole number that brings its longest side to
/// [`SHOWN_EDGE`] or less.
fn factor(width: u32, height: u32, size: Size) -> u32 {
    let longest = width.max(height);
    if size == Size::Full || longest <= SHOWN_EDGE {
        return 1;
    }
    longest.div_ceil(SHOWN_EDGE)
}

/// The size a picture of `width` × `height` is decoded to.
#[cfg(test)]
fn shown_size(width: u32, height: u32, size: Size) -> (u32, u32) {
    let k = factor(width, height, size);
    (width.div_ceil(k), height.div_ceil(k))
}

/// `pixels` of `channels` bytes each (grey, grey and alpha, RGB or RGBA),
/// `width` wide, as RGBA reduced `k` times on each side: each pixel the mean
/// of the block of `k` × `k` it stands for (fewer at the right and bottom
/// edges). A whole factor and a box filter, on every core: a third of a
/// second less than a resampling filter for a 48-megapixel photo, and as
/// good when shrinking.
fn reduce(pixels: &[u8], width: u32, height: u32, channels: usize, k: u32) -> (Vec<u8>, u32, u32) {
    let (w, h) = (width.div_ceil(k), height.div_ceil(k));
    let (width, k_us) = (width as usize, k as usize);
    let row_bytes = w as usize * 4;
    let mut out = vec![0u8; row_bytes * h as usize];
    let threads = std::thread::available_parallelism()
        .map_or(4, usize::from)
        .clamp(1, 16);
    let rows_each = (h as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (band, rows) in out.chunks_mut(rows_each * row_bytes).enumerate() {
            scope.spawn(move || {
                let mut sums = vec![0u32; w as usize * 4];
                let mut counts = vec![0u32; w as usize];
                for (r, row) in rows.chunks_mut(row_bytes).enumerate() {
                    let y0 = (band * rows_each + r) * k_us;
                    let y1 = (y0 + k_us).min(height as usize);
                    sums.fill(0);
                    counts.fill(0);
                    for y in y0..y1 {
                        let line = &pixels[y * width * channels..(y + 1) * width * channels];
                        for (x, px) in line.chunks_exact(channels).enumerate() {
                            let ox = x / k_us;
                            let rgba = match channels {
                                1 => [px[0], px[0], px[0], 255],
                                2 => [px[0], px[0], px[0], px[1]],
                                3 => [px[0], px[1], px[2], 255],
                                _ => [px[0], px[1], px[2], px[3]],
                            };
                            for (sum, value) in sums[ox * 4..ox * 4 + 4].iter_mut().zip(rgba) {
                                *sum += u32::from(value);
                            }
                            counts[ox] += 1;
                        }
                    }
                    for (ox, count) in counts.iter().enumerate() {
                        let count = (*count).max(1);
                        for c in 0..4 {
                            row[ox * 4 + c] = ((sums[ox * 4 + c] + count / 2) / count) as u8;
                        }
                    }
                }
            });
        }
    });
    (out, w, h)
}

/// A decoded picture as the viewer's pixels, reduced `k` times: the handle,
/// and the picture's own size.
fn pixels(decoded: ::image::DynamicImage, size: Size) -> Loaded {
    use ::image::DynamicImage as D;
    let (width, height) = (decoded.width(), decoded.height());
    let k = factor(width, height, size);
    if k == 1 {
        let rgba = decoded.into_rgba8();
        return Loaded::Pixels(
            Handle::from_rgba(width, height, rgba.into_raw()),
            width,
            height,
        );
    }
    // Reduced from the decoder's own layout: an RGB photo is not first
    // copied whole at four bytes a pixel.
    let (rgba, w, h) = match &decoded {
        D::ImageLuma8(image) => reduce(image.as_raw(), width, height, 1, k),
        D::ImageLumaA8(image) => reduce(image.as_raw(), width, height, 2, k),
        D::ImageRgb8(image) => reduce(image.as_raw(), width, height, 3, k),
        D::ImageRgba8(image) => reduce(image.as_raw(), width, height, 4, k),
        // Sixteen bits or floats: brought to eight first.
        other => reduce(other.to_rgba8().as_raw(), width, height, 4, k),
    };
    Loaded::Pixels(Handle::from_rgba(w, h, rgba), width, height)
}

/// A picture of one frame, decoded to pixels after its size is checked.
fn still(bytes: &[u8], size: Size) -> Result<Loaded, String> {
    let reader = ::image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| err.to_string())?;
    if reader.format().is_none() {
        return Err("not a picture the preview can read".into());
    }
    // The reader's own limits stay (512 MiB a decoding): a file that claims
    // a small size and asks for more is refused there.
    let decoder = reader.into_decoder().map_err(|err| err.to_string())?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "{width} × {height} pixels, more than the {} million the preview decodes",
            MAX_PIXELS / 1_000_000
        ));
    }
    let decoded = ::image::DynamicImage::from_decoder(decoder).map_err(|err| err.to_string())?;
    Ok(pixels(decoded, size))
}

/// An AVIF or an animation, or `None` for a picture of one frame.
fn decode(bytes: &[u8], size: Size) -> Result<Option<Loaded>, String> {
    if avif::is_avif(&bytes[..bytes.len().min(512)]) {
        let decoded = avif::decode(bytes)?;
        let rgba = ::image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
            .ok_or("the decoder gave fewer pixels than the picture has")?;
        return Ok(Some(pixels(::image::DynamicImage::ImageRgba8(rgba), size)));
    }
    let animation = if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let decoder = ::image::codecs::gif::GifDecoder::new(Cursor::new(bytes));
        // A GIF the decoder cannot start on is read as a still picture,
        // which says what is wrong with it.
        match decoder {
            Ok(decoder) => {
                let size = decoder.dimensions();
                animate(decoder, size).ok().flatten()
            }
            Err(_) => None,
        }
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        match ::image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)) {
            Ok(decoder) if decoder.has_animation() => {
                let size = decoder.dimensions();
                animate(decoder, size).ok().flatten()
            }
            _ => None,
        }
    } else {
        None
    };
    Ok(animation.map(|animation| Loaded::Animated(Arc::new(animation))))
}

/// The frames of an animation, or `None` when it has fewer than two or would
/// take more than [`FRAME_BUDGET`]. A file that breaks after some frames
/// keeps the ones before: they are what a browser would play.
fn animate<'a>(
    decoder: impl AnimationDecoder<'a>,
    (width, height): (u32, u32),
) -> Result<Option<Animation>, String> {
    let frame_bytes = width as usize * height as usize * 4;
    if frame_bytes == 0 || frame_bytes > FRAME_BUDGET / 2 {
        return Ok(None);
    }
    let mut frames = Vec::new();
    let mut left = FRAME_BUDGET;
    for frame in decoder.into_frames() {
        let frame = match frame {
            Ok(frame) => frame,
            Err(_) if frames.len() >= 2 => break,
            Err(err) => return Err(err.to_string()),
        };
        if frame_bytes > left {
            return Ok(None);
        }
        left -= frame_bytes;
        let delay = Duration::from(frame.delay());
        let buffer = frame.into_buffer();
        // Frames come composed onto the canvas; one of another size would be
        // drawn at the wrong place, so it is not played.
        if buffer.dimensions() != (width, height) {
            return Ok(None);
        }
        frames.push(Frame {
            handle: Handle::from_rgba(width, height, buffer.into_raw()),
            delay,
        });
    }
    if frames.len() < 2 {
        return Ok(None);
    }
    let timeline = Timeline::new(frames.iter().map(|frame| frame.delay));
    Ok(Some(Animation {
        width,
        height,
        frames,
        timeline,
    }))
}

#[cfg(test)]
mod tests;
