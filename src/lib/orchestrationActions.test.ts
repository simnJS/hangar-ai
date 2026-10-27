import { describe, expect, it, vi } from "vitest";
import type { AgentId, Pane } from "../types";

// The hook reaches for the app's store, which talks to Tauri; the rule
// itself needs neither.
vi.mock("../store", () => ({ useStore: () => ({}) }));

const { canCreateOrchestrator, pinRefusal } = await import("./orchestrationActions");

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
