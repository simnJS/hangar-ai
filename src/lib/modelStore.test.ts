import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { modelFormatOf, modelUrl } from "./modelFiles";
import {
  activateModel,
  adoptModelScope,
  closeAllModels,
  closeModel,
  closeModels,
  closeWorkspaceModels,
  getModels,
  isKitTab,
  isModel,
  isPreviewFile,
  kitParamsKey,
  kitTabParams,
  kitTabPath,
  markModelsSeen,
  modelScope,
  openKitTab,
  pruneModels,
  reloadKitTab,
  reloadModel,
  setKitTabBadge,
  setKitTabTitle,
  setModelPanel,
  settleModel,
  showPreview,
  takeModelScope,
  watchKitTabParams,
  whenSettled,
  type KitTabToOpen,
  type OpenKitTab,
  type OpenPreview,
} from "./modelStore";

// whenSettled arms a timer through window.setTimeout.
vi.stubGlobal("window", globalThis);

const show = (path: string, openedBy: string | null = "Ava", workspaceId = "ws") =>
  showPreview({ kind: "model", path, format: "glb", workspaceId, openedBy, modifiedMs: 1 });

const info = {
  triangles: 12,
  vertices: 8,
  meshes: 1,
  materials: 1,
  animations: [],
  size: [1, 1, 1] as [number, number, number],
  center: [0, 0, 0] as [number, number, number],
};

describe("modelStore", () => {
  beforeEach(() => closeAllModels());

  it("opens the panel on a model and puts it in front", () => {
    const id = show("C:/m/chair.glb");
    const scope = modelScope("ws");
    expect(scope.panelOpen).toBe(true);
    expect(scope.activeId).toBe(id);
    expect(scope.models[0].title).toBe("chair.glb");
    expect(scope.unseen).toBe(true);
  });

  it("reloads a model shown again instead of opening it twice", () => {
    const first = show("C:/m/chair.glb");
    settleModel(first, { info });
    const again = show("C:/m/chair.glb", "Ben");
    const [model] = modelScope("ws").models;
    expect(again).toBe(first);
    expect(modelScope("ws").models).toHaveLength(1);
    expect(model.revision).toBe(1);
    expect(model.status).toBe("loading");
    expect(model.openedBy).toBe("Ben");
  });

  it("closes the panel with the last model", () => {
    const a = show("C:/m/a.glb");
    const b = show("C:/m/b.glb");
    setModelPanel("ws", { expanded: true });
    closeModel(b);
    expect(modelScope("ws").activeId).toBe(a);
    expect(modelScope("ws").expanded).toBe(true);
    closeModel(a);
    expect(modelScope("ws").panelOpen).toBe(false);
    expect(modelScope("ws").expanded).toBe(false);
    expect(getModels().all).toEqual([]);
  });

  it("tells a waiting caller how the load ended", async () => {
    const id = show("C:/m/a.glb");
    const waiting = whenSettled(id, 1000);
    settleModel(id, { error: "not a glTF file" });
    expect((await waiting)?.error).toBe("not a glTF file");
  });
});

