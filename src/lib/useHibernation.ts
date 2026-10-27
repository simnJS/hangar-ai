import { useEffect, useMemo, useRef } from "react";
import {
  forcedSleepVerdict,
  hibernationVerdict,
  HIBERNATE_TICK_MS,
  stepAside,
  type Awake,
  type PaneRest,
  type Verdict,
} from "./hibernate";
import {
  isAsleep,
  lastTouch,
  markAsleep,
  markAwake,
  paneRest,
  pruneAsleep,
  resumeOnWake,
  touchWorkspace,
} from "./hibernateStore";
import { ptyHasChildren, ptyKill } from "./ipc";
import { isDelivering } from "./deliveryRuns";
import { deliveringIn } from "./deliveryControl";
import { getPaneInsight } from "./paneInsight";
import { sidebarOrder } from "./folders";
import type { AppState, Workspace } from "../types";

/** What a workspace is put to sleep from: the app as it stands, read when asked. */
export interface SleepContext {
  snapshot: () => AppState;
  /** The workspaces whose terminals the main window has mounted. */
  openedIds: readonly string[];
  /** The workspaces the main window showed, the latest first. */
  recent: readonly string[];
  /** Unmounts a workspace's terminals, which ends their PTYs. */
  unmount: (workspaceId: string) => void;
  /** Shows another workspace in the main window. */
  show: (workspaceId: string) => void;
}

interface Options extends Omit<SleepContext, "recent"> {
  /** The sweep: off in a workspace window, before the store loads, and when the setting is. */
  enabled: boolean;
  delayMs: number;
  /** The workspace the main window shows. */
  activeId: string | null;
  /** Says on screen which workspaces went to sleep, and how many agents stopped. */
  announce: (workspaces: Workspace[], agents: number) => void;
  /** Says why a workspace the user asked to put to sleep stays awake. */
  refuse: (workspace: Workspace, verdict: Awake) => void;
}

/** Putting one workspace to sleep by hand: the navigator's menu, the palette. */
export interface ForcedSleep {
  /**
   * What the rules say right now, from what the window knows without asking
   * the system — `probe` when only the shells' processes are left to ask
   * about. Cheap and without side effects, so a render can ask.
   */
  verdict: (workspaceId: string) => Verdict | null;
  /** The answer once the shells have been asked about, which takes a moment. */
  check: (workspaceId: string) => Promise<Verdict | null>;
  /**
   * Puts it to sleep, the main window moving on to another workspace first
   * when it shows this one — or says why it stays awake. Everything is read
   * and asked again at the click: the menu was only what things looked like
   * when it opened.
   */
  run: (workspaceId: string) => Promise<void>;
}

/**
 * Puts the workspaces nobody uses to sleep (lib/hibernate), and wakes them,
 * and puts one to sleep when the user asks.
 *
 * One timer for the whole window, every thirty seconds. Use is counted from
 * the workspace on screen: it is touched on every tick and once more as it
 * leaves the screen, so its delay starts when the user looks away. Typing,
 * clicking, a broadcast or a saved command can only reach the workspace on
 * screen, and are counted where they happen as well; what reaches one out of
 * sight is an agent over MCP, which lib/control counts.
 *
 * Asked by hand, the same decision without the clock (`forcedSleepVerdict`)
 * and the same way to sleep (`putToSleep`), whether or not the setting lets
 * the timer run.
 *
 * Waking needs nothing from here: whatever mounts the workspace again — a
 * click in the navigator, an agent — starts its panes, and each resumes its
 * session (lib/hibernateStore).
 */
