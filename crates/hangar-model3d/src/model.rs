//! A loaded model, as plain data: meshes, where they stand, their materials
//! and textures. Every loader produces [`ModelParts`]; [`Model::new`] derives
//! what the viewer and the agents need from them (statistics, bounds, the
//! points the camera frames).
//!
//! Nothing here touches the GPU: a model is read on a background thread and
//! shared as an `Arc<Model>`, and each device that draws it uploads its own
//! copy (`render`).

use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::{Pod, Zeroable};
use glam::{DMat4, DVec3, Mat4};

use crate::animation::{Clip, Deform, Playhead, Rig};
use crate::formats::ModelFormat;
use crate::framing::{Bounds, Vec3, radius_of};
use crate::stats::{ModelInfo, model_info};

/// One vertex, as it goes to the GPU. Normals are always present (a loader
/// computes them when the file has none) and colours default to white, so the
/// shader multiplies them in unconditionally.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Linear, 8 bits per channel: what glTF's `COLOR_0` and a PLY's
    /// red/green/blue hold.
    pub color: [u8; 4],
}

impl Vertex {
    pub const WHITE: [u8; 4] = [255; 4];

    pub fn at(position: [f32; 3]) -> Self {
        Vertex {
            position,
            normal: [0.0, 1.0, 0.0],
            uv: [0.0; 2],
            color: Self::WHITE,
        }
    }
}

/// What a mesh's elements are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    /// Three elements to a face.
    Triangles,
    /// Two elements to a segment.
    Lines,
    Points,
}

/// A run of a mesh's elements drawn with one material: an OBJ object that
/// switches material halfway is one mesh of two ranges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawRange {
    /// In indices when the mesh is indexed, in vertices when it is not.
    pub start: u32,
    pub count: u32,
    /// `None` draws in the default material: the clay the plain formats use.
    pub material: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub name: String,
    pub topology: Topology,
    /// The vertices as the file leaves them: a skinned mesh is posed as its
    /// nodes stand, a morphing one at its starting weights. Everything that
    /// measures the model (bounds, statistics, framing) reads these.
    pub vertices: Vec<Vertex>,
    pub indices: Option<Vec<u32>>,
    pub ranges: Vec<DrawRange>,
    /// What moves the vertices when a clip plays; `None` for a mesh nothing
    /// deforms, which is nearly all of them.
    pub deform: Option<Deform>,
    bounds: Bounds,
}

impl Mesh {
    /// A mesh drawn whole with one material.
    pub fn new(
        name: impl Into<String>,
        topology: Topology,
        vertices: Vec<Vertex>,
        indices: Option<Vec<u32>>,
        material: Option<usize>,
    ) -> Mesh {
        let count = indices.as_ref().map_or(vertices.len(), Vec::len) as u32;
        Mesh::with_ranges(
            name,
            topology,
            vertices,
            indices,
            vec![DrawRange {
                start: 0,
                count,
                material,
            }],
        )
    }

    pub fn with_ranges(
        name: impl Into<String>,
        topology: Topology,
        vertices: Vec<Vertex>,
        indices: Option<Vec<u32>>,
        ranges: Vec<DrawRange>,
    ) -> Mesh {
        let bounds = Bounds::around(vertices.iter().map(|v| v.position.map(f64::from)));
        Mesh {
            name: name.into(),
            topology,
            vertices,
            indices,
            ranges,
            deform: None,
            bounds,
        }
    }

    /// Elements in the buffer a draw reads: indices when indexed, vertices
    /// otherwise.
    pub fn element_count(&self) -> usize {
        self.indices.as_ref().map_or(self.vertices.len(), Vec::len)
    }

    /// Elements the ranges cover. A range that leaves part of a buffer out
    /// leaves it out of the count too, and one that runs past the end is held
    /// to it.
    pub fn drawn_elements(&self) -> usize {
        let total = self.element_count();
        self.ranges
            .iter()
            .map(|range| {
                let from = (range.start as usize).min(total);
                (range.count as usize).min(total - from)
            })
            .sum()
    }

    /// The box around the vertices, in the mesh's own coordinates.
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
}

