//! What a picture file is, and what a viewer is given to draw for it.
//!
//! iced draws a still picture from the bytes of a file it can decode (PNG,
//! JPEG, BMP, a GIF or WebP's first frame) or from pixels. Two kinds need this
//! module first, off the UI thread:
//!
//! - **An animated GIF or WebP.** Its frames are decoded here, composed onto
//!   the whole canvas, each with the time it stays (`Animation`); the stage
//!   plays them (`image_stage.rs`). The webview did this by itself.
//! - **An AVIF.** iced cannot decode it; `avif.rs` does, to pixels.
//!
//! Everything else is handed on as the bytes it is, for the renderer.
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

/// What the viewer is given for a picture file.
#[derive(Debug, Clone)]
pub enum Loaded {
    /// The file as it is, for the renderer to decode.
    Encoded(Bytes),
    /// A picture decoded here, as pixels.
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
pub fn load(bytes: Vec<u8>) -> Result<Loaded, String> {
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode(&bytes)))
        .unwrap_or_else(|_| Err("the file is damaged".to_string()));
    match decoded {
        Ok(Some(loaded)) => Ok(loaded),
        // A picture of one frame, or one that would take too much memory, is
        // drawn by the renderer from the file: its first frame.
        Ok(None) => Ok(Loaded::Encoded(Bytes(Arc::new(bytes)))),
        Err(why) => Err(why),
    }
}

/// The pictures made here, or `None` for a file left to the renderer.
fn decode(bytes: &[u8]) -> Result<Option<Loaded>, String> {
    if avif::is_avif(&bytes[..bytes.len().min(512)]) {
        let pixels = avif::decode(bytes)?;
        let handle = Handle::from_rgba(pixels.width, pixels.height, pixels.rgba);
        return Ok(Some(Loaded::Pixels(handle, pixels.width, pixels.height)));
    }
    let animation = if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let decoder = ::image::codecs::gif::GifDecoder::new(Cursor::new(bytes));
        // A GIF the decoder cannot start on is left to the renderer, which
        // says what it makes of it.
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
