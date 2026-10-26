import {
  AnimationClip,
  Group,
  LineBasicMaterial,
  LineSegments,
  LoadingManager,
  Mesh,
  MeshStandardMaterial,
  Points,
  PointsMaterial,
  type Material,
  type Object3D,
  type WebGLRenderer,
} from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { DRACOLoader } from "three/examples/jsm/loaders/DRACOLoader.js";
import { KTX2Loader } from "three/examples/jsm/loaders/KTX2Loader.js";
import { FBXLoader } from "three/examples/jsm/loaders/FBXLoader.js";
import { USDLoader } from "three/examples/jsm/loaders/USDLoader.js";
import { MTLLoader } from "three/examples/jsm/loaders/MTLLoader.js";
import { MeshoptDecoder } from "three/examples/jsm/libs/meshopt_decoder.module.js";
// The decoders are app assets, emitted beside the bundle and served by the
// app itself: the viewer works offline, and never fetches code from a CDN.
import dracoWrapperUrl from "three/examples/jsm/libs/draco/gltf/draco_wasm_wrapper.js?url";
import dracoWasmUrl from "three/examples/jsm/libs/draco/gltf/draco_decoder.wasm?url";
import basisWrapperUrl from "three/examples/jsm/libs/basis/basis_transcoder.js?url";
import basisWasmUrl from "three/examples/jsm/libs/basis/basis_transcoder.wasm?url";
// Inlined, so the worker needs no file of its own next to the bundle (the
// site's demo is a single HTML file).
import ParseWorker from "./parse.worker?worker&inline";
import type { ModelFormat } from "../modelFiles";
import { baseUrlOf, FORMAT_PLANS, httpError } from "./formats";
import { standardizeMaterials } from "./materials";
import { unpackGeometry, type ParseRequest, type ParseResponse } from "./transfer";

export type LoadPhase = "download" | "parse" | "prepare";

export interface LoadProgress {
  phase: LoadPhase;
  /** Bytes read so far, during the download. */
  loaded: number;
  /** Bytes expected, when known. */
  total: number;
}

export interface LoadedModel {
  /** The file's own content, in its own coordinates. */
  content: Object3D;
  /** What goes in the scene: the content, turned upright when the format is Z-up. */
  root: Object3D;
  clips: AnimationClip[];
}

/** A load that a newer one, or a closed tab, made pointless. */
export class LoadAborted extends Error {
  constructor() {
    super("The load was cancelled.");
    this.name = "LoadAborted";
  }
}

/** The clay the plain geometry formats are drawn in: neutral, a little glossy. */
const clay = (vertexColors: boolean) =>
  new MeshStandardMaterial({
    name: "default",
    color: vertexColors ? 0xffffff : 0xb9bec6,
    roughness: 0.55,
    metalness: 0,
    vertexColors,
  });

/** A frame for the window to paint the progress it was just told about. */
export const nextFrame = () =>
  new Promise<void>((resolve) => {
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(() => resolve());
    else setTimeout(resolve, 16);
  });

/**
 * A loading manager that can say when every texture it started has arrived,
 * so the viewer reports a model as loaded only once it looks loaded — a
 * snapshot taken right after must not show it untextured.
 */
class TrackedManager extends LoadingManager {
  private pending = 0;
  private waiters: (() => void)[] = [];

  constructor() {
    super();
    const start = this.itemStart;
    const end = this.itemEnd;
    const error = this.itemError;
    const settle = () => {
      this.pending = Math.max(0, this.pending - 1);
      if (this.pending === 0) for (const resolve of this.waiters.splice(0)) resolve();
    };
    this.itemStart = (url) => {
      this.pending += 1;
      start(url);
    };
    this.itemEnd = (url) => {
      end(url);
      settle();
    };
    this.itemError = (url) => {
      error(url);
      settle();
    };
  }

