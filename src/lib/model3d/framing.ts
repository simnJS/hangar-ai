import type { SnapshotAngle } from "../modelStore";

/**
 * Where the camera goes to show a whole model, as plain arithmetic on
 * tuples: no three.js here, so the main bundle and the tests can use it.
 *
 * Models are taken as glTF lays them out — +Y up, the front facing +Z —
 * which is also what every loader the viewer uses hands back. "right" is the
 * camera on +X, as in Blender's numpad 3.
 */

export type Vec3 = [number, number, number];

export interface Box {
  min: Vec3;
  max: Vec3;
}

export interface ViewDirection {
  /** Unit vector from the model's centre toward the camera. */
  dir: Vec3;
  /** Which way is up on screen. Never parallel to `dir`. */
  up: Vec3;
}

/**
 * The three-quarter view every model opens on, and the "iso" snapshot: from
 * the front right, 30° above the horizon. Lower than a true isometric (35°),
 * which flattens a building into its roof.
 */
export const DEFAULT_AZIMUTH = Math.PI / 4;
export const DEFAULT_ELEVATION = Math.PI / 6;

/** A direction from an azimuth (0 = front, positive toward +X) and an elevation. */
export function directionFrom(azimuth: number, elevation: number): Vec3 {
  const flat = Math.cos(elevation);
  return [flat * Math.sin(azimuth), Math.sin(elevation), flat * Math.cos(azimuth)];
}

/**
 * The canonical views an agent can ask for. Looking straight down, the
 * model's front is at the bottom of the image; straight up, at the top — the
 * way a plan and a reflected ceiling plan are drawn.
 */
export function viewDirection(angle: SnapshotAngle): ViewDirection {
  switch (angle) {
    case "front":
      return { dir: [0, 0, 1], up: [0, 1, 0] };
    case "back":
      return { dir: [0, 0, -1], up: [0, 1, 0] };
    case "left":
      return { dir: [-1, 0, 0], up: [0, 1, 0] };
    case "right":
      return { dir: [1, 0, 0], up: [0, 1, 0] };
    case "top":
      return { dir: [0, 1, 0], up: [0, 0, -1] };
    case "bottom":
      return { dir: [0, -1, 0], up: [0, 0, 1] };
    case "iso":
      return { dir: directionFrom(DEFAULT_AZIMUTH, DEFAULT_ELEVATION), up: [0, 1, 0] };
  }
}

const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const length = (a: Vec3) => Math.hypot(a[0], a[1], a[2]);

export function normalize(a: Vec3): Vec3 {
  const l = length(a);
  return l > 0 ? [a[0] / l, a[1] / l, a[2] / l] : [0, 0, 1];
}

export const centerOf = (box: Box): Vec3 => [
  (box.min[0] + box.max[0]) / 2,
  (box.min[1] + box.max[1]) / 2,
  (box.min[2] + box.max[2]) / 2,
];

/** Half the box's diagonal: the radius of the sphere around it. */
export const radiusOf = (box: Box) => length(sub(box.max, box.min)) / 2;

/** The eight corners, which is all a box has to fit. */
function corners({ min, max }: Box): Vec3[] {
  const out: Vec3[] = [];
  for (const x of [min[0], max[0]])
    for (const y of [min[1], max[1]]) for (const z of [min[2], max[2]]) out.push([x, y, z]);
  return out;
}

/**
 * What is framed: the model's box, or better, points on the model itself —
 * a sample of its vertices. A box frames its empty corners too: a drone
 * flying high over a building stretches the box up, and the building ends
 * up low in the frame under a box's worth of sky.
 */
export type Shape = Box | Vec3[];

const pointsOf = (shape: Shape): Vec3[] => (Array.isArray(shape) ? shape : corners(shape));

function boundsOf(points: Vec3[]): Box {
  const min: Vec3 = [Infinity, Infinity, Infinity];
  const max: Vec3 = [-Infinity, -Infinity, -Infinity];
  for (const p of points) {
    for (let i = 0; i < 3; i++) {
      min[i] = Math.min(min[i], p[i]);
      max[i] = Math.max(max[i], p[i]);
    }
  }
  return points.length ? { min, max } : { min: [0, 0, 0], max: [0, 0, 0] };
}

/**
 * Distance from `center`, along `dir`, at which every point of the shape is
 * inside the camera's frustum with `margin` to spare on the tighter side.
 * Fitting points rather than the bounding sphere is what keeps a long, flat
 * model — a sword, a terrain tile — from sitting small in the middle of the
 * view.
 */
