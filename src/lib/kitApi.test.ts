import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneActivity } from "./agentState";
import {
  createKitHost,
  dataRelativePath,
  kitMayCallEvent,
  kitPanes,
  kitSender,
  projectRelativePath,
  workspaceRelativePath,
  KIT_DATA_WRITE_MAX,
  KIT_DATA_WRITE_WINDOW_MS,
  KIT_FILES_WRITE_MAX,
  KIT_FILES_WRITE_WINDOW_MS,
  KIT_LOOPBACK_PATH_MAX_CHARS,
  KIT_MAX_PENDING,
  KIT_NOTIFY_MAX,
  KIT_SEND_MAX,
  KIT_SEND_MAX_CHARS,
  KIT_SEND_WINDOW_MS,
  KIT_TAB_BADGE_TEXT_MAX,
  KIT_TAB_CHANGE_MAX,
  KIT_TAB_CHANGE_WINDOW_MS,
  KIT_TAB_PARAMS_EVENT,
  KIT_TAB_TITLE_MAX,
  type KitFrame,
  type KitHost,
  type KitHostDeps,
} from "./kitApi";
import {
  KIT_BINARY_MAX_BYTES,
  KIT_COMMAND_PATTERN,
  KIT_DATA_MAX_BYTES,
  KIT_FILE_MAX_BYTES,
  KIT_LIST_PATTERN_MAX_CHARS,
  KIT_LOOPBACK_MAX_BODY_BYTES,
  KIT_LOOPBACK_MAX_TIMEOUT_MS,
  KIT_LOOPBACK_PATTERN,
  KIT_LOOPBACK_TIMEOUT_MS,
  KIT_METHODS,
  KIT_PERMISSIONS,
  type KitInfo,
  type KitMethod,
} from "./kits";
import { workspaceKitTabs } from "./kitTabs";
import type { Pane, Workspace } from "../types";

const pane = (id: string, name: string, agent: Pane["agent"], role?: Pane["role"]): Pane => ({
  id,
  name,
  agent,
  sessionId: null,
  cwd: null,
  shellId: null,
  title: null,
  ...(role ? { role } : {}),
});

const workspace: Workspace = {
  id: "ws-1",
  name: "Game",
  cwd: "C:/projects/game",
  extraRoots: [],
  panes: [
    pane("p-ava", "Ava", "claude", "orchestrator"),
    pane("p-ben", "Ben", "codex"),
    pane("p-cleo", "Cleo", "claude"),
    pane("p-sh", "Shell", "shell"),
  ],
  themeId: null,
  shellId: null,
  tree: null,
  savedCommands: [],
  folderId: null,
  kits: [{ id: "hello-kit", granted: [...KIT_PERMISSIONS] }],
};

const activities: Record<string, PaneActivity> = {
  "p-ava": { workspaceId: "ws-1", activity: "working", bridged: true },
  "p-ben": { workspaceId: "ws-1", activity: "idle", bridged: false },
  "p-cleo": { workspaceId: "ws-1", activity: "idle", bridged: false },
};

interface Sent {
  hangar: 1;
  id?: string | number;
  result?: unknown;
  error?: { code: string; message: string };
  event?: string;
  data?: unknown;
}

/** A frame's window: what Hangar posts to it, kept. */
function makeFrame() {
  const sent: Sent[] = [];
  const frame: KitFrame = { postMessage: vi.fn((message: unknown) => void sent.push(message as Sent)) };
  return { frame, sent };
}

/** Lets the host's promise chains run. */
const settle = async () => {
  for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
};

/** Everything a tab can hold, but what makes `files.read` reach hidden files: below, that is said. */
const READING_ONLY = KIT_PERMISSIONS.filter((permission) => permission !== "files.write");

let clock = 1_000_000;
let granted: string[] = [...READING_ONLY];
let current: Workspace | null = workspace;
let commandsAccepted: string | null = null;
let tabParams: Record<string, string> = {};
let tabCaller: { pane: string } | null = null;
/** What the tab's calls changed of the tab itself. */
let tabChanges: unknown[] = [];

const COMMAND_ANSWER = { exitCode: 0, stdout: "{}", stderr: "", timedOut: false, truncated: false };
const BINARY_ANSWER = { base64: "iVBORw==", mime: "image/png" };
const LIST_ANSWER = { files: [{ path: "src/a.rs", size: 3, modified: 1793478269409 }], truncated: false };

function makeDeps(overrides: Partial<KitHostDeps> = {}): KitHostDeps {
  return {
    readFile: vi.fn(async (_root: string, path: string) => `text of ${path}`),
    resolveFile: vi.fn(async (root: string, path: string) => `${root}/${path}`),
    readHiddenFile: vi.fn(async (_root: string, path: string) => `hidden text of ${path}`),
    readBinaryFile: vi.fn(async () => BINARY_ANSWER),
    listFiles: vi.fn(async () => LIST_ANSWER),
    writeFile: vi.fn(async () => undefined),
    writeBinaryFile: vi.fn(async () => undefined),
    showFile: vi.fn(async () => undefined),
    enqueue: vi.fn(async () => ({ id: "q-1" })),
    activity: (paneId: string) => activities[paneId],
    notify: vi.fn(async () => undefined),
    readData: vi.fn(async (_kit: string, path: string) => `data of ${path}`),
    writeData: vi.fn(async () => undefined),
    loopbackFetch: vi.fn(async () => ({ status: 200, text: "{}" })),
    runCommand: vi.fn(async () => COMMAND_ANSWER),
    now: () => clock,
    ...overrides,
  };
}

function setup(overrides: Partial<KitHostDeps> = {}) {
  const { frame, sent } = makeFrame();
  const deps = makeDeps(overrides);
  const host = createKitHost(
    () => frame,
    {
      kitId: "hello-kit",
      kitName: () => "Hello kit",
      workspace: () => current,
      permissions: () => granted,
      commandsAccepted: () => commandsAccepted,
      params: () => tabParams,
      caller: () => tabCaller,
      setTitle: (title) => tabChanges.push({ title }),
      setBadge: (badge) => tabChanges.push({ badge }),
    },
    deps,
  );
  let next = 0;
  /** A request from the frame, answered. */
  const call = async (method: string, params?: unknown) => {
    const id = `r${++next}`;
    host.receive({ source: frame, origin: "null", data: { hangar: 1, id, method, params } });
    await settle();
    return sent.find((message) => message.id === id);
  };
  return { host, frame, sent, deps, call };
}

const errorOf = (answer: Sent | undefined) => answer?.error?.code;

beforeEach(() => {
  clock = 1_000_000;
  granted = [...READING_ONLY];
  current = workspace;
  commandsAccepted = null;
  tabParams = {};
  tabCaller = null;
  tabChanges = [];
});

