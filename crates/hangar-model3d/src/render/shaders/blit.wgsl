// Copies the finished picture into iced's frame, pixel for pixel: the model is
// drawn into a texture of its own (multisampled, with a depth buffer iced's
// pass does not have) and the widget's rectangle of the window is filled from
// it. A copy rather than a sample, so the picture is never resampled.

struct Blit {
    // The window pixel the texture's top-left corner lands on.
    origin: vec2<i32>,
    // The window's format is not sRGB: encode by hand.
    encode: u32,
    padding: u32,
};

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blit: Blit;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle that covers the screen.
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn encode_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(picture));
    let texel = clamp(vec2<i32>(position.xy) - blit.origin, vec2<i32>(0), size - vec2<i32>(1));
    var color = textureLoad(picture, texel, 0).rgb;
    if (blit.encode != 0u) {
        color = encode_srgb(color);
    }
    return vec4<f32>(color, 1.0);
}
