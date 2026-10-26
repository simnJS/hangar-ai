import { Color, DoubleSide, Mesh, PlaneGeometry, ShaderMaterial, Vector2 } from "three";
import { gridSpacing } from "./framing";

/**
 * The floor under the model: minor and major lines drawn by a shader on one
 * large plane, rather than GridHelper's line segments.
 *
 * Lines are anti-aliased from their screen-space width, so they stay one
 * pixel thin at any distance instead of shimmering into moiré; minor lines
 * fade out where they would crowd closer than a few pixels, and everything
 * fades with distance from the model, so the floor has no hard edge.
 *
 * It lies a hair under the model's lowest point and is pushed back in depth,
 * so a model whose underside sits exactly on the floor — most of them —
 * always wins the depth test: no z-fighting from any side.
 */

const vertexShader = /* glsl */ `
  varying vec3 vWorld;
  #include <common>
  #include <logdepthbuf_pars_vertex>
  void main() {
    vec4 world = modelMatrix * vec4(position, 1.0);
    vWorld = world.xyz;
    gl_Position = projectionMatrix * viewMatrix * world;
    #include <logdepthbuf_vertex>
  }
`;

const fragmentShader = /* glsl */ `
  uniform vec3 uMinorColor;
  uniform vec3 uMajorColor;
  uniform float uMinor;
  uniform float uMajor;
  uniform float uOpacity;
  uniform vec2 uCenter;
  uniform float uFade;
  varying vec3 vWorld;
  #include <common>
  #include <logdepthbuf_pars_fragment>

  // Coverage of the nearest line of a grid of the given spacing, in [0, 1],
  // about one pixel wide whatever the distance.
  float lines(vec2 p, float spacing, out float density) {
    vec2 coord = p / spacing;
    vec2 width = fwidth(coord);
    vec2 g = abs(fract(coord - 0.5) - 0.5) / max(width, vec2(1e-6));
    density = max(width.x, width.y);
    return 1.0 - min(min(g.x, g.y), 1.0);
  }

  void main() {
    #include <logdepthbuf_fragment>
    float minorDensity;
    float majorDensity;
    float minor = lines(vWorld.xz, uMinor, minorDensity);
    float major = lines(vWorld.xz, uMajor, majorDensity);
    // Minor lines closer than about five pixels apart blur into a tint:
    // they fade out before that happens.
    minor *= 1.0 - smoothstep(0.12, 0.3, minorDensity);
    major *= 1.0 - smoothstep(0.25, 0.6, majorDensity);
    float fade = 1.0 - smoothstep(uFade * 0.3, uFade, distance(vWorld.xz, uCenter));
    fade *= fade;
    float alpha = max(minor * 0.55, major) * fade * uOpacity;
    if (alpha < 0.002) discard;
    vec3 color = mix(uMinorColor, uMajorColor, step(minor * 0.55, major));
    gl_FragColor = vec4(color, alpha);
    #include <colorspace_fragment>
  }
`;

export class FloorGrid extends Mesh<PlaneGeometry, ShaderMaterial> {
  constructor() {
    super(
      new PlaneGeometry(1, 1),
      new ShaderMaterial({
        vertexShader,
        fragmentShader,
        uniforms: {
          uMinorColor: { value: new Color(0x808080) },
          uMajorColor: { value: new Color(0xa0a0a0) },
          uMinor: { value: 0.1 },
          uMajor: { value: 1 },
          uOpacity: { value: 1 },
          uCenter: { value: new Vector2() },
          uFade: { value: 10 },
        },
        transparent: true,
        depthWrite: false,
        side: DoubleSide,
        // Behind whatever shares its plane, from above and from below.
        polygonOffset: true,
        polygonOffsetFactor: 1,
        polygonOffsetUnits: 4,
        // The colours are the theme's, as the chrome draws them.
        toneMapped: false,
      }),
    );
    this.rotation.x = -Math.PI / 2;
    this.renderOrder = -1;
    this.name = "hangar:grid";
  }

  /**
   * Lays the floor under a model: at its lowest point, across a couple of
   * its footprints, fading out from there — a floor to stand on, not a
   * plane to the horizon competing with the model.
   */
  fit(center: [number, number, number], size: [number, number, number], floor: number) {
    const footprint = Math.max(size[0], size[2], size[1] * 0.6, 1e-3);
    const { minor, major } = gridSpacing(Math.max(size[0], size[1], size[2]));
    const extent = footprint * 1.9;
    this.scale.set(extent * 2, extent * 2, 1);
    this.position.set(center[0], floor - Math.max(...size, 1e-3) * 1e-4, center[2]);
    const uniforms = this.material.uniforms;
    uniforms.uMinor.value = minor;
    uniforms.uMajor.value = major;
    uniforms.uFade.value = extent;
    (uniforms.uCenter.value as Vector2).set(center[0], center[2]);
    this.updateMatrixWorld();
  }

  setColors(minor: Color, major: Color, opacity: number) {
    const uniforms = this.material.uniforms;
    (uniforms.uMinorColor.value as Color).copy(minor);
    (uniforms.uMajorColor.value as Color).copy(major);
    uniforms.uOpacity.value = opacity;
  }

  dispose() {
    this.geometry.dispose();
    this.material.dispose();
  }
}