describe("what a tab may ask about itself, with no permission", () => {
  beforeEach(() => {
    granted = [];
  });

  it("tab.params answers what it was opened with, and nothing without any", async () => {
    const { call } = setup();
    expect((await call("tab.params"))?.result).toEqual({ params: {}, caller: null });
    tabParams = { run: "4567" };
    expect((await call("tab.params", {}))?.result).toEqual({ params: { run: "4567" }, caller: null });
  });

  it("tab.params says who asked, from Hangar's record and never from the message", async () => {
    const { call } = setup();
    tabCaller = { pane: "Ava" };
    expect((await call("tab.params"))?.result).toEqual({ params: {}, caller: { pane: "Ava" } });
    const claimed = await call("tab.params", { caller: { pane: "Boss" }, pane: "Boss" });
    expect(claimed?.result).toEqual({ params: {}, caller: { pane: "Ava" } });
    tabCaller = null;
    expect((await call("tab.params"))?.result).toEqual({ params: {}, caller: null });
  });

  it("the tab.params event reaches its page whatever it was granted", () => {
    const { host, sent } = setup();
    host.emit(KIT_TAB_PARAMS_EVENT, { params: { view: "b" } });
    expect(sent).toEqual([{ hangar: 1, event: "tab.params", data: { params: { view: "b" } } }]);
    // The others still take their permission.
    host.emit("panes.changed", {});
    expect(sent).toHaveLength(1);
    expect(kitMayCallEvent("tab.params", [])).toBe(true);
    expect(kitMayCallEvent("panes.changed", [])).toBe(false);
  });

  it("tab.setTitle takes a short plain text, trimmed", async () => {
    const { call } = setup();
    const title = "x".repeat(KIT_TAB_TITLE_MAX);
    expect((await call("tab.setTitle", { title }))?.result).toEqual({});
    await call("tab.setTitle", { title: "  Run #4567 " });
    expect(tabChanges).toEqual([{ title }, { title: "Run #4567" }]);
    expect(KIT_TAB_TITLE_MAX).toBe(40);
  });

  it("tab.setTitle counts what a person sees, and keeps markup as text", async () => {
    const { call } = setup();
    const emoji = "🚀".repeat(KIT_TAB_TITLE_MAX);
    expect(errorOf(await call("tab.setTitle", { title: emoji }))).toBeUndefined();
    expect(errorOf(await call("tab.setTitle", { title: "<b>Run</b>" }))).toBeUndefined();
    expect(tabChanges).toEqual([{ title: emoji }, { title: "<b>Run</b>" }]);
  });

  it("tab.setTitle refuses what is too long, empty, or not plain text on one line", async () => {
    const { call } = setup();
    for (const title of [
      "x".repeat(KIT_TAB_TITLE_MAX + 1),
      "",
      "   ",
      "two\nlines",
      "a\tb",
      "‮reversed",
      "zero​width",
      42,
      null,
      undefined,
    ]) {
      const answer = await call("tab.setTitle", { title });
      expect(answer?.error?.code, String(title)).toBe("invalid");
      expect(answer?.error?.message.startsWith("title "), String(title)).toBe(true);
    }
    expect(errorOf(await call("tab.setTitle"))).toBe("invalid");
    expect(tabChanges).toEqual([]);
  });

  it("tab.setBadge takes each state, with a short text or none", async () => {
    const { call } = setup();
    for (const state of ["running", "success", "failure", "neutral"]) {
      expect((await call("tab.setBadge", { state }))?.result).toEqual({});
    }
    await call("tab.setBadge", { state: "running", text: "x".repeat(KIT_TAB_BADGE_TEXT_MAX) });
    await call("tab.setBadge", { state: "neutral", text: null });
    await call("tab.setBadge", { state: null });
    expect(tabChanges).toEqual([
      { badge: { state: "running", text: null } },
      { badge: { state: "success", text: null } },
      { badge: { state: "failure", text: null } },
      { badge: { state: "neutral", text: null } },
      { badge: { state: "running", text: "x".repeat(12) } },
      { badge: { state: "neutral", text: null } },
      { badge: null },
    ]);
    expect(KIT_TAB_BADGE_TEXT_MAX).toBe(12);
  });

  it("tab.setBadge refuses a state it does not know, and a text past its limit", async () => {
    const { call } = setup();
    for (const params of [
      {},
      { state: "pending" },
      { state: "constructor" },
      { state: 1 },
      { state: "running", text: "x".repeat(KIT_TAB_BADGE_TEXT_MAX + 1) },
      { state: "running", text: "" },
      { state: "running", text: "a\nb" },
      { state: "running", text: 12 },
      "running",
    ]) {
      expect(errorOf(await call("tab.setBadge", params)), JSON.stringify(params)).toBe("invalid");
    }
    expect(tabChanges).toEqual([]);
  });

  it("limits how often each may redraw the strip, and lets go after a minute", async () => {
    const { call } = setup();
    for (let i = 0; i < KIT_TAB_CHANGE_MAX; i += 1) {
      expect(errorOf(await call("tab.setBadge", { state: "running" }))).toBeUndefined();
    }
    expect(errorOf(await call("tab.setBadge", { state: "running" }))).toBe("rate_limited");
    // Each its own: a badge counting does not stop the title.
    expect(errorOf(await call("tab.setTitle", { title: "Run" }))).toBeUndefined();
    clock += KIT_TAB_CHANGE_WINDOW_MS;
    expect(errorOf(await call("tab.setBadge", { state: "running" }))).toBeUndefined();
  }, 30_000);

  it("is not on the list of methods a permission gives, and an unknown name stays unknown", async () => {
    for (const method of ["tab.params", "tab.setTitle", "tab.setBadge"]) {
      expect(Object.keys(KIT_METHODS)).not.toContain(method);
    }
    const { call } = setup();
    expect(errorOf(await call("tab.close"))).toBe("unknown");
    expect(errorOf(await call("constructor"))).toBe("unknown");
    expect(errorOf(await call("__proto__"))).toBe("unknown");
  });

  it("answers nothing from another frame, about this tab or any", async () => {
    const { host, sent } = setup();
    const stranger = makeFrame();
    host.receive({ source: stranger.frame, origin: "null", data: { hangar: 1, id: "x", method: "tab.params" } });
    host.receive({ source: stranger.frame, origin: "null", data: { hangar: 1, id: "y", method: "tab.setTitle", params: { title: "Mine" } } });
    await settle();
    expect(sent).toEqual([]);
    expect(stranger.sent).toEqual([]);
    expect(tabChanges).toEqual([]);
  });

  it("does nothing once the tab is closed", async () => {
    const { host, frame, sent } = setup();
    host.close();
    host.receive({ source: frame, origin: "null", data: { hangar: 1, id: "x", method: "tab.setTitle", params: { title: "Late" } } });
    host.emit(KIT_TAB_PARAMS_EVENT, { params: {} });
    await settle();
    expect(sent).toEqual([]);
    expect(tabChanges).toEqual([]);
  });
});

/** Each method, with params it accepts and what its fake deps answer. */
const CALLS: Record<KitMethod, { params?: unknown; result: unknown }> = {
  "workspace.info": { result: { id: "ws-1", name: "Game", folder: "C:/projects/game" } },
  "panes.list": { result: kitPanes(workspace, (id) => activities[id]) },
  "panes.send": { params: { pane: "Ava", text: "Build the place" }, result: { queued: true, pane: "Ava" } },
  "files.read": { params: { path: "src/main.luau" }, result: "text of src/main.luau" },
  "files.readBinary": { params: { path: "shots/a.png" }, result: BINARY_ANSWER },
  "files.list": { params: { path: "src", pattern: "*.rs" }, result: LIST_ANSWER },
  "files.write": { params: { path: ".env", text: "A=1" }, result: {} },
  "files.writeBinary": { params: { path: ".hangar/annotations/1.png", base64: "iVBORw==" }, result: {} },
  "preview.show": { params: { path: "docs/plan.md" }, result: { opened: true, path: "docs/plan.md" } },
  "data.read": { params: { path: "queue/state.json" }, result: { text: "data of queue/state.json" } },
  "data.write": { params: { path: "queue/state.json", text: "{}" }, result: {} },
  "loopback.fetch": { params: { port: 8765, path: "/queue" }, result: { status: 200, text: "{}" } },
  "command.run": { params: { name: "ci-status", args: { run: "42" } }, result: COMMAND_ANSWER },
  notify: { params: { title: "Built", body: "The place is ready." }, result: null },
};

/** What a tab holds to make this very call. */
function permissionFor(method: KitMethod): string {
  const needs = KIT_METHODS[method];
  if (needs === KIT_LOOPBACK_PATTERN) return "loopback:8765";
  if (needs === KIT_COMMAND_PATTERN) return "command:ci-status";
  return needs;
}

describe("each permission, granted and refused", () => {
  for (const [method, { params, result }] of Object.entries(CALLS) as [KitMethod, (typeof CALLS)[KitMethod]][]) {
    const permission = permissionFor(method);

    it(`answers ${method} with ${permission} granted`, async () => {
      granted = [permission];
      const { call } = setup();
      const answer = await call(method, params);
      expect(answer?.error).toBeUndefined();
      expect(answer?.result).toEqual(result);
    });

    it(`answers ${method} "denied" without ${permission}, and does nothing`, async () => {
      granted = KIT_PERMISSIONS.filter((entry) => entry !== permission);
      const { call, deps } = setup();
      const answer = await call(method, params);
      expect(answer?.error).toEqual({ code: "denied", message: `this tab was not granted ${permission}` });
      expect(deps.readFile).not.toHaveBeenCalled();
      expect(deps.readHiddenFile).not.toHaveBeenCalled();
      expect(deps.readBinaryFile).not.toHaveBeenCalled();
      expect(deps.listFiles).not.toHaveBeenCalled();
      expect(deps.writeFile).not.toHaveBeenCalled();
      expect(deps.writeBinaryFile).not.toHaveBeenCalled();
      expect(deps.resolveFile).not.toHaveBeenCalled();
      expect(deps.showFile).not.toHaveBeenCalled();
      expect(deps.enqueue).not.toHaveBeenCalled();
      expect(deps.notify).not.toHaveBeenCalled();
      expect(deps.readData).not.toHaveBeenCalled();
      expect(deps.writeData).not.toHaveBeenCalled();
      expect(deps.loopbackFetch).not.toHaveBeenCalled();
      expect(deps.runCommand).not.toHaveBeenCalled();
    });
  }

  it("reads what is granted at each call: a grant taken back denies the next call", async () => {
    const { call } = setup();
    expect(errorOf(await call("panes.list"))).toBeUndefined();
    granted = [];
    expect(errorOf(await call("panes.list"))).toBe("denied");
  });

  it("answers an unknown method as such, whatever is granted", async () => {
    const { call } = setup();
    const answer = await call("files.delete", { path: "x.txt", text: "x" });
    expect(errorOf(answer)).toBe("unknown");
    expect(errorOf(await call("constructor"))).toBe("unknown");
    expect(errorOf(await call("__proto__"))).toBe("unknown");
  });
});

