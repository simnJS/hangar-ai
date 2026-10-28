import { isResumable } from "./agents";
import { AGENTS, type Pane, type Workspace } from "../types";
import type { Translator } from "../i18n";

/**
 * Whether a workspace can be put to sleep: its terminals closed to give the
 * memory its agents hold back, and reopened — every agent on the session it
 * was in — the next time anyone looks at it. Either because nobody has used
 * it for a while (`hibernationVerdict`), or because the user asked for it from
 * the navigator or the palette (`forcedSleepVerdict`).
 *
 * Nothing here touches the app. The window gathers what it knows about each
 * pane (lib/hibernateStore, the panes' own state) and asks; a test hands it
 * whatever it likes. The rules lean one way throughout: anything that cannot
 * be shown to be at rest keeps the workspace awake, because a conversation
 * lost to save a few hundred megabytes is not a trade anyone asked for — and
 * asking for the sleep by hand changes how long to wait, never that.
 */

/** What the window knows about one pane of the workspace. */
export interface PaneRest {
  /** When it last printed anything, or when it was mounted if it never did. */
  lastOutputAt: number;
  /** Its agent is working or waiting for an answer right now. */
  busy: boolean;
  /** When its agent last stopped working or waiting, or when it was mounted. */
  restingSince: number;
  /** Prompts held for it until its agent is free (the hangar-bridge queue). */
  queued: number;
  /** The agent quit back to its shell: what the pane runs now is anyone's guess. */
  agentExited: boolean;
  /**
   * Whether its shell has started a process that is still running — a dev
   * server, a watcher, a build. Undefined until the system has been asked,
   * which is only worth doing once everything else says yes.
   */
  hasChildren?: boolean;
}

export interface HibernateInput {
  now: number;
  /** How long a workspace has to be left alone, in ms. */
  delayMs: number;
  panes: Pane[];
  /** Its terminals are mounted in this window. */
  mounted: boolean;
  /** It is the workspace the main window shows. */
  onScreen: boolean;
  /** It is shown in a window of its own. */
  detached: boolean;
  /**
   * One of its deliveries is being checked, reviewed or landed right now
   * (lib/deliveryRuns): stopping it halfway would leave a merge half done.
   */
  delivering: boolean;
  /** When someone last typed into it, clicked it, or sent it anything. */
  lastInteraction: number;
  /** By pane id; a pane with no entry has not been seen and counts as busy. */
  rest: Record<string, PaneRest | undefined>;
}

/** What a sleep asked for by hand is decided on: no clock, the same protections. */
export interface ForcedSleepInput {
  panes: Pane[];
  /** Asleep already: there is nothing left to stop. */
  asleep: boolean;
  mounted: boolean;
  /** It is the workspace the main window shows: it moves on to another first. */
  onScreen: boolean;
  /** The main window has another workspace to show in its place (`stepAside`). */
  canStepAside: boolean;
  detached: boolean;
  delivering: boolean;
  rest: Record<string, PaneRest | undefined>;
}

/** Why a workspace stays awake: the first rule it fails. */
export type Blocker =
  | "asleep"
  | "notMounted"
  | "onScreen"
  /** On screen, with nothing else for the main window to show instead. */
  | "alone"
  | "detached"
  | "worker"
  /** The project manager lives here: it must hear events, so the HQ never sleeps. */
  | "manager"
  | "delivering"
  | "recentInteraction"
  | "noAgents"
  | "unknownPane"
  | "notResumable"
  | "noSession"
  | "busy"
  | "recentlyBusy"
  | "queued"
  | "shellOutput"
  | "shellChildren"
  /**
   * Never a rule's answer: the system could not say what the shells run, or
   * the workspace changed while it was being asked. Either way, awake.
   */
  | "unknownProcesses";

export type Awake = { kind: "awake"; reason: Blocker; paneId?: string };

export type Verdict =
  | Awake
  /** Every rule holds but whether these shells still run something. */
  | { kind: "probe"; paneIds: string[] }
  /** Put it to sleep: `agents` panes running an agent will be stopped. */
  | { kind: "sleep"; agents: number };

/** "At least the delay": a pane quiet for exactly that long has waited enough. */
const waited = (since: number, now: number, delayMs: number) => now - since >= delayMs;

/** A sleep asked for by hand waits for nothing. */
const always = () => true;

