import { beforeEach, describe, expect, it, vi } from "vitest";
import { modelFormatOf, modelUrl } from "./modelFiles";
import {
  closeAllModels,
  closeModel,
  getModels,
  settleModel,
  showModel,
  whenSettled,
} from "./modelStore";

// whenSettled arms a timer through window.setTimeout.
vi.stubGlobal("window", globalThis);

const show = (path: string, openedBy: string | null = "Ava") =>
  showModel({ path, format: "glb", workspaceId: "ws", openedBy, modifiedMs: 1 });

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
    const state = getModels();
    expect(state.panelOpen).toBe(true);
    expect(state.activeId).toBe(id);
    expect(state.models[0].title).toBe("chair.glb");
  });

  it("reloads a model shown again instead of opening it twice", () => {
    const first = show("C:/m/chair.glb");
    settleModel(first, { info });
    const again = show("C:/m/chair.glb", "Ben");
    const [model] = getModels().models;
    expect(again).toBe(first);
    expect(getModels().models).toHaveLength(1);
    expect(model.revision).toBe(1);
    expect(model.status).toBe("loading");
    expect(model.openedBy).toBe("Ben");
  });

  it("closes the panel with the last model", () => {
    const a = show("C:/m/a.glb");
    const b = show("C:/m/b.glb");
    closeModel(b);
    expect(getModels().activeId).toBe(a);
    closeModel(a);
    expect(getModels().panelOpen).toBe(false);
  });

  it("tells a waiting caller how the load ended", async () => {
    const id = show("C:/m/a.glb");
    const waiting = whenSettled(id, 1000);
    settleModel(id, { error: "not a glTF file" });
    expect((await waiting)?.error).toBe("not a glTF file");
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
