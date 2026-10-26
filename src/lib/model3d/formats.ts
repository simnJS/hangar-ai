import type { ModelFormat } from "../modelFiles";

/**
 * How each format is read, decided in one table so the engine never guesses
 * from a file name twice.
 */

export type Parser =
  /** GLTFLoader, with Draco, meshopt and KTX2. */
  | "gltf"
  | "fbx"
  | "usd"
  /**
   * Plain geometry parsed on a worker: OBJ, STL and PLY are text or flat
   * arrays that the loaders walk vertex by vertex on whatever thread runs
   * them, which for a 50 MB file is seconds of a frozen window.
   */
  | "worker";

export interface FormatPlan {
  parser: Parser;
  /**
   * The unit the format promises, when it promises one: glTF is in metres by
   * specification. The others are in whatever the exporter wrote.
   */
  unit: "m" | null;
  /**
   * The file is laid out Z-up and is turned upright for display. STL comes
   * out of CAD and slicers that way; the sizes the viewer reports stay the
   * file's own.
   */
  zUp: boolean;
}

export const FORMAT_PLANS: Record<ModelFormat, FormatPlan> = {
  glb: { parser: "gltf", unit: "m", zUp: false },
  gltf: { parser: "gltf", unit: "m", zUp: false },
  fbx: { parser: "fbx", unit: null, zUp: false },
  usdz: { parser: "usd", unit: null, zUp: false },
  obj: { parser: "worker", unit: null, zUp: false },
  stl: { parser: "worker", unit: null, zUp: true },
  ply: { parser: "worker", unit: null, zUp: false },
};

/**
 * The folder a model's URL sits in, with its trailing slash: what a glTF's
 * `textures/wood.png` or an OBJ's `.mtl` resolves against.
 */
export function baseUrlOf(url: string): string {
  const cut = url.split(/[?#]/)[0];
  const slash = cut.lastIndexOf("/");
  return slash < 0 ? "" : cut.slice(0, slash + 1);
}

/**
 * A model file is read whole before it is parsed. A 404 from the model
 * scheme says why in its body; the agent reading the error gets that reason.
 */
export function httpError(status: number, statusText: string, body: string): string {
  const reason = body.trim().split("\n")[0].slice(0, 200) || statusText;
  if (status === 404) return `The file was not found (${reason}).`;
  if (status === 403) return `Hangar may not read this file (${reason}).`;
  return `The file could not be read: HTTP ${status}${reason ? ` (${reason})` : ""}.`;
}
