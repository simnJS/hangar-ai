import { describe, expect, it } from "vitest";
import type { KitInfo, KitManifest } from "./kits";
import { effectivePermissions, kitFileUrl, kitTabState, workspaceKitTabs } from "./kitTabs";

const manifest = (id: string, tabs: KitManifest["tabs"]): KitManifest => ({
  schema: 1,
  id,
  name: `${id} name`,
  version: "1.0.0",
  description: "",
  tabs,
  setup: [],
});

const kit = (id: string, tabs: KitManifest["tabs"], extra: Partial<KitInfo> = {}): KitInfo => ({
  id,
  source: { kind: "folder", path: `D:/kits/${id}` },
  dir: `D:/kits/${id}`,
  installedVersion: "1.0.0",
  addedAt: 1,
  manifest: manifest(id, tabs),
  error: null,
  update: null,
  ...extra,
});

const status = { id: "status", title: "Status", entry: "tabs/status/index.html", permissions: ["panes.read", "notify"] as const };
const explorer = {
  id: "explorer",
  title: "Explorer",
  entry: "tabs/explorer/index.html",
  permissions: ["files.read", "preview.show", "panes.send"] as const,
};

const installed: KitInfo[] = [
  kit("hello-kit", [{ ...status, permissions: [...status.permissions] }]),
  kit("roblox", [{ ...explorer, permissions: [...explorer.permissions] }]),
  kit("broken-kit", [], { error: "its manifest is invalid", manifest: null }),
  kit("future-kit", [{ ...status, permissions: [...status.permissions] }], { error: "needs Hangar >=9.0.0" }),
];

describe("kitFileUrl", () => {
  it("reaches the scheme the way each platform's webview does", () => {
    expect(kitFileUrl("hello-kit", "tabs/status/index.html", true)).toBe(
      "http://hangar-kit.localhost/hello-kit/tabs/status/index.html",
    );
    expect(kitFileUrl("hello-kit", "tabs/status/index.html", false)).toBe(
      "hangar-kit://localhost/hello-kit/tabs/status/index.html",
    );
  });

  it("encodes each segment and keeps the separators, so relative URLs stay in the kit", () => {
    expect(kitFileUrl("hello-kit", "./tabs\\my tab/é#1.html", true)).toBe(
      "http://hangar-kit.localhost/hello-kit/tabs/my%20tab/%C3%A9%231.html",
    );
  });
});

describe("effectivePermissions", () => {
  it("keeps what the tab declares and the person granted, both", () => {
    expect(effectivePermissions(["panes.read", "notify"], ["notify", "files.read"])).toEqual(["notify"]);
    expect(effectivePermissions(["panes.read"], [])).toEqual([]);
    expect(effectivePermissions([], ["panes.read"])).toEqual([]);
    expect(effectivePermissions(["files.write", "notify"], ["files.write", "notify"])).toEqual(["notify"]);
  });
});

describe("workspaceKitTabs", () => {
  it("lists the tabs of the kits enabled on the workspace, in their order", () => {
    const ws = {
      kits: [
        { id: "roblox", granted: ["files.read", "panes.send", "notify"] },
        { id: "hello-kit", granted: ["panes.read"] },
      ],
    };
    expect(workspaceKitTabs(ws, installed)).toEqual([
      {
        kitId: "roblox",
        kitName: "roblox name",
        tab: { ...explorer, permissions: [...explorer.permissions] },
        permissions: ["panes.send", "files.read"],
      },
      {
        kitId: "hello-kit",
        kitName: "hello-kit name",
        tab: { ...status, permissions: [...status.permissions] },
        permissions: ["panes.read"],
      },
    ]);
  });

  it("leaves out kits that are not enabled, not installed, or not usable", () => {
    const ws = {
      kits: [
        { id: "missing-kit", granted: [] },
        { id: "broken-kit", granted: [] },
        { id: "future-kit", granted: ["notify"] },
      ],
    };
    expect(workspaceKitTabs(ws, installed)).toEqual([]);
    expect(workspaceKitTabs({}, installed)).toEqual([]);
    expect(workspaceKitTabs({ kits: [{ id: "hello-kit", granted: [] }] }, null)).toEqual([]);
  });

  it("refuses a kit whose manifest names another id than the one enabled", () => {
    const renamed = kit("hello-kit", [{ ...status, permissions: [] }], { manifest: manifest("other", []) });
    expect(workspaceKitTabs({ kits: [{ id: "hello-kit", granted: [] }] }, [renamed])).toEqual([]);
  });
});

describe("kitTabState", () => {
  const ws = { kits: [{ id: "hello-kit", granted: ["panes.read", "notify"] }, { id: "broken-kit", granted: [] }] };

  it("is ready with the page's URL and what it may call", () => {
    expect(kitTabState(ws, installed, "hello-kit", "status", true)).toEqual({
      kind: "ready",
      url: "http://hangar-kit.localhost/hello-kit/tabs/status/index.html",
      choice: {
        kitId: "hello-kit",
        kitName: "hello-kit name",
        tab: { ...status, permissions: [...status.permissions] },
        permissions: ["panes.read", "notify"],
      },
    });
  });

  it("says why a tab cannot run", () => {
    expect(kitTabState(ws, installed, "roblox", "explorer").kind).toBe("disabled");
    expect(kitTabState(ws, null, "hello-kit", "status").kind).toBe("loading");
    expect(kitTabState(ws, [], "hello-kit", "status").kind).toBe("missing");
    expect(kitTabState(ws, installed, "hello-kit", "gone").kind).toBe("missing");
    expect(kitTabState(ws, installed, "broken-kit", "status")).toEqual({
      kind: "broken",
      error: "its manifest is invalid",
    });
    expect(kitTabState(null, installed, "hello-kit", "status").kind).toBe("disabled");
  });
});
