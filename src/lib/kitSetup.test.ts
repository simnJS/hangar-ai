import { describe, expect, it, vi } from "vitest";
import type { KitSetupStatus } from "./kits";
import {
  currentOs,
  showWorkspace,
  pendingSteps,
  runSetupCommand,
  setupKeystrokes,
  setupSummary,
} from "./kitSetup";

const step = (id: string, done: boolean): KitSetupStatus => ({
  id,
  title: id,
  command: `run ${id}`,
  sha256: id,
  done,
});

describe("step status", () => {
  it("offers only the steps not done on this machine", () => {
    expect(pendingSteps([step("a", true), step("b", false)]).map((s) => s.id)).toEqual(["b"]);
  });

  it("counts done steps; no step is complete", () => {
    expect(setupSummary([step("a", true), step("b", false)])).toEqual({
      done: 1,
      total: 2,
      complete: false,
    });
    expect(setupSummary([])).toEqual({ done: 0, total: 0, complete: true });
  });
});

describe("what is typed", () => {
  it("is the command verbatim plus Enter", () => {
    expect(setupKeystrokes("npm install --prefix server")).toBe("npm install --prefix server\r");
  });
});

describe("currentOs", () => {
  it("maps platforms like the Rust side", () => {
    expect(currentOs("Win32")).toBe("windows");
    expect(currentOs("MacIntel")).toBe("macos");
    expect(currentOs("Linux x86_64")).toBe("linux");
  });
});

describe("runSetupCommand", () => {
  const base = () => ({
    openPane: vi.fn(() => "pane-1" as string | null),
    alive: vi.fn(async () => true),
    write: vi.fn(async () => undefined),
    wait: vi.fn(async () => undefined),
  });

  it("types the command once the terminal is up", async () => {
    const deps = base();
    deps.alive.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    await expect(runSetupCommand("echo hi", deps)).resolves.toEqual({ ok: true, paneId: "pane-1" });
    expect(deps.write).toHaveBeenCalledWith("pane-1", "echo hi\r");
  });

  it("types nothing when the terminal never starts", async () => {
    const deps = { ...base(), tries: 3 };
    deps.alive.mockResolvedValue(false);
    const closePane = vi.fn();
    await expect(runSetupCommand("echo hi", { ...deps, closePane })).resolves.toEqual({
      ok: false,
      reason: "not-started",
    });
    expect(deps.write).not.toHaveBeenCalled();
    expect(closePane).toHaveBeenCalledWith("pane-1");
  });

  it("types nothing when the workspace is full", async () => {
    const deps = base();
    deps.openPane.mockReturnValue(null);
    await expect(runSetupCommand("echo hi", deps)).resolves.toEqual({
      ok: false,
      reason: "no-pane",
    });
    expect(deps.write).not.toHaveBeenCalled();
  });

  it("reports a failed write", async () => {
    const deps = base();
    deps.write.mockRejectedValue(new Error("pty not found"));
    const closePane = vi.fn();
    await expect(runSetupCommand("echo hi", { ...deps, closePane })).resolves.toEqual({
      ok: false,
      reason: "write-failed",
      detail: "pty not found",
    });
    expect(closePane).toHaveBeenCalledWith("pane-1");
  });
});

describe("showWorkspace", () => {
  const run = async (patch: { activeId: string | null; asleep: boolean }) => {
    const calls: (string | null)[] = [];
    await showWorkspace({
      workspaceId: "w1",
      ...patch,
      setActive: (id) => calls.push(id),
      wait: async () => undefined,
    });
    return calls;
  };

  it("selects a workspace that is not the one on screen", async () => {
    expect(await run({ activeId: "w2", asleep: false })).toEqual(["w1"]);
  });

  it("selects a sleeping one", async () => {
    expect(await run({ activeId: "w2", asleep: true })).toEqual(["w1"]);
  });

  it("selects a sleeping workspace that is already selected from nothing, to wake it", async () => {
    expect(await run({ activeId: "w1", asleep: true })).toEqual([null, "w1"]);
  });
});
