//! Wavefront OBJ, with the `.mtl` libraries it names.
//!
//! `tobj` triangulates and unifies the indices; what it hands back is cut
//! into one mesh for each object and material, which is how the old viewer's
//! scene came out of three.js's loader too (a material change inside an
//! object split it into groups of one mesh; the draw is the same).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;

use super::geometry::flat_shaded;
use super::resources::{ImageSource, decode_all};
use super::{Ctx, placed_at_origin};
use crate::formats::LoadError;
use crate::model::{AlphaMode, Image, Material, Mesh, ModelParts, TextureRef, Topology, Vertex};
use crate::theme::srgb_to_linear;

/// Blinn-Phong's exponent to a GGX roughness, by matching the lobes' widths:
/// a shininess of 30, Phong's default, comes out at 0.5. The same conversion
/// `materials.ts` made for every legacy material.
pub fn roughness_from_shininess(shininess: f32) -> f32 {
    (2.0 / (shininess.max(0.0) + 2.0))
        .powf(0.25)
        .clamp(0.04, 1.0)
}

/// A texture name as an `.mtl` writes it: after any options (`-bm 1.0`,
/// `-s 2 2 2`), the file name is the last word.
fn texture_path(raw: &str) -> &str {
    let raw = raw.trim();
    if raw.starts_with('-') {
        raw.rsplit(char::is_whitespace).next().unwrap_or(raw)
    } else {
        raw
    }
}

fn linear(color: [f32; 3]) -> [f32; 3] {
    color.map(srgb_to_linear)
}

