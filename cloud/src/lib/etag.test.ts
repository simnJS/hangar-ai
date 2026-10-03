import { describe, expect, it } from "vitest";

import { boardETag, ifNoneMatchSatisfied } from "./etag";

describe("boardETag", () => {
  it("is the board revision, quoted", () => {
    expect(boardETag(0)).toBe('"0"');
    expect(boardETag(42)).toBe('"42"');
  });
});

describe("ifNoneMatchSatisfied", () => {
  it("matches the revision the client already holds", () => {
    expect(ifNoneMatchSatisfied('"7"', boardETag(7))).toBe(true);
  });

  it("does not match a stale revision", () => {
    expect(ifNoneMatchSatisfied('"6"', boardETag(7))).toBe(false);
    // Prefix collisions must not count: rev 7 is not rev 70.
    expect(ifNoneMatchSatisfied('"70"', boardETag(7))).toBe(false);
  });

  it("treats a weak validator as equal — proxies may add the marker", () => {
    expect(ifNoneMatchSatisfied('W/"7"', boardETag(7))).toBe(true);
  });

  it("accepts a list of candidates", () => {
    expect(ifNoneMatchSatisfied('"5", "6", W/"7"', boardETag(7))).toBe(true);
    expect(ifNoneMatchSatisfied('"5", "6"', boardETag(7))).toBe(false);
  });

  it("honours the wildcard", () => {
    expect(ifNoneMatchSatisfied("*", boardETag(3))).toBe(true);
  });

  it("is false when the client sent nothing", () => {
    expect(ifNoneMatchSatisfied(null, boardETag(3))).toBe(false);
    expect(ifNoneMatchSatisfied(undefined, boardETag(3))).toBe(false);
    expect(ifNoneMatchSatisfied("", boardETag(3))).toBe(false);
  });
});