describe("only the tab's own frame is answered", () => {
  it("ignores a message from another window, without a word", async () => {
    const { host, sent, deps } = setup();
    const stranger = makeFrame().frame;
    host.receive({
      source: stranger,
      origin: "null",
      data: { hangar: 1, id: "x", method: "panes.send", params: { pane: "Ava", text: "hi" } },
    });
    host.receive({ source: null, origin: "null", data: { hangar: 1, id: "y", method: "panes.list" } });
    await settle();
    expect(sent).toEqual([]);
    expect(deps.enqueue).not.toHaveBeenCalled();
  });

  it("ignores its own frame when the page has an origin: it is not the isolated page", async () => {
    const { host, frame, sent } = setup();
    for (const origin of ["http://tauri.localhost", "https://example.com", ""]) {
      host.receive({ source: frame, origin, data: { hangar: 1, id: "x", method: "panes.list" } });
    }
    await settle();
    expect(sent).toEqual([]);
  });

  it("ignores what is not a request of the protocol", async () => {
    const { host, frame, sent } = setup();
    for (const data of [
      null,
      "panes.list",
      [1, 2],
      { id: "x", method: "panes.list" },
      { hangar: 2, id: "x", method: "panes.list" },
      { hangar: 1, method: "panes.list" },
      { hangar: 1, id: "", method: "panes.list" },
      { hangar: 1, id: "x".repeat(200), method: "panes.list" },
      { hangar: 1, id: Number.NaN, method: "panes.list" },
      { hangar: 1, id: { nested: true }, method: "panes.list" },
    ]) {
      host.receive({ source: frame, origin: "null", data });
    }
    await settle();
    expect(sent).toEqual([]);
  });

  it("answers a request without a method name as invalid", async () => {
    const { host, frame, sent } = setup();
    host.receive({ source: frame, origin: "null", data: { hangar: 1, id: 7, method: 42 } });
    await settle();
    expect(sent).toEqual([{ hangar: 1, id: 7, error: { code: "invalid", message: "method must be a string" } }]);
  });

  it("stops answering once closed, a call in flight included", async () => {
    let release: (text: string) => void = () => undefined;
    const { host, frame, sent, call } = setup({
      readFile: vi.fn(() => new Promise<string>((resolve) => (release = resolve))),
    });
    host.receive({
      source: frame,
      origin: "null",
      data: { hangar: 1, id: "slow", method: "files.read", params: { path: "a.txt" } },
    });
    host.close();
    release("late");
    await settle();
    expect(await call("panes.list")).toBeUndefined();
    host.emit("panes.changed", {});
    expect(sent).toEqual([]);
  });
});

describe("params are checked before anything is done", () => {
  it("refuses params that are not an object", async () => {
    const { call, deps } = setup();
    for (const params of [undefined, null, "Ava", ["Ava", "hi"], 3]) {
      expect(errorOf(await call("panes.send", params))).toBe("invalid");
      expect(errorOf(await call("files.read", params))).toBe("invalid");
      expect(errorOf(await call("notify", params))).toBe("invalid");
    }
    expect(deps.enqueue).not.toHaveBeenCalled();
    expect(deps.readFile).not.toHaveBeenCalled();
  });

  it("files.read and preview.show take a relative path inside the workspace folder only", async () => {
    const { call, deps } = setup();
    for (const path of [
      "",
      "   ",
      42,
      undefined,
      "/etc/passwd",
      "\\\\server\\share\\x.txt",
      "C:/Windows/win.ini",
      "C:\\Windows\\win.ini",
      "../secret.txt",
      "src/../../secret.txt",
      "src\\..\\..\\secret.txt",
      ".env",
      ".git/config",
      "src/.secrets/key.txt",
      "README.md:stream",
      "README.md.",
      "a\0b",
      ".",
      "x/".repeat(600),
    ]) {
      const read = await call("files.read", { path });
      expect(errorOf(read), String(path)).toBe("invalid");
      const show = await call("preview.show", { path });
      expect(errorOf(show), String(path)).toBe("invalid");
    }
    expect(deps.readFile).not.toHaveBeenCalled();
    expect(deps.resolveFile).not.toHaveBeenCalled();
    expect(deps.showFile).not.toHaveBeenCalled();
  });

  it("hands Rust the path relative to the workspace folder, normalized", async () => {
    const { call, deps } = setup();
    expect((await call("files.read", { path: "src\\levels/./one.json" }))?.result).toBe(
      "text of src/levels/one.json",
    );
    expect(deps.readFile).toHaveBeenCalledWith("C:/projects/game", "src/levels/one.json");
    expect(workspaceRelativePath("./a//b\\c.txt")).toBe("a/b/c.txt");
  });

  it("says what Rust refused a file for", async () => {
    const { call } = setup({
      readFile: vi.fn(async () => {
        throw "big.log is larger than 1 MB";
      }),
      resolveFile: vi.fn(async () => {
        throw new Error("missing.png was not found in the workspace folder");
      }),
    });
    expect((await call("files.read", { path: "big.log" }))?.error).toEqual({
      code: "failed",
      message: "big.log is larger than 1 MB",
    });
    expect((await call("preview.show", { path: "missing.png" }))?.error).toEqual({
      code: "failed",
      message: "missing.png was not found in the workspace folder",
    });
  });

  it("opens the file Rust resolved, under the kit's name", async () => {
    const { call, deps } = setup();
    await call("preview.show", { path: "art/logo.png" });
    expect(deps.showFile).toHaveBeenCalledWith(workspace, "C:/projects/game/art/logo.png", "Hello kit");
  });

  it("panes.send needs a pane and a text, within bounds", async () => {
    const { call, deps } = setup();
    expect(errorOf(await call("panes.send", { text: "hi" }))).toBe("invalid");
    expect(errorOf(await call("panes.send", { pane: "Ava" }))).toBe("invalid");
    expect(errorOf(await call("panes.send", { pane: "Ava", text: "   " }))).toBe("invalid");
    expect(errorOf(await call("panes.send", { pane: "Ava", text: 3 }))).toBe("invalid");
    expect(
      errorOf(await call("panes.send", { pane: "Ava", text: "x".repeat(KIT_SEND_MAX_CHARS + 1) })),
    ).toBe("invalid");
    expect(deps.enqueue).not.toHaveBeenCalled();
  });

  it("notify needs a title, and says it comes from the kit", async () => {
    const { call, deps } = setup();
    expect(errorOf(await call("notify", { body: "x" }))).toBe("invalid");
    expect(errorOf(await call("notify", { title: "x".repeat(121) }))).toBe("invalid");
    expect(errorOf(await call("notify", { title: "Hi", body: 5 }))).toBe("invalid");
    expect((await call("notify", { title: "Build done" }))?.result).toBeNull();
    expect(deps.notify).toHaveBeenCalledWith("Hello kit · Build done", "");
  });
});

