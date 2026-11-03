//! FBX, the binary 7.x flavour every current exporter writes (Blender, Maya,
//! 3ds Max, Unity, Unreal). Meshes with their normals, texture coordinates,
//! vertex colours and per-face materials; the nodes' transforms, pivots and
//! rotation orders; materials as colours and textures, embedded or beside the
//! file.
//!
//! Not read: the old ASCII flavour (rare, and `fbxcel` does not parse it),
//! skinning and animation (a skinned mesh is shown as it is bound), and the
//! file's own unit and up axis, which three.js's loader did not apply either.

use std::collections::HashMap;
use std::io::Cursor;

use fbxcel_dom::any::AnyDocument;
use fbxcel_dom::v7400::data::mesh::layer::TypedLayerElementHandle;
use fbxcel_dom::v7400::object::model::TypedModelHandle;
use fbxcel_dom::v7400::object::property::loaders::{F64Arr3Loader, PrimitiveLoader};
use fbxcel_dom::v7400::object::{ObjectHandle, TypedObjectHandle, material, texture};
use glam::{DMat4, DQuat, DVec3, Mat4};

use super::geometry::flat_normals_in_place;
use super::obj::roughness_from_shininess;
use super::resources::decode_image;
use super::{Ctx, Resources};
use crate::formats::LoadError;
use crate::model::{
    AlphaMode, Image, Instance, Material, Mesh, ModelParts, TextureRef, Topology, Vertex,
};
use crate::theme::srgb_to_linear;

/// A vector property of an object, or `default` when the file does not set it.
fn vector(object: &ObjectHandle<'_>, name: &str, default: [f64; 3]) -> DVec3 {
    let properties = object.properties_by_native_typename("FbxNode");
    properties
        .get_property(name)
        .and_then(|property| property.load_value(F64Arr3Loader).ok())
        .map_or(DVec3::from(default), DVec3::from)
}

fn number(object: &ObjectHandle<'_>, name: &str, default: i32) -> i32 {
    let properties = object.properties_by_native_typename("FbxNode");
    properties
        .get_property(name)
        .and_then(|property| property.load_value(PrimitiveLoader::<i32>::new()).ok())
        .unwrap_or(default)
}

/// A rotation of Euler angles in degrees, turned about the axes in the order
/// the node's `RotationOrder` names: "XYZ" turns about X first.
fn rotation(degrees: DVec3, order: i32) -> DQuat {
    let [x, y, z] = [degrees.x, degrees.y, degrees.z].map(f64::to_radians);
    let axis = |axis: usize| match axis {
        0 => DQuat::from_rotation_x(x),
        1 => DQuat::from_rotation_y(y),
        _ => DQuat::from_rotation_z(z),
    };
    // The order of the three turns; the last one named is applied last.
    let turns: [usize; 3] = match order {
        1 => [0, 2, 1],
        2 => [1, 2, 0],
        3 => [1, 0, 2],
        4 => [2, 0, 1],
        5 => [2, 1, 0],
        _ => [0, 1, 2],
    };
    axis(turns[2]) * axis(turns[1]) * axis(turns[0])
}

/// A node's matrix in its parent: FBX's
/// `T · Roff · Rp · Rpre · R · Rpost⁻¹ · Rp⁻¹ · Soff · Sp · S · Sp⁻¹`.
fn local_matrix(node: &ObjectHandle<'_>) -> DMat4 {
    let translation = vector(node, "Lcl Translation", [0.0; 3]);
    let order = number(node, "RotationOrder", 0);
    let rotate = rotation(vector(node, "Lcl Rotation", [0.0; 3]), order);
    let scale = vector(node, "Lcl Scaling", [1.0; 3]);
    let rotation_offset = vector(node, "RotationOffset", [0.0; 3]);
    let rotation_pivot = vector(node, "RotationPivot", [0.0; 3]);
    let scaling_offset = vector(node, "ScalingOffset", [0.0; 3]);
    let scaling_pivot = vector(node, "ScalingPivot", [0.0; 3]);
    let pre = rotation(vector(node, "PreRotation", [0.0; 3]), 0);
    let post = rotation(vector(node, "PostRotation", [0.0; 3]), 0);
    DMat4::from_translation(translation)
        * DMat4::from_translation(rotation_offset)
        * DMat4::from_translation(rotation_pivot)
        * DMat4::from_quat(pre)
        * DMat4::from_quat(rotate)
        * DMat4::from_quat(post.inverse())
        * DMat4::from_translation(-rotation_pivot)
        * DMat4::from_translation(scaling_offset)
        * DMat4::from_translation(scaling_pivot)
        * DMat4::from_scale(scale)
        * DMat4::from_translation(-scaling_pivot)
}

