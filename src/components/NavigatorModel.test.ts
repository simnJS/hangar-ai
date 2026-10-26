import { describe, expect, it } from "vitest";
import type { AgentId, Pane, SplitNode, Workspace, WorkspaceFolder } from "../types";
import { buildNavTree, fold, readingOrder, type NavInput } from "./NavigatorModel";

const pane = (id: string, name: string, agent: AgentId = "claude"): Pane => ({
  id,
  name,
  agent,
  sessionId: null,
  cwd: null,
  shellId: null,
  title: null,
});

const ws = (
  id: string,
  panes: Pane[],
  options: { folderId?: string | null; tree?: SplitNode | null; name?: string } = {},
): Workspace => ({
  id,
  name: options.name ?? id,
  cwd: `C:\\dev\\${id}`,
  extraRoots: [],
  panes,
  themeId: null,
  shellId: null,
  tree: options.tree ?? null,
  savedCommands: [],
  folderId: options.folderId ?? null,
});

const folder = (id: string, collapsed = false): WorkspaceFolder => ({ id, name: id, collapsed });

const leaf = (id: string): SplitNode => ({ type: "pane", id });

const hangar = ws("hangar", [
  pane("p1", "Ava"),
  pane("p2", "Ben", "codex"),
  pane("p3", "Cléo", "gemini"),
  pane("p4", "Dan", "shell"),
]);
const store = ws("storefront", [pane("p5", "Eve"), pane("p6", "Finn", "shell")], {
  folderId: "clients",
});
const docs = ws("docs", [pane("p7", "Gus")]);

const input = (patch: Partial<NavInput> = {}): NavInput => ({
  workspaces: [hangar, store, docs],
  folders: [folder("clients")],
  activeId: "hangar",
  opened: {},
  query: "",
  attentionOnly: false,
  wantsYou: () => false,
  ...patch,
});

const keys = (patch: Partial<NavInput> = {}) => buildNavTree(input(patch)).rows.map((r) => r.key);

describe("readingOrder", () => {
  it("follows the split tree rather than the stored order", () => {
    const tree: SplitNode = {
      type: "split",
      dir: "row",
      ratio: 0.5,
      a: leaf("b"),
      b: leaf("a"),
    };
    const order = readingOrder(ws("w", [pane("a", "Ava"), pane("b", "Ben")], { tree }));
    expect(order.map((p) => p.name)).toEqual(["Ben", "Ava"]);
  });

  it("is empty for a workspace without panes", () => {
    expect(readingOrder(ws("empty", []))).toEqual([]);
  });
});

describe("fold", () => {
  it("ignores case and accents", () => {
    expect(fold("Cléo À TOI")).toBe("cleo a toi");
  });
});

describe("buildNavTree without a filter", () => {
  it("lists folders first, opens the active workspace and only it", () => {
    expect(keys()).toEqual([
      "folder:clients",
      "ws:storefront",
      "ws:hangar",
      "pane:p1",
      "pane:p2",
      "pane:p3",
      "pane:p4",
      "ws:docs",
    ]);
  });

  it("keeps the active workspace open and unfoldable", () => {
    const tree = buildNavTree(input({ opened: { hangar: false } }));
    const active = tree.loose.find((entry) => entry.workspace.id === "hangar");
    expect(active).toMatchObject({ expanded: true, foldable: false });
  });

  it("opens the other workspaces opened by hand", () => {
    expect(keys({ opened: { docs: true } })).toContain("pane:p7");
  });

  it("keeps an empty folder, and a collapsed one shut", () => {
    const tree = buildNavTree(input({ folders: [folder("clients", true), folder("empty")] }));
    expect(tree.folders.map((f) => [f.folder.id, f.expanded, f.size])).toEqual([
      ["clients", false, 1],
      ["empty", true, 0],
    ]);
    expect(tree.rows.map((r) => r.key)).not.toContain("ws:storefront");
  });

  it("files a workspace whose folder is gone with the loose ones", () => {
    const orphan = ws("orphan", [], { folderId: "deleted" });
    expect(keys({ workspaces: [orphan], activeId: null })).toEqual(["folder:clients", "ws:orphan"]);
  });

  it("records each row's parent for the keyboard", () => {
    const rows = buildNavTree(input()).rows;
    expect(rows.find((r) => r.key === "ws:storefront")?.parent).toBe("folder:clients");
    expect(rows.find((r) => r.key === "pane:p2")?.parent).toBe("ws:hangar");
    expect(rows.find((r) => r.key === "ws:docs")?.parent).toBeNull();
  });
});

describe("buildNavTree with a query", () => {
  it("keeps a workspace found through an agent, open on that agent only", () => {
    const tree = buildNavTree(input({ query: "eve" }));
    expect(tree.rows.map((r) => r.key)).toEqual(["folder:clients", "ws:storefront", "pane:p5"]);
    expect(tree.folders[0].workspaces[0]).toMatchObject({ expanded: true, foldable: false });
  });

  it("matches the kind of agent too, without case or accents", () => {
    expect(keys({ query: "CODEX" })).toEqual(["ws:hangar", "pane:p2"]);
    expect(keys({ query: "cleo" })).toEqual(["ws:hangar", "pane:p3"]);
  });

  it("keeps every agent of a workspace whose own name matches", () => {
    const tree = buildNavTree(input({ query: "hang" }));
    expect(tree.loose.map((e) => [e.workspace.id, e.panes.length])).toEqual([["hangar", 4]]);
  });

  it("leaves a workspace matched by name folded as it was", () => {
    expect(keys({ query: "docs" })).toEqual(["ws:docs"]);
    expect(keys({ query: "docs", opened: { docs: true } })).toEqual(["ws:docs", "pane:p7"]);
  });

  it("shows a folder whose name matches, open, with all it holds", () => {
    const tree = buildNavTree(input({ query: "client", folders: [folder("clients", true)] }));
    expect(tree.folders).toHaveLength(1);
    expect(tree.folders[0].expanded).toBe(true);
    expect(tree.folders[0].workspaces.map((e) => e.panes.length)).toEqual([2]);
  });

  it("hides folders with nothing that matches, and says it is filtering", () => {
    const tree = buildNavTree(input({ query: "zzz" }));
    expect(tree).toMatchObject({ folders: [], loose: [], rows: [], filtering: true });
  });

  it("ignores blanks around the query", () => {
    expect(buildNavTree(input({ query: "   " })).filtering).toBe(false);
  });
});

describe("buildNavTree with only the agents that want you", () => {
  const wantsYou = (id: string) => id === "p2" || id === "p5";

  it("keeps only those agents, their workspaces open", () => {
    expect(keys({ attentionOnly: true, wantsYou })).toEqual([
      "folder:clients",
      "ws:storefront",
      "pane:p5",
      "ws:hangar",
      "pane:p2",
    ]);
  });

  it("drops the active workspace too when nobody in it waits", () => {
    expect(keys({ attentionOnly: true, wantsYou: (id) => id === "p7" })).toEqual([
      "ws:docs",
      "pane:p7",
    ]);
  });

  it("combines with the query", () => {
    expect(keys({ attentionOnly: true, wantsYou, query: "ben" })).toEqual(["ws:hangar", "pane:p2"]);
  });

  it("does not keep a folder for its name alone", () => {
    expect(keys({ attentionOnly: true, wantsYou: () => false, query: "clients" })).toEqual([]);
  });
});