export function fitDistance(
  shape: Shape,
  view: ViewDirection,
  vfovDegrees: number,
  aspect: number,
  margin = 1.12,
  center?: Vec3,
): number {
  const points = pointsOf(shape);
  const bounds = Array.isArray(shape) ? boundsOf(points) : shape;
  const aim = center ?? centerOf(bounds);
  const { dir, right, up } = basis(view);
  const tanV = Math.tan((vfovDegrees * Math.PI) / 360) / margin;
  const tanH = tanV * Math.max(aspect, 1e-3);

  let distance = 0;
  for (const corner of points) {
    const offset = sub(corner, aim);
    // Depth toward the camera: a corner nearer to it needs the camera
    // further back to keep the same corner inside the frustum.
    const z = dot(offset, dir);
    distance = Math.max(
      distance,
      z + Math.abs(dot(offset, right)) / tanH,
      z + Math.abs(dot(offset, up)) / tanV,
    );
  }
  // A point, or a box flat in the view plane seen edge-on: still something
  // to stand back from.
  return Math.max(distance, radiusOf(bounds) * 0.5, 1e-3);
}

/** The camera's axes for a view: where it looks, its right, its up. */
function basis(view: ViewDirection) {
  const dir = normalize(view.dir);
  const right = normalize(cross(view.up, dir));
  return { dir, right, up: cross(dir, right) };
}

export interface Framing {
  /** What the camera looks at. */
  target: Vec3;
  position: Vec3;
  distance: number;
}

/**
 * The view of the whole shape from `view`, centred on what the camera sees
 * rather than on the middle of the model: seen from above, a building's
 * roof and its forecourt project unevenly around its centre, and aiming at
 * the centre would leave the model sitting low in the frame with empty sky
 * above it. The aim is moved across the view plane until the projection is
 * balanced, then the distance fitted again; two rounds settle it.
 */
export function frameView(
  shape: Shape,
  view: ViewDirection,
  vfovDegrees: number,
  aspect: number,
  margin = 1.12,
): Framing {
  const points = pointsOf(shape);
  const { dir, right, up } = basis(view);
  const tanV = Math.tan((vfovDegrees * Math.PI) / 360);
  const tanH = tanV * Math.max(aspect, 1e-3);
  let target = centerOf(Array.isArray(shape) ? boundsOf(points) : shape);
  let distance = fitDistance(points, view, vfovDegrees, aspect, margin, target);
  for (let round = 0; round < 2; round++) {
    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;
    for (const point of points) {
      const offset = sub(point, target);
      const depth = Math.max(distance - dot(offset, dir), 1e-6);
      const x = dot(offset, right) / (depth * tanH);
      const y = dot(offset, up) / (depth * tanV);
      minX = Math.min(minX, x);
      maxX = Math.max(maxX, x);
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
    // Screen offsets back to world units at the target's depth.
    const shiftX = ((minX + maxX) / 2) * distance * tanH;
    const shiftY = ((minY + maxY) / 2) * distance * tanV;
    target = [
      target[0] + right[0] * shiftX + up[0] * shiftY,
      target[1] + right[1] * shiftX + up[1] * shiftY,
      target[2] + right[2] * shiftX + up[2] * shiftY,
    ];
    distance = fitDistance(points, view, vfovDegrees, aspect, margin, target);
  }
  return {
    target,
    position: [
      target[0] + dir[0] * distance,
      target[1] + dir[1] * distance,
      target[2] + dir[2] * distance,
    ],
    distance,
  };
}

/**
 * Near and far planes for a camera `distance` from the centre of a model of
 * `radius`. Kept as tight as the view allows: depth precision is spread
 * between the two, and a near plane left at a tiny constant is what makes a
 * big model's coplanar faces flicker. The far plane leaves room for the
 * ground grid, which fades out well beyond the model.
 */
export function clipPlanes(distance: number, radius: number): { near: number; far: number } {
  const r = Math.max(radius, 1e-3);
  // A twentieth of the gap to the model's sphere, so the floor grid running
  // under the camera is not clipped either; zoomed into the model, a fixed
  // fraction of the distance.
  const near = Math.max(r * 1e-3, distance * 2e-3, (distance - r) * 0.05);
  const far = distance + r * 60;
  return { near, far };
}

/**
 * The grid's spacing: a power of ten that puts the model across a handful of
 * major cells, each split in ten minor ones, so a 6 m hangar sits on a metre
 * grid and a 20 cm prop on a centimetre one.
 */
export function gridSpacing(size: number): { minor: number; major: number } {
  const span = Math.max(size, 1e-4);
  const major = 10 ** Math.floor(Math.log10(span / 2));
  return { minor: major / 10, major };
}
