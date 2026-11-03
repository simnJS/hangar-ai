//! Drawing a model with wgpu.
//!
//! One [`Gpu`] holds everything a device needs to draw: the pipelines, the
//! models it has uploaded, and a target (multisampled colour, depth and the
//! resolved picture) for each viewer. Two things drive it: iced's shader
//! widget, which hands over the window's device (`viewer.rs`), and a device of
//! our own for pictures that are asked for with no window to draw in
//! (`offscreen.rs`).
//!
//! Every pass draws into the same formats — `Rgba8UnormSrgb` colour, 4×
//! multisampled, with a `Depth32Float` buffer — so the pipelines do not depend
//! on the window's format; only the last copy into the window does.

mod frame;
pub(crate) mod offscreen;
mod pipelines;
mod upload;

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use iced::wgpu;

use crate::theme::ViewerTheme;
use pipelines::{Layouts, Pipelines};

pub(crate) use frame::Scene;

pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub(crate) const SAMPLES: u32 = 4;

/// Past this many vertices, drawing every normal costs more than it shows.
pub const NORMALS_LIMIT: usize = 600_000;

/// Runs GPU work and reports what the device objected to instead of letting it
/// panic: on the window's device, an error nobody caught takes the whole
/// application down, and a model the preview cannot upload (a texture past the
/// device's limits, say) is not worth that.
pub(crate) fn guarded<T>(device: &wgpu::Device, work: impl FnOnce() -> T) -> Result<T, String> {
    device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = work();
    let validation = pollster::block_on(device.pop_error_scope());
    let memory = pollster::block_on(device.pop_error_scope());
    match validation.or(memory) {
        Some(error) => Err(error.to_string()),
        None => Ok(value),
    }
}

/// What is drawn besides the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayOptions {
    /// The triangles' edges, over the shaded model.
    pub wireframe: bool,
    /// The floor under the model.
    pub grid: bool,
    /// The origin's X, Y and Z axes, over everything.
    pub axes: bool,
    /// A short line out of every vertex along its normal.
    pub normals: bool,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        DisplayOptions {
            wireframe: false,
            grid: true,
            axes: false,
            normals: false,
        }
    }
}

/// The camera, as the shaders need it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CameraMatrices {
    pub view_proj: Mat4,
    pub eye: [f32; 3],
}

pub(crate) struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layouts: Layouts,
    pipelines: Pipelines,
    samplers: HashMap<upload::SamplerKey, wgpu::Sampler>,
    /// Bound where a material has no texture: never read, but a bind group
    /// must be whole.
    dummy_view: wgpu::TextureView,
    mip_sampler: wgpu::Sampler,
    /// The origin's three axes, as pairs of end points along X, Y and Z.
    axes: wgpu::Buffer,
    /// Bytes between two matrices in a model's objects buffer: dynamic
    /// offsets must be a multiple of what the device wants.
    object_align: u64,
    models: HashMap<u64, upload::GpuModel>,
    /// Counts uploads, to tell one upload of a model from the next.
    generation: u64,
    targets: HashMap<u64, frame::Target>,
    /// Counts frames, for dropping what has not been drawn in a while.
    frame: u64,
}

/// Models kept on the GPU besides the one being drawn, so tabs switch
/// instantly, as the old viewer's `KEEP_LOADED` did.
const KEEP_MODELS: usize = 4;
/// A viewer's target is dropped when it has not been drawn for this many
/// frames.
const KEEP_TARGET_FRAMES: u64 = 600;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct AxesVertex([f32; 3]);

impl Gpu {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Gpu {
        let layouts = Layouts::new(device);
        let pipelines = Pipelines::new(device, &layouts);
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model3d no texture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let axes = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ]
        .map(AxesVertex);
        let axes = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("model3d axes"),
                contents: bytemuck::cast_slice(&axes),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        Gpu {
            device: device.clone(),
            queue: queue.clone(),
            layouts,
            pipelines,
            samplers: HashMap::new(),
            dummy_view: dummy.create_view(&wgpu::TextureViewDescriptor::default()),
            mip_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("model3d mip"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..wgpu::SamplerDescriptor::default()
            }),
            axes,
            object_align: alignment.max(64),
            models: HashMap::new(),
            generation: 0,
            targets: HashMap::new(),
            frame: 0,
        }
    }

    /// Called at the end of each frame: counts it, and lets go of what has
    /// not been drawn in a while — models that were dropped, models past the
    /// few kept, and targets of viewers that are gone.
    pub(crate) fn trim(&mut self) {
        self.frame += 1;
        self.models
            .retain(|_, model| model.source.strong_count() > 0);
        if self.models.len() > KEEP_MODELS + 1 {
            let mut by_use: Vec<(u64, u64)> = self
                .models
                .iter()
                .map(|(id, model)| (model.last_used, *id))
                .collect();
            by_use.sort_unstable_by(|a, b| b.cmp(a));
            for (_, id) in by_use.into_iter().skip(KEEP_MODELS + 1) {
                self.models.remove(&id);
            }
        }
        let frame = self.frame;
        self.targets
            .retain(|_, target| frame.saturating_sub(target.last_used) < KEEP_TARGET_FRAMES);
    }

    /// Drops a model's copy on the GPU.
    pub(crate) fn forget(&mut self, model: u64) {
        self.models.remove(&model);
    }

    /// Lets go of a viewer's target.
    pub(crate) fn release(&mut self, viewer: u64) {
        self.targets.remove(&viewer);
    }

    /// The picture a viewer last drew, resolved to one sample per pixel.
    pub(crate) fn picture(&self, viewer: u64) -> Option<&wgpu::Texture> {
        self.targets.get(&viewer).map(|target| &target.resolved)
    }

    /// The theme's background as the clear colour: the colours of the window
    /// are sRGB, the target is linear.
    pub(crate) fn clear_color(theme: &ViewerTheme) -> wgpu::Color {
        let [r, g, b] = crate::theme::to_linear(theme.background);
        wgpu::Color {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: 1.0,
        }
    }
}
