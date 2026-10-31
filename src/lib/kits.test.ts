import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  KIT_COMMAND_PATTERN,
  KIT_EVENTS,
  kitArgvUsesKitDir,
  kitReadsHiddenFiles,
  KIT_LOOPBACK_PATTERN,
  KIT_METHODS,
  KIT_PERMISSIONS,
  isKitCommandName,
  isKitCommandsGrant,
  isKitHooksGrant,
  isKitPermission,
  effectiveKits,
  everywhereKits,
  kitCommandName,
  kitCommandsGrant,
  kitHooksGrant,
  kitIssueNotes,
  kitLoopbackPort,
  kitMayCall,
  kitPermissionsIn,
  loadPaneKits,
  paneKits,
  withEffectiveKits,
  workspaceKitIds,
  workspaceKits,
  type KitInfo,
} from "./kits";
import type { Settings, Workspace } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

type Kits = Pick<Workspace, "kits">;
type Everywhere = Pick<Settings, "kitsEverywhere">;

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
      workspaceKits(raw([{ id: "roblox", granted: ["notify", "files.delete", 3, "notify"] }])),
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
    expect(isKitPermission("files.write")).toBe(true);
    expect(isKitPermission("files.delete")).toBe(false);
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
        "files.delete",
        "panes.read",
        "notify",
      ]),
    ).toEqual(["panes.read", "notify", "loopback:8765", "loopback:9000"]);
  });

  it("names a command by the rule the manifest holds", () => {
    expect(kitCommandName("command:ci-status")).toBe("ci-status");
    expect(kitCommandName("command:r")).toBe("r");
    expect(kitCommandName("command:ci-2")).toBe("ci-2");
    for (const name of [
      "command:",
      "command:CI",
      "command:1ci",
      "command:-ci",
      "command:ci status",
      "command:ci_status",
      "command:ci.status",
      "command:ci-status ",
      "commands:0123456789abcdef",
      "ci-status",
      KIT_COMMAND_PATTERN,
      3,
      null,
      undefined,
    ]) {
      expect(kitCommandName(name), String(name)).toBeNull();
    }
    expect(isKitCommandName("ci-status")).toBe(true);
    expect(isKitCommandName("CI")).toBe(false);
    expect(isKitCommandName(3)).toBe(false);
  });

  it("knows a command's permission, and the grant of a kit's commands apart from it", () => {
    expect(isKitPermission("command:ci-status")).toBe(true);
    expect(isKitPermission("command:")).toBe(false);
    expect(isKitPermission("commands:0123456789abcdef")).toBe(false);
    expect(isKitCommandsGrant("commands:0123456789abcdef")).toBe(true);
    expect(isKitCommandsGrant("command:ci-status")).toBe(false);
    expect(isKitCommandsGrant("commands:0123")).toBe(false);
    expect(isKitCommandsGrant("commands:0123456789ABCDEF")).toBe(false);
    expect(isKitCommandsGrant("hooks:0123456789abcdef")).toBe(false);
    expect(isKitCommandsGrant(undefined)).toBe(false);
    expect(isKitHooksGrant("commands:0123456789abcdef")).toBe(false);
  });

  it("orders commands last, by name, and leaves the grant of a kit's commands out", () => {
    expect(
      kitPermissionsIn([
        "command:view",
        "loopback:8765",
        "commands:0123456789abcdef",
        "command:list",
        "notify",
        "command:Bad",
        "command:view",
      ]),
    ).toEqual(["notify", "loopback:8765", "command:list", "command:view"]);
  });
});