export function useHibernation(options: Options): ForcedSleep {
  const { enabled, activeId, openedIds } = options;

  // The delay of the workspace the user leaves starts as they leave it.
  useEffect(() => {
    if (!enabled || !activeId) return;
    touchWorkspace(activeId);
    return () => touchWorkspace(activeId);
  }, [enabled, activeId]);

  // Where the main window goes back to when the one it shows is put to sleep.
  const recent = useRef<string[]>([]);
  useEffect(() => {
    if (activeId) recent.current = [activeId, ...recent.current.filter((id) => id !== activeId)];
  }, [activeId]);

  // Mounted again: awake, and as good as used just now.
  useEffect(() => {
    for (const id of openedIds) {
      if (isAsleep(id)) {
        markAwake(id);
        touchWorkspace(id);
      }
    }
  }, [openedIds]);

  // Read as the sweep goes rather than captured when the timer was set: a
  // probe takes a moment, and the app moves on meanwhile.
  const live = useRef(options);
  live.current = options;

  const forced = useMemo((): ForcedSleep => {
    const context = (): SleepContext => ({ ...live.current, recent: recent.current });
    return {
      verdict: (id) => readWorkspace(id, context(), FORCED)?.verdict ?? null,
      check: async (id) => (await decide(id, context, FORCED))?.verdict ?? null,
      run: async (id) => {
        const outcome = await forceSleep(id, context);
        if (!outcome) return;
        if (outcome.kind === "slept") live.current.announce([outcome.workspace], outcome.agents);
        // Asleep already — the timer got there first, or a second click
        // did: what was asked is done.
        else if (outcome.verdict.reason !== "asleep") {
          live.current.refuse(outcome.workspace, outcome.verdict);
        }
      },
    };
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let running = false;
    const tick = async () => {
      // A probe of the process table can outlast a tick on a loaded machine.
      if (running) return;
      running = true;
      try {
        const slept = await sweep(() => ({ ...live.current, recent: recent.current }));
        if (slept.length) {
          live.current.announce(
            slept.map(({ workspace }) => workspace),
            slept.reduce((sum, { agents }) => sum + agents, 0),
          );
        }
      } finally {
        running = false;
      }
    };
    const timer = window.setInterval(tick, HIBERNATE_TICK_MS);
    return () => window.clearInterval(timer);
  }, [enabled]);

  return forced;
}

/** Left alone for the delay, or asked for by hand. */
export type SleepMode = { kind: "idle"; delayMs: number } | { kind: "forced" };

const FORCED: SleepMode = { kind: "forced" };

/** One workspace, and what the rules said about it. */
export interface Reading {
  workspace: Workspace;
  verdict: Verdict;
}

const UNKNOWN_PROCESSES: Awake = { kind: "awake", reason: "unknownProcesses" };

type Slept = { workspace: Workspace; agents: number };

/** Every mounted workspace, asked in turn; the ones put to sleep come back. */
export async function sweep(current: () => SleepContext & { delayMs: number }): Promise<Slept[]> {
  const slept: Slept[] = [];
  for (const { id } of current().snapshot().workspaces) {
    const reading = await decide(id, current, { kind: "idle", delayMs: current().delayMs });
    if (reading?.verdict.kind === "sleep") {
      putToSleep(reading.workspace, reading.verdict.agents, current());
      slept.push({ workspace: reading.workspace, agents: reading.verdict.agents });
    }
  }
  // A workspace deleted in its sleep has nothing left to wake.
  pruneAsleep(new Set(current().snapshot().workspaces.map((ws) => ws.id)));
  return slept;
}

export type ForcedOutcome =
  /** `shown` is the workspace the main window moved on to, if it had to. */
  | { kind: "slept"; workspace: Workspace; agents: number; shown: string | null }
  | { kind: "refused"; workspace: Workspace; verdict: Awake };

/**
 * One workspace put to sleep because the user asked: decided like the
 * sweep, without the clock, then put to sleep the same way. The main window
 * moves on before, and only once the answer is yes — a refusal leaves the
 * user where they were. Null when the workspace is gone.
 */
export async function forceSleep(
  workspaceId: string,
  current: () => SleepContext,
): Promise<ForcedOutcome | null> {
  const reading = await decide(workspaceId, current, FORCED);
  if (!reading) return null;
  const { workspace, verdict } = reading;
  if (verdict.kind !== "sleep") {
    // `decide` never ends on a probe; were it to, the shells were not asked.
    return { kind: "refused", workspace, verdict: verdict.kind === "awake" ? verdict : UNKNOWN_PROCESSES };
  }
  const context = current();
  let shown: string | null = null;
  if (context.snapshot().activeWorkspaceId === workspaceId) {
    shown = stepAsideFrom(workspaceId, context);
    if (!shown) return { kind: "refused", workspace, verdict: { kind: "awake", reason: "alone" } };
    context.show(shown);
  }
  putToSleep(workspace, verdict.agents, context);
  return { kind: "slept", workspace, agents: verdict.agents, shown };
}

/**
 * Whether one workspace goes to sleep now. Asking the system about its shells
 * takes a moment, during which anything can change — the user opens it, an
 * agent starts a turn — so after a probe everything is read again, answer in
 * hand, and only that second reading decides. Null when the workspace is gone.
 */