/// A mesh standing somewhere in the file. A glTF mesh several nodes point at
/// is several instances of the same mesh, as the old viewer's scene held
/// several objects sharing one geometry.
#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub mesh: usize,
    /// Mesh to the file's own coordinates, before any turn upright.
    pub transform: Mat4,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlphaMode {
    Opaque,
    /// Fragments under the cut-off are dropped.
    Mask(f32),
    Blend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wrap {
    Repeat,
    Mirror,
    Clamp,
}

/// A texture a material reads, and how it is sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureRef {
    pub image: usize,
    pub wrap: [Wrap; 2],
    pub nearest: bool,
}

impl TextureRef {
    pub fn new(image: usize) -> Self {
        TextureRef {
            image,
            wrap: [Wrap::Repeat; 2],
            nearest: false,
        }
    }
}

/// A material in the metallic-roughness model. The legacy materials the plain
/// formats come with (Phong, from an OBJ's `.mtl`) are converted to this when
/// they are read — what `materials.ts` did to every model so that none looks
/// flat next to a glTF.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// Linear RGB and alpha.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear RGB, strength included.
    pub emissive: [f32; 3],
    pub alpha: AlphaMode,
    pub double_sided: bool,
    /// Drawn in its base colour, whatever the light: an artist's choice, which
    /// the viewer keeps.
    pub unlit: bool,
    pub base_color_texture: Option<TextureRef>,
    pub metallic_roughness_texture: Option<TextureRef>,
    pub normal_texture: Option<TextureRef>,
    pub normal_scale: f32,
    pub occlusion_texture: Option<TextureRef>,
    pub occlusion_strength: f32,
    pub emissive_texture: Option<TextureRef>,
    /// `KHR_texture_transform` as the first two rows of a 3×3 matrix, applied
    /// to every texture of the material alike.
    pub uv_transform: [f32; 6],
}

impl Default for Material {
    fn default() -> Self {
        Material {
            name: String::new(),
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 1.0,
            emissive: [0.0; 3],
            alpha: AlphaMode::Opaque,
            double_sided: false,
            unlit: false,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            occlusion_texture: None,
            occlusion_strength: 1.0,
            emissive_texture: None,
            uv_transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        }
    }
}

impl Material {
    /// The clay the plain geometry formats are drawn in: neutral, a little
    /// glossy. With vertex colours it is white, so the colours show as they
    /// are.
    pub fn clay(vertex_colors: bool) -> Material {
        let base = if vertex_colors {
            [1.0; 3]
        } else {
            // 0xb9bec6, as the old viewer had it.
            [0xb9_u8, 0xbe, 0xc6].map(|c| crate::theme::srgb_to_linear(f32::from(c) / 255.0))
        };
        Material {
            name: "default".into(),
            base_color: [base[0], base[1], base[2], 1.0],
            roughness: 0.55,
            metallic: 0.0,
            ..Material::default()
        }
    }

    /// Every texture the material reads, whatever its slot.
    pub fn textures(&self) -> impl Iterator<Item = TextureRef> {
        [
            self.base_color_texture,
            self.metallic_roughness_texture,
            self.normal_texture,
            self.occlusion_texture,
            self.emissive_texture,
        ]
        .into_iter()
        .flatten()
    }
}

/// A decoded texture: 8-bit RGBA, top row first.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What a loader hands to [`Model::new`].
#[derive(Clone, Debug, Default)]
pub struct ModelParts {
    pub meshes: Vec<Mesh>,
    pub instances: Vec<Instance>,
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
    pub clips: Vec<Clip>,
    /// The node graph the clips move; `None` when none of them has anything
    /// to play, and for the formats that have no animation.
    pub rig: Option<Rig>,
    /// Things that did not stop the model from opening but are missing from
    /// it: a texture that could not be decoded, an extension that was
    /// ignored.
    pub warnings: Vec<String>,
}

/// Vertices a framing looks at, at most this many of them.
const HULL_SAMPLES: usize = 6000;

