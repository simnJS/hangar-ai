import { beforeEach, describe, expect, it, vi } from "vitest";
import { modelFormatOf, modelUrl } from "./modelFiles";
import {
  activateModel,
  adoptModelScope,
  closeAllModels,
  closeModel,
  closeWorkspaceModels,
  getModels,
  markModelsSeen,
  modelScope,
  pruneModels,
  reloadModel,
  setModelPanel,
  settleModel,
  showModel,
  takeModelScope,
  whenSettled,
} from "./modelStore";

// whenSettled arms a timer through window.setTimeout.
vi.stubGlobal("window", globalThis);

const show = (path: string, openedBy: string | null = "Ava", workspaceId = "ws") =>
  showModel({ path, format: "glb", workspaceId, openedBy, modifiedMs: 1 });

const info = {
  triangles: 12,
  vertices: 8,
  meshes: 1,
  materials: 1,
  animations: [],
  size: [1, 1, 1] as [number, number, number],
  center: [0, 0, 0] as [number, number, number],
};

describe("modelStore", () => {
  beforeEach(() => closeAllModels());

  it("opens the panel on a model and puts it in front", () => {
    const id = show("C:/m/chair.glb");
    const scope = modelScope("ws");
    expect(scope.panelOpen).toBe(true);
    expect(scope.activeId).toBe(id);
    expect(scope.models[0].title).toBe("chair.glb");
    expect(scope.unseen).toBe(true);
  });

  it("reloads a model shown again instead of opening it twice", () => {
    const first = show("C:/m/chair.glb");
    settleModel(first, { info });
    const again = show("C:/m/chair.glb", "Ben");
    const [model] = modelScope("ws").models;
    expect(again).toBe(first);
    expect(modelScope("ws").models).toHaveLength(1);
    expect(model.revision).toBe(1);
    expect(model.status).toBe("loading");
    expect(model.openedBy).toBe("Ben");
  });

  it("closes the panel with the last model", () => {
    const a = show("C:/m/a.glb");
    const b = show("C:/m/b.glb");
    setModelPanel("ws", { expanded: true });
    closeModel(b);
    expect(modelScope("ws").activeId).toBe(a);
    expect(modelScope("ws").expanded).toBe(true);
    closeModel(a);
    expect(modelScope("ws").panelOpen).toBe(false);
    expect(modelScope("ws").expanded).toBe(false);
    expect(getModels().all).toEqual([]);
  });

  it("tells a waiting caller how the load ended", async () => {
    const id = show("C:/m/a.glb");
    const waiting = whenSettled(id, 1000);
    settleModel(id, { error: "not a glTF file" });
    expect((await waiting)?.error).toBe("not a glTF file");
  });
});

