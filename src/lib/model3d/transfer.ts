import { Box3, BufferAttribute, BufferGeometry, Sphere, Vector3, type TypedArray } from "three";

/**
 * Geometry as plain arrays, to cross from the parsing worker to the window
 * without a copy: the arrays' buffers are transferred, not cloned.
 */

export interface PackedAttribute {
  array: TypedArray;
  itemSize: number;
  normalized: boolean;
}

export interface PackedGeometry {
  attributes: Record<string, PackedAttribute>;
  index: TypedArray | null;
  groups: { start: number; count: number; materialIndex: number }[];
  /**
   * Bounds measured on the worker: three.js would otherwise walk every
   * vertex again on the main thread, for culling and for framing.
   */
  box: { min: [number, number, number]; max: [number, number, number] } | null;
  sphere: { center: [number, number, number]; radius: number } | null;
}

export interface PackedObject {
  name: string;
  kind: "mesh" | "lines" | "points";
  geometry: PackedGeometry;
  /** OBJ material names, one per group, as `usemtl` gave them. */
  materials: string[];
  /** An OBJ object without smoothing groups is drawn faceted. */
  flat: boolean;
  vertexColors: boolean;
}

export interface ParseRequest {
  id: number;
  format: "obj" | "stl" | "ply";
  buffer: ArrayBuffer;
}

export type ParseResponse =
  | {
      id: number;
      ok: true;
      objects: PackedObject[];
      /** The `mtllib` files an OBJ names, relative to it. */
      materialLibraries: string[];
    }
  | { id: number; ok: false; error: string };

export function packGeometry(geometry: BufferGeometry): { packed: PackedGeometry; transfer: ArrayBuffer[] } {
  if (!geometry.boundingBox) geometry.computeBoundingBox();
  if (!geometry.boundingSphere) geometry.computeBoundingSphere();
  const { boundingBox: box, boundingSphere: sphere } = geometry;
  const attributes: Record<string, PackedAttribute> = {};
  const transfer = new Set<ArrayBuffer>();
  for (const [name, attribute] of Object.entries(geometry.attributes)) {
    // Interleaved attributes never come out of these three loaders.
    if (!(attribute instanceof BufferAttribute)) continue;
    attributes[name] = {
      array: attribute.array,
      itemSize: attribute.itemSize,
      normalized: attribute.normalized,
    };
    if (attribute.array.buffer instanceof ArrayBuffer) transfer.add(attribute.array.buffer);
  }
  const index = geometry.index?.array ?? null;
  if (index && index.buffer instanceof ArrayBuffer) transfer.add(index.buffer);
  return {
    packed: {
      attributes,
      index,
      groups: geometry.groups.map(({ start, count, materialIndex }) => ({
        start,
        count,
        materialIndex: materialIndex ?? 0,
      })),
      box: box && !box.isEmpty() ? { min: box.min.toArray(), max: box.max.toArray() } : null,
      sphere: sphere ? { center: sphere.center.toArray(), radius: sphere.radius } : null,
    },
    transfer: [...transfer],
  };
}

export function unpackGeometry(packed: PackedGeometry): BufferGeometry {
  const geometry = new BufferGeometry();
  for (const [name, { array, itemSize, normalized }] of Object.entries(packed.attributes)) {
    geometry.setAttribute(name, new BufferAttribute(array, itemSize, normalized));
  }
  if (packed.index) geometry.setIndex(new BufferAttribute(packed.index, 1));
  for (const { start, count, materialIndex } of packed.groups) geometry.addGroup(start, count, materialIndex);
  const { box, sphere } = packed;
  if (box) geometry.boundingBox = new Box3(new Vector3(...box.min), new Vector3(...box.max));
  if (sphere) geometry.boundingSphere = new Sphere(new Vector3(...sphere.center), sphere.radius);
  return geometry;
}
