import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { translate } from "../i18n";
import { KIT_PERMISSIONS, type KitInfo, type KitInspection, type KitTab } from "./kits";
import {
  PERMISSION_LABEL,
  acceptedGrants,
  disableKit,
  enableKit,
  hooksToAccept,
  kitName,
  kitRequested,
  kitState,
  panesToRestart,
  permissionDiff,
  permissionLabel,
  requestedPermissions,
  sameKits,
  sourceLine,
  updateOutcome,
} from "./kitActivation";

// The view is drawn from props; the store and Tauri are not needed.
vi.mock("../store", () => ({ useStore: () => ({}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

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

  it("puts a local server's permissions after the closed set, by port", () => {
    expect(requestedPermissions([tab(["loopback:9000", "notify"]), tab(["loopback:8765", "data.write"])])).toEqual([
      "data.write",
      "notify",
      "loopback:8765",
      "loopback:9000",
    ]);
  });

  it("says what a local server's permission means, with its port, in both languages", () => {
    const say = (locale: "en" | "fr") => (key: Parameters<typeof translate>[1], params?: Parameters<typeof translate>[2]) =>
      translate(locale, key, params);
    expect(permissionLabel(say("en"), "loopback:8765")).toBe(
      "talk to the local server on port 8765, and make it do whatever that server allows",
    );
    expect(permissionLabel(say("fr"), "loopback:8765")).toBe(
      "parler au serveur local sur le port 8765, et lui faire faire tout ce que ce serveur permet",
    );
    expect(permissionLabel(say("en"), "panes.send")).toBe("send messages to your agents");
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

const HOOKS = "hooks:0123456789abcdef";
const OTHER_HOOKS = "hooks:fedcba9876543210";

const gitKit = (hooksPermission: string | null, hooksConsent = true): KitInfo => ({
  ...kit("1.0.0", [tab(["panes.read"])]),
  source: { kind: "git", url: "https://github.com/o/kits" },
  hooksPermission,
  hooksConsent,
});

describe("hooks, in the permission diff", () => {
  it("asks for a GitHub kit's hooks beside its tabs' permissions", () => {
    expect(kitRequested(gitKit(HOOKS))).toEqual(["panes.read", HOOKS]);
  });

  it("asks nothing for a kit without hooks, a linked folder, or a kit from before", () => {
    expect(kitRequested(gitKit(null))).toEqual(["panes.read"]);
    expect(kitRequested(gitKit(HOOKS, false))).toEqual(["panes.read"]);
    const folder: KitInfo = { ...gitKit(HOOKS), source: { kind: "folder", path: "D:\\k" } };
    expect(kitRequested(folder)).toEqual(["panes.read"]);
    expect(kitRequested(kit("1.0.0", [tab(["panes.read"])]))).toEqual(["panes.read"]);
  });

  it("splits hooks like any permission: the very ones accepted are kept, others are added", () => {
    const requested = ["panes.read" as const, HOOKS as `hooks:${string}`];
    expect(permissionDiff(requested, ["panes.read", HOOKS])).toEqual({
      kept: ["panes.read", HOOKS],
      added: [],
    });
    const changed = permissionDiff(requested, ["panes.read", OTHER_HOOKS]);
    expect(changed).toEqual({ kept: ["panes.read"], added: [HOOKS] });
    expect(hooksToAccept(changed)).toBe(true);
    expect(hooksToAccept(permissionDiff(["panes.read", "notify"], ["panes.read"]))).toBe(false);
  });

  it("shows a workspace that accepted other hooks, or none, as to review", () => {
    const entry = (granted: string[]) => ({ id: "hello-kit", granted });
    const requested = kitRequested(gitKit(HOOKS));
    expect(kitState(entry(["panes.read", HOOKS]), requested)).toBe("on");
    expect(kitState(entry(["panes.read", OTHER_HOOKS]), requested)).toBe("review");
    expect(kitState(entry(["panes.read"]), requested)).toBe("review");
  });

  it("leaves a workspace as it was when the kit has no hooks", () => {
    const entry = { id: "hello-kit", granted: ["panes.read"] };
    expect(kitState(entry, kitRequested(gitKit(null)))).toBe("on");
    // Hooks gone since they were accepted: nothing is lost by it.
    expect(kitState({ ...entry, granted: ["panes.read", OTHER_HOOKS] }, kitRequested(gitKit(null)))).toBe("on");
  });

  it("stores what the person was shown: a GitHub kit's hash, a linked folder's nothing, no hooks nothing", () => {
    const shown: Pick<KitInspection, "tabs" | "hooksPermission"> = {
      tabs: [tab(["panes.read", "loopback:8765"])],
      hooksPermission: HOOKS,
    };
    expect(acceptedGrants(gitKit(HOOKS), shown)).toEqual(["panes.read", "loopback:8765", HOOKS]);
    // The kit changed under the open view: what was accepted is what was seen.
    expect(acceptedGrants(gitKit(OTHER_HOOKS), shown)).toEqual(["panes.read", "loopback:8765", HOOKS]);
    expect(acceptedGrants({ source: { kind: "folder", path: "D:\\k" } }, shown)).toEqual([
      "panes.read",
      "loopback:8765",
    ]);
    expect(acceptedGrants(gitKit(null), { ...shown, hooksPermission: null })).toEqual([
      "panes.read",
      "loopback:8765",
    ]);
    expect(acceptedGrants(gitKit(null), null)).toEqual([]);
  });

  it("stays held after accepting hooks the kit has since changed", () => {
    const seen = acceptedGrants(gitKit(HOOKS), { tabs: [tab(["panes.read"])], hooksPermission: HOOKS });
    const stored = enableKit([], "hello-kit", seen);
    expect(stored).toEqual([{ id: "hello-kit", granted: ["panes.read", HOOKS] }]);
    expect(kitState(stored[0], kitRequested(gitKit(OTHER_HOOKS)))).toBe("review");
  });

  it("tells an update that changes the hooks apart from one that does not", () => {
    expect(updateOutcome(gitKit(HOOKS), gitKit(HOOKS)).newPermissions).toEqual([]);
    expect(updateOutcome(gitKit(HOOKS), gitKit(OTHER_HOOKS)).newPermissions).toEqual([OTHER_HOOKS]);
    expect(updateOutcome(gitKit(null), gitKit(HOOKS)).newPermissions).toEqual([HOOKS]);
    expect(updateOutcome(gitKit(HOOKS), gitKit(null)).newPermissions).toEqual([]);
  });
});

describe("the activation view", () => {
  const load = () => import("../components/KitActivation");
  const hooks = [
    { event: "PostToolUse", matcher: "Write|Edit", command: 'echo "edited" >> "$HANGAR_KITS_DATA/hello-kit/log"' },
    { event: "SessionStart", matcher: "", command: "node ${CLAUDE_PLUGIN_ROOT}/start.js" },
  ];
  const inspection: KitInspection = {
    skills: [],
    commands: [],
    agents: [],
    mcpServers: [],
    hooks,
    hooksPermission: HOOKS,
    tabs: [tab(["loopback:8765", "data.read"])],
    setup: [],
  };

  async function draw(locale: "en" | "fr", info: KitInfo, patch: { entry?: { id: string; granted: string[] }; shown?: KitInspection } = {}) {
    const { KitActivationView } = await load();
    const noop = () => undefined;
    return renderToStaticMarkup(
      createElement(KitActivationView, {
        t: (key, params) => translate(locale, key, params),
        kit: info,
        inspection: patch.shown ?? inspection,
        steps: [],
        entry: patch.entry,
        workspaceName: "Studio",
        canRun: true,
        os: "windows",
        running: null,
        ran: null,
        error: null,
        onAccept: noop,
        onClose: noop,
        onRun: noop,
        onConfirm: noop,
        onNotYet: noop,
      }),
    );
  }

  it("shows the hooks verbatim, in the setup command's block, and says they run by themselves", async () => {
    const out = await draw("en", gitKit(HOOKS));
    expect(out).toContain("Claude Code runs these by itself, without asking, each time they match.");
    expect(out).toContain("PostToolUse · Write|Edit");
    expect(out).toContain("SessionStart");
    // React escapes the markup, nothing else: the text is the command, whole.
    expect(out).toContain("echo &quot;edited&quot; &gt;&gt; &quot;$HANGAR_KITS_DATA/hello-kit/log&quot;</pre>");
    expect(out).toContain(">node ${CLAUDE_PLUGIN_ROOT}/start.js</pre>");
    expect(out.match(/<pre class="kit-step__cmd"/g)).toHaveLength(2);
  });

  it("says it in French, with accents", async () => {
    const out = await draw("fr", gitKit(HOOKS));
    expect(out).toContain("Claude Code les lance de lui-même, sans demander, chaque fois qu&#x27;elles correspondent.");
    expect(out).toContain("parler au serveur local sur le port 8765");
  });

  it("says in plain words what a local server's permission lets the tab do", async () => {
    const out = await draw("en", gitKit(HOOKS));
    expect(out).toContain("talk to the local server on port 8765, and make it do whatever that server allows");
    expect(out).toContain("read the data this kit keeps in its own folder");
  });

  it("says when a plugin has no hook", async () => {
    const out = await draw("en", gitKit(null), { shown: { ...inspection, hooks: [], hooksPermission: null } });
    expect(out).toContain("No hook: nothing runs by itself.");
    expect(out).not.toContain("runs these by itself");
  });

  it("marks hooks that changed, and asks to accept them", async () => {
    const out = await draw("en", gitKit(OTHER_HOOKS), {
      entry: { id: "hello-kit", granted: ["loopback:8765", "data.read", "panes.read", HOOKS] },
    });
    expect(out).toContain("this kit stays out of the workspace&#x27;s new panes");
    expect(out).toContain("Accept what changed");
    expect(out).toContain("kit-act__tag--new");
  });

  it("closes without asking once these very hooks are accepted", async () => {
    const out = await draw("en", gitKit(HOOKS), {
      entry: { id: "hello-kit", granted: ["panes.read", HOOKS] },
    });
    expect(out).toContain(">Close<");
    expect(out).toContain("Already accepted");
    expect(out).not.toContain("stays out");
  });

  it("does not ask about a linked folder's hooks, and says they apply as they are edited", async () => {
    const folder: KitInfo = { ...gitKit(OTHER_HOOKS), source: { kind: "folder", path: "D:\\k" } };
    const out = await draw("en", folder, { entry: { id: "hello-kit", granted: ["panes.read"] } });
    expect(out).toContain("A linked folder is read live");
    expect(out).toContain(">Close<");
    expect(out).not.toContain("Accept what changed");
    expect(out).not.toContain("stays out");
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
