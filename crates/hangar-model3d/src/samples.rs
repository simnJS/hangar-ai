//! Small models written out in every format, for the tests and the `view`
//! example: a unit cube, so a reader's output can be checked by eye and by
//! arithmetic. Nothing here is read from disk.

use std::fmt::Write as _;

use crate::model::Vertex;

mod animated;

pub use animated::{
    animated_cube_glb, morphing_glb, morphing_still_glb, skinned_glb, skinned_still_glb,
};

/// The eight corners of a cube of side one centred on the origin: bit 0 is +X,
/// bit 1 is +Y, bit 2 is +Z.
pub const CORNERS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [-0.5, 0.5, -0.5],
    [0.5, 0.5, -0.5],
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [-0.5, 0.5, 0.5],
    [0.5, 0.5, 0.5],
];

/// Six quads of [`CORNERS`], each wound counter-clockwise seen from outside.
pub const QUADS: [[usize; 4]; 6] = [
    [4, 5, 7, 6], // +Z
    [1, 0, 2, 3], // -Z
    [5, 1, 3, 7], // +X
    [0, 4, 6, 2], // -X
    [2, 6, 7, 3], // +Y
    [0, 1, 5, 4], // -Y
];

/// The cube as twelve triangles over the eight corners.
pub fn cube_triangles() -> Vec<[usize; 3]> {
    QUADS
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect()
}

/// A cube with four vertices to a face, so each has its own normal and its own
/// texture coordinates: 24 vertices, 36 indices, as glTF exporters write one.
pub fn cube_vertices() -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for quad in QUADS {
        let a = CORNERS[quad[0]];
        let b = CORNERS[quad[1]];
        let c = CORNERS[quad[2]];
        let normal = crate::load::face_normal(a, b, c).unwrap_or([0.0, 1.0, 0.0]);
        let base = vertices.len() as u32;
        for (corner, uv) in quad.iter().zip(uvs) {
            vertices.push(Vertex {
                position: CORNERS[*corner],
                normal,
                uv,
                color: Vertex::WHITE,
            });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// A binary glTF from its JSON and its binary chunk.
pub fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    fn padded(mut bytes: Vec<u8>, fill: u8) -> Vec<u8> {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(fill);
        }
        bytes
    }
    let json = padded(json.as_bytes().to_vec(), b' ');
    let bin = padded(bin.to_vec(), 0);
    let total = 12 + 8 + json.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(json);
    if !bin.is_empty() {
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(bin);
    }
    out
}

/// The geometry chunk of the glTF cubes: positions, normals, texture
/// coordinates, then indices, each in its own buffer view.
fn cube_buffer() -> Vec<u8> {
    let (vertices, indices) = cube_vertices();
    let mut bin = Vec::new();
    for v in &vertices {
        v.position.iter().for_each(|c| bin.extend(c.to_le_bytes()));
    }
    for v in &vertices {
        v.normal.iter().for_each(|c| bin.extend(c.to_le_bytes()));
    }
    for v in &vertices {
        v.uv.iter().for_each(|c| bin.extend(c.to_le_bytes()));
    }
    for i in &indices {
        bin.extend((*i as u16).to_le_bytes());
    }
    bin
}

const CUBE_VIEWS: &str = r#"
    {"buffer":0,"byteOffset":0,"byteLength":288,"target":34962},
    {"buffer":0,"byteOffset":288,"byteLength":288,"target":34962},
    {"buffer":0,"byteOffset":576,"byteLength":192,"target":34962},
    {"buffer":0,"byteOffset":768,"byteLength":72,"target":34963}"#;

const CUBE_ACCESSORS: &str = r#"
    {"bufferView":0,"componentType":5126,"count":24,"type":"VEC3","min":[-0.5,-0.5,-0.5],"max":[0.5,0.5,0.5]},
    {"bufferView":1,"componentType":5126,"count":24,"type":"VEC3"},
    {"bufferView":2,"componentType":5126,"count":24,"type":"VEC2"},
    {"bufferView":3,"componentType":5123,"count":36,"type":"SCALAR"}"#;

