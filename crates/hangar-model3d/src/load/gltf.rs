//! glTF 2.0, binary or JSON: the format the preview treats as its own, and
//! the one agents are told to export.
//!
//! Buffers and textures are read through [`Resources`], so a `.gltf` finds its
//! `.bin` and pictures beside it (inside the folders the app allowed) and a
//! `.glb` carries them inside. Meshes keep the node hierarchy's placement;
//! skinned ones are posed once, as the file's nodes stand, and drawn there.

use std::collections::BTreeSet;

use glam::{Mat4, Vec3, Vec4};
use gltf::Gltf;
use gltf::buffer::Source as BufferSource;
use gltf::image::Source as ImageSourceKind;
use gltf::material::AlphaMode as GltfAlpha;
use gltf::mesh::Mode;
use gltf::texture::{MagFilter, WrappingMode};

use super::geometry::{flat_normals_in_place, flat_shaded, strip};
use super::resources::{ImageSource, decode_all};
use super::{Ctx, Resources};
use crate::formats::LoadError;
use crate::model::{
    AlphaMode, Clip, Image, Instance, Material, Mesh, ModelParts, TextureRef, Topology, Vertex,
    Wrap,
};

/// Extensions a file may require that this build cannot honour, and what to
/// call them to whoever has to export the model again.
const UNSUPPORTED: &[(&str, &str)] = &[
    ("KHR_draco_mesh_compression", "Draco geometry compression"),
    ("EXT_meshopt_compression", "meshopt compression"),
    ("KHR_mesh_quantization", "quantized vertex data"),
];

fn unsupported(gltf: &Gltf) -> Option<LoadError> {
    gltf.document.extensions_required().find_map(|required| {
        UNSUPPORTED
            .iter()
            .find(|(name, _)| *name == required)
            .map(|(_, what)| {
                LoadError::Unsupported(format!(
                    "This glTF file needs {what}, which the preview cannot decode: export the \
                     model again without it (compressing only the textures is fine) and show \
                     that file."
                ))
            })
    })
}

/// A file's JSON and binary chunk, checked.
fn open(bytes: &[u8], ctx: &Ctx) -> Result<Gltf, LoadError> {
    match Gltf::from_slice(bytes) {
        Ok(gltf) => match unsupported(&gltf) {
            Some(error) => Err(error),
            None => Ok(gltf),
        },
        Err(error) => {
            // The validator says "unsupported extension" at best; a file that
            // needs Draco deserves to be told so.
            if let Some(reason) = Gltf::from_slice_without_validation(bytes)
                .ok()
                .as_ref()
                .and_then(unsupported)
            {
                return Err(reason);
            }
            Err(ctx.parse_error(match error {
                gltf::Error::Validation(problems) => problems
                    .iter()
                    .take(3)
                    .map(|(path, problem)| format!("{path}: {problem}"))
                    .collect::<Vec<_>>()
                    .join("; "),
                other => other.to_string(),
            }))
        }
    }
}

fn wrap(mode: WrappingMode) -> Wrap {
    match mode {
        WrappingMode::ClampToEdge => Wrap::Clamp,
        WrappingMode::MirroredRepeat => Wrap::Mirror,
        WrappingMode::Repeat => Wrap::Repeat,
    }
}

/// How glTF's `KHR_texture_transform` moves a texture coordinate, as the first
/// two rows of a 3×3 matrix: scale, then rotate, then offset.
fn uv_matrix(offset: [f32; 2], rotation: f32, scale: [f32; 2]) -> [f32; 6] {
    let (s, c) = rotation.sin_cos();
    [
        c * scale[0],
        -s * scale[1],
        offset[0],
        s * scale[0],
        c * scale[1],
        offset[1],
    ]
}

/// What the materials need of the images, and what is left of them once the
/// ones that would not decode are dropped.
struct Pictures {
    /// glTF image index to this model's, `None` for one that failed.
    of_gltf: Vec<Option<usize>>,
    images: Vec<Image>,
}

