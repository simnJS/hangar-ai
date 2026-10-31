import { describe, expect, it } from "vitest";
import { kitIconUrl } from "./kitIcon";
import type { KitInfo } from "./kits";

function kit(over: Partial<KitInfo> & { icon?: string; manifestId?: string } = {}) {
  const { icon, manifestId, ...rest } = over;
  return {
    id: "hello-kit",
    error: null,
    manifest: { id: manifestId ?? "hello-kit", icon } as KitInfo["manifest"],
    ...rest,
  } as Pick<KitInfo, "id" | "error" | "manifest">;
}

describe("kitIconUrl", () => {
  it("uses the http .localhost form on Windows", () => {
    expect(kitIconUrl(kit({ icon: "icon.svg" }), true)).toBe("http://hangar-kit.localhost/hello-kit/icon.svg");
  });

  it("uses the scheme itself elsewhere", () => {
    expect(kitIconUrl(kit({ icon: "assets/icon.png" }), false)).toBe("hangar-kit://localhost/hello-kit/assets/icon.png");
  });

  it("encodes each segment of the path", () => {
    expect(kitIconUrl(kit({ icon: "my icons/é#1.svg" }), false)).toBe(
      "hangar-kit://localhost/hello-kit/my%20icons/%C3%A9%231.svg",
    );
  });

  it("gives nothing for a kit without icon, broken, or under another id", () => {
    expect(kitIconUrl(kit(), true)).toBeNull();
    expect(kitIconUrl(kit({ icon: "  " }), true)).toBeNull();
    expect(kitIconUrl(kit({ icon: "i.svg", error: "bad manifest" }), true)).toBeNull();
    expect(kitIconUrl({ id: "x", error: null, manifest: null }, true)).toBeNull();
    expect(kitIconUrl(kit({ icon: "i.svg", manifestId: "other" }), true)).toBeNull();
  });
});
