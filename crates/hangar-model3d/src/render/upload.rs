//! A model's copy on one GPU: vertex and index buffers, the textures with
//! their mip chains, a bind group for each material, and where each instance
//! stands. Made once, the first time a device draws the model, and dropped
//! when the model is.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use bytemuck::{Pod, Zeroable};
use iced::wgpu;
use iced::wgpu::util::{BufferInitDescriptor, DeviceExt};

use super::Gpu;
use crate::model::{AlphaMode, Image, Material, Model, TextureRef, Topology, Vertex, Wrap};

/// `Material` in mesh.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    emissive: [f32; 4],
    factors: [f32; 4],
    flags: [u32; 4],
    uv_x: [f32; 4],
    uv_y: [f32; 4],
}

const HAS_BASE: u32 = 1;
const HAS_METAL_ROUGH: u32 = 2;
const HAS_NORMAL: u32 = 4;
const HAS_OCCLUSION: u32 = 8;
const HAS_EMISSIVE: u32 = 16;

/// Past this many triangles the wireframe is drawn without merging the edges
/// two triangles share: finding them costs a sort, and at that density nobody
/// can tell a doubled line from a single one.
const MERGE_EDGES_LIMIT: usize = 300_000;

pub(super) struct GpuRange {
    pub start: u32,
    pub count: u32,
    /// An index into the model's GPU materials; the last is the default.
    pub material: usize,
}

pub(super) struct Counted {
    pub buffer: wgpu::Buffer,
    pub count: u32,
}

pub(super) struct GpuMesh {
    pub topology: Topology,
    pub vertices: wgpu::Buffer,
    pub vertex_count: u32,
    pub indices: Option<Counted>,
    pub ranges: Vec<GpuRange>,
    /// The triangles' edges as line indices over `vertices`, made the first
    /// time the wireframe is shown.
    pub wire: Option<Counted>,
    /// A short line out of each vertex along its normal, made the first time
    /// the normals are shown.
    pub normals: Option<Counted>,
    /// Half the diagonal of the mesh's own box: what the normals' length is a
    /// fraction of.
    pub local_radius: f32,
}

pub(super) struct GpuMaterial {
    pub bind_group: wgpu::BindGroup,
    pub blend: bool,
    pub double_sided: bool,
}

pub(super) struct GpuInstance {
    pub mesh: usize,
    /// The instance's matrix has a negative determinant: it turns the faces
    /// inside out.
    pub mirrored: bool,
    /// Where the instance's box is centred, for sorting transparent ones.
    pub center: [f32; 3],
    /// Byte offset of its matrix in the objects buffer.
    pub offset: u32,
}

pub(super) struct GpuModel {
    pub source: Weak<Model>,
    pub meshes: Vec<GpuMesh>,
    pub materials: Vec<GpuMaterial>,
    pub instances: Vec<GpuInstance>,
    /// One matrix per instance, `object_align` bytes apart, bound at a
    /// dynamic offset.
    pub objects: wgpu::Buffer,
    /// Which upload this is: a bind group made over the buffer of an earlier
    /// one must not be taken for this one's.
    pub generation: u64,
    pub last_used: u64,
}

impl GpuModel {
    /// The GPU material a range is drawn with.
    pub fn material(&self, range: &GpuRange) -> &GpuMaterial {
        &self.materials[range.material]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct SamplerKey {
    pub wrap: [Wrap; 2],
    pub nearest: bool,
}

fn address(wrap: Wrap) -> wgpu::AddressMode {
    match wrap {
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
    }
}

fn buffer<T: Pod>(
    device: &wgpu::Device,
    label: &str,
    data: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage,
    })
}