describe("one preview per workspace", () => {
  beforeEach(() => closeAllModels());

  it("keeps each workspace's models, tab in front and panel apart", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const table = show("C:/a/table.glb", "Ava", "A");
    const car = show("C:/b/car.glb", "Ben", "B");
    activateModel(chair);
    setModelPanel("B", { open: false });

    expect(modelScope("A").models.map((model) => model.id)).toEqual([chair, table]);
    expect(modelScope("A").activeId).toBe(chair);
    expect(modelScope("A").panelOpen).toBe(true);
    expect(modelScope("B").models.map((model) => model.id)).toEqual([car]);
    expect(modelScope("B").activeId).toBe(car);
    expect(modelScope("B").panelOpen).toBe(false);
    // The window's viewer holds them all.
    expect(getModels().all.map((model) => model.id).sort()).toEqual([car, chair, table].sort());
  });

  it("has nothing for a workspace no model was shown in, and its panel stays shut", () => {
    show("C:/a/chair.glb", "Ava", "A");
    expect(modelScope("B")).toMatchObject({ models: [], activeId: null, panelOpen: false });
    setModelPanel("B", { open: true });
    expect(modelScope("B").panelOpen).toBe(false);
    expect(modelScope(null).models).toEqual([]);
  });

  it("opens the same file twice in two workspaces, each its own", () => {
    const inA = show("C:/shared/robot.glb", "Ava", "A");
    const inB = show("C:/shared/robot.glb", "Ben", "B");
    expect(inA).not.toBe(inB);
    settleModel(inA, { info });
    expect(modelScope("A").models[0].status).toBe("ready");
    expect(modelScope("B").models[0].status).toBe("loading");
  });

  it("closes in one workspace without touching the other", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const car = show("C:/b/car.glb", "Ben", "B");
    setModelPanel("B", { expanded: true });
    closeModel(chair);
    expect(modelScope("A").panelOpen).toBe(false);
    expect(modelScope("B")).toMatchObject({ activeId: car, panelOpen: true, expanded: true });
    show("C:/a/lamp.glb", "Ava", "A");
    closeWorkspaceModels("B");
    expect(modelScope("B").models).toEqual([]);
    expect(modelScope("A").models.map((model) => model.title)).toEqual(["lamp.glb"]);
  });

  it("brings a tab forward in its own workspace only", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    show("C:/a/table.glb", "Ava", "A");
    const car = show("C:/b/car.glb", "Ben", "B");
    activateModel(chair);
    expect(modelScope("A").activeId).toBe(chair);
    expect(modelScope("B").activeId).toBe(car);
  });

  it("marks what an agent changed as unseen until the workspace's panel is on screen", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    show("C:/b/car.glb", "Ben", "B");
    markModelsSeen("A");
    expect(modelScope("A").unseen).toBe(false);
    expect(modelScope("B").unseen).toBe(true);
    reloadModel(chair, 5);
    expect(modelScope("A").unseen).toBe(true);
  });

  it("forgets the workspaces that were deleted", () => {
    show("C:/a/chair.glb", "Ava", "A");
    show("C:/b/car.glb", "Ben", "B");
    pruneModels(["A", "C"]);
    expect(modelScope("B").models).toEqual([]);
    expect(modelScope("A").models).toHaveLength(1);
    expect(getModels().all).toHaveLength(1);
  });

  it("hands a workspace's preview over to another window as it was", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    show("C:/a/table.glb", "Ava", "A");
    activateModel(chair);
    setModelPanel("A", { expanded: true });
    const scope = takeModelScope("A");
    expect(modelScope("A").models).toEqual([]);
    expect(takeModelScope("A")).toBeNull();
    // The other window, which here is this one again.
    adoptModelScope("A", scope!);
    expect(modelScope("A")).toMatchObject({ activeId: chair, panelOpen: true, expanded: true });
    expect(modelScope("A").models.map((model) => model.title)).toEqual(["chair.glb", "table.glb"]);
  });

  it("keeps what the receiving window has open, and does not open a file twice", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const scope = takeModelScope("A")!;
    const fresh = show("C:/a/chair.glb", "Ava", "A");
    show("C:/a/lamp.glb", "Ava", "A");
    const stool = { ...scope.models[0], id: "x", path: "C:/a/stool.glb", title: "stool.glb" };
    adoptModelScope("A", { ...scope, models: [...scope.models, stool] });
    expect(modelScope("A").models.map((model) => model.title)).toEqual(["chair.glb", "lamp.glb", "stool.glb"]);
    expect(modelScope("A").models[0].id).toBe(fresh);
    expect(modelScope("A").models.some((model) => model.id === chair)).toBe(false);
  });
});

describe("modelFiles", () => {
  it("knows the formats by extension, whatever the case", () => {
    expect(modelFormatOf("a/b/Robot.GLB")).toBe("glb");
    expect(modelFormatOf("a/b/notes.txt")).toBeNull();
    expect(modelFormatOf("README")).toBeNull();
  });

  it("encodes segments but keeps separators, so relative URLs resolve", () => {
    expect(modelUrl("C:\\Users\\me\\my models\\cube.gltf", true)).toBe(
      "http://hangar-model.localhost/C%3A/Users/me/my%20models/cube.gltf",
    );
    expect(modelUrl("/home/me/cube.gltf", false)).toBe("hangar-model://localhost/home/me/cube.gltf");
  });
});
