import { describe, expect, it, vi } from "vitest";
import type { AgentId, AppState, Pane, Workspace } from "../types";

// The hook reaches for the app's store, which talks to Tauri; the rule
// itself needs neither.
vi.mock("../store", () => ({ useStore: () => ({}) }));

const { canCreateOrchestrator, openManager, pinRefusal } = await import("./orchestrationActions");

const pane = (id: string, agent: AgentId = "claude", patch: Partial<Pane> = {}): Pane => ({
  id,
  name: id,
  agent,
  sessionId: null,
  cwd: null,
  shellId: null,
  title: null,
  ...patch,
});

describe("pinRefusal", () => {
  it("pins a Claude Code pane in a workspace that has no orchestrator", () => {
    expect(pinRefusal({ panes: [pane("a"), pane("b", "codex")] }, "a")).toBeNull();
  });

  it("keeps to one orchestrator per workspace", () => {
    const panes = [pane("a", "claude", { role: "orchestrator" }), pane("b")];
    expect(pinRefusal({ panes }, "b")).toBe("taken");
    expect(pinRefusal({ panes }, "a")).toBe("pinned");
  });

  it("pins Claude Code only: the role reaches the agent through its mod", () => {
    for (const agent of ["shell", "codex", "gemini", "opencode"] as const) {
      expect(pinRefusal({ panes: [pane("x", agent)] }, "x"), agent).toBe("notClaude");
    }
  });

  it("never turns a worker into the orchestrator", () => {
    const panes = [pane("w", "claude", { role: "worker", taskId: "1" })];
    expect(pinRefusal({ panes }, "w")).toBe("worker");
  });

  it("says so when the pane is gone", () => {
    expect(pinRefusal({ panes: [pane("a")] }, "zz")).toBe("missing");
  });
});

describe("canCreateOrchestrator", () => {
  it("creates one only where there is none, workers or not", () => {
    expect(canCreateOrchestrator({ panes: [pane("a"), pane("w", "claude", { role: "worker" })] })).toBe(
      true,
    );
    expect(canCreateOrchestrator({ panes: [pane("a", "claude", { role: "orchestrator" })] })).toBe(
      false,
    );
  });
});

describe("the HQ and its manager", () => {
  it("refuses an orchestrator in the HQ, and a manager pane as one", () => {
    expect(pinRefusal({ hq: true, panes: [pane("a")] }, "a")).toBe("hq");
    expect(canCreateOrchestrator({ hq: true, panes: [] })).toBe(false);
    expect(pinRefusal({ panes: [pane("m", "claude", { role: "manager" })] }, "m")).toBe("manager");
    // Wherever it sits, never a worker's or an orchestrator's.
    expect(
      pinRefusal({ hq: true, panes: [pane("m", "claude", { role: "manager" })] }, "m"),
    ).toBe("manager");
  });

  /** A tiny store: what openManager reads and does, in memory. */
  function fakeStore(workspaces: Workspace[] = []) {
    let state = { workspaces, activeWorkspaceId: null as string | null } as unknown as AppState;
    let n = 0;
    const dirCalls: number[] = [];
    const deps = {
      snapshot: () => state,
      addWorkspace: (draft: { name: string; cwd: string }) => {
        const id = `ws${++n}`;
        const ws = {
          id,
          name: draft.name,
          cwd: draft.cwd,
          panes: [pane(`p${++n}`)],
        } as unknown as Workspace;
        state = { ...state, workspaces: [...state.workspaces, ws], activeWorkspaceId: id };
        return id;
      },
      updateWorkspace: (id: string, patch: Partial<Workspace>) => {
        state = {
          ...state,
          workspaces: state.workspaces.map((ws) => (ws.id === id ? { ...ws, ...patch } : ws)),
        };
      },
      setActiveWorkspace: (id: string | null) => {
        state = { ...state, activeWorkspaceId: id };
      },
      addPane: (workspaceId: string) => {
        const id = `p${++n}`;
        state = {
          ...state,
          workspaces: state.workspaces.map((ws) =>
            ws.id === workspaceId ? { ...ws, panes: [...ws.panes, pane(id)] } : ws,
          ),
        };
        return id;
      },
      updatePane: (workspaceId: string, paneId: string, patch: Partial<Pane>) => {
        state = {
          ...state,
          workspaces: state.workspaces.map((ws) =>
            ws.id === workspaceId
              ? { ...ws, panes: ws.panes.map((p) => (p.id === paneId ? { ...p, ...patch } : p)) }
              : ws,
          ),
        };
      },
      hqDir: async () => {
        dirCalls.push(1);
        return "/data/hq";
      },
      hqName: "HQ",
    };
    return { deps, dirCalls, get: () => state };
  }

  const managers = (state: AppState) =>
    state.workspaces.flatMap((ws) => ws.panes.filter((p) => p.role === "manager"));

  it("creates the HQ once: a Claude Code with the role, on the orchestrator's model", async () => {
    const store = fakeStore();
    const first = await openManager(store.deps);
    expect(first.kind).toBe("created");
    const [hq] = store.get().workspaces;
    expect(hq).toMatchObject({ hq: true, name: "HQ", cwd: "/data/hq" });
    expect(hq.panes).toHaveLength(1);
    expect(hq.panes[0]).toMatchObject({ agent: "claude", role: "manager", model: "opus" });
    expect(store.get().activeWorkspaceId).toBe(hq.id);

    // A second call only switches to it.
    store.deps.setActiveWorkspace("elsewhere");
    const second = await openManager(store.deps);
    expect(second).toEqual({ kind: "shown", workspaceId: hq.id, paneId: hq.panes[0].id });
    expect(store.get().activeWorkspaceId).toBe(hq.id);
    expect(store.get().workspaces).toHaveLength(1);
    expect(store.dirCalls).toHaveLength(1);
    expect(managers(store.get())).toHaveLength(1);
  });

  it("adds a manager again to an HQ whose manager was closed", async () => {
    const store = fakeStore([
      { id: "hq", name: "HQ", cwd: "/data/hq", hq: true, panes: [] } as unknown as Workspace,
    ]);
    const result = await openManager(store.deps);
    expect(result.kind).toBe("restored");
    expect(managers(store.get())).toHaveLength(1);
    expect(managers(store.get())[0]).toMatchObject({ role: "manager", agent: "claude", model: "opus" });
    expect(store.get().activeWorkspaceId).toBe("hq");
    expect(store.dirCalls).toHaveLength(0);
  });

  it("never makes two managers, even when asked twice at once", async () => {
    const store = fakeStore();
    const [a, b] = await Promise.all([openManager(store.deps), openManager(store.deps)]);
    expect(a).toEqual(b);
    expect(store.get().workspaces).toHaveLength(1);
    expect(managers(store.get())).toHaveLength(1);
    await openManager(store.deps);
    expect(managers(store.get())).toHaveLength(1);
  });

  it("says so when the folder cannot be made, and creates nothing", async () => {
    const store = fakeStore();
    const result = await openManager({
      ...store.deps,
      hqDir: async () => {
        throw new Error("denied");
      },
    });
    expect(result).toEqual({ kind: "failed", error: "Error: denied" });
    expect(store.get().workspaces).toHaveLength(0);
  });
});
