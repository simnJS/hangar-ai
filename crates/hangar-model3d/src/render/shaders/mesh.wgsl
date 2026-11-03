// The model itself: metallic-roughness PBR under a studio made of a gradient
// and a few soft boxes, with one key light for the highlights. The look is the
// old viewer's — a room environment at 0.8, a key light of 1.6, Khronos' PBR
// Neutral tone mapping so base colours come out as authored — without the
// environment map: the studio is a function of the direction, so there is
// nothing to prefilter.
//
// Prepended with common.wgsl. `Material` mirrors `MaterialUniform` in upload.rs.

struct Material {
    base_color: vec4<f32>,
    // Emissive colour, and the occlusion map's strength.
    emissive: vec4<f32>,
    // Metallic, roughness, normal map scale, alpha cut-off.
    factors: vec4<f32>,
    // Which maps are there (bits below), the alpha mode (0 opaque, 1 mask,
    // 2 blend), and whether the material is drawn unlit.
    flags: vec4<u32>,
    // The texture transform: two rows of a 3x3 matrix.
    uv_x: vec4<f32>,
    uv_y: vec4<f32>,
};

const HAS_BASE: u32 = 1u;
const HAS_METAL_ROUGH: u32 = 2u;
const HAS_NORMAL: u32 = 4u;
const HAS_OCCLUSION: u32 = 8u;
const HAS_EMISSIVE: u32 = 16u;

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_tex: texture_2d<f32>;
@group(1) @binding(2) var metal_rough_tex: texture_2d<f32>;
@group(1) @binding(3) var normal_tex: texture_2d<f32>;
@group(1) @binding(4) var occlusion_tex: texture_2d<f32>;
@group(1) @binding(5) var emissive_tex: texture_2d<f32>;
@group(1) @binding(6) var base_smp: sampler;
@group(1) @binding(7) var metal_rough_smp: sampler;
@group(1) @binding(8) var normal_smp: sampler;
@group(1) @binding(9) var occlusion_smp: sampler;
@group(1) @binding(10) var emissive_smp: sampler;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

// A normal carried through the instance's matrix: the cofactor matrix, which
// is the inverse transpose up to a scale, so a squashed model keeps its
// normals true. A mirrored instance (negative determinant) turns them inside
// out and is flipped back.
fn world_normal(normal: vec3<f32>) -> vec3<f32> {
    let m0 = object.model[0].xyz;
    let m1 = object.model[1].xyz;
    let m2 = object.model[2].xyz;
    let c = cross(m1, m2) * normal.x + cross(m2, m0) * normal.y + cross(m0, m1) * normal.z;
    let det = dot(m0, cross(m1, m2));
    return normalize(c) * select(1.0, -1.0, det < 0.0);
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    let world = object.model * vec4<f32>(in.position, 1.0);
    out.world = world.xyz;
    out.clip = frame.view_proj * world;
    out.normal = world_normal(in.normal);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

// Points are drawn as small squares: a vertex stream stepped per instance, six
// vertices to a point. The size is in pixels, whatever the distance.
@vertex
fn vs_points(@builtin(vertex_index) corner: u32, in: VsIn) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    var out: VsOut;
    let world = object.model * vec4<f32>(in.position, 1.0);
    var clip = frame.view_proj * world;
    let offset = corners[corner] * (frame.viewport.z * 0.5) / frame.viewport.xy * 2.0;
    clip = vec4<f32>(clip.xy + offset * clip.w, clip.zw);
    out.world = world.xyz;
    out.clip = clip;
    out.normal = vec3<f32>(0.0, 1.0, 0.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

fn texture_uv(uv: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        dot(material.uv_x.xyz, vec3<f32>(uv, 1.0)),
        dot(material.uv_y.xyz, vec3<f32>(uv, 1.0)),
    );
}

// Khronos PBR Neutral tone mapping: it leaves colours alone below 0.76 and
// rolls the highlights off above it, desaturating as they go to white.
fn tone_map(input: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;
    var color = input;
    let x = min(color.r, min(color.g, color.b));
    var offset = 0.04;
    if (x < 0.08) {
        offset = x - 6.25 * x * x;
    }
    color = color - offset;
    let peak = max(color.r, max(color.g, color.b));
    if (peak < start_compression) {
        return color;
    }
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);
    color = color * (new_peak / peak);
    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(color, vec3<f32>(new_peak), g);
}

const PI: f32 = 3.14159265;
const SKY: vec3<f32> = vec3<f32>(0.78, 0.80, 0.85);
const GROUND: vec3<f32> = vec3<f32>(0.32, 0.30, 0.28);
const ENVIRONMENT: f32 = 0.8;
const KEY_INTENSITY: f32 = 1.6;

// The light a diffuse surface facing `n` collects from the studio.
fn irradiance(n: vec3<f32>) -> vec3<f32> {
    return mix(GROUND, SKY, smoothstep(0.0, 1.0, n.y * 0.5 + 0.5));
}