fn decode_pictures(
    document: &gltf::Document,
    buffers: &[Vec<u8>],
    resources: &Resources<'_>,
    ctx: &Ctx,
    warnings: &mut Vec<String>,
) -> Result<Pictures, LoadError> {
    // Only the pictures a material reads: a file may carry others (lightmaps
    // for an engine) that the preview has no use for and that cost seconds.
    let mut used = BTreeSet::new();
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        let mut each = |texture: Option<gltf::Texture<'_>>| {
            if let Some(texture) = texture {
                used.insert(texture.source().index());
            }
        };
        each(pbr.base_color_texture().map(|t| t.texture()));
        each(pbr.metallic_roughness_texture().map(|t| t.texture()));
        each(material.normal_texture().map(|t| t.texture()));
        each(material.occlusion_texture().map(|t| t.texture()));
        each(material.emissive_texture().map(|t| t.texture()));
        if let Some(sg) = material.pbr_specular_glossiness() {
            each(sg.diffuse_texture().map(|t| t.texture()));
        }
    }
    let used: Vec<usize> = used.into_iter().collect();
    let mut sources = Vec::with_capacity(used.len());
    for &index in &used {
        let image = document
            .images()
            .nth(index)
            .ok_or_else(|| ctx.parse_error(format!("image {index} does not exist")))?;
        sources.push(match image.source() {
            ImageSourceKind::View { view, .. } => {
                let buffer = &buffers[view.buffer().index()];
                let range = view.offset()..view.offset() + view.length();
                ImageSource::Bytes(
                    buffer
                        .get(range)
                        .ok_or_else(|| {
                            ctx.parse_error(format!("image {index} is outside its buffer"))
                        })?
                        .to_vec(),
                )
            }
            ImageSourceKind::Uri { uri, .. } => ImageSource::Uri(uri.to_string()),
        });
    }
    let decoded = decode_all(resources, sources, ctx.cancel);
    ctx.cancel.check()?;

    let mut of_gltf = vec![None; document.images().count()];
    let mut images = Vec::new();
    for (index, outcome) in used.into_iter().zip(decoded) {
        match outcome {
            Ok(image) => {
                of_gltf[index] = Some(images.len());
                images.push(image);
            }
            Err(reason) => {
                let name = document
                    .images()
                    .nth(index)
                    .and_then(|i| i.name().map(str::to_string))
                    .unwrap_or_else(|| format!("image {index}"));
                warnings.push(format!("The texture {name} could not be read: {reason}."));
            }
        }
    }
    Ok(Pictures { of_gltf, images })
}

fn materials(
    document: &gltf::Document,
    pictures: &Pictures,
    warnings: &mut Vec<String>,
) -> Vec<Material> {
    let mut other_uv_set = false;
    let converted = document
        .materials()
        .map(|source| {
            let pbr = source.pbr_metallic_roughness();
            let mut transform: Option<[f32; 6]> = None;
            let mut texture =
                |texture: gltf::Texture<'_>,
                 tex_coord: u32,
                 moved: Option<gltf::texture::TextureTransform<'_>>| {
                    if tex_coord != 0 {
                        other_uv_set = true;
                    }
                    if transform.is_none() {
                        transform = moved.map(|t| uv_matrix(t.offset(), t.rotation(), t.scale()));
                    }
                    let image = pictures
                        .of_gltf
                        .get(texture.source().index())
                        .copied()
                        .flatten()?;
                    let sampler = texture.sampler();
                    Some(TextureRef {
                        image,
                        wrap: [wrap(sampler.wrap_s()), wrap(sampler.wrap_t())],
                        nearest: sampler.mag_filter() == Some(MagFilter::Nearest),
                    })
                };
            let mut info = |info: Option<gltf::texture::Info<'_>>| {
                info.and_then(|i| texture(i.texture(), i.tex_coord(), i.texture_transform()))
            };

            let mut material = Material {
                name: source.name().unwrap_or_default().to_string(),
                base_color: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                base_color_texture: info(pbr.base_color_texture()),
                metallic_roughness_texture: info(pbr.metallic_roughness_texture()),
                emissive_texture: info(source.emissive_texture()),
                ..Material::default()
            };
            if let Some(sg) = source.pbr_specular_glossiness() {
                // The older specular-glossiness model, approximated: the
                // diffuse colour is the base colour, glossiness the inverse
                // of roughness, and nothing is metal.
                material.base_color = sg.diffuse_factor();
                material.base_color_texture = info(sg.diffuse_texture());
                material.metallic = 0.0;
                material.roughness = 1.0 - sg.glossiness_factor();
                material.metallic_roughness_texture = None;
            }
            if let Some(normal) = source.normal_texture() {
                material.normal_texture = texture(normal.texture(), normal.tex_coord(), None);
                material.normal_scale = normal.scale();
            }
            if let Some(occlusion) = source.occlusion_texture() {
                material.occlusion_texture =
                    texture(occlusion.texture(), occlusion.tex_coord(), None);
                material.occlusion_strength = occlusion.strength();
            }
            let strength = source.emissive_strength().unwrap_or(1.0);
            material.emissive = source.emissive_factor().map(|c| c * strength);
            material.alpha = match source.alpha_mode() {
                GltfAlpha::Opaque => AlphaMode::Opaque,
                GltfAlpha::Mask => AlphaMode::Mask(source.alpha_cutoff().unwrap_or(0.5)),
                GltfAlpha::Blend => AlphaMode::Blend,
            };
            material.double_sided = source.double_sided();
            material.unlit = source.unlit();
            if let Some(uv) = transform {
                material.uv_transform = uv;
            }
            material
        })
        .collect();
    if other_uv_set {
        warnings.push(
            "Some textures use a second set of texture coordinates, which the preview does not read; they are drawn with the first.".into(),
        );
    }
    converted
}

