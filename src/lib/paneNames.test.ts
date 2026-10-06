import { describe, expect, it } from "vitest";
import { PANE_NAMES, pickPaneNames, sessionName, uniquePaneNames } from "./paneNames";
import type { Pane, Workspace } from "../types";

const pane = (id: string, name: string): Pane => ({
  id,
  name,
  agent: "claude",
  sessionId: null,
  cwd: null,
  shellId: null,
  title: null,
});

const workspace = (id: string, panes: Pane[]): Workspace => ({
  id,
  name: id,
  cwd: `/repo/${id}`,
  extraRoots: [],
  panes,
  themeId: null,
  shellId: null,
  tree: null,
  savedCommands: [],
  folderId: null,
});

describe("pane names", () => {
  it("keeps the names panes already had first in the list", () => {
    expect(PANE_NAMES.slice(0, 4)).toEqual(["Ava", "Max", "Leo", "Mia"]);
    expect(new Set(PANE_NAMES).size).toBe(PANE_NAMES.length);
  });

  it("picks names nobody uses yet, in order", () => {
    expect(pickPaneNames(3, ["Ava", "Leo"])).toEqual(["Max", "Mia", "Sam"]);
  });

  it("renames only the later duplicates across workspaces, and the unnamed", () => {
    const one = workspace("one", [pane("a", "Ava"), pane("b", "Max")]);
    const two = workspace("two", [pane("c", "Ava"), pane("d", ""), pane("e", "Leo")]);

    const [first, second] = uniquePaneNames([one, two]);

    expect(first.panes.map((p) => p.name)).toEqual(["Ava", "Max"]);
    expect(second.panes.map((p) => p.name)).toEqual(["Mia", "Sam", "Leo"]);
    // Ids, agents and sessions are untouched: only the name moves.
    expect(second.panes[0]).toMatchObject({ id: "c", agent: "claude" });
  });

  it("returns the very same workspaces when every name is already unique", () => {
    const list = [workspace("one", [pane("a", "Ava")]), workspace("two", [pane("b", "Max")])];
    expect(uniquePaneNames(list)).toBe(list);
  });

  it("names a Claude Code session after its pane and workspace, safe to quote", () => {
    expect(sessionName("Ava", "Hangar.AI")).toBe("Ava (Hangar.AI)");
    expect(sessionName("Ava", `My "big" $app`)).toBe("Ava (My big app)");
    expect(sessionName("Ava", "  ")).toBe("Ava");
  });
});
