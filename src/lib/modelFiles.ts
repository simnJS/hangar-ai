import { invoke } from "@tauri-apps/api/core";

/**
 * 3D model files, as the app reads them (see src-tauri/src/model.rs).
 *
 * The webview never gets a file path: it loads a model from the
 * `hangar-model` URI scheme, which Rust serves only from the folders the app
 * allowed (a workspace and its worktrees) and only for model and texture
 * files. Each path segment is a URL segment, so a glTF's `textures/wood.png`
 * or an OBJ's `.mtl` resolve next to the model by plain relative URLs.
 */

export const MODEL_FORMATS = ["glb", "gltf", "obj", "stl", "ply", "fbx", "usdz"] as const;
export type ModelFormat = (typeof MODEL_FORMATS)[number];

/** Largest model the viewer accepts, textures not included. */
export const MAX_MODEL_BYTES = 256 * 1024 * 1024;

export interface ModelFileInfo {
  /** Canonical absolute path, as Rust resolved it. */
  path: string;
  size: number;
  /** Epoch milliseconds; what the viewer watches to reload a rewritten model. */
  modifiedMs: number;
}

/** The format a file name says, or null for anything the viewer cannot open. */
export function modelFormatOf(path: string): ModelFormat | null {
  const dot = path.lastIndexOf(".");
  if (dot < 0) return null;
  const ext = path.slice(dot + 1).toLowerCase();
  return (MODEL_FORMATS as readonly string[]).includes(ext) ? (ext as ModelFormat) : null;
}

/**
 * The folders the scheme may serve from, replacing the previous list. Paths
 * outside every one of them are refused by Rust, whatever the URL says.
 */
export const modelAllowRoots = (roots: string[]) => invoke<void>("model_allow_roots", { roots });

/** Size and modification time, and a refusal for anything not allowed or too big. */
export const modelStat = (path: string) => invoke<ModelFileInfo>("model_stat", { path });

const IS_WINDOWS = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);

/**
 * The URL the viewer loads `path` from. Windows' webview only routes custom
 * schemes as `http://<scheme>.localhost`; the others take the scheme itself.
 * Segments are encoded one by one, never the separators, so relative URLs
 * inside the model keep working.
 */
export function modelUrl(path: string, platformIsWindows = IS_WINDOWS): string {
  const parts = path.replace(/\\/g, "/").split("/").filter((part, i) => part || i === 0);
  const encoded = parts.map((part) => encodeURIComponent(part)).join("/");
  const base = platformIsWindows ? "http://hangar-model.localhost" : "hangar-model://localhost";
  return `${base}/${encoded.replace(/^\//, "")}`;
}
