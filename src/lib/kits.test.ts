import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  KIT_EVENTS,
  KIT_LOOPBACK_PATTERN,
  KIT_METHODS,
  KIT_PERMISSIONS,
  isKitHooksGrant,
  isKitPermission,
  kitHooksGrant,
  kitIssueNotes,
  kitLoopbackPort,
  kitMayCall,
  kitPermissionsIn,
  loadPaneKits,
  paneKits,
  workspaceKitIds,
  workspaceKits,
  type KitInfo,
} from "./kits";
import type { Workspace } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

type Kits = Pick<Workspace, "kits">;

/** state.json as someone may have edited it by hand. */
const raw = (kits: unknown): Kits => ({ kits }) as Kits;

describe("workspaceKits", () => {
  it("reads the kits a workspace enabled, with what they were granted", () => {
    expect(
      workspaceKits({
        kits: [
          { id: "roblox", granted: ["panes.read", "notify"] },
          { id: "unreal-5", granted: [] },
        ],
      }),
    ).toEqual([
      { id: "roblox", granted: ["panes.read", "notify"] },
      { id: "unreal-5", granted: [] },
    ]);
  });

  it("reads a workspace saved before kits, or without any, as none", () => {
    expect(workspaceKits({})).toEqual([]);
    expect(workspaceKits(null)).toEqual([]);
    expect(workspaceKits(undefined)).toEqual([]);
    expect(workspaceKits(raw({ roblox: true }))).toEqual([]);
    expect(workspaceKits(raw("roblox"))).toEqual([]);
  });

  it("takes anything malformed as no kit", () => {
    expect(
      workspaceKits(
        raw([
          null,
          "roblox",
          { id: "roblox" },
          { id: "Roblox", granted: [] },
          { id: "../escape", granted: [] },
          { id: "r", granted: [] },
          { id: 42, granted: [] },
          { id: "studio", granted: "panes.read" },
          { id: "ok", granted: [] },
          { id: "ok", granted: ["notify"] },
        ]),
      ),
    ).toEqual([{ id: "ok", granted: [] }]);
  });

  it("never grants a permission Hangar does not know", () => {
    expect(
      workspaceKits(raw([{ id: "roblox", granted: ["notify", "files.write", 3, "notify"] }])),
    ).toEqual([{ id: "roblox", granted: ["notify"] }]);
  });

  it("keeps a local server's permission and accepted hooks, and only well-formed ones", () => {
    expect(
      workspaceKits(
        raw([
          {
            id: "roblox",
            granted: [
              "data.read",
              "loopback:8765",
              "hooks:0123456789abcdef",
              "loopback:80",
              "loopback:abc",
              "hooks:short",
              "hooks:0123456789ABCDEF",
            ],
          },
        ]),
      ),
    ).toEqual([{ id: "roblox", granted: ["data.read", "loopback:8765", "hooks:0123456789abcdef"] }]);
  });
});

describe("permissions", () => {
  it("reads a local server's port as the manifest does", () => {
    expect(kitLoopbackPort("loopback:8765")).toBe(8765);
    expect(kitLoopbackPort("loopback:1024")).toBe(1024);
    expect(kitLoopbackPort("loopback:65535")).toBe(65535);
    for (const name of [
      "loopback:80",
      "loopback:1023",
      "loopback:0",
      "loopback:abc",
      "loopback:65536",
      "loopback:",
      "loopback:08765",
      "loopback:+8765",
      "loopback:8765 ",
      "Loopback:8765",
      "notify",
      8765,
      null,
    ]) {
      expect(kitLoopbackPort(name), String(name)).toBeNull();
    }
  });

  it("knows the closed set and the pattern, and hooks apart from both", () => {
    expect(isKitPermission("data.read")).toBe(true);
    expect(isKitPermission("data.write")).toBe(true);
    expect(isKitPermission("loopback:8765")).toBe(true);
    expect(isKitPermission("loopback:80")).toBe(false);
    expect(isKitPermission("files.write")).toBe(false);
    expect(isKitPermission("hooks:0123456789abcdef")).toBe(false);
    expect(isKitHooksGrant("hooks:0123456789abcdef")).toBe(true);
    expect(isKitHooksGrant("hooks:0123")).toBe(false);
    expect(isKitHooksGrant(undefined)).toBe(false);
  });

  it("orders them the closed set's way, then by port", () => {
    expect(
      kitPermissionsIn([
        "loopback:9000",
        "notify",
        "hooks:0123456789abcdef",
        "loopback:8765",
        "files.write",
        "panes.read",
        "notify",
      ]),
    ).toEqual(["panes.read", "notify", "loopback:8765", "loopback:9000"]);
  });
});

