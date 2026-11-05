//! A device of our own, for the pictures nobody is looking at: an agent asking
//! for a screenshot of a model, the capture button, a model that is open in a
//! tab that is not on screen. It needs no window and no surface, so it works
//! from any thread.
//!
//! One device is made the first time it is needed and kept while pictures are
//! asked for: starting a GPU takes longer than drawing a cube, and an agent
//! often asks for several angles in a row. It keeps only the model it last
//! drew, and the whole device goes after [`IDLE`] without a picture: the
//! window's device already holds every model on screen, and a second copy of
//! each in VRAM for the rest of the process is not worth it.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use iced::wgpu;

use super::{Gpu, Scene};

/// Why a picture could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// No graphics adapter could be opened.
    NoGpu(String),
    /// The model is not loaded, or there is nothing to draw.
    Empty,
    /// The GPU failed while drawing or reading the picture back.
    Gpu(String),
    /// The picture could not be encoded as a PNG.
    Encode(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::NoGpu(reason) => write!(
                f,
                "No graphics adapter could be opened to draw the picture ({reason})."
            ),
            CaptureError::Empty => f.write_str("There is no model to take a picture of."),
            CaptureError::Gpu(reason) => {
                write!(f, "The GPU failed while drawing the picture: {reason}")
            }
            CaptureError::Encode(reason) => write!(f, "The picture could not be encoded: {reason}"),
        }
    }
}

impl std::error::Error for CaptureError {}

struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    gpu: Gpu,
}

/// The capture device, when one is open, and what lets it go.
struct Context {
    open: Option<Offscreen>,
    /// When the last picture was drawn.
    used: Option<Instant>,
    /// A thread is waiting to close the device once it is idle.
    reaper: bool,
}

static CONTEXT: Mutex<Context> = Mutex::new(Context {
    open: None,
    used: None,
    reaper: false,
});

/// How long the capture device stays open without a picture.
pub(crate) const IDLE: Duration = Duration::from_secs(30);

fn context() -> std::sync::MutexGuard<'static, Context> {
    CONTEXT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Closes the capture device if it has not drawn for `idle`; whether it did.
fn close_if_idle(held: &mut Context, idle: Duration) -> bool {
    let idle = held.used.is_none_or(|used| used.elapsed() >= idle);
    if idle {
        held.open = None;
        held.used = None;
    }
    idle
}

/// Starts the thread that closes the device once it is idle, if none runs.
fn watch_idle(held: &mut Context) {
    if held.reaper {
        return;
    }
    let started = std::thread::Builder::new()
        .name("model3d-capture-idle".into())
        .spawn(|| {
            loop {
                std::thread::sleep(IDLE);
                let mut held = context();
                if close_if_idle(&mut held, IDLE) {
                    held.reaper = false;
                    return;
                }
            }
        });
    // Without the thread the device stays open, as it did before.
    held.reaper = started.is_ok();
}

/// The target the captures are drawn into: any key will do, there is one
/// picture at a time.
const CAPTURE_KEY: u64 = u64::MAX;

fn open() -> Result<Offscreen, CaptureError> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let adapter = [false, true]
        .into_iter()
        .find_map(|fallback| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: fallback,
                compatible_surface: None,
            }))
            .ok()
        })
        .ok_or_else(|| CaptureError::NoGpu("none found".into()))?;
    // As large a picture as the adapter can draw, and the smaller limits if
    // it will not take those.
    let candidates = [
        wgpu::Limits::default().using_resolution(adapter.limits()),
        adapter.limits(),
    ];
    let mut last = String::new();
    for limits in candidates {
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("model3d capture"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            ..wgpu::DeviceDescriptor::default()
        })) {
            Ok((device, queue)) => {
                let gpu = super::guarded(&device, || Gpu::new(&device, &queue))
                    .map_err(CaptureError::Gpu)?;
                return Ok(Offscreen { device, queue, gpu });
            }
            Err(error) => last = error.to_string(),
        }
    }
    Err(CaptureError::NoGpu(last))
}

