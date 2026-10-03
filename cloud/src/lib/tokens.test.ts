import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";

import {
  TOKEN_PREFIX,
  TOKEN_TOUCH_INTERVAL_MS,
  generateBoardToken,
  hashBoardToken,
  looksLikeBoardToken,
  parseBearer,
  shouldTouchToken,
} from "./tokens";

describe("generateBoardToken", () => {
  it("carries the prefix that makes a leaked token recognisable", () => {
    expect(generateBoardToken().startsWith(TOKEN_PREFIX)).toBe(true);
  });

  it("encodes 32 random bytes as base64url", () => {
    const body = generateBoardToken().slice(TOKEN_PREFIX.length);
    // 32 bytes -> 43 base64 characters, no padding, url-safe alphabet only.
    expect(body).toHaveLength(43);
    expect(body).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it("never repeats", () => {
    const seen = new Set(Array.from({ length: 200 }, generateBoardToken));
    expect(seen.size).toBe(200);
  });
});

describe("hashBoardToken", () => {
  it("hashes the full token, prefix included", () => {
    const token = "hgr_abcdef";
    expect(hashBoardToken(token)).toBe(
      createHash("sha256").update(token, "utf8").digest("hex"),
    );
  });

  it("is stable and 64 hex characters", () => {
    const token = generateBoardToken();
    expect(hashBoardToken(token)).toBe(hashBoardToken(token));
    expect(hashBoardToken(token)).toMatch(/^[0-9a-f]{64}$/);
  });

  it("does not leak the plaintext", () => {
    const token = generateBoardToken();
    expect(hashBoardToken(token)).not.toContain(token.slice(TOKEN_PREFIX.length));
  });

  it("separates two tokens that differ by one character", () => {
    expect(hashBoardToken("hgr_a")).not.toBe(hashBoardToken("hgr_b"));
  });
});

describe("looksLikeBoardToken", () => {
  it("rejects the prefix on its own and foreign credentials", () => {
    expect(looksLikeBoardToken(generateBoardToken())).toBe(true);
    expect(looksLikeBoardToken(TOKEN_PREFIX)).toBe(false);
    expect(looksLikeBoardToken("sk_live_whatever")).toBe(false);
  });
});

describe("parseBearer", () => {
  it("reads the token out of an Authorization header", () => {
    expect(parseBearer("Bearer hgr_token")).toBe("hgr_token");
  });

  it("accepts any casing and extra whitespace", () => {
    expect(parseBearer("bearer   hgr_token")).toBe("hgr_token");
    expect(parseBearer("  BEARER hgr_token  ")).toBe("hgr_token");
  });

  it("returns null for anything that is not a bearer credential", () => {
    expect(parseBearer(null)).toBeNull();
    expect(parseBearer("")).toBeNull();
    expect(parseBearer("Bearer")).toBeNull();
    expect(parseBearer("Bearer ")).toBeNull();
    expect(parseBearer("Basic hgr_token")).toBeNull();
  });
});

describe("shouldTouchToken", () => {
  const now = 1_700_000_000_000;

  it("writes the first time a token is used", () => {
    expect(shouldTouchToken(null, now)).toBe(true);
    expect(shouldTouchToken(undefined, now)).toBe(true);
  });

  it("skips the write for a token used moments ago", () => {
    expect(shouldTouchToken(now - 1_000, now)).toBe(false);
    expect(shouldTouchToken(now - (TOKEN_TOUCH_INTERVAL_MS - 1), now)).toBe(false);
  });

  it("writes again once the interval has elapsed", () => {
    expect(shouldTouchToken(now - TOKEN_TOUCH_INTERVAL_MS, now)).toBe(true);
    expect(shouldTouchToken(now - 3_600_000, now)).toBe(true);
  });
});
