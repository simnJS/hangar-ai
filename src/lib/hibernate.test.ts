import { describe, expect, it } from "vitest";
import { hibernationVerdict, type HibernateInput, type PaneRest } from "./hibernate";
import type { AgentId, Pane } from "../types";

const MIN = 60_000;
const DELAY = 10 * MIN;
const NOW = 1_000 * MIN;
/** Long enough ago for every rule. */
const LONG_AGO = NOW - 3 * DELAY;

function pane(id: string, agent: AgentId, sessionId: string | null = `s-${id}`): Pane {
  return { id, name: id, agent, sessionId, cwd: null, shellId: null, title: null };
}

/** At rest since long ago; a shell whose children are not known yet. */
function rest(patch: Partial<PaneRest> = {}): PaneRest {
  return {
    lastOutputAt: LONG_AGO,
    busy: false,
    restingSince: LONG_AGO,
    queued: 0,
    agentExited: false,
    ...patch,
  };
}

/** Two Claude Code panes and a Codex, all resting: a workspace ready to sleep. */
function input(patch: Partial<HibernateInput> = {}): HibernateInput {
  const panes = patch.panes ?? [pane("a", "claude"), pane("b", "claude"), pane("c", "codex")];
  return {
    now: NOW,
    delayMs: DELAY,
    mounted: true,
    onScreen: false,
    detached: false,
    delivering: false,
    lastInteraction: LONG_AGO,
    ...patch,
    panes,
    rest: patch.rest ?? Object.fromEntries(panes.map((p) => [p.id, rest()])),
  };
}

const reason = (value: ReturnType<typeof hibernationVerdict>) =>
  value.kind === "awake" ? value.reason : value.kind;

