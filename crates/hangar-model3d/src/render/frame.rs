//! One picture: getting a scene's data to the GPU (`prepare`) and recording
//! the pass that draws it (`encode`).
//!
//! The two are separate because iced wants them to be: it prepares every
//! primitive of the window first, then records the frame.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use iced::wgpu;

use super::pipelines::{Cull, Kind, MeshKey};
use super::upload::{GpuMaterial, GpuModel};
use super::{
    COLOR_FORMAT, CameraMatrices, DEPTH_FORMAT, DisplayOptions, Gpu, NORMALS_LIMIT, SAMPLES,
};
use crate::animation::Playhead;
use crate::framing::grid_spacing;
use crate::model::{Model, Topology};
use crate::theme::{ViewerTheme, to_linear};

/// `Frame` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    key_dir: [f32; 4],
    viewport: [f32; 4],
}

/// `Grid` in grid.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GridUniform {
    centre: [f32; 4],
    params: [f32; 4],
    minor_color: [f32; 4],
    major_color: [f32; 4],
}

/// The key light's direction, from the model toward the light: the old
/// viewer's, up and to the front right.
const KEY_DIRECTION: [f32; 3] = [1.0, 1.6, 1.3];
/// How large points are drawn, in logical pixels.
const POINT_SIZE: f32 = 2.0;
/// The slots of a target's style buffer.
const STYLE_WIRE: u64 = 0;
const STYLE_NORMALS: u64 = 1;
const STYLE_AXIS: u64 = 2;

/// What is to be drawn.
pub(crate) struct Scene<'a> {
    pub model: Option<&'a Arc<Model>>,
    pub camera: CameraMatrices,
    pub display: DisplayOptions,
    pub theme: &'a ViewerTheme,
    /// Physical pixels to a logical one, for things drawn at a size in
    /// pixels.
    pub pixel_ratio: f32,
    /// Where in the model's animation to draw it; `None` draws the file's own
    /// pose.
    pub playhead: Option<Playhead>,
    /// The pixels of textures to send to the GPU in this frame, at most (one
    /// texture always goes): a window spreads a large model's textures over
    /// several frames, a picture taken for an agent sends them all.
    pub upload_budget: usize,
}

#[derive(Clone, Copy)]
struct Item {
    instance: u32,
    range: u32,
}

#[derive(Default)]
struct Plan {
    model: Option<u64>,
    opaque: Vec<Item>,
    blend: Vec<Item>,
    wire: Vec<u32>,
    normals: Vec<u32>,
    grid: bool,
    axes: bool,
}

pub(super) struct Target {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    pub resolved: wgpu::Texture,
    resolved_view: wgpu::TextureView,
    frame_buffer: wgpu::Buffer,
    /// The camera and the current model's matrices, by the model upload they
    /// were made for.
    scene_group: Option<(u64, wgpu::BindGroup)>,
    grid_buffer: wgpu::Buffer,
    grid_group: wgpu::BindGroup,
    style_buffer: wgpu::Buffer,
    style_group: wgpu::BindGroup,
    axes_buffer: wgpu::Buffer,
    /// The camera, and the axes' one matrix.
    axes_group: wgpu::BindGroup,
    clear: wgpu::Color,
    plan: Plan,
    pub last_used: u64,
}

fn extent(size: (u32, u32)) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
    }
}

