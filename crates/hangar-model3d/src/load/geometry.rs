//! What every reader needs to turn raw arrays into meshes: normals for files
//! that have none, and polygons cut into triangles.

use crate::model::Vertex;

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(a: [f32; 3]) -> Option<[f32; 3]> {
    let length = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    (length > 1e-20 && length.is_finite()).then(|| [a[0] / length, a[1] / length, a[2] / length])
}

/// The unit normal of a triangle by its winding, or `None` for one with no
/// area.
pub fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Option<[f32; 3]> {
    normalized(cross(sub(b, a), sub(c, a)))
}

/// A face's normal, or up for a triangle with no area: it draws nothing, so
/// any normal will do, but not NaN.
fn face_normal_or_up(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    face_normal(a, b, c).unwrap_or([0.0, 1.0, 0.0])
}

/// Splits an indexed mesh into triangles that share nothing, each with its
/// own face normal: what a file without normals is drawn with. Faceted, as
/// three.js drew an OBJ or a glTF that gave no normals (a non-indexed geometry
/// given `computeVertexNormals`, and a material with flat shading).
pub fn flat_shaded(vertices: &[Vertex], indices: &[u32]) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(indices.len());
    for &[a, b, c] in indices.as_chunks::<3>().0 {
        let corners = [
            vertices[a as usize],
            vertices[b as usize],
            vertices[c as usize],
        ];
        let normal = face_normal_or_up(
            corners[0].position,
            corners[1].position,
            corners[2].position,
        );
        for mut corner in corners {
            corner.normal = normal;
            out.push(corner);
        }
    }
    out
}

/// The same for vertices that are already laid out three to a triangle.
pub fn flat_normals_in_place(vertices: &mut [Vertex]) {
    for triangle in vertices.as_chunks_mut::<3>().0 {
        let normal = face_normal_or_up(
            triangle[0].position,
            triangle[1].position,
            triangle[2].position,
        );
        for corner in triangle {
            corner.normal = normal;
        }
    }
}

/// Smooth normals: each vertex gets the average of its faces', weighted by
/// their area, as three.js's `computeVertexNormals` does for an indexed
/// geometry.
pub fn smooth_normals(vertices: &mut [Vertex], indices: &[u32]) {
    let mut sums = vec![[0.0f32; 3]; vertices.len()];
    for &[a, b, c] in indices.as_chunks::<3>().0 {
        let [a, b, c] = [a as usize, b as usize, c as usize];
        // The cross product's length is twice the area, so adding it
        // unnormalised is the area weighting.
        let weighted = cross(
            sub(vertices[b].position, vertices[a].position),
            sub(vertices[c].position, vertices[a].position),
        );
        for index in [a, b, c] {
            for axis in 0..3 {
                sums[index][axis] += weighted[axis];
            }
        }
    }
    for (vertex, sum) in vertices.iter_mut().zip(sums) {
        vertex.normal = normalized(sum).unwrap_or([0.0, 1.0, 0.0]);
    }
}

/// A convex polygon as a fan of triangles around its first vertex.
pub fn fan(polygon: &[u32], out: &mut Vec<u32>) {
    for pair in polygon.windows(2).skip(1) {
        out.extend([polygon[0], pair[0], pair[1]]);
    }
}

/// A strip's triangles, alternating winding so every face looks the same way.
pub fn strip(indices: &[u32], out: &mut Vec<u32>) {
    for (i, window) in indices.windows(3).enumerate() {
        if i % 2 == 0 {
            out.extend([window[0], window[1], window[2]]);
        } else {
            out.extend([window[1], window[0], window[2]]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, z: f32) -> Vertex {
        Vertex::at([x, y, z])
    }

    #[test]
    fn reads_the_normal_from_the_winding() {
        let up = face_normal([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]).unwrap();
        assert_eq!(up, [0.0, 1.0, 0.0]);
        assert_eq!(
            face_normal([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]),
            None
        );
    }

    #[test]
    fn splits_an_indexed_mesh_into_faceted_triangles() {
        let vertices = [
            at(0.0, 0.0, 0.0),
            at(1.0, 0.0, 0.0),
            at(0.0, 0.0, -1.0),
            at(1.0, 0.0, -1.0),
        ];
        let flat = flat_shaded(&vertices, &[0, 1, 2, 2, 1, 3]);
        assert_eq!(flat.len(), 6);
        assert!(flat.iter().all(|v| v.normal == [0.0, 1.0, 0.0]));
    }

    #[test]
    fn gives_a_triangle_with_no_area_a_usable_normal() {
        let vertices = [at(0.0, 0.0, 0.0), at(1.0, 0.0, 0.0), at(2.0, 0.0, 0.0)];
        let flat = flat_shaded(&vertices, &[0, 1, 2]);
        assert!(flat.iter().all(|v| v.normal.iter().all(|c| c.is_finite())));
    }

    #[test]
    fn averages_the_faces_around_a_vertex_by_their_area() {
        // A small face and a big one meet at vertex 0: the average leans to
        // the big one.
        let mut vertices = [
            at(0.0, 0.0, 0.0),
            at(1.0, 0.0, 0.0),
            at(0.0, 1.0, 0.0),
            at(0.0, 0.0, 10.0),
            at(10.0, 0.0, 0.0),
        ];
        smooth_normals(&mut vertices, &[0, 1, 2, 0, 3, 4]);
        let n = vertices[0].normal;
        assert!((n.iter().map(|c| c * c).sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(n[1].abs() > n[2].abs(), "{n:?}");
    }

    #[test]
    fn cuts_a_polygon_into_a_fan() {
        let mut out = Vec::new();
        fan(&[0, 1, 2, 3, 4], &mut out);
        assert_eq!(out, [0, 1, 2, 0, 2, 3, 0, 3, 4]);
        out.clear();
        fan(&[7, 8], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn keeps_a_strips_faces_looking_the_same_way() {
        let mut out = Vec::new();
        strip(&[0, 1, 2, 3, 4], &mut out);
        assert_eq!(out, [0, 1, 2, 2, 1, 3, 2, 3, 4]);
    }
}