const installed = (patch: Partial<KitInfo> & Pick<KitInfo, "id">): KitInfo => ({
  source: { kind: "git", url: "https://github.com/o/kits" },
  dir: "C:/k",
  installedVersion: "1.0.0",
  addedAt: 0,
  manifest: null,
  error: null,
  update: null,
  ...patch,
});

const HOOKS = "hooks:0123456789abcdef";
const OTHER_HOOKS = "hooks:fedcba9876543210";

describe("the kits a pane starts with", () => {
  const roblox: Kits = { kits: [{ id: "roblox", granted: [] }] };
  const other: Kits = {};

  it("are its own workspace's, and no other's", () => {
    expect(workspaceKitIds(roblox)).toEqual(["roblox"]);
    expect(workspaceKitIds(other)).toEqual([]);
    expect(paneKits(roblox, [])).toEqual({ ids: ["roblox"], held: [] });
    expect(paneKits(other, [])).toEqual({ ids: [], held: [] });
  });

  it("leave out a GitHub kit whose hooks changed since they were accepted", () => {
    const ws: Kits = { kits: [{ id: "studio", granted: ["panes.read", HOOKS] }] };
    const kit = (hooksPermission: string) => installed({ id: "studio", hooksPermission, hooksConsent: true });
    expect(paneKits(ws, [kit(HOOKS)])).toEqual({ ids: ["studio"], held: [] });
    expect(paneKits(ws, [kit(OTHER_HOOKS)])).toEqual({ ids: [], held: ["studio"] });
  });

  it("leave out a GitHub kit whose hooks were never accepted here", () => {
    const ws: Kits = { kits: [{ id: "studio", granted: ["panes.read"] }] };
    const kit = installed({ id: "studio", hooksPermission: HOOKS, hooksConsent: true });
    expect(paneKits(ws, [kit])).toEqual({ ids: [], held: ["studio"] });
  });

  it("keep a linked folder kit whatever its hooks: the person is writing it", () => {
    const ws: Kits = { kits: [{ id: "studio", granted: [] }] };
    const folder = installed({
      id: "studio",
      source: { kind: "folder", path: "D:/kits/studio" },
      hooksPermission: HOOKS,
      hooksConsent: true,
    });
    expect(kitHooksGrant(folder)).toBeNull();
    expect(paneKits(ws, [folder])).toEqual({ ids: ["studio"], held: [] });
  });

  it("keep a kit without hooks: it stores nothing, and behaves as before", () => {
    const ws: Kits = { kits: [{ id: "studio", granted: ["notify"] }] };
    const kit = installed({ id: "studio", hooksPermission: null, hooksConsent: true });
    expect(kitHooksGrant(kit)).toBeNull();
    expect(paneKits(ws, [kit])).toEqual({ ids: ["studio"], held: [] });
  });

  it("keep a GitHub kit installed before this was asked, until an update changes its hooks", () => {
    const ws: Kits = { kits: [{ id: "studio", granted: ["notify"] }] };
    const before = installed({ id: "studio", hooksPermission: HOOKS, hooksConsent: false });
    expect(paneKits(ws, [before])).toEqual({ ids: ["studio"], held: [] });
    // Nor does a list without these fields, from a backend that predates them.
    expect(paneKits(ws, [installed({ id: "studio" })])).toEqual({ ids: ["studio"], held: [] });
    const changed = { ...before, hooksPermission: OTHER_HOOKS, hooksConsent: true };
    expect(paneKits(ws, [changed])).toEqual({ ids: [], held: ["studio"] });
  });

  it("pass on a kit that is missing or broken, for Rust to say so", () => {
    const ws: Kits = {
      kits: [
        { id: "gone", granted: [] },
        { id: "bad", granted: [] },
      ],
    };
    const broken = installed({ id: "bad", error: "no manifest", hooksPermission: null, hooksConsent: true });
    expect(paneKits(ws, [broken])).toEqual({ ids: ["gone", "bad"], held: [] });
  });

  it("hold out one kit and keep the others, in their order", () => {
    const ws: Kits = {
      kits: [
        { id: "first", granted: [] },
        { id: "held", granted: [OTHER_HOOKS] },
        { id: "last", granted: [] },
      ],
    };
    const kits = [
      installed({ id: "first" }),
      installed({ id: "held", hooksPermission: HOOKS, hooksConsent: true }),
      installed({ id: "last", source: { kind: "folder", path: "D:/last" }, hooksPermission: HOOKS }),
    ];
    expect(paneKits(ws, kits)).toEqual({ ids: ["first", "last"], held: ["held"] });
  });

  describe("read from the kits on disk", () => {
    const list = vi.mocked(invoke);

    beforeEach(() => {
      list.mockReset();
    });

    it("reads nothing for a workspace without kits, or a pane that loads none", async () => {
      expect(await loadPaneKits({})).toEqual({ ids: [], held: [] });
      expect(await loadPaneKits(null)).toEqual({ ids: [], held: [] });
      expect(list).not.toHaveBeenCalled();
    });

    it("holds out what the kits say now", async () => {
      list.mockResolvedValue([installed({ id: "studio", hooksPermission: HOOKS, hooksConsent: true })]);
      expect(await loadPaneKits({ kits: [{ id: "studio", granted: [] }] })).toEqual({ ids: [], held: ["studio"] });
      expect(list).toHaveBeenCalledWith("kits_list");
    });

    it("hands the ids on when the kits cannot be read: Rust keeps them out and says why", async () => {
      list.mockRejectedValue("the kits index cannot be read");
      expect(await loadPaneKits({ kits: [{ id: "studio", granted: [] }] })).toEqual({ ids: ["studio"], held: [] });
      list.mockResolvedValue(undefined);
      expect(await loadPaneKits({ kits: [{ id: "studio", granted: [] }] })).toEqual({ ids: ["studio"], held: [] });
    });
  });
});

