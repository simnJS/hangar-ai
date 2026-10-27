import { useEffect, useRef } from "react";
import { hibernationVerdict, HIBERNATE_TICK_MS, type PaneRest } from "./hibernate";
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
import type { AppState, Workspace } from "../types";

interface Options {
  /** Off in a workspace window, before the store loads, and when the setting is. */
  enabled: boolean;
  delayMs: number;
  /** The workspace the main window shows. */
  activeId: string | null;
  snapshot: () => AppState;
  /** The workspaces whose terminals the main window has mounted. */
  openedIds: string[];
  /** Unmounts a workspace's terminals, which ends their PTYs. */
  unmount: (workspaceId: string) => void;
  /** Says on screen which workspaces went to sleep, and how many agents stopped. */
  announce: (workspaces: Workspace[], agents: number) => void;
}

/**
 * Puts the workspaces nobody uses to sleep (lib/hibernate), and wakes them.
 *
 * One timer for the whole window, every thirty seconds. Use is counted from
 * the workspace on screen: it is touched on every tick and once more as it
 * leaves the screen, so its delay starts when the user looks away. Typing,
 * clicking, a broadcast or a saved command can only reach the workspace on
 * screen, and are counted where they happen as well; what reaches one out of
 * sight is an agent over MCP, which lib/control counts.
 *
 * Waking needs nothing from here: whatever mounts the workspace again — a
 * click in the navigator, an agent — starts its panes, and each resumes its
 * session (lib/hibernateStore).
 */
export function useHibernation(options: Options) {
  const { enabled, activeId, openedIds } = options;

  // The delay of the workspace the user leaves starts as they leave it.
  useEffect(() => {
    if (!enabled || !activeId) return;
    touchWorkspace(activeId);
    return () => touchWorkspace(activeId);
  }, [enabled, activeId]);

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

  useEffect(() => {
    if (!enabled) return;
    let running = false;
    const tick = async () => {
      // A probe of the process table can outlast a tick on a loaded machine.
      if (running) return;
      running = true;
      try {
        const slept = await sweep(() => live.current);
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
}

type Slept = { workspace: Workspace; agents: number };

/** Every mounted workspace, asked in turn; the ones put to sleep come back. */
async function sweep(current: () => Options): Promise<Slept[]> {
  const slept: Slept[] = [];
  for (const { id } of current().snapshot().workspaces) {
    const verdict = await decide(id, current);
    if (verdict) {
      putToSleep(verdict.workspace, verdict.agents, current());
      slept.push(verdict);
    }
  }
  // A workspace deleted in its sleep has nothing left to wake.
  pruneAsleep(new Set(current().snapshot().workspaces.map((ws) => ws.id)));
  return slept;
}

/**
 * Whether one workspace goes to sleep now. Asking the system about its shells
 * takes a moment, during which anything can change — the user opens it, an
 * agent starts a turn — so after a probe everything is read again, answer in
 * hand, and only that second reading decides.
 */
async function decide(workspaceId: string, current: () => Options): Promise<Slept | null> {
  let probed: Map<string, boolean> | undefined;
  for (;;) {
    const { snapshot, openedIds, delayMs } = current();
    const state = snapshot();
    const workspace = state.workspaces.find((ws) => ws.id === workspaceId);
    if (!workspace || !openedIds.includes(workspaceId)) return null;
    const now = Date.now();
    const onScreen = workspaceId === state.activeWorkspaceId;
    // Never seen before — mounted by an agent, say: its delay starts now.
    if (onScreen || lastTouch(workspaceId) === undefined) touchWorkspace(workspaceId, now);

    const verdict = hibernationVerdict({
      now,
      delayMs,
      panes: workspace.panes,
      mounted: true,
      onScreen,
      detached: Boolean(workspace.detached),
      // A worker's delivery_submit is tracked by lib/deliveryControl, a
      // « Fusionner » by lib/deliveryRuns: either keeps the workspace up.
      delivering: isDelivering(workspaceId) || deliveringIn(workspaceId),
      lastInteraction: lastTouch(workspaceId) ?? now,
      rest: Object.fromEntries(workspace.panes.map((pane) => [pane.id, restOf(pane.id, probed)])),
    });
    if (verdict.kind === "sleep") return { workspace, agents: verdict.agents };
    if (verdict.kind === "awake" || probed) return null;

    try {
      const running = new Set(await ptyHasChildren(verdict.paneIds));
      probed = new Map(verdict.paneIds.map((id) => [id, running.has(id)]));
    } catch {
      // Not knowing what a shell runs is reason enough to leave it running.
      return null;
    }
  }
}

function restOf(paneId: string, probed?: Map<string, boolean>): PaneRest | undefined {
  const rest = paneRest(paneId);
  if (!rest) return undefined;
  return {
    ...rest,
    queued: getPaneInsight(paneId).queue.length,
    hasChildren: probed?.get(paneId),
  };
}

function putToSleep(workspace: Workspace, agents: number, options: Options) {
  // Every agent pane has a session by now (lib/hibernate): each resumes it.
  resumeOnWake(workspace.panes.filter((pane) => pane.agent !== "shell").map((pane) => pane.id));
  // The whole tree under each shell, so the agents' helpers go too. The
  // unmount below kills the shells again, which costs nothing.
  for (const pane of workspace.panes) ptyKill(pane.id, true).catch(() => undefined);
  markAsleep(workspace.id, { at: Date.now(), agents });
  options.unmount(workspace.id);
}
