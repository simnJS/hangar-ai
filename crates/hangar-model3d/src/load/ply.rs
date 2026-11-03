//! PLY: scans and photogrammetry output, with vertex colours, and often with
//! no faces at all.

use std::io::{BufReader, Cursor};

use ply_rs::parser::Parser;
use ply_rs::ply::{DefaultElement, Property, PropertyAccess};

use super::geometry::{fan, smooth_normals};
use super::{Ctx, placed_at_origin};
use crate::formats::LoadError;
use crate::model::{Material, Mesh, ModelParts, Topology, Vertex};

#[derive(Default)]
struct Point {
    position: [f32; 3],
    normal: [f32; 3],
    has_normal: bool,
    color: [u8; 4],
    has_color: bool,
}

fn number(property: &Property) -> Option<f64> {
    Some(match property {
        Property::Char(v) => f64::from(*v),
        Property::UChar(v) => f64::from(*v),
        Property::Short(v) => f64::from(*v),
        Property::UShort(v) => f64::from(*v),
        Property::Int(v) => f64::from(*v),
        Property::UInt(v) => f64::from(*v),
        Property::Float(v) => f64::from(*v),
        Property::Double(v) => *v,
        _ => return None,
    })
}

/// A colour channel as a byte: integer files hold 0-255, float files 0-1.
fn channel(property: &Property) -> Option<u8> {
    match property {
        Property::Float(_) | Property::Double(_) => {
            number(property).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        }
        _ => number(property).map(|v| v.clamp(0.0, 255.0) as u8),
    }
}

impl PropertyAccess for Point {
    fn new() -> Self {
        Point {
            color: [255; 4],
            ..Point::default()
        }
    }

    fn set_property(&mut self, key: String, property: Property) {
        let value = number(&property).unwrap_or(0.0) as f32;
        match key.as_str() {
            "x" => self.position[0] = value,
            "y" => self.position[1] = value,
            "z" => self.position[2] = value,
            "nx" => {
                self.normal[0] = value;
                self.has_normal = true;
            }
            "ny" => self.normal[1] = value,
            "nz" => self.normal[2] = value,
            "red" | "r" | "diffuse_red" => {
                self.color[0] = channel(&property).unwrap_or(255);
                self.has_color = true;
            }
            "green" | "g" | "diffuse_green" => self.color[1] = channel(&property).unwrap_or(255),
            "blue" | "b" | "diffuse_blue" => self.color[2] = channel(&property).unwrap_or(255),
            "alpha" | "a" | "diffuse_alpha" => self.color[3] = channel(&property).unwrap_or(255),
            _ => {}
        }
    }
}

#[derive(Default)]
struct Face {
    indices: Vec<u32>,
}

fn list(property: &Property) -> Option<Vec<u32>> {
    Some(match property {
        Property::ListChar(v) => v.iter().map(|&i| i.max(0) as u32).collect(),
        Property::ListUChar(v) => v.iter().map(|&i| u32::from(i)).collect(),
        Property::ListShort(v) => v.iter().map(|&i| i.max(0) as u32).collect(),
        Property::ListUShort(v) => v.iter().map(|&i| u32::from(i)).collect(),
        Property::ListInt(v) => v.iter().map(|&i| i.max(0) as u32).collect(),
        Property::ListUInt(v) => v.clone(),
        _ => return None,
    })
}

impl PropertyAccess for Face {
    fn new() -> Self {
        Face::default()
    }

    fn set_property(&mut self, key: String, property: Property) {
        if matches!(key.as_str(), "vertex_indices" | "vertex_index")
            && let Some(indices) = list(&property)
        {
            self.indices = indices;
        }
    }
}

#[derive(Default)]
struct Edge {
    ends: [u32; 2],
}

impl PropertyAccess for Edge {
    fn new() -> Self {
        Edge::default()
    }

    fn set_property(&mut self, key: String, property: Property) {
        let value = number(&property).unwrap_or(0.0).max(0.0) as u32;
        match key.as_str() {
            "vertex1" => self.ends[0] = value,
            "vertex2" => self.ends[1] = value,
            _ => {}
        }
    }
}

