import { describe, expect, it } from "vitest";
import type { Workspace, WorkspaceFolder } from "../types";
import { sidebarOrder } from "./folders";

const ws = (id: string, folderId: string | null = null) => ({ id, folderId }) as Workspace;
const folder = (id: string): WorkspaceFolder => ({ id, name: id, collapsed: false });

const ids = (list: Workspace[]) => list.map((workspace) => workspace.id);

describe("sidebarOrder", () => {
  it("lists the loose workspaces, then each folder's in folder order", () => {
    const workspaces = [ws("loose1"), ws("b1", "B"), ws("a1", "A"), ws("loose2"), ws("a2", "A")];
    expect(ids(sidebarOrder(workspaces, [folder("A"), folder("B")]))).toEqual([
      "loose1",
      "loose2",
      "a1",
      "a2",
      "b1",
    ]);
  });

  it("follows the folders when they are reordered", () => {
    const workspaces = [ws("a1", "A"), ws("b1", "B")];
    expect(ids(sidebarOrder(workspaces, [folder("B"), folder("A")]))).toEqual(["b1", "a1"]);
  });

  it("keeps a workspace whose folder is gone, with the loose ones", () => {
    const workspaces = [ws("orphan", "deleted"), ws("a1", "A"), ws("loose")];
    expect(ids(sidebarOrder(workspaces, [folder("A")]))).toEqual(["orphan", "loose", "a1"]);
  });

  it("lists every workspace exactly once", () => {
    const workspaces = [ws("a1", "A"), ws("x", "gone"), ws("y"), ws("b1", "B")];
    const ordered = ids(sidebarOrder(workspaces, [folder("A"), folder("B"), folder("empty")]));
    expect([...ordered].sort()).toEqual(ids(workspaces).sort());
  });

  it("is the stored order when there are no folders", () => {
    const workspaces = [ws("c"), ws("a"), ws("b")];
    expect(ids(sidebarOrder(workspaces, []))).toEqual(["c", "a", "b"]);
  });
});
