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

const { listen } = await import("@tauri-apps/api/event");
const { handBackModels, localModelHost, remoteModelHost, serveModelHandovers, serveModelHost } =
  await import("./modelHost");
const { closeAllModels, modelScope, noteModelView, registerSnapshotter, settleModel, showModel } =
  await import("./modelStore");

const chair = {
  path: "C:\\dev\\shop\\chair.glb",
  format: "glb" as const,
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

/** Lets the bus deliver what was emitted: its listeners register asynchronously. */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

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
    expect(modelScope("w2").models.map((model) => [model.id, model.openedBy])).toEqual([[id, "Eve"]]);
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

describe("a workspace's preview in this window", () => {
  beforeEach(() => {
    closeAllModels();
    registerSnapshotter(null);
    noteModelView(null, false);
  });
  afterEach(() => vi.useRealTimers());

  it("gives a viewer that is mounting the time to register its renderer", async () => {
    vi.useFakeTimers();
    const host = localModelHost("w1");
    const id = await host.show(chair);
    await host.reveal(id);
    expect(modelScope("w1").panelOpen).toBe(true);
    const pending = host.snapshot(id, { angle: "front", width: 64, height: 64 });
    await vi.advanceTimersByTimeAsync(400);
    registerSnapshotter(async (modelId, options) => `${modelId}:${options.angle}`);
    await vi.advanceTimersByTimeAsync(200);
    expect(await pending).toBe(`${id}:front`);
  });

  it("closes the panel with the last model it closes", async () => {
    const host = localModelHost("w1");
    const a = await host.show(chair);
    const b = await host.show({ ...chair, path: "C:\\dev\\shop\\table.glb" });
    await host.close([a]);
    expect(modelScope("w1").models.map((model) => model.id)).toEqual([b]);
    await host.close([b]);
    expect(modelScope("w1").panelOpen).toBe(false);
  });

  it("opens in its own workspace whatever is on screen, and says what is", async () => {
    noteModelView("w1", true);
    const shop = localModelHost("w2");
    const id = await shop.show(chair);
    expect(modelScope("w2").activeId).toBe(id);
    expect(modelScope("w1").models).toEqual([]);
    expect(await shop.list()).toMatchObject({ activeId: id, onScreen: false, shown: false });
    noteModelView("w2", false);
    expect(await shop.list()).toMatchObject({ onScreen: true, shown: false });
    noteModelView("w2", true);
    expect(await shop.list()).toMatchObject({ onScreen: true, shown: true });
  });

  it("never lists, renders, reveals or closes another workspace's model", async () => {
    registerSnapshotter(async (modelId) => modelId);
    const hangar = localModelHost("w1");
    const shop = localModelHost("w2");
    const mine = await hangar.show({ ...chair, path: "C:\\dev\\hangar\\robot.glb" });
    const theirs = await shop.show(chair);
    expect((await hangar.list()).models.map((model) => model.id)).toEqual([mine]);
    await expect(hangar.snapshot(theirs, { angle: "iso", width: 64, height: 64 })).rejects.toThrow(
      /not open in this workspace/,
    );
    await expect(hangar.reveal(theirs)).rejects.toThrow(/not open in this workspace/);
    expect(await hangar.whenSettled(theirs, 10)).toBeNull();
    await hangar.close([mine, theirs]);
    expect(modelScope("w1").models).toEqual([]);
    expect(modelScope("w2").models.map((model) => model.id)).toEqual([theirs]);
    expect(await shop.snapshot(theirs, { angle: "iso", width: 64, height: 64 })).toBe(theirs);
  });
});

describe("a workspace moving between windows", () => {
  const stops: (() => void)[] = [];
  beforeEach(() => closeAllModels());
  afterEach(() => {
    for (const stop of stops.splice(0)) stop();
  });

  it("takes its models to its window, and brings them back", async () => {
    const sent: { to: string; workspaceId: string; scope: { models: unknown[] } }[] = [];
    stops.push(await listen("model:handover", (event) => void sent.push(event.payload as never)));
    // The main window has a model open for w3 before w3 moves out.
    const id = showModel({ ...chair, workspaceId: "w3" });
    stops.push(serveModelHandovers());
    await flush();
    // w3's window comes up and asks for what is its own.
    stops.push(serveModelHost("w3"));
    await flush();
    await flush();
    expect(sent.map(({ to, workspaceId, scope }) => [to, workspaceId, scope.models.length])).toEqual([
      ["w3", "w3", 1],
    ]);
    // Taken from the main window, adopted by w3's: the same store here.
    expect(modelScope("w3").models.map((model) => model.id)).toEqual([id]);

    handBackModels("w3");
    await flush();
    expect(sent.map(({ to }) => to)).toEqual(["w3", "main"]);
    expect(modelScope("w3").models.map((model) => model.id)).toEqual([id]);
  });

  it("sends nothing for a workspace the main window had no model for", async () => {
    const sent: unknown[] = [];
    stops.push(await listen("model:handover", (event) => void sent.push(event.payload)));
    stops.push(serveModelHandovers());
    stops.push(serveModelHost("w4"));
    await flush();
    await flush();
    handBackModels("w4");
    await flush();
    expect(sent).toEqual([]);
  });
});