#[derive(Debug)]
pub struct Model {
    /// Unique in the process: what the renderer keys its GPU copy by.
    pub id: u64,
    pub name: String,
    pub format: ModelFormat,
    pub meshes: Vec<Mesh>,
    pub instances: Vec<Instance>,
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
    pub clips: Vec<Clip>,
    pub rig: Option<Rig>,
    pub warnings: Vec<String>,
    /// What the info strip lists and an agent is told, measured in the file's
    /// own coordinates, before any turn upright.
    pub info: ModelInfo,
    /// File to display: a turn upright for the Z-up formats, identity for the
    /// rest.
    pub root: Mat4,
    /// The box around the model as displayed.
    pub bounds: Bounds,
    /// Points on the model as displayed, which framing fits rather than the
    /// box.
    pub hull: Vec<Vec3>,
    /// Half the diagonal of `bounds`, never zero.
    pub radius: f64,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Model {
    pub fn new(name: impl Into<String>, format: ModelFormat, parts: ModelParts) -> Model {
        let ModelParts {
            meshes,
            instances,
            materials,
            images,
            clips,
            rig,
            warnings,
        } = parts;
        let root = if format.plan().z_up {
            Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        } else {
            Mat4::IDENTITY
        };
        let mut model = Model {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            name: name.into(),
            format,
            meshes,
            instances,
            materials,
            images,
            clips,
            rig,
            warnings,
            info: ModelInfo::default(),
            root,
            bounds: Bounds::EMPTY,
            hull: Vec::new(),
            radius: 1e-3,
        };
        model.info = model_info(&model);
        model.bounds = model.bounds_under(root);
        model.hull = model.sample_hull();
        model.radius = radius_of(&model.bounds).max(1e-3);
        model
    }

    /// The box around every instance under `extra * instance.transform`.
    /// Vertex by vertex it is tight around a rotated mesh, which is what
    /// framing needs; past a few million vertices each mesh's own box
    /// transformed is close enough and a hundred times cheaper.
    pub(crate) fn bounds_under(&self, extra: Mat4) -> Bounds {
        let vertices: usize = self
            .instances
            .iter()
            .map(|i| self.meshes[i.mesh].vertices.len())
            .sum();
        let precise = vertices <= crate::stats::PRECISE_BOUNDS_LIMIT;
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        let mut any = false;
        for instance in &self.instances {
            let mesh = &self.meshes[instance.mesh];
            if mesh.vertices.is_empty() {
                continue;
            }
            any = true;
            let matrix = DMat4::from_cols_array(
                &(extra * instance.transform).to_cols_array().map(f64::from),
            );
            if precise {
                for vertex in &mesh.vertices {
                    let p = matrix.transform_point3(DVec3::from(vertex.position.map(f64::from)));
                    if p.is_finite() {
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            } else {
                for corner in mesh.bounds().corners() {
                    let p = matrix.transform_point3(DVec3::from(corner));
                    min = min.min(p);
                    max = max.max(p);
                }
            }
        }
        if any {
            Bounds {
                min: min.to_array(),
                max: max.to_array(),
            }
        } else {
            Bounds::EMPTY
        }
    }

    /// A sample of the model's vertices as displayed, spread over all its
    /// meshes: every vertex of a low-poly asset, a regular stride through a
    /// scan.
    fn sample_hull(&self) -> Vec<Vec3> {
        let total: usize = self
            .instances
            .iter()
            .map(|i| self.meshes[i.mesh].vertices.len())
            .sum();
        if total == 0 {
            return vec![self.bounds.min, self.bounds.max];
        }
        let stride = total.div_ceil(HULL_SAMPLES).max(1);
        let mut points = Vec::with_capacity(total / stride + self.instances.len());
        let mut skip = 0usize;
        for instance in &self.instances {
            let mesh = &self.meshes[instance.mesh];
            let matrix = DMat4::from_cols_array(
                &(self.root * instance.transform)
                    .to_cols_array()
                    .map(f64::from),
            );
            let count = mesh.vertices.len();
            let mut i = skip;
            while i < count {
                let p =
                    matrix.transform_point3(DVec3::from(mesh.vertices[i].position.map(f64::from)));
                if p.is_finite() {
                    points.push(p.to_array());
                }
                i += stride;
            }
            // Carries the stride across meshes, so many small ones are sampled
            // as evenly as one big one.
            skip = (i - count) % stride;
        }
        if points.is_empty() {
            return vec![self.bounds.min, self.bounds.max];
        }
        points
    }

    /// File to display, for one instance.
    pub fn display_transform(&self, instance: &Instance) -> Mat4 {
        self.root * instance.transform
    }

    /// The material a range is drawn with, the clay when it names none.
    pub fn material(&self, index: Option<usize>) -> Option<&Material> {
        index.and_then(|i| self.materials.get(i))
    }

    /// The instances as `at` (a clip and a moment in it) puts them, on the
    /// processor: for tests, and for anyone who wants the numbers a picture
    /// is made from. The renderer does the same on its own buffers.
    pub fn posed(&self, at: Option<Playhead>) -> Vec<PosedInstance> {
        let Some(rig) = &self.rig else {
            return self
                .instances
                .iter()
                .map(|instance| PosedInstance {
                    matrix: self.display_transform(instance),
                    vertices: None,
                })
                .collect();
        };
        let playing = at.and_then(|head| self.clips.get(head.clip).map(|clip| (clip, head.time)));
        let pose = rig.pose(playing);
        self.instances
            .iter()
            .enumerate()
            .map(|(index, instance)| {
                let mesh = &self.meshes[instance.mesh];
                let vertices = mesh.deform.as_ref().zip(rig.attachments().get(index)).map(
                    |(deform, attachment)| {
                        let joints = attachment.skin.map(|skin| rig.joint_matrices(skin, &pose));
                        let mut vertices = mesh.vertices.clone();
                        deform.apply(joints.as_deref(), rig.weights(index, &pose), &mut vertices);
                        vertices
                    },
                );
                PosedInstance {
                    matrix: self.root * rig.instance_matrix(index, &pose),
                    vertices,
                }
            })
            .collect()
    }

    /// The box around the model as displayed in a pose.
    pub fn posed_bounds(&self, at: Option<Playhead>) -> Bounds {
        let posed = self.posed(at);
        let points = self
            .instances
            .iter()
            .zip(&posed)
            .flat_map(|(instance, posed)| {
                let matrix = DMat4::from_cols_array(&posed.matrix.to_cols_array().map(f64::from));
                let vertices = posed
                    .vertices
                    .as_deref()
                    .unwrap_or(&self.meshes[instance.mesh].vertices);
                vertices
                    .iter()
                    .map(move |v| {
                        matrix
                            .transform_point3(DVec3::from(v.position.map(f64::from)))
                            .to_array()
                    })
                    .collect::<Vec<_>>()
            });
        Bounds::around(points)
    }
}

/// An instance in a pose.
#[derive(Clone, Debug)]
pub struct PosedInstance {
    /// File to display: where the instance stands.
    pub matrix: Mat4,
    /// The mesh's vertices as the pose bends them; `None` for a mesh no clip
    /// deforms, which is drawn as the mesh holds it.
    pub vertices: Option<Vec<Vertex>>,
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A unit cube of 24 vertices and 12 triangles, as three.js's
    /// `BoxGeometry` builds it: four vertices to a face so each has its own
    /// normal.
    pub fn cube_mesh(material: Option<usize>) -> Mesh {
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
            ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ];
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (normal, u, v) in faces {
            let base = vertices.len() as u32;
            for (a, b) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                let position = [
                    (normal[0] + u[0] * a + v[0] * b) * 0.5,
                    (normal[1] + u[1] * a + v[1] * b) * 0.5,
                    (normal[2] + u[2] * a + v[2] * b) * 0.5,
                ];
                vertices.push(Vertex {
                    position,
                    normal,
                    uv: [(a + 1.0) / 2.0, (1.0 - b) / 2.0],
                    color: Vertex::WHITE,
                });
            }
            indices.extend([base, base + 2, base + 1, base + 2, base + 3, base + 1]);
        }
        Mesh::new(
            "cube",
            Topology::Triangles,
            vertices,
            Some(indices),
            material,
        )
    }

    pub fn standing(mesh: usize, transform: Mat4) -> Instance {
        Instance { mesh, transform }
    }
}
