// The floor under the model: minor and major lines drawn by a shader on one
// large quad.
//
// Lines are anti-aliased from their screen-space width, so they stay one pixel
// thin at any distance instead of shimmering into moiré; minor lines fade out
// where they would crowd closer than a few pixels, and everything fades with
// distance from the model, so the floor has no hard edge. Prepended with
// common.wgsl (the object group is unused here).

struct Grid {
    // The quad's centre (x, floor height, z) and its half-size.
    centre: vec4<f32>,
    // Minor spacing, major spacing, the distance the fade ends at, opacity.
    params: vec4<f32>,
    minor_color: vec4<f32>,
    major_color: vec4<f32>,
};

@group(1) @binding(0) var<uniform> grid: Grid;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) corner: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[corner];
    let world = vec3<f32>(
        grid.centre.x + c.x * grid.centre.w,
        grid.centre.y,
        grid.centre.z + c.y * grid.centre.w,
    );
    var out: VsOut;
    out.world = world;
    out.clip = frame.view_proj * vec4<f32>(world, 1.0);
    return out;
}

// Coverage of the nearest line of a grid of the given spacing, in [0, 1],
// about one pixel wide whatever the distance; and how crowded the lines are.
fn lines(p: vec2<f32>, spacing: f32) -> vec2<f32> {
    let coord = p / spacing;
    let width = fwidth(coord);
    let g = abs(fract(coord - 0.5) - 0.5) / max(width, vec2<f32>(1e-6));
    return vec2<f32>(1.0 - min(min(g.x, g.y), 1.0), max(width.x, width.y));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let minor_lines = lines(in.world.xz, grid.params.x);
    let major_lines = lines(in.world.xz, grid.params.y);
    // Minor lines closer than about five pixels apart blur into a tint: they
    // fade out before that happens.
    let minor = minor_lines.x * (1.0 - smoothstep(0.12, 0.3, minor_lines.y));
    let major = major_lines.x * (1.0 - smoothstep(0.25, 0.6, major_lines.y));
    var fade = 1.0 - smoothstep(grid.params.z * 0.3, grid.params.z, distance(in.world.xz, grid.centre.xz));
    fade = fade * fade;
    let alpha = max(minor * 0.55, major) * fade * grid.params.w;
    if (alpha < 0.002) {
        discard;
    }
    let color = mix(grid.minor_color.rgb, grid.major_color.rgb, step(minor * 0.55, major));
    return vec4<f32>(color, alpha);
}
