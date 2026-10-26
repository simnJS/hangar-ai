import {
  AnimationMixer,
  Box3,
  BufferGeometry,
  Color,
  DirectionalLight,
  Float32BufferAttribute,
  Group,
  LineBasicMaterial,
  LineSegments,
  LoopRepeat,
  MeshBasicMaterial,
  NeutralToneMapping,
  PerspectiveCamera,
  PMREMGenerator,
  Scene,
  SRGBColorSpace,
  Vector3,
  WebGLRenderer,
  type AnimationAction,
  type AnimationClip,
  type Material,
  type Line,
  type Mesh,
  type Object3D,
  type Points,
  type Texture,
  type WebGLRenderTarget,
} from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { VertexNormalsHelper } from "three/examples/jsm/helpers/VertexNormalsHelper.js";
import type { ModelFormat } from "../modelFiles";
import type { ModelInfo, SnapshotOptions } from "../modelStore";
import {
  clipPlanes,
  DEFAULT_AZIMUTH,
  DEFAULT_ELEVATION,
  directionFrom,
  frameView,
  viewDirection,
  type Box,
  type Vec3,
  type ViewDirection,
} from "./framing";
import { FloorGrid } from "./grid";
import { disposeObject, LoadAborted, ModelLoaders, nextFrame, type LoadProgress } from "./loaders";
import { texturesOf } from "./materials";
import { modelInfo } from "./stats";

/**
 * The 3D preview's renderer: one WebGL canvas, the models loaded into it,
 * and everything the panel asks of them — framing, display toggles,
 * animation, snapshots. No React and no store in here; the panel drives it.
 *
 * Only ever reached through a dynamic import, so three.js stays in a chunk of
 * its own that a window without a model never downloads or parses.
 */

export { LoadAborted };
export type { LoadProgress };

export interface DisplayOptions {
  wireframe: boolean;
  grid: boolean;
  axes: boolean;
  normals: boolean;
}

/** The theme as the viewport draws it; plain CSS colours, read by the panel. */
export interface ViewerTheme {
  dark: boolean;
  background: string;
  gridMinor: string;
  gridMajor: string;
  axisX: string;
  axisY: string;
  axisZ: string;
  wire: string;
  normals: string;
}

export interface AnimationState {
  clips: string[];
  clip: number;
  playing: boolean;
  time: number;
  duration: number;
}

export type ViewerEvent =
  | { type: "animation"; modelId: string; state: AnimationState }
  | { type: "contextlost" }
  | { type: "contextrestored" };

export interface LoadRequest {
  id: string;
  revision: number;
  url: string;
  format: ModelFormat;
  /** The file's size, for the progress bar when the response does not say. */
  sizeHint?: number;
}

/** Field of view: a long-ish lens, which flatters an object more than a wide one. */
const FOV = 35;
/** Models kept loaded besides the one on screen, so tabs switch instantly. */
const KEEP_LOADED = 3;
/** Past this many vertices, drawing every normal costs more than it shows. */
export const NORMALS_LIMIT = 600_000;
/** Normals follow a playing animation frame by frame up to this many vertices. */
const LIVE_NORMALS_LIMIT = 150_000;
/** How long the camera takes to glide back when recentred. */
const RECENTER_MS = 420;

interface Entry {
  id: string;
  revision: number;
  root: Object3D;
  content: Object3D;
  info: ModelInfo;
  box: Box;
  /** Points on the model, which framing fits rather than the box. */
  hull: Vec3[];
  radius: number;
  clips: AnimationClip[];
  mixer: AnimationMixer | null;
  action: AnimationAction | null;
  clip: number;
  playing: boolean;
  /** The camera as the user left it on this model. */
  view: SavedView | null;
  wire: Object3D[] | null;
  normals: Object3D[] | null;
  lastShown: number;
}

interface SavedView {
  position: Vector3;
  target: Vector3;
  /** Still the automatic framing: the user has not moved the camera since. */
  auto: boolean;
}

const boxOf = (object: Object3D): Box => {
  object.updateWorldMatrix(true, true);
  const box = new Box3().setFromObject(object);
  if (box.isEmpty()) return { min: [0, 0, 0], max: [0, 0, 0] };
  return { min: box.min.toArray(), max: box.max.toArray() };
};

/** Vertices a framing looks at, at most this many of them. */
const HULL_SAMPLES = 6000;

/**
 * A sample of the model's vertices in world space, spread over all its
 * meshes: every vertex of a low-poly asset, a regular stride through a scan.
 * Skinned meshes are sampled as posed, so a character framed mid-animation
 * is framed as it stands.
 */