/**
 * The rules, in the order that answers cheapest:
 *
 * 1. Mounted here, not the workspace on screen, not in a window of its own.
 *    Never the HQ (the manager must hear events). No worker of an orchestrator in it, and no delivery running: work
 *    handed out goes on with nobody looking — that is the point of handing
 *    it out — and a worker stopped mid-task, or a delivery mid-merge, is
 *    work lost rather than memory saved.
 * 2. Nobody has touched it for the delay.
 * 3. At least one pane runs an agent: freeing agents is the point, and a
 *    workspace of shells holds little memory and all its value on screen.
 * 4. Every agent pane can come back as it was: Claude Code or Codex, with a
 *    session id captured. Gemini and OpenCode cannot resume, and a pane
 *    without an id would come back on a blank conversation.
 * 5. Every agent has been idle or handed back for the delay — not working,
 *    not waiting for an answer — with nothing queued for it.
 * 6. Every shell has printed nothing for the delay and runs no process of its
 *    own. An agent pane whose agent quit is a shell again and is held to the
 *    same. Only then are the shells' processes asked about (`probe`).
 */
export function hibernationVerdict(input: HibernateInput): Verdict {
  const { now, delayMs, panes } = input;
  if (!input.mounted) return { kind: "awake", reason: "notMounted" };
  if (input.onScreen) return { kind: "awake", reason: "onScreen" };
  if (input.detached) return { kind: "awake", reason: "detached" };
  const handedOut = workInFlight(panes, input.delivering);
  if (handedOut) return handedOut;
  if (!waited(input.lastInteraction, now, delayMs)) {
    return { kind: "awake", reason: "recentInteraction" };
  }
  if (!panes.some((pane) => pane.agent !== "shell")) return { kind: "awake", reason: "noAgents" };
  return panesAtRest(panes, input.rest, (since) => waited(since, now, delayMs));
}

/**
 * The same rules for a sleep the user asked for, minus the clock: nobody has
 * to have left it alone, nor an agent to have rested, nor a shell to have
 * been quiet, for any length of time. Every protection stays — an agent at
 * work or waiting, prompts queued, a process in a shell, a session that could
 * not come back, a worker, a delivery — since asking for it by hand is no
 * reason to lose what those protect.
 *
 * Two rules read differently. The workspace on screen can go, once the main
 * window has another to show in its place — a window of its own has nothing
 * else to show, so a workspace there stays. And a workspace of shells alone
 * can go too: there is little memory to free, but nothing to lose either,
 * and the user asked.
 */
export function forcedSleepVerdict(input: ForcedSleepInput): Verdict {
  if (input.asleep) return { kind: "awake", reason: "asleep" };
  if (input.detached) return { kind: "awake", reason: "detached" };
  if (!input.mounted) return { kind: "awake", reason: "notMounted" };
  if (input.onScreen && !input.canStepAside) return { kind: "awake", reason: "alone" };
  return workInFlight(input.panes, input.delivering) ?? panesAtRest(input.panes, input.rest, always);
}

/**
 * Rule 1's second half: work an orchestrator handed out goes on with nobody
 * looking — that is the point of handing it out — and a worker stopped
 * mid-task, or a delivery mid-merge, is work lost rather than memory saved.
 */
function workInFlight(panes: Pane[], delivering: boolean): Awake | null {
  const manager = panes.find((pane) => pane.role === "manager");
  if (manager) return { kind: "awake", reason: "manager", paneId: manager.id };
  const worker = panes.find((pane) => pane.role === "worker");
  if (worker) return { kind: "awake", reason: "worker", paneId: worker.id };
  if (delivering) return { kind: "awake", reason: "delivering" };
  return null;
}

/**
 * Rules 4 to 6, pane by pane. `rested` says whether a pane has been quiet
 * long enough since a moment: the delay for a workspace left alone, any time
 * at all for one put to sleep by hand.
 */