impl Gpu {
    pub(super) fn sampler(&mut self, key: SamplerKey) -> wgpu::Sampler {
        let device = &self.device;
        self.samplers
            .entry(key)
            .or_insert_with(|| {
                let filter = if key.nearest {
                    wgpu::FilterMode::Nearest
                } else {
                    wgpu::FilterMode::Linear
                };
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("model3d texture"),
                    address_mode_u: address(key.wrap[0]),
                    address_mode_v: address(key.wrap[1]),
                    mag_filter: filter,
                    min_filter: filter,
                    mipmap_filter: if key.nearest {
                        wgpu::FilterMode::Nearest
                    } else {
                        wgpu::FilterMode::Linear
                    },
                    anisotropy_clamp: if key.nearest { 1 } else { 8 },
                    ..wgpu::SamplerDescriptor::default()
                })
            })
            .clone()
    }

    /// A texture on the GPU with every mip level filled in. `srgb` says
    /// whether the pixels are colours (base colour, emissive) or data
    /// (normals, roughness, occlusion).
    fn texture(
        &self,
        image: &Image,
        srgb: bool,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::TextureView {
        let limit = self.device.limits().max_texture_dimension_2d;
        let shrunk;
        let (image, width, height) = if image.width > limit || image.height > limit {
            let scale = f64::from(limit) / f64::from(image.width.max(image.height));
            let width = ((f64::from(image.width) * scale) as u32).max(1);
            let height = ((f64::from(image.height) * scale) as u32).max(1);
            let source = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
                .expect("a decoded image has width x height x 4 bytes");
            let resized = image::imageops::resize(
                &source,
                width,
                height,
                image::imageops::FilterType::Triangle,
            );
            shrunk = Image {
                width,
                height,
                rgba: resized.into_raw(),
            };
            (&shrunk, width, height)
        } else {
            (image, image.width, image.height)
        };

        let levels = 32 - width.max(height).leading_zeros();
        let format = if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("model3d texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let pipeline = if srgb {
            &self.pipelines.mip_srgb
        } else {
            &self.pipelines.mip
        };
        let level_view = |level: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..wgpu::TextureViewDescriptor::default()
            })
        };
        for level in 1..levels {
            let source = level_view(level - 1);
            let destination = level_view(level);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("model3d mip"),
                layout: &self.layouts.mip,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.mip_sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("model3d mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn material_bind_group(
        &mut self,
        material: &Material,
        model: &Model,
        textures: &mut HashMap<(usize, bool), wgpu::TextureView>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::BindGroup {
        let mut flags = 0u32;
        let mut slot = |gpu: &mut Gpu, reference: Option<TextureRef>, srgb: bool, bit: u32| {
            let Some(reference) = reference else {
                return (
                    gpu.dummy_view.clone(),
                    gpu.sampler(SamplerKey {
                        wrap: [Wrap::Repeat; 2],
                        nearest: false,
                    }),
                );
            };
            let view = textures
                .entry((reference.image, srgb))
                .or_insert_with(|| gpu.texture(&model.images[reference.image], srgb, encoder))
                .clone();
            flags |= bit;
            (
                view,
                gpu.sampler(SamplerKey {
                    wrap: reference.wrap,
                    nearest: reference.nearest,
                }),
            )
        };
        let base = slot(self, material.base_color_texture, true, HAS_BASE);
        let metal_rough = slot(
            self,
            material.metallic_roughness_texture,
            false,
            HAS_METAL_ROUGH,
        );
        let normal = slot(self, material.normal_texture, false, HAS_NORMAL);
        let occlusion = slot(self, material.occlusion_texture, false, HAS_OCCLUSION);
        let emissive = slot(self, material.emissive_texture, true, HAS_EMISSIVE);

        let (mode, cutoff) = match material.alpha {
            AlphaMode::Opaque => (0, 0.0),
            AlphaMode::Mask(cutoff) => (1, cutoff),
            AlphaMode::Blend => (2, 0.0),
        };
        let uv = material.uv_transform;
        let uniform = MaterialUniform {
            base_color: material.base_color,
            emissive: [
                material.emissive[0],
                material.emissive[1],
                material.emissive[2],
                material.occlusion_strength,
            ],
            factors: [
                material.metallic,
                material.roughness,
                material.normal_scale,
                cutoff,
            ],
            flags: [flags, mode, u32::from(material.unlit), 0],
            uv_x: [uv[0], uv[1], uv[2], 0.0],
            uv_y: [uv[3], uv[4], uv[5], 0.0],
        };
        let uniform = buffer(
            &self.device,
            "model3d material",
            &[uniform],
            wgpu::BufferUsages::UNIFORM,
        );
        fn view(t: &(wgpu::TextureView, wgpu::Sampler)) -> wgpu::BindingResource<'_> {
            wgpu::BindingResource::TextureView(&t.0)
        }
        fn samp(t: &(wgpu::TextureView, wgpu::Sampler)) -> wgpu::BindingResource<'_> {
            wgpu::BindingResource::Sampler(&t.1)
        }
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model3d material"),
            layout: &self.layouts.material,
            entries: &[
                entry(0, uniform.as_entire_binding()),
                entry(1, view(&base)),
                entry(2, view(&metal_rough)),
                entry(3, view(&normal)),
                entry(4, view(&occlusion)),
                entry(5, view(&emissive)),
                entry(6, samp(&base)),
                entry(7, samp(&metal_rough)),
                entry(8, samp(&normal)),
                entry(9, samp(&occlusion)),
                entry(10, samp(&emissive)),
            ],
        })
    }

    /// Puts a model on the GPU.
    pub(super) fn upload(&mut self, model: &Arc<Model>) -> GpuModel {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("model3d upload"),
            });
        let mut textures = HashMap::new();
        let mut materials: Vec<GpuMaterial> = Vec::new();
        // The last one is the default, for ranges that name none.
        let clay = Material::clay(false);
        for material in model.materials.iter().chain([&clay]) {
            let bind_group = self.material_bind_group(material, model, &mut textures, &mut encoder);
            materials.push(GpuMaterial {
                bind_group,
                blend: material.alpha == AlphaMode::Blend,
                double_sided: material.double_sided,
            });
        }
        self.queue.submit([encoder.finish()]);
        let default_material = materials.len() - 1;

        let meshes: Vec<GpuMesh> = model
            .meshes
            .iter()
            .map(|mesh| {
                let element_count = mesh.element_count() as u32;
                GpuMesh {
                    topology: mesh.topology,
                    vertices: buffer(
                        &self.device,
                        "model3d vertices",
                        &mesh.vertices,
                        wgpu::BufferUsages::VERTEX,
                    ),
                    vertex_count: mesh.vertices.len() as u32,
                    indices: mesh.indices.as_ref().map(|indices| Counted {
                        buffer: buffer(
                            &self.device,
                            "model3d indices",
                            indices,
                            wgpu::BufferUsages::INDEX,
                        ),
                        count: indices.len() as u32,
                    }),
                    ranges: mesh
                        .ranges
                        .iter()
                        .filter_map(|range| {
                            // A range is held to the buffer it reads, so a
                            // file that lies about its counts cannot make the
                            // GPU read past the end.
                            let start = range.start.min(element_count);
                            let count = range.count.min(element_count - start);
                            (count > 0).then_some(GpuRange {
                                start,
                                count,
                                material: range
                                    .material
                                    .filter(|&m| m < model.materials.len())
                                    .unwrap_or(default_material),
                            })
                        })
                        .collect(),
                    wire: None,
                    normals: None,
                    local_radius: (crate::framing::radius_of(&mesh.bounds()) as f32).max(1e-6),
                }
            })
            .collect();

        let align = self.object_align;
        let mut objects = vec![0u8; (align as usize) * model.instances.len().max(1)];
        let mut instances = Vec::with_capacity(model.instances.len());
        for (i, instance) in model.instances.iter().enumerate() {
            let matrix = model.display_transform(instance);
            let offset = i * align as usize;
            objects[offset..offset + 64]
                .copy_from_slice(bytemuck::cast_slice(&matrix.to_cols_array()));
            let bounds = model.meshes[instance.mesh].bounds();
            let center =
                matrix.transform_point3(glam::Vec3::from(bounds.center().map(|c| c as f32)));
            instances.push(GpuInstance {
                mesh: instance.mesh,
                mirrored: matrix.determinant() < 0.0,
                center: center.to_array(),
                offset: offset as u32,
            });
        }
        let objects_buffer = self.device.create_buffer_init(&BufferInitDescriptor {
            label: Some("model3d objects"),
            contents: &objects,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        self.generation += 1;

        GpuModel {
            source: Arc::downgrade(model),
            meshes,
            materials,
            instances,
            objects: objects_buffer,
            generation: self.generation,
            last_used: self.frame,
        }
    }

    /// The wireframe's line indices for one mesh: the edges of the triangles
    /// the mesh draws.
    pub(super) fn wire_indices(&self, source: &crate::model::Mesh) -> Option<Counted> {
        if source.topology != Topology::Triangles {
            return None;
        }
        let element = |i: usize| {
            source
                .indices
                .as_ref()
                .map_or(i as u32, |indices| indices[i])
        };
        let mut edges: Vec<[u32; 2]> = Vec::new();
        for range in &source.ranges {
            let end = (range.start as usize + range.count as usize).min(source.element_count());
            let mut at = range.start as usize;
            while at + 3 <= end {
                let [a, b, c] = [element(at), element(at + 1), element(at + 2)];
                edges.extend([[a, b], [b, c], [c, a]]);
                at += 3;
            }
        }
        if edges.len() / 3 <= MERGE_EDGES_LIMIT {
            // Two triangles share an edge, and a translucent line drawn twice
            // is a stronger line: each is kept once.
            for edge in &mut edges {
                edge.sort_unstable();
            }
            edges.sort_unstable();
            edges.dedup();
        }
        let flat: Vec<u32> = edges.into_iter().flatten().collect();
        (!flat.is_empty()).then(|| Counted {
            buffer: buffer(
                &self.device,
                "model3d wire",
                &flat,
                wgpu::BufferUsages::INDEX,
            ),
            count: flat.len() as u32,
        })
    }

    /// A segment out of every vertex along its normal, as a vertex buffer of
    /// positions.
    pub(super) fn normal_lines(&self, source: &crate::model::Mesh, length: f32) -> Option<Counted> {
        if source.topology != Topology::Triangles || source.vertices.is_empty() {
            return None;
        }
        let mut positions: Vec<[f32; 3]> = Vec::with_capacity(source.vertices.len() * 2);
        for Vertex {
            position, normal, ..
        } in &source.vertices
        {
            positions.push(*position);
            positions.push([
                position[0] + normal[0] * length,
                position[1] + normal[1] * length,
                position[2] + normal[2] * length,
            ]);
        }
        Some(Counted {
            buffer: buffer(
                &self.device,
                "model3d normals",
                &positions,
                wgpu::BufferUsages::VERTEX,
            ),
            count: positions.len() as u32,
        })
    }
}
