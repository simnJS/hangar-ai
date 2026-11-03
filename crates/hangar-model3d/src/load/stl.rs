//! STL: CAD exports and slicer input, as bare triangles.
//!
//! Binary files are read here, ASCII ones by `stl_io`. The crate decides
//! between the two by the first line alone, and a binary file whose 80-byte
//! header starts "solid " — as many exporters write it — would be taken for
//! text. The size settles it: a binary file is 84 bytes plus 50 for each
//! triangle, to the byte.

use std::io::Cursor;

use super::geometry::flat_normals_in_place;
use super::{Ctx, placed_at_origin};
use crate::formats::LoadError;
use crate::model::{Mesh, ModelParts, Topology, Vertex};

pub(super) fn read(bytes: &[u8], ctx: &Ctx) -> Result<ModelParts, LoadError> {
    let vertices = if is_binary(bytes) {
        binary_vertices(bytes)
    } else {
        ascii_vertices(bytes).map_err(|reason| ctx.parse_error(reason))?
    };
    if vertices.is_empty() {
        return Err(ctx.parse_error("it holds no triangles"));
    }
    let mut vertices = vertices;
    // Facet by facet, from the winding: the normals in the file are often
    // zero, and a viewer that trusts them draws a black model.
    flat_normals_in_place(&mut vertices);
    let mut parts = ModelParts {
        meshes: vec![Mesh::new("", Topology::Triangles, vertices, None, None)],
        ..ModelParts::default()
    };
    placed_at_origin(&mut parts);
    Ok(parts)
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.len() >= 84 && {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as u64;
        84 + 50 * count == bytes.len() as u64
    }
}

fn float(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn binary_vertices(bytes: &[u8]) -> Vec<Vertex> {
    // Magics and its relatives put colours in the header's "COLOR=" and the
    // facets' attribute bytes; three.js's loader reads them the same way.
    let colored = bytes[..80].windows(6).any(|w| w == b"COLOR=");
    let count = (bytes.len() - 84) / 50;
    let mut vertices = Vec::with_capacity(count * 3);
    for facet in bytes[84..].as_chunks::<50>().0 {
        let attribute = u16::from_le_bytes([facet[48], facet[49]]);
        let color = if colored && attribute & 0x8000 == 0 {
            let channel =
                |shift: u16| (((attribute >> shift) & 0x1f) as f32 / 31.0 * 255.0).round() as u8;
            [channel(0), channel(5), channel(10), 255]
        } else {
            Vertex::WHITE
        };
        for corner in 0..3 {
            let at = 12 + corner * 12;
            let mut vertex =
                Vertex::at([float(facet, at), float(facet, at + 4), float(facet, at + 8)]);
            vertex.color = color;
            vertices.push(vertex);
        }
    }
    vertices
}

fn ascii_vertices(bytes: &[u8]) -> Result<Vec<Vertex>, String> {
    let mut cursor = Cursor::new(bytes);
    let triangles = stl_io::create_stl_reader(&mut cursor).map_err(|err| err.to_string())?;
    let mut vertices = Vec::new();
    for triangle in triangles {
        let triangle = triangle.map_err(|err| err.to_string())?;
        for corner in triangle.vertices {
            vertices.push(Vertex::at([corner[0], corner[1], corner[2]]));
        }
    }
    Ok(vertices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::ModelFormat;
    use crate::load::{Cancel, LoadOptions};
    use crate::samples;

    fn read_bytes(bytes: &[u8]) -> Result<ModelParts, LoadError> {
        let ctx = Ctx {
            format: ModelFormat::Stl,
            base: std::path::Path::new("."),
            options: &LoadOptions::default(),
            cancel: &Cancel::default(),
        };
        read(bytes, &ctx)
    }

    fn assert_outward(parts: &ModelParts) {
        for mesh in &parts.meshes {
            for triangle in mesh.vertices.as_chunks::<3>().0 {
                let centroid = [0, 1, 2]
                    .map(|axis| triangle.iter().map(|v| v.position[axis]).sum::<f32>() / 3.0);
                let n = triangle[0].normal;
                let outward = n[0] * centroid[0] + n[1] * centroid[1] + n[2] * centroid[2];
                assert!(outward > 0.0, "normal {n:?} at {centroid:?} points inward");
            }
        }
    }

    #[test]
    fn reads_a_binary_stl_as_twelve_faceted_triangles() {
        let parts = read_bytes(&samples::cube_stl_binary()).unwrap();
        assert_eq!(parts.meshes.len(), 1);
        assert_eq!(parts.meshes[0].vertices.len(), 36);
        assert!(parts.meshes[0].indices.is_none());
        assert_outward(&parts);
    }

    #[test]
    fn reads_a_binary_stl_whose_header_says_solid() {
        let parts = read_bytes(&samples::cube_stl_binary_with_solid_header()).unwrap();
        assert_eq!(parts.meshes[0].vertices.len(), 36);
    }

    #[test]
    fn reads_an_ascii_stl() {
        let parts = read_bytes(samples::cube_stl_ascii().as_bytes()).unwrap();
        assert_eq!(parts.meshes[0].vertices.len(), 36);
        assert_outward(&parts);
    }

    #[test]
    fn reads_the_colours_of_a_binary_stl_that_has_them() {
        let mut bytes = samples::cube_stl_binary();
        bytes[..6].copy_from_slice(b"COLOR=");
        // The first facet: pure red in 5-5-5 bits, valid because bit 15 is 0.
        bytes[84 + 48..84 + 50].copy_from_slice(&0x001fu16.to_le_bytes());
        let parts = read_bytes(&bytes).unwrap();
        assert_eq!(parts.meshes[0].vertices[0].color, [255, 0, 0, 255]);
        assert_eq!(parts.meshes[0].vertices[3].color, [0, 0, 0, 255]);
    }

    #[test]
    fn says_what_is_wrong_with_a_file_that_is_neither() {
        let error = read_bytes(b"this is not a model at all").unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("This STL file could not be read"),
            "{error}"
        );
        let empty = read_bytes(&[0u8; 84]).unwrap_err();
        assert!(empty.to_string().contains("no triangles"), "{empty}");
    }

    #[test]
    fn draws_the_part_upright() {
        // Z-up in the file: a 1 by 1 by 5 bar along Z stands 5 high once shown.
        let mut bar = read_bytes(&samples::cube_stl_binary()).unwrap();
        let mut vertices = bar.meshes[0].vertices.clone();
        for vertex in &mut vertices {
            vertex.position[2] *= 5.0;
        }
        bar.meshes[0] = Mesh::new("", Topology::Triangles, vertices, None, None);
        let model = crate::model::Model::new("bar.stl", ModelFormat::Stl, bar);
        assert!((model.bounds.size()[1] - 5.0).abs() < 1e-5);
        assert!((model.info.size[2] - 5.0).abs() < 1e-5);
    }
}
