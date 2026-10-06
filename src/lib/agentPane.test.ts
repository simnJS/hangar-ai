import { describe, expect, it } from "vitest";
import { findAgentPane } from "./agentPane";
import type { AgentId, Workspace } from "../types";

function workspace(id: string, name: string, panes: [string, AgentId][]): Workspace {
  return {
    id,
    name,
    cwd: `/repo/${id}`,
    extraRoots: [],
    panes: panes.map(([paneName, agent]) => ({
      id: `${id}:${paneName}`,
      name: paneName,
      agent,
      sessionId: null,
      cwd: null,
      shellId: null,
      title: null,
    })),
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
  };
}

const front = workspace("front", "Storefront", [
  ["Ava", "claude"],
  ["Ava 2", "claude"],
  ["Max", "codex"],
]);
const back = workspace("back", "Hangar", [
  ["Ava", "claude"],
  ["Leo", "shell"],
]);
const all = [front, back];

describe("findAgentPane", () => {
  it("finds a pane of the active workspace by its name inside the assignee", () => {
    expect(findAgentPane("Max", all, "front")?.paneId).toBe("front:Max");
    expect(findAgentPane("codex:max", all, "front")?.paneId).toBe("front:Max");
  });

  it("prefers the longest pane name", () => {
    expect(findAgentPane("ava-2", all, "front")?.paneId).toBe("front:Ava 2");
    expect(findAgentPane("ava", all, "front")?.paneId).toBe("front:Ava");
  });

  it("finds a name that only one workspace has, wherever it is", () => {
    expect(findAgentPane("Leo", all, "front")).toEqual({ workspaceId: "back", paneId: "back:Leo" });
    expect(findAgentPane("hangar-leo", all, "front")).toEqual({
      workspaceId: "back",
      paneId: "back:Leo",
    });
  });

  it("reads a Claude Code session name, workspace and all", () => {
    // The fixture still has an "Ava" in both workspaces, as a board from
    // before names were unique might: the workspace settles it.
    expect(findAgentPane("Ava (Hangar)", all, "front")).toEqual({
      workspaceId: "back",
      paneId: "back:Ava",
    });
    expect(findAgentPane("Ava 2 (Storefront)", all, "back")?.paneId).toBe("front:Ava 2");
  });

  it("falls back on the agent kind only when one pane runs it", () => {
    expect(findAgentPane("codex-1", all, "front")?.paneId).toBe("front:Max");
    expect(findAgentPane("claude", all, "front")).toBeNull();
    expect(findAgentPane("claude", all, "back")?.paneId).toBe("back:Ava");
  });

  it("does not match a word that only contains a pane name", () => {
    expect(findAgentPane("maximilian", all, "front")).toBeNull();
    expect(findAgentPane("agent", all, "front")).toBeNull();
  });
});
