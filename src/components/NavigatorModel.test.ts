import { describe, expect, it } from "vitest";
import type { AgentId, Pane, SplitNode, Workspace, WorkspaceFolder } from "../types";
import {
  buildNavTree,
  fold,
  namesakeHints,
  readingOrder,
  type NavInput,
} from "./NavigatorModel";

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

describe("namesakeHints", () => {
  const at = (id: string, name: string, cwd: string) => ({ ...ws(id, []), name, cwd });
  const hints = (...list: Workspace[]) => Object.fromEntries(namesakeHints(list));

  it("says nothing for a name no other workspace has", () => {
    expect(hints(at("a", "api", "C:\\work\\api"), at("b", "web", "C:\\work\\web"))).toEqual({});
  });

  it("names the folder that differs, and only that one", () => {
    expect(
      hints(
        at("a", "api", "C:\\work\\api"),
        at("b", "API", "C:\\perso\\api"),
        at("c", "web", "C:\\web"),
      ),
    ).toEqual({ a: "work", b: "perso" });
  });

  it("passes over a folder that only repeats the name", () => {
    expect(hints(at("a", "api", "C:\\dev\\api"), at("b", "api", "C:\\dev\\api-v2"))).toEqual({
      b: "api-v2",
    });
  });

  it("numbers namesakes opened on the same folder", () => {
    expect(
      hints(
        at("a", "api", "C:\\dev\\api"),
        at("b", "api", "C:/dev/api/"),
        at("c", "api", "D:\\x\\api"),
      ),
    ).toEqual({ a: "1", b: "2", c: "x" });
  });
});

describe("buildNavTree without a filter", () => {
  it("lists the loose workspaces first, then each folder with its own", () => {
    expect(keys()).toEqual(["ws:hangar", "ws:docs", "folder:clients", "ws:storefront"]);
  });

  it("lists workspaces only, never their agents", () => {
    expect(buildNavTree(input()).rows.map((r) => r.kind)).not.toContain("pane");
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
    expect(keys({ workspaces: [orphan] })).toEqual(["ws:orphan", "folder:clients"]);
  });

  it("records each row's parent for the keyboard", () => {
    const rows = buildNavTree(input()).rows;
    expect(rows.find((r) => r.key === "ws:storefront")?.parent).toBe("folder:clients");
    expect(rows.find((r) => r.key === "ws:docs")?.parent).toBeNull();
    expect(rows.find((r) => r.key === "folder:clients")).toMatchObject({
      parent: null,
      expanded: true,
      foldable: true,
    });
  });
});

describe("buildNavTree with a query", () => {
  it("finds a workspace by its name, without case or accents", () => {
    expect(keys({ query: "DOCS" })).toEqual(["ws:docs"]);
    expect(keys({ query: "hang" })).toEqual(["ws:hangar"]);
  });

  it("finds a workspace through one of its agents, by name or kind", () => {
    expect(keys({ query: "eve" })).toEqual(["folder:clients", "ws:storefront"]);
    expect(keys({ query: "cleo" })).toEqual(["ws:hangar"]);
    expect(keys({ query: "CODEX" })).toEqual(["ws:hangar"]);
  });

  it("shows a folder whose name matches, open, with all it holds", () => {
    const tree = buildNavTree(input({ query: "client", folders: [folder("clients", true)] }));
    expect(tree.folders).toHaveLength(1);
    expect(tree.folders[0]).toMatchObject({ expanded: true, size: 1 });
    expect(tree.rows.map((r) => r.key)).toEqual(["folder:clients", "ws:storefront"]);
  });

  it("holds folders open and unfoldable while it filters", () => {
    const tree = buildNavTree(input({ query: "eve", folders: [folder("clients", true)] }));
    expect(tree.rows[0]).toMatchObject({ key: "folder:clients", expanded: true, foldable: false });
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

  it("keeps only the workspaces where one does", () => {
    expect(keys({ attentionOnly: true, wantsYou })).toEqual([
      "ws:hangar",
      "folder:clients",
      "ws:storefront",
    ]);
  });

  it("drops the active workspace too when nobody in it waits", () => {
    expect(keys({ attentionOnly: true, wantsYou: (id) => id === "p7" })).toEqual(["ws:docs"]);
  });

  it("combines with the query", () => {
    expect(keys({ attentionOnly: true, wantsYou, query: "ben" })).toEqual(["ws:hangar"]);
    expect(keys({ attentionOnly: true, wantsYou, query: "gus" })).toEqual([]);
  });

  it("does not keep a folder for its name alone", () => {
    expect(keys({ attentionOnly: true, wantsYou: () => false, query: "clients" })).toEqual([]);
  });
});