describe("hidden files", () => {
  const HIDDEN = [".env", ".env.local", ".github/workflows/ci.yml", "src/.secrets/key.txt"];
  const GIT = [".git", ".git/config", ".GIT/config", "sub/.git/hooks/pre-commit", ".git\\config", "./.git/config"];

  describe("with files.read alone", () => {
    beforeEach(() => {
      granted = ["files.read"];
    });

    it("refuses them, as it always has", async () => {
      const { call, deps } = setup();
      for (const path of HIDDEN) {
        expect(errorOf(await call("files.read", { path })), path).toBe("invalid");
        expect(errorOf(await call("files.readBinary", { path: `${path}.png` })), path).toBe("invalid");
      }
      expect(deps.readFile).not.toHaveBeenCalled();
      expect(deps.readHiddenFile).not.toHaveBeenCalled();
      expect(deps.readBinaryFile).not.toHaveBeenCalled();
    });

    it("reads what it reads through the same door, and tells Rust hidden files stay out", async () => {
      const { call, deps } = setup();
      expect((await call("files.read", { path: "README.md" }))?.result).toBe("text of README.md");
      expect(deps.readFile).toHaveBeenCalledWith("C:/projects/game", "README.md");
      expect(deps.readHiddenFile).not.toHaveBeenCalled();
      await call("files.readBinary", { path: "shots\\a.png" });
      expect(deps.readBinaryFile).toHaveBeenCalledWith("C:/projects/game", "shots/a.png", false);
    });

    it("still lists them: their names and sizes are what a kit starts from", async () => {
      const { call, deps } = setup();
      expect((await call("files.list", { path: ".github" }))?.result).toEqual(LIST_ANSWER);
      expect(deps.listFiles).toHaveBeenCalledWith("C:/projects/game", ".github", null);
    });
  });

  describe("with files.read and files.write together", () => {
    beforeEach(() => {
      granted = ["files.read", "files.write"];
    });

    it("reads them through the door that may", async () => {
      const { call, deps } = setup();
      for (const path of HIDDEN) {
        expect((await call("files.read", { path }))?.result, path).toBe(`hidden text of ${path}`);
        expect(deps.readHiddenFile).toHaveBeenLastCalledWith("C:/projects/game", path);
      }
      expect(deps.readFile).not.toHaveBeenCalled();
      await call("files.readBinary", { path: ".shots/a.png" });
      expect(deps.readBinaryFile).toHaveBeenCalledWith("C:/projects/game", ".shots/a.png", true);
    });

    it("never reaches .git, whatever else it may", async () => {
      const { call, deps } = setup();
      for (const path of GIT) {
        expect(errorOf(await call("files.read", { path })), path).toBe("invalid");
        expect(errorOf(await call("files.readBinary", { path: `${path}/a.png` })), path).toBe("invalid");
        expect(errorOf(await call("files.write", { path, text: "x" })), path).toBe("invalid");
        expect(errorOf(await call("files.list", { path })), path).toBe("invalid");
      }
      expect(deps.readHiddenFile).not.toHaveBeenCalled();
      expect(deps.readBinaryFile).not.toHaveBeenCalled();
      expect(deps.writeFile).not.toHaveBeenCalled();
      expect(deps.listFiles).not.toHaveBeenCalled();
    });

    it("hands on what Rust says of a missing file, which a page tells apart", async () => {
      const { call } = setup({
        readHiddenFile: vi.fn(async () => {
          throw { code: "not_found", message: ".env was not found in the workspace folder" };
        }),
      });
      expect((await call("files.read", { path: ".env" }))?.error).toEqual({
        code: "not_found",
        message: ".env was not found in the workspace folder",
      });
    });

    it("stops the next call a grant is taken back", async () => {
      const { call, deps } = setup();
      expect((await call("files.read", { path: ".env" }))?.result).toBe("hidden text of .env");
      granted = ["files.read"];
      expect(errorOf(await call("files.read", { path: ".env" }))).toBe("invalid");
      expect(deps.readHiddenFile).toHaveBeenCalledTimes(1);
    });

    it("does not change what preview.show opens", async () => {
      granted = ["files.read", "files.write", "preview.show"];
      const { call, deps } = setup();
      expect(errorOf(await call("preview.show", { path: ".env" }))).toBe("invalid");
      expect(deps.showFile).not.toHaveBeenCalled();
    });
  });

  it("files.write alone reads nothing", async () => {
    granted = ["files.write"];
    const { call, deps } = setup();
    expect(errorOf(await call("files.read", { path: ".env" }))).toBe("denied");
    expect(errorOf(await call("files.readBinary", { path: "a.png" }))).toBe("denied");
    expect(errorOf(await call("files.list"))).toBe("denied");
    expect(deps.readHiddenFile).not.toHaveBeenCalled();
  });
});

describe("files.write", () => {
  beforeEach(() => {
    granted = ["files.write"];
  });

  it("hands Rust the normalized path, the text, and what the page last read", async () => {
    const { call, deps } = setup();
    expect((await call("files.write", { path: "config\\./app.toml", text: "a=1\r\n" }))?.result).toEqual({});
    expect(deps.writeFile).toHaveBeenLastCalledWith("C:/projects/game", "config/app.toml", "a=1\r\n", null);
    await call("files.write", { path: ".env", text: "", expected: "A=1\n" });
    expect(deps.writeFile).toHaveBeenLastCalledWith("C:/projects/game", ".env", "", "A=1\n");
    // An empty text is what the page may have read.
    await call("files.write", { path: ".env", text: "x", expected: "" });
    expect(deps.writeFile).toHaveBeenLastCalledWith("C:/projects/game", ".env", "x", "");
  });

  it("refuses what leaves the folder before anything is done", async () => {
    const { call, deps } = setup();
    for (const path of [
      "",
      "   ",
      42,
      undefined,
      "/etc/passwd",
      "\\\\server\\share\\x.txt",
      "C:/Windows/win.ini",
      "C:\\Windows\\win.ini",
      "../secret.txt",
      "src/../../secret.txt",
      "src\\..\\..\\secret.txt",
      "README.md:stream",
      "README.md.",
      "a\0b",
      ".",
      "x/".repeat(600),
    ]) {
      expect(errorOf(await call("files.write", { path, text: "x" })), String(path)).toBe("invalid");
    }
    expect(deps.writeFile).not.toHaveBeenCalled();
  });

  it("takes a text of a megabyte at most, and what was read the same", async () => {
    const { call, deps } = setup();
    const atCap = "a".repeat(KIT_FILE_MAX_BYTES);
    expect(errorOf(await call("files.write", { path: "a.txt", text: atCap }))).toBeUndefined();
    expect(errorOf(await call("files.write", { path: "a.txt", text: `${atCap}a` }))).toBe("invalid");
    // In bytes, not in characters.
    expect(errorOf(await call("files.write", { path: "a.txt", text: "é".repeat(KIT_FILE_MAX_BYTES / 2 + 1) }))).toBe(
      "invalid",
    );
    expect(errorOf(await call("files.write", { path: "a.txt", text: "x", expected: `${atCap}a` }))).toBe("invalid");
    expect(deps.writeFile).toHaveBeenCalledTimes(1);
  });

  it("needs a text, and an expected that is one", async () => {
    const { call, deps } = setup();
    for (const params of [
      { path: "a.txt" },
      { path: "a.txt", text: 3 },
      { path: "a.txt", text: null },
      { path: "a.txt", text: "x", expected: 3 },
      { path: "a.txt", text: "x", expected: null },
    ]) {
      expect(errorOf(await call("files.write", params)), JSON.stringify(params)).toBe("invalid");
    }
    expect(deps.writeFile).not.toHaveBeenCalled();
  });

  it("says so when the file changed since, and it is the page's to tell apart", async () => {
    const { call } = setup({
      writeFile: vi.fn(async () => {
        throw { code: "conflict", message: ".env changed since it was read: nothing was written" };
      }),
    });
    expect((await call("files.write", { path: ".env", text: "x", expected: "y" }))?.error).toEqual({
      code: "conflict",
      message: ".env changed since it was read: nothing was written",
    });
  });

  it(`writes at most ${KIT_FILES_WRITE_MAX} a minute, apart from data.write's`, async () => {
    granted = ["files.write", "data.write"];
    const { call, deps } = setup();
    for (let i = 0; i < KIT_FILES_WRITE_MAX; i += 1) {
      expect(errorOf(await call("files.write", { path: "a.txt", text: `${i}` }))).toBeUndefined();
    }
    expect(errorOf(await call("files.write", { path: "a.txt", text: "over" }))).toBe("rate_limited");
    expect(deps.writeFile).toHaveBeenCalledTimes(KIT_FILES_WRITE_MAX);
    expect(errorOf(await call("data.write", { path: "a.txt", text: "x" }))).toBeUndefined();
    clock += KIT_FILES_WRITE_WINDOW_MS;
    expect(errorOf(await call("files.write", { path: "a.txt", text: "later" }))).toBeUndefined();
  });

  it("does not spend the quota on calls that fail their checks", async () => {
    const { call } = setup();
    for (let i = 0; i < KIT_FILES_WRITE_MAX * 2; i += 1) await call("files.write", { path: "../x", text: "x" });
    expect(errorOf(await call("files.write", { path: "a.txt", text: "x" }))).toBeUndefined();
  });

  it("answers unavailable once the workspace is gone", async () => {
    const { call, deps } = setup();
    current = null;
    expect(errorOf(await call("files.write", { path: "a.txt", text: "x" }))).toBe("unavailable");
    expect(deps.writeFile).not.toHaveBeenCalled();
  });

  it("holds its paths to the rule the window and Rust share", () => {
    expect(projectRelativePath("./a//b\\.env")).toBe("a/b/.env");
    expect(projectRelativePath(".gitignore")).toBe(".gitignore");
    expect(projectRelativePath(".github/ci.yml")).toBe(".github/ci.yml");
    expect(() => projectRelativePath(".git/config")).toThrow("under .git");
    expect(() => projectRelativePath("a/.GIT")).toThrow("under .git");
  });
});