/// Every node's matrix in the file's coordinates, from the roots down.
fn world_matrices(document: &gltf::Document) -> Vec<Mat4> {
    let count = document.nodes().count();
    let mut world = vec![Mat4::IDENTITY; count];
    let mut has_parent = vec![false; count];
    for node in document.nodes() {
        for child in node.children() {
            has_parent[child.index()] = true;
        }
    }
    let mut stack: Vec<(usize, Mat4)> = document
        .nodes()
        .filter(|n| !has_parent[n.index()])
        .map(|n| (n.index(), Mat4::IDENTITY))
        .collect();
    // A file with a cycle in its nodes is invalid, but not a reason to loop.
    let mut budget = count * 2 + 16;
    while let Some((index, parent)) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let node = document
            .nodes()
            .nth(index)
            .expect("node indices are in range");
        let matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        world[index] = matrix;
        stack.extend(node.children().map(|child| (child.index(), matrix)));
    }
    world
}

/// The matrices that pose a skin: joint in the file's coordinates, times the
/// inverse of the pose the mesh was bound in.
fn joint_matrices(skin: &gltf::Skin<'_>, buffers: &[Vec<u8>], world: &[Mat4]) -> Vec<Mat4> {
    let bind: Vec<Mat4> = skin
        .reader(|b| buffers.get(b.index()).map(Vec::as_slice))
        .read_inverse_bind_matrices()
        .map(|matrices| matrices.map(|m| Mat4::from_cols_array_2d(&m)).collect())
        .unwrap_or_default();
    skin.joints()
        .enumerate()
        .map(|(i, joint)| world[joint.index()] * bind.get(i).copied().unwrap_or(Mat4::IDENTITY))
        .collect()
}

/// Moves each vertex to where its joints put it: the blend of up to four joint
/// matrices, weighted. A vertex nothing weighs on stays where it is.
fn skin_vertices(
    vertices: &mut [Vertex],
    pose: &[Mat4],
    joints: &[[u16; 4]],
    weights: &[[f32; 4]],
) {
    for ((vertex, joints), weights) in vertices.iter_mut().zip(joints).zip(weights) {
        let mut blended = Mat4::ZERO;
        let mut total = 0.0;
        for (&joint, &weight) in joints.iter().zip(weights) {
            if weight > 0.0
                && let Some(matrix) = pose.get(usize::from(joint))
            {
                blended += *matrix * weight;
                total += weight;
            }
        }
        if total > 1e-6 {
            blended *= 1.0 / total;
            let [x, y, z] = vertex.position;
            let moved = blended * Vec4::new(x, y, z, 1.0);
            vertex.position = [moved.x, moved.y, moved.z];
            let normal = blended
                .transform_vector3(Vec3::from(vertex.normal))
                .normalize_or_zero();
            vertex.normal = normal.to_array();
        }
    }
}

