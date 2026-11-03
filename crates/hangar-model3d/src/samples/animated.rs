//! Animated glTF models for the tests and the `view` example: a cube that
//! slides and turns, a box skinned to two joints, and a box with a morph
//! target. Each is small enough to check by arithmetic.

use super::{cube_vertices, glb};

/// A glTF's binary chunk and the accessors over it, built a piece at a time.
#[derive(Default)]
struct Pieces {
    bin: Vec<u8>,
    views: Vec<String>,
    accessors: Vec<String>,
}

type Bounds = Option<(Vec<f32>, Vec<f32>)>;

impl Pieces {
    /// Adds `bytes` as a buffer view and an accessor over it; returns the
    /// accessor's index. `bounds` is the min and max a position accessor must
    /// carry.
    fn accessor(
        &mut self,
        bytes: &[u8],
        component: u32,
        count: usize,
        kind: &str,
        bounds: Bounds,
    ) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend(bytes);
        self.views.push(format!(
            r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{}}}"#,
            bytes.len()
        ));
        let bounds = bounds.map_or(String::new(), |(min, max)| {
            format!(r#","min":{min:?},"max":{max:?}"#)
        });
        self.accessors.push(format!(
            r#"{{"bufferView":{},"componentType":{component},"count":{count},"type":"{kind}"{bounds}}}"#,
            self.views.len() - 1
        ));
        self.accessors.len() - 1
    }

    fn floats(&mut self, data: &[f32], kind: &str, bounds: Bounds) -> usize {
        let width = match kind {
            "SCALAR" => 1,
            "VEC3" => 3,
            "VEC4" => 4,
            other => panic!("no such accessor type {other}"),
        };
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.accessor(&bytes, 5126, data.len() / width, kind, bounds)
    }

    /// Keyframe times, with the min and max a sampler's input carries.
    fn times(&mut self, times: &[f32]) -> usize {
        let min = times.iter().copied().fold(f32::INFINITY, f32::min);
        let max = times.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        self.floats(times, "SCALAR", Some((vec![min], vec![max])))
    }

    /// The unit cube of 24 vertices: positions, normals and indices.
    fn cube(&mut self) -> [usize; 3] {
        let (vertices, indices) = cube_vertices();
        let positions: Vec<f32> = vertices.iter().flat_map(|v| v.position).collect();
        let normals: Vec<f32> = vertices.iter().flat_map(|v| v.normal).collect();
        let position = self.floats(&positions, "VEC3", Some((vec![-0.5; 3], vec![0.5; 3])));
        let normal = self.floats(&normals, "VEC3", None);
        let index_bytes: Vec<u8> = indices
            .iter()
            .flat_map(|i| (*i as u16).to_le_bytes())
            .collect();
        let index = self.accessor(&index_bytes, 5123, indices.len(), "SCALAR", None);
        [position, normal, index]
    }

    /// The glb: `json` has `{ACCESSORS}`, `{VIEWS}` and `{BUFFER}` to fill.
    fn glb(&self, json: &str) -> Vec<u8> {
        let json = json
            .replace("{ACCESSORS}", &self.accessors.join(","))
            .replace("{VIEWS}", &self.views.join(","))
            .replace("{BUFFER}", &self.bin.len().to_string());
        glb(&json, &self.bin)
    }
}

/// A cube that plays two clips: "Slide", two seconds out two units along X and
/// back, and "Spin", a second of a quarter turn about Y.
pub fn animated_cube_glb() -> Vec<u8> {
    let mut pieces = Pieces::default();
    let [position, normal, index] = pieces.cube();
    let slide_times = pieces.times(&[0.0, 1.0, 2.0]);
    let slide = pieces.floats(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0], "VEC3", None);
    let spin_times = pieces.times(&[0.0, 1.0]);
    let half_angle = std::f32::consts::FRAC_PI_4;
    let spin = pieces.floats(
        &[
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            half_angle.sin(),
            0.0,
            half_angle.cos(),
        ],
        "VEC4",
        None,
    );
    pieces.glb(&format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, "name": "cube"}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": {position}, "NORMAL": {normal}}}, "indices": {index}}}]}}],
  "animations": [
    {{"name": "Slide",
      "samplers": [{{"input": {slide_times}, "output": {slide}, "interpolation": "LINEAR"}}],
      "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "translation"}}}}]}},
    {{"name": "Spin",
      "samplers": [{{"input": {spin_times}, "output": {spin}, "interpolation": "LINEAR"}}],
      "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "rotation"}}}}]}}
  ],
  "accessors": [{{ACCESSORS}}],
  "bufferViews": [{{VIEWS}}],
  "buffers": [{{"byteLength": {{BUFFER}}}}]
}}"#
    ))
}