describe("one preview per workspace", () => {
  beforeEach(() => closeAllModels());

  it("keeps each workspace's models, tab in front and panel apart", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const table = show("C:/a/table.glb", "Ava", "A");
    const car = show("C:/b/car.glb", "Ben", "B");
    activateModel(chair);
    setModelPanel("B", { open: false });

    expect(modelScope("A").models.map((model) => model.id)).toEqual([chair, table]);
    expect(modelScope("A").activeId).toBe(chair);
    expect(modelScope("A").panelOpen).toBe(true);
    expect(modelScope("B").models.map((model) => model.id)).toEqual([car]);
    expect(modelScope("B").activeId).toBe(car);
    expect(modelScope("B").panelOpen).toBe(false);
    // The window's viewer holds them all.
    expect(getModels().all.map((model) => model.id).sort()).toEqual([car, chair, table].sort());
  });

  it("has nothing for a workspace no model was shown in, and its panel stays shut", () => {
    show("C:/a/chair.glb", "Ava", "A");
    expect(modelScope("B")).toMatchObject({ models: [], activeId: null, panelOpen: false });
    setModelPanel("B", { open: true });
    expect(modelScope("B").panelOpen).toBe(false);
    expect(modelScope(null).models).toEqual([]);
  });

  it("opens the same file twice in two workspaces, each its own", () => {
    const inA = show("C:/shared/robot.glb", "Ava", "A");
    const inB = show("C:/shared/robot.glb", "Ben", "B");
    expect(inA).not.toBe(inB);
    settleModel(inA, { info });
    expect(modelScope("A").models[0].status).toBe("ready");
    expect(modelScope("B").models[0].status).toBe("loading");
  });

  it("closes in one workspace without touching the other", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const car = show("C:/b/car.glb", "Ben", "B");
    setModelPanel("B", { expanded: true });
    closeModel(chair);
    expect(modelScope("A").panelOpen).toBe(false);
    expect(modelScope("B")).toMatchObject({ activeId: car, panelOpen: true, expanded: true });
    show("C:/a/lamp.glb", "Ava", "A");
    closeWorkspaceModels("B");
    expect(modelScope("B").models).toEqual([]);
    expect(modelScope("A").models.map((model) => model.title)).toEqual(["lamp.glb"]);
  });

  it("brings a tab forward in its own workspace only", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    show("C:/a/table.glb", "Ava", "A");
    const car = show("C:/b/car.glb", "Ben", "B");
    activateModel(chair);
    expect(modelScope("A").activeId).toBe(chair);
    expect(modelScope("B").activeId).toBe(car);
  });

  it("marks what an agent changed as unseen until the workspace's panel is on screen", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    show("C:/b/car.glb", "Ben", "B");
    markModelsSeen("A");
    expect(modelScope("A").unseen).toBe(false);
    expect(modelScope("B").unseen).toBe(true);
    reloadModel(chair, 5);
    expect(modelScope("A").unseen).toBe(true);
  });

  it("forgets the workspaces that were deleted", () => {
    show("C:/a/chair.glb", "Ava", "A");
    show("C:/b/car.glb", "Ben", "B");
    pruneModels(["A", "C"]);
    expect(modelScope("B").models).toEqual([]);
    expect(modelScope("A").models).toHaveLength(1);
    expect(getModels().all).toHaveLength(1);
  });

  it("hands a workspace's preview over to another window as it was", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    show("C:/a/table.glb", "Ava", "A");
    activateModel(chair);
    setModelPanel("A", { expanded: true });
    const scope = takeModelScope("A");
    expect(modelScope("A").models).toEqual([]);
    expect(takeModelScope("A")).toBeNull();
    // The other window, which here is this one again.
    adoptModelScope("A", scope!);
    expect(modelScope("A")).toMatchObject({ activeId: chair, panelOpen: true, expanded: true });
    expect(modelScope("A").models.map((model) => model.title)).toEqual(["chair.glb", "table.glb"]);
  });

  it("keeps what the receiving window has open, and does not open a file twice", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const scope = takeModelScope("A")!;
    const fresh = show("C:/a/chair.glb", "Ava", "A");
    show("C:/a/lamp.glb", "Ava", "A");
    const stool = { ...scope.models[0], id: "x", path: "C:/a/stool.glb", title: "stool.glb" };
    adoptModelScope("A", { ...scope, models: [...scope.models, stool] });
    expect(modelScope("A").models.map((model) => model.title)).toEqual(["chair.glb", "lamp.glb", "stool.glb"]);
    expect(modelScope("A").models[0].id).toBe(fresh);
    expect(modelScope("A").models.some((model) => model.id === chair)).toBe(false);
  });
});