/// One primitive as a mesh, or `None` for one the preview cannot draw (no
/// positions, indices past the end of the vertices).
fn primitive_mesh(
    name: &str,
    primitive: &gltf::Primitive<'_>,
    buffers: &[Vec<u8>],
    material: Option<usize>,
    pose: Option<&[Mat4]>,
    warnings: &mut Vec<String>,
) -> Option<Mesh> {
    let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
    let positions: Vec<[f32; 3]> = reader.read_positions()?.collect();
    let count = positions.len();
    if count == 0 || count > u32::MAX as usize {
        return None;
    }
    let normals: Option<Vec<[f32; 3]>> = reader
        .read_normals()
        .map(Iterator::collect)
        .filter(|n: &Vec<_>| n.len() == count);
    let uvs: Option<Vec<[f32; 2]>> = reader
        .read_tex_coords(0)
        .map(|t| t.into_f32().collect())
        .filter(|t: &Vec<_>| t.len() == count);
    let colors: Option<Vec<[u8; 4]>> = reader
        .read_colors(0)
        .map(|c| c.into_rgba_u8().collect())
        .filter(|c: &Vec<_>| c.len() == count);
    let mut vertices: Vec<Vertex> = (0..count)
        .map(|i| Vertex {
            position: positions[i],
            normal: normals.as_ref().map_or([0.0, 1.0, 0.0], |n| n[i]),
            uv: uvs.as_ref().map_or([0.0; 2], |t| t[i]),
            color: colors.as_ref().map_or(Vertex::WHITE, |c| c[i]),
        })
        .collect();

    if let Some(pose) = pose {
        let joints: Option<Vec<[u16; 4]>> = reader.read_joints(0).map(|j| j.into_u16().collect());
        let weights: Option<Vec<[f32; 4]>> = reader.read_weights(0).map(|w| w.into_f32().collect());
        if let (Some(joints), Some(weights)) = (joints, weights)
            && joints.len() == count
            && weights.len() == count
        {
            skin_vertices(&mut vertices, pose, &joints, &weights);
        }
    }

    let given: Option<Vec<u32>> = reader.read_indices().map(|i| i.into_u32().collect());
    if let Some(indices) = &given
        && indices.iter().any(|&i| i as usize >= count)
    {
        warnings.push(format!(
            "A mesh of {name} was left out: its indices name vertices that are not there."
        ));
        return None;
    }
    // The element list in the order the mode reads it, whether or not the
    // file gave indices.
    let sequence = || given.clone().unwrap_or_else(|| (0..count as u32).collect());
    let (topology, indices): (Topology, Option<Vec<u32>>) = match primitive.mode() {
        Mode::Triangles => (Topology::Triangles, given.clone()),
        Mode::TriangleStrip => {
            let mut out = Vec::new();
            strip(&sequence(), &mut out);
            (Topology::Triangles, Some(out))
        }
        Mode::TriangleFan => {
            let seq = sequence();
            let mut out = Vec::new();
            for pair in seq.windows(2).skip(1) {
                out.extend([seq[0], pair[0], pair[1]]);
            }
            (Topology::Triangles, Some(out))
        }
        Mode::Lines => (Topology::Lines, Some(sequence())),
        Mode::LineStrip | Mode::LineLoop => {
            let seq = sequence();
            let mut out: Vec<u32> = seq.windows(2).flat_map(|w| [w[0], w[1]]).collect();
            if primitive.mode() == Mode::LineLoop && seq.len() > 2 {
                out.extend([seq[seq.len() - 1], seq[0]]);
            }
            (Topology::Lines, Some(out))
        }
        Mode::Points => (Topology::Points, given.clone()),
    };

    if topology == Topology::Triangles && normals.is_none() {
        // Faceted, which is what a glTF without normals asks for (the spec:
        // flat shading) and what three.js drew.
        return Some(match indices {
            Some(indices) => Mesh::new(
                name,
                topology,
                flat_shaded(&vertices, &indices),
                None,
                material,
            ),
            None => {
                flat_normals_in_place(&mut vertices);
                Mesh::new(name, topology, vertices, None, material)
            }
        });
    }
    Some(Mesh::new(name, topology, vertices, indices, material))
}