/// A red cube, as a binary glTF with its buffer inside.
pub fn cube_glb() -> Vec<u8> {
    cube_glb_with(
        r#"{"name": "red", "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.1, 0.1, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.5}}"#,
    )
}

/// The same cube in a material of the caller's, as glTF JSON: for trying
/// alpha modes, emissive colours and the rest.
pub fn cube_glb_with(material: &str) -> Vec<u8> {
    let json = format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, "name": "cube"}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}}, "indices": 3, "material": 0}}]}}],
  "materials": [{material}],
  "accessors": [{CUBE_ACCESSORS}],
  "bufferViews": [{CUBE_VIEWS}],
  "buffers": [{{"byteLength": 840}}]
}}"#
    );
    glb(&json, &cube_buffer())
}

/// A checkerboard PNG of two by two pixels, two shades of a colour.
pub fn checker_png(a: [u8; 3], b: [u8; 3]) -> Vec<u8> {
    let mut image = image::RgbaImage::new(2, 2);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let [r, g, bl] = if (x + y) % 2 == 0 { a } else { b };
        *pixel = image::Rgba([r, g, bl, 255]);
    }
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("a PNG of two pixels encodes");
    out
}

/// A cube with a checkerboard texture inside the file, a tangent-space normal
/// map beside it, and a second cube sharing the mesh: the mesh stands twice.
pub fn textured_cube_glb() -> Vec<u8> {
    let mut bin = cube_buffer();
    let texture = checker_png([230, 230, 230], [40, 90, 200]);
    let normal_map = checker_png([128, 128, 255], [160, 128, 230]);
    let texture_at = bin.len();
    bin.extend(&texture);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let normal_at = bin.len();
    bin.extend(&normal_map);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let json = format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0, 1]}}],
  "nodes": [
    {{"mesh": 0, "name": "left", "translation": [-1.0, 0.0, 0.0]}},
    {{"mesh": 0, "name": "right", "translation": [1.0, 0.0, 0.0], "scale": [1.0, 2.0, 1.0]}}
  ],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}}, "indices": 3, "material": 0}}]}}],
  "materials": [{{"name": "checker", "pbrMetallicRoughness": {{"baseColorTexture": {{"index": 0}}, "metallicFactor": 0.0, "roughnessFactor": 0.6}}, "normalTexture": {{"index": 1, "scale": 1.0}}}}],
  "textures": [{{"source": 0, "sampler": 0}}, {{"source": 1}}],
  "samplers": [{{"magFilter": 9728, "minFilter": 9728, "wrapS": 10497, "wrapT": 33071}}],
  "images": [
    {{"bufferView": 4, "mimeType": "image/png"}},
    {{"bufferView": 5, "mimeType": "image/png"}}
  ],
  "accessors": [{CUBE_ACCESSORS}],
  "bufferViews": [{CUBE_VIEWS},
    {{"buffer":0,"byteOffset":{texture_at},"byteLength":{}}},
    {{"buffer":0,"byteOffset":{normal_at},"byteLength":{}}}],
  "buffers": [{{"byteLength": {}}}]
}}"#,
        texture.len(),
        normal_map.len(),
        bin.len()
    );
    glb(&json, &bin)
}

/// A cube as a `.gltf` with its buffer and its texture as data URIs, the way a
/// tool that wants one file without GLB writes it.
pub fn cube_gltf_with_data_uris() -> String {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let buffer = engine.encode(cube_buffer());
    format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}}, "indices": 3}}]}}],
  "accessors": [{CUBE_ACCESSORS}],
  "bufferViews": [{CUBE_VIEWS}],
  "buffers": [{{"byteLength": 840, "uri": "data:application/octet-stream;base64,{buffer}"}}]
}}"#
    )
}