describe("hibernationVerdict", () => {
  it("puts an untouched workspace of resting, resumable agents to sleep", () => {
    expect(hibernationVerdict(input())).toEqual({ kind: "sleep", agents: 3 });
  });

  it("never touches a workspace that is not mounted, on screen or in its own window", () => {
    expect(reason(hibernationVerdict(input({ mounted: false })))).toBe("notMounted");
    expect(reason(hibernationVerdict(input({ onScreen: true })))).toBe("onScreen");
    expect(reason(hibernationVerdict(input({ detached: true })))).toBe("detached");
  });

  it("waits out the whole delay after the last interaction, and not a millisecond more", () => {
    expect(reason(hibernationVerdict(input({ lastInteraction: NOW - DELAY + 1 })))).toBe(
      "recentInteraction",
    );
    expect(reason(hibernationVerdict(input({ lastInteraction: NOW })))).toBe("recentInteraction");
    expect(hibernationVerdict(input({ lastInteraction: NOW - DELAY })).kind).toBe("sleep");
  });

  it("follows the delay it is given", () => {
    const fiveMinutes = { delayMs: 5 * MIN, lastInteraction: NOW - 6 * MIN };
    expect(hibernationVerdict(input(fiveMinutes)).kind).toBe("sleep");
    expect(reason(hibernationVerdict(input({ ...fiveMinutes, delayMs: 60 * MIN })))).toBe(
      "recentInteraction",
    );
  });

  it("leaves a workspace of shells alone: there is no agent to free", () => {
    const panes = [pane("x", "shell", null), pane("y", "shell", null)];
    expect(reason(hibernationVerdict(input({ panes })))).toBe("noAgents");
  });

  it("keeps awake an agent that could not resume its session", () => {
    for (const agent of ["gemini", "opencode"] as const) {
      const panes = [pane("a", "claude"), pane("g", agent)];
      const verdict = hibernationVerdict(input({ panes }));
      expect(verdict).toEqual({ kind: "awake", reason: "notResumable", paneId: "g" });
    }
  });

  it("keeps awake a resumable agent whose session was never captured", () => {
    const panes = [pane("a", "claude"), pane("b", "codex", null)];
    expect(hibernationVerdict(input({ panes }))).toEqual({
      kind: "awake",
      reason: "noSession",
      paneId: "b",
    });
  });

  it("keeps awake a workspace with an agent working or waiting for an answer", () => {
    const verdict = hibernationVerdict(
      input({ rest: { a: rest(), b: rest({ busy: true }), c: rest() } }),
    );
    expect(verdict).toEqual({ kind: "awake", reason: "busy", paneId: "b" });
  });

  it("waits out the delay after an agent's last turn, to the millisecond", () => {
    const at = (restingSince: number) =>
      hibernationVerdict(input({ rest: { a: rest(), b: rest(), c: rest({ restingSince }) } }));
    expect(reason(at(NOW - DELAY + 1))).toBe("recentlyBusy");
    expect(at(NOW - DELAY).kind).toBe("sleep");
  });

  it("keeps awake an agent with prompts queued for it", () => {
    const verdict = hibernationVerdict(
      input({ rest: { a: rest({ queued: 2 }), b: rest(), c: rest() } }),
    );
    expect(verdict).toEqual({ kind: "awake", reason: "queued", paneId: "a" });
  });

  describe("orchestration", () => {
    const orchestrator: Pane = { ...pane("o", "claude"), role: "orchestrator" };
    const worker: Pane = { ...pane("w", "claude"), role: "worker", taskId: "t1" };

    it("never puts a workspace to sleep while one of its workers is there", () => {
      const panes = [orchestrator, worker, pane("c", "codex")];
      expect(hibernationVerdict(input({ panes }))).toEqual({
        kind: "awake",
        reason: "worker",
        paneId: "w",
      });
    });

    it("holds the workers above everything else a workspace left alone could pass", () => {
      // Busy, never seen, without a session: the worker still answers first.
      const panes = [{ ...worker, sessionId: null }, pane("c", "codex")];
      expect(reason(hibernationVerdict(input({ panes, rest: {} })))).toBe("worker");
    });

    it("never puts a workspace to sleep while one of its deliveries runs", () => {
      expect(reason(hibernationVerdict(input({ delivering: true })))).toBe("delivering");
      const panes = [orchestrator, pane("c", "codex")];
      expect(reason(hibernationVerdict(input({ panes, delivering: true })))).toBe("delivering");
    });

    it("lets an orchestrator with no worker and nothing landing sleep like any agent", () => {
      const panes = [orchestrator, pane("c", "codex")];
      expect(hibernationVerdict(input({ panes }))).toEqual({ kind: "sleep", agents: 2 });
    });

    it("still leaves the workspace on screen to the screen", () => {
      const panes = [orchestrator, worker];
      expect(reason(hibernationVerdict(input({ panes, onScreen: true })))).toBe("onScreen");
    });
  });

  it("keeps awake while a pane has not been seen at all", () => {
    expect(hibernationVerdict(input({ rest: { a: rest(), b: rest() } }))).toEqual({
      kind: "awake",
      reason: "unknownPane",
      paneId: "c",
    });
  });

  describe("shells", () => {
    const panes = [pane("a", "claude"), pane("sh", "shell", null), pane("sh2", "shell", null)];

    it("asks about the shells' processes only once everything else holds", () => {
      expect(hibernationVerdict(input({ panes }))).toEqual({
        kind: "probe",
        paneIds: ["sh", "sh2"],
      });
      // An agent still working is answer enough: nobody is asked anything.
      const busy = { a: rest({ busy: true }), sh: rest(), sh2: rest() };
      expect(reason(hibernationVerdict(input({ panes, rest: busy })))).toBe("busy");
    });

    it("sleeps once the shells are known to run nothing", () => {
      const quiet = { a: rest(), sh: rest({ hasChildren: false }), sh2: rest({ hasChildren: false }) };
      expect(hibernationVerdict(input({ panes, rest: quiet }))).toEqual({ kind: "sleep", agents: 1 });
    });

    it("keeps awake a shell running a dev server, a watcher or a build", () => {
      const running = { a: rest(), sh: rest({ hasChildren: false }), sh2: rest({ hasChildren: true }) };
      expect(hibernationVerdict(input({ panes, rest: running }))).toEqual({
        kind: "awake",
        reason: "shellChildren",
        paneId: "sh2",
      });
    });

    it("keeps awake a shell that printed within the delay, to the millisecond", () => {
      const at = (lastOutputAt: number) =>
        hibernationVerdict(
          input({ panes, rest: { a: rest(), sh: rest({ lastOutputAt }), sh2: rest({ hasChildren: false }) } }),
        );
      expect(at(NOW - DELAY + 1)).toEqual({ kind: "awake", reason: "shellOutput", paneId: "sh" });
      // Quiet for exactly the delay: only its processes are left to ask about.
      expect(at(NOW - DELAY)).toEqual({ kind: "probe", paneIds: ["sh"] });
    });

    it("never asks the shell of a running agent: the agent is its child", () => {
      const agentsOnly = [pane("a", "claude"), pane("b", "codex")];
      expect(hibernationVerdict(input({ panes: agentsOnly })).kind).toBe("sleep");
    });

    it("holds an agent pane whose agent quit to the shell rules", () => {
      const quit = [pane("a", "claude"), pane("b", "claude")];
      const exited = (patch: Partial<PaneRest>) =>
        hibernationVerdict(
          input({ panes: quit, rest: { a: rest(), b: rest({ agentExited: true, ...patch }) } }),
        );
      expect(exited({})).toEqual({ kind: "probe", paneIds: ["b"] });
      expect(exited({ hasChildren: true })).toEqual({
        kind: "awake",
        reason: "shellChildren",
        paneId: "b",
      });
      expect(reason(exited({ lastOutputAt: NOW - MIN }))).toBe("shellOutput");
      expect(exited({ hasChildren: false })).toEqual({ kind: "sleep", agents: 2 });
    });
  });
});
