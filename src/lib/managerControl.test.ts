import { describe, expect, it } from "vitest";
import type { ControlDeps } from "./control";
import { RELAY_PREFIX, managerControl } from "./managerControl";
import type { ProjectsDeps } from "./projects";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

function pane(id: string, name: string, role?: PaneRole): Pane {
  return {
    id,
    name,
    agent: "claude",
    sessionId: null,
    cwd: null,
    shellId: null,
    title: null,
    ...(role ? { role } : {}),
  };
}

function workspace(id: string, name: string, panes: Pane[], extra: Partial<Workspace> = {}): Workspace {
  return {
    id,
    name,
    cwd: `C:\\dev\\${id}`,
    extraRoots: [],
    panes,
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
    ...extra,
  };
}

const MANAGER = { paneId: "m", paneName: "boss", cwd: "C:\\dev\\hq" };

function setup(asleep: string[] = [], open: string[] = ["shop", "blog", "wiki", "hq"]) {
  const workspaces = [
    workspace("hq", "HQ", [pane("m", "boss", "manager")], { hq: true }),
    workspace("shop", "Shop", [pane("o", "Jack", "orchestrator"), pane("w", "Eli", "worker")]),
    workspace("blog", "Blog", [pane("b", "Ava")]),
    workspace("wiki", "Wiki", [pane("k", "Kim", "orchestrator")]),
  ];
  const state = { ...DEFAULT_SETTINGS, workspaces, activeWorkspaceId: "hq" } as unknown as AppState;
  const calls = {
    enqueued: [] as [string, string, string | undefined][],
    opened: [] as string[],
    touched: [] as string[],
  };
  const projects: ProjectsDeps = {
    snapshot: () => state,
    loadBoard: async () => [],
    loadMemory: async () => [],
    isRepo: async () => true,
    activity: () => undefined,
    asleep: (id) => asleep.includes(id),
    isOpen: (id) => open.includes(id),
    journal: () => [],
  };
  const deps = {
    snapshot: () => state,
    isOpen: (id: string) => open.includes(id),
    asleep: (id: string) => asleep.includes(id),
    touch: (id: string) => void calls.touched.push(id),
    openWorkspace: (id: string) => void calls.opened.push(id),
    enqueue: async (paneId: string, text: string, from?: string) => void calls.enqueued.push([paneId, text, from]),
    projects,
  } as unknown as ControlDeps;
  return { deps, calls };
}

describe("project_* access", () => {
  it("refuses everyone but the manager", async () => {
    const { deps } = setup();
    for (const op of ["project_list", "project_status", "project_send"]) {
      for (const caller of [
        { paneId: "o", paneName: "Jack", cwd: null },
        { paneId: "w", paneName: "Eli", cwd: null },
        { paneId: null, paneName: null, cwd: null },
      ]) {
        await expect(managerControl(op, { workspace: "Shop", text: "hi" }, caller, deps)).rejects.toThrow(
          /project manager/,
        );
      }
    }
  });
});

describe("project_list", () => {
  it("leaves the HQ out and keeps a workspace without orchestrator", async () => {
    const { deps } = setup();
    const answer = (await managerControl("project_list", {}, MANAGER, deps)) as {
      projects: { name: string; orchestrator: unknown }[];
      text: string;
    };
    expect(answer.projects.map((project) => project.name)).toEqual(["Shop", "Blog", "Wiki"]);
    expect(answer.projects.find((project) => project.name === "Blog")?.orchestrator).toBeNull();
    expect(answer.text).toContain("Blog");
    expect(answer.text).toContain("no orchestrator");
    expect(answer.text).not.toContain("HQ —");
  });
});

describe("project_status", () => {
  it("answers one project, found by name or folder", async () => {
    const { deps } = setup();
    for (const workspace of ["shop", "C:/DEV/shop/"]) {
      const answer = (await managerControl("project_status", { workspace }, MANAGER, deps)) as {
        project: { name: string };
        text: string;
      };
      expect(answer.project.name).toBe("Shop");
      expect(answer.text).toContain("Shop");
    }
  });

  it("lists the names when the project is unknown, the HQ left out", async () => {
    const { deps } = setup();
    await expect(managerControl("project_status", { workspace: "Nope" }, MANAGER, deps)).rejects.toThrow(
      'No project "Nope". Projects: "Shop", "Blog", "Wiki".',
    );
  });
});

describe("project_send", () => {
  it("queues the text for the orchestrator, framed as relayed", async () => {
    const { deps, calls } = setup();
    const answer = (await managerControl(
      "project_send",
      { workspace: "Shop", text: "  ship the cart first  " },
      MANAGER,
      deps,
    )) as { queued: boolean; orchestrator: string; note: string };
    expect(calls.enqueued).toEqual([["o", `${RELAY_PREFIX} ship the cart first`, "boss"]]);
    expect(calls.touched).toEqual(["shop"]);
    expect(calls.opened).toEqual([]);
    expect(answer).toMatchObject({ queued: true, workspace: "Shop", orchestrator: "Jack" });
    expect(answer.note).toContain("events");
  });

  it("wakes an asleep workspace", async () => {
    const { deps, calls } = setup(["wiki"], ["hq", "shop", "blog"]);
    const answer = await managerControl("project_send", { workspace: "Wiki", text: "go" }, MANAGER, deps);
    expect(calls.opened).toEqual(["wiki"]);
    expect(calls.enqueued).toHaveLength(1);
    expect(answer).toMatchObject({ queued: true, woke: true });
  });

  it("refuses a workspace without orchestrator and says only the person can pin one", async () => {
    const { deps, calls } = setup();
    await expect(managerControl("project_send", { workspace: "Blog", text: "go" }, MANAGER, deps)).rejects.toThrow(
      /has no orchestrator.*Only the person can pin one/s,
    );
    expect(calls.enqueued).toEqual([]);
  });

  it("refuses the HQ itself", async () => {
    const { deps, calls } = setup();
    await expect(managerControl("project_send", { workspace: "HQ", text: "go" }, MANAGER, deps)).rejects.toThrow(
      /is the HQ/,
    );
    expect(calls.enqueued).toEqual([]);
  });

  it("refuses an empty text", async () => {
    const { deps } = setup();
    await expect(managerControl("project_send", { workspace: "Shop", text: " " }, MANAGER, deps)).rejects.toThrow(
      /Nothing to send/,
    );
  });
});