/// A unit cube skinned to two joints at the origin: the vertices of its top
/// half follow the upper joint, the others the lower. The clip "Stretch" lifts
/// the upper joint two units in a second, so the box grows from one unit tall
/// to three.
pub fn skinned_glb() -> Vec<u8> {
    skinned(true)
}

/// The same box, skinned and with no clip: posed once as its nodes stand.
pub fn skinned_still_glb() -> Vec<u8> {
    skinned(false)
}

fn skinned(animated: bool) -> Vec<u8> {
    let mut pieces = Pieces::default();
    let [position, normal, index] = pieces.cube();
    let (vertices, _) = cube_vertices();
    let joint_bytes: Vec<u8> = vertices
        .iter()
        .flat_map(|v| [u8::from(v.position[1] > 0.0), 0, 0, 0])
        .collect();
    let joints = pieces.accessor(&joint_bytes, 5121, vertices.len(), "VEC4", None);
    let weight_data: Vec<f32> = vertices.iter().flat_map(|_| [1.0, 0.0, 0.0, 0.0]).collect();
    let weights = pieces.floats(&weight_data, "VEC4", None);
    let times = pieces.times(&[0.0, 1.0]);
    let lift = pieces.floats(&[0.0, 0.0, 0.0, 0.0, 2.0, 0.0], "VEC3", None);
    // The upper joint sits where the clip's first frame puts it, so a still
    // file and the first frame of the animated one are the same box.
    let animations = if animated {
        format!(
            r#""animations": [
    {{"name": "Stretch",
      "samplers": [{{"input": {times}, "output": {lift}, "interpolation": "LINEAR"}}],
      "channels": [{{"sampler": 0, "target": {{"node": 2, "path": "translation"}}}}]}}
  ],"#
        )
    } else {
        String::new()
    };
    pieces.glb(&format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0, 1]}}],
  "nodes": [
    {{"mesh": 0, "skin": 0, "name": "skinned box"}},
    {{"name": "lower", "children": [2]}},
    {{"name": "upper"}}
  ],
  "skins": [{{"joints": [1, 2]}}],
  "meshes": [{{"primitives": [{{"attributes": {{"POSITION": {position}, "NORMAL": {normal}, "JOINTS_0": {joints}, "WEIGHTS_0": {weights}}}, "indices": {index}}}]}}],
  {animations}
  "accessors": [{{ACCESSORS}}],
  "bufferViews": [{{VIEWS}}],
  "buffers": [{{"byteLength": {{BUFFER}}}}]
}}"#
    ))
}

/// A unit cube with one morph target that lifts the vertices of its top half
/// by two units at weight one. It starts at weight zero; the clip "Grow" takes
/// it to one in a second, so the box grows from one unit tall to three.
pub fn morphing_glb() -> Vec<u8> {
    morphing(0.0, true)
}

/// The same box with no clip, standing at the given weight of its target.
pub fn morphing_still_glb(weight: f32) -> Vec<u8> {
    morphing(weight, false)
}

fn morphing(weight: f32, animated: bool) -> Vec<u8> {
    let mut pieces = Pieces::default();
    let [position, normal, index] = pieces.cube();
    let (vertices, _) = cube_vertices();
    let lift: Vec<f32> = vertices
        .iter()
        .flat_map(|v| [0.0, if v.position[1] > 0.0 { 2.0 } else { 0.0 }, 0.0])
        .collect();
    let target = pieces.floats(&lift, "VEC3", Some((vec![0.0; 3], vec![0.0, 2.0, 0.0])));
    let times = pieces.times(&[0.0, 1.0]);
    let weights = pieces.floats(&[0.0, 1.0], "SCALAR", None);
    let animations = if animated {
        format!(
            r#""animations": [
    {{"name": "Grow",
      "samplers": [{{"input": {times}, "output": {weights}, "interpolation": "LINEAR"}}],
      "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "weights"}}}}]}}
  ],"#
        )
    } else {
        String::new()
    };
    pieces.glb(&format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"mesh": 0, "name": "morphing box"}}],
  "meshes": [{{"weights": [{weight:?}], "primitives": [{{"attributes": {{"POSITION": {position}, "NORMAL": {normal}}}, "indices": {index}, "targets": [{{"POSITION": {target}}}]}}]}}],
  {animations}
  "accessors": [{{ACCESSORS}}],
  "bufferViews": [{{VIEWS}}],
  "buffers": [{{"byteLength": {{BUFFER}}}}]
}}"#
    ))
}
