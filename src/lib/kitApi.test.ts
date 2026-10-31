import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneActivity } from "./agentState";
import {
  createKitHost,
  kitPanes,
  kitSender,
  workspaceRelativePath,
  KIT_MAX_PENDING,
  KIT_NOTIFY_MAX,
  KIT_SEND_MAX,
  KIT_SEND_MAX_CHARS,
  KIT_SEND_WINDOW_MS,
  type KitFrame,
  type KitHost,
  type KitHostDeps,
} from "./kitApi";
import { KIT_METHODS, KIT_PERMISSIONS, type KitMethod } from "./kits";
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

let clock = 1_000_000;
let granted: string[] = [...KIT_PERMISSIONS];
let current: Workspace | null = workspace;

function makeDeps(overrides: Partial<KitHostDeps> = {}): KitHostDeps {
  return {
    readFile: vi.fn(async (_root: string, path: string) => `text of ${path}`),
    resolveFile: vi.fn(async (root: string, path: string) => `${root}/${path}`),
    showFile: vi.fn(async () => undefined),
    enqueue: vi.fn(async () => ({ id: "q-1" })),
    activity: (paneId: string) => activities[paneId],
    notify: vi.fn(async () => undefined),
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
  granted = [...KIT_PERMISSIONS];
  current = workspace;
});

/** Each method, with params it accepts and what its fake deps answer. */
const CALLS: Record<KitMethod, { params?: unknown; result: unknown }> = {
  "workspace.info": { result: { id: "ws-1", name: "Game", folder: "C:/projects/game" } },
  "panes.list": { result: kitPanes(workspace, (id) => activities[id]) },
  "panes.send": { params: { pane: "Ava", text: "Build the place" }, result: { queued: true, pane: "Ava" } },
  "files.read": { params: { path: "src/main.luau" }, result: "text of src/main.luau" },
  "preview.show": { params: { path: "docs/plan.md" }, result: { opened: true, path: "docs/plan.md" } },
  notify: { params: { title: "Built", body: "The place is ready." }, result: null },
};

describe("each permission, granted and refused", () => {
  for (const [method, { params, result }] of Object.entries(CALLS) as [KitMethod, (typeof CALLS)[KitMethod]][]) {
    const permission = KIT_METHODS[method];

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
      expect(deps.resolveFile).not.toHaveBeenCalled();
      expect(deps.showFile).not.toHaveBeenCalled();
      expect(deps.enqueue).not.toHaveBeenCalled();
      expect(deps.notify).not.toHaveBeenCalled();
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
    const answer = await call("files.write", { path: "x.txt", text: "x" });
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