describe("files of every kind beside the models", () => {
  beforeEach(() => closeAllModels());

  const file = (
    kind: Exclude<OpenPreview["kind"], "model" | "kit">,
    path: string,
    workspaceId = "ws",
    openedBy: string | null = "Ava",
  ) => showPreview({ kind, path, mime: "application/octet-stream", size: 100, workspaceId, openedBy, modifiedMs: 1 });

  it("opens each kind as a tab of its workspace, the latest in front", () => {
    const chair = show("C:/a/chair.glb");
    const shot = file("image", "C:/a/shot.png");
    const notes = file("markdown", "C:/a/NOTES.md");
    const scope = modelScope("ws");
    expect(scope.models.map((entry) => [entry.kind, entry.title])).toEqual([
      ["model", "chair.glb"],
      ["image", "shot.png"],
      ["markdown", "NOTES.md"],
    ]);
    expect(scope.activeId).toBe(notes);
    expect(scope.panelOpen).toBe(true);
    // A file is ready at once; only a model waits for the 3D viewer.
    expect(scope.models.map((entry) => entry.status)).toEqual(["loading", "ready", "ready"]);
    activateModel(shot);
    expect(modelScope("ws").activeId).toBe(shot);
    activateModel(chair);
    expect(modelScope("ws").activeId).toBe(chair);
  });

  it("keeps the 3D viewer's list to the models, and the window's to everything", () => {
    const chair = show("C:/a/chair.glb", "Ava", "A");
    const page = file("html", "C:/b/page.html", "B");
    const before = getModels().all;
    expect(before.map((model) => model.id)).toEqual([chair]);
    expect(getModels().entries.map((entry) => entry.id).sort()).toEqual([chair, page].sort());
    // An image opening does not disturb the viewer, which watches its list.
    file("image", "C:/b/shot.png", "B");
    expect(getModels().all).toBe(before);
    expect(getModels().all.every(isModel)).toBe(true);
  });

  it("brings a file shown again forward, once, with a new revision for its viewer", () => {
    const log = file("text", "C:/a/run.log", "ws", "Ava");
    file("pdf", "C:/a/report.pdf");
    const again = showPreview({
      kind: "text",
      path: "C:/a/run.log",
      title: "Run log",
      mime: "text/plain",
      size: 900,
      workspaceId: "ws",
      openedBy: "Ben",
      modifiedMs: 7,
    });
    expect(again).toBe(log);
    const scope = modelScope("ws");
    expect(scope.models).toHaveLength(2);
    expect(scope.activeId).toBe(log);
    expect(scope.models[0]).toMatchObject({ title: "Run log", revision: 1, size: 900, openedBy: "Ben", modifiedMs: 7 });
  });

  it("closes one kind or another, and the last one shuts its panel", () => {
    const chair = show("C:/a/chair.glb");
    const clip = file("video", "C:/a/clip.mp4");
    const song = file("audio", "C:/a/theme.ogg");
    closeModel(song);
    expect(modelScope("ws").activeId).toBe(clip);
    closeModels([chair, clip]);
    expect(modelScope("ws")).toMatchObject({ models: [], panelOpen: false });
    expect(getModels().entries).toEqual([]);
  });

  it("keeps each workspace's files apart, and closes one workspace's only", () => {
    const pageA = file("html", "C:/shared/index.html", "A");
    const pageB = file("html", "C:/shared/index.html", "B");
    expect(pageA).not.toBe(pageB);
    show("C:/b/car.glb", "Ben", "B");
    closeWorkspaceModels("B");
    expect(modelScope("B").models).toEqual([]);
    expect(modelScope("A").models.map((entry) => entry.id)).toEqual([pageA]);
  });

  it("answers a waiting caller at once for a file, which has nothing to load", async () => {
    const pdf = file("pdf", "C:/a/report.pdf");
    expect((await whenSettled(pdf, 1000))?.status).toBe("ready");
  });

  it("settles models only, and has a rewritten file read again, ready all along", () => {
    const shot = file("image", "C:/a/shot.png");
    settleModel(shot, { error: "not a glTF file" });
    expect(modelScope("ws").models[0]).toMatchObject({ status: "ready", revision: 0, error: null });
    markModelsSeen("ws");
    reloadModel(shot, 99, 2048);
    expect(modelScope("ws").models[0]).toMatchObject({ status: "ready", revision: 1, modifiedMs: 99, size: 2048 });
    expect(modelScope("ws").unseen).toBe(true);
  });

  it("hands a mixed preview over to another window as it was", () => {
    show("C:/a/chair.glb", "Ava", "A");
    const page = file("html", "C:/a/page.html", "A");
    const scope = takeModelScope("A")!;
    adoptModelScope("A", scope);
    expect(modelScope("A").models.map((entry) => entry.kind)).toEqual(["model", "html"]);
    expect(modelScope("A").activeId).toBe(page);
  });
});