pub(super) fn read(bytes: &[u8], ctx: &Ctx) -> Result<ModelParts, LoadError> {
    let Gltf { document, blob } = open(bytes, ctx)?;
    let resources = ctx.resources();
    let mut warnings = Vec::new();

    let mut blob = blob;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for buffer in document.buffers() {
        ctx.cancel.check()?;
        let data = match buffer.source() {
            BufferSource::Bin => blob
                .take()
                .ok_or_else(|| ctx.parse_error("it refers to a binary chunk it does not have"))?,
            BufferSource::Uri(uri) => resources.read(uri).map_err(|reason| {
                LoadError::Read(format!(
                    "A buffer of this glTF file could not be read: {reason}."
                ))
            })?,
        };
        if data.len() < buffer.length() {
            return Err(ctx.parse_error(format!(
                "buffer {} is {} bytes, shorter than the {} the file says",
                buffer.index(),
                data.len(),
                buffer.length()
            )));
        }
        buffers.push(data);
    }

    let pictures = decode_pictures(&document, &buffers, &resources, ctx, &mut warnings)?;
    let mut parts = ModelParts {
        materials: materials(&document, &pictures, &mut warnings),
        images: pictures.images,
        ..ModelParts::default()
    };
    let world = world_matrices(&document);

    // The default material a primitive gets when it names none: glTF's own,
    // white, fully metallic and rough. Made on first use.
    let mut default_material: Option<usize> = None;
    let mut material_of = |parts: &mut ModelParts, primitive: &gltf::Primitive<'_>| {
        primitive.material().index().or_else(|| {
            Some(*default_material.get_or_insert_with(|| {
                parts.materials.push(Material {
                    name: "default".into(),
                    metallic: 1.0,
                    roughness: 1.0,
                    ..Material::default()
                });
                parts.materials.len() - 1
            }))
        })
    };

    // A mesh several nodes use is converted once.
    let mut converted: Vec<Option<Vec<usize>>> = vec![None; document.meshes().count()];
    let roots: Vec<gltf::Node<'_>> = match document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        Some(scene) => scene.nodes().collect(),
        None => {
            let mut child = vec![false; world.len()];
            for node in document.nodes() {
                for c in node.children() {
                    child[c.index()] = true;
                }
            }
            document.nodes().filter(|n| !child[n.index()]).collect()
        }
    };
    let mut stack: Vec<_> = roots.into_iter().rev().collect();
    let mut budget = world.len() * 2 + 16;
    while let Some(node) = stack.pop() {
        ctx.cancel.check()?;
        if budget == 0 {
            break;
        }
        budget -= 1;
        // Reversed, so the file's order of siblings is the order they are
        // drawn in.
        let children: Vec<_> = node.children().collect();
        stack.extend(children.into_iter().rev());
        let Some(mesh) = node.mesh() else { continue };
        let label = mesh.name().or(node.name()).unwrap_or("mesh").to_string();

        if let Some(skin) = node.skin() {
            // Posed once, as the nodes stand: the vertices come out in the
            // file's coordinates, so the mesh stands at the origin.
            let pose = joint_matrices(&skin, &buffers, &world);
            for primitive in mesh.primitives() {
                let material = material_of(&mut parts, &primitive);
                if let Some(built) = primitive_mesh(
                    &label,
                    &primitive,
                    &buffers,
                    material,
                    Some(&pose),
                    &mut warnings,
                ) {
                    parts.meshes.push(built);
                    parts.instances.push(Instance {
                        mesh: parts.meshes.len() - 1,
                        transform: Mat4::IDENTITY,
                    });
                }
            }
            continue;
        }

        if converted[mesh.index()].is_none() {
            let mut built_here = Vec::new();
            for primitive in mesh.primitives() {
                let material = material_of(&mut parts, &primitive);
                if let Some(built) =
                    primitive_mesh(&label, &primitive, &buffers, material, None, &mut warnings)
                {
                    parts.meshes.push(built);
                    built_here.push(parts.meshes.len() - 1);
                }
            }
            converted[mesh.index()] = Some(built_here);
        }
        for &mesh_index in converted[mesh.index()].as_deref().unwrap_or_default() {
            parts.instances.push(Instance {
                mesh: mesh_index,
                transform: world[node.index()],
            });
        }
    }

    for animation in document.animations() {
        let mut duration = 0.0f32;
        for channel in animation.channels() {
            let reader = channel.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            if let Some(times) = reader.read_inputs() {
                duration = times.fold(duration, f32::max);
            }
        }
        parts.clips.push(Clip {
            name: animation.name().unwrap_or_default().to_string(),
            duration,
        });
    }
    parts.warnings = warnings;
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::ModelFormat;
    use crate::load::{Cancel, LoadOptions, load, load_bytes};
    use crate::model::Model;
    use crate::samples;

    fn model_from(bytes: &[u8], format: ModelFormat) -> Result<Model, LoadError> {
        load_bytes(
            "test",
            format,
            bytes,
            std::path::Path::new("."),
            &LoadOptions::default(),
            &Cancel::default(),
            &mut |_| {},
        )
    }

    #[test]
    fn reads_a_cube_from_a_glb() {
        let model = model_from(&samples::cube_glb(), ModelFormat::Glb).unwrap();
        assert_eq!(model.info.triangles, 12);
        assert_eq!(model.info.vertices, 24);
        assert_eq!(model.info.meshes, 1);
        assert_eq!(model.info.materials, 1);
        assert_eq!(model.info.size, [1.0, 1.0, 1.0]);
        assert_eq!(model.info.center, [0.0, 0.0, 0.0]);
        let material = &model.materials[0];
        assert_eq!(material.name, "red");
        assert_eq!(material.base_color, [0.8, 0.1, 0.1, 1.0]);
        assert_eq!(material.roughness, 0.5);
    }

    #[test]
    fn reads_alpha_modes_emissive_strength_unlit_and_double_sidedness() {
        let material = r#"{
            "alphaMode": "MASK", "alphaCutoff": 0.3, "doubleSided": true,
            "emissiveFactor": [1.0, 0.5, 0.0],
            "extensions": {"KHR_materials_emissive_strength": {"emissiveStrength": 2.0}, "KHR_materials_unlit": {}},
            "pbrMetallicRoughness": {"baseColorFactor": [1.0, 1.0, 1.0, 0.5], "metallicFactor": 0.25, "roughnessFactor": 0.75}
        }"#;
        let model = model_from(&samples::cube_glb_with(material), ModelFormat::Glb).unwrap();
        let m = &model.materials[0];
        assert_eq!(m.alpha, AlphaMode::Mask(0.3));
        assert!(m.double_sided && m.unlit);
        assert_eq!(m.emissive, [2.0, 1.0, 0.0]);
        assert_eq!((m.metallic, m.roughness), (0.25, 0.75));

        let blend = r#"{"alphaMode": "BLEND", "pbrMetallicRoughness": {}}"#;
        let model = model_from(&samples::cube_glb_with(blend), ModelFormat::Glb).unwrap();
        assert_eq!(model.materials[0].alpha, AlphaMode::Blend);
        // glTF's own defaults: white, fully metallic and rough.
        assert_eq!(model.materials[0].base_color, [1.0; 4]);
        assert_eq!(model.materials[0].metallic, 1.0);
    }

    #[test]
    fn places_a_mesh_every_node_uses_and_counts_each_placement() {
        let model = model_from(&samples::textured_cube_glb(), ModelFormat::Glb).unwrap();
        assert_eq!(model.meshes.len(), 1);
        assert_eq!(model.instances.len(), 2);
        assert_eq!(model.info.meshes, 2);
        assert_eq!(model.info.triangles, 24);
        // One cube a unit left, one a unit right and twice as tall.
        assert_eq!(model.info.size, [3.0, 2.0, 1.0]);
        assert_eq!(model.info.center, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn decodes_the_textures_inside_the_file() {
        let model = model_from(&samples::textured_cube_glb(), ModelFormat::Glb).unwrap();
        assert_eq!(model.images.len(), 2);
        let material = &model.materials[0];
        let base = material.base_color_texture.unwrap();
        assert!(base.nearest);
        assert_eq!(base.wrap, [Wrap::Repeat, Wrap::Clamp]);
        assert_eq!(model.images[base.image].width, 2);
        assert!(material.normal_texture.is_some());
        assert_eq!(material.normal_scale, 1.0);
        assert!(model.warnings.is_empty(), "{:?}", model.warnings);
    }

    #[test]
    fn reads_a_gltf_with_its_buffer_inline() {
        let model = model_from(
            samples::cube_gltf_with_data_uris().as_bytes(),
            ModelFormat::Gltf,
        )
        .unwrap();
        assert_eq!(model.info.triangles, 12);
        // No material in the file: the default one stands in.
        assert_eq!(model.info.materials, 1);
        assert_eq!(model.materials[0].metallic, 1.0);
    }

    #[test]
    fn reads_the_files_beside_a_gltf() {
        let dir = tempfile::tempdir().unwrap();
        let glb = samples::cube_glb();
        // Split the GLB's chunks into a .gltf and a .bin.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json = std::str::from_utf8(&glb[20..20 + json_len])
            .unwrap()
            .trim_end();
        let bin = &glb[20 + json_len + 8..];
        let json = json.replace(
            "\"buffers\": [{\"byteLength\": 840}]",
            "\"buffers\": [{\"byteLength\": 840, \"uri\": \"geometry%20data.bin\"}]",
        );
        assert!(json.contains("geometry%20data.bin"));
        std::fs::write(dir.path().join("geometry data.bin"), bin).unwrap();
        std::fs::write(dir.path().join("cube.gltf"), json).unwrap();
        let model = load(dir.path().join("cube.gltf")).unwrap();
        assert_eq!(model.info.triangles, 12);
    }

    #[test]
    fn says_which_buffer_is_missing() {
        let json =
            r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"missing.bin"}]}"#;
        let error = model_from(json.as_bytes(), ModelFormat::Gltf).unwrap_err();
        assert!(error.to_string().contains("missing.bin"), "{error}");
    }

    #[test]
    fn tells_the_exporter_when_a_file_needs_draco() {
        let json = r#"{"asset":{"version":"2.0"},"extensionsUsed":["KHR_draco_mesh_compression"],"extensionsRequired":["KHR_draco_mesh_compression"]}"#;
        let error = model_from(json.as_bytes(), ModelFormat::Gltf).unwrap_err();
        assert!(matches!(error, LoadError::Unsupported(_)));
        assert!(error.to_string().contains("Draco"), "{error}");
    }

    #[test]
    fn says_what_is_wrong_with_a_file_that_is_not_a_gltf() {
        let error = model_from(b"definitely not json", ModelFormat::Gltf).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("This GLTF file could not be read"),
            "{error}"
        );
        let truncated = samples::cube_glb();
        let error = model_from(&truncated[..100], ModelFormat::Glb).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("This GLB file could not be read"),
            "{error}"
        );
    }

    #[test]
    fn leaves_out_a_mesh_whose_indices_point_past_its_vertices() {
        let mut glb = samples::cube_glb();
        // The last index of the 36, in the binary chunk's tail, made huge.
        let end = glb.len();
        glb[end - 2..end].copy_from_slice(&5000u16.to_le_bytes());
        let model = model_from(&glb, ModelFormat::Glb).unwrap();
        assert_eq!(model.info.meshes, 0);
        assert!(
            model.warnings.iter().any(|w| w.contains("indices")),
            "{:?}",
            model.warnings
        );
    }

    #[test]
    fn computes_faceted_normals_for_a_mesh_without_any() {
        let (vertices, indices) = samples::cube_vertices();
        let mut bin = Vec::new();
        for v in &vertices {
            v.position.iter().for_each(|c| bin.extend(c.to_le_bytes()));
        }
        for i in &indices {
            bin.extend((*i as u16).to_le_bytes());
        }
        let json = r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],
          "meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1}]}],
          "accessors":[{"bufferView":0,"componentType":5126,"count":24,"type":"VEC3","min":[-0.5,-0.5,-0.5],"max":[0.5,0.5,0.5]},
                       {"bufferView":1,"componentType":5123,"count":36,"type":"SCALAR"}],
          "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":288},{"buffer":0,"byteOffset":288,"byteLength":72}],
          "buffers":[{"byteLength":360}]}"#;
        let model = model_from(&samples::glb(json, &bin), ModelFormat::Glb).unwrap();
        let mesh = &model.meshes[0];
        // Unwelded: three vertices to a triangle, each facet its own normal.
        assert_eq!(mesh.vertices.len(), 36);
        assert!(mesh.indices.is_none());
        for triangle in mesh.vertices.as_chunks::<3>().0 {
            let centroid =
                [0, 1, 2].map(|a| triangle.iter().map(|v| v.position[a]).sum::<f32>() / 3.0);
            let n = triangle[0].normal;
            assert!(n[0] * centroid[0] + n[1] * centroid[1] + n[2] * centroid[2] > 0.0);
        }
    }

    #[test]
    fn matches_the_texture_transform_of_the_spec() {
        let m = uv_matrix([0.5, 0.25], 0.0, [2.0, 3.0]);
        assert_eq!(m, [2.0, -0.0, 0.5, 0.0, 3.0, 0.25]);
        let quarter = uv_matrix([0.0; 2], std::f32::consts::FRAC_PI_2, [1.0; 2]);
        assert!((quarter[1] + 1.0).abs() < 1e-6 && (quarter[3] - 1.0).abs() < 1e-6);
    }
}
