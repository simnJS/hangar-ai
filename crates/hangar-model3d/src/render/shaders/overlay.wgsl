// Lines drawn over the model in one colour: the wireframe, the normals, the
// axes. Prepended with common.wgsl.

struct Style {
    color: vec4<f32>,
};

@group(1) @binding(0) var<uniform> style: Style;

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return frame.view_proj * object.model * vec4<f32>(position, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return style.color;
}