export async function decide(
  workspaceId: string,
  current: () => SleepContext,
  mode: SleepMode,
): Promise<Reading | null> {
  let probed: Map<string, boolean> | undefined;
  for (;;) {
    const reading = readWorkspace(workspaceId, current(), mode, probed);
    if (!reading || reading.verdict.kind !== "probe") return reading;
    // Shells that came while the others were being asked: nobody asked them.
    if (probed) return { workspace: reading.workspace, verdict: UNKNOWN_PROCESSES };
    try {
      const running = new Set(await ptyHasChildren(reading.verdict.paneIds));
      probed = new Map(reading.verdict.paneIds.map((id) => [id, running.has(id)]));
    } catch {
      // Not knowing what a shell runs is reason enough to leave it running.
      return { workspace: reading.workspace, verdict: UNKNOWN_PROCESSES };
    }
  }
}

/**
 * One workspace as it stands, put to the rules of `mode`; `probed` is what
 * the system said its shells run, once asked. Null when it is gone.
 */
export function readWorkspace(
  workspaceId: string,
  context: SleepContext,
  mode: SleepMode,
  probed?: ReadonlyMap<string, boolean>,
): Reading | null {
  const state = context.snapshot();
  const workspace = state.workspaces.find((ws) => ws.id === workspaceId);
  if (!workspace) return null;
  const asleep = isAsleep(workspaceId);
  // Put to sleep, a workspace is unmounted on the window's next render: from
  // the moment it is marked asleep it no longer counts as mounted.
  const mounted = context.openedIds.includes(workspaceId) && !asleep;
  const onScreen = workspaceId === state.activeWorkspaceId;
  const seen = {
    panes: workspace.panes,
    mounted,
    onScreen,
    detached: Boolean(workspace.detached),
    // A worker's delivery_submit is tracked by lib/deliveryControl, a
    // « Fusionner » by lib/deliveryRuns: either keeps the workspace up.
    delivering: isDelivering(workspaceId) || deliveringIn(workspaceId),
    rest: Object.fromEntries(workspace.panes.map((pane) => [pane.id, restOf(pane.id, probed)])),
  };

  if (mode.kind === "forced") {
    const canStepAside = !onScreen || stepAsideFrom(workspaceId, context) !== null;
    return { workspace, verdict: forcedSleepVerdict({ ...seen, asleep, canStepAside }) };
  }

  const now = Date.now();
  // Never seen before — mounted by an agent, say: its delay starts now. Not
  // before it is mounted, or it would start the delay already spent.
  if (mounted && (onScreen || lastTouch(workspaceId) === undefined)) {
    touchWorkspace(workspaceId, now);
  }
  const verdict = hibernationVerdict({
    ...seen,
    now,
    delayMs: mode.delayMs,
    lastInteraction: lastTouch(workspaceId) ?? now,
  });
  return { workspace, verdict };
}

/** Where the main window would go if `workspaceId` left it (lib/hibernate). */
function stepAsideFrom(workspaceId: string, context: SleepContext): string | null {
  const state = context.snapshot();
  return stepAside({
    current: workspaceId,
    workspaces: sidebarOrder(state.workspaces, state.folders),
    recent: context.recent,
    mounted: context.openedIds.filter((id) => !isAsleep(id)),
  });
}

function restOf(paneId: string, probed?: ReadonlyMap<string, boolean>): PaneRest | undefined {
  const rest = paneRest(paneId);
  if (!rest) return undefined;
  return {
    ...rest,
    queued: getPaneInsight(paneId).queue.length,
    hasChildren: probed?.get(paneId),
  };
}

/**
 * The one way a workspace goes to sleep, whoever decided it: every pane's
 * process tree killed, its agents owed their sessions on the next launch,
 * the workspace marked asleep and its terminals unmounted.
 */
export function putToSleep(
  workspace: Workspace,
  agents: number,
  context: Pick<SleepContext, "unmount">,
) {
  // Every agent pane has a session by now (lib/hibernate): each resumes it.
  resumeOnWake(workspace.panes.filter((pane) => pane.agent !== "shell").map((pane) => pane.id));
  // The whole tree under each shell, so the agents' helpers go too. The
  // unmount below kills the shells again, which costs nothing.
  for (const pane of workspace.panes) ptyKill(pane.id, true).catch(() => undefined);
  markAsleep(workspace.id, { at: Date.now(), agents });
  context.unmount(workspace.id);
}