describe("files.writeBinary", () => {
  beforeEach(() => {
    granted = ["files.write"];
  });

  it("hands Rust the normalized path and the base64, and nothing else", async () => {
    const { call, deps } = setup();
    expect((await call("files.writeBinary", { path: ".hangar\\annotations/./1.png", base64: "iVBORw==" }))?.result).toEqual({});
    expect(deps.writeBinaryFile).toHaveBeenCalledWith("C:/projects/game", ".hangar/annotations/1.png", "iVBORw==");
  });

  it("refuses a path that is not the project's, or goes under .git", async () => {
    const { call, deps } = setup();
    for (const path of ["../x.png", "/abs.png", "C:/x.png", ".git/x.png", "a/.GIT/x.png", "a:b.png", "", undefined, 3]) {
      expect(errorOf(await call("files.writeBinary", { path, base64: "iVBORw==" })), String(path)).toBe("invalid");
    }
    expect(deps.writeBinaryFile).not.toHaveBeenCalled();
  });

  it("refuses a base64 that is not a string, or too long for 10 MB", async () => {
    const { call, deps } = setup();
    for (const base64 of [undefined, 3, null, ["a"]]) {
      expect(errorOf(await call("files.writeBinary", { path: "a.png", base64 })), String(base64)).toBe("invalid");
    }
    const atCap = "A".repeat(Math.ceil(KIT_BINARY_MAX_BYTES / 3) * 4);
    expect(errorOf(await call("files.writeBinary", { path: "a.png", base64: `${atCap}A` }))).toBe("invalid");
    expect(deps.writeBinaryFile).not.toHaveBeenCalled();
    expect(errorOf(await call("files.writeBinary", { path: "a.png", base64: atCap }))).toBeUndefined();
  });

  it("says conflict when the file exists, and invalid when Rust says the content is no image", async () => {
    const { call } = setup({
      writeBinaryFile: vi.fn(async () => {
        throw { code: "conflict", message: "a.png already exists: nothing was written" };
      }),
    });
    expect((await call("files.writeBinary", { path: "a.png", base64: "iVBORw==" }))?.error).toEqual({
      code: "conflict",
      message: "a.png already exists: nothing was written",
    });
    const refused = setup({
      writeBinaryFile: vi.fn(async () => {
        throw { code: "invalid", message: "the content is not a png image" };
      }),
    });
    expect(errorOf(await refused.call("files.writeBinary", { path: "a.png", base64: "AAAA" }))).toBe("invalid");
  });

  it("shares files.write's quota, and a failed check spends none", async () => {
    const { call, deps } = setup();
    for (let i = 0; i < KIT_FILES_WRITE_MAX * 2; i += 1) await call("files.writeBinary", { path: "../x.png", base64: "AAAA" });
    for (let i = 0; i < KIT_FILES_WRITE_MAX - 1; i += 1) {
      expect(errorOf(await call("files.write", { path: "a.txt", text: `${i}` }))).toBeUndefined();
    }
    expect(errorOf(await call("files.writeBinary", { path: "a.png", base64: "iVBORw==" }))).toBeUndefined();
    expect(errorOf(await call("files.writeBinary", { path: "b.png", base64: "iVBORw==" }))).toBe("rate_limited");
    expect(errorOf(await call("files.write", { path: "a.txt", text: "over" }))).toBe("rate_limited");
    expect(deps.writeBinaryFile).toHaveBeenCalledTimes(1);
  });

  it("answers unavailable once the workspace is gone", async () => {
    const { call, deps } = setup();
    current = null;
    expect(errorOf(await call("files.writeBinary", { path: "a.png", base64: "iVBORw==" }))).toBe("unavailable");
    expect(deps.writeBinaryFile).not.toHaveBeenCalled();
  });
});

describe("files.readBinary", () => {
  beforeEach(() => {
    granted = ["files.read"];
  });

  it("answers what Rust read", async () => {
    const { call, deps } = setup();
    expect((await call("files.readBinary", { path: "shots/a.png" }))?.result).toEqual(BINARY_ANSWER);
    expect(deps.readBinaryFile).toHaveBeenCalledWith("C:/projects/game", "shots/a.png", false);
  });

  it("refuses a path that leaves the folder", async () => {
    const { call, deps } = setup();
    for (const path of ["", 42, undefined, "/x.png", "C:/x.png", "../x.png", "a/../../x.png", "x.png:s", ".hidden.png"]) {
      expect(errorOf(await call("files.readBinary", { path })), String(path)).toBe("invalid");
    }
    expect(errorOf(await call("files.readBinary"))).toBe("invalid");
    expect(deps.readBinaryFile).not.toHaveBeenCalled();
  });

  it("hands on a type Rust does not read, by its code", async () => {
    const { call } = setup({
      readBinaryFile: vi.fn(async () => {
        throw { code: "unsupported", message: "a.txt is not an image or a PDF" };
      }),
    });
    expect((await call("files.readBinary", { path: "a.txt" }))?.error).toEqual({
      code: "unsupported",
      message: "a.txt is not an image or a PDF",
    });
  });
});

describe("files.list", () => {
  beforeEach(() => {
    granted = ["files.read"];
  });

  it("lists the workspace folder when given no folder", async () => {
    const { call, deps } = setup();
    for (const params of [undefined, {}, { path: "." }, { path: "./" }, { path: ".\\" }]) {
      expect((await call("files.list", params))?.result, JSON.stringify(params)).toEqual(LIST_ANSWER);
      expect(deps.listFiles).toHaveBeenLastCalledWith("C:/projects/game", null, null);
    }
  });

  it("hands Rust a normalized folder and the pattern", async () => {
    const { call, deps } = setup();
    await call("files.list", { path: "config\\deep/", pattern: "*.json" });
    expect(deps.listFiles).toHaveBeenLastCalledWith("C:/projects/game", "config/deep", "*.json");
    await call("files.list", { pattern: ".env*" });
    expect(deps.listFiles).toHaveBeenLastCalledWith("C:/projects/game", null, ".env*");
  });

  it("refuses a folder that leaves the workspace, and a pattern that is not one", async () => {
    const { call, deps } = setup();
    for (const path of ["", "   ", 42, null, "/etc", "C:/Windows", "..", "../x", "./..", "a/../..", "a:b"]) {
      expect(errorOf(await call("files.list", { path })), String(path)).toBe("invalid");
    }
    for (const pattern of ["", "   ", 42, null, "*".repeat(KIT_LIST_PATTERN_MAX_CHARS + 1)]) {
      expect(errorOf(await call("files.list", { pattern })), String(pattern)).toBe("invalid");
    }
    expect(errorOf(await call("files.list", "src"))).toBe("invalid");
    expect(deps.listFiles).not.toHaveBeenCalled();
  });

  it("answers unavailable once the workspace is gone", async () => {
    const { call } = setup();
    current = null;
    expect(errorOf(await call("files.list"))).toBe("unavailable");
  });
});

