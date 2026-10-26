import { describe, expect, it } from "vitest";
import { nextRegion, REGION_ORDER, type Region } from "./regions";

const all: Region[] = [...REGION_ORDER];

describe("nextRegion", () => {
  it("walks the regions as they sit on screen", () => {
    expect(nextRegion(all, "navigator", 1)).toBe("toolbar");
    expect(nextRegion(all, "toolbar", 1)).toBe("main");
    expect(nextRegion(all, "main", 1)).toBe("dock");
    expect(nextRegion(all, "dock", 1)).toBe("inspector");
    expect(nextRegion(all, "inspector", 1)).toBe("status");
  });

  it("wraps around both ways", () => {
    expect(nextRegion(all, "status", 1)).toBe("navigator");
    expect(nextRegion(all, "navigator", -1)).toBe("status");
    expect(nextRegion(all, "main", -1)).toBe("toolbar");
  });

  it("skips the regions that are not on screen, in whatever order they are given", () => {
    const shown: Region[] = ["status", "main", "toolbar"];
    expect(nextRegion(shown, "toolbar", 1)).toBe("main");
    expect(nextRegion(shown, "main", 1)).toBe("status");
    expect(nextRegion(shown, "toolbar", -1)).toBe("status");
  });

  it("starts at either end from outside every region", () => {
    expect(nextRegion(all, null, 1)).toBe("navigator");
    expect(nextRegion(all, null, -1)).toBe("status");
    // Focus in a region that has just gone counts as outside.
    expect(nextRegion(["toolbar", "main"], "inspector", 1)).toBe("toolbar");
  });

  it("has nowhere to go without a region", () => {
    expect(nextRegion([], null, 1)).toBeNull();
  });
});
