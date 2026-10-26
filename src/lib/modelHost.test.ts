import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Tauri's event bus, in process: every window hears every emit, as in the app.
vi.mock("@tauri-apps/api/event", () => {
  const subscribers = new Map<string, Set<(event: { payload: unknown }) => void>>();
  return {
    listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
      const set = subscribers.get(name) ?? new Set();
      subscribers.set(name, set);
      set.add(handler);
      return () => set.delete(handler);
    },
    emit: async (name: string, payload: unknown) => {
      for (const handler of [...(subscribers.get(name) ?? [])]) handler({ payload });
    },
  };
});

// The store and the host arm their timers through window.setTimeout.
vi.stubGlobal("window", globalThis);

const { localModelHost, remoteModelHost, serveModelHost } = await import("./modelHost");
const { closeAllModels, getModels, registerSnapshotter, settleModel } = await import("./modelStore");

const chair = {
  path: "C:\\dev\\shop\\chair.glb",
  format: "glb" as const,
  workspaceId: "w2",
  openedBy: "Eve",
  modifiedMs: 1,
};

const info = {
  triangles: 12,
  vertices: 8,
  meshes: 1,
  materials: 1,
  animations: [],
  size: [1, 1, 1] as [number, number, number],
  center: [0, 0, 0] as [number, number, number],
};

describe("a detached workspace's preview", () => {
  let stop: () => void = () => undefined;
  beforeEach(() => {
    closeAllModels();
    registerSnapshotter(null);
    // The window drawing "w2" — the same store here, which is what lets the
    // test see the call land.
    stop = serveModelHost("w2");
  });
  afterEach(() => {
    stop();
    vi.useRealTimers();
  });

  it("is reached through the window that shows the workspace", async () => {
    const host = await remoteModelHost("w2");
    expect(host).not.toBeNull();
    const id = await host!.show(chair);
    expect(getModels().models.map((model) => [model.id, model.openedBy])).toEqual([[id, "Eve"]]);
    const waiting = host!.whenSettled(id, 1000);
    settleModel(id, { info });
    expect((await waiting)?.info).toEqual(info);
    expect((await host!.list()).activeId).toBe(id);
  });

  it("carries the other window's errors back as they were", async () => {
    registerSnapshotter(async () => {
      throw new Error("WebGL context lost");
    });
    const host = await remoteModelHost("w2");
    const id = await host!.show(chair);
    await expect(host!.snapshot(id, { angle: "iso", width: 64, height: 64 })).rejects.toThrow(
      "WebGL context lost",
    );
  });

  it("is null when no window answers for the workspace", async () => {
    vi.useFakeTimers();
    const pending = remoteModelHost("w9");
    await vi.advanceTimersByTimeAsync(2000);
    expect(await pending).toBeNull();
  });
});

describe("this window's preview", () => {
  beforeEach(() => {
    closeAllModels();
    registerSnapshotter(null);
  });
  afterEach(() => vi.useRealTimers());

  it("gives a panel that is mounting again the time to register its renderer", async () => {
    vi.useFakeTimers();
    const id = await localModelHost.show(chair);
    await localModelHost.reveal(id);
    expect(getModels().panelOpen).toBe(true);
    const pending = localModelHost.snapshot(id, { angle: "front", width: 64, height: 64 });
    await vi.advanceTimersByTimeAsync(400);
    registerSnapshotter(async (modelId, options) => `${modelId}:${options.angle}`);
    await vi.advanceTimersByTimeAsync(200);
    expect(await pending).toBe(`${id}:front`);
  });

  it("closes the panel with the last model it closes", async () => {
    const a = await localModelHost.show(chair);
    const b = await localModelHost.show({ ...chair, path: "C:\\dev\\shop\\table.glb" });
    await localModelHost.close([a]);
    expect(getModels().models.map((model) => model.id)).toEqual([b]);
    await localModelHost.close([b]);
    expect(getModels().panelOpen).toBe(false);
  });
});