describe("panes.send reaches the workspace's Claude Code agents only", () => {
  it("queues the text for the agent, labelled as the kit's", async () => {
    const { call, deps } = setup();
    const answer = await call("panes.send", { pane: "ava", text: "  Build the place\n" });
    expect(answer?.result).toEqual({ queued: true, pane: "Ava" });
    expect(deps.enqueue).toHaveBeenCalledWith("p-ava", "  Build the place\n", kitSender("hello-kit"));
    expect(kitSender("hello-kit")).toBe("kit:hello-kit");
  });

  it("refuses another agent, a shell, a pane of another workspace, and an agent without the bridge", async () => {
    const { call, deps } = setup();
    expect((await call("panes.send", { pane: "Ben", text: "hi" }))?.error?.code).toBe("denied");
    expect((await call("panes.send", { pane: "Shell", text: "ls" }))?.error?.code).toBe("denied");
    expect((await call("panes.send", { pane: "Dan", text: "hi" }))?.error?.code).toBe("not_found");
    expect((await call("panes.send", { pane: "p-ava", text: "hi" }))?.error?.code).toBe("not_found");
    expect((await call("panes.send", { pane: "Cleo", text: "hi" }))?.error?.code).toBe("unavailable");
    expect(deps.enqueue).not.toHaveBeenCalled();
  });

  it(`sends at most ${KIT_SEND_MAX} a minute`, async () => {
    const { call, deps } = setup();
    for (let i = 0; i < KIT_SEND_MAX; i += 1) {
      expect(errorOf(await call("panes.send", { pane: "Ava", text: `n${i}` }))).toBeUndefined();
    }
    const over = await call("panes.send", { pane: "Ava", text: "one more" });
    expect(errorOf(over)).toBe("rate_limited");
    expect(deps.enqueue).toHaveBeenCalledTimes(KIT_SEND_MAX);
    // A refused call does not count: the window frees up on time.
    clock += KIT_SEND_WINDOW_MS;
    expect(errorOf(await call("panes.send", { pane: "Ava", text: "later" }))).toBeUndefined();
  });

  it("does not spend the quota on calls that fail their checks", async () => {
    const { call } = setup();
    for (let i = 0; i < KIT_SEND_MAX * 2; i += 1) await call("panes.send", { pane: "Ben", text: "hi" });
    expect(errorOf(await call("panes.send", { pane: "Ava", text: "hi" }))).toBeUndefined();
  });

  it(`raises at most ${KIT_NOTIFY_MAX} notifications a minute`, async () => {
    const { call, deps } = setup();
    for (let i = 0; i < KIT_NOTIFY_MAX; i += 1) await call("notify", { title: `n${i}` });
    expect(errorOf(await call("notify", { title: "one more" }))).toBe("rate_limited");
    expect(deps.notify).toHaveBeenCalledTimes(KIT_NOTIFY_MAX);
  });
});

describe("what a tab sees", () => {
  it("lists the panes in order, with what panes.send can reach", () => {
    expect(kitPanes(workspace, (id) => activities[id])).toEqual([
      { name: "Ava", agent: "claude", role: "orchestrator", state: "working", canSend: true },
      { name: "Ben", agent: "codex", role: null, state: "idle", canSend: false },
      { name: "Cleo", agent: "claude", role: null, state: "idle", canSend: false },
      { name: "Shell", agent: "shell", role: null, state: null, canSend: false },
    ]);
  });

  it("says the workspace is gone rather than answering for another", async () => {
    const { call } = setup();
    current = null;
    expect(errorOf(await call("workspace.info"))).toBe("unavailable");
    expect(errorOf(await call("panes.list"))).toBe("unavailable");
  });

  it("tells panes.changed only to a tab granted panes.read", async () => {
    const { host, sent } = setup();
    host.emit("panes.changed", {});
    expect(sent).toEqual([{ hangar: 1, event: "panes.changed", data: {} }]);
    granted = ["workspace.read"];
    host.emit("panes.changed", {});
    expect(sent).toHaveLength(1);
  });

  it(`answers at most ${KIT_MAX_PENDING} calls at once`, async () => {
    const { host, frame, sent } = setup({ readFile: vi.fn(() => new Promise<string>(() => undefined)) });
    for (let i = 0; i <= KIT_MAX_PENDING; i += 1) {
      host.receive({
        source: frame,
        origin: "null",
        data: { hangar: 1, id: `f${i}`, method: "files.read", params: { path: "a.txt" } },
      });
    }
    await settle();
    expect(sent).toEqual([
      {
        hangar: 1,
        id: `f${KIT_MAX_PENDING}`,
        error: { code: "rate_limited", message: `at most ${KIT_MAX_PENDING} calls at once` },
      },
    ]);
  });

  it("answers with the request's own id, a number included", async () => {
    const { host, frame, sent } = setup();
    host.receive({ source: frame, origin: "null", data: { hangar: 1, id: 12, method: "workspace.info" } });
    await settle();
    expect(sent[0]).toEqual({ hangar: 1, id: 12, result: CALLS["workspace.info"].result });
  });
});

describe("a kit's own data", () => {
  it("reads and writes under the host's own record of the tab, whatever the message says", async () => {
    const { call, deps } = setup();
    granted = ["data.read", "data.write"];
    await call("data.read", { path: "a.txt", kit: "other-kit", kitId: "other-kit" });
    expect(deps.readData).toHaveBeenCalledWith("hello-kit", "a.txt");
    await call("data.write", { path: "a.txt", text: "x", kit: "other-kit", kitId: "other-kit" });
    expect(deps.writeData).toHaveBeenCalledWith("hello-kit", "a.txt", "x");
  });

  it("holds each permission apart: reading does not let it write, nor writing read", async () => {
    const { call, deps } = setup();
    granted = ["data.read"];
    expect(errorOf(await call("data.write", { path: "a.txt", text: "x" }))).toBe("denied");
    expect(deps.writeData).not.toHaveBeenCalled();
    granted = ["data.write"];
    expect(errorOf(await call("data.read", { path: "a.txt" }))).toBe("denied");
    expect(deps.readData).not.toHaveBeenCalled();
  });

  it("takes a relative path inside the kit's folder only", async () => {
    const { call, deps } = setup();
    granted = ["data.read", "data.write"];
    for (const path of [
      "",
      "   ",
      42,
      undefined,
      null,
      "/etc/passwd",
      "\\\\server\\share\\x.txt",
      "C:/Windows/win.ini",
      "C:\\Windows\\win.ini",
      "../other-kit/state.json",
      "a/../../other-kit/state.json",
      "a\\..\\..\\other-kit\\state.json",
      "state.json:stream",
      "state.json.",
      "a\0b",
      ".",
      "x/".repeat(600),
    ]) {
      expect(errorOf(await call("data.read", { path })), String(path)).toBe("invalid");
      expect(errorOf(await call("data.write", { path, text: "x" })), String(path)).toBe("invalid");
    }
    expect(deps.readData).not.toHaveBeenCalled();
    expect(deps.writeData).not.toHaveBeenCalled();
    for (const params of [undefined, null, "a.txt", ["a.txt"], 3]) {
      expect(errorOf(await call("data.read", params))).toBe("invalid");
      expect(errorOf(await call("data.write", params))).toBe("invalid");
    }
  });

  it("hands Rust the path normalized, and lets a kit keep hidden names in its own folder", async () => {
    const { call, deps } = setup();
    granted = ["data.read"];
    await call("data.read", { path: "queue\\today/./log.txt" });
    expect(deps.readData).toHaveBeenCalledWith("hello-kit", "queue/today/log.txt");
    expect(dataRelativePath(".cache/state")).toBe(".cache/state");
  });

  it("writes a text, of a megabyte at most, counted in bytes", async () => {
    const { call, deps } = setup();
    granted = ["data.write"];
    expect(errorOf(await call("data.write", { path: "a.txt" }))).toBe("invalid");
    expect(errorOf(await call("data.write", { path: "a.txt", text: 3 }))).toBe("invalid");
    const over = await call("data.write", { path: "a.txt", text: "x".repeat(KIT_DATA_MAX_BYTES + 1) });
    expect(over?.error?.code).toBe("invalid");
    expect(over?.error?.message).toContain("1 MB");
    // Fits by characters, not by bytes: é is two.
    expect(errorOf(await call("data.write", { path: "a.txt", text: "é".repeat(KIT_DATA_MAX_BYTES / 2 + 1) }))).toBe(
      "invalid",
    );
    expect(deps.writeData).not.toHaveBeenCalled();
    expect(errorOf(await call("data.write", { path: "a.txt", text: "x".repeat(KIT_DATA_MAX_BYTES) }))).toBeUndefined();
    expect(errorOf(await call("data.write", { path: "empty.txt", text: "" }))).toBeUndefined();
  });

  it("keeps Rust's code: absent is not_found, and a refused path says why", async () => {
    const { call } = setup({
      readData: vi.fn(async () => {
        throw { code: "not_found", message: "a.txt is not in this kit's data" };
      }),
      writeData: vi.fn(async () => {
        throw { code: "failed", message: "away/a.txt leads outside the kit's data folder" };
      }),
    });
    granted = ["data.read", "data.write"];
    expect((await call("data.read", { path: "a.txt" }))?.error).toEqual({
      code: "not_found",
      message: "a.txt is not in this kit's data",
    });
    expect((await call("data.write", { path: "away/a.txt", text: "x" }))?.error).toEqual({
      code: "failed",
      message: "away/a.txt leads outside the kit's data folder",
    });
  });

  it("says failed for what comes without a code", async () => {
    const { call } = setup({
      readData: vi.fn(async () => {
        throw "the app is closing";
      }),
    });
    granted = ["data.read"];
    expect((await call("data.read", { path: "a.txt" }))?.error).toEqual({
      code: "failed",
      message: "the app is closing",
    });
  });

  it(`writes at most ${KIT_DATA_WRITE_MAX} a minute, and reads as often as asked`, async () => {
    const { call, deps } = setup();
    granted = ["data.read", "data.write"];
    for (let i = 0; i < KIT_DATA_WRITE_MAX; i += 1) {
      expect(errorOf(await call("data.write", { path: "a.txt", text: `${i}` }))).toBeUndefined();
    }
    expect(errorOf(await call("data.write", { path: "a.txt", text: "more" }))).toBe("rate_limited");
    expect(deps.writeData).toHaveBeenCalledTimes(KIT_DATA_WRITE_MAX);
    // A bad call spends nothing: it is refused for what it is.
    expect(errorOf(await call("data.write", { path: "../a.txt", text: "x" }))).toBe("invalid");

    // Reading has no such limit: a tab polling is never held back, here with
    // the write limit reached and no time gone by at all.
    for (let i = 0; i < KIT_DATA_WRITE_MAX * 2; i += 1) {
      expect(errorOf(await call("data.read", { path: "a.txt" }))).toBeUndefined();
    }
    clock += KIT_DATA_WRITE_WINDOW_MS;
    expect(errorOf(await call("data.write", { path: "a.txt", text: "later" }))).toBeUndefined();
    expect(KIT_DATA_WRITE_WINDOW_MS).toBe(60_000);
  });

  it("does not answer a foreign source, nor spend anything for it", async () => {
    const { host, sent, deps } = setup();
    granted = ["data.read", "data.write", "loopback:8765"];
    const stranger = makeFrame().frame;
    for (const [method, params] of [
      ["data.read", { path: "a.txt" }],
      ["data.write", { path: "a.txt", text: "x" }],
      ["loopback.fetch", { port: 8765, path: "/" }],
    ] as const) {
      host.receive({ source: stranger, origin: "null", data: { hangar: 1, id: "x", method, params } });
    }
    await settle();
    expect(sent).toEqual([]);
    expect(deps.readData).not.toHaveBeenCalled();
    expect(deps.writeData).not.toHaveBeenCalled();
    expect(deps.loopbackFetch).not.toHaveBeenCalled();
  });
});

