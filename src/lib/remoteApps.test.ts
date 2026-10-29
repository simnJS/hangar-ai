import { describe, expect, it } from "vitest";
import type { ControlDeps } from "./control";
import { candidatesOf, remoteApps, type AppsSources } from "./remoteApps";
import { runRemote } from "./remoteControl";
import type { RemoteApps } from "../remote/types";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

function pane(id: string, name: string, role?: PaneRole, extra: Partial<Pane> = {}): Pane {
  return { id, name, agent: "claude", sessionId: null, cwd: null, shellId: null, title: null, ...(role ? { role } : {}), ...extra };
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

const worker = (id: string, name: string, portBase: number | null, taskId: string | null = null) =>
  pane(id, name, "worker", { portBase, taskId });

function depsOf(workspaces: Workspace[]): ControlDeps {
  const state = { workspaces, settings: { ...DEFAULT_SETTINGS } } as unknown as AppState;
  return { snapshot: () => state } as unknown as ControlDeps;
}

interface Rig {
  sources: AppsSources;
  /** Each call's ports, in the order asked. */
  probed: number[][];
  /** The boards read, by folder. */
  boards: string[];
}

function rig(listening: number[], boards: Record<string, { id: string; title: string }[] | Error> = {}): Rig {
  const probed: number[][] = [];
  const read: string[] = [];
  return {
    probed,
    boards: read,
    sources: {
      listening: async (ports) => {
        probed.push(ports);
        return ports.filter((port) => listening.includes(port));
      },
      tasks: async (cwd) => {
        read.push(cwd);
        const board = boards[cwd];
        if (board instanceof Error) throw board;
        return board ?? [];
      },
    },
  };
}

const shop = workspace(
  "w-shop",
  "Shop",
  [pane("o1", "Jack", "orchestrator"), worker("p-ada", "Ada", 21000, "t-cart"), worker("p-ben", "Ben", 21020, "t-pay")],
  { mobilePorts: [5173] },
);
const blog = workspace("w-blog", "Blog", [pane("p-sh", "shell")], { mobilePorts: [3000, 4321] });
const hq = workspace("hq", "HQ", [pane("pm", "Max", "manager"), worker("p-hq", "Odd", 21100)], { hq: true, mobilePorts: [8080] });

const shopBoard = { "C:\\dev\\w-shop": [{ id: "t-cart", title: "The cart keeps its items" }, { id: "t-pay", title: "Pay by card" }] };

describe("remote_apps", () => {
  it("lists every worker's port block and every port the person added, kept to the ones that listen", async () => {
    const { sources, probed } = rig([21003, 21020, 5173, 4321, 9999], shopBoard);
    const answer = await remoteApps(depsOf([shop, blog]), {}, sources);
    expect(answer).toEqual<RemoteApps>({
      apps: [
        { workspaceId: "w-shop", workspace: "Shop", port: 21003, source: "worker", pane: "Ada", task: "The cart keeps its items" },
        { workspaceId: "w-shop", workspace: "Shop", port: 21020, source: "worker", pane: "Ben", task: "Pay by card" },
        { workspaceId: "w-shop", workspace: "Shop", port: 5173, source: "custom" },
        { workspaceId: "w-blog", workspace: "Blog", port: 4321, source: "custom" },
      ],
    });
    // Each worker's twenty ports, then the person's, asked once.
    expect(probed).toHaveLength(1);
    expect(probed[0]).toHaveLength(20 + 20 + 1 + 2);
    expect(probed[0].slice(0, 2)).toEqual([21000, 21001]);
    expect(probed[0]).toContain(21039);
    expect(probed[0]).not.toContain(21040);
    expect(probed[0].slice(-3)).toEqual([5173, 3000, 4321]);
  });

  it("lists nothing when no server listens, and reads no board then", async () => {
    const { sources, boards } = rig([], shopBoard);
    expect(await remoteApps(depsOf([shop, blog]), {}, sources)).toEqual({ apps: [] });
    expect(boards).toEqual([]);
  });

  it("leaves the HQ out: its manager's ports, and any it was given, are no project's", async () => {
    const { sources, probed } = rig([21100, 8080, 5173]);
    const answer = await remoteApps(depsOf([hq, shop]), {}, sources);
    expect(answer.apps.map((app) => app.workspaceId)).toEqual(["w-shop"]);
    expect(probed.flat()).not.toContain(21100);
    expect(probed.flat()).not.toContain(8080);
  });

  it("asks nothing of the PC when no project has a port to offer", async () => {
    const { sources, probed } = rig([3000]);
    expect(await remoteApps(depsOf([hq, workspace("w", "Plain", [pane("p", "Sam")])]), {}, sources)).toEqual({ apps: [] });
    expect(probed).toEqual([]);
  });

  it("lists one project's when asked, by id or by name", async () => {
    const { sources } = rig([21003, 5173, 3000], shopBoard);
    const byId = await remoteApps(depsOf([shop, blog]), { workspace: "w-blog" }, sources);
    expect(byId.apps).toEqual([{ workspaceId: "w-blog", workspace: "Blog", port: 3000, source: "custom" }]);
    const byName = await remoteApps(depsOf([shop, blog]), { workspace: " shop " }, sources);
    expect(byName.apps.map((app) => app.port)).toEqual([21003, 5173]);
  });

  it("refuses a project it does not know, the HQ included", async () => {
    const { sources } = rig([3000]);
    await expect(remoteApps(depsOf([hq, shop]), { workspace: "w-nope" }, sources)).rejects.toThrow('No project "w-nope" in Hangar.');
    await expect(remoteApps(depsOf([hq, shop]), { workspace: "hq" }, sources)).rejects.toThrow('No project "hq" in Hangar.');
    await expect(remoteApps(depsOf([hq, shop]), { workspace: "" }, sources)).rejects.toThrow("Name the project");
  });

  it("names a worker's task only when its board has it, and lists the app all the same", async () => {
    const lost = workspace("w-lost", "Lost", [worker("p-cy", "Cy", 21200, "t-gone"), worker("p-di", "Di", 21220, null)]);
    const { sources } = rig([21200, 21220], { "C:\\dev\\w-lost": [{ id: "t-other", title: "Another task" }] });
    expect((await remoteApps(depsOf([lost]), {}, sources)).apps).toEqual([
      { workspaceId: "w-lost", workspace: "Lost", port: 21200, source: "worker", pane: "Cy" },
      { workspaceId: "w-lost", workspace: "Lost", port: 21220, source: "worker", pane: "Di" },
    ]);
  });

  it("lists a worker's app even when its board cannot be read", async () => {
    const { sources } = rig([21000], { "C:\\dev\\w-shop": new Error("board.json is not valid JSON") });
    expect((await remoteApps(depsOf([shop]), {}, sources)).apps).toEqual([
      { workspaceId: "w-shop", workspace: "Shop", port: 21000, source: "worker", pane: "Ada" },
    ]);
  });

  it("reads only the boards of the projects whose workers have an app up", async () => {
    const { sources, boards } = rig([21003, 3000], shopBoard);
    await remoteApps(depsOf([shop, blog]), {}, sources);
    expect(boards).toEqual(["C:\\dev\\w-shop"]);
  });

  it("fails when the PC cannot say which ports listen, rather than list them all", async () => {
    const failing: AppsSources = {
      listening: async () => {
        throw new Error("remote_ports_listening is not a command");
      },
      tasks: async () => [],
    };
    await expect(remoteApps(depsOf([shop]), {}, failing)).rejects.toThrow("not a command");
  });
});

describe("the ports a project offers", () => {
  it("skips a worker without a port block, and the panes that are not workers", () => {
    const ws = workspace("w", "W", [pane("o", "Jo", "orchestrator", { portBase: 21000 }), worker("a", "Al", null), worker("b", "Bo", 21040)]);
    const ports = candidatesOf([ws]).map((candidate) => candidate.app.port);
    expect(ports).toHaveLength(20);
    expect(ports[0]).toBe(21040);
  });

  it("keeps a port the person added once, a worker's when it falls in its block", () => {
    const ws = workspace("w", "W", [worker("b", "Bo", 21040)], { mobilePorts: [21045, 5173, 5173, 3000] });
    const custom = candidatesOf([ws]).filter((candidate) => candidate.app.source === "custom");
    expect(custom.map((candidate) => candidate.app.port)).toEqual([3000, 5173]);
    expect(candidatesOf([ws]).find((candidate) => candidate.app.port === 21045)?.app.source).toBe("worker");
  });

  it("leaves out what the file holds that is not a port the person could have added", () => {
    const ws = workspace("w", "W", [], { mobilePorts: [80, 5173.5, 70000, "3000", null, 4000] as unknown as number[] });
    expect(candidatesOf([ws]).map((candidate) => candidate.app.port)).toEqual([4000]);
  });
});

describe("through the phone's requests", () => {
  it("answers remote_apps, whatever the agents' switch says", async () => {
    const { sources } = rig([5173]);
    const deps = { ...depsOf([shop]), enabled: () => false } as ControlDeps;
    const answer = await runRemote("remote_apps", {}, deps, undefined, undefined, undefined, undefined, sources);
    expect(answer).toEqual({ apps: [{ workspaceId: "w-shop", workspace: "Shop", port: 5173, source: "custom" }] });
  });
});