/// The matrix of the geometry inside its node: it moves the mesh, not the
/// node's children.
fn geometric_matrix(node: &ObjectHandle<'_>) -> DMat4 {
    let t = vector(node, "GeometricTranslation", [0.0; 3]);
    let r = vector(node, "GeometricRotation", [0.0; 3]);
    let s = vector(node, "GeometricScaling", [1.0; 3]);
    DMat4::from_translation(t) * DMat4::from_quat(rotation(r, 0)) * DMat4::from_scale(s)
}

/// A node's matrix in the file's coordinates, from the root down.
fn world_matrix(model: &TypedModelHandle<'_>) -> DMat4 {
    let mut matrix = DMat4::IDENTITY;
    let mut current = Some(*model);
    // A file whose nodes loop is invalid, but not a reason to loop.
    for _ in 0..4096 {
        let Some(node) = current else { break };
        matrix = local_matrix(node_object(&node)) * matrix;
        current = node_parent(&node);
    }
    matrix
}

fn node_object<'a, 'b>(model: &'b TypedModelHandle<'a>) -> &'b ObjectHandle<'a> {
    model
}

fn node_parent<'a>(model: &TypedModelHandle<'a>) -> Option<TypedModelHandle<'a>> {
    model.parent_model()
}

/// A material's textures, read once: the picture's pixels, or why not.
struct Pictures<'a> {
    resources: Resources<'a>,
    images: Vec<Image>,
    by_name: HashMap<String, Option<usize>>,
    warnings: Vec<String>,
}

impl Pictures<'_> {
    /// The image behind a texture node: embedded in the file, or a file beside
    /// it, named relative to the model or by its bare name.
    fn of(&mut self, texture: &texture::TextureHandle<'_>) -> Option<TextureRef> {
        let clip = texture.video_clip()?;
        let name = clip.relative_filename().unwrap_or_default().to_string();
        let key = format!("{}#{}", name, clip.content().map_or(0, <[u8]>::len));
        if let Some(found) = self.by_name.get(&key) {
            return found.map(TextureRef::new);
        }
        let decoded = match clip.content().filter(|bytes| !bytes.is_empty()) {
            Some(bytes) => decode_image(bytes),
            None => {
                let normalized = name.replace('\\', "/");
                let bare = normalized.rsplit('/').next().unwrap_or(&normalized);
                self.resources
                    .read(&normalized)
                    .or_else(|_| self.resources.read(bare))
                    .and_then(|bytes| decode_image(&bytes))
            }
        };
        let index = match decoded {
            Ok(image) => {
                self.images.push(image);
                Some(self.images.len() - 1)
            }
            Err(reason) => {
                self.warnings
                    .push(format!("The texture {name} could not be read: {reason}."));
                None
            }
        };
        self.by_name.insert(key, index);
        index.map(TextureRef::new)
    }
}

fn linear(color: [f64; 3]) -> [f32; 3] {
    color.map(|c| srgb_to_linear(c as f32))
}

