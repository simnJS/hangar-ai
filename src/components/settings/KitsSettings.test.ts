import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { KitInfo, KitInspection, KitSetupStatus } from "../../lib/kits";

// The views are drawn from props; the store and Tauri are not needed.
vi.mock("../../store", () => ({ useStore: () => ({}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { KitsView } = await import("./KitsSettings");
const { KitActivationView } = await import("../KitActivation");
const { translate } = await import("../../i18n");
type ViewProps = Parameters<typeof KitsView>[0];
type ActProps = Parameters<typeof KitActivationView>[0];

const manifest = (version = "1.0.0"): NonNullable<KitInfo["manifest"]> => ({
  schema: 1,
  id: "roblox",
  name: "Roblox",
  version,
  description: "Build places with agents.",
  tabs: [
    { id: "explorer", title: "Explorer", entry: "tabs/e.html", permissions: ["panes.send", "notify"] },
  ],
  setup: [],
});

const gitKit: KitInfo = {
  id: "roblox",
  source: { kind: "git", url: "https://github.com/o/kits", ref: "main", subdir: "roblox" },
  dir: "C:\\k",
  installedVersion: "1.0.0",
  addedAt: 0,
  manifest: manifest(),
  error: null,
  update: null,
};
const folderKit: KitInfo = {
  ...gitKit,
  id: "hello-kit",
  source: { kind: "folder", path: "D:\\kits\\hello-kit" },
  manifest: { ...manifest(), id: "hello-kit", name: "Hello kit" },
};
const brokenKit: KitInfo = {
  ...gitKit,
  id: "bad-kit",
  manifest: null,
  error: "tabs[0].permissions: unknown permission \"files.write\"",
};

function viewProps(locale: "en" | "fr", patch: Partial<ViewProps> = {}): ViewProps {
  const noop = () => undefined;
  return {
    t: (key, params) => translate(locale, key, params),
    kits: [gitKit, folderKit, brokenKit],
    error: null,
    flash: null,
    notes: {},
    removing: null,
    adding: null,
    folderError: null,
    gitUrl: "",
    gitError: null,
    onGitUrl: noop,
    onAddFolder: noop,
    onAddGit: noop,
    onCheck: noop,
    onUpdate: noop,
    onAskRemove: noop,
    onRemove: noop,
    ...patch,
  };
}

const html = (props: ViewProps) => renderToStaticMarkup(createElement(KitsView, props));

describe("Settings › Kits", () => {
  it("lists each kit with its version and source, and shows a broken manifest's error", () => {
    const out = html(viewProps("en"));
    expect(out).toContain("Roblox");
    expect(out).toContain("v1.0.0");
    expect(out).toContain("GitHub: https://github.com/o/kits (main · roblox)");
    expect(out).toContain("Folder: D:\\kits\\hello-kit");
    expect(out).toContain("linked, read live");
    expect(out).toContain("This kit cannot be used");
    expect(out).toContain("unknown permission");
  });

  it("offers an update check on a GitHub kit and a reload on a linked folder", () => {
    const out = html(viewProps("en", { kits: [gitKit, folderKit] }));
    expect(out).toContain("Check for update");
    expect(out).toContain("Reload");
    expect(out).toContain("Add from a folder");
    expect(out).toContain("Add from GitHub");
  });

  it("shows the old and the new version of an available update", () => {
    const out = html(
      viewProps("en", {
        kits: [gitKit],
        notes: { roblox: { kind: "available", current: "1.0.0", latest: "1.1.0" } },
      }),
    );
    expect(out).toContain("v1.0.0 → v1.1.0");
    expect(out).toContain(">Update<");
  });

  it("says a linked folder is left untouched before removing it", () => {
    const out = html(viewProps("en", { kits: [folderKit], removing: "hello-kit" }));
    expect(out).toContain("The linked folder is left untouched");
    expect(out).toContain("Remove the kit");
  });

  it("shows the inline errors of both ways of adding", () => {
    const out = html(viewProps("en", { folderError: "no hangar-kit.json", gitError: "not a GitHub link" }));
    expect(out).toContain("no hangar-kit.json");
    expect(out).toContain("not a GitHub link");
    expect(out).toContain('aria-invalid="true"');
  });

  it("speaks French, with accents", () => {
    const out = html(viewProps("fr", { kits: [folderKit], removing: "hello-kit" }));
    expect(out).toContain("Le dossier lié n&#x27;est pas touché");
    expect(out).toContain("Ajouter depuis un dossier");
  });
});

const inspection: KitInspection = {
  plugin: { name: "roblox", dir: "C:\\k" },
  skills: ["build"],
  commands: ["place"],
  agents: [],
  mcpServers: ["studio"],
  tabs: manifest().tabs,
  setup: [],
};
const step: KitSetupStatus = {
  id: "deps",
  title: "Install the server's dependencies",
  command: "npm install --prefix server",
  sha256: "x",
  done: false,
};

function actProps(locale: "en" | "fr", patch: Partial<ActProps> = {}): ActProps {
  const noop = () => undefined;
  return {
    t: (key, params) => translate(locale, key, params),
    kit: gitKit,
    inspection,
    steps: [step],
    entry: undefined,
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
    ...patch,
  };
}

const act = (props: ActProps) => renderToStaticMarkup(createElement(KitActivationView, props));

describe("the activation view", () => {
  it("lists what the kit brings, its permissions in plain words and the exact command", () => {
    const out = act(actProps("en"));
    expect(out).toContain("build");
    expect(out).toContain("studio");
    expect(out).toContain("send messages to your agents");
    expect(out).toContain('<pre class="kit-step__cmd"');
    expect(out).toContain(">npm install --prefix server</pre>");
    expect(out).toContain("Kits reach Claude Code panes only");
    expect(out).toContain("Accept and enable");
  });

  it("says the permissions in French", () => {
    expect(act(actProps("fr"))).toContain("envoyer des messages à vos agents");
  });

  it("marks the permissions an update added, and asks to accept them again", () => {
    const out = act(actProps("en", { entry: { id: "roblox", granted: ["notify"] } }));
    expect(out).toContain("kit-act__tag--new");
    expect(out).toContain("Accept the permissions");
  });

  it("offers Run only for a step not done, and disables it before the workspace exists", () => {
    expect(act(actProps("en"))).toContain(">Run<");
    expect(act(actProps("en", { steps: [{ ...step, done: true }] }))).not.toContain(">Run<");
    const early = act(actProps("en", { canRun: false }));
    expect(early).toMatch(/<button[^>]*disabled[^>]*>Run</);
  });

  it("asks whether it worked after the command was typed, never marking it itself", () => {
    const out = act(actProps("en", { ran: { stepId: "deps", paneName: "Ava" } }));
    expect(out).toContain("It worked, mark it done");
    expect(out).toContain("Ava");
  });

  it("closes without accepting once everything is accepted", () => {
    const out = act(actProps("en", { entry: { id: "roblox", granted: ["panes.send", "notify"] } }));
    expect(out).toContain(">Close<");
    expect(out).not.toContain("Accept and enable");
  });
});