describe("loopback.fetch", () => {
  beforeEach(() => {
    granted = ["loopback:8765"];
  });

  it("asks Rust for a GET by default, with the default timeout", async () => {
    const { call, deps } = setup();
    const answer = await call("loopback.fetch", { port: 8765, path: "/queue?since=1" });
    expect(answer?.result).toEqual({ status: 200, text: "{}" });
    expect(deps.loopbackFetch).toHaveBeenCalledWith({
      port: 8765,
      path: "/queue?since=1",
      method: "GET",
      body: null,
      timeoutMs: KIT_LOOPBACK_TIMEOUT_MS,
    });
  });

  it("carries a POST's body and timeout, and nothing else of the page's", async () => {
    const { call, deps } = setup();
    await call("loopback.fetch", {
      port: 8765,
      path: "/run",
      method: "POST",
      body: "{\"go\":true}",
      timeoutMs: 15_000,
      headers: { Cookie: "a=b", Host: "example.com" },
      host: "example.com",
      url: "http://example.com/",
    });
    expect(deps.loopbackFetch).toHaveBeenCalledWith({
      port: 8765,
      path: "/run",
      method: "POST",
      body: "{\"go\":true}",
      timeoutMs: 15_000,
    });
  });

  it("refuses a port not granted, even with another loopback granted", async () => {
    const { call, deps } = setup();
    granted = ["loopback:8765", "loopback:9000"];
    expect(errorOf(await call("loopback.fetch", { port: 9000, path: "/" }))).toBeUndefined();
    const other = await call("loopback.fetch", { port: 8766, path: "/" });
    expect(other?.error).toEqual({ code: "denied", message: "this tab was not granted loopback:8766" });
    expect(errorOf(await call("loopback.fetch", { port: 80, path: "/" }))).toBe("invalid");
    expect(deps.loopbackFetch).toHaveBeenCalledTimes(1);
  });

  it("reads the port from the permission as written: another grant does not stand in for it", async () => {
    const { call, deps } = setup();
    granted = ["data.read", "panes.send", "notify", "loopback:80", "loopback:08765"];
    for (const port of [8765, 80]) {
      expect(errorOf(await call("loopback.fetch", { port, path: "/" }))).toBe("denied");
    }
    expect(deps.loopbackFetch).not.toHaveBeenCalled();
  });

  it("tells a tab with no loopback at all only that it was not granted one", async () => {
    const { call } = setup();
    granted = ["notify"];
    const answer = await call("loopback.fetch", { port: "abc", path: "/" });
    expect(answer?.error).toEqual({ code: "denied", message: `this tab was not granted ${KIT_LOOPBACK_PATTERN}` });
    expect(errorOf(await call("loopback.fetch", undefined))).toBe("denied");
  });

  it("refuses a port that is no port", async () => {
    const { call, deps } = setup();
    for (const port of [undefined, null, "8765", 8765.5, 1023, 0, -1, 65536, Number.NaN, [8765], {}]) {
      expect(errorOf(await call("loopback.fetch", { port, path: "/" })), String(port)).toBe("invalid");
    }
    for (const params of [undefined, null, "/queue", ["/queue"], 3]) {
      expect(errorOf(await call("loopback.fetch", params))).toBe("invalid");
    }
    expect(deps.loopbackFetch).not.toHaveBeenCalled();
  });

  it("takes a path that cannot change the host", async () => {
    const { call, deps } = setup();
    for (const path of [
      undefined,
      3,
      "",
      "queue",
      "//example.com/queue",
      "http://example.com/",
      "/a b",
      "/a\r\nHost: example.com",
      "/a\\b",
      "/\\example.com",
      "/é",
      "/a#b",
      `/${"a".repeat(KIT_LOOPBACK_PATH_MAX_CHARS)}`,
    ]) {
      expect(errorOf(await call("loopback.fetch", { port: 8765, path })), String(path)).toBe("invalid");
    }
    expect(deps.loopbackFetch).not.toHaveBeenCalled();
  });

  it("takes GET or POST, a body on a POST only, and a timeout up to 15 seconds", async () => {
    const { call, deps } = setup();
    const at = (patch: object) => call("loopback.fetch", { port: 8765, path: "/", ...patch });
    for (const method of ["PUT", "DELETE", "get", "", 3, null]) {
      expect(errorOf(await at({ method })), String(method)).toBe("invalid");
    }
    expect(errorOf(await at({ body: "x" }))).toBe("invalid");
    expect(errorOf(await at({ method: "GET", body: "x" }))).toBe("invalid");
    expect(errorOf(await at({ method: "POST", body: 3 }))).toBe("invalid");
    expect(errorOf(await at({ method: "POST", body: "x".repeat(KIT_LOOPBACK_MAX_BODY_BYTES + 1) }))).toBe("invalid");
    for (const timeoutMs of [0, -1, 1.5, "5000", null, KIT_LOOPBACK_MAX_TIMEOUT_MS + 1]) {
      expect(errorOf(await at({ timeoutMs })), String(timeoutMs)).toBe("invalid");
    }
    expect(deps.loopbackFetch).not.toHaveBeenCalled();
    expect(errorOf(await at({ timeoutMs: KIT_LOOPBACK_MAX_TIMEOUT_MS }))).toBeUndefined();
  });

  it("answers unreachable when nothing is there, and never logs it", async () => {
    const warn = vi.spyOn(console, "warn");
    const error = vi.spyOn(console, "error");
    const log = vi.spyOn(console, "log");
    const { call } = setup({
      loopbackFetch: vi.fn(async () => {
        throw { code: "unreachable", message: "nothing answers on port 8765" };
      }),
    });
    expect((await call("loopback.fetch", { port: 8765, path: "/" }))?.error).toEqual({
      code: "unreachable",
      message: "nothing answers on port 8765",
    });
    expect(warn).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    expect(log).not.toHaveBeenCalled();
    vi.restoreAllMocks();
  });

  it("is not held back when polled: it has no limit of its own", async () => {
    const { call, deps } = setup();
    const polls = KIT_SEND_MAX * 2;
    for (let i = 0; i < polls; i += 1) {
      expect(errorOf(await call("loopback.fetch", { port: 8765, path: "/queue" }))).toBeUndefined();
    }
    expect(deps.loopbackFetch).toHaveBeenCalledTimes(polls);
  });
});