/// A device for tests that draw through the window's path instead of this
/// module's own: the same adapter search, a device of its own. `None` on a
/// machine with no adapter, where such a test has nothing to check.
#[cfg(test)]
pub(crate) fn test_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    match open() {
        Ok(Offscreen { device, queue, .. }) => Some((device, queue)),
        Err(error) => {
            eprintln!("skipped, no GPU: {error}");
            None
        }
    }
}

/// Draws the scene at `size` and reads it back: RGBA, 8 bits a channel, top
/// row first, sRGB-encoded as a PNG wants it.
pub(crate) fn render(
    scene: &Scene<'_>,
    size: (u32, u32),
) -> Result<(Vec<u8>, (u32, u32)), CaptureError> {
    if scene.model.is_none() {
        return Err(CaptureError::Empty);
    }
    let mut guard = context();
    if guard.open.is_none() {
        guard.open = Some(open()?);
    }
    let context = guard.open.as_mut().expect("the context was just made");
    let result = draw(context, scene, size);
    context.gpu.release(CAPTURE_KEY);
    // Only the model just drawn stays, for the next angle of it.
    context.gpu.keep_only(scene.model.map(|model| model.id));
    context.gpu.trim();
    guard.used = Some(Instant::now());
    watch_idle(&mut guard);
    result
}

fn draw(
    context: &mut Offscreen,
    scene: &Scene<'_>,
    size: (u32, u32),
) -> Result<(Vec<u8>, (u32, u32)), CaptureError> {
    let Offscreen { device, queue, gpu } = context;
    let max = device.limits().max_texture_dimension_2d;
    let size = (size.0.clamp(1, max), size.1.clamp(1, max));

    // Both error scopes are popped whatever happens inside, and an
    // out-of-memory is reported rather than left to wgpu's handler (which
    // panics on this thread).
    let recorded = super::guarded(device, || record(device, queue, gpu, scene, size))
        .map_err(CaptureError::Gpu)??;
    let (readback, receiver, unpadded, padded) = recorded;
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| CaptureError::Gpu(error.to_string()))?;
    receiver
        .recv()
        .map_err(|error| CaptureError::Gpu(error.to_string()))?
        .map_err(|error| CaptureError::Gpu(error.to_string()))?;

    let mapped = readback.slice(..).get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded * size.1) as usize);
    for row in mapped.chunks_exact(padded as usize) {
        pixels.extend_from_slice(&row[..unpadded as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok((pixels, size))
}

/// What a picture waits on once it is submitted: the buffer it is read back
/// into, the end of its mapping, and its rows' width without and with
/// padding.
type Recorded = (
    wgpu::Buffer,
    std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    u32,
    u32,
);

/// Draws the scene and asks for it back.
fn record(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    gpu: &mut Gpu,
    scene: &Scene<'_>,
    size: (u32, u32),
) -> Result<Recorded, CaptureError> {
    gpu.prepare(CAPTURE_KEY, size, scene);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("model3d capture"),
    });
    gpu.encode(&mut encoder, CAPTURE_KEY);

    let picture = gpu.picture(CAPTURE_KEY).ok_or(CaptureError::Empty)?;
    let unpadded = size.0 * 4;
    let padded = unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("model3d readback"),
        size: u64::from(padded) * u64::from(size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: picture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size.1),
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    Ok((readback, receiver, unpadded, padded))
}

/// A picture as PNG bytes.
pub(crate) fn encode_png(pixels: Vec<u8>, size: (u32, u32)) -> Result<Vec<u8>, CaptureError> {
    let image = image::RgbaImage::from_raw(size.0, size.1, pixels)
        .ok_or_else(|| CaptureError::Encode("the pixels do not fill the picture".into()))?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| CaptureError::Encode(error.to_string()))?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capture_device_goes_once_it_has_not_drawn_for_a_while() {
        let mut held = Context {
            open: None,
            used: Some(Instant::now()),
            reaper: true,
        };
        assert!(!close_if_idle(&mut held, IDLE), "just used");
        held.used = Instant::now().checked_sub(IDLE + Duration::from_secs(1));
        assert!(held.used.is_some());
        assert!(close_if_idle(&mut held, IDLE));
        assert!(held.open.is_none() && held.used.is_none());
    }
}