describe("kitIssueNotes", () => {
  const t = (key: string, params?: Record<string, string | number>) =>
    `${key}${params ? ` ${JSON.stringify(params)}` : ""}`;

  it("says which kit stayed out and why, one line each", () => {
    expect(
      kitIssueNotes(
        [
          { id: "gone", reason: "missing" },
          { id: "bad", reason: "broken", detail: "version: is missing" },
        ],
        t,
      ),
    ).toEqual([
      'kits.warnMissing {"id":"gone"}',
      'kits.warnBroken {"id":"bad","detail":"version: is missing"}',
    ]);
  });

  it("puts every kit a policy kept out on one line", () => {
    expect(
      kitIssueNotes(
        [
          { id: "roblox", reason: "forbidden" },
          { id: "unreal", reason: "forbidden" },
        ],
        t,
      ),
    ).toEqual(['kits.warnForbidden {"ids":"roblox, unreal"}']);
    expect(kitIssueNotes([], t)).toEqual([]);
  });

  it("says a kit stayed out for its hooks", () => {
    expect(kitIssueNotes([{ id: "studio", reason: "hooks" }], t)).toEqual(['kitsManage.warnHooks {"id":"studio"}']);
  });
});

describe("the tab protocol", () => {
  it("gates every method and event behind a known permission", () => {
    for (const permission of [...Object.values(KIT_METHODS), ...Object.values(KIT_EVENTS)]) {
      if (permission === KIT_LOOPBACK_PATTERN) continue;
      expect(KIT_PERMISSIONS).toContain(permission);
    }
    expect(Object.keys(KIT_METHODS).sort()).toEqual([
      "data.read",
      "data.write",
      "files.read",
      "loopback.fetch",
      "notify",
      "panes.list",
      "panes.send",
      "preview.show",
      "workspace.info",
    ]);
  });

  it("lets a tab call only what it was granted", () => {
    expect(kitMayCall("panes.list", ["panes.read"])).toBe(true);
    expect(kitMayCall("panes.send", ["panes.read"])).toBe(false);
    expect(kitMayCall("files.read", [])).toBe(false);
    expect(kitMayCall("data.read", ["data.read"])).toBe(true);
    expect(kitMayCall("data.write", ["data.read"])).toBe(false);
    expect(kitMayCall("toString", ["workspace.read"])).toBe(false);
    expect(kitMayCall("shell.run", [...KIT_PERMISSIONS])).toBe(false);
  });

  it("lets a tab reach a local server only on the port it was granted", () => {
    const call = (granted: string[], params?: unknown) => kitMayCall("loopback.fetch", granted, params);
    expect(call(["loopback:8765"], { port: 8765 })).toBe(true);
    expect(call(["loopback:8765"], { port: 8766 })).toBe(false);
    expect(call(["loopback:8765"], { port: "8765" })).toBe(false);
    expect(call(["loopback:8765"])).toBe(false);
    expect(call(["loopback:8765"], null)).toBe(false);
    expect(call([...KIT_PERMISSIONS], { port: 8765 })).toBe(false);
    // The pattern's own spelling is no permission.
    expect(call([KIT_LOOPBACK_PATTERN], { port: 8765 })).toBe(false);
    expect(call(["loopback:80"], { port: 80 })).toBe(false);
  });
});
