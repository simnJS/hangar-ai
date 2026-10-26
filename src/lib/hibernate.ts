import { isResumable } from "./agents";
import type { Pane } from "../types";

/**
 * Whether a workspace nobody uses can be put to sleep: its terminals closed to
 * give the memory its agents hold back, and reopened — every agent on the
 * session it was in — the next time anyone looks at it.
 *
 * Nothing here touches the app. The window gathers what it knows about each
 * pane (lib/hibernateStore, the panes' own state) and asks; a test hands it
 * whatever it likes. The rules lean one way throughout: anything that cannot
 * be shown to be at rest keeps the workspace awake, because a conversation
 * lost to save a few hundred megabytes is not a trade anyone asked for.
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
  /** When someone last typed into it, clicked it, or sent it anything. */
  lastInteraction: number;
  /** By pane id; a pane with no entry has not been seen and counts as busy. */
  rest: Record<string, PaneRest | undefined>;
}

/** Why a workspace stays awake: the first rule it fails. */
export type Blocker =
  | "notMounted"
  | "onScreen"
  | "detached"
  | "recentInteraction"
  | "noAgents"
  | "unknownPane"
  | "notResumable"
  | "noSession"
  | "busy"
  | "recentlyBusy"
  | "queued"
  | "shellOutput"
  | "shellChildren";

export type Verdict =
  | { kind: "awake"; reason: Blocker; paneId?: string }
  /** Every rule holds but whether these shells still run something. */
  | { kind: "probe"; paneIds: string[] }
  /** Put it to sleep: `agents` panes running an agent will be stopped. */
  | { kind: "sleep"; agents: number };

/** "At least the delay": a pane quiet for exactly that long has waited enough. */
const waited = (since: number, now: number, delayMs: number) => now - since >= delayMs;

/**
 * The rules, in the order that answers cheapest:
 *
 * 1. Mounted here, not the workspace on screen, not in a window of its own.
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
  const { now, delayMs, panes, rest } = input;
  if (!input.mounted) return { kind: "awake", reason: "notMounted" };
  if (input.onScreen) return { kind: "awake", reason: "onScreen" };
  if (input.detached) return { kind: "awake", reason: "detached" };
  if (!waited(input.lastInteraction, now, delayMs)) {
    return { kind: "awake", reason: "recentInteraction" };
  }

  const agents = panes.filter((pane) => pane.agent !== "shell");
  if (agents.length === 0) return { kind: "awake", reason: "noAgents" };

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
      if (!waited(state.restingSince, now, delayMs)) return awake("recentlyBusy");
      if (state.queued > 0) return awake("queued");
      shell = state.agentExited;
    }
    if (!shell) continue;

    if (!waited(state.lastOutputAt, now, delayMs)) return awake("shellOutput");
    if (state.hasChildren === true) return awake("shellChildren");
    if (state.hasChildren === undefined) shells.push(pane.id);
  }

  if (shells.length > 0) return { kind: "probe", paneIds: shells };
  return { kind: "sleep", agents: agents.length };
}

/** The delays the settings offer, in minutes. */
export const HIBERNATE_DELAYS = [5, 10, 20, 30, 60];

/** How often the window looks for a workspace to put to sleep. */
export const HIBERNATE_TICK_MS = 30_000;