describe("command.run", () => {
  const RUN = { name: "ci-status", args: { run: "42", repo: "o/r" } };
  const GRANT = "commands:0123456789abcdef";

  beforeEach(() => {
    granted = ["command:ci-status"];
  });

  it("asks Rust for that command, with the kit, the folder and the grant of the host's own record", async () => {
    commandsAccepted = GRANT;
    const { call, deps } = setup();
    const answer = await call("command.run", {
      ...RUN,
      kit: "another-kit",
      cwd: "C:/elsewhere",
      accepted: "commands:ffffffffffffffff",
    });
    expect(answer?.result).toEqual(COMMAND_ANSWER);
    expect(deps.runCommand).toHaveBeenCalledTimes(1);
    expect(deps.runCommand).toHaveBeenCalledWith({
      kit: "hello-kit",
      name: "ci-status",
      args: { run: "42", repo: "o/r" },
      cwd: "C:/projects/game",
      accepted: GRANT,
    });
  });

  it("hands back whatever the program answered: a code, a timeout, a cut", async () => {
    const answered = { exitCode: null, stdout: "partial", stderr: "", timedOut: true, truncated: true };
    const { call } = setup({ runCommand: vi.fn(async () => answered) });
    expect((await call("command.run", RUN))?.result).toEqual(answered);
  });

  it("takes a command without args", async () => {
    const { call, deps } = setup();
    expect(errorOf(await call("command.run", { name: "ci-status" }))).toBeUndefined();
    expect(vi.mocked(deps.runCommand).mock.calls[0][0].args).toEqual({});
  });

  it("does not let another command's permission open it", async () => {
    granted = ["command:deploy", "workspace.read", "loopback:8765"];
    const { call, deps } = setup();
    const answer = await call("command.run", RUN);
    expect(answer?.error).toEqual({ code: "denied", message: "this tab was not granted command:ci-status" });
    expect(deps.runCommand).not.toHaveBeenCalled();
  });

  it("does not take the pattern's own spelling, or a grant of the kit's commands, for a permission", async () => {
    for (const held of [[KIT_COMMAND_PATTERN], [GRANT], ["commands:ci-status"], ["command:ci-status2"]]) {
      granted = held;
      const { call, deps } = setup();
      expect(errorOf(await call("command.run", RUN))).toBe("denied");
      expect(deps.runCommand).not.toHaveBeenCalled();
    }
  });

  it("answers a message from another frame or with an origin with nothing, and runs nothing", async () => {
    const { host, sent, deps } = setup();
    const request = { hangar: 1, id: "x", method: "command.run", params: RUN };
    host.receive({ source: makeFrame().frame, origin: "null", data: request });
    host.receive({ source: null, origin: "null", data: request });
    await settle();
    expect(sent).toEqual([]);
    expect(deps.runCommand).not.toHaveBeenCalled();
  });

  it("refuses a name that names no command, and tells a tab holding none only that", async () => {
    const { call, deps } = setup();
    for (const name of [undefined, 3, "", "CI", "ci status", "../x", "command:ci-status", null]) {
      expect(errorOf(await call("command.run", { name, args: {} }))).toBe("invalid");
    }
    expect(errorOf(await call("command.run"))).toBe("invalid");
    expect(errorOf(await call("command.run", "ci-status"))).toBe("invalid");

    granted = ["panes.read"];
    const refused = await call("command.run", { name: 3 });
    expect(refused?.error).toEqual({ code: "denied", message: "this tab was not granted command:<name>" });
    expect(deps.runCommand).not.toHaveBeenCalled();
  });

  it("refuses args that are not an object, and a value that is not a string", async () => {
    const { call, deps } = setup();
    for (const args of [[], "run=42", 42, null, ["42"]]) {
      expect(errorOf(await call("command.run", { name: "ci-status", args }))).toBe("invalid");
    }
    for (const value of [42, null, true, { a: 1 }, ["42"]]) {
      const answer = await call("command.run", { name: "ci-status", args: { run: value } });
      expect(answer?.error).toEqual({ code: "bad_args", message: "run must be a string" });
    }
    expect(deps.runCommand).not.toHaveBeenCalled();
  });

  it("keeps a key called __proto__ a key, for Rust to refuse", async () => {
    const { call, deps } = setup();
    const args = JSON.parse('{"__proto__":"x","run":"42"}') as Record<string, string>;
    await call("command.run", { name: "ci-status", args });
    const sent = vi.mocked(deps.runCommand).mock.calls[0][0].args;
    expect(Object.keys(sent).sort()).toEqual(["__proto__", "run"]);
    expect(Object.getPrototypeOf(sent)).toBe(Object.prototype);
  });

  it("keeps the codes Rust refuses with, and logs nothing for a program that is not installed", async () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => undefined);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    for (const code of ["bad_args", "not_found", "busy", "denied", "failed", "unavailable"]) {
      const refusal = { code, message: `because ${code}` };
      const { call } = setup({ runCommand: vi.fn(() => Promise.reject(refusal)) });
      expect((await call("command.run", RUN))?.error).toEqual(refusal);
    }
    expect(log).not.toHaveBeenCalled();
    expect(warn).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    vi.restoreAllMocks();
  });

  it("says so when the workspace is gone", async () => {
    current = null;
    const { call, deps } = setup();
    expect(errorOf(await call("command.run", RUN))).toBe("unavailable");
    expect(deps.runCommand).not.toHaveBeenCalled();
  });

  it("is not held back when polled: it has no limit of its own", async () => {
    const { call, deps } = setup();
    const polls = KIT_SEND_MAX * 2;
    for (let i = 0; i < polls; i += 1) {
      expect(errorOf(await call("command.run", RUN))).toBeUndefined();
    }
    expect(deps.runCommand).toHaveBeenCalledTimes(polls);
  });

  describe("once a kit's commands have to be accepted", () => {
    const CHANGED = "commands:fedcba9876543210";
    const tab = { id: "ci", title: "CI", entry: "tabs/ci.html", permissions: ["command:ci-status" as const] };
    const kitFrom = (source: KitInfo["source"], commandsPermission: string): KitInfo => ({
      id: "hello-kit",
      source,
      dir: "D:/kits/hello-kit",
      installedVersion: "1.0.0",
      addedAt: 1,
      manifest: {
        schema: 1,
        id: "hello-kit",
        name: "Hello kit",
        version: "1.0.0",
        description: "",
        tabs: [tab],
        setup: [],
      },
      error: null,
      update: null,
      commandsPermission,
    });
    const github = (commandsPermission: string) =>
      kitFrom({ kind: "git", url: "https://github.com/o/kits" }, commandsPermission);
    const folder = (commandsPermission: string) =>
      kitFrom({ kind: "folder", path: "D:/kits/hello-kit" }, commandsPermission);

    /** A call from the tab, holding what `workspaceKitTabs` says it may hold. */
    async function runAs(kit: KitInfo, heldByWorkspace: string[]) {
      const [choice] = workspaceKitTabs({ kits: [{ id: "hello-kit", granted: heldByWorkspace }] }, [kit]);
      granted = choice.permissions;
      commandsAccepted = choice.commandsAccepted;
      const { call, deps } = setup();
      return { answer: await call("command.run", RUN), deps };
    }

    it("answers denied for a GitHub kit whose commands changed, until they are accepted again", async () => {
      const { answer, deps } = await runAs(github(CHANGED), ["command:ci-status", GRANT]);
      expect(answer?.error).toEqual({ code: "denied", message: "this tab was not granted command:ci-status" });
      expect(deps.runCommand).not.toHaveBeenCalled();

      const again = await runAs(github(CHANGED), ["command:ci-status", CHANGED]);
      expect(errorOf(again.answer)).toBeUndefined();
      expect(vi.mocked(again.deps.runCommand).mock.calls[0][0].accepted).toBe(CHANGED);
    });

    it("answers denied for a GitHub kit whose commands were never accepted", async () => {
      const { answer, deps } = await runAs(github(GRANT), ["command:ci-status"]);
      expect(errorOf(answer)).toBe("denied");
      expect(deps.runCommand).not.toHaveBeenCalled();
    });

    it("does not ask a linked folder kit, whatever its commands became", async () => {
      const { answer, deps } = await runAs(folder(CHANGED), ["command:ci-status"]);
      expect(errorOf(answer)).toBeUndefined();
      expect(vi.mocked(deps.runCommand).mock.calls[0][0].accepted).toBeNull();
    });
  });
});

describe("hosts are bound to their own frame", () => {
  it("does not answer a second tab's frame from the first tab's host", async () => {
    const first = setup();
    const second = makeFrame();
    const host: KitHost = first.host;
    host.receive({ source: second.frame, origin: "null", data: { hangar: 1, id: "x", method: "panes.list" } });
    await settle();
    expect(first.sent).toEqual([]);
    expect(second.sent).toEqual([]);
  });
});