function sampleHull(root: Object3D, box: Box): Vec3[] {
  root.updateWorldMatrix(true, true);
  const drawables: Mesh[] = [];
  let total = 0;
  root.traverse((object) => {
    const node = object as Mesh;
    const position = node.geometry?.getAttribute("position");
    if (!position || !(node.isMesh || (object as Points).isPoints || (object as Line).isLine)) return;
    drawables.push(node);
    total += position.count;
  });
  if (!total) return [box.min, box.max];
  const stride = Math.max(1, Math.ceil(total / HULL_SAMPLES));
  const points: Vec3[] = [];
  const vertex = new Vector3();
  let skip = 0;
  for (const node of drawables) {
    const count = node.geometry.getAttribute("position").count;
    for (let i = skip; i < count; i += stride) {
      if (node.isMesh) node.getVertexPosition(i, vertex);
      else vertex.fromBufferAttribute(node.geometry.getAttribute("position"), i);
      vertex.applyMatrix4(node.matrixWorld);
      points.push([vertex.x, vertex.y, vertex.z]);
    }
    // Carries the stride across meshes, so many small ones are sampled as
    // evenly as one big one.
    skip = (skip - count) % stride;
    if (skip < 0) skip += stride;
  }
  return points;
}

const radiusOfBox = (box: Box) =>
  Math.max(
    Math.hypot(box.max[0] - box.min[0], box.max[1] - box.min[1], box.max[2] - box.min[2]) / 2,
    1e-3,
  );

const prefersReducedMotion = () =>
  typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

const easeOut = (t: number) => 1 - (1 - t) ** 3;

/** A PNG as base64, without the data: prefix. */
async function blobToBase64(blob: Blob): Promise<string> {
  const url = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
  return url.slice(url.indexOf(",") + 1);
}

export class ModelViewer {
  readonly canvas: HTMLCanvasElement;
  private readonly renderer: WebGLRenderer;
  private readonly scene = new Scene();
  private readonly camera = new PerspectiveCamera(FOV, 1, 0.01, 1000);
  private readonly controls: OrbitControls;
  private pmrem: PMREMGenerator;
  private environment: WebGLRenderTarget;
  private readonly keyLight = new DirectionalLight(0xffffff, 1.6);
  private readonly grid = new FloorGrid();
  private readonly axes: LineSegments;
  private readonly layer = new Group();
  private readonly helpers = new Group();
  private readonly wireMaterial = new MeshBasicMaterial({
    wireframe: true,
    transparent: true,
    opacity: 0.32,
    depthWrite: false,
    toneMapped: false,
  });
  private readonly loaders: ModelLoaders;
  private readonly entries = new Map<string, Entry>();
  private readonly listeners = new Set<(event: ViewerEvent) => void>();
  private readonly resizeObserver: ResizeObserver;
  private display: DisplayOptions = { wireframe: false, grid: true, axes: false, normals: false };
  private theme: ViewerTheme | null = null;
  private activeId: string | null = null;
  private raf = 0;
  private dirty = true;
  private lastFrame = 0;
  private width = 0;
  private height = 0;
  private tween: {
    from: { position: Vector3; target: Vector3 };
    to: { position: Vector3; target: Vector3 };
    start: number;
  } | null = null;
  private disposed = false;
  /**
   * The camera is where framing put it, untouched by the user: a panel that
   * changes shape — shown for the first time, expanded, dragged wider —
   * frames the model again. The first drag or scroll ends that.
   */
  private autoFramed = false;

