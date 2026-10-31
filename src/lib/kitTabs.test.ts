import { describe, expect, it } from "vitest";
import { withEffectiveKits, type KitInfo, type KitManifest } from "./kits";
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

  it("keeps a local server's port as declared and granted, that very port", () => {
    expect(effectivePermissions(["loopback:8765", "data.read"], ["loopback:8765", "data.read"])).toEqual([
      "data.read",
      "loopback:8765",
    ]);
    expect(effectivePermissions(["loopback:8765"], ["loopback:9000"])).toEqual([]);
    expect(effectivePermissions(["loopback:9000", "loopback:8765"], ["loopback:8765", "loopback:9000"])).toEqual([
      "loopback:8765",
      "loopback:9000",
    ]);
    expect(effectivePermissions(["loopback:80"], ["loopback:80"])).toEqual([]);
  });

  it("never hands a tab the pseudo permission of accepted hooks", () => {
    const hooks = "hooks:0123456789abcdef";
    expect(effectivePermissions([hooks, "notify"], [hooks, "notify"])).toEqual(["notify"]);
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
        commandsAccepted: null,
      },
      {
        kitId: "hello-kit",
        kitName: "hello-kit name",
        tab: { ...status, permissions: [...status.permissions] },
        permissions: ["panes.read"],
        commandsAccepted: null,
      },
    ]);
  });

  describe("a kit's commands", () => {
    const GRANT = "commands:0123456789abcdef";
    const CHANGED = "commands:fedcba9876543210";
    const ci = { id: "ci", title: "CI", entry: "tabs/ci.html", permissions: ["command:view" as const, "notify" as const] };
    const github = (commandsPermission: string) =>
      kit("ci-kit", [ci], { source: { kind: "git", url: "https://github.com/o/kits" }, commandsPermission });
    const on = (granted: string[]) => ({ kits: [{ id: "ci-kit", granted }] });
    const held = ["command:view", "notify"];

    it("holds what the tab declares once the workspace accepted these very commands", () => {
      const [choice] = workspaceKitTabs(on([...held, GRANT]), [github(GRANT)]);
      expect(choice.permissions).toEqual(["notify", "command:view"]);
      expect(choice.commandsAccepted).toBe(GRANT);
    });

    it("holds none of them when they changed, and keeps the rest of what it was granted", () => {
      const [choice] = workspaceKitTabs(on([...held, GRANT]), [github(CHANGED)]);
      expect(choice.permissions).toEqual(["notify"]);
      expect(choice.commandsAccepted).toBeNull();
    });

    it("holds none of them when they were never accepted", () => {
      const [choice] = workspaceKitTabs(on(held), [github(GRANT)]);
      expect(choice.permissions).toEqual(["notify"]);
    });

    it("still needs the tab's own permission once they are accepted", () => {
      const [choice] = workspaceKitTabs(on(["notify", GRANT]), [github(GRANT)]);
      expect(choice.permissions).toEqual(["notify"]);
      expect(choice.commandsAccepted).toBe(GRANT);
    });

    it("asks a linked folder nothing", () => {
      const folder = kit("ci-kit", [ci], { commandsPermission: CHANGED });
      const [choice] = workspaceKitTabs(on(held), [folder]);
      expect(choice.permissions).toEqual(["notify", "command:view"]);
      expect(choice.commandsAccepted).toBeNull();
    });

    it("reads what a kit enabled on all workspaces accepted, and a workspace's own entry first", () => {
      const settings = { kitsEverywhere: [{ id: "ci-kit", granted: [...held, GRANT] }] };
      const everywhere = workspaceKitTabs(withEffectiveKits({}, settings), [github(GRANT)]);
      expect(everywhere[0].permissions).toEqual(["notify", "command:view"]);
      expect(everywhere[0].commandsAccepted).toBe(GRANT);

      const changed = workspaceKitTabs(withEffectiveKits({}, settings), [github(CHANGED)]);
      expect(changed[0].permissions).toEqual(["notify"]);

      const own = workspaceKitTabs(withEffectiveKits(on(["notify"]), settings), [github(GRANT)]);
      expect(own[0].permissions).toEqual(["notify"]);
      expect(own[0].commandsAccepted).toBeNull();
    });

    it("is ready to open on any workspace once it is enabled on all of them", () => {
      const settings = { kitsEverywhere: [{ id: "ci-kit", granted: [...held, GRANT] }] };
      expect(kitTabState({}, [github(GRANT)], "ci-kit", "ci").kind).toBe("disabled");
      const state = kitTabState(withEffectiveKits({}, settings), [github(GRANT)], "ci-kit", "ci");
      expect(state.kind).toBe("ready");
      expect(state.kind === "ready" && state.choice.permissions).toEqual(["notify", "command:view"]);
    });
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
        commandsAccepted: null,
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