// What a mirror-ish surface sees in direction `dir`: the studio's gradient and
// three soft boxes, spread wider and dimmer as the surface gets rougher.
fn environment(dir: vec3<f32>, roughness: f32) -> vec3<f32> {
    let blur = roughness * roughness;
    var color = mix(GROUND, SKY, smoothstep(-0.25 - blur, 0.25 + blur, dir.y));
    let softness = 1.0 / (1.0 + blur * 12.0);
    let inner = cos(0.30 + blur * 1.3);
    let outer = cos(0.55 + blur * 1.6);
    color += vec3<f32>(1.0, 0.98, 0.95) * 5.0 * softness
        * smoothstep(outer, inner, dot(dir, normalize(vec3<f32>(0.5, 1.0, 0.6))));
    color += vec3<f32>(0.9, 0.95, 1.0) * 2.5 * softness
        * smoothstep(outer, inner, dot(dir, normalize(vec3<f32>(-1.0, 0.4, 0.2))));
    color += vec3<f32>(1.0, 0.95, 0.9) * 2.0 * softness
        * smoothstep(outer, inner, dot(dir, normalize(vec3<f32>(0.2, 0.3, -1.0))));
    return color;
}

fn distribution_ggx(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn visibility_smith(n_dot_l: f32, n_dot_v: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let gv = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - a2) + a2);
    let gl = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

// Karis' analytic fit of the split-sum environment BRDF: what the studio's
// reflection is scaled by.
fn environment_brdf(f0: vec3<f32>, roughness: f32, n_dot_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

// A normal map without tangents in the mesh: the tangent frame comes from how
// the texture coordinates change across the screen, as the glTF sample viewer
// does it.
fn perturb_normal(n: vec3<f32>, world: vec3<f32>, uv: vec2<f32>, packed: vec3<f32>, scale: f32) -> vec3<f32> {
    let dp1 = dpdx(world);
    let dp2 = dpdy(world);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    let det = duv1.x * duv2.y - duv2.x * duv1.y;
    if (abs(det) < 1e-14) {
        return n;
    }
    let t_raw = (duv2.y * dp1 - duv1.y * dp2) / det;
    let t = normalize(t_raw - n * dot(n, t_raw));
    let b = cross(n, t);
    let m = packed * 2.0 - 1.0;
    return normalize(t * (m.x * scale) + b * (m.y * scale) + n * m.z);
}

fn base_color(in: VsOut, uv: vec2<f32>) -> vec4<f32> {
    var base = material.base_color * in.color;
    if ((material.flags.x & HAS_BASE) != 0u) {
        base = base * textureSample(base_tex, base_smp, uv);
    }
    return base;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let uv = texture_uv(in.uv);
    let base = base_color(in, uv);
    let mode = material.flags.y;
    if (mode == 1u && base.a < material.factors.w) {
        discard;
    }
    var alpha = 1.0;
    if (mode == 2u) {
        alpha = base.a;
    }
    if (material.flags.z != 0u) {
        return vec4<f32>(tone_map(base.rgb), alpha);
    }

    var metallic = material.factors.x;
    var roughness = material.factors.y;
    if ((material.flags.x & HAS_METAL_ROUGH) != 0u) {
        let packed = textureSample(metal_rough_tex, metal_rough_smp, uv);
        roughness = roughness * packed.g;
        metallic = metallic * packed.b;
    }
    roughness = clamp(roughness, 0.045, 1.0);
    metallic = clamp(metallic, 0.0, 1.0);

    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    if ((material.flags.x & HAS_NORMAL) != 0u) {
        let packed = textureSample(normal_tex, normal_smp, uv).xyz;
        n = perturb_normal(n, in.world, uv, packed, material.factors.z);
    }

    let v = normalize(frame.eye.xyz - in.world);
    let n_dot_v = max(dot(n, v), 1e-4);
    let alpha_r = roughness * roughness;
    let diffuse_color = base.rgb * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), base.rgb, metallic);

    // The key light.
    let l = frame.key_dir.xyz;
    let h = normalize(l + v);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_h = max(dot(n, h), 0.0);
    let v_dot_h = max(dot(v, h), 0.0);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - v_dot_h, 5.0);
    let specular = fresnel * distribution_ggx(n_dot_h, alpha_r) * visibility_smith(n_dot_l, n_dot_v, alpha_r);
    var color = (diffuse_color * (1.0 - fresnel) / PI + specular) * KEY_INTENSITY * n_dot_l;

    // The studio.
    var ambient = diffuse_color * irradiance(n) * ENVIRONMENT
        + environment(reflect(-v, n), roughness) * environment_brdf(f0, roughness, n_dot_v) * ENVIRONMENT;
    if ((material.flags.x & HAS_OCCLUSION) != 0u) {
        let occlusion = textureSample(occlusion_tex, occlusion_smp, uv).r;
        ambient = ambient * (1.0 + material.emissive.w * (occlusion - 1.0));
    }
    color = color + ambient;

    var emissive = material.emissive.rgb;
    if ((material.flags.x & HAS_EMISSIVE) != 0u) {
        emissive = emissive * textureSample(emissive_tex, emissive_smp, uv).rgb;
    }
    color = color + emissive;
    return vec4<f32>(tone_map(color), alpha);
}

// Lines and points: in their own colour, unlit, as the old viewer's basic
// materials were.
@fragment
fn fs_unlit(in: VsOut) -> @location(0) vec4<f32> {
    let base = base_color(in, texture_uv(in.uv));
    return vec4<f32>(tone_map(base.rgb), base.a);
}
