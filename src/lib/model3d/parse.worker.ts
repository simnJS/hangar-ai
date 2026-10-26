import type { BufferGeometry, Material, Object3D } from "three";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { PLYLoader } from "three/examples/jsm/loaders/PLYLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";
import { packGeometry, type PackedObject, type ParseRequest, type ParseResponse } from "./transfer";

/**
 * Parses the formats that are plain geometry — OBJ, STL, PLY — away from the
 * window. Their loaders walk the file vertex by vertex, which for a large
 * scan or a CAD export is seconds the window would otherwise spend frozen,
 * spinner included. What comes back is arrays, transferred without a copy;
 * materials are made in the window, where textures can load.
 */

/**
 * The worker's own global, typed by hand: the project compiles against the
 * DOM library, and pulling the WebWorker one in beside it redeclares half of
 * both.
 */
const scope = self as unknown as {
  onmessage: ((event: MessageEvent<ParseRequest>) => void) | null;
  postMessage: (message: ParseResponse, transfer?: Transferable[]) => void;
};

interface Drawable extends Object3D {
  geometry: BufferGeometry;
  material: Material | Material[];
  isMesh?: boolean;
  isLineSegments?: boolean;
  isPoints?: boolean;
}

function packObject(object: Drawable, transfer: ArrayBuffer[]): PackedObject {
  // An OBJ without vertex normals gets them here rather than in the window:
  // for a scan, that is a few million vertices walked.
  if (object.isMesh && !object.geometry.getAttribute("normal")) object.geometry.computeVertexNormals();
  const { packed, transfer: buffers } = packGeometry(object.geometry);
  transfer.push(...buffers);
  const materials = Array.isArray(object.material) ? object.material : [object.material];
  const first = materials[0] as Material & { flatShading?: boolean; vertexColors?: boolean };
  return {
    name: object.name,
    kind: object.isPoints ? "points" : object.isLineSegments ? "lines" : "mesh",
    geometry: packed,
    materials: materials.map((material) => material.name),
    flat: Boolean(first?.flatShading),
    vertexColors: Boolean(first?.vertexColors),
  };
}

function parse({ id, format, buffer }: ParseRequest): { response: ParseResponse; transfer: ArrayBuffer[] } {
  const transfer: ArrayBuffer[] = [];
  if (format === "obj") {
    const text = new TextDecoder().decode(buffer);
    const group = new OBJLoader().parse(text) as Object3D & { materialLibraries?: string[] };
    const objects: PackedObject[] = [];
    group.traverse((child) => {
      const node = child as Drawable;
      if (node.geometry && (node.isMesh || node.isLineSegments || node.isPoints)) {
        objects.push(packObject(node, transfer));
      }
    });
    return {
      response: { id, ok: true, objects, materialLibraries: group.materialLibraries ?? [] },
      transfer,
    };
  }

  const geometry =
    format === "stl" ? new STLLoader().parse(buffer) : new PLYLoader().parse(buffer);
  if (geometry.index && !geometry.getAttribute("normal")) geometry.computeVertexNormals();
  const { packed, transfer: buffers } = packGeometry(geometry);
  const hasColors = Boolean(geometry.getAttribute("color"));
  // A PLY of vertices and no faces is a point cloud, the way scanners write
  // them; PLYLoader only indexes the faces it read.
  const pointsOnly = format === "ply" && !geometry.index;
  return {
    response: {
      id,
      ok: true,
      objects: [
        {
          name: "",
          kind: pointsOnly ? "points" : "mesh",
          geometry: packed,
          materials: [],
          flat: false,
          vertexColors: hasColors,
        },
      ],
      materialLibraries: [],
    },
    transfer: buffers,
  };
}

scope.onmessage = (event) => {
  try {
    const { response, transfer } = parse(event.data);
    scope.postMessage(response, transfer);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    scope.postMessage({ id: event.data.id, ok: false, error: message });
  }
};