fn convert_material(
    source: &material::MaterialHandle<'_>,
    pictures: &mut Pictures<'_>,
) -> Material {
    let properties = source.properties();
    let diffuse = properties
        .diffuse_color_or_default()
        .map_or([0.8; 3], |c| [c.r, c.g, c.b]);
    let diffuse_factor = properties.diffuse_factor_or_default().unwrap_or(1.0);
    let emissive = properties
        .emissive_color_or_default()
        .map_or([0.0; 3], |c| [c.r, c.g, c.b]);
    let emissive_factor = properties.emissive_factor_or_default().unwrap_or(1.0);
    let shininess = properties.shininess_or_default().unwrap_or(30.0);
    // FBX says how transparent a material is, not how opaque.
    let opacity =
        (1.0 - properties.transparency_factor_or_default().unwrap_or(0.0)).clamp(0.0, 1.0);
    let base = linear(diffuse.map(|c| c * diffuse_factor));
    Material {
        name: source.name().unwrap_or_default().to_string(),
        base_color: [base[0], base[1], base[2], opacity as f32],
        metallic: 0.0,
        roughness: roughness_from_shininess(shininess as f32),
        emissive: linear(emissive.map(|c| c * emissive_factor)),
        alpha: if opacity < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        base_color_texture: source
            .diffuse_texture()
            .and_then(|texture| pictures.of(&texture)),
        normal_texture: source
            .normal_map_texture()
            .and_then(|texture| pictures.of(&texture)),
        ..Material::default()
    }
}