/// A cube as an OBJ with quads, two materials and a material library, and the
/// `.mtl` it names.
pub fn cube_obj() -> (String, String) {
    let mut obj = String::from("# a cube\nmtllib cube.mtl\no cube\n");
    for [x, y, z] in CORNERS {
        writeln!(obj, "v {x} {y} {z}").unwrap();
    }
    for (i, quad) in QUADS.iter().enumerate() {
        // The last two faces use the second material.
        if i == 0 {
            obj.push_str("usemtl red\n");
        } else if i == 4 {
            obj.push_str("usemtl blue\n");
        }
        let [a, b, c, d] = quad.map(|corner| corner + 1);
        writeln!(obj, "f {a} {b} {c} {d}").unwrap();
    }
    let mtl = "newmtl red\nKd 0.8 0.1 0.1\nNs 30\n\nnewmtl blue\nKd 0.1 0.2 0.9\nKe 0.0 0.0 0.1\nNs 200\nd 1.0\n"
        .to_string();
    (obj, mtl)
}

/// A binary STL of the cube, 12 facets.
pub fn cube_stl_binary() -> Vec<u8> {
    let triangles = cube_triangles();
    let mut out = vec![0u8; 80];
    out.extend((triangles.len() as u32).to_le_bytes());
    for triangle in &triangles {
        out.extend([0u8; 12]);
        for corner in triangle {
            CORNERS[*corner]
                .iter()
                .for_each(|c| out.extend(c.to_le_bytes()));
        }
        out.extend([0u8; 2]);
    }
    out
}

/// The same with a header that starts "solid", as many exporters write, so a
/// reader that decides by the header alone takes it for text.
pub fn cube_stl_binary_with_solid_header() -> Vec<u8> {
    let mut out = cube_stl_binary();
    let label = b"solid cube exported by a tool";
    out[..label.len()].copy_from_slice(label);
    out
}

/// An ASCII STL of the cube.
pub fn cube_stl_ascii() -> String {
    let mut out = String::from("solid cube\n");
    for triangle in cube_triangles() {
        out.push_str("  facet normal 0 0 0\n    outer loop\n");
        for corner in triangle {
            let [x, y, z] = CORNERS[corner];
            writeln!(out, "      vertex {x} {y} {z}").unwrap();
        }
        out.push_str("    endloop\n  endfacet\n");
    }
    out.push_str("endsolid cube\n");
    out
}

/// A PLY of the cube in ASCII: eight coloured vertices and six quads.
pub fn cube_ply_ascii() -> String {
    let mut out = String::from(
        "ply\nformat ascii 1.0\ncomment a cube\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 6\nproperty list uchar int vertex_indices\nend_header\n",
    );
    for (i, [x, y, z]) in CORNERS.iter().enumerate() {
        writeln!(out, "{x} {y} {z} {} {} {}", i * 30, 255 - i * 30, 128).unwrap();
    }
    for [a, b, c, d] in QUADS {
        writeln!(out, "4 {a} {b} {c} {d}").unwrap();
    }
    out
}

/// The same PLY in binary, little- or big-endian.
pub fn cube_ply_binary(big_endian: bool) -> Vec<u8> {
    let format = if big_endian {
        "binary_big_endian"
    } else {
        "binary_little_endian"
    };
    let mut out = format!(
        "ply\nformat {format} 1.0\nelement vertex 8\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nelement face 12\nproperty list uchar uint vertex_indices\nend_header\n"
    )
    .into_bytes();
    let float = |v: f32| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    for corner in CORNERS {
        corner.iter().for_each(|c| out.extend(float(*c)));
        // Pointing out of the corner, which is what a scanner's normal does.
        corner.iter().for_each(|c| out.extend(float(c * 2.0)));
    }
    for triangle in cube_triangles() {
        out.push(3);
        for corner in triangle {
            let corner = corner as u32;
            out.extend(if big_endian {
                corner.to_be_bytes()
            } else {
                corner.to_le_bytes()
            });
        }
    }
    out
}

/// A PLY with vertices and no faces: the point cloud a scanner writes.
pub fn point_cloud_ply(points: usize) -> String {
    let mut out = format!(
        "ply\nformat ascii 1.0\nelement vertex {points}\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n"
    );
    for i in 0..points {
        let t = i as f32 / points as f32 * std::f32::consts::TAU;
        writeln!(
            out,
            "{} {} {} 255 {} 0",
            t.cos(),
            t.sin(),
            t * 0.1,
            (i * 7) % 256
        )
        .unwrap();
    }
    out
}