describe("a kit's commands, on a workspace", () => {
  const GRANT = "commands:0123456789abcdef";

  it("keeps what was accepted through the defensive reader, on a workspace and for all of them", () => {
    const entry = { id: "roblox", granted: ["command:view", GRANT, "command:Bad", "commands:nope"] };
    const kept = [{ id: "roblox", granted: ["command:view", GRANT] }];
    expect(workspaceKits({ kits: [entry] })).toEqual(kept);
    expect(everywhereKits({ kitsEverywhere: [entry] })).toEqual(kept);
    expect(effectiveKits({}, { kitsEverywhere: [entry] })).toEqual(kept);
    expect(withEffectiveKits({}, { kitsEverywhere: [entry] }).kits).toEqual(kept);
  });

  it("asks a GitHub kit that has commands, and no other", () => {
    const git = { kind: "git" as const, url: "https://github.com/o/kits" };
    const folder = { kind: "folder" as const, path: "D:\\k" };
    expect(kitCommandsGrant({ source: git, commandsPermission: GRANT })).toBe(GRANT);
    expect(kitCommandsGrant({ source: git, commandsPermission: null })).toBeNull();
    expect(kitCommandsGrant({ source: git })).toBeNull();
    expect(kitCommandsGrant({ source: git, commandsPermission: "nonsense" })).toBeNull();
    expect(kitCommandsGrant({ source: folder, commandsPermission: GRANT })).toBeNull();
  });

  it("does not hold a kit out of the panes for its commands: only its tabs wait", () => {
    const kit = installed({ id: "roblox", commandsPermission: GRANT });
    expect(paneKits({ kits: [{ id: "roblox", granted: [] }] }, [kit])).toEqual({ ids: ["roblox"], held: [] });
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

  it("leave out a GitHub kit accepted before the hash covered what else it starts, once", () => {
    // Accepted on 1.8.0, with its hooks' hash; it also has an MCP server, so its hash moved.
    const ws: Kits = { kits: [{ id: "studio", granted: ["panes.read", HOOKS] }] };
    const moved = installed({ id: "studio", hooksPermission: OTHER_HOOKS, hooksConsent: true });
    expect(paneKits(ws, [moved])).toEqual({ ids: [], held: ["studio"] });
    const accepted: Kits = { kits: [{ id: "studio", granted: ["panes.read", OTHER_HOOKS] }] };
    expect(paneKits(accepted, [moved])).toEqual({ ids: ["studio"], held: [] });
  });

  it("keep a GitHub kit that only has hooks: its hash did not move", () => {
    const ws: Kits = { kits: [{ id: "studio", granted: ["panes.read", HOOKS] }] };
    const same = installed({ id: "studio", hooksPermission: HOOKS, hooksConsent: true });
    expect(paneKits(ws, [same])).toEqual({ ids: ["studio"], held: [] });
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

describe("the kits enabled on all workspaces", () => {
  const everywhere: Everywhere = {
    kitsEverywhere: [
      { id: "ci", granted: ["notify"] },
      { id: "studio", granted: ["panes.read"] },
    ],
  };
  const rawSettings = (kitsEverywhere: unknown): Everywhere => ({ kitsEverywhere }) as Everywhere;

  it("are added to a workspace's own, which come first", () => {
    const ws: Kits = { kits: [{ id: "roblox", granted: [] }] };
    expect(effectiveKits(ws, everywhere).map((kit) => kit.id)).toEqual(["roblox", "ci", "studio"]);
    expect(workspaceKits(ws).map((kit) => kit.id)).toEqual(["roblox"]);
    expect(effectiveKits({}, everywhere).map((kit) => kit.id)).toEqual(["ci", "studio"]);
    expect(effectiveKits(null, everywhere).map((kit) => kit.id)).toEqual(["ci", "studio"]);
  });

  it("lose to a workspace's own entry for the same kit, grants included", () => {
    const ws: Kits = { kits: [{ id: "ci", granted: [] }] };
    expect(effectiveKits(ws, everywhere)).toEqual([
      { id: "ci", granted: [] },
      { id: "studio", granted: ["panes.read"] },
    ]);
  });

  it("read as none when kitsEverywhere is malformed or absent", () => {
    for (const bad of [undefined, null, "ci", 3, { ci: true }, [null, 4, "ci", { id: "ci" }, { id: "X", granted: [] }, { id: "ci", granted: "notify" }]]) {
      expect(everywhereKits(rawSettings(bad))).toEqual([]);
    }
    expect(everywhereKits(null)).toEqual([]);
    expect(effectiveKits({ kits: [{ id: "roblox", granted: [] }] }, rawSettings("ci"))).toEqual([
      { id: "roblox", granted: [] },
    ]);
  });

  it("keep the good entries of a half-broken list, once each, without unknown permissions", () => {
    expect(
      everywhereKits(
        rawSettings([
          { id: "ci", granted: ["notify", "files.delete", "notify"] },
          { id: "ci", granted: ["panes.read"] },
          null,
        ]),
      ),
    ).toEqual([{ id: "ci", granted: ["notify"] }]);
  });

  it("are loaded by a pane of any workspace", () => {
    const kits = [installed({ id: "ci" }), installed({ id: "studio" })];
    for (const ws of [{}, { kits: [{ id: "roblox", granted: [] }] }] as Kits[]) {
      const ids = paneKits(ws, kits, everywhere).ids;
      expect(ids).toContain("ci");
      expect(ids).toContain("studio");
    }
    expect(paneKits({}, kits)).toEqual({ ids: [], held: [] });
    expect(workspaceKitIds({}, everywhere)).toEqual(["ci", "studio"]);
    expect(withEffectiveKits({ kits: undefined }, everywhere).kits?.map((kit) => kit.id)).toEqual(["ci", "studio"]);
  });

  it("hold back a kit whose hooks changed, on every workspace", () => {
    const settings: Everywhere = { kitsEverywhere: [{ id: "ci", granted: [HOOKS] }] };
    const same = installed({ id: "ci", hooksPermission: HOOKS, hooksConsent: true });
    const changed = installed({ id: "ci", hooksPermission: OTHER_HOOKS, hooksConsent: true });
    const empty = {};
    const own: Kits = { kits: [{ id: "roblox", granted: [] }] };
    for (const ws of [empty, own] as Kits[]) {
      expect(paneKits(ws, [same], settings).held).toEqual([]);
      expect(paneKits(ws, [same], settings).ids).toContain("ci");
      expect(paneKits(ws, [changed], settings)).toMatchObject({ held: ["ci"] });
      expect(paneKits(ws, [changed], settings).ids).not.toContain("ci");
    }
  });

  it("a workspace's own accepted hooks still let the kit through", () => {
    const settings: Everywhere = { kitsEverywhere: [{ id: "ci", granted: [] }] };
    const kit = installed({ id: "ci", hooksPermission: HOOKS, hooksConsent: true });
    expect(paneKits({}, [kit], settings)).toEqual({ ids: [], held: ["ci"] });
    expect(paneKits({ kits: [{ id: "ci", granted: [HOOKS] }] }, [kit], settings)).toEqual({ ids: ["ci"], held: [] });
  });

  it("are read from the kits on disk for a workspace with none of its own", async () => {
    const list = vi.mocked(invoke);
    list.mockReset();
    expect(await loadPaneKits({}, {})).toEqual({ ids: [], held: [] });
    expect(list).not.toHaveBeenCalled();
    list.mockResolvedValue([installed({ id: "ci", hooksPermission: HOOKS, hooksConsent: true })]);
    expect(await loadPaneKits({}, { kitsEverywhere: [{ id: "ci", granted: [HOOKS] }] })).toEqual({
      ids: ["ci"],
      held: [],
    });
    expect(await loadPaneKits(null, { kitsEverywhere: [{ id: "ci", granted: [] }] })).toEqual({
      ids: [],
      held: ["ci"],
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
      if (permission === KIT_LOOPBACK_PATTERN || permission === KIT_COMMAND_PATTERN) continue;
      expect(KIT_PERMISSIONS).toContain(permission);
    }
    expect(Object.keys(KIT_METHODS).sort()).toEqual([
      "command.run",
      "data.read",
      "data.write",
      "files.list",
      "files.read",
      "files.readBinary",
      "files.write",
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

  it("gives each file method its permission", () => {
    expect(kitMayCall("files.write", ["files.write"])).toBe(true);
    expect(kitMayCall("files.write", ["files.read"])).toBe(false);
    for (const method of ["files.read", "files.readBinary", "files.list"]) {
      expect(kitMayCall(method, ["files.read"]), method).toBe(true);
      expect(kitMayCall(method, ["files.write"]), method).toBe(false);
      expect(kitMayCall(method, []), method).toBe(false);
    }
  });

  it("reads hidden files only for a tab holding both file permissions", () => {
    expect(kitReadsHiddenFiles(["files.read", "files.write"])).toBe(true);
    expect(kitReadsHiddenFiles(["files.write", "notify", "files.read"])).toBe(true);
    expect(kitReadsHiddenFiles(["files.read"])).toBe(false);
    expect(kitReadsHiddenFiles(["files.write"])).toBe(false);
    expect(kitReadsHiddenFiles([])).toBe(false);
  });

  it("knows an argv that runs something of the kit's, as an argument and not as the program", () => {
    expect(kitArgvUsesKitDir(["node", "{kitDir}/tools/audit.js", "{id}"])).toBe(true);
    expect(kitArgvUsesKitDir(["node", "--root={kitDir}"])).toBe(true);
    expect(kitArgvUsesKitDir(["gh", "run", "{kitdir}", "{{kitDir}}x"])).toBe(true);
    expect(kitArgvUsesKitDir(["gh", "run", "{run}"])).toBe(false);
    expect(kitArgvUsesKitDir(["{kitDir}"])).toBe(false);
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

  it("lets a tab run only the command it was granted", () => {
    const call = (granted: string[], params?: unknown) => kitMayCall("command.run", granted, params);
    expect(call(["command:view"], { name: "view" })).toBe(true);
    expect(call(["command:view"], { name: "list" })).toBe(false);
    expect(call(["command:view"], { name: "view ", args: {} })).toBe(false);
    expect(call(["command:view"], { name: 3 })).toBe(false);
    expect(call(["command:view"])).toBe(false);
    expect(call(["command:view"], null)).toBe(false);
    expect(call([...KIT_PERMISSIONS], { name: "view" })).toBe(false);
    // Neither the pattern's own spelling nor the kit's grant is a permission.
    expect(call([KIT_COMMAND_PATTERN], { name: "view" })).toBe(false);
    expect(call(["commands:0123456789abcdef"], { name: "view" })).toBe(false);
  });
});
