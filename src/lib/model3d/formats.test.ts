import { describe, expect, it } from "vitest";
import { baseUrlOf, FORMAT_PLANS, httpError } from "./formats";
import { MODEL_FORMATS, modelFormatOf, modelUrl } from "../modelFiles";

describe("FORMAT_PLANS", () => {
  it("has a way to read every format the viewer accepts", () => {
    for (const format of MODEL_FORMATS) expect(FORMAT_PLANS[format]).toBeDefined();
  });

  it("parses the flat geometry formats off the main thread", () => {
    expect(FORMAT_PLANS.obj.parser).toBe("worker");
    expect(FORMAT_PLANS.stl.parser).toBe("worker");
    expect(FORMAT_PLANS.ply.parser).toBe("worker");
  });

  it("sends both glTF flavours through the glTF loader, in metres", () => {
    expect(FORMAT_PLANS.glb).toEqual({ parser: "gltf", unit: "m", zUp: false });
    expect(FORMAT_PLANS.gltf).toEqual(FORMAT_PLANS.glb);
  });

  it("dispatches a file by its extension, whatever its case", () => {
    expect(FORMAT_PLANS[modelFormatOf("C:\\art\\Chair.FBX")!].parser).toBe("fbx");
    expect(FORMAT_PLANS[modelFormatOf("/home/me/scan.ply")!].parser).toBe("worker");
    expect(modelFormatOf("/home/me/notes.txt")).toBeNull();
  });
});

describe("baseUrlOf", () => {
  it("is the model's folder, for the relative URLs inside it", () => {
    const url = modelUrl("C:\\Users\\me\\game\\props\\crate.gltf", true);
    expect(baseUrlOf(url)).toBe("http://hangar-model.localhost/C%3A/Users/me/game/props/");
    expect(new URL("textures/wood.png", baseUrlOf(url)).href).toBe(
      "http://hangar-model.localhost/C%3A/Users/me/game/props/textures/wood.png",
    );
  });

  it("ignores a query or a fragment", () => {
    expect(baseUrlOf("http://x.localhost/a/b.glb?v=3#top")).toBe("http://x.localhost/a/");
  });
});

describe("httpError", () => {
  it("passes the scheme's reason on to whoever reads the error", () => {
    expect(httpError(403, "Forbidden", "outside every allowed folder\n")).toBe(
      "Hangar may not read this file (outside every allowed folder).",
    );
    expect(httpError(404, "Not Found", "")).toBe("The file was not found (Not Found).");
    expect(httpError(500, "", "")).toBe("The file could not be read: HTTP 500.");
  });
});
