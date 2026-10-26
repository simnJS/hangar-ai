import { describe, expect, it } from "vitest";
import {
  clipPlanes,
  directionFrom,
  fitDistance,
  frameView,
  gridSpacing,
  viewDirection,
  type Box,
  type Vec3,
} from "./framing";
import type { SnapshotAngle } from "../modelStore";

const ANGLES: SnapshotAngle[] = ["front", "back", "left", "right", "top", "bottom", "iso"];
const unit: Box = { min: [-0.5, -0.5, -0.5], max: [0.5, 0.5, 0.5] };
const len = (v: Vec3) => Math.hypot(...v);
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

/** Where a point lands in normalised device coordinates, for a camera built like the engine's. */
function project(point: Vec3, box: Box, angle: SnapshotAngle, fov: number, aspect: number) {
  const view = viewDirection(angle);
  const { position: eye } = frameView(box, view, fov, aspect);
  const dir = view.dir.map((v) => v / len(view.dir)) as Vec3;
  const right: Vec3 = [
    view.up[1] * dir[2] - view.up[2] * dir[1],
    view.up[2] * dir[0] - view.up[0] * dir[2],
    view.up[0] * dir[1] - view.up[1] * dir[0],
  ];
  const r = right.map((v) => v / len(right)) as Vec3;
  const up: Vec3 = [
    dir[1] * r[2] - dir[2] * r[1],
    dir[2] * r[0] - dir[0] * r[2],
    dir[0] * r[1] - dir[1] * r[0],
  ];
  const rel: Vec3 = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
  const depth = -dot(rel, dir);
  const tanV = Math.tan((fov * Math.PI) / 360);
  return { x: dot(rel, r) / (depth * tanV * aspect), y: dot(rel, up) / (depth * tanV), depth };
}

function cornersOf({ min, max }: Box): Vec3[] {
  const out: Vec3[] = [];
  for (const x of [min[0], max[0]])
    for (const y of [min[1], max[1]]) for (const z of [min[2], max[2]]) out.push([x, y, z]);
  return out;
}

describe("viewDirection", () => {
  it("gives a unit direction and an up that is not along it, for every angle", () => {
    for (const angle of ANGLES) {
      const { dir, up } = viewDirection(angle);
      expect(len(dir)).toBeCloseTo(1, 9);
      expect(Math.abs(dot(dir, up))).toBeLessThan(0.99);
    }
  });

  it("puts the camera in front on +Z, on the right on +X, above on +Y", () => {
    expect(viewDirection("front").dir).toEqual([0, 0, 1]);
    expect(viewDirection("right").dir).toEqual([1, 0, 0]);
    expect(viewDirection("left").dir).toEqual([-1, 0, 0]);
    expect(viewDirection("top").dir).toEqual([0, 1, 0]);
  });

  it("draws a plan with the model's front at the bottom of the image", () => {
    // Seen from above, a point at the front (+Z) projects below the centre.
    const { y } = project([0, 0, 0.5], unit, "top", 35, 1);
    expect(y).toBeLessThan(0);
  });

  it("looks at the iso view from the front right and from above", () => {
    const { dir } = viewDirection("iso");
    expect(dir[0]).toBeGreaterThan(0);
    expect(dir[2]).toBeGreaterThan(0);
    expect(Math.asin(dir[1])).toBeCloseTo(Math.PI / 6, 9);
  });

  it("measures the azimuth from the front toward +X", () => {
    const side = directionFrom(Math.PI / 2, 0);
    expect(side[0]).toBeCloseTo(1, 9);
    expect(side[2]).toBeCloseTo(0, 9);
  });
});