pub(super) fn read(bytes: &[u8], ctx: &Ctx) -> Result<ModelParts, LoadError> {
    let mut reader = BufReader::new(Cursor::new(bytes));
    let header = Parser::<DefaultElement>::new()
        .read_header(&mut reader)
        .map_err(|err| ctx.parse_error(err))?;

    let mut points: Vec<Point> = Vec::new();
    let mut faces: Vec<Face> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    // Elements come in the order the header lists them and each one has to be
    // read to get to the next, whether or not it is wanted.
    for (name, definition) in &header.elements {
        ctx.cancel.check()?;
        let failed = |err: std::io::Error| ctx.parse_error(format!("{name}: {err}"));
        match name.as_str() {
            "vertex" => {
                points = Parser::<Point>::new()
                    .read_payload_for_element(&mut reader, definition, &header)
                    .map_err(failed)?;
            }
            "face" => {
                faces = Parser::<Face>::new()
                    .read_payload_for_element(&mut reader, definition, &header)
                    .map_err(failed)?;
            }
            "edge" => {
                edges = Parser::<Edge>::new()
                    .read_payload_for_element(&mut reader, definition, &header)
                    .map_err(failed)?;
            }
            _ => {
                Parser::<DefaultElement>::new()
                    .read_payload_for_element(&mut reader, definition, &header)
                    .map_err(failed)?;
            }
        }
    }
    if points.is_empty() {
        return Err(ctx.parse_error("it holds no vertices"));
    }

    let has_color = points.iter().any(|p| p.has_color);
    let has_normal = points.iter().all(|p| p.has_normal);
    let mut warnings = Vec::new();
    let vertices: Vec<Vertex> = points
        .iter()
        .map(|p| Vertex {
            position: p.position,
            normal: if has_normal {
                p.normal
            } else {
                [0.0, 1.0, 0.0]
            },
            uv: [0.0; 2],
            color: if has_color { p.color } else { Vertex::WHITE },
        })
        .collect();

    let count = vertices.len() as u32;
    let mut triangles: Vec<u32> = Vec::new();
    let mut skipped = 0usize;
    for face in &faces {
        if face.indices.len() < 3 || face.indices.iter().any(|&i| i >= count) {
            skipped += 1;
            continue;
        }
        fan(&face.indices, &mut triangles);
    }
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} faces were skipped: they have fewer than three corners or name a vertex that is not there."
        ));
    }

    let (topology, indices) = if !triangles.is_empty() {
        (Topology::Triangles, Some(triangles))
    } else if !edges.is_empty() {
        let segments = edges
            .iter()
            .filter(|e| e.ends.iter().all(|&i| i < count))
            .flat_map(|e| e.ends)
            .collect();
        (Topology::Lines, Some(segments))
    } else {
        // A scanner's point cloud: vertices and nothing to join them.
        (Topology::Points, None)
    };

    let mut vertices = vertices;
    if topology == Topology::Triangles && !has_normal {
        // A scan is a smooth surface sampled finely: normals averaged over
        // the faces round it off, where facets would show every triangle.
        smooth_normals(&mut vertices, indices.as_deref().unwrap_or_default());
    }

    let mut parts = ModelParts {
        meshes: vec![Mesh::new("", topology, vertices, indices, Some(0))],
        materials: vec![Material::clay(has_color)],
        warnings,
        ..ModelParts::default()
    };
    if topology != Topology::Triangles {
        // Points and segments are unlit: the clay's colour is all they use.
        parts.materials[0].unlit = true;
    }
    placed_at_origin(&mut parts);
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::ModelFormat;
    use crate::load::{Cancel, LoadOptions};
    use crate::samples;

    fn read_bytes(bytes: &[u8]) -> Result<ModelParts, LoadError> {
        let ctx = Ctx {
            format: ModelFormat::Ply,
            base: std::path::Path::new("."),
            options: &LoadOptions::default(),
            cancel: &Cancel::default(),
        };
        read(bytes, &ctx)
    }

    #[test]
    fn reads_an_ascii_ply_with_colours_and_quads() {
        let parts = read_bytes(samples::cube_ply_ascii().as_bytes()).unwrap();
        let mesh = &parts.meshes[0];
        assert_eq!(mesh.topology, Topology::Triangles);
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.indices.as_ref().unwrap().len(), 36);
        assert_eq!(mesh.vertices[1].color, [30, 225, 128, 255]);
        // Colours show as they are: the clay is white.
        assert_eq!(parts.materials[0].base_color, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn smooths_the_normals_of_a_ply_that_has_none() {
        let parts = read_bytes(samples::cube_ply_ascii().as_bytes()).unwrap();
        for vertex in &parts.meshes[0].vertices {
            // At a cube's corner the three faces average to the diagonal.
            let n = vertex.normal;
            for axis in 0..3 {
                assert!(n[axis].signum() == vertex.position[axis].signum(), "{n:?}");
            }
        }
    }

    #[test]
    fn reads_binary_files_of_either_endianness() {
        for big in [false, true] {
            let parts = read_bytes(&samples::cube_ply_binary(big)).unwrap();
            let mesh = &parts.meshes[0];
            assert_eq!(mesh.vertices.len(), 8, "big endian: {big}");
            assert_eq!(mesh.indices.as_ref().unwrap().len(), 36);
            // The file's own normals are kept.
            assert_eq!(mesh.vertices[7].normal, [1.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn reads_vertices_without_faces_as_a_point_cloud() {
        let parts = read_bytes(samples::point_cloud_ply(500).as_bytes()).unwrap();
        let mesh = &parts.meshes[0];
        assert_eq!(mesh.topology, Topology::Points);
        assert_eq!(mesh.vertices.len(), 500);
        assert!(parts.materials[0].unlit);
    }

    #[test]
    fn skips_a_face_that_names_a_vertex_that_is_not_there() {
        let ply = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 2\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n3 0 1 9\n";
        let parts = read_bytes(ply.as_bytes()).unwrap();
        assert_eq!(parts.meshes[0].indices.as_ref().unwrap().len(), 3);
        assert_eq!(parts.warnings.len(), 1);
    }

    #[test]
    fn says_what_is_wrong_with_a_file_that_is_not_a_ply() {
        let error = read_bytes(b"not a ply").unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("This PLY file could not be read"),
            "{error}"
        );
        let truncated = read_bytes(&samples::cube_ply_binary(false)[..200]).unwrap_err();
        assert!(matches!(truncated, LoadError::Parse { .. }), "{truncated}");
    }
}
