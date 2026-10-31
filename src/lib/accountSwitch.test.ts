import { describe, expect, it } from "vitest";
import type { ClaudeAccount } from "./accounts";
import {
  accountName,
  defaultLabel,
  panesToRestartOnDefaultChange,
  panesToRestartOnWorkspaceChange,
  runningCount,
  usageByAccount,
  usagePercent,
  workspacesUsing,
  type SwitchWorkspace,
} from "./accountSwitch";

const accounts: ClaudeAccount[] = [
  { id: "main", label: "Principal", isMain: true, loggedIn: true },
  { id: "perso", label: "Perso", isMain: false, loggedIn: true },
];

const workspaces: SwitchWorkspace[] = [
  {
    id: "w1",
    panes: [
      { id: "a", agent: "claude", running: true },
      { id: "b", agent: "shell", running: true },
      { id: "c", agent: "claude", claudeAccount: "perso", running: true },
      { id: "d", agent: "claude", running: false },
    ],
  },
  { id: "w2", claudeAccount: "perso", panes: [{ id: "e", agent: "claude", running: true }] },
  { id: "w3", claudeAccount: "  ", panes: [{ id: "f", agent: "codex" }, { id: "g", agent: "claude" }] },
];

const ids = (panes: { id: string }[]) => panes.map((p) => p.id);

describe("panes to restart", () => {
  it("restarts only the Claude panes that follow the workspace", () => {
    expect(ids(panesToRestartOnWorkspaceChange(workspaces[0]))).toEqual(["a", "d"]);
    expect(ids(panesToRestartOnWorkspaceChange(workspaces[1]))).toEqual(["e"]);
  });
  it("leaves workspaces and panes with their own account out of a default change", () => {
    expect(ids(panesToRestartOnDefaultChange(workspaces))).toEqual(["a", "d", "g"]);
  });
  it("counts the running ones", () => {
    expect(runningCount(panesToRestartOnWorkspaceChange(workspaces[0]))).toBe(1);
  });
});

describe("usageByAccount", () => {
  const owner: Record<string, string> = { p1: "main", p2: "main", p3: "perso" };
  const of = (id: string) => owner[id];
  it("keeps the latest measure per account and window", () => {
    const usage = usageByAccount(
      [
        { paneId: "p1", at: 200, rateLimits: [{ kind: "five_hour", percentUsed: 40, resetsAt: null }] },
        {
          paneId: "p2",
          at: 100,
          rateLimits: [
            { kind: "five_hour", percentUsed: 10, resetsAt: null },
            { kind: "seven_day", percentUsed: 55.4, resetsAt: "x" },
          ],
        },
        { paneId: "p3", at: 50, rateLimits: [{ kind: "seven_day", percentUsed: 5, resetsAt: null }] },
      ],
      of,
    );
    expect(usage.main.fiveHour?.percentUsed).toBe(40);
    expect(usage.main.weekly?.percentUsed).toBe(55.4);
    expect(usage.perso.fiveHour).toBeUndefined();
    expect(usagePercent(usage.main.weekly)).toBe(55);
    expect(usagePercent(usage.perso.fiveHour)).toBeNull();
  });
  it("ignores unknown panes, gateway windows and non-numbers", () => {
    const usage = usageByAccount(
      [
        { paneId: "zz", at: 1, rateLimits: [{ kind: "five_hour", percentUsed: 9, resetsAt: null }] },
        { paneId: "p1", at: 1, rateLimits: [{ kind: "monthly", percentUsed: 9, resetsAt: null }] },
        { paneId: "p1", at: 2, rateLimits: [{ kind: "five_hour", percentUsed: NaN, resetsAt: null }] },
      ],
      of,
    );
    expect(usage).toEqual({});
  });
  it("clamps a percent", () => {
    expect(usagePercent({ percentUsed: 140, resetsAt: null, at: 0 })).toBe(100);
  });
});

describe("labels", () => {
  it("names an account or says unknown", () => {
    expect(accountName(accounts, "perso", "unknown")).toBe("Perso");
    expect(accountName(accounts, "gone", "unknown")).toBe("unknown");
  });
  it("labels the default", () => {
    expect(defaultLabel(accounts, { claudeAccount: "perso" }, "Default ({name})", "unknown")).toBe(
      "Default (Perso)",
    );
    expect(defaultLabel(accounts, {}, "Default ({name})", "unknown")).toBe("Default (Principal)");
    expect(defaultLabel(accounts, { claudeAccount: "gone" }, "Default ({name})", "unknown")).toBe(
      "Default (Principal)",
    );
  });
  it("lists the workspaces on an account", () => {
    expect(ids(workspacesUsing(workspaces, "perso"))).toEqual(["w2"]);
  });
});