impl Gpu {
    /// The bind group of a camera and a buffer of matrices.
    fn scene_group(&self, frame: &wgpu::Buffer, objects: &wgpu::Buffer) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model3d scene"),
            layout: &self.layouts.frame,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: objects,
                        offset: 0,
                        size: std::num::NonZeroU64::new(64),
                    }),
                },
            ],
        })
    }

    fn new_target(&self, size: (u32, u32)) -> Target {
        let device = &self.device;
        let texture = |label: &str, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent(size),
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let view =
            |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa = texture(
            "model3d colour",
            COLOR_FORMAT,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let depth = texture(
            "model3d depth",
            DEPTH_FORMAT,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let resolved = texture(
            "model3d picture",
            COLOR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );

        let uniform = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let group = |label: &str,
                     layout: &wgpu::BindGroupLayout,
                     buffer: &wgpu::Buffer,
                     size: Option<u64>| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: 0,
                        size: size.and_then(std::num::NonZeroU64::new),
                    }),
                }],
            })
        };
        let align = self.object_align;
        let frame_buffer = uniform("model3d frame", std::mem::size_of::<FrameUniform>() as u64);
        let grid_buffer = uniform("model3d grid", std::mem::size_of::<GridUniform>() as u64);
        let style_buffer = uniform("model3d style", align * 5);
        let axes_buffer = uniform("model3d axes", align);
        Target {
            size,
            msaa: view(&msaa),
            depth: view(&depth),
            resolved_view: view(&resolved),
            resolved,
            scene_group: None,
            grid_group: group("model3d grid", &self.layouts.grid, &grid_buffer, None),
            style_group: group(
                "model3d style",
                &self.layouts.style,
                &style_buffer,
                Some(16),
            ),
            axes_group: self.scene_group(&frame_buffer, &axes_buffer),
            frame_buffer,
            grid_buffer,
            style_buffer,
            axes_buffer,
            clear: wgpu::Color::BLACK,
            plan: Plan::default(),
            last_used: self.frame,
        }
    }

    /// Uploads what the scene needs and works out what to draw. `viewer`
    /// names the target the picture is drawn into, `size` is its size in
    /// physical pixels. Returns whether textures are still waiting to go to
    /// the GPU, which a later frame sends.
    pub(crate) fn prepare(&mut self, viewer: u64, size: (u32, u32), scene: &Scene<'_>) -> bool {
        let max = self.device.limits().max_texture_dimension_2d;
        let size = (size.0.clamp(1, max), size.1.clamp(1, max));

        #[cfg(test)]
        if std::mem::take(&mut self.refuse_next) {
            // A buffer past the device's limit: a validation error, as any
            // upload the device refuses would raise.
            let _ = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("too large"),
                size: u64::MAX / 8,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            });
        }

        let mut more = false;
        let mut gpu_model: Option<GpuModel> = None;
        if let Some(model) = scene.model {
            let uploaded = match self.models.remove(&model.id) {
                Some(uploaded) => uploaded,
                None => self.upload(model),
            };
            gpu_model = Some(uploaded);
        }
        if let (Some(model), Some(uploaded)) = (scene.model, gpu_model.as_mut()) {
            uploaded.last_used = self.frame;
            more = self.upload_textures(uploaded, model, scene.upload_budget);
            let normals = scene.display.normals && model.info.vertices <= NORMALS_LIMIT;
            self.pose(model, uploaded, scene.playhead, normals);
            self.prepare_overlays(model, uploaded, scene.display);
        }

        if self.targets.get(&viewer).map(|t| t.size) != Some(size) {
            let target = self.new_target(size);
            self.targets.insert(viewer, target);
        }
        let mut target = self
            .targets
            .remove(&viewer)
            .expect("the target was just made");
        target.last_used = self.frame;
        target.clear = Gpu::clear_color(scene.theme);
        self.write_uniforms(&target, scene, size);
        if let Some(uploaded) = &gpu_model
            && target
                .scene_group
                .as_ref()
                .map(|(generation, _)| *generation)
                != Some(uploaded.generation)
        {
            let group = self.scene_group(&target.frame_buffer, &uploaded.objects);
            target.scene_group = Some((uploaded.generation, group));
        }
        target.plan = plan(scene, gpu_model.as_ref());
        self.targets.insert(viewer, target);
        if let (Some(model), Some(uploaded)) = (scene.model, gpu_model) {
            self.models.insert(model.id, uploaded);
        }
        more
    }

    /// The buffers the wireframe and the normals draw from, made the first
    /// time they are asked for.
    fn prepare_overlays(&self, model: &Model, uploaded: &mut GpuModel, display: DisplayOptions) {
        let normals = display.normals && model.info.vertices <= NORMALS_LIMIT;
        for (mesh, source) in uploaded.meshes.iter_mut().zip(&model.meshes) {
            if display.wireframe && mesh.wire.is_none() {
                mesh.wire = self.wire_indices(source);
            }
            if normals && mesh.normals.is_none() {
                mesh.normals = self.normal_lines(source, mesh.local_radius * 0.02);
            }
        }
    }

    fn write_uniforms(&self, target: &Target, scene: &Scene<'_>, size: (u32, u32)) {
        let camera = &scene.camera;
        let key = Vec3::from(KEY_DIRECTION).normalize();
        let frame = FrameUniform {
            view_proj: camera.view_proj.to_cols_array_2d(),
            eye: [camera.eye[0], camera.eye[1], camera.eye[2], 0.0],
            key_dir: [key.x, key.y, key.z, 0.0],
            viewport: [
                size.0 as f32,
                size.1 as f32,
                POINT_SIZE * scene.pixel_ratio,
                0.0,
            ],
        };
        self.queue
            .write_buffer(&target.frame_buffer, 0, bytemuck::bytes_of(&frame));

        let theme = scene.theme;
        let color = |c: iced::Color, alpha: f32| {
            let [r, g, b] = to_linear(c);
            [r, g, b, alpha]
        };
        let style: Vec<(u64, [f32; 4])> = vec![
            (STYLE_WIRE, color(theme.wire, theme.wire_opacity())),
            (STYLE_NORMALS, color(theme.normals, 1.0)),
            (STYLE_AXIS, color(theme.axis_x, 1.0)),
            (STYLE_AXIS + 1, color(theme.axis_y, 1.0)),
            (STYLE_AXIS + 2, color(theme.axis_z, 1.0)),
        ];
        for (slot, value) in style {
            self.queue.write_buffer(
                &target.style_buffer,
                slot * self.object_align,
                bytemuck::bytes_of(&value),
            );
        }

        if let Some(model) = scene.model {
            // Lays the floor under the model: at its lowest point, across a
            // couple of its footprints, fading out from there — a floor to
            // stand on, not a plane to the horizon competing with the model.
            let size = model.bounds.size();
            let center = model.bounds.center();
            let longest = size[0].max(size[1]).max(size[2]);
            let footprint = size[0].max(size[2]).max(size[1] * 0.6).max(1e-3);
            let (minor, major) = grid_spacing(longest);
            let extent = footprint * 1.9;
            let floor = model.bounds.min[1] - longest.max(1e-3) * 1e-4;
            let grid = GridUniform {
                centre: [
                    center[0] as f32,
                    floor as f32,
                    center[2] as f32,
                    extent as f32,
                ],
                params: [
                    minor as f32,
                    major as f32,
                    extent as f32,
                    theme.grid_opacity(),
                ],
                minor_color: color(theme.grid_minor, 1.0),
                major_color: color(theme.grid_major, 1.0),
            };
            self.queue
                .write_buffer(&target.grid_buffer, 0, bytemuck::bytes_of(&grid));
            let axis_length = (longest.max(1e-3) * 0.6) as f32;
            let scale = Mat4::from_scale(Vec3::splat(axis_length));
            self.queue.write_buffer(
                &target.axes_buffer,
                0,
                bytemuck::cast_slice(&scale.to_cols_array()),
            );
        }
    }

    /// Records the pass that draws the viewer's picture into its target.
    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder, viewer: u64) {
        let Some(target) = self.targets.get(&viewer) else {
            return;
        };
        let model = target.plan.model.and_then(|id| self.models.get(&id));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("model3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.msaa,
                depth_slice: None,
                resolve_target: Some(&target.resolved_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(target.clear),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let (Some(model), Some((_, scene))) = (model, &target.scene_group) else {
            return;
        };

        self.draw_items(&mut pass, model, scene, &target.plan.opaque);
        if target.plan.grid {
            pass.set_pipeline(&self.pipelines.grid);
            pass.set_bind_group(0, scene, &[0]);
            pass.set_bind_group(1, &target.grid_group, &[]);
            pass.draw(0..6, 0..1);
        }
        self.draw_items(&mut pass, model, scene, &target.plan.blend);

        if !target.plan.wire.is_empty() {
            pass.set_pipeline(&self.pipelines.wire);
            pass.set_bind_group(
                1,
                &target.style_group,
                &[(STYLE_WIRE * self.object_align) as u32],
            );
            for &index in &target.plan.wire {
                let instance = &model.instances[index as usize];
                let mesh = &model.meshes[instance.mesh];
                let Some(wire) = &mesh.wire else { continue };
                pass.set_bind_group(0, scene, &[instance.offset]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(wire.buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..wire.count, 0, 0..1);
            }
        }
        if !target.plan.normals.is_empty() {
            pass.set_pipeline(&self.pipelines.normals);
            pass.set_bind_group(
                1,
                &target.style_group,
                &[(STYLE_NORMALS * self.object_align) as u32],
            );
            for &index in &target.plan.normals {
                let instance = &model.instances[index as usize];
                let Some(normals) = &model.meshes[instance.mesh].normals else {
                    continue;
                };
                pass.set_bind_group(0, scene, &[instance.offset]);
                pass.set_vertex_buffer(0, normals.buffer.slice(..));
                pass.draw(0..normals.count, 0..1);
            }
        }
        if target.plan.axes {
            pass.set_pipeline(&self.pipelines.axes);
            pass.set_bind_group(0, &target.axes_group, &[0]);
            pass.set_vertex_buffer(0, self.axes.slice(..));
            for axis in 0..3u64 {
                pass.set_bind_group(
                    1,
                    &target.style_group,
                    &[((STYLE_AXIS + axis) * self.object_align) as u32],
                );
                pass.draw(axis as u32 * 2..axis as u32 * 2 + 2, 0..1);
            }
        }
    }

    fn draw_items(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        model: &GpuModel,
        scene: &wgpu::BindGroup,
        items: &[Item],
    ) {
        let mut pipeline: Option<MeshKey> = None;
        let mut material: Option<usize> = None;
        for item in items {
            let instance = &model.instances[item.instance as usize];
            let mesh = &model.meshes[instance.mesh];
            let range = &mesh.ranges[item.range as usize];
            let key = mesh_key(mesh.topology, model.material(range), instance.mirrored);
            if pipeline != Some(key) {
                pass.set_pipeline(&self.pipelines.mesh[&key]);
                pipeline = Some(key);
            }
            if material != Some(range.material) {
                pass.set_bind_group(1, &model.material(range).bind_group, &[]);
                material = Some(range.material);
            }
            pass.set_bind_group(0, scene, &[instance.offset]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            let end = range.start + range.count;
            match (mesh.topology, &mesh.indices) {
                // A point is a square drawn for each vertex, so the range is
                // over the vertices whether or not the file indexed them.
                (Topology::Points, _) => {
                    let end = end.min(mesh.vertex_count);
                    if range.start < end {
                        pass.draw(0..6, range.start..end);
                    }
                }
                (_, Some(indices)) => {
                    pass.set_index_buffer(indices.buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(range.start..end, 0, 0..1);
                }
                (_, None) => pass.draw(range.start..end, 0..1),
            }
        }
    }
}

fn mesh_key(topology: Topology, material: &GpuMaterial, mirrored: bool) -> MeshKey {
    match topology {
        Topology::Triangles => MeshKey {
            kind: Kind::Triangles,
            blend: material.blend,
            cull: if material.double_sided {
                Cull::Nothing
            } else if mirrored {
                Cull::BackMirrored
            } else {
                Cull::Back
            },
        },
        Topology::Lines => MeshKey {
            kind: Kind::Lines,
            blend: false,
            cull: Cull::Nothing,
        },
        Topology::Points => MeshKey {
            kind: Kind::Points,
            blend: false,
            cull: Cull::Nothing,
        },
    }
}

/// What to draw this frame, and in which order: opaque things first, by
/// pipeline and material so the GPU changes state as little as it can;
/// translucent ones last, furthest first, so they blend over what is behind
/// them.
fn plan(scene: &Scene<'_>, uploaded: Option<&GpuModel>) -> Plan {
    let (Some(model), Some(uploaded)) = (scene.model, uploaded) else {
        return Plan::default();
    };
    let mut opaque = Vec::new();
    let mut blend: Vec<(f32, Item)> = Vec::new();
    let eye = Vec3::from(scene.camera.eye);
    for (index, instance) in uploaded.instances.iter().enumerate() {
        let mesh = &uploaded.meshes[instance.mesh];
        for (range_index, range) in mesh.ranges.iter().enumerate() {
            let item = Item {
                instance: index as u32,
                range: range_index as u32,
            };
            if mesh.topology == Topology::Triangles && uploaded.material(range).blend {
                blend.push((Vec3::from(instance.center).distance_squared(eye), item));
            } else {
                opaque.push(item);
            }
        }
    }
    opaque.sort_by_key(|item| {
        let instance = &uploaded.instances[item.instance as usize];
        let mesh = &uploaded.meshes[instance.mesh];
        let range = &mesh.ranges[item.range as usize];
        (
            mesh.topology as u8,
            uploaded.material(range).double_sided,
            instance.mirrored,
            range.material,
        )
    });
    blend.sort_by(|a, b| b.0.total_cmp(&a.0));

    let wire = scene.display.wireframe;
    let normals = scene.display.normals && model.info.vertices <= NORMALS_LIMIT;
    let overlay = |wanted: bool, has: &dyn Fn(&super::upload::GpuMesh) -> bool| -> Vec<u32> {
        if !wanted {
            return Vec::new();
        }
        uploaded
            .instances
            .iter()
            .enumerate()
            .filter(|(_, instance)| has(&uploaded.meshes[instance.mesh]))
            .map(|(index, _)| index as u32)
            .collect()
    };
    Plan {
        model: Some(model.id),
        opaque,
        blend: blend.into_iter().map(|(_, item)| item).collect(),
        wire: overlay(wire, &|mesh| mesh.wire.is_some()),
        normals: overlay(normals, &|mesh| mesh.normals.is_some()),
        grid: scene.display.grid,
        axes: scene.display.axes,
    }
}
