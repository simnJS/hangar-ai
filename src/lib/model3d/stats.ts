import { Box3, type AnimationClip, type BufferGeometry, type Material, type Object3D } from "three";
import type { ModelInfo } from "../modelStore";

/**
 * What a loaded model is made of, as the agent that showed it gets it back
 * and the info strip lists it.
 *
 * Counts are what the GPU draws: an instanced mesh counts once as a mesh and
 * as many times as it has instances in triangles and vertices, and a draw
 * range that leaves part of a buffer out leaves it out of the count too.
 */

interface Drawable extends Object3D {
  geometry?: BufferGeometry;
  material?: Material | Material[];
  isMesh?: boolean;
  isPoints?: boolean;
  isLine?: boolean;
  isInstancedMesh?: boolean;
  count?: number;
}

/** Above this many vertices the bounds come from each mesh's box, not its vertices. */
const PRECISE_BOUNDS_LIMIT = 2_000_000;

/** Elements the draw call covers: indices when indexed, vertices otherwise. */
function drawnElements(geometry: BufferGeometry): number {
  const position = geometry.getAttribute("position");
  if (!position) return 0;
  const total = geometry.index ? geometry.index.count : position.count;
  const { start, count } = geometry.drawRange;
  const from = Math.min(Math.max(0, start), total);
  return Math.max(0, Math.min(total - from, Number.isFinite(count) ? count : total));
}

export interface ModelCounts {
  triangles: number;
  vertices: number;
  meshes: number;
  materials: number;
}

export function countGeometry(root: Object3D): ModelCounts {
  let triangles = 0;
  let vertices = 0;
  let meshes = 0;
  const materials = new Set<Material>();

  root.traverse((object) => {
    const node = object as Drawable;
    if (!node.geometry || !(node.isMesh || node.isPoints || node.isLine)) return;
    const instances = node.isInstancedMesh ? Math.max(0, node.count ?? 1) : 1;
    vertices += (node.geometry.getAttribute("position")?.count ?? 0) * instances;
    if (node.isMesh) {
      meshes += 1;
      triangles += Math.floor(drawnElements(node.geometry) / 3) * instances;
    }
    const list = Array.isArray(node.material) ? node.material : node.material ? [node.material] : [];
    for (const material of list) materials.add(material);
  });

  return { triangles, vertices, meshes, materials: materials.size };
}

/** Clip names in file order; an unnamed clip is named by its place. */
export const clipNames = (clips: AnimationClip[]) =>
  clips.map((clip, i) => clip.name.trim() || `Animation ${i + 1}`);

export function modelInfo(root: Object3D, clips: AnimationClip[]): ModelInfo {
  const counts = countGeometry(root);
  root.updateWorldMatrix(true, true);
  // Vertex by vertex, the box is tight around a rotated or skinned mesh,
  // which is what framing needs; past a few million vertices, each mesh's
  // own box transformed is close enough and a hundred times cheaper.
  const box = new Box3().setFromObject(root, counts.vertices <= PRECISE_BOUNDS_LIMIT);
  const empty = box.isEmpty();
  const round = (value: number) => (empty ? 0 : Number(value.toPrecision(6)));
  return {
    ...counts,
    animations: clipNames(clips),
    size: [round(box.max.x - box.min.x), round(box.max.y - box.min.y), round(box.max.z - box.min.z)],
    center: [
      round((box.min.x + box.max.x) / 2),
      round((box.min.y + box.max.y) / 2),
      round((box.min.z + box.max.z) / 2),
    ],
  };
}
