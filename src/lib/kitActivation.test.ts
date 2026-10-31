import { describe, expect, it } from "vitest";
import { translate } from "../i18n";
import { KIT_PERMISSIONS, type KitInfo, type KitTab } from "./kits";
import {
  PERMISSION_LABEL,
  disableKit,
  enableKit,
  kitName,
  kitState,
  panesToRestart,
  permissionDiff,
  requestedPermissions,
  sameKits,
  sourceLine,
  updateOutcome,
} from "./kitActivation";

const tab = (permissions: KitTab["permissions"]): KitTab => ({
  id: "t",
  title: "T",
  entry: "tabs/t/index.html",
  permissions,
});

const kit = (version: string, tabs: KitTab[], error: string | null = null): KitInfo => ({
  id: "hello-kit",
  source: { kind: "folder", path: "D:\\kits\\hello-kit" },
  dir: "D:\\kits\\hello-kit",
  installedVersion: version,
  addedAt: 0,
  manifest: {
    schema: 1,
    id: "hello-kit",
    name: "Hello kit",
    version,
    description: "",
    tabs,
    setup: [],
  },
  error,
  update: null,
});

describe("permissions", () => {
  it("merges the tabs' requests once each, in the closed set's order", () => {
    const asked = requestedPermissions([
      tab(["notify", "panes.read"]),
      tab(["panes.read", "workspace.read"]),
    ]);
    expect(asked).toEqual(["workspace.read", "panes.read", "notify"]);
  });

  it("asks for nothing when no tab does", () => {
    expect(requestedPermissions([])).toEqual([]);
    expect(requestedPermissions([tab([])])).toEqual([]);
  });

  it("splits what is asked by what was accepted", () => {
    expect(permissionDiff(["panes.read", "panes.send"], ["panes.read", "notify"])).toEqual({
      kept: ["panes.read"],
      added: ["panes.send"],
    });
  });

  it("describes every permission in plain words, in both languages", () => {
    for (const permission of KIT_PERMISSIONS) {
      const key = PERMISSION_LABEL[permission];
      expect(translate("en", key)).not.toBe(key);
      expect(translate("fr", key)).not.toBe(key);
    }
    expect(translate("fr", PERMISSION_LABEL["panes.send"])).toBe(
      "envoyer des messages à vos agents",
    );
  });
});

describe("an update that adds permissions", () => {
  it("lists the added ones", () => {
    const before = kit("1.0.0", [tab(["panes.read"])]);
    const after = kit("1.1.0", [tab(["panes.read", "panes.send"])]);
    expect(updateOutcome(before, after)).toEqual({
      from: "1.0.0",
      to: "1.1.0",
      newPermissions: ["panes.send"],
    });
  });

  it("shows the workspace that accepted the old ones as needing a review", () => {
    const entry = { id: "hello-kit", granted: ["panes.read" as const] };
    expect(kitState(entry, ["panes.read"])).toBe("on");
    expect(kitState(entry, ["panes.read", "panes.send"])).toBe("review");
  });

  it("does not ask again for permissions the kit dropped", () => {
    const entry = { id: "hello-kit", granted: ["panes.read" as const, "notify" as const] };
    expect(kitState(entry, ["panes.read"])).toBe("on");
  });
});

describe("kitState", () => {
  it("is off without an entry, and broken when the kit cannot be used", () => {
    expect(kitState(undefined, [])).toBe("off");
    expect(kitState({ id: "hello-kit", granted: [] }, null)).toBe("broken");
  });
});

describe("enabling and disabling", () => {
  it("adds a kit with exactly the accepted permissions, and replaces a grant", () => {
    const one = enableKit([], "hello-kit", ["notify"]);
    expect(one).toEqual([{ id: "hello-kit", granted: ["notify"] }]);
    expect(enableKit(one, "hello-kit", ["notify", "panes.read"])).toEqual([
      { id: "hello-kit", granted: ["notify", "panes.read"] },
    ]);
  });

  it("removes only the named kit", () => {
    const kits = [
      { id: "a-kit", granted: [] },
      { id: "b-kit", granted: [] },
    ];
    expect(disableKit(kits, "a-kit")).toEqual([{ id: "b-kit", granted: [] }]);
  });

  it("compares two lists by content, not by order", () => {
    expect(
      sameKits(
        [{ id: "a-kit", granted: ["notify", "panes.read"] }],
        [{ id: "a-kit", granted: ["panes.read", "notify"] }],
      ),
    ).toBe(true);
    expect(sameKits([{ id: "a-kit", granted: [] }], [])).toBe(false);
  });
});

describe("restarting", () => {
  it("restarts the Claude Code panes only", () => {
    const pane = (id: string, agent: "claude" | "shell" | "codex") => ({
      id,
      name: id,
      agent,
      sessionId: null,
      cwd: null,
      shellId: null,
      title: null,
    });
    expect(
      panesToRestart({
        panes: [pane("a", "claude"), pane("b", "shell"), pane("c", "codex"), pane("d", "claude")],
      }),
    ).toEqual(["a", "d"]);
  });
});

describe("labels", () => {
  it("names a kit by its manifest, or by its id when it is unreadable", () => {
    expect(kitName(kit("1.0.0", []))).toBe("Hello kit");
    expect(kitName({ id: "broken-kit", manifest: null })).toBe("broken-kit");
  });

  it("writes a source in one line", () => {
    expect(sourceLine({ kind: "folder", path: "D:\\k" })).toEqual({ kind: "folder", text: "D:\\k" });
    expect(sourceLine({ kind: "git", url: "https://github.com/o/r" }).text).toBe(
      "https://github.com/o/r",
    );
    expect(
      sourceLine({ kind: "git", url: "https://github.com/o/r", ref: "main", subdir: "roblox" }).text,
    ).toBe("https://github.com/o/r (main · roblox)");
  });
});