describe("fitDistance", () => {
  it("keeps every corner inside the frame, for every angle and shape", () => {
    const boxes: Box[] = [
      unit,
      // A sword: long and thin.
      { min: [-0.05, 0, -1.2], max: [0.05, 0.1, 1.2] },
      // A terrain tile: wide and flat, off the origin.
      { min: [10, -0.2, 10], max: [30, 0.3, 26] },
    ];
    for (const box of boxes)
      for (const angle of ANGLES)
        for (const aspect of [0.4, 1, 2.2]) {
          for (const corner of cornersOf(box)) {
            const { x, y, depth } = project(corner, box, angle, 35, aspect);
            expect(depth).toBeGreaterThan(0);
            expect(Math.abs(x)).toBeLessThanOrEqual(1 + 1e-9);
            expect(Math.abs(y)).toBeLessThanOrEqual(1 + 1e-9);
          }
        }
  });

  it("fills the tighter side up to the margin, not the whole sphere", () => {
    const box: Box = { min: [-1, -1, -1], max: [1, 1, 1] };
    const corners = cornersOf(box).map((c) => project(c, box, "front", 35, 1));
    const reach = Math.max(...corners.map(({ x, y }) => Math.max(Math.abs(x), Math.abs(y))));
    expect(reach).toBeCloseTo(1 / 1.12, 2);
  });

  it("balances the projected model in the frame instead of aiming at its centre", () => {
    // A slab with a tall mast: seen from above at an angle, its middle is
    // nowhere near the middle of what the camera sees.
    const box: Box = { min: [-6, -0.2, -5], max: [6, 5, 6] };
    for (const aspect of [0.5, 1.6]) {
      const points = cornersOf(box).map((c) => project(c, box, "iso", 35, aspect));
      const xs = points.map((p) => p.x);
      const ys = points.map((p) => p.y);
      expect(Math.abs(Math.min(...xs) + Math.max(...xs))).toBeLessThan(0.02);
      expect(Math.abs(Math.min(...ys) + Math.max(...ys))).toBeLessThan(0.02);
    }
  });

  it("stands further back in a narrow panel than in a wide one", () => {
    const view = viewDirection("front");
    const wide = { min: [-2, -0.5, -0.5], max: [2, 0.5, 0.5] } as Box;
    expect(fitDistance(wide, view, 35, 0.5)).toBeGreaterThan(fitDistance(wide, view, 35, 2));
  });

  it("still steps back from a single point", () => {
    const point: Box = { min: [1, 2, 3], max: [1, 2, 3] };
    expect(fitDistance(point, viewDirection("iso"), 35, 1)).toBeGreaterThan(0);
  });
});

describe("framing points", () => {
  it("frames what is there, not the corners of its box", () => {
    // A slab and one small thing high above its far corner: the box is tall,
    // the content is not, and the slab should not sit low under empty sky.
    const slab = cornersOf({ min: [-6, -0.2, -5], max: [6, 0, 6] });
    const drone: Vec3[] = [
      [4, 5, -4],
      [4.5, 5.2, -4.5],
    ];
    const points = [...slab, ...drone];
    const box: Box = { min: [-6, -0.2, -5], max: [6, 5.2, 6] };
    const fromPoints = frameView(points, viewDirection("front"), 35, 4 / 3);
    const fromBox = frameView(box, viewDirection("front"), 35, 4 / 3);
    // The box's empty top front edge, nearest the camera, projects highest
    // of all; without it the aim comes down onto the slab.
    expect(fromPoints.target[1]).toBeLessThan(fromBox.target[1] - 0.3);
  });

  it("still keeps every point in the frame", () => {
    const points: Vec3[] = [
      [0, 0, 0],
      [3, 1, -2],
      [-1, 4, 1],
      [2, -1, 5],
    ];
    const view = viewDirection("iso");
    const { position, target } = frameView(points, view, 35, 0.6);
    const dir = view.dir.map((v) => v / len(view.dir)) as Vec3;
    const rightRaw: Vec3 = [
      view.up[1] * dir[2] - view.up[2] * dir[1],
      view.up[2] * dir[0] - view.up[0] * dir[2],
      view.up[0] * dir[1] - view.up[1] * dir[0],
    ];
    const right = rightRaw.map((v) => v / len(rightRaw)) as Vec3;
    const up: Vec3 = [
      dir[1] * right[2] - dir[2] * right[1],
      dir[2] * right[0] - dir[0] * right[2],
      dir[0] * right[1] - dir[1] * right[0],
    ];
    const tanV = Math.tan((35 * Math.PI) / 360);
    expect(target).toHaveLength(3);
    for (const p of points) {
      const rel: Vec3 = [p[0] - position[0], p[1] - position[1], p[2] - position[2]];
      const depth = -dot(rel, dir);
      expect(Math.abs(dot(rel, right) / (depth * tanV * 0.6))).toBeLessThanOrEqual(1);
      expect(Math.abs(dot(rel, up) / (depth * tanV))).toBeLessThanOrEqual(1);
    }
  });
});

describe("clipPlanes", () => {
  it("keeps the model between the planes", () => {
    for (const [distance, radius] of [
      [3, 1],
      [300, 100],
      [0.02, 0.01],
    ]) {
      const { near, far } = clipPlanes(distance, radius);
      expect(near).toBeGreaterThan(0);
      expect(near).toBeLessThan(distance - radius || distance);
      expect(far).toBeGreaterThan(distance + radius);
    }
  });

  it("keeps the depth range tight enough for a 24-bit buffer", () => {
    const { near, far } = clipPlanes(30, 10);
    expect(far / near).toBeLessThan(5000);
  });

  it("never reaches zero from inside the model", () => {
    expect(clipPlanes(0.5, 10).near).toBeGreaterThan(0);
  });
});

describe("gridSpacing", () => {
  it("puts a few major cells across the model", () => {
    expect(gridSpacing(6)).toEqual({ minor: 0.1, major: 1 });
    expect(gridSpacing(0.2).major).toBeCloseTo(0.1, 12);
    expect(gridSpacing(2400).major).toBe(1000);
  });
});
