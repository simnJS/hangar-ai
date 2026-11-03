// What every pass of the viewer shares: the camera, and where the thing being
// drawn stands. The structs mirror `FrameUniform` in frame.rs; keep them in
// step.
//
// Both are in group 0 because iced's device allows two bind groups and no more:
// group 0 is the scene (the camera, and the object's matrix at a dynamic
// offset), group 1 is whatever the pass draws with (a material, a line style,
// the grid).

struct Frame {
    view_proj: mat4x4<f32>,
    // The camera's position in world space.
    eye: vec4<f32>,
    // Unit vector from the model toward the key light.
    key_dir: vec4<f32>,
    // Width and height of the target in pixels, and the size points are drawn at.
    viewport: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct Object {
    model: mat4x4<f32>,
};

@group(0) @binding(1) var<uniform> object: Object;