  constructor(private readonly host: HTMLElement) {
    this.canvas = document.createElement("canvas");
    this.canvas.className = "model-view__canvas";
    host.appendChild(this.canvas);

    this.renderer = new WebGLRenderer({
      canvas: this.canvas,
      antialias: true,
      // Transparent: the panel paints the backdrop in CSS, from the theme's
      // tokens, so a theme switch needs no re-render to look right.
      alpha: true,
      powerPreference: "high-performance",
    });
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.outputColorSpace = SRGBColorSpace;
    // Khronos PBR Neutral: base colours come out as authored, which is what
    // someone checking an asset needs; ACES and AgX shift hues and saturation
    // in ways a game engine's own grading may not.
    this.renderer.toneMapping = NeutralToneMapping;
    this.renderer.toneMappingExposure = 1;
    this.renderer.shadowMap.enabled = false;

    // A neutral studio: the room environment lights every PBR material evenly,
    // and one soft key light from the front right gives the forms a side.
    this.pmrem = new PMREMGenerator(this.renderer);
    this.environment = this.buildEnvironment();
    this.scene.environmentIntensity = 0.8;
    this.keyLight.position.set(0.6, 1, 0.8);
    this.scene.add(this.keyLight, this.grid, this.layer, this.helpers);

    this.axes = this.createAxes();
    this.helpers.add(this.axes);
    this.axes.visible = false;
    // Nothing to stand on until there is a model.
    this.grid.visible = false;

    this.controls = new OrbitControls(this.camera, this.canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.screenSpacePanning = true;
    this.controls.zoomToCursor = true;
    this.controls.addEventListener("change", () => this.requestRender());
    // A hand on the camera ends a recentring glide rather than fighting it.
    this.controls.addEventListener("start", () => {
      this.tween = null;
      this.autoFramed = false;
    });
    // The keyboard too, once the view has the focus: arrows pan, with Shift
    // (or Ctrl, Cmd) they orbit; a step is a few degrees, not a pixel.
    this.controls.keyRotateSpeed = 24;
    this.controls.keyPanSpeed = 16;
    this.controls.listenToKeyEvents(host);
    host.addEventListener("keydown", this.onKeyDown);

    this.loaders = new ModelLoaders(this.renderer);

    this.canvas.addEventListener("webglcontextlost", this.onContextLost);
    this.canvas.addEventListener("webglcontextrestored", this.onContextRestored);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(host);
    this.resize();
  }

  // -------------------------------------------------------------------------
  // Events
  // -------------------------------------------------------------------------

  on(listener: (event: ViewerEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: ViewerEvent) {
    for (const listener of this.listeners) listener(event);
  }

  /** Zoom and recentre from the keyboard; the arrows are OrbitControls'. */
  private onKeyDown = (event: KeyboardEvent) => {
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.key.startsWith("Arrow")) {
      this.tween = null;
      this.autoFramed = false;
      return;
    }
    if (event.key === "+" || event.key === "=") this.zoom(1);
    else if (event.key === "-" || event.key === "_") this.zoom(-1);
    else if (event.key === "Home") this.recenter();
    else return;
    event.preventDefault();
  };

  /** One step closer (1) or further (-1). */
  zoom(direction: 1 | -1) {
    this.tween = null;
    this.autoFramed = false;
    // OrbitControls scales the distance by the factor going in, and by its
    // inverse going out.
    if (direction > 0) this.controls.dollyIn(0.8);
    else this.controls.dollyOut(0.8);
    this.controls.update();
    this.requestRender();
  }

  private onContextLost = (event: Event) => {
    // Asks the browser to give the context back; three.js re-uploads
    // everything on the next render once it does.
    event.preventDefault();
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.emit({ type: "contextlost" });
  };

  /** The studio's light, rendered once into a prefiltered environment map. */
  private buildEnvironment(): WebGLRenderTarget {
    const room = new RoomEnvironment();
    const target = this.pmrem.fromScene(room, 0.04);
    room.dispose();
    this.scene.environment = target.texture;
    return target;
  }

  private onContextRestored = () => {
    // Textures and buffers come back from their data; the environment map
    // was rendered on the GPU, and comes back empty unless rendered again.
    this.environment.dispose();
    this.pmrem.dispose();
    this.pmrem = new PMREMGenerator(this.renderer);
    this.environment = this.buildEnvironment();
    this.emit({ type: "contextrestored" });
    this.requestRender();
  };

  // -------------------------------------------------------------------------
  // Loading
  // -------------------------------------------------------------------------

  /** This revision of this model is loaded and ready to show. */
  has(id: string, revision: number): boolean {
    return this.entries.get(id)?.revision === revision;
  }

  /** Every model the viewer holds, by id. */
  ids(): string[] {
    return [...this.entries.keys()];
  }

  /** Some revision of the model is loaded: the one to keep on screen while a newer one loads. */
  hasAny(id: string): boolean {
    return this.entries.has(id);
  }

  async load(
    request: LoadRequest,
    options: { signal: AbortSignal; onProgress: (progress: LoadProgress) => void },
  ): Promise<ModelInfo> {
    const loaded = await this.loaders.load(
      request.url,
      request.format,
      options.signal,
      options.onProgress,
      request.sizeHint,
    );
    if (this.disposed || options.signal.aborted) {
      disposeObject(loaded.root);
      throw new LoadAborted();
    }

    // Measured in the file's own coordinates, before any turn upright: the
    // sizes an agent gets back are the ones its file says.
    const holder = loaded.content.parent;
    holder?.remove(loaded.content);
    const info = modelInfo(loaded.content, loaded.clips);
    holder?.add(loaded.content);
    options.onProgress({ phase: "prepare", loaded: 0, total: 0 });
    try {
      await this.prepare(loaded.root);
    } catch {
      // Precompiling is an optimisation; the first frame compiles what it missed.
    }
    if (this.disposed || options.signal.aborted) {
      disposeObject(loaded.root);
      throw new LoadAborted();
    }

    const previous = this.entries.get(request.id);
    const box = boxOf(loaded.root);
    const hull = sampleHull(loaded.root, box);
    const entry: Entry = {
      id: request.id,
      revision: request.revision,
      root: loaded.root,
      content: loaded.content,
      info,
      box,
      hull,
      radius: radiusOfBox(box),
      clips: loaded.clips,
      mixer: null,
      action: null,
      clip: 0,
      playing: false,
      view: null,
      wire: null,
      normals: null,
      lastShown: previous?.lastShown ?? 0,
    };
    if (loaded.clips.length) {
      entry.mixer = new AnimationMixer(loaded.root);
      // A model that moves is shown moving: the first clip plays on open,
      // and a reload carries on with the clip and the time it was at.
      const clip = previous && previous.clip < loaded.clips.length ? previous.clip : 0;
      this.startClip(entry, clip, previous ? previous.playing : true);
      if (previous?.action && entry.action) {
        entry.action.time = Math.min(previous.action.time, loaded.clips[clip].duration);
      }
      entry.mixer.update(0);
    }

    // A reload keeps the camera where the user left it, unless the model
    // changed scale so much that the old view would be inside it or lost.
    if (previous) {
      const kept = this.activeId === request.id ? this.currentView() : previous.view;
      const ratio = entry.radius / previous.radius;
      entry.view = kept && ratio > 0.25 && ratio < 4 ? kept : null;
      this.disposeEntry(previous);
    }
    this.entries.set(request.id, entry);
    if (this.activeId === request.id) this.attach(entry, Boolean(entry.view));
    this.evict();
    return info;
  }

  /**
   * Gets the GPU work out of the way before the model is on screen: shaders
   * compiled in parallel where the driver can, textures uploaded a few at a
   * time with a frame between them, instead of one long stall on the first
   * frame of a large model.
   */
  private async prepare(root: Object3D) {
    await this.renderer.compileAsync(root, this.camera, this.scene);
    const textures = new Set<Texture>();
    root.traverse((object) => {
      const material = (object as Mesh).material as Material | Material[] | undefined;
      const list = Array.isArray(material) ? material : material ? [material] : [];
      for (const each of list) for (const texture of texturesOf(each)) textures.add(texture);
    });
    let budget = performance.now();
    for (const texture of textures) {
      this.renderer.initTexture(texture);
      if (performance.now() - budget > 8) {
        await nextFrame();
        budget = performance.now();
      }
    }
  }

  /** Drops the least recently shown models past the few kept for quick tab switches. */
  private evict() {
    const others = [...this.entries.values()]
      .filter((entry) => entry.id !== this.activeId)
      .sort((a, b) => b.lastShown - a.lastShown);
    for (const entry of others.slice(KEEP_LOADED)) {
      this.entries.delete(entry.id);
      this.disposeEntry(entry);
    }
  }

  /** The model was closed: everything it held goes. */
  remove(id: string) {
    const entry = this.entries.get(id);
    if (!entry) return;
    this.entries.delete(id);
    if (this.activeId === id) {
      this.activeId = null;
      this.layer.clear();
      this.grid.visible = false;
      this.axes.visible = false;
      this.requestRender();
    }
    this.disposeEntry(entry);
  }

  private disposeEntry(entry: Entry) {
    entry.mixer?.stopAllAction();
    entry.mixer?.uncacheRoot(entry.root);
    this.removeHelpers(entry);
    if (entry.root.parent) entry.root.parent.remove(entry.root);
    disposeObject(entry.root);
  }

  // -------------------------------------------------------------------------
  // Showing
  // -------------------------------------------------------------------------

  /**
   * Puts a model in front, where its camera was, or framed. One that is not loaded yet leaves the stage empty
   * — never the previous tab's model under the new one's loading or error
   * state — and appears as soon as its load lands.
   */
  show(id: string) {
    const entry = this.entries.get(id);
    if (entry && this.activeId === id && entry.root.parent === this.layer) return;
    const current = this.activeEntry();
    if (current && current !== entry) current.view = this.currentView();
    this.activeId = id;
    if (!entry) {
      this.layer.clear();
      this.grid.visible = false;
      this.axes.visible = false;
      this.requestRender();
      return;
    }
    this.attach(entry, Boolean(entry.view));
    this.evict();
  }

  private activeEntry(): Entry | null {
    return this.activeId ? (this.entries.get(this.activeId) ?? null) : null;
  }

  private attach(entry: Entry, keepView: boolean) {
    this.layer.clear();
    this.layer.add(entry.root);
    entry.lastShown = performance.now();
    this.tween = null;
    this.fitHelpers(entry);
    this.applyDisplay(entry);
    if (keepView && entry.view) {
      this.camera.position.copy(entry.view.position);
      this.controls.target.copy(entry.view.target);
      this.autoFramed = entry.view.auto;
    } else {
      const view = this.defaultView(entry);
      this.camera.position.copy(view.position);
      this.controls.target.copy(view.target);
      this.autoFramed = true;
    }
    this.controls.minDistance = entry.radius * 0.02;
    this.controls.maxDistance = entry.radius * 40;
    this.camera.up.set(0, 1, 0);
    this.updateClipPlanes(entry);
    this.controls.update();
    this.emitAnimation(entry);
    this.requestRender();
  }

  private currentView(): SavedView {
    return {
      position: this.camera.position.clone(),
      target: this.controls.target.clone(),
      auto: this.autoFramed,
    };
  }

  /** The three-quarter view, fitted to the panel's current shape. */
  private defaultView(entry: Entry, aspect = this.aspect()) {
    const view: ViewDirection = {
      dir: directionFrom(DEFAULT_AZIMUTH, DEFAULT_ELEVATION),
      up: [0, 1, 0],
    };
    const { position, target } = frameView(entry.hull, view, FOV, aspect);
    return { position: new Vector3(...position), target: new Vector3(...target) };
  }

  /** Glides back to the framed view, or jumps when motion is reduced. */
  recenter() {
    const entry = this.activeEntry();
    if (!entry) return;
    const to = this.defaultView(entry);
    this.autoFramed = true;
    if (prefersReducedMotion()) {
      this.camera.position.copy(to.position);
      this.controls.target.copy(to.target);
      this.controls.update();
      this.requestRender();
      return;
    }
    this.tween = { from: this.currentView(), to, start: performance.now() };
    this.requestRender();
  }

  private aspect() {
    return this.width > 0 && this.height > 0 ? this.width / this.height : 4 / 3;
  }

  private updateClipPlanes(
    entry: Entry | null,
    camera = this.camera,
    target = this.controls.target,
  ) {
    const radius = entry?.radius ?? 1;
    const { near, far } = clipPlanes(camera.position.distanceTo(target), radius);
    if (camera.near !== near || camera.far !== far) {
      camera.near = near;
      camera.far = far;
      camera.updateProjectionMatrix();
    }
  }

  // -------------------------------------------------------------------------
  // Display toggles and theme
  // -------------------------------------------------------------------------

  setDisplay(patch: Partial<DisplayOptions>) {
    this.display = { ...this.display, ...patch };
    const entry = this.activeEntry();
    if (entry) this.applyDisplay(entry);
    this.grid.visible = this.display.grid && Boolean(entry);
    this.axes.visible = this.display.axes && Boolean(entry);
    this.requestRender();
  }

  /** Whether the normals of the model on screen can be drawn at all. */
  canDrawNormals(id: string): boolean {
    const entry = this.entries.get(id);
    return Boolean(entry) && entry!.info.vertices <= NORMALS_LIMIT;
  }

  private applyDisplay(entry: Entry) {
    this.grid.visible = this.display.grid;
    this.axes.visible = this.display.axes;
    if (this.display.wireframe) this.ensureWire(entry);
    for (const wire of entry.wire ?? []) wire.visible = this.display.wireframe;
    // Solid faces step back a little under the wireframe, which would
    // otherwise flicker in and out of them; a material that had an offset of
    // its own (a decal) gets it back when the wireframe goes.
    entry.root.traverse((object) => {
      if (object.userData.hangarWire) return;
      const material = (object as Mesh).material as Material | Material[] | undefined;
      const list = Array.isArray(material) ? material : material ? [material] : [];
      for (const each of list) {
        if (each === this.wireMaterial) continue;
        const own = (each.userData.hangarOffset ??= [
          each.polygonOffset,
          each.polygonOffsetFactor,
          each.polygonOffsetUnits,
        ]) as [boolean, number, number];
        each.polygonOffset = this.display.wireframe || own[0];
        each.polygonOffsetFactor = this.display.wireframe ? Math.max(1, own[1]) : own[1];
        each.polygonOffsetUnits = this.display.wireframe ? Math.max(1, own[2]) : own[2];
      }
    });
    const normals = this.display.normals && entry.info.vertices <= NORMALS_LIMIT;
    if (normals) this.ensureNormals(entry);
    for (const helper of entry.normals ?? []) helper.visible = normals;
  }

  /**
   * The wireframe is drawn over the shaded model, not instead of it: a copy
   * of each mesh sharing its geometry — and its skeleton, so a skinned mesh's
   * wires follow the animation — with one translucent line material.
   */
  private ensureWire(entry: Entry) {
    if (entry.wire) return;
    const meshes: Mesh[] = [];
    entry.root.traverse((object) => {
      if ((object as Mesh).isMesh && !object.userData.hangarWire) meshes.push(object as Mesh);
    });
    entry.wire = meshes.map((mesh) => {
      const wire = mesh.clone(false) as Mesh;
      wire.material = this.wireMaterial;
      wire.position.set(0, 0, 0);
      wire.quaternion.identity();
      wire.scale.set(1, 1, 1);
      wire.userData = { hangarWire: true };
      wire.renderOrder = 1;
      // Shared, so morph targets driven by an animation move the wires too.
      if (mesh.morphTargetInfluences) wire.morphTargetInfluences = mesh.morphTargetInfluences;
      mesh.add(wire);
      return wire;
    });
  }

  private ensureNormals(entry: Entry) {
    if (entry.normals) return;
    const color = new Color(this.theme?.normals ?? "#4f9dff");
    const size = entry.radius * 0.02;
    const helpers: Object3D[] = [];
    entry.root.updateWorldMatrix(true, true);
    entry.root.traverse((object) => {
      const mesh = object as Mesh;
      if (!mesh.isMesh || object.userData.hangarWire) return;
      if (!mesh.geometry.getAttribute("normal")) return;
      const helper = new VertexNormalsHelper(mesh, size, color.getHex());
      (helper.material as LineBasicMaterial).toneMapped = false;
      helpers.push(helper);
      this.helpers.add(helper);
    });
    entry.normals = helpers;
  }

  private removeHelpers(entry: Entry) {
    for (const helper of entry.normals ?? []) {
      this.helpers.remove(helper);
      const lines = helper as LineSegments;
      lines.geometry.dispose();
      (lines.material as Material).dispose();
    }
    entry.normals = null;
    for (const wire of entry.wire ?? []) wire.removeFromParent();
    entry.wire = null;
  }

  private createAxes(): LineSegments {
    const geometry = new BufferGeometry();
    geometry.setAttribute(
      "position",
      new Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1], 3),
    );
    geometry.setAttribute("color", new Float32BufferAttribute(new Array(18).fill(1), 3));
    // Drawn over the model: where the origin is — the pivot an engine will
    // place the asset by — matters most when it is inside the mesh.
    const material = new LineBasicMaterial({
      vertexColors: true,
      depthTest: false,
      transparent: true,
      toneMapped: false,
    });
    const axes = new LineSegments(geometry, material);
    axes.renderOrder = 10;
    axes.name = "hangar:axes";
    return axes;
  }

  private fitHelpers(entry: Entry) {
    const size: [number, number, number] = [
      entry.box.max[0] - entry.box.min[0],
      entry.box.max[1] - entry.box.min[1],
      entry.box.max[2] - entry.box.min[2],
    ];
    const center: [number, number, number] = [
      (entry.box.min[0] + entry.box.max[0]) / 2,
      (entry.box.min[1] + entry.box.max[1]) / 2,
      (entry.box.min[2] + entry.box.max[2]) / 2,
    ];
    this.grid.fit(center, size, entry.box.min[1]);
    const length = Math.max(...size, 1e-3) * 0.6;
    this.axes.scale.setScalar(length);
    const r = entry.radius;
    this.keyLight.position.set(center[0] + r, center[1] + r * 1.6, center[2] + r * 1.3);
    this.keyLight.target.position.set(center[0], center[1], center[2]);
    this.keyLight.target.updateMatrixWorld();
  }

  setTheme(theme: ViewerTheme) {
    this.theme = theme;
    this.grid.setColors(
      new Color(theme.gridMinor),
      new Color(theme.gridMajor),
      theme.dark ? 0.9 : 1,
    );
    const colors = this.axes.geometry.getAttribute("color");
    const axisColors = [theme.axisX, theme.axisY, theme.axisZ].map((css) => new Color(css));
    axisColors.forEach((color, axis) => {
      colors.setXYZ(axis * 2, color.r, color.g, color.b);
      colors.setXYZ(axis * 2 + 1, color.r, color.g, color.b);
    });
    colors.needsUpdate = true;
    this.wireMaterial.color.set(theme.wire);
    this.wireMaterial.opacity = theme.dark ? 0.32 : 0.42;
    const normals = new Color(theme.normals);
    for (const entry of this.entries.values()) {
      for (const helper of entry.normals ?? []) {
        ((helper as LineSegments).material as LineBasicMaterial).color.copy(normals);
      }
    }
    this.requestRender();
  }

  // -------------------------------------------------------------------------
  // Animation
  // -------------------------------------------------------------------------

  private startClip(entry: Entry, index: number, playing: boolean) {
    if (!entry.mixer) return;
    entry.action?.stop();
    const clip = entry.clips[index];
    const action = entry.mixer.clipAction(clip);
    action.setLoop(LoopRepeat, Infinity);
    action.reset().play();
    action.paused = !playing;
    entry.action = action;
    entry.clip = index;
    entry.playing = playing;
  }

  animationState(id: string | null = this.activeId): AnimationState | null {
    const entry = id ? this.entries.get(id) : null;
    if (!entry || !entry.action) return null;
    const duration = entry.clips[entry.clip]?.duration ?? 0;
    return {
      clips: entry.info.animations,
      clip: entry.clip,
      playing: entry.playing,
      time: duration > 0 ? entry.action.time % duration : 0,
      duration,
    };
  }

  private emitAnimation(entry: Entry) {
    const state = this.animationState(entry.id);
    if (state) this.emit({ type: "animation", modelId: entry.id, state });
  }

  setClip(index: number) {
    const entry = this.activeEntry();
    if (!entry?.mixer || !entry.clips[index]) return;
    this.startClip(entry, index, true);
    this.emitAnimation(entry);
    this.requestRender();
  }

  setPlaying(playing: boolean) {
    const entry = this.activeEntry();
    if (!entry?.action) return;
    entry.playing = playing;
    entry.action.paused = !playing;
    this.lastFrame = performance.now();
    this.emitAnimation(entry);
    this.requestRender();
  }

  seek(time: number) {
    const entry = this.activeEntry();
    if (!entry?.action || !entry.mixer) return;
    const duration = entry.clips[entry.clip].duration;
    entry.action.time = Math.max(0, Math.min(time, duration));
    entry.mixer.update(0);
    if (entry.normals) for (const helper of entry.normals) (helper as VertexNormalsHelper).update();
    this.emitAnimation(entry);
    this.requestRender();
  }

  // -------------------------------------------------------------------------
  // Rendering
  // -------------------------------------------------------------------------

  requestRender() {
    this.dirty = true;
    this.schedule();
  }

  private schedule() {
    if (this.raf || this.disposed) return;
    this.raf = requestAnimationFrame(this.frame);
  }

  /**
   * Draws on demand: a frame only when something moved — the camera, its
   * damping settling, a playing animation, a toggle. A still model costs no
   * GPU time at all.
   */
  private frame = (now: number) => {
    this.raf = 0;
    // Hidden (another view, a closed panel): no frames at all, not even for
    // a playing animation. A resize back to a real size starts them again.
    if (this.disposed || !this.width || !this.height) {
      this.lastFrame = 0;
      return;
    }
    // A long pause (a hidden window) is not a jump in the animation.
    const delta = Math.min(0.1, this.lastFrame ? (now - this.lastFrame) / 1000 : 0);
    this.lastFrame = now;
    let again = false;

    if (this.tween) {
      const t = Math.min(1, (now - this.tween.start) / RECENTER_MS);
      const k = easeOut(t);
      this.camera.position.lerpVectors(this.tween.from.position, this.tween.to.position, k);
      this.controls.target.lerpVectors(this.tween.from.target, this.tween.to.target, k);
      if (t >= 1) this.tween = null;
      again = true;
      this.dirty = true;
    }

    if (this.controls.update(delta)) {
      this.dirty = true;
      again = true;
    }

    const entry = this.activeEntry();
    if (entry?.mixer && entry.playing) {
      entry.mixer.update(delta);
      if (entry.normals && entry.info.vertices <= LIVE_NORMALS_LIMIT) {
        for (const helper of entry.normals) (helper as VertexNormalsHelper).update();
      }
      this.emitAnimation(entry);
      this.dirty = true;
      again = true;
    }

    if (this.dirty) this.renderNow();
    if (again) this.schedule();
    else this.lastFrame = 0;
  };

  private resize() {
    const width = Math.round(this.host.clientWidth);
    const height = Math.round(this.host.clientHeight);
    if (width === this.width && height === this.height) return;
    this.width = width;
    this.height = height;
    if (!width || !height) return;
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    const entry = this.activeEntry();
    if (entry && this.autoFramed && !this.tween) {
      const view = this.defaultView(entry);
      this.camera.position.copy(view.position);
      this.controls.target.copy(view.target);
      this.controls.update();
    }
    // Drawn now rather than on the next frame, so a panel being dragged
    // wider never shows one frame of a stretched image; the frame after
    // carries on with whatever was moving.
    this.renderNow();
    this.requestRender();
  }

  private renderNow() {
    if (!this.width || !this.height) return;
    this.updateClipPlanes(this.activeEntry());
    this.renderer.render(this.scene, this.camera);
    this.dirty = false;
  }

  // -------------------------------------------------------------------------
  // Pictures
  // -------------------------------------------------------------------------

  /**
   * Renders one frame at `width × height` and returns it as a PNG.
   *
   * It borrows the visible canvas: the drawing buffer is resized, drawn
   * into, read back and restored within one task, so nothing of it ever
   * reaches the screen, and the picture goes through exactly the pipeline
   * the user sees — tone mapping, colour space, anti-aliasing.
   */
  private async renderToPng(
    camera: PerspectiveCamera,
    width: number,
    height: number,
    background: string | null,
    prepare: () => () => void,
  ): Promise<string> {
    const gl = this.renderer.getContext();
    const max = gl.getParameter(gl.MAX_RENDERBUFFER_SIZE) as number;
    const w = Math.max(1, Math.min(Math.round(width), max));
    const h = Math.max(1, Math.min(Math.round(height), max));

    const ratio = this.renderer.getPixelRatio();
    const clearColor = new Color();
    this.renderer.getClearColor(clearColor);
    const clearAlpha = this.renderer.getClearAlpha();

    const restore = prepare();
    let pixels: Uint8Array;
    try {
      this.renderer.setPixelRatio(1);
      this.renderer.setSize(w, h, false);
      if (background) this.renderer.setClearColor(new Color(background), 1);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      this.renderer.render(this.scene, camera);
      pixels = new Uint8Array(w * h * 4);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    } finally {
      restore();
      this.renderer.setClearColor(clearColor, clearAlpha);
      this.renderer.setPixelRatio(ratio);
      this.renderer.setSize(Math.max(1, this.width), Math.max(1, this.height), false);
      this.camera.aspect = this.aspect();
      this.camera.updateProjectionMatrix();
      this.renderNow();
    }

    // WebGL reads bottom-up; an image is top-down.
    const image = new ImageData(w, h);
    const row = w * 4;
    for (let y = 0; y < h; y++) {
      image.data.set(pixels.subarray((h - 1 - y) * row, (h - y) * row), y * row);
    }
    let blob: Blob;
    if (typeof OffscreenCanvas !== "undefined") {
      const canvas = new OffscreenCanvas(w, h);
      canvas.getContext("2d")!.putImageData(image, 0, 0);
      blob = await canvas.convertToBlob({ type: "image/png" });
    } else {
      const canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
      canvas.getContext("2d")!.putImageData(image, 0, 0);
      blob = await new Promise<Blob>((resolve, reject) =>
        canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("PNG encoding failed"))), "image/png"),
      );
    }
    return blobToBase64(blob);
  }

  /**
   * A picture of any loaded model from a canonical angle, for the agent that
   * asked. The user's camera, tab and toggles are left exactly as they are:
   * the model is swapped in for the one frame and the helpers that are the
   * user's business — wireframe, normals, axes — are left out.
   */
  async snapshot(id: string, options: SnapshotOptions, background: string): Promise<string> {
    const entry = this.entries.get(id);
    if (!entry) throw new Error("The model is not loaded.");
    const view = viewDirection(options.angle);
    const camera = new PerspectiveCamera(FOV, options.width / options.height);
    const framing = frameView(entry.hull, view, FOV, options.width / options.height);
    const target = new Vector3(...framing.target);
    camera.position.set(...framing.position);
    camera.up.set(...view.up);
    camera.lookAt(target);
    this.updateClipPlanes(entry, camera, target);

    return this.renderToPng(camera, options.width, options.height, background, () => {
      const shown = [...this.layer.children];
      const grid = this.grid.visible;
      const axes = this.axes.visible;
      const helpers = this.helpers.visible;
      const wires = (entry.wire ?? []).map((wire) => wire.visible);
      this.layer.clear();
      this.layer.add(entry.root);
      this.fitHelpers(entry);
      // The floor stays: it grounds the model and gives it a scale. Seen
      // from below it would only be lines across the underside.
      this.grid.visible = options.angle !== "bottom";
      this.axes.visible = false;
      this.helpers.visible = false;
      for (const wire of entry.wire ?? []) wire.visible = false;
      return () => {
        this.layer.clear();
        for (const child of shown) this.layer.add(child);
        const active = this.activeEntry();
        if (active) this.fitHelpers(active);
        this.grid.visible = grid;
        this.axes.visible = axes;
        this.helpers.visible = helpers;
        (entry.wire ?? []).forEach((wire, i) => (wire.visible = wires[i]));
      };
    });
  }

  /** What the user sees, at `scale` times the panel's size, for the capture button. */
  async capture(scale: number, background: string): Promise<string> {
    const camera = this.camera.clone();
    const width = Math.min(4096, Math.max(1, this.width) * scale);
    const height = Math.min(4096, Math.max(1, this.height) * scale);
    return this.renderToPng(camera, width, height, background, () => () => undefined);
  }

  // -------------------------------------------------------------------------

  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    cancelAnimationFrame(this.raf);
    this.resizeObserver.disconnect();
    this.host.removeEventListener("keydown", this.onKeyDown);
    this.controls.stopListenToKeyEvents();
    this.canvas.removeEventListener("webglcontextlost", this.onContextLost);
    this.canvas.removeEventListener("webglcontextrestored", this.onContextRestored);
    this.controls.dispose();
    for (const entry of this.entries.values()) this.disposeEntry(entry);
    this.entries.clear();
    this.loaders.dispose();
    this.grid.dispose();
    this.axes.geometry.dispose();
    (this.axes.material as Material).dispose();
    this.wireMaterial.dispose();
    this.environment.dispose();
    this.pmrem.dispose();
    this.renderer.renderLists.dispose();
    this.renderer.dispose();
    // Handed back now rather than whenever the collector gets to it: the
    // terminals' WebGL renderers share the browser's small budget of contexts.
    this.renderer.forceContextLoss();
    this.listeners.clear();
    this.canvas.remove();
  }
}
