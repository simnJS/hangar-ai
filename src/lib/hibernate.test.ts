import { describe, expect, it, vi } from "vitest";
import {
  forcedSleepVerdict,
  hibernationVerdict,
  sleepRefusal,
  stepAside,
  type Blocker,
  type ForcedSleepInput,
  type HibernateInput,
  type PaneRest,
} from "./hibernate";
import type { AgentId, Pane } from "../types";

// translate() needs the dictionaries, not the app's store, which talks to Tauri.
vi.mock("../store", () => ({ useStore: () => ({}) }));
const { translate } = await import("../i18n");

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

describe("forcedSleepVerdict", () => {
  /** At rest, but only just: every clock the timer waits on reads zero. */
  const justNow = (patch: Partial<PaneRest> = {}) =>
    rest({ lastOutputAt: NOW, restingSince: NOW, ...patch });

  function forced(patch: Partial<ForcedSleepInput> = {}): ForcedSleepInput {
    const panes = patch.panes ?? [pane("a", "claude"), pane("b", "claude"), pane("c", "codex")];
    return {
      asleep: false,
      mounted: true,
      onScreen: false,
      canStepAside: true,
      detached: false,
      delivering: false,
      ...patch,
      panes,
      rest: patch.rest ?? Object.fromEntries(panes.map((p) => [p.id, justNow()])),
    };
  }

  it("puts to sleep a workspace used a second ago, its agents only just done", () => {
    expect(forcedSleepVerdict(forced())).toEqual({ kind: "sleep", agents: 3 });
  });

  it("leaves alone a workspace asleep, in a window of its own, or never opened", () => {
    expect(reason(forcedSleepVerdict(forced({ asleep: true, mounted: false })))).toBe("asleep");
    expect(reason(forcedSleepVerdict(forced({ detached: true })))).toBe("detached");
    expect(reason(forcedSleepVerdict(forced({ mounted: false })))).toBe("notMounted");
  });

  it("takes the workspace on screen once the window has another to show", () => {
    expect(forcedSleepVerdict(forced({ onScreen: true })).kind).toBe("sleep");
    expect(reason(forcedSleepVerdict(forced({ onScreen: true, canStepAside: false })))).toBe(
      "alone",
    );
    // Out of sight, having nowhere to go matters to nobody.
    expect(forcedSleepVerdict(forced({ canStepAside: false })).kind).toBe("sleep");
  });

  it("never stops a worker nor a delivery", () => {
    const worker: Pane = { ...pane("w", "claude"), role: "worker", taskId: "t1" };
    expect(forcedSleepVerdict(forced({ panes: [pane("a", "claude"), worker] }))).toEqual({
      kind: "awake",
      reason: "worker",
      paneId: "w",
    });
    expect(reason(forcedSleepVerdict(forced({ delivering: true })))).toBe("delivering");
  });

  it("keeps awake an agent that could not come back as it was", () => {
    for (const agent of ["gemini", "opencode"] as const) {
      const panes = [pane("a", "claude"), pane("g", agent)];
      expect(forcedSleepVerdict(forced({ panes }))).toEqual({
        kind: "awake",
        reason: "notResumable",
        paneId: "g",
      });
    }
    const fresh = [pane("a", "claude"), pane("b", "codex", null)];
    expect(forcedSleepVerdict(forced({ panes: fresh }))).toEqual({
      kind: "awake",
      reason: "noSession",
      paneId: "b",
    });
  });

  it("keeps awake an agent working, waiting, or with prompts queued", () => {
    const at = (b: Partial<PaneRest>) =>
      forcedSleepVerdict(forced({ rest: { a: justNow(), b: justNow(b), c: justNow() } }));
    expect(at({ busy: true })).toEqual({ kind: "awake", reason: "busy", paneId: "b" });
    expect(at({ queued: 1 })).toEqual({ kind: "awake", reason: "queued", paneId: "b" });
  });

  it("keeps awake while a pane has not been seen at all", () => {
    expect(forcedSleepVerdict(forced({ rest: { a: justNow(), b: justNow() } }))).toEqual({
      kind: "awake",
      reason: "unknownPane",
      paneId: "c",
    });
  });

  describe("shells", () => {
    const panes = [pane("a", "claude"), pane("sh", "shell", null)];
    const shell = (patch: Partial<PaneRest>) =>
      forced({ panes, rest: { a: justNow(), sh: justNow(patch) } });

    it("asks what a shell runs however recently it printed, then goes by the answer", () => {
      expect(forcedSleepVerdict(shell({}))).toEqual({ kind: "probe", paneIds: ["sh"] });
      expect(forcedSleepVerdict(shell({ hasChildren: false }))).toEqual({
        kind: "sleep",
        agents: 1,
      });
      expect(forcedSleepVerdict(shell({ hasChildren: true }))).toEqual({
        kind: "awake",
        reason: "shellChildren",
        paneId: "sh",
      });
    });

    it("holds an agent pane whose agent quit to the same", () => {
      const quit = forced({
        panes: [pane("a", "claude")],
        rest: { a: justNow({ agentExited: true }) },
      });
      expect(forcedSleepVerdict(quit)).toEqual({ kind: "probe", paneIds: ["a"] });
    });

    it("puts quiet shells alone to sleep: nothing to lose, and it was asked", () => {
      const only = [pane("x", "shell", null), pane("y", "shell", null)];
      const quiet = { x: rest({ hasChildren: false }), y: rest({ hasChildren: false }) };
      expect(forcedSleepVerdict(forced({ panes: only, rest: quiet }))).toEqual({
        kind: "sleep",
        agents: 0,
      });
      // Left alone, the same workspace is not worth the timer's while.
      expect(reason(hibernationVerdict(input({ panes: only, rest: quiet })))).toBe("noAgents");
    });
  });

  it("never answers with the timer's clock", () => {
    const timed: Blocker[] = ["recentInteraction", "recentlyBusy", "shellOutput", "onScreen"];
    const cases: Partial<ForcedSleepInput>[] = [
      {},
      { onScreen: true },
      { panes: [pane("a", "claude"), pane("sh", "shell", null)] },
      { rest: { a: justNow({ busy: true }), b: justNow(), c: justNow() } },
    ];
    for (const patch of cases) {
      const verdict = forcedSleepVerdict(forced(patch));
      if (verdict.kind === "awake") expect(timed).not.toContain(verdict.reason);
    }
  });
});

