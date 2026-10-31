import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { translate } from "../i18n";
import {
  KIT_PERMISSIONS,
  kitReadsHiddenFiles,
  type KitDeclaredCommand,
  type KitInfo,
  type KitInspection,
  type KitRunning,
  type KitTab,
} from "./kits";
import { workspaceKitTabs } from "./kitTabs";
import {
  PERMISSION_LABEL,
  RUNNING_LABEL,
  acceptedGrants,
  commandsToAccept,
  disableKit,
  enableKit,
  hooksToAccept,
  kitArgvLine,
  kitName,
  kitPlainHooks,
  kitRequested,
  kitRuleText,
  kitRunningGroups,
  kitRunningLine,
  kitState,
  onlyCommandsToAccept,
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

  it("says what changing files means, hidden ones named, in both languages", () => {
    const say = (locale: "en" | "fr") => (key: Parameters<typeof translate>[1], params?: Parameters<typeof translate>[2]) =>
      translate(locale, key, params);
    expect(permissionLabel(say("en"), "files.write")).toBe(
      "change text files in your project, hidden ones like .env included",
    );
    expect(permissionLabel(say("fr"), "files.write")).toBe(
      "modifier les fichiers texte de votre projet, y compris les fichiers cachés comme .env",
    );
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

describe("an update that adds files.write", () => {
  const entry = { id: "hello-kit", granted: ["files.read", "notify"] };

  it("reads « to review » until it is accepted", () => {
    expect(kitState(entry, kitRequested(kit("1.0.0", [tab(["files.read", "notify"])])))).toBe("on");
    const updated = kit("1.1.0", [tab(["files.read", "files.write", "notify"])]);
    expect(kitState(entry, kitRequested(updated))).toBe("review");
    expect(permissionDiff(kitRequested(updated)!, entry.granted).added).toEqual(["files.write"]);
    expect(kitState({ ...entry, granted: [...entry.granted, "files.write"] }, kitRequested(updated))).toBe("on");
  });

  it("gives the tab nothing until then: it stays out of what it may call", () => {
    const updated = kit("1.1.0", [tab(["files.read", "files.write", "notify"])]);
    const [choice] = workspaceKitTabs({ kits: [entry] }, [updated]);
    expect(choice.permissions).toEqual(["files.read", "notify"]);
    expect(kitReadsHiddenFiles(choice.permissions)).toBe(false);
    const [accepted] = workspaceKitTabs({ kits: [{ ...entry, granted: [...entry.granted, "files.write"] }] }, [updated]);
    expect(kitReadsHiddenFiles(accepted.permissions)).toBe(true);
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

const COMMANDS = "commands:0123456789abcdef";
const OTHER_COMMANDS = "commands:fedcba9876543210";

const commandKit = (commandsPermission: string | null, source: KitInfo["source"] = { kind: "git", url: "https://github.com/o/kits" }): KitInfo => ({
  ...kit("1.0.0", [tab(["command:ci-run"])]),
  source,
  commandsPermission,
});

const FOLDER: KitInfo["source"] = { kind: "folder", path: "D:\\k" };

describe("a kit's commands, in the permission diff", () => {
  it("says what a command's permission lets the tab do, by its name, in both languages", () => {
    const say = (locale: "en" | "fr") => (key: Parameters<typeof translate>[1], params?: Parameters<typeof translate>[2]) =>
      translate(locale, key, params);
    expect(permissionLabel(say("en"), "command:ci-run")).toBe(
      "run the command ci-run on your computer, with your logins",
    );
    expect(permissionLabel(say("fr"), "command:ci-run")).toBe(
      "lancer la commande ci-run sur votre ordinateur, avec vos identifiants",
    );
  });

  it("merges the tabs' commands once each, after everything else", () => {
    expect(requestedPermissions([tab(["command:b", "notify"]), tab(["command:a", "loopback:8765", "command:b"])])).toEqual([
      "notify",
      "loopback:8765",
      "command:a",
      "command:b",
    ]);
  });

  it("asks for a GitHub kit's commands beside its tabs' permissions", () => {
    expect(kitRequested(commandKit(COMMANDS))).toEqual(["command:ci-run", COMMANDS]);
  });

  it("asks nothing for a kit without commands, or a linked folder", () => {
    expect(kitRequested(commandKit(null))).toEqual(["command:ci-run"]);
    expect(kitRequested(commandKit(COMMANDS, FOLDER))).toEqual(["command:ci-run"]);
  });

  it("splits them like any permission: the very ones accepted are kept, others are added", () => {
    const requested = ["command:ci-run" as const, COMMANDS as `commands:${string}`];
    expect(permissionDiff(requested, ["command:ci-run", COMMANDS])).toEqual({ kept: requested, added: [] });
    const changed = permissionDiff(requested, ["command:ci-run", OTHER_COMMANDS]);
    expect(changed).toEqual({ kept: ["command:ci-run"], added: [COMMANDS] });
    expect(commandsToAccept(changed)).toBe(true);
    expect(hooksToAccept(changed)).toBe(false);
    expect(commandsToAccept(permissionDiff(["notify"], []))).toBe(false);
    expect(commandsToAccept(permissionDiff([HOOKS as `hooks:${string}`], []))).toBe(false);
  });

  it("tells a kit waiting for its commands alone from one waiting for more", () => {
    const commands = COMMANDS as `commands:${string}`;
    expect(onlyCommandsToAccept(permissionDiff([commands], []))).toBe(true);
    expect(onlyCommandsToAccept(permissionDiff(["notify", commands], ["notify"]))).toBe(true);
    expect(onlyCommandsToAccept(permissionDiff(["notify", commands], []))).toBe(false);
    expect(onlyCommandsToAccept(permissionDiff([HOOKS as `hooks:${string}`, commands], []))).toBe(false);
    expect(onlyCommandsToAccept(permissionDiff(["notify"], ["notify"]))).toBe(false);
  });

  it("shows a workspace that accepted other commands, or none, as to review", () => {
    const entry = (granted: string[]) => ({ id: "hello-kit", granted });
    const requested = kitRequested(commandKit(COMMANDS));
    expect(kitState(entry(["command:ci-run", COMMANDS]), requested)).toBe("on");
    expect(kitState(entry(["command:ci-run", OTHER_COMMANDS]), requested)).toBe("review");
    expect(kitState(entry(["command:ci-run"]), requested)).toBe("review");
    // A linked folder is on whatever its commands became.
    expect(kitState(entry(["command:ci-run"]), kitRequested(commandKit(COMMANDS, FOLDER)))).toBe("on");
  });

  it("stores what the person was shown: a GitHub kit's hash, a linked folder's nothing, no commands nothing", () => {
    const shown: Pick<KitInspection, "tabs" | "hooksPermission" | "commandsPermission"> = {
      tabs: [tab(["command:ci-run"])],
      hooksPermission: HOOKS,
      commandsPermission: COMMANDS,
    };
    expect(acceptedGrants(commandKit(COMMANDS), shown)).toEqual(["command:ci-run", HOOKS, COMMANDS]);
    // The kit changed under the open view: what was accepted is what was seen.
    expect(acceptedGrants(commandKit(OTHER_COMMANDS), shown)).toEqual(["command:ci-run", HOOKS, COMMANDS]);
    expect(acceptedGrants(commandKit(COMMANDS, FOLDER), shown)).toEqual(["command:ci-run"]);
    expect(acceptedGrants(commandKit(null), { ...shown, hooksPermission: null, commandsPermission: null })).toEqual([
      "command:ci-run",
    ]);
  });

  it("stays to review after accepting commands the kit has since changed", () => {
    const seen = acceptedGrants(commandKit(COMMANDS), { tabs: [tab(["command:ci-run"])], commandsPermission: COMMANDS });
    const stored = enableKit([], "hello-kit", seen);
    expect(stored).toEqual([{ id: "hello-kit", granted: ["command:ci-run", COMMANDS] }]);
    expect(kitState(stored[0], kitRequested(commandKit(OTHER_COMMANDS)))).toBe("review");
  });

  it("tells an update that changes the commands apart from one that does not", () => {
    expect(updateOutcome(commandKit(COMMANDS), commandKit(COMMANDS)).newPermissions).toEqual([]);
    expect(updateOutcome(commandKit(COMMANDS), commandKit(OTHER_COMMANDS)).newPermissions).toEqual([OTHER_COMMANDS]);
    expect(updateOutcome(commandKit(null), commandKit(COMMANDS)).newPermissions).toEqual([COMMANDS]);
  });
});

describe("an argv, as the person reads it", () => {
  it("is written as it is, placeholders included", () => {
    expect(kitArgvLine(["gh", "run", "view", "{run}", "--repo={repo}", "--json", "status"])).toBe(
      "gh run view {run} --repo={repo} --json status",
    );
    expect(kitArgvLine(["docker", "ps", "--format", "{{.Names}}", "C:\\tools"])).toBe(
      "docker ps --format {{.Names}} C:\\tools",
    );
  });

  it("does not let one item read as two, or two as one", () => {
    expect(kitArgvLine(["gh", "run view"])).toBe('gh "run view"');
    expect(kitArgvLine(["gh", "", "x"])).toBe('gh "" x');
    expect(kitArgvLine(["gh", 'a"b'])).toBe('gh "a\\"b"');
    expect(kitArgvLine(["gh", "it's"])).toBe('gh "it\'s"');
    expect(kitArgvLine(["gh", "a\nb"])).toBe('gh "a\\nb"');
  });

  it("writes out what would not show, or would turn the line around", () => {
    expect(kitArgvLine(["gh", "safe\u202Emr -"])).toBe('gh "safe\\u{202e}mr -"');
    expect(kitArgvLine(["gh\u200B", "x"])).toBe('"gh\\u{200b}" x');
    expect(kitRuleText("[a-z]+\u202E")).toBe("[a-z]+\\u{202e}");
    expect(kitRuleText("^[\\w.-]+/[\\w.-]+$")).toBe("^[\\w.-]+/[\\w.-]+$");
  });
});

/** French typesetting puts no-break spaces before `;` and `:`; the strings are written with plain ones. */
const plainSpaces = (text: string) => text.replace(/[  ]/g, " ");

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

  async function draw(
    locale: "en" | "fr",
    info: KitInfo,
    patch: { entry?: { id: string; granted: string[] }; shown?: KitInspection; everywhere?: boolean } = {},
  ) {
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
        everywhere: patch.everywhere,
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

  describe("files.write", () => {
    const shown = (tabs: KitTab[]): KitInspection => ({ ...inspection, hooks: [], tabs });
    const SECRETS = "so it can also read hidden files such as .env, and the secrets they hold";

    it("says in plain words that a tab can change the project's files", async () => {
      const out = await draw("en", kit("1.0.0", []), { shown: shown([tab(["files.write"])]) });
      expect(out).toContain("change text files in your project, hidden ones like .env included");
      expect(out).not.toContain(SECRETS);
    });

    it("names .env and secrets for a tab that holds both, with what could carry them off", async () => {
      const both = shown([tab(["files.read", "files.write"])]);
      const en = await draw("en", kit("1.0.0", []), { shown: both });
      expect(en).toContain(SECRETS);
      expect(en).toContain("panes.send");
      const fr = plainSpaces(await draw("fr", kit("1.0.0", []), { shown: both }));
      expect(fr).toContain("les fichiers cachés comme .env, et les secrets qu&#x27;ils contiennent");
      expect(fr).toContain("panes.send");
    });

    it("says nothing of secrets for a tab that only reads, or across two tabs", async () => {
      const reads = await draw("en", kit("1.0.0", []), { shown: shown([tab(["files.read"])]) });
      expect(reads).not.toContain(SECRETS);
      const apart = await draw("en", kit("1.0.0", []), {
        shown: shown([tab(["files.read"]), { ...tab(["files.write"]), id: "other" }]),
      });
      expect(apart).not.toContain(SECRETS);
    });

    it("shows what an update added as new, and what asks for it to be accepted", async () => {
      const updated = kit("1.1.0", [tab(["files.read", "files.write"])]);
      const out = await draw("en", updated, {
        shown: shown([tab(["files.read", "files.write"])]),
        entry: { id: "hello-kit", granted: ["files.read"] },
      });
      expect(out).toContain("kit-act__tag--new");
      expect(out).toContain(SECRETS);
      expect(out).toContain("btn--primary");
    });
  });

  describe("{kitDir}", () => {
    const audit: KitDeclaredCommand = {
      name: "audit",
      description: "Runs the kit's audit.",
      argv: ["node", "{kitDir}/tools/audit.js", "{id}"],
      args: { id: { pattern: "[0-9]+" } },
      timeoutSeconds: 30,
    };
    const shown = (commands: KitDeclaredCommand[]): KitInspection => ({
      ...inspection,
      hooks: [],
      declaredCommands: commands,
    });

    it("is shown as written, with no rule of its own, and said to be the kit's folder", async () => {
      const out = await draw("en", kit("1.0.0", []), { shown: shown([audit]) });
      expect(out).toContain(">node {kitDir}/tools/audit.js {id}</pre>");
      expect(out).toContain("<code>{id}</code>");
      expect(out).not.toContain("<code>{kitDir}</code>");
      expect(out).toContain("{kitDir} is this kit&#x27;s own folder");
      const fr = plainSpaces(await draw("fr", kit("1.0.0", []), { shown: shown([audit]) }));
      expect(fr).toContain("{kitDir} est le dossier de ce kit");
    });

    it("is not spoken of for a command that does not use it", async () => {
      const gh = { ...audit, argv: ["gh", "run", "view", "{id}"] };
      const out = await draw("en", kit("1.0.0", []), { shown: shown([gh]) });
      expect(out).not.toContain("{kitDir}");
    });
  });

  it("says it in French, with accents", async () => {
    const out = await draw("fr", gitKit(HOOKS));
    expect(out).toContain("Claude Code les lance de lui-même, sans demander, chaque fois qu&#x27;ils correspondent.");
    expect(out).toContain("parler au serveur local sur le port 8765");
  });

  it("says in plain words what a local server's permission lets the tab do", async () => {
    const out = await draw("en", gitKit(HOOKS));
    expect(out).toContain("talk to the local server on port 8765, and make it do whatever that server allows");
    expect(out).toContain("read the data this kit keeps in its own folder");
  });

  it("says when a plugin runs nothing by itself", async () => {
    const out = await draw("en", gitKit(null), { shown: { ...inspection, hooks: [], hooksPermission: null } });
    expect(out).toContain("Nothing runs by itself.");
    expect(out).not.toContain("runs these by itself");
  });

  describe("what else a plugin starts by itself", () => {
    const running: KitRunning[] = [
      {
        kind: "mcp",
        name: "local",
        argv: ["node", "${CLAUDE_PLUGIN_ROOT}/server.js", "4 4"],
        env: ["API_TOKEN", "ROOT"],
      },
      { kind: "mcp", name: "remote", text: "https://api.example.com/mcp", headers: ["Authorization"] },
      { kind: "mcp", name: "remote · headersHelper", text: "node ./headers.js" },
      { kind: "mcp", name: "./packed.mcpb", text: "./packed.mcpb", unreadable: true },
      {
        kind: "monitor",
        name: "deploy",
        text: "./poll.sh",
        when: "on-skill-invoke:deploy",
        description: "Deploys",
      },
      { kind: "lsp", name: "gopls", argv: ["gopls", "serve"], when: ".go, .mod" },
      { kind: "skillHook", name: "skills/guarded/SKILL.md", text: "hooks:\n  Stop: check.sh" },
      { kind: "settings", name: "subagentStatusLine", text: "./line.sh" },
      { kind: "grammar", name: "gdscript", text: "npm:hljs-gdscript@1.0.0", unreadable: true },
      { kind: "bin", name: "tool", text: "bin/tool" },
      { kind: "binary", name: "fetched", text: "sha256:abcd", unreadable: true },
      { kind: "dependency", name: "audit-logger", text: "audit-logger", unreadable: true },
      { kind: "unknown", name: "plugin.json › startup", text: "node ./boot.js" },
      { kind: "unknown", name: ".lsp.json", unreadable: true },
    ];
    const shown: KitInspection = {
      ...inspection,
      hooks: [...hooks, { event: "modules", matcher: "", command: "./register.js" }],
      running,
    };

    it("shows each one as written, in the setup command's block", async () => {
      const out = await draw("en", gitKit(HOOKS), { shown });
      expect(out).toContain('>node ${CLAUDE_PLUGIN_ROOT}/server.js &quot;4 4&quot;</pre>');
      expect(out).toContain(">https://api.example.com/mcp</pre>");
      expect(out).toContain(">node ./headers.js</pre>");
      expect(out).toContain(">./poll.sh</pre>");
      expect(out).toContain("deploy · on-skill-invoke:deploy");
      expect(out).toContain("Deploys");
      expect(out).toContain(">gopls serve</pre>");
      expect(out).toContain("gopls · .go, .mod");
      expect(out).toContain(">hooks:\n  Stop: check.sh</pre>");
      expect(out).toContain(">./line.sh</pre>");
      expect(out).toContain(">bin/tool</pre>");
      expect(out).toContain(">./register.js</pre>");
    });

    it("gives only the names of what it sets and sends", async () => {
      const out = await draw("en", gitKit(HOOKS), { shown });
      expect(out).toContain("Environment variables it sets (values not shown): API_TOKEN, ROOT");
      expect(out).toContain("Headers it sends (values not shown): Authorization");
    });

    it.each([
      ["module", "Hooks modules", "Claude Code loads this code into itself when a session starts"],
      ["mcp", "MCP servers", "when a session starts, and keeps it for the whole session"],
      ["monitor", "Monitors", "when a session starts, or the first time the skill it names runs"],
      ["lsp", "Language servers", "the first time Claude edits a file of the types shown"],
      ["skillHook", "Hooks of skills and commands", "While that skill or command is in use"],
      ["settings", "Settings", "They apply when a session starts"],
      ["grammar", "Highlighting grammars", "Claude Code loads this JavaScript"],
      ["bin", "Programs Claude can call by name", "Nothing starts these by itself"],
      ["binary", "Programs to download", "Claude Code may download these"],
      ["dependency", "Other plugins", "Claude Code enables these along with this one"],
      ["unknown", "Not understood", "Hangar does not know what these are"],
    ])("says plainly when a %s runs", async (_kind, title, sentence) => {
      const out = await draw("en", gitKit(HOOKS), { shown });
      expect(out).toContain(title);
      expect(out).toContain(sentence);
    });

    it("says what Hangar cannot read, and the sentence of each kind, in French with accents", async () => {
      const out = plainSpaces(await draw("fr", gitKit(HOOKS), { shown }));
      expect(out).toContain("Ne peut pas être lu par Hangar : ce que cela lance n&#x27;est pas montré ici.");
      expect(out).toContain("Non compris");
      expect(out).toContain("Moniteurs");
      expect(out).toContain("la première fois que le skill qu&#x27;il nomme se lance");
      expect(out).toContain("Variables d&#x27;environnement qu&#x27;il définit (valeurs masquées) : API_TOKEN, ROOT");
    });

    it("says it of each bundle, remote grammar, download, other plugin and broken file", async () => {
      const out = await draw("en", gitKit(HOOKS), { shown });
      const unreadable = running.filter((item) => item.unreadable).length;
      expect(out.match(/Cannot be read by Hangar/g)).toHaveLength(unreadable);
      // A broken file has nothing to show but its name, and its name is shown.
      expect(out).toContain('<span class="kit-step__title">.lsp.json</span>');
    });

    it("runs nothing is not said when only the rest runs", async () => {
      const out = await draw("en", gitKit(HOOKS), { shown: { ...inspection, hooks: [], running: [running[0]] } });
      expect(out).not.toContain("Nothing runs by itself.");
      expect(out).toContain("MCP servers");
      expect(out).not.toContain(">Hooks <");
    });

    it("asks once for a kit accepted when the list held only its hooks, and marks all of it", async () => {
      const out = await draw("en", gitKit(OTHER_HOOKS), {
        shown: { ...shown, hooksPermission: OTHER_HOOKS },
        entry: { id: "hello-kit", granted: ["loopback:8765", "data.read", HOOKS] },
      });
      expect(out).toContain("was not shown in full then");
      expect(out).toContain("this kit stays out of the workspace&#x27;s new panes");
      expect(out).toContain("Accept what changed");
      const steps = hooks.length + running.length + 1;
      expect(out.match(/kit-act__tag--new/g)).toHaveLength(steps);
    });

    it("does not ask once accepted, and accepting stores what was shown", async () => {
      const out = await draw("en", gitKit(HOOKS), {
        shown,
        entry: { id: "hello-kit", granted: ["panes.read", HOOKS] },
      });
      expect(out).toContain(">Close<");
      expect(out).not.toContain("was not shown in full then");
      expect(acceptedGrants(gitKit(OTHER_HOOKS), { tabs: [], hooksPermission: HOOKS })).toEqual([HOOKS]);
    });

    it("does not ask about a linked folder, though it shows the list", async () => {
      const folder: KitInfo = { ...gitKit(OTHER_HOOKS), source: { kind: "folder", path: "D:\\k" } };
      const out = await draw("en", folder, { shown, entry: { id: "hello-kit", granted: ["panes.read"] } });
      expect(out).toContain("MCP servers");
      expect(out).toContain("A linked folder is read live");
      expect(out).toContain(">Close<");
      expect(out).not.toContain("stays out");
    });
  });

  describe("the groups of what runs by itself", () => {
    it("go in the view's order, hooks modules first", () => {
      const groups = kitRunningGroups({
        hooks: [
          { event: "Stop", matcher: "", command: "x.sh" },
          { event: "modules", matcher: "", command: "./register.js" },
        ],
        running: [
          { kind: "unknown", name: "z" },
          { kind: "lsp", name: "l", argv: ["l"] },
          { kind: "mcp", name: "a", argv: ["a"] },
          { kind: "mcp", name: "b", text: "https://b" },
        ],
      });
      expect(groups.map((group) => group.kind)).toEqual(["module", "mcp", "lsp", "unknown"]);
      expect(groups[0].items).toEqual([{ name: "modules", text: "./register.js" }]);
      expect(groups[1].items.map((item) => item.name)).toEqual(["a", "b"]);
    });

    it("leave nothing out: a kind this build does not know is not understood", () => {
      const groups = kitRunningGroups({
        running: [{ kind: "teleporter" as KitRunning["kind"], name: "t", text: "./t.sh" }],
      });
      expect(groups).toEqual([{ kind: "unknown", items: [{ kind: "teleporter", name: "t", text: "./t.sh" }] }]);
    });

    it("are empty for a kit that runs nothing, or before it is read", () => {
      expect(kitRunningGroups(null)).toEqual([]);
      expect(kitRunningGroups({ hooks: [], running: [] })).toEqual([]);
    });

    it("keep the hooks proper apart from the modules", () => {
      expect(
        kitPlainHooks({
          hooks: [
            { event: "Stop", matcher: "", command: "x.sh" },
            { event: "modules", matcher: "", command: "./register.js" },
          ],
        }),
      ).toEqual([{ event: "Stop", matcher: "", command: "x.sh" }]);
      expect(kitPlainHooks(null)).toEqual([]);
    });

    it("write an item's line as it is written, what would not show written out", () => {
      expect(kitRunningLine({ argv: ["node", "a b"] })).toBe('node "a b"');
      expect(kitRunningLine({ text: "tail -F 'x y' | grep z" })).toBe("tail -F 'x y' | grep z");
      expect(kitRunningLine({ text: "safe\u202Erm -" })).toBe("safe\\u{202e}rm -");
      expect(kitRunningLine({ text: "hooks:\n  Stop: x\u200B" })).toBe("hooks:\n  Stop: x\\u{200b}");
      expect(kitRunningLine({})).toBeNull();
    });

    it("have a title and a sentence for each kind", () => {
      for (const kind of Object.keys(RUNNING_LABEL) as (keyof typeof RUNNING_LABEL)[]) {
        for (const locale of ["en", "fr"] as const) {
          expect(translate(locale, RUNNING_LABEL[kind].title)).not.toBe(RUNNING_LABEL[kind].title);
          expect(translate(locale, RUNNING_LABEL[kind].when)).not.toBe(RUNNING_LABEL[kind].when);
        }
      }
    });
  });

  describe("the diff, when a hash covers more than the hooks", () => {
    const requested = ["panes.read" as const, OTHER_HOOKS as `hooks:${string}`];

    it("asks again when what else runs changed, with the same permissions", () => {
      const diff = permissionDiff(requested, ["panes.read", HOOKS]);
      expect(diff.kept).toEqual(["panes.read"]);
      expect(diff.added).toEqual([OTHER_HOOKS]);
      expect(hooksToAccept(diff)).toBe(true);
    });

    it("does not ask when the workspace holds the current hash", () => {
      const diff = permissionDiff(requested, ["panes.read", OTHER_HOOKS]);
      expect(diff.added).toEqual([]);
      expect(hooksToAccept(diff)).toBe(false);
    });

    it("reads as to review, and then as on", () => {
      const kit = gitKit(OTHER_HOOKS);
      const asked = kitRequested(kit);
      expect(kitState({ id: "hello-kit", granted: ["panes.read", HOOKS] }, asked)).toBe("review");
      expect(kitState({ id: "hello-kit", granted: ["panes.read", OTHER_HOOKS] }, asked)).toBe("on");
    });
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

  describe("a kit's commands", () => {
    const ciRun = {
      name: "ci-run",
      description: "The state of a CI run.",
      argv: ["gh", "run", "view", "{run}", "--repo={repo}"],
      args: {
        repo: { pattern: "[\\w.-]+/[\\w.-]+" },
        run: { pattern: "[0-9]+", description: "The run's number" },
      },
      timeoutSeconds: 30,
    };
    const shown: KitInspection = {
      ...inspection,
      hooks: [],
      hooksPermission: null,
      tabs: [tab(["command:ci-run"])],
      declaredCommands: [ciRun],
      commandsPermission: COMMANDS,
    };

    it("shows each command verbatim, in the setup command's block, with its sentence and each rule", async () => {
      const out = await draw("en", commandKit(COMMANDS), { shown });
      expect(out).toContain("Commands its tabs can run");
      expect(out).toContain("The state of a CI run.");
      expect(out).toContain('<pre class="kit-step__cmd"');
      expect(out).toContain(">gh run view {run} --repo={repo}</pre>");
      expect(out).toContain(
        "A tab of this kit can run this command on your computer whenever it wants, with your logins, for instance your gh login; only the parts in braces change, and they must match their rule.",
      );
      // The rule sits with its placeholder, and they stay as written.
      expect(out).toContain("<code>{run}</code></dt><dd><code>[0-9]+</code> The run&#x27;s number</dd>");
      expect(out).toContain("<code>{repo}</code></dt><dd><code>[\\w.-]+/[\\w.-]+</code></dd>");
    });

    it("says it in French, with accents", async () => {
      const out = plainSpaces(await draw("fr", commandKit(COMMANDS), { shown }));
      expect(out).toContain("Commandes que ses onglets peuvent lancer");
      expect(out).toContain(
        "Un onglet de ce kit peut lancer cette commande sur votre ordinateur quand il le veut, avec vos identifiants, par exemple votre connexion gh ; seules les parties entre accolades changent, et elles doivent respecter leur règle.",
      );
      expect(out).toContain("lancer la commande ci-run sur votre ordinateur, avec vos identifiants");
    });

    it("does not speak of parts that change when nothing does", async () => {
      const fixed = { ...ciRun, argv: ["gh", "run", "list"], args: {} };
      const out = await draw("en", commandKit(COMMANDS), { shown: { ...shown, declaredCommands: [fixed] } });
      expect(out).toContain("whenever it wants, with your logins, for instance your gh login; nothing in it changes.");
      expect(out).not.toContain("parts in braces");
      expect(out).not.toContain("kit-step__rules");
    });

    it("says when its tabs run nothing", async () => {
      const out = await draw("en", commandKit(null), { shown: { ...shown, declaredCommands: [], commandsPermission: null } });
      expect(out).toContain("No command: its tabs run nothing on your computer.");
      expect(out).not.toContain("whenever it wants");
    });

    it("counts them among what a kit brings", async () => {
      const out = await draw("en", commandKit(COMMANDS), { shown: { ...shown, tabs: [] } });
      expect(out).not.toContain(translate("en", "kitsManage.actNothing"));
    });

    it("marks commands that changed, and asks to accept them", async () => {
      const out = await draw("en", commandKit(OTHER_COMMANDS), {
        shown,
        entry: { id: "hello-kit", granted: ["command:ci-run", COMMANDS] },
      });
      expect(out).toContain("Until you accept them, this kit&#x27;s tabs run none of them.");
      expect(out).toContain("Accept what changed");
      expect(out).toContain("kit-act__tag--new");
    });

    it("closes without asking once these very commands are accepted", async () => {
      const out = await draw("en", commandKit(COMMANDS), {
        shown,
        entry: { id: "hello-kit", granted: ["command:ci-run", COMMANDS] },
      });
      expect(out).toContain(">Close<");
      expect(out).toContain("Already accepted");
      expect(out).not.toContain("run none of them");
    });

    it("does not ask about a linked folder's commands, and says they apply as they are edited", async () => {
      const out = await draw("en", commandKit(OTHER_COMMANDS, FOLDER), {
        shown,
        entry: { id: "hello-kit", granted: ["command:ci-run"] },
      });
      expect(out).toContain("A linked folder is read live: what you change in these commands applies");
      expect(out).toContain(">Close<");
      expect(out).not.toContain("Accept what changed");
    });
  });

  describe("on all workspaces", () => {
    it("does not name a workspace when accepting holds for all of them", async () => {
      const out = await draw("en", gitKit(HOOKS), { everywhere: true });
      expect(out).toContain("On all workspaces");
      expect(out).not.toContain("Studio");
      expect(await draw("en", gitKit(HOOKS))).toContain("Studio");
      expect(await draw("fr", gitKit(HOOKS), { everywhere: true })).toContain("Sur tous les workspaces");
    });

    const row = async (locale: "en" | "fr", info: KitInfo, granted: string[]) => {
      const { KitEverywhereRow } = await load();
      return renderToStaticMarkup(
        createElement(KitEverywhereRow, {
          t: (key, params) => translate(locale, key, params),
          kit: info,
          entry: { id: info.id, granted },
        }),
      );
    };

    it("shows the kit on, with its switch locked, and says where it is managed", async () => {
      const out = await row("en", gitKit(HOOKS), ["panes.read", HOOKS]);
      expect(out).toMatch(/<input[^>]*checked=""/);
      expect(out).toMatch(/<input[^>]*disabled=""/);
      expect(out).toContain('aria-label="Hello kit is enabled on all workspaces: change it in Settings › Kits"');
      expect(out).toContain("Managed in Settings › Kits.");
      expect(out).toContain(">On all workspaces<");
      expect(out).not.toContain("<button");
    });

    it("says so in French", async () => {
      const out = plainSpaces(await row("fr", gitKit(HOOKS), ["panes.read", HOOKS]));
      expect(out).toContain('aria-label="Hello kit est activé sur tous les workspaces : modifiez-le dans Réglages › Kits"');
      expect(out).toContain("Géré dans Réglages › Kits.");
    });

    it("says when it waits for a review, or cannot be used", async () => {
      expect(await row("en", gitKit(OTHER_HOOKS), ["panes.read", HOOKS])).toContain("kit-act__tag--new");
      const broken = await row("en", kit("1.0.0", [], "its manifest is invalid"), []);
      expect(broken).toContain("its manifest is invalid");
      expect(broken).toContain(translate("en", "kitsManage.statusBroken"));
    });
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