function panesAtRest(
  panes: Pane[],
  rest: Record<string, PaneRest | undefined>,
  rested: (since: number) => boolean,
): Verdict {
  const shells: string[] = [];
  for (const pane of panes) {
    const awake = (reason: Blocker): Verdict => ({ kind: "awake", reason, paneId: pane.id });
    const state = rest[pane.id];
    if (!state) return awake("unknownPane");

    let shell = pane.agent === "shell";
    if (!shell) {
      if (!isResumable(pane.agent)) return awake("notResumable");
      if (!pane.sessionId) return awake("noSession");
      if (state.busy) return awake("busy");
      if (!rested(state.restingSince)) return awake("recentlyBusy");
      if (state.queued > 0) return awake("queued");
      shell = state.agentExited;
    }
    if (!shell) continue;

    if (!rested(state.lastOutputAt)) return awake("shellOutput");
    if (state.hasChildren === true) return awake("shellChildren");
    if (state.hasChildren === undefined) shells.push(pane.id);
  }

  if (shells.length > 0) return { kind: "probe", paneIds: shells };
  return { kind: "sleep", agents: panes.filter((pane) => pane.agent !== "shell").length };
}

export interface StepAsideInput {
  /** The workspace leaving the screen. */
  current: string;
  /** Every workspace, in the navigator's order. */
  workspaces: Pick<Workspace, "id" | "detached">[];
  /** The workspaces the main window showed, the latest first. */
  recent: readonly string[];
  /** The workspaces whose terminals are mounted in the main window. */
  mounted: readonly string[];
}

/**
 * Where the main window goes when the workspace it shows is put to sleep:
 * the one used last before it among those still awake — so that putting
 * several to sleep in a row never wakes the one put to sleep just before —
 * or, none of those shown yet, the first awake in the navigator. With none
 * awake, the same among the others, which being shown wakes: a start, but
 * nothing lost. Never one in a window of its own, of which the main window
 * would only say that it is elsewhere. Null when there is nowhere to go.
 */
export function stepAside({ current, workspaces, recent, mounted }: StepAsideInput): string | null {
  const candidates = workspaces.filter((ws) => ws.id !== current && !ws.detached);
  const rank = (id: string) => {
    const at = recent.indexOf(id);
    return at < 0 ? Number.POSITIVE_INFINITY : at;
  };
  const order = (list: typeof candidates) =>
    // Stable, and Infinity - Infinity is NaN: ties keep the navigator's order.
    [...list].sort((a, b) => rank(a.id) - rank(b.id) || 0);
  const awake = candidates.filter((ws) => mounted.includes(ws.id));
  return (order(awake)[0] ?? order(candidates)[0])?.id ?? null;
}

/** The delays the settings offer, in minutes. */
export const HIBERNATE_DELAYS = [5, 10, 20, 30, 60];

/** How often the window looks for a workspace to put to sleep. */
export const HIBERNATE_TICK_MS = 30_000;

/**
 * Why a workspace the user asked to put to sleep stays awake, in a sentence:
 * the tooltip of the navigator's entry, what a screen reader reads with it,
 * and the notice when the answer changes between the menu and the click.
 * About the pane that said no, by its name. `waiting` tells an agent waiting
 * for an answer from one at work, which the rules count alike.
 */
export function sleepRefusal(
  t: Translator,
  verdict: Awake,
  panes: Pane[],
  waiting: (paneId: string) => boolean,
): string {
  const pane = panes.find((entry) => entry.id === verdict.paneId);
  const name = pane?.name ?? "";
  switch (verdict.reason) {
    case "asleep":
      return t("sleep.asleep");
    case "detached":
      return t("sleep.detached");
    case "notMounted":
      return t("sleep.notMounted");
    case "alone":
      return t("sleep.alone");
    case "worker":
      return t("sleep.worker", { name });
    case "manager":
      return t("sleep.manager", { name });
    case "delivering":
      return t("sleep.delivering");
    case "noAgents":
      return t("sleep.noAgents");
    case "unknownPane":
      return t("sleep.starting", { name });
    case "notResumable":
      return t("sleep.notResumable", {
        name,
        agent: AGENTS.find((agent) => agent.id === pane?.agent)?.label ?? pane?.agent ?? "",
      });
    case "noSession":
      return t("sleep.noSession", { name });
    case "busy":
    case "recentlyBusy":
      return t(pane && waiting(pane.id) ? "sleep.waiting" : "sleep.working", { name });
    case "queued":
      return t("sleep.queued", { name });
    case "shellChildren":
      return t("sleep.shellChildren", { name });
    case "unknownProcesses":
      return t("sleep.unknownProcesses");
    // Only the clock of a workspace left alone says these, and a sleep asked
    // for by hand has none.
    case "onScreen":
    case "recentInteraction":
    case "shellOutput":
      return t("sleep.inUse");
  }
}