describe("stepAside", () => {
  const ws = (id: string, detached = false) => ({ id, detached });
  const workspaces = [ws("a"), ws("b"), ws("c"), ws("d")];
  const from = (recent: string[], mounted: string[], list = workspaces) =>
    stepAside({ current: "a", workspaces: list, recent, mounted });

  it("goes back to the workspace used last before this one", () => {
    expect(from(["a", "c", "b"], ["a", "b", "c"])).toBe("c");
  });

  it("passes over one asleep, so putting several to sleep never wakes the last", () => {
    // c was shown just before, then put to sleep: it is no longer mounted.
    expect(from(["a", "c", "b"], ["a", "b"])).toBe("b");
  });

  it("takes the first awake in the navigator when none was shown before", () => {
    expect(from(["a"], ["a", "d", "c"])).toBe("c");
  });

  it("wakes one rather than leave the window empty, the one used last first", () => {
    expect(from(["a", "d"], ["a"])).toBe("d");
    expect(from(["a"], ["a"])).toBe("b");
  });

  it("never goes to a workspace in a window of its own, and says when there is nowhere", () => {
    expect(from(["a", "b"], ["a", "b"], [ws("a"), ws("b", true), ws("c")])).toBe("c");
    expect(from(["a", "b"], ["a", "b"], [ws("a"), ws("b", true)])).toBeNull();
    expect(from(["a"], ["a"], [ws("a")])).toBeNull();
  });
});

describe("sleepRefusal", () => {
  type Args = Parameters<typeof translate>;
  const fr = (key: Args[1], params?: Args[2]) => translate("fr", key, params);
  const en = (key: Args[1], params?: Args[2]) => translate("en", key, params);
  const panes = [
    { ...pane("p1", "claude"), name: "Ava" },
    { ...pane("p2", "shell", null), name: "Leo" },
    { ...pane("p3", "gemini", null), name: "Gus" },
  ];
  const say = (
    t: typeof fr,
    reason: Blocker,
    paneId?: string,
    waiting: (id: string) => boolean = () => false,
  ) => sleepRefusal(t, { kind: "awake", reason, paneId }, panes, waiting);

  it("names the pane that keeps the workspace awake", () => {
    expect(say(fr, "busy", "p1")).toBe("Ava travaille encore");
    expect(say(fr, "busy", "p1", (id) => id === "p1")).toBe("Ava attend ta réponse");
    expect(say(fr, "shellChildren", "p2")).toBe("Un processus tourne dans le shell de Leo");
    expect(say(fr, "notResumable", "p3")).toBe("Gemini ne reprend pas sa session (Gus)");
    expect(say(en, "shellChildren", "p2")).toBe("A process is running in Leo's shell");
    expect(say(en, "notResumable", "p3")).toBe("Gemini cannot resume its session (Gus)");
  });

  it("has a sentence for every reason, in both languages", () => {
    const every: Record<Blocker, true> = {
      asleep: true,
      notMounted: true,
      onScreen: true,
      alone: true,
      detached: true,
      worker: true,
      delivering: true,
      recentInteraction: true,
      noAgents: true,
      unknownPane: true,
      notResumable: true,
      noSession: true,
      busy: true,
      recentlyBusy: true,
      queued: true,
      shellOutput: true,
      shellChildren: true,
      unknownProcesses: true,
    };
    for (const t of [fr, en]) {
      for (const reason of Object.keys(every) as Blocker[]) {
        const text = say(t, reason, "p1");
        expect(text, reason).not.toMatch(/^sleep\.|[{}]|undefined/);
        expect(text.length, reason).toBeGreaterThan(5);
      }
    }
  });

  it("reads as a notice when the click finds the answer changed", () => {
    const reason = say(fr, "shellChildren", "p2");
    expect(fr("sleep.refused", { name: "checkout", reason })).toBe(
      "checkout reste éveillé. Un processus tourne dans le shell de Leo.",
    );
  });
});
