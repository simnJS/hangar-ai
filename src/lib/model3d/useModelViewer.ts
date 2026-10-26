import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { modelStat, modelUrl } from "../modelFiles";
import {
  getModels,
  registerSnapshotter,
  reloadModel,
  settleModel,
  type OpenModel,
} from "../modelStore";
import type { LoadProgress, ModelViewer, ViewerTheme } from "./engine";
import { readViewerTheme } from "./viewerTheme";

/**
 * The 3D preview's plumbing, between the store and the engine: creating the
 * viewer, loading what the store says is open, answering snapshots, watching
 * the file. The panel component keeps to layout and controls.
 *
 * The engine — and three.js with it — arrives through a dynamic import here,
 * the first time a panel mounts. Nothing else in the main bundle imports it
 * except for types.
 */

/** How often the file on screen is looked at for a newer version. */
const WATCH_MS = 1500;
/** A changed file is read again this long after, to not load it half-written. */
const SETTLE_MS = 300;

export type ViewerStatus =
  | { kind: "starting" }
  | { kind: "ready" }
  | { kind: "lost" }
  | { kind: "failed"; error: string };

/**
 * One viewer for the panel's lifetime. `generation` recreates it, which is
 * how the panel recovers from a GPU reset the browser did not give back.
 */
export function useViewer(hostRef: RefObject<HTMLElement | null>, generation: number) {
  const [viewer, setViewer] = useState<ModelViewer | null>(null);
  const [status, setStatus] = useState<ViewerStatus>({ kind: "starting" });
  // Settled once per viewer, for the snapshotter, which may be asked before
  // three.js has even arrived.
  const ready = useRef<Deferred | null>(null);
  ready.current ??= deferred();

  useEffect(() => {
    let alive = true;
    let created: ModelViewer | null = null;
    setStatus({ kind: "starting" });
    import("./engine")
      .then(({ ModelViewer }) => {
        const host = hostRef.current;
        if (!alive || !host) return;
        created = new ModelViewer(host);
        created.setTheme(readViewerTheme());
        created.on((event) => {
          if (event.type === "contextlost") setStatus({ kind: "lost" });
          else if (event.type === "contextrestored") setStatus({ kind: "ready" });
        });
        setViewer(created);
        setStatus({ kind: "ready" });
        ready.current?.resolve(created);
      })
      .catch((error: unknown) => {
        if (!alive) return;
        const message = error instanceof Error ? error.message : String(error);
        setStatus({ kind: "failed", error: message });
        ready.current?.reject(new Error(`The 3D preview could not start: ${message}`));
      });
    return () => {
      alive = false;
      created?.dispose();
      setViewer(null);
      ready.current = deferred();
    };
  }, [hostRef, generation]);

  return { viewer, status, ready };
}

type Deferred = ReturnType<typeof deferred>;

function deferred() {
  let resolve!: (v: ModelViewer) => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<ModelViewer>((ok, ko) => {
    resolve = ok;
    reject = ko;
  });
  // Nobody may be waiting; a failure is shown on screen either way.
  promise.catch(() => undefined);
  return { promise, resolve, reject };
}

/** The theme's colours for the viewport, kept current as the theme changes. */
export function useViewerTheme(viewer: ModelViewer | null): ViewerTheme {
  const [theme, setTheme] = useState(readViewerTheme);
  useEffect(() => {
    // The theme lands as inline custom properties on <html>
    // (themes.ts › applyThemeToDocument): that is the change to watch.
    const observer = new MutationObserver(() => setTheme(readViewerTheme()));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    viewer?.setTheme(theme);
  }, [viewer, theme]);
  return theme;
}

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

interface InFlight {
  revision: number;
  controller: AbortController;
  promise: Promise<void>;
}

const isAbort = (error: unknown) => error instanceof Error && error.name === "LoadAborted";

/**
 * Loads models into the viewer as the store asks: the one in front, and any
 * other a snapshot is wanted of. One load per model at a time — a newer
 * revision cancels the one in flight — and every outcome is reported to the
 * store with `settleModel`, which is what an agent waiting on `model_show`
 * hears.
 */
