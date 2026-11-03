import { afterEach, describe, expect, it, vi } from "vitest";
import { webglWanted } from "./webgl";

const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const WINDOWS = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0 Edg/131.0";
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15";
const ANDROID = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 Chrome/131.0 Mobile";

function platform(userAgent: string, stored: string | null | "throws" = null) {
  vi.stubGlobal("navigator", { userAgent });
  vi.stubGlobal("localStorage", {
    getItem: () => {
      if (stored === "throws") throw new Error("blocked");
      return stored;
    },
  });
}

describe("webglWanted", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps WebGL on Windows and macOS", () => {
    platform(WINDOWS);
    expect(webglWanted()).toBe(true);
    platform(MAC);
    expect(webglWanted()).toBe(true);
  });

  it("falls back to the DOM renderer on Linux, where WebKitGTK is slow with WebGL", () => {
    platform(LINUX);
    expect(webglWanted()).toBe(false);
  });

  it("does not mistake Android for desktop Linux", () => {
    platform(ANDROID);
    expect(webglWanted()).toBe(true);
  });

  it("lets a stored choice override the platform either way", () => {
    platform(LINUX, "1");
    expect(webglWanted()).toBe(true);
    platform(WINDOWS, "0");
    expect(webglWanted()).toBe(false);
  });

  it("ignores any other stored value", () => {
    platform(LINUX, "yes");
    expect(webglWanted()).toBe(false);
  });

  it("uses the platform default when storage is blocked", () => {
    platform(LINUX, "throws");
    expect(webglWanted()).toBe(false);
    platform(WINDOWS, "throws");
    expect(webglWanted()).toBe(true);
  });
});