pub(super) fn read(bytes: &[u8], ctx: &Ctx) -> Result<ModelParts, LoadError> {
    if bytes.starts_with(b"; FBX") {
        return Err(LoadError::Unsupported(
            "This FBX file is the old text flavour, which the preview cannot read: export it \
             again as a binary FBX, or as .glb, and show that file."
                .into(),
        ));
    }
    let document = match AnyDocument::from_seekable_reader(Cursor::new(bytes)) {
        Ok(AnyDocument::V7400(_, document)) => document,
        Ok(_) => {
            return Err(LoadError::Unsupported(
                "This FBX file is older than version 7, which the preview cannot read: export it \
                 again as a newer FBX, or as .glb, and show that file."
                    .into(),
            ));
        }
        Err(error) => return Err(ctx.parse_error(error)),
    };

    let mut pictures = Pictures {
        resources: ctx.resources(),
        images: Vec::new(),
        by_name: HashMap::new(),
        warnings: Vec::new(),
    };
    let mut parts = ModelParts::default();
    // Materials are converted once and shared by every mesh that uses them.
    let mut material_of: HashMap<i64, usize> = HashMap::new();

    for object in document.objects() {
        ctx.cancel.check()?;
        let TypedObjectHandle::Model(model @ TypedModelHandle::Mesh(_)) = object.get_typed() else {
            continue;
        };
        let TypedModelHandle::Mesh(mesh_model) = &model else {
            continue;
        };
        let geometry = match mesh_model.geometry() {
            Ok(geometry) => geometry,
            Err(error) => {
                pictures.warnings.push(format!(
                    "The mesh {} was left out: {error}.",
                    object.name().unwrap_or("")
                ));
                continue;
            }
        };
        let polygons = match geometry.polygon_vertices() {
            Ok(polygons) => polygons,
            Err(error) => {
                pictures.warnings.push(format!(
                    "The mesh {} was left out: {error}.",
                    object.name().unwrap_or("")
                ));
                continue;
            }
        };
        // Polygons are cut into fans around their first corner: right for the
        // convex faces exporters write, and what three.js's loader did too.
        let triangles = polygons
            .triangulate_each(|_, corners, out| {
                for pair in corners.windows(2).skip(1) {
                    out.push([corners[0], pair[0], pair[1]]);
                }
                Ok(())
            })
            .map_err(|error| ctx.parse_error(error))?;

        let (mut normals, mut uvs, mut slots, mut colors) = (None, None, None, None);
        for layer in geometry.layers() {
            for entry in layer.layer_element_entries() {
                match entry.typed_layer_element() {
                    Ok(TypedLayerElementHandle::Normal(h)) if normals.is_none() => {
                        normals = h.normals().ok();
                    }
                    Ok(TypedLayerElementHandle::Uv(h)) if uvs.is_none() => uvs = h.uv().ok(),
                    Ok(TypedLayerElementHandle::Material(h)) if slots.is_none() => {
                        slots = h.materials().ok();
                    }
                    Ok(TypedLayerElementHandle::Color(h)) if colors.is_none() => {
                        colors = h.color().ok();
                    }
                    _ => {}
                }
            }
        }

        // Vertices unwelded, three to a triangle, with the material slot each
        // triangle uses.
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut slot_of_triangle: Vec<u32> = Vec::new();
        for (i, tri_vi) in triangles.triangle_vertex_indices().enumerate() {
            let Some(point) = triangles.control_point(tri_vi) else {
                continue;
            };
            let mut vertex = Vertex::at([point.x as f32, point.y as f32, point.z as f32]);
            if let Some(normals) = &normals
                && let Ok(n) = normals.normal(&triangles, tri_vi)
            {
                vertex.normal = [n.x as f32, n.y as f32, n.z as f32];
            }
            if let Some(uvs) = &uvs
                && let Ok(uv) = uvs.uv(&triangles, tri_vi)
            {
                // FBX measures v from the bottom of the picture.
                vertex.uv = [uv.x as f32, 1.0 - uv.y as f32];
            }
            if let Some(colors) = &colors
                && let Ok(c) = colors.color(&triangles, tri_vi)
            {
                vertex.color = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            vertices.push(vertex);
            if i % 3 == 0 {
                let slot = slots
                    .as_ref()
                    .and_then(|slots| slots.material_index(&triangles, tri_vi).ok())
                    .map_or(0, |index| index.to_u32());
                slot_of_triangle.push(slot);
            }
        }
        // A triangle's three corners are kept or left out together.
        vertices.truncate(slot_of_triangle.len() * 3);
        if vertices.is_empty() {
            continue;
        }
        if normals.is_none() {
            flat_normals_in_place(&mut vertices);
        }

        // One mesh for each material slot the triangles use.
        let model_materials: Vec<_> = mesh_model.materials().collect();
        let matrix = world_matrix(&model) * geometric_matrix(&object);
        let transform = Mat4::from_cols_array(&matrix.to_cols_array().map(|c| c as f32));
        let mut by_slot: Vec<(u32, Vec<Vertex>)> = Vec::new();
        for (triangle, &slot) in vertices.as_chunks::<3>().0.iter().zip(&slot_of_triangle) {
            let at = match by_slot.iter().position(|(s, _)| *s == slot) {
                Some(at) => at,
                None => {
                    by_slot.push((slot, Vec::new()));
                    by_slot.len() - 1
                }
            };
            by_slot[at].1.extend_from_slice(triangle);
        }
        for (slot, vertices) in by_slot {
            let material = model_materials.get(slot as usize).map(|source| {
                *material_of
                    .entry(source.object_id().raw())
                    .or_insert_with(|| {
                        parts
                            .materials
                            .push(convert_material(source, &mut pictures));
                        parts.materials.len() - 1
                    })
            });
            parts.meshes.push(Mesh::new(
                object.name().unwrap_or_default(),
                Topology::Triangles,
                vertices,
                None,
                material,
            ));
            parts.instances.push(Instance {
                mesh: parts.meshes.len() - 1,
                transform,
            });
        }
    }

    if parts.meshes.is_empty() {
        return Err(ctx.parse_error("it holds no meshes"));
    }
    parts.images = pictures.images;
    parts.warnings = pictures.warnings;
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::ModelFormat;
    use crate::load::{Cancel, LoadOptions, load_bytes};
    use crate::model::Model;
    use crate::samples::{CORNERS, QUADS};
    use fbxcel::low::FbxVersion;
    use fbxcel::writer::v7400::binary::{FbxFooter, Writer};

    type W = Writer<Cursor<Vec<u8>>>;

    fn node(w: &mut W, name: &str, body: impl FnOnce(&mut W)) {
        w.new_node(name).unwrap();
        body(w);
        w.close_node().unwrap();
    }

    fn leaf_string(w: &mut W, name: &str, value: &str) {
        w.new_node(name)
            .unwrap()
            .append_string_direct(value)
            .unwrap();
        w.close_node().unwrap();
    }

    fn leaf_i32(w: &mut W, name: &str, value: i32) {
        w.new_node(name).unwrap().append_i32(value).unwrap();
        w.close_node().unwrap();
    }

    /// `P: name, type, label, flags, values...` of a `Properties70` node.
    fn property(w: &mut W, name: &str, ty: &str, values: &[f64]) {
        let mut attributes = w.new_node("P").unwrap();
        attributes.append_string_direct(name).unwrap();
        attributes.append_string_direct(ty).unwrap();
        attributes.append_string_direct("").unwrap();
        attributes.append_string_direct("A").unwrap();
        for value in values {
            attributes.append_f64(*value).unwrap();
        }
        w.close_node().unwrap();
    }

    /// An object's attributes: id, "Name\0\x01Class", subclass.
    fn object(
        w: &mut W,
        kind: &str,
        id: i64,
        name: &str,
        subclass: &str,
        body: impl FnOnce(&mut W),
    ) {
        let mut attributes = w.new_node(kind).unwrap();
        attributes.append_i64(id).unwrap();
        attributes
            .append_string_direct(&format!("{name}\u{0}\u{1}{kind}"))
            .unwrap();
        attributes.append_string_direct(subclass).unwrap();
        body(w);
        w.close_node().unwrap();
    }

    fn connect(w: &mut W, child: i64, parent: i64) {
        let mut c = w.new_node("C").unwrap();
        c.append_string_direct("OO").unwrap();
        c.append_i64(child).unwrap();
        c.append_i64(parent).unwrap();
        w.close_node().unwrap();
    }

    /// A layer element: its index, how it maps to the mesh, and its data.
    fn layer_element(
        w: &mut W,
        name: &str,
        mapping: &str,
        reference: &str,
        data: impl FnOnce(&mut W),
    ) {
        w.new_node(name).unwrap().append_i32(0).unwrap();
        leaf_i32(w, "Version", 101);
        leaf_string(w, "Name", "");
        leaf_string(w, "MappingInformationType", mapping);
        leaf_string(w, "ReferenceInformationType", reference);
        data(w);
        w.close_node().unwrap();
    }

    fn array_f64(w: &mut W, name: &str, values: Vec<f64>) {
        w.new_node(name)
            .unwrap()
            .append_arr_f64_from_iter(None, values)
            .unwrap();
        w.close_node().unwrap();
    }

    fn array_i32(w: &mut W, name: &str, values: Vec<i32>) {
        w.new_node(name)
            .unwrap()
            .append_arr_i32_from_iter(None, values)
            .unwrap();
        w.close_node().unwrap();
    }

    /// A cube of two materials, with normals and texture coordinates, under a
    /// parent node, as a binary FBX 7.4. The cube's own node is moved, turned
    /// and scaled by the arguments.
    fn cube_fbx(translation: [f64; 3], rotation: [f64; 3], scaling: [f64; 3]) -> Vec<u8> {
        let mut w: W = Writer::new(Cursor::new(Vec::new()), FbxVersion::V7_4).unwrap();
        // The one scene the document is about, whose root is the connections'
        // object 0.
        node(&mut w, "Documents", |w| {
            leaf_i32(w, "Count", 1);
            let mut document = w.new_node("Document").unwrap();
            document.append_i64(1).unwrap();
            document.append_string_direct("Scene").unwrap();
            document.append_string_direct("Scene").unwrap();
            w.new_node("RootNode").unwrap().append_i64(0).unwrap();
            w.close_node().unwrap();
            w.close_node().unwrap();
        });
        node(&mut w, "Objects", |w| {
            object(w, "Geometry", 100, "cube", "Mesh", |w| {
                array_f64(
                    w,
                    "Vertices",
                    CORNERS.iter().flatten().map(|&c| f64::from(c)).collect(),
                );
                // Each quad ends on a corner written as its bitwise inverse.
                let indices = QUADS
                    .iter()
                    .flat_map(|q| [q[0] as i32, q[1] as i32, q[2] as i32, !(q[3] as i32)])
                    .collect();
                array_i32(w, "PolygonVertexIndex", indices);
                layer_element(w, "LayerElementNormal", "ByPolygonVertex", "Direct", |w| {
                    let normals = QUADS.iter().flat_map(|q| {
                        let n =
                            crate::load::face_normal(CORNERS[q[0]], CORNERS[q[1]], CORNERS[q[2]])
                                .unwrap();
                        std::iter::repeat_n(n.map(f64::from), 4).flatten()
                    });
                    array_f64(w, "Normals", normals.collect());
                });
                layer_element(
                    w,
                    "LayerElementUV",
                    "ByPolygonVertex",
                    "IndexToDirect",
                    |w| {
                        array_f64(w, "UV", vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
                        array_i32(w, "UVIndex", (0..24).map(|i| i % 4).collect());
                    },
                );
                layer_element(
                    w,
                    "LayerElementMaterial",
                    "ByPolygon",
                    "IndexToDirect",
                    |w| {
                        array_i32(w, "Materials", vec![0, 0, 0, 0, 1, 1]);
                    },
                );
                w.new_node("Layer").unwrap().append_i32(0).unwrap();
                leaf_i32(w, "Version", 100);
                for ty in [
                    "LayerElementNormal",
                    "LayerElementUV",
                    "LayerElementMaterial",
                ] {
                    node(w, "LayerElement", |w| {
                        leaf_string(w, "Type", ty);
                        leaf_i32(w, "TypedIndex", 0);
                    });
                }
                w.close_node().unwrap();
            });
            object(w, "Model", 200, "cube", "Mesh", |w| {
                node(w, "Properties70", |w| {
                    property(w, "Lcl Translation", "Lcl Translation", &translation);
                    property(w, "Lcl Rotation", "Lcl Rotation", &rotation);
                    property(w, "Lcl Scaling", "Lcl Scaling", &scaling);
                });
            });
            object(w, "Model", 300, "parent", "Null", |w| {
                node(w, "Properties70", |w| {
                    property(w, "Lcl Translation", "Lcl Translation", &[10.0, 0.0, 0.0]);
                });
            });
            object(w, "Material", 400, "red", "", |w| {
                leaf_string(w, "ShadingModel", "phong");
                node(w, "Properties70", |w| {
                    property(w, "DiffuseColor", "Color", &[0.8, 0.1, 0.1]);
                    property(w, "ShininessExponent", "Number", &[30.0]);
                });
            });
            object(w, "Material", 401, "glass", "", |w| {
                leaf_string(w, "ShadingModel", "phong");
                node(w, "Properties70", |w| {
                    property(w, "DiffuseColor", "Color", &[0.1, 0.2, 0.9]);
                    property(w, "TransparencyFactor", "Number", &[0.5]);
                });
            });
        });
        node(&mut w, "Connections", |w| {
            connect(w, 300, 0);
            connect(w, 200, 300);
            connect(w, 100, 200);
            connect(w, 400, 200);
            connect(w, 401, 200);
        });
        w.finalize_and_flush(&FbxFooter::default())
            .unwrap()
            .into_inner()
    }

    fn model_from(bytes: &[u8]) -> Result<Model, LoadError> {
        load_bytes(
            "cube.fbx",
            ModelFormat::Fbx,
            bytes,
            std::path::Path::new("."),
            &LoadOptions::default(),
            &Cancel::default(),
            &mut |_| {},
        )
    }

    #[test]
    fn reads_a_cube_with_its_materials_normals_and_texture_coordinates() {
        let model = model_from(&cube_fbx([0.0; 3], [0.0; 3], [1.0; 3])).unwrap();
        // One mesh for each material the faces use.
        assert_eq!(model.info.meshes, 2);
        assert_eq!(model.info.materials, 2);
        assert_eq!(model.info.triangles, 12);
        let names: Vec<_> = model.materials.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["red", "glass"]);
        // Diffuse colours are sRGB in the file, linear in the material.
        assert!((model.materials[0].base_color[0] - srgb_to_linear(0.8)).abs() < 1e-5);
        assert_eq!(model.materials[1].alpha, AlphaMode::Blend);
        assert!((model.materials[1].base_color[3] - 0.5).abs() < 1e-6);
        // Four faces on the first material, two on the second.
        assert_eq!(model.meshes[0].vertices.len(), 24);
        assert_eq!(model.meshes[1].vertices.len(), 12);
        // The file's normals are used: each triangle's three agree and point out.
        for mesh in &model.meshes {
            for triangle in mesh.vertices.as_chunks::<3>().0 {
                let n = triangle[0].normal;
                let centroid =
                    [0, 1, 2].map(|a| triangle.iter().map(|v| v.position[a]).sum::<f32>() / 3.0);
                assert!(n[0] * centroid[0] + n[1] * centroid[1] + n[2] * centroid[2] > 0.0);
            }
        }
        // The texture coordinates, with v turned to the top-left origin.
        assert!(model.meshes[0].vertices.iter().any(|v| v.uv == [1.0, 1.0]));
        assert!(model.meshes[0].vertices.iter().any(|v| v.uv == [0.0, 1.0]));
        assert_eq!(model.info.size, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn stands_the_mesh_where_its_node_and_its_parents_put_it() {
        let model = model_from(&cube_fbx([1.0, 2.0, 3.0], [0.0; 3], [2.0, 1.0, 1.0])).unwrap();
        // The parent is ten along X, the node (1, 2, 3) from it, and stretched.
        assert_eq!(model.info.center, [11.0, 2.0, 3.0]);
        assert_eq!(model.info.size, [2.0, 1.0, 1.0]);
    }

    #[test]
    fn turns_the_mesh_by_the_nodes_rotation() {
        let model = model_from(&cube_fbx([0.0; 3], [0.0, 0.0, 90.0], [2.0, 1.0, 1.0])).unwrap();
        // Two long along X, turned a quarter about Z: two long along Y.
        let size = model.info.size;
        assert!(
            (size[0] - 1.0).abs() < 1e-4 && (size[1] - 2.0).abs() < 1e-4,
            "{size:?}"
        );
    }

    #[test]
    fn follows_the_rotation_order_of_the_file() {
        let order = |order: i32, degrees: [f64; 3]| rotation(DVec3::from(degrees), order);
        let point = DVec3::X;
        // XYZ turns about X first, then Y, then Z: a point on X, turned 90
        // about Z, ends on Y.
        assert!((order(0, [0.0, 0.0, 90.0]) * point - DVec3::Y).length() < 1e-12);
        // About Y then Z (order ZYX turns Z first, then Y, then X).
        let zyx = order(5, [90.0, 0.0, 90.0]) * DVec3::X;
        let xyz = order(0, [90.0, 0.0, 90.0]) * DVec3::X;
        assert!(
            (zyx - xyz).length() > 0.5,
            "the orders differ: {zyx:?} {xyz:?}"
        );
    }

    #[test]
    fn says_what_is_wrong_with_a_file_that_is_not_a_binary_fbx() {
        let ascii = model_from(b"; FBX 7.3.0 project file\nFBXHeaderExtension: {}").unwrap_err();
        assert!(matches!(ascii, LoadError::Unsupported(_)), "{ascii}");
        assert!(ascii.to_string().contains("text flavour"));
        let garbage = model_from(b"definitely not an fbx").unwrap_err();
        assert!(
            garbage
                .to_string()
                .starts_with("This FBX file could not be read"),
            "{garbage}"
        );
    }
}