export function useModelLoads(viewer: ModelViewer | null) {
  const [progress, setProgress] = useState<Record<string, LoadProgress>>({});
  const inflight = useRef(new Map<string, InFlight>());
  const failed = useRef(new Map<string, { revision: number; error: string }>());
  const lastPaint = useRef(new Map<string, number>());

  // A new viewer starts empty: whatever was in flight for the old one is moot.
  useEffect(() => {
    const running = inflight.current;
    const errors = failed.current;
    return () => {
      for (const { controller } of running.values()) controller.abort();
      running.clear();
      errors.clear();
    };
  }, [viewer]);

  const report = useCallback((id: string, next: LoadProgress | null) => {
    // Progress is drawn ten times a second at most: a stream of 64 kB chunks
    // would otherwise re-render the panel for every one of them.
    const now = performance.now();
    const last = lastPaint.current.get(id) ?? 0;
    setProgress((current) => {
      if (!next) {
        if (!(id in current)) return current;
        const rest = { ...current };
        delete rest[id];
        return rest;
      }
      const before = current[id];
      if (before && before.phase === next.phase && now - last < 100) return current;
      lastPaint.current.set(id, now);
      return { ...current, [id]: next };
    });
  }, []);

  /**
   * Loads `model` into `viewer` unless that revision is there or on its way.
   * The viewer is passed in rather than read from the render, so a snapshot
   * asked for while the engine was still arriving uses the one that just
   * arrived, not the null of the render before.
   */
  const ensure = useCallback(
    (model: OpenModel, viewer: ModelViewer): Promise<void> => {
      if (viewer.has(model.id, model.revision)) return Promise.resolve();
      const failure = failed.current.get(model.id);
      if (failure?.revision === model.revision) return Promise.reject(new Error(failure.error));
      const running = inflight.current.get(model.id);
      if (running?.revision === model.revision) return running.promise;
      running?.controller.abort();

      const controller = new AbortController();
      // The size, for a progress bar that fills even when the scheme streams
      // the file without announcing its length.
      const promise = modelStat(model.path)
        .then(
          (stat) => stat.size,
          () => 0,
        )
        .then((sizeHint) =>
          viewer.load(
            {
              id: model.id,
              revision: model.revision,
              url: modelUrl(model.path),
              format: model.format,
              sizeHint,
            },
            { signal: controller.signal, onProgress: (next) => report(model.id, next) },
          ),
        )
        .then((info) => {
          failed.current.delete(model.id);
          settleModel(model.id, { info });
        })
        .catch((error: unknown) => {
          if (isAbort(error) || controller.signal.aborted) throw error;
          const message = errorText(error);
          failed.current.set(model.id, { revision: model.revision, error: message });
          settleModel(model.id, { error: message });
          throw new Error(message);
        })
        .finally(() => {
          if (inflight.current.get(model.id)?.promise === promise) {
            inflight.current.delete(model.id);
            report(model.id, null);
          }
        });
      inflight.current.set(model.id, { revision: model.revision, controller, promise });
      return promise;
    },
    [report],
  );

  /** Forgets the models that were closed: loads stop, GPU memory goes. */
  const forget = useCallback(
    (open: Set<string>) => {
      for (const [id, { controller }] of inflight.current) {
        if (open.has(id)) continue;
        controller.abort();
        inflight.current.delete(id);
        report(id, null);
      }
      for (const id of failed.current.keys()) if (!open.has(id)) failed.current.delete(id);
      for (const id of viewer?.ids() ?? []) if (!open.has(id)) viewer?.remove(id);
    },
    [viewer, report],
  );

  return { progress, ensure, forget };
}

/**
 * Renders snapshots for agents: of any model open in the window, loaded first
 * when it is not — the one in front or not, its workspace on screen or not,
 * the panel shown or not. The user's view is never touched
 * (ModelViewer.snapshot). Which workspace's model an agent may ask for is
 * lib/modelHost's business.
 */
export function useSnapshotter(
  ready: RefObject<{ promise: Promise<ModelViewer> } | null>,
  ensure: (model: OpenModel, viewer: ModelViewer) => Promise<void>,
) {
  const ensureRef = useRef(ensure);
  ensureRef.current = ensure;
  useEffect(() => {
    registerSnapshotter(async (id, options) => {
      const pending = ready.current;
      if (!pending) throw new Error("The 3D preview is not mounted.");
      const viewer = await pending.promise;
      // The file may be rewritten while it loads: whatever revision the
      // store holds once a load lands is the one pictured.
      for (let attempt = 0; attempt < 3; attempt++) {
        const model = getModels().all.find((entry) => entry.id === id);
        if (!model) throw new Error("That model is no longer open in Hangar.");
        await ensureRef.current(model, viewer);
        const latest = getModels().all.find((entry) => entry.id === id);
        if (!latest || viewer.has(id, latest.revision)) break;
      }
      return viewer.snapshot(id, options, readViewerTheme().background);
    });
    return () => registerSnapshotter(null);
  }, [ready]);
}

/**
 * Watches the file in front while the panel is on screen: a newer
 * modification time reloads it, the camera staying where it is. A file that
 * disappears or turns unreadable keeps its last version on screen and says
 * so, rather than going blank under the user.
 */
export function useModelWatch(model: OpenModel | null, watching: boolean) {
  const [missing, setMissing] = useState<string | null>(null);
  const id = model?.id ?? null;
  const path = model?.path ?? null;
  const modifiedMs = model?.modifiedMs ?? 0;

  useEffect(() => setMissing(null), [id]);

  useEffect(() => {
    if (!watching || !id || !path) return;
    let cancelled = false;
    let busy = false;
    const tick = async () => {
      if (busy || document.hidden) return;
      busy = true;
      try {
        const stat = await modelStat(path);
        if (cancelled) return;
        setMissing(null);
        if (stat.modifiedMs <= modifiedMs) return;
        // Once more a moment later: an agent writing the file in several
        // goes would otherwise have its first half loaded.
        await new Promise((resolve) => window.setTimeout(resolve, SETTLE_MS));
        const again = await modelStat(path);
        if (cancelled) return;
        if (again.modifiedMs === stat.modifiedMs && again.size === stat.size) {
          reloadModel(id, again.modifiedMs);
        }
      } catch (error) {
        if (!cancelled) setMissing(errorText(error));
      } finally {
        busy = false;
      }
    };
    const timer = window.setInterval(tick, WATCH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [watching, id, path, modifiedMs]);

  return missing;
}