describe("kit tabs beside the files", () => {
  beforeEach(() => closeAllModels());

  const kitTab = (tabId: string, workspaceId = "ws", title = tabId) =>
    openKitTab({ workspaceId, kitId: "hello-kit", tabId, title });

  it("opens a kit's tab in front, its panel open, as the person's own doing", () => {
    const id = kitTab("status", "ws", "Status");
    const scope = modelScope("ws");
    expect(scope.activeId).toBe(id);
    expect(scope.panelOpen).toBe(true);
    expect(scope.unseen).toBe(false);
    expect(scope.models[0]).toMatchObject({
      kind: "kit",
      kitId: "hello-kit",
      tabId: "status",
      title: "Status",
      path: kitTabPath("hello-kit", "status"),
      status: "ready",
      openedBy: null,
      revision: 0,
    });
    expect(isKitTab(scope.models[0])).toBe(true);
    expect(isPreviewFile(scope.models[0])).toBe(false);
    expect(isModel(scope.models[0])).toBe(false);
  });

  it("brings an open tab forward as it is, under the manifest's title of now", () => {
    const status = kitTab("status", "ws", "Status");
    const shot = showPreview({
      kind: "image",
      path: "C:/a/shot.png",
      mime: "image/png",
      size: 10,
      workspaceId: "ws",
      openedBy: "Ava",
      modifiedMs: 1,
    });
    expect(modelScope("ws").activeId).toBe(shot);
    expect(kitTab("status", "ws", "State")).toBe(status);
    const scope = modelScope("ws");
    expect(scope.models).toHaveLength(2);
    expect(scope.activeId).toBe(status);
    expect(scope.models[0]).toMatchObject({ title: "State", revision: 0 });
  });

  it("keeps each workspace's tabs apart, and the window's list of them", () => {
    const a = kitTab("status", "A");
    const b = kitTab("status", "B");
    expect(a).not.toBe(b);
    expect(getModels().kits.map((tab) => tab.id).sort()).toEqual([a, b].sort());
    expect(getModels().all).toEqual([]);
    // A file opening does not disturb the frames, which watch their list.
    const before = getModels().kits;
    showPreview({ kind: "text", path: "C:/a/x.txt", mime: "text/plain", size: 1, workspaceId: "A", openedBy: null, modifiedMs: 1 });
    expect(getModels().kits).toBe(before);
    closeWorkspaceModels("A");
    expect(getModels().kits.map((tab) => tab.id)).toEqual([b]);
  });

  it("reloads from its own button, never from a file watch", () => {
    const id = kitTab("status");
    reloadKitTab(id);
    expect(modelScope("ws").models[0].revision).toBe(1);
    reloadModel(id, 99, 5);
    expect(modelScope("ws").models[0]).toMatchObject({ revision: 1, modifiedMs: 0 });
  });

  it("closes like any tab, and moves with its workspace to another window", () => {
    const status = kitTab("status");
    const other = kitTab("other");
    closeModel(other);
    expect(modelScope("ws").activeId).toBe(status);
    const scope = takeModelScope("ws")!;
    adoptModelScope("ws", scope);
    expect(modelScope("ws").models.map((entry) => entry.kind)).toEqual(["kit"]);
    closeModels([status]);
    expect(modelScope("ws")).toMatchObject({ models: [], panelOpen: false });
  });

  describe("with params", () => {
    const run = (run: string, patch: Partial<KitTabToOpen> = {}) =>
      openKitTab({ workspaceId: "ws", kitId: "ci", tabId: "run", title: "Run", params: { run }, ...patch });

    it("knows a set of params whichever order its names came in", () => {
      expect(kitParamsKey({ a: "1", b: "2" })).toBe(kitParamsKey({ b: "2", a: "1" }));
      expect(kitParamsKey({ a: "1" })).not.toBe(kitParamsKey({ a: "2" }));
      expect(kitTabPath("ci", "run")).toBe("kit:ci/run");
      expect(kitTabPath("ci", "run", {})).toBe("kit:ci/run");
      expect(kitTabPath("ci", "run", { b: "2", a: "1" })).toBe(kitTabPath("ci", "run", { a: "1", b: "2" }));
      expect(kitTabPath("ci", "run", { a: "1" })).not.toBe(kitTabPath("ci", "run"));
    });

    it("holds a tab per set of params, and brings the one asked for forward", () => {
      const one = run("1");
      const two = run("2");
      expect(two).not.toBe(one);
      expect(modelScope("ws").models).toHaveLength(2);
      expect(kitTabParams(one)).toEqual({ run: "1" });
      expect(run("1")).toBe(one);
      expect(modelScope("ws").activeId).toBe(one);
      expect(modelScope("ws").models).toHaveLength(2);
    });

    it("follows its workspace to another window, and is not opened twice there", () => {
      const one = run("1");
      const scope = takeModelScope("ws")!;
      expect(kitTabParams(one)).toEqual({});
      adoptModelScope("ws", scope);
      expect(kitTabParams(one)).toEqual({ run: "1" });
      adoptModelScope("ws", { ...scope, models: scope.models.map((entry) => ({ ...entry, id: "again" })) });
      expect(modelScope("ws").models).toHaveLength(1);
    });

    it("the same request opens through a workspace's host as through the store", () => {
      const id = showPreview({
        kind: "kit",
        path: kitTabPath("ci", "run", { run: "1" }),
        workspaceId: "ws",
        kitId: "ci",
        tabId: "run",
        title: "Run",
        params: { run: "1" },
        openedBy: "Ava",
      });
      expect(run("1")).toBe(id);
      expect(modelScope("ws").models[0]).toMatchObject({ kind: "kit", openedBy: "Ava" });
    });

    it("is news when an agent opens it, not when the person does", () => {
      run("1");
      expect(modelScope("ws").unseen).toBe(false);
      closeAllModels();
      run("1", { openedBy: "Ava" });
      expect(modelScope("ws").unseen).toBe(true);
    });
  });

  describe("a singleton", () => {
    const board = (params?: Record<string, string>, patch: Partial<KitTabToOpen> = {}) =>
      openKitTab({ workspaceId: "ws", kitId: "ci", tabId: "board", title: "Board", singleton: true, params, ...patch });

    it("is one tab, and its page hears of new params without reloading", () => {
      const id = board({ view: "a" });
      const heard: unknown[] = [];
      const stop = watchKitTabParams(id, (params) => heard.push(params));

      expect(board({ view: "b" })).toBe(id);
      expect(modelScope("ws").models).toHaveLength(1);
      expect(kitTabParams(id)).toEqual({ view: "b" });
      expect(modelScope("ws").models[0]).toMatchObject({ revision: 0, path: kitTabPath("ci", "board") });
      expect(heard).toEqual([{ view: "b" }]);

      // Nothing new: nothing to hear, and it keeps what it holds.
      board({ view: "b" });
      board();
      board({});
      expect(heard).toEqual([{ view: "b" }]);
      expect(kitTabParams(id)).toEqual({ view: "b" });

      stop();
      board({ view: "c" });
      expect(heard).toEqual([{ view: "b" }]);
    });

    it("tells only its own page", () => {
      const mine = board({ view: "a" });
      const other = openKitTab({
        workspaceId: "ws",
        kitId: "ci",
        tabId: "other",
        title: "Other",
        singleton: true,
        params: { view: "a" },
      });
      const heard: string[] = [];
      watchKitTabParams(other, () => heard.push("other"));
      board({ view: "b" });
      expect(mine).not.toBe(other);
      expect(heard).toEqual([]);
      expect(kitTabParams(other)).toEqual({ view: "a" });
    });
  });

  describe("reveal", () => {
    const tab = (reveal?: boolean, tabId = "run") =>
      openKitTab({ workspaceId: "ws", kitId: "ci", tabId, title: tabId, openedBy: "Ava", reveal });

    it("opens the panel by default, the tab in front", () => {
      const first = tab();
      expect(modelScope("ws")).toMatchObject({ panelOpen: true, activeId: first });
      setModelPanel("ws", { open: false });
      const second = tab(true, "other");
      expect(modelScope("ws")).toMatchObject({ panelOpen: true, activeId: second });
    });

    it("false adds the tab without opening the panel, and says it is there", () => {
      const id = tab(false);
      expect(modelScope("ws")).toMatchObject({ panelOpen: false, activeId: id, unseen: true });
      expect(getModels().kits.map((entry) => entry.id)).toEqual([id]);
    });

    it("false leaves what is in front, and an open panel, as they are", () => {
      const first = tab();
      markModelsSeen("ws");
      tab(false, "other");
      expect(modelScope("ws")).toMatchObject({ panelOpen: true, activeId: first });
      // Brought forward by nobody, and nothing changed: no news either.
      const again = tab(false, "other");
      expect(modelScope("ws").activeId).toBe(first);
      expect(again).not.toBe(first);
    });

    it("false asks nothing of a tab already there", () => {
      tab();
      setModelPanel("ws", { open: false });
      markModelsSeen("ws");
      tab(false);
      expect(modelScope("ws")).toMatchObject({ panelOpen: false, unseen: false });
    });
  });

  describe("what a page says of itself", () => {
    const opened = () => openKitTab({ workspaceId: "ws", kitId: "ci", tabId: "run", title: "Run" });

    it("names its tab, which asking for it again leaves as it is", () => {
      const id = opened();
      setKitTabTitle(id, "Run #4567");
      expect(modelScope("ws").models[0]).toMatchObject({ title: "Run #4567", titled: true });
      opened();
      expect(modelScope("ws").models[0].title).toBe("Run #4567");
    });

    it("sets its state in the strip, until it is taken away or the page reloads", () => {
      const id = opened();
      const entry = () => modelScope("ws").models[0] as OpenKitTab;
      expect(entry().badge).toBeNull();
      setKitTabBadge(id, { state: "running", text: "2/5" });
      expect(entry().badge).toEqual({ state: "running", text: "2/5" });
      setKitTabBadge(id, null);
      expect(entry().badge).toBeNull();
      setKitTabBadge(id, { state: "failure", text: null });
      reloadKitTab(id);
      expect(entry().badge).toBeNull();
    });

    it("redraws nothing when nothing changes", () => {
      const id = opened();
      setKitTabBadge(id, { state: "success", text: "ok" });
      setKitTabTitle(id, "Run");
      const before = getModels().kits;
      setKitTabBadge(id, { state: "success", text: "ok" });
      setKitTabTitle(id, "Run");
      expect(getModels().kits).toBe(before);
    });

    it("reaches no other kind of tab", () => {
      const shot = show("C:/m/chair.glb");
      const before = getModels().entries;
      setKitTabTitle(shot, "Nope");
      setKitTabBadge(shot, { state: "failure", text: null });
      expect(getModels().entries).toBe(before);
    });
  });
});

