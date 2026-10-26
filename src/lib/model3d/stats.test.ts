import { describe, expect, it } from "vitest";
import {
  AnimationClip,
  BoxGeometry,
  BufferAttribute,
  BufferGeometry,
  Group,
  InstancedMesh,
  LineSegments,
  Mesh,
  MeshStandardMaterial,
  Points,
  PointsMaterial,
  VectorKeyframeTrack,
} from "three";
import { clipNames, countGeometry, modelInfo } from "./stats";

const box = () => new BoxGeometry(1, 1, 1);

describe("countGeometry", () => {
  it("counts an indexed box as twelve triangles over twenty-four vertices", () => {
    const counts = countGeometry(new Mesh(box(), new MeshStandardMaterial()));
    expect(counts).toEqual({ triangles: 12, vertices: 24, meshes: 1, materials: 1 });
  });

  it("counts a non-indexed geometry by its vertices", () => {
    const geometry = box().toNonIndexed();
    expect(countGeometry(new Mesh(geometry)).triangles).toBe(12);
  });

  it("multiplies an instanced mesh by its instances, but counts it once as a mesh", () => {
    const counts = countGeometry(new InstancedMesh(box(), new MeshStandardMaterial(), 5));
    expect(counts.triangles).toBe(60);
    expect(counts.vertices).toBe(120);
    expect(counts.meshes).toBe(1);
  });

  it("leaves out what the draw range leaves out", () => {
    const geometry = box();
    geometry.setDrawRange(6, 12);
    expect(countGeometry(new Mesh(geometry)).triangles).toBe(4);
  });

  it("counts a shared material once", () => {
    const material = new MeshStandardMaterial();
    const group = new Group();
    group.add(
      new Mesh(box(), material),
      new Mesh(box(), material),
      new Mesh(box(), [material, new MeshStandardMaterial()]),
    );
    expect(countGeometry(group)).toMatchObject({ meshes: 3, materials: 2, triangles: 36 });
  });

  it("counts the vertices of point clouds and lines, but no triangles", () => {
    const cloud = new BufferGeometry();
    cloud.setAttribute("position", new BufferAttribute(new Float32Array(300), 3));
    const group = new Group();
    group.add(new Points(cloud, new PointsMaterial()), new LineSegments(box()));
    expect(countGeometry(group)).toMatchObject({ triangles: 0, vertices: 124, meshes: 0 });
  });
});

describe("modelInfo", () => {
  it("measures the model where it stands, scale included", () => {
    const group = new Group();
    const mesh = new Mesh(box());
    mesh.position.set(2, 0.5, 0);
    mesh.scale.set(4, 1, 2);
    group.add(mesh);
    const info = modelInfo(group, []);
    expect(info.size).toEqual([4, 1, 2]);
    expect(info.center).toEqual([2, 0.5, 0]);
  });

  it("reports an empty model as nothing at the origin", () => {
    const info = modelInfo(new Group(), []);
    expect(info).toMatchObject({ triangles: 0, size: [0, 0, 0], center: [0, 0, 0], animations: [] });
  });

  it("lists the clips in file order, naming the unnamed ones", () => {
    const track = new VectorKeyframeTrack(".position", [0, 1], [0, 0, 0, 1, 1, 1]);
    const clips = [new AnimationClip("Flight", 1, [track]), new AnimationClip("", 1, [track])];
    expect(clipNames(clips)).toEqual(["Flight", "Animation 2"]);
  });
});