pub(super) fn read(bytes: &[u8], ctx: &Ctx) -> Result<ModelParts, LoadError> {
    let resources = ctx.resources();
    let warnings = RefCell::new(Vec::<String>::new());
    let options = tobj::LoadOptions {
        single_index: true,
        triangulate: true,
        ignore_points: true,
        ignore_lines: true,
    };
    let (models, materials) = tobj::load_obj_buf(&mut Cursor::new(bytes), &options, |name| {
        let name = name.to_string_lossy();
        resources
            .read(&name)
            .map_err(|reason| {
                warnings.borrow_mut().push(format!(
                    "The material library {name} could not be read ({reason}); the model is shown in clay."
                ));
                tobj::LoadError::OpenFileFailed
            })
            .and_then(|bytes| tobj::load_mtl_buf(&mut Cursor::new(bytes)))
    })
    .map_err(|err| ctx.parse_error(err))?;
    // A library that is missing leaves the model in clay rather than not
    // opening it.
    let materials = materials.unwrap_or_default();

    // Every texture the materials name, decoded once whichever slots use it.
    let mut names: Vec<String> = Vec::new();
    for material in &materials {
        for name in [&material.diffuse_texture, &material.normal_texture]
            .into_iter()
            .flatten()
        {
            let name = texture_path(name).to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let decoded = decode_all(
        &resources,
        names.iter().map(|n| ImageSource::Uri(n.clone())).collect(),
        ctx.cancel,
    );
    ctx.cancel.check()?;
    let mut images: Vec<Image> = Vec::new();
    let mut image_of: HashMap<String, usize> = HashMap::new();
    let mut warnings = warnings.into_inner();
    for (name, outcome) in names.into_iter().zip(decoded) {
        match outcome {
            Ok(image) => {
                image_of.insert(name, images.len());
                images.push(image);
            }
            Err(reason) => {
                warnings.push(format!("The texture {name} could not be read: {reason}."))
            }
        }
    }
    let texture = |name: &Option<String>| {
        name.as_deref()
            .and_then(|n| image_of.get(texture_path(n)))
            .map(|&image| TextureRef::new(image))
    };

    let mut converted: Vec<Material> = materials
        .iter()
        .map(|m| {
            let diffuse = linear(m.diffuse.unwrap_or([0.8, 0.8, 0.8]));
            let opacity = m.dissolve.unwrap_or(1.0).clamp(0.0, 1.0);
            Material {
                name: m.name.clone(),
                base_color: [diffuse[0], diffuse[1], diffuse[2], opacity],
                metallic: 0.0,
                roughness: roughness_from_shininess(m.shininess.unwrap_or(30.0)),
                emissive: linear(m.emissive.unwrap_or([0.0; 3])),
                alpha: if opacity < 1.0 {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                base_color_texture: texture(&m.diffuse_texture),
                // The `.mtl` calls it a bump map, but exporters put a
                // tangent-space normal map there; a height map shown as a
                // normal map is wrong in a way that is easy to see, and the
                // other way round would drop every normal map.
                normal_texture: texture(&m.normal_texture),
                ..Material::default()
            }
        })
        .collect();

    let mut meshes = Vec::new();
    // The clay for a model with vertex colours and no material of its own.
    let mut colored_clay: Option<usize> = None;
    for model in &models {
        ctx.cancel.check()?;
        let mesh = &model.mesh;
        let count = mesh.positions.len() / 3;
        if count == 0 || mesh.indices.is_empty() {
            continue;
        }
        let has_normals = mesh.normals.len() == count * 3;
        let has_uvs = mesh.texcoords.len() == count * 2;
        let has_colors = mesh.vertex_color.len() == count * 3;
        let vertices: Vec<Vertex> = (0..count)
            .map(|i| Vertex {
                position: [
                    mesh.positions[i * 3],
                    mesh.positions[i * 3 + 1],
                    mesh.positions[i * 3 + 2],
                ],
                normal: if has_normals {
                    [
                        mesh.normals[i * 3],
                        mesh.normals[i * 3 + 1],
                        mesh.normals[i * 3 + 2],
                    ]
                } else {
                    [0.0, 1.0, 0.0]
                },
                // OBJ measures v from the bottom of the picture, glTF and the
                // GPU from the top.
                uv: if has_uvs {
                    [mesh.texcoords[i * 2], 1.0 - mesh.texcoords[i * 2 + 1]]
                } else {
                    [0.0; 2]
                },
                color: if has_colors {
                    let channel = |k: usize| {
                        (mesh.vertex_color[i * 3 + k].clamp(0.0, 1.0) * 255.0).round() as u8
                    };
                    [channel(0), channel(1), channel(2), 255]
                } else {
                    Vertex::WHITE
                },
            })
            .collect();

        let material = match mesh.material_id.filter(|&id| id < converted.len()) {
            Some(id) => Some(id),
            None if has_colors => Some(*colored_clay.get_or_insert_with(|| {
                converted.push(Material::clay(true));
                converted.len() - 1
            })),
            None => None,
        };
        let name = model.name.clone();
        let mesh = if has_normals {
            Mesh::new(
                name,
                Topology::Triangles,
                vertices,
                Some(mesh.indices.clone()),
                material,
            )
        } else {
            // No normals in the file: faceted, as the old viewer drew it.
            Mesh::new(
                name,
                Topology::Triangles,
                flat_shaded(&vertices, &mesh.indices),
                None,
                material,
            )
        };
        meshes.push(mesh);
    }
    if meshes.is_empty() {
        return Err(ctx.parse_error("it holds no faces"));
    }
    let mut parts = ModelParts {
        meshes,
        materials: converted,
        images,
        warnings,
        ..ModelParts::default()
    };
    placed_at_origin(&mut parts);
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::{Cancel, LoadOptions, load, load_with};
    use crate::samples;

    fn write_cube(dir: &std::path::Path) -> std::path::PathBuf {
        let (obj, mtl) = samples::cube_obj();
        std::fs::write(dir.join("cube.mtl"), mtl).unwrap();
        let path = dir.join("cube.obj");
        std::fs::write(&path, obj).unwrap();
        path
    }

    #[test]
    fn splits_an_object_where_its_material_changes() {
        let dir = tempfile::tempdir().unwrap();
        let model = load(write_cube(dir.path())).unwrap();
        assert_eq!(model.info.meshes, 2);
        assert_eq!(model.info.materials, 2);
        // Six quads are twelve triangles.
        assert_eq!(model.info.triangles, 12);
        let names: Vec<_> = model.materials.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["red", "blue"]);
    }

    #[test]
    fn converts_the_phong_material_to_the_metallic_roughness_model() {
        let dir = tempfile::tempdir().unwrap();
        let model = load(write_cube(dir.path())).unwrap();
        let red = &model.materials[0];
        // Kd is sRGB in the file, linear in the material.
        assert!((red.base_color[0] - srgb_to_linear(0.8)).abs() < 1e-5);
        assert_eq!(red.metallic, 0.0);
        // Ns 30 is a roughness of one half; Ns 200 is shinier.
        assert!((red.roughness - 0.5).abs() < 1e-5);
        assert!(model.materials[1].roughness < 0.35);
        assert!(model.materials[1].emissive[2] > 0.0);
    }

    #[test]
    fn computes_faceted_normals_for_a_file_that_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let model = load(write_cube(dir.path())).unwrap();
        for mesh in &model.meshes {
            assert!(mesh.indices.is_none());
            for triangle in mesh.vertices.as_chunks::<3>().0 {
                assert_eq!(triangle[0].normal, triangle[1].normal);
                assert_eq!(triangle[1].normal, triangle[2].normal);
            }
        }
    }

    #[test]
    fn keeps_the_normals_a_file_gives() {
        let obj =
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1/1 2/2/1 3/3/1\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tri.obj");
        std::fs::write(&path, obj).unwrap();
        let model = load(path).unwrap();
        let mesh = &model.meshes[0];
        assert_eq!(mesh.vertices.len(), 3);
        assert!(mesh.indices.is_some());
        assert_eq!(mesh.vertices[0].normal, [0.0, 0.0, 1.0]);
        // v is flipped to the top-left origin.
        assert_eq!(mesh.vertices[2].uv, [0.0, 0.0]);
        assert_eq!(mesh.vertices[0].uv, [0.0, 1.0]);
    }

    #[test]
    fn opens_a_model_whose_library_is_missing_in_clay() {
        let dir = tempfile::tempdir().unwrap();
        let (obj, _) = samples::cube_obj();
        let path = dir.path().join("cube.obj");
        std::fs::write(&path, obj).unwrap();
        let model = load(&path).unwrap();
        assert!(
            model.warnings.iter().any(|w| w.contains("cube.mtl")),
            "{:?}",
            model.warnings
        );
        assert_eq!(model.info.triangles, 12);
    }

    #[test]
    fn loads_the_textures_a_material_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("wood.png"),
            samples::checker_png([200, 150, 90], [90, 60, 30]),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("woodn.png"),
            samples::checker_png([128, 128, 255], [140, 128, 250]),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("t.mtl"),
            "newmtl wood\nKd 1 1 1\nmap_Kd wood.png\nmap_Bump -bm 1.0 woodn.png\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("t.obj"),
            "mtllib t.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl wood\nf 1/1 2/2 3/3\n",
        )
        .unwrap();
        let model = load(dir.path().join("t.obj")).unwrap();
        assert_eq!(model.images.len(), 2);
        let wood = &model.materials[0];
        assert_eq!(wood.base_color_texture.unwrap().image, 0);
        assert_eq!(wood.normal_texture.unwrap().image, 1);
    }

    #[test]
    fn colours_vertices_from_the_extended_v_lines() {
        let obj = "v 0 0 0 1 0 0\nv 1 0 0 0 1 0\nv 0 1 0 0 0 1\nf 1 2 3\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.obj");
        std::fs::write(&path, obj).unwrap();
        let model = load(path).unwrap();
        let mesh = &model.meshes[0];
        assert_eq!(mesh.vertices[1].color, [0, 255, 0, 255]);
        // The clay is white, so the colours are not tinted.
        assert_eq!(model.materials[0].base_color, [1.0; 4]);
    }

    #[test]
    fn keeps_to_the_allowed_folders_for_the_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("in");
        std::fs::create_dir(&inside).unwrap();
        std::fs::write(dir.path().join("cube.mtl"), samples::cube_obj().1).unwrap();
        let (obj, _) = samples::cube_obj();
        let obj = obj.replace("mtllib cube.mtl", "mtllib ../cube.mtl");
        let path = inside.join("cube.obj");
        std::fs::write(&path, obj).unwrap();
        let options = LoadOptions {
            roots: Some(vec![inside.canonicalize().unwrap()]),
        };
        let model = load_with(&path, &options, &Cancel::default(), &mut |_| {}).unwrap();
        assert!(
            model.warnings.iter().any(|w| w.contains("outside")),
            "{:?}",
            model.warnings
        );
    }

    #[test]
    fn says_what_is_wrong_with_a_file_with_no_faces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.obj");
        std::fs::write(&path, "# nothing\n").unwrap();
        let error = load(path).unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("This OBJ file could not be read"),
            "{error}"
        );
    }

    #[test]
    fn matches_phong_shininess_to_roughness() {
        assert!((roughness_from_shininess(30.0) - 0.5).abs() < 1e-6);
        assert_eq!(roughness_from_shininess(0.0), 1.0);
        assert_eq!(roughness_from_shininess(1.0e9), 0.04);
    }

    #[test]
    fn reads_the_file_name_after_a_textures_options() {
        assert_eq!(texture_path("wood.png"), "wood.png");
        assert_eq!(texture_path("-bm 1.0 wood.png"), "wood.png");
    }
}