describe("a kit's tabs across a restart", () => {
  /** A window's storage, which outlives the store: one fresh import per start. */
  let saved: Map<string, string>;
  const start = async () => {
    vi.resetModules();
    return import("./modelStore");
  };

  beforeEach(() => {
    saved = new Map();
    vi.stubGlobal("localStorage", {
      get length() {
        return saved.size;
      },
      key: (at: number) => [...saved.keys()][at] ?? null,
      getItem: (key: string) => saved.get(key) ?? null,
      setItem: (key: string, value: string) => void saved.set(key, value),
      removeItem: (key: string) => void saved.delete(key),
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  const CI = { workspaceId: "ws", kitId: "ci", tabId: "run", title: "Run" };

  it("keeps a tab's params, and its own title, behind a closed panel", async () => {
    const before = await start();
    const id = before.openKitTab({ ...CI, params: { run: "4567" }, openedBy: "Ava" });
    before.setKitTabTitle(id, "Run #4567");
    before.setKitTabBadge(id, { state: "running", text: null });

    const after = await start();
    expect(after.modelScope("ws").models).toEqual([]);
    after.restoreKitTabs("ws");
    const scope = after.modelScope("ws");
    expect(scope.models).toHaveLength(1);
    expect(scope.models[0]).toMatchObject({
      kind: "kit",
      kitId: "ci",
      tabId: "run",
      params: { run: "4567" },
      title: "Run #4567",
      titled: true,
      openedBy: "Ava",
      // The page says it again once it runs.
      badge: null,
      path: before.kitTabPath("ci", "run", { run: "4567" }),
    });
    expect(scope).toMatchObject({ panelOpen: false, unseen: false, activeId: scope.models[0].id });
    expect(after.kitTabParams(scope.models[0].id)).toEqual({ run: "4567" });
  });

  it("the same params land on the restored tab, and a singleton still takes new ones", async () => {
    const before = await start();
    before.openKitTab({ ...CI, params: { run: "1" } });
    before.openKitTab({ ...CI, tabId: "board", singleton: true, params: { view: "a" } });

    const after = await start();
    after.restoreKitTabs("ws");
    const [run, board] = after.modelScope("ws").models;
    expect(after.openKitTab({ ...CI, params: { run: "1" } })).toBe(run.id);
    expect(after.openKitTab({ ...CI, tabId: "board", singleton: true, params: { view: "b" } })).toBe(board.id);
    expect(after.kitTabParams(board.id)).toEqual({ view: "b" });

    const again = await start();
    again.restoreKitTabs("ws");
    expect(again.modelScope("ws").models.map((entry) => (entry as OpenKitTab).params)).toEqual([
      { run: "1" },
      { view: "b" },
    ]);
  });

  it("forgets a tab that was closed", async () => {
    const before = await start();
    const one = before.openKitTab({ ...CI, params: { run: "1" } });
    before.openKitTab({ ...CI, params: { run: "2" } });
    before.closeModel(one);

    const after = await start();
    after.restoreKitTabs("ws");
    expect(after.modelScope("ws").models.map((entry) => (entry as OpenKitTab).params)).toEqual([{ run: "2" }]);
    after.closeWorkspaceModels("ws");
    expect(saved.size).toBe(0);
  });

  it("brings no file back, and a file opened first does not lose what was saved", async () => {
    const before = await start();
    before.openKitTab({ ...CI, params: { run: "1" } });

    const after = await start();
    after.showPreview({ kind: "model", path: "C:/m/a.glb", format: "glb", workspaceId: "ws", openedBy: "Ava", modifiedMs: 1 });
    after.restoreKitTabs("ws");
    expect(after.modelScope("ws").models.map((entry) => entry.kind)).toEqual(["model", "kit"]);

    const again = await start();
    again.restoreKitTabs("ws");
    expect(again.modelScope("ws").models.map((entry) => entry.kind)).toEqual(["kit"]);
  });

  it("opening a tab first still brings the others back, and each workspace has its own", async () => {
    const before = await start();
    before.openKitTab({ ...CI, params: { run: "1" } });
    before.openKitTab({ ...CI, workspaceId: "other", params: { run: "9" } });

    const after = await start();
    after.openKitTab({ ...CI, params: { run: "2" } });
    expect(after.modelScope("ws").models.map((entry) => (entry as OpenKitTab).params)).toEqual([
      { run: "1" },
      { run: "2" },
    ]);
    expect(after.modelScope("other").models).toEqual([]);
    after.restoreKitTabs("other");
    expect(after.modelScope("other").models.map((entry) => (entry as OpenKitTab).params)).toEqual([{ run: "9" }]);
  });

  it("restores once, and what is open already is not opened twice", async () => {
    const before = await start();
    before.openKitTab({ ...CI, params: { run: "1" } });

    const after = await start();
    after.restoreKitTabs("ws");
    after.restoreKitTabs("ws");
    expect(after.modelScope("ws").models).toHaveLength(1);
    const scope = after.takeModelScope("ws")!;
    after.adoptModelScope("ws", scope);
    expect(after.modelScope("ws").models).toHaveLength(1);
  });

  it("keeps a workspace's tabs saved as it moves to a window of its own, whichever acts first", async () => {
    // Each window has a store of its own, and they share the storage.
    const main = await start();
    main.openKitTab({ ...CI, params: { run: "1" } });

    // Its window restored them before the main window handed them over.
    const own = await start();
    own.restoreKitTabs("ws");
    own.adoptModelScope("ws", main.takeModelScope("ws")!);
    expect(own.modelScope("ws").models).toHaveLength(1);
    let next = await start();
    next.restoreKitTabs("ws");
    expect(next.modelScope("ws").models.map((entry) => (entry as OpenKitTab).params)).toEqual([{ run: "1" }]);

    // And back: the main window takes them in, its own record gone with them.
    main.adoptModelScope("ws", own.takeModelScope("ws")!);
    expect(main.modelScope("ws").models).toHaveLength(1);
    next = await start();
    next.restoreKitTabs("ws");
    expect(next.modelScope("ws").models).toHaveLength(1);
  });

  it("reads the storage with care: what is not a tab is no tab", async () => {
    const before = await start();
    const keyOf = (workspaceId: string) => {
      before.openKitTab({ ...CI, workspaceId });
      return [...saved.keys()].find((key) => key.endsWith(workspaceId))!;
    };
    const key = keyOf("ws");
    for (const raw of [
      "not json",
      '{"a":1}',
      '[1,null,{"kitId":"ci"}]',
      '[{"kitId":"ci","tabId":"run","path":"elsewhere","title":"x","params":{}}]',
      '[{"kitId":"ci","tabId":"run","path":"kit:ci/run","title":"x","params":{"a":1}}]',
      '[{"kitId":"ci","tabId":"run","path":"kit:ci/run","title":"x","params":["a"]}]',
    ]) {
      saved.set(key, raw);
      const after = await start();
      after.restoreKitTabs("ws");
      expect(after.modelScope("ws").models, raw).toEqual([]);
    }
  });

  it("forgets what deleted workspaces saved, and nothing else in the storage", async () => {
    const before = await start();
    before.openKitTab({ ...CI, workspaceId: "kept", params: { run: "1" } });
    before.openKitTab({ ...CI, workspaceId: "deleted", params: { run: "2" } });
    saved.set("somebody.else", "x");
    before.forgetSavedKitTabs(["kept"]);
    expect(saved.has("somebody.else")).toBe(true);

    const after = await start();
    after.restoreKitTabs("kept");
    after.restoreKitTabs("deleted");
    expect(after.modelScope("kept").models).toHaveLength(1);
    expect(after.modelScope("deleted").models).toEqual([]);
  });

  it("works without any storage, as under test or when it is refused", async () => {
    vi.stubGlobal("localStorage", undefined);
    const store = await start();
    const id = store.openKitTab({ ...CI, params: { run: "1" } });
    expect(store.kitTabParams(id)).toEqual({ run: "1" });
    store.restoreKitTabs("other");
    expect(store.modelScope("other").models).toEqual([]);
    store.forgetSavedKitTabs([]);
  });
});

describe("modelFiles", () => {
  it("knows the formats by extension, whatever the case", () => {
    expect(modelFormatOf("a/b/Robot.GLB")).toBe("glb");
    expect(modelFormatOf("a/b/notes.txt")).toBeNull();
    expect(modelFormatOf("README")).toBeNull();
  });

  it("encodes segments but keeps separators, so relative URLs resolve", () => {
    expect(modelUrl("C:\\Users\\me\\my models\\cube.gltf", true)).toBe(
      "http://hangar-model.localhost/C%3A/Users/me/my%20models/cube.gltf",
    );
    expect(modelUrl("/home/me/cube.gltf", false)).toBe("hangar-model://localhost/home/me/cube.gltf");
  });
});