  /** Resolves once nothing is in flight, or after `ms` regardless. */
  idle(ms: number): Promise<void> {
    if (this.pending === 0) return Promise.resolve();
    return new Promise((resolve) => {
      const timer = setTimeout(resolve, ms);
      this.waiters.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
  }
}

/** Reads the whole file, reporting progress as it streams in. */
export async function fetchBytes(
  url: string,
  signal: AbortSignal,
  onProgress: (loaded: number, total: number) => void,
  sizeHint = 0,
): Promise<ArrayBuffer> {
  let response: Response;
  try {
    response = await fetch(url, { signal });
  } catch (error) {
    if (signal.aborted) throw new LoadAborted();
    const reason = error instanceof Error ? error.message : String(error);
    throw new Error(`The file could not be read (${reason}).`);
  }
  if (!response.ok) {
    const body = await response.text().catch(() => "");
    throw new Error(httpError(response.status, response.statusText, body));
  }
  const total = Number(response.headers.get("content-length")) || sizeHint;
  if (!response.body) return response.arrayBuffer();

  const reader = response.body.getReader();
  // Written straight into one buffer when the size is known: no second copy
  // of a 200 MB file just to join its chunks.
  let buffer = new Uint8Array(total > 0 ? total : 1 << 20);
  let loaded = 0;
  onProgress(0, total);
  for (;;) {
    let chunk: ReadableStreamReadResult<Uint8Array>;
    try {
      chunk = await reader.read();
    } catch (error) {
      if (signal.aborted) throw new LoadAborted();
      throw error;
    }
    if (chunk.done) break;
    const bytes = chunk.value;
    if (loaded + bytes.length > buffer.length) {
      const grown = new Uint8Array(Math.max(buffer.length * 2, loaded + bytes.length));
      grown.set(buffer.subarray(0, loaded));
      buffer = grown;
    }
    buffer.set(bytes, loaded);
    loaded += bytes.length;
    onProgress(loaded, total);
  }
  return loaded === buffer.length ? buffer.buffer : buffer.slice(0, loaded).buffer;
}

/**
 * The loaders one viewer keeps for its whole life: the glTF one holds the
 * Draco and KTX2 decoders, whose workers and WebAssembly are expensive to
 * start, and the parsing worker is reused from one model to the next.
 */
export class ModelLoaders {
  private draco: DRACOLoader | null = null;
  private ktx2: KTX2Loader | null = null;
  private worker: Worker | null = null;
  private nextRequest = 1;
  private requests = new Map<number, (response: ParseResponse) => void>();

  constructor(private readonly renderer: WebGLRenderer) {}

  private gltfLoader(manager: LoadingManager): GLTFLoader {
    if (!this.draco) {
      this.draco = new DRACOLoader().setDecoderPath({ js: dracoWrapperUrl, wasm: dracoWasmUrl });
    }
    if (!this.ktx2) {
      // KTX2Loader only takes a folder for its transcoder; the bundled files
      // have hashed names, so the two requests are pointed at them by name.
      const decoders = new LoadingManager();
      decoders.setURLModifier((url) =>
        url.endsWith("basis_transcoder.js")
          ? basisWrapperUrl
          : url.endsWith("basis_transcoder.wasm")
            ? basisWasmUrl
            : url,
      );
      this.ktx2 = new KTX2Loader(decoders).setTranscoderPath("hangar-decoders/").detectSupport(this.renderer);
    }
    return new GLTFLoader(manager)
      .setDRACOLoader(this.draco)
      .setKTX2Loader(this.ktx2)
      .setMeshoptDecoder(MeshoptDecoder);
  }

  private parseOnWorker(format: "obj" | "stl" | "ply", buffer: ArrayBuffer, signal: AbortSignal) {
    if (!this.worker) {
      this.worker = new ParseWorker();
      this.worker.onmessage = (event: MessageEvent<ParseResponse>) => {
        const resolve = this.requests.get(event.data.id);
        this.requests.delete(event.data.id);
        resolve?.(event.data);
      };
      this.worker.onerror = (event) => {
        // A worker that failed to start fails every request waiting on it.
        const message = event.message || "the parsing worker stopped";
        for (const [id, resolve] of this.requests) resolve({ id, ok: false, error: message });
        this.requests.clear();
        this.worker?.terminate();
        this.worker = null;
      };
    }
    const id = this.nextRequest++;
    const worker = this.worker;
    return new Promise<ParseResponse>((resolve, reject) => {
      const abort = () => {
        this.requests.delete(id);
        reject(new LoadAborted());
      };
      signal.addEventListener("abort", abort, { once: true });
      this.requests.set(id, (response) => {
        signal.removeEventListener("abort", abort);
        resolve(response);
      });
      worker.postMessage({ id, format, buffer } satisfies ParseRequest, [buffer]);
    });
  }

  /**
   * Reads, parses and dresses one model. Everything heavy is either off the
   * main thread (the download, texture decoding, Draco, the plain formats)
   * or preceded by a frame, so the loading state is on screen before it.
   */
  async load(
    url: string,
    format: ModelFormat,
    signal: AbortSignal,
    onProgress: (progress: LoadProgress) => void,
    sizeHint?: number,
  ): Promise<LoadedModel> {
    const plan = FORMAT_PLANS[format];
    const base = baseUrlOf(url);
    const buffer = await fetchBytes(
      url,
      signal,
      (loaded, total) => onProgress({ phase: "download", loaded, total }),
      sizeHint,
    );
    if (signal.aborted) throw new LoadAborted();
    onProgress({ phase: "parse", loaded: buffer.byteLength, total: buffer.byteLength });
    await nextFrame();

    const manager = new TrackedManager();
    let content: Object3D;
    let clips: AnimationClip[] = [];
    try {
      switch (plan.parser) {
        case "gltf": {
          const gltf = await this.gltfLoader(manager).parseAsync(buffer, base);
          content = gltf.scene ?? gltf.scenes[0] ?? new Group();
          clips = gltf.animations ?? [];
          break;
        }
        case "fbx": {
          const group = new FBXLoader(manager).parse(buffer, base);
          content = group;
          clips = group.animations ?? [];
          break;
        }
        case "usd": {
          content = await new Promise<Object3D>((resolve, reject) => {
            new USDLoader(manager).parse(buffer, base, resolve, reject);
          });
          break;
        }
        case "worker":
          content = await this.parsePlain(format as "obj" | "stl" | "ply", buffer, base, manager, signal);
          break;
      }
    } catch (error) {
      if (error instanceof LoadAborted || signal.aborted) throw new LoadAborted();
      const reason = error instanceof Error ? error.message : String(error);
      throw new Error(`This ${format.toUpperCase()} file could not be read: ${reason}`);
    }
    // Textures the parse started (FBX, MTL) finish before the model counts as loaded.
    await manager.idle(15000);
    if (signal.aborted) {
      disposeObject(content);
      throw new LoadAborted();
    }

    standardizeMaterials(content);
    let root = content;
    if (plan.zUp) {
      root = new Group();
      root.rotation.x = -Math.PI / 2;
      root.add(content);
    }
    return { content, root, clips };
  }

  private async parsePlain(
    format: "obj" | "stl" | "ply",
    buffer: ArrayBuffer,
    base: string,
    manager: LoadingManager,
    signal: AbortSignal,
  ): Promise<Object3D> {
    const response = await this.parseOnWorker(format, buffer, signal);
    if (!response.ok) throw new Error(response.error);

    // An OBJ's materials come from the .mtl files it names, next to it.
    const named = new Map<string, Material>();
    for (const library of response.materialLibraries) {
      try {
        const creator = await new MTLLoader(manager).setResourcePath(base).loadAsync(base + library);
        creator.preload();
        for (const [name, material] of Object.entries(creator.materials)) named.set(name, material);
      } catch {
        // A missing .mtl leaves the model in clay rather than not opening it.
      }
    }

    const group = new Group();
    group.name = format;
    for (const object of response.objects) {
      const geometry = unpackGeometry(object.geometry);
      if (object.kind === "points") {
        const material = new PointsMaterial({
          size: 2,
          sizeAttenuation: false,
          vertexColors: object.vertexColors,
          color: object.vertexColors ? 0xffffff : 0xb9bec6,
        });
        group.add(Object.assign(new Points(geometry, material), { name: object.name }));
        continue;
      }
      if (object.kind === "lines") {
        const material = new LineBasicMaterial({ vertexColors: object.vertexColors });
        group.add(Object.assign(new LineSegments(geometry, material), { name: object.name }));
        continue;
      }
      if (!geometry.getAttribute("normal")) geometry.computeVertexNormals();
      const pick = (name: string) => {
        const material = named.get(name)?.clone() ?? clay(object.vertexColors);
        const shaded = material as MeshStandardMaterial;
        if (object.flat) shaded.flatShading = true;
        if (object.vertexColors) shaded.vertexColors = true;
        return material;
      };
      const materials = object.materials.length ? object.materials.map(pick) : [clay(object.vertexColors)];
      const mesh = new Mesh(geometry, materials.length > 1 ? materials : materials[0]);
      mesh.name = object.name;
      group.add(mesh);
    }
    for (const material of named.values()) material.dispose();
    return group;
  }

  dispose() {
    this.draco?.dispose();
    this.ktx2?.dispose();
    this.worker?.terminate();
    this.worker = null;
    for (const [id, resolve] of this.requests) resolve({ id, ok: false, error: "the viewer closed" });
    this.requests.clear();
  }
}

/**
 * Frees what a model holds on the GPU and in memory: geometries, materials,
 * every texture they reference, skeleton textures, and the decoded images
 * behind textures — an ImageBitmap is not released by three.js on its own.
 */
export function disposeObject(root: Object3D) {
  const materials = new Set<Material>();
  root.traverse((object) => {
    const node = object as Object3D & {
      geometry?: { dispose(): void };
      material?: Material | Material[];
      skeleton?: { dispose(): void };
    };
    node.geometry?.dispose();
    node.skeleton?.dispose();
    const list = Array.isArray(node.material) ? node.material : node.material ? [node.material] : [];
    for (const material of list) materials.add(material);
  });
  for (const material of materials) {
    for (const value of Object.values(material)) {
      if (!value || typeof value !== "object" || !(value as { isTexture?: boolean }).isTexture) continue;
      const texture = value as { dispose(): void; image?: unknown };
      const image = texture.image as { close?: () => void } | undefined;
      if (typeof ImageBitmap !== "undefined" && image instanceof ImageBitmap) image.close();
      texture.dispose();
    }
    material.dispose();
  }
}
