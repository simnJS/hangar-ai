import { describe, expect, it } from "vitest";
import {
  KIT_EVENTS,
  KIT_METHODS,
  KIT_PERMISSIONS,
  kitIssueNotes,
  kitMayCall,
  workspaceKitIds,
  workspaceKits,
} from "./kits";
import type { Workspace } from "../types";

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
});

describe("the kits a pane starts with", () => {
  const roblox: Kits = { kits: [{ id: "roblox", granted: [] }] };
  const other: Kits = {};

  it("are its own workspace's, and no other's", () => {
    expect(workspaceKitIds(roblox)).toEqual(["roblox"]);
    expect(workspaceKitIds(other)).toEqual([]);
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
});

describe("the tab protocol", () => {
  it("gates every method and event behind a known permission", () => {
    for (const permission of [...Object.values(KIT_METHODS), ...Object.values(KIT_EVENTS)]) {
      expect(KIT_PERMISSIONS).toContain(permission);
    }
    expect(Object.keys(KIT_METHODS).sort()).toEqual(
      ["files.read", "notify", "panes.list", "panes.send", "preview.show", "workspace.info"],
    );
  });

  it("lets a tab call only what it was granted", () => {
    expect(kitMayCall("panes.list", ["panes.read"])).toBe(true);
    expect(kitMayCall("panes.send", ["panes.read"])).toBe(false);
    expect(kitMayCall("files.read", [])).toBe(false);
    expect(kitMayCall("toString", ["workspace.read"])).toBe(false);
    expect(kitMayCall("shell.run", [...KIT_PERMISSIONS])).toBe(false);
  });
});
