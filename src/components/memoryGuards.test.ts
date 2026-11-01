import { describe, expect, it } from "vitest";
import type { MemoryEntry } from "../lib/memory";
import { findTitleClash, nearCap, sortEntries, titleKey } from "./memoryGuards";

const entry = (id: string, title: string, updated_at = 1): MemoryEntry => ({
  id,
  title,
  content: "body",
  tags: [],
  workspace: "",
  agent: "",
  created_at: 1,
  updated_at,
});

describe("findTitleClash", () => {
  const entries = [entry("a", "Build steps"), entry("b", "ONNX cannot ship")];

  it("finds the entry a title would replace, ignoring case and edge whitespace", () => {
    expect(findTitleClash(entries, "build steps")?.id).toBe("a");
    expect(findTitleClash(entries, "  ONNX Cannot Ship  ")?.id).toBe("b");
  });

  it("finds nothing for a free title or a blank one", () => {
    expect(findTitleClash(entries, "Something else")).toBeNull();
    expect(findTitleClash(entries, "   ")).toBeNull();
    expect(findTitleClash([], "Build steps")).toBeNull();
  });

  it("does not match a title that only contains another", () => {
    expect(findTitleClash(entries, "Build")).toBeNull();
  });

  it("uses the same key as the backend upsert", () => {
    expect(titleKey("  bUiLd StEpS ")).toBe("build steps");
  });
});

describe("nearCap", () => {
  it("starts at 900 entries", () => {
    expect(nearCap(899)).toBe(false);
    expect(nearCap(900)).toBe(true);
    expect(nearCap(1000)).toBe(true);
  });
});

describe("sortEntries", () => {
  const entries = [entry("mid", "m", 5), entry("old", "o", 1), entry("new", "n", 9)];

  it("puts the oldest update first for a clean-up", () => {
    expect(sortEntries(entries, "oldest").map((e) => e.id)).toEqual(["old", "mid", "new"]);
  });

  it("puts the latest update first by default and leaves the input alone", () => {
    expect(sortEntries(entries, "recent").map((e) => e.id)).toEqual(["new", "mid", "old"]);
    expect(entries.map((e) => e.id)).toEqual(["mid", "old", "new"]);
  });
});
