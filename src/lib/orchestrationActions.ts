import { useMemo } from "react";
import { preferredDir } from "./layout";
import { orchestrationOf, orchestratorOf } from "./orchestration";
import { useStore } from "../store";
import type { Workspace } from "../types";

/**
 * Pinning, unpinning and creating a workspace's orchestrator: the one Claude
 * Code pane of a workspace that plans on the board and starts its workers
 * (lib/orchestration). The pane menu, the palette and the orchestration
 * panel all come through here, so the rule — one per workspace, Claude Code
 * only, never a worker — is held in one place.
 */

/** Why a pane cannot be pinned, or null when it can. */
export type PinRefusal =
  /** No such pane in the workspace. */
  | "missing"
  /** It runs something else than Claude Code: the briefing is Claude Code's mod. */
  | "notClaude"
  /** It is already a worker, on a task of its own. */
  | "worker"
  /** It is the orchestrator already. */
  | "pinned"
  /** Another pane of the workspace is. */
  | "taken";

export function pinRefusal(ws: Pick<Workspace, "panes">, paneId: string): PinRefusal | null {
  const pane = ws.panes.find((entry) => entry.id === paneId);
  if (!pane) return "missing";
  if (pane.role === "orchestrator") return "pinned";
  if (pane.role === "worker") return "worker";
  if (pane.agent !== "claude") return "notClaude";
  if (orchestratorOf(ws)) return "taken";
  return null;
}

/** Whether the workspace can have an orchestrator created in it. */
export const canCreateOrchestrator = (ws: Pick<Workspace, "panes">) => !orchestratorOf(ws);

export interface OrchestrationActions {
  /** Pins the pane; false when `pinRefusal` has a reason not to. */
  pin: (workspaceId: string, paneId: string) => boolean;
  /** The pane goes back to being a pane like any other. */
  unpin: (workspaceId: string, paneId: string) => void;
  /**
   * A new Claude Code pane beside `near`, pinned from the start and on the
   * orchestrator's model. Its id, or null when the workspace already has an
   * orchestrator or no room for a pane.
   */
  create: (workspaceId: string, near: string | null) => string | null;
}

export function useOrchestrationActions(): OrchestrationActions {
  const { snapshot, addPane, updatePane } = useStore();
  return useMemo(() => {
    const workspace = (id: string) => snapshot().workspaces.find((ws) => ws.id === id) ?? null;
    return {
      pin(workspaceId, paneId) {
        const ws = workspace(workspaceId);
        if (!ws || pinRefusal(ws, paneId)) return false;
        updatePane(workspaceId, paneId, { role: "orchestrator" });
        // orchestration: announce role (agent D)
        // The pane may well be in the middle of a session, whose system
        // prompt was settled when it started: it is told of its new role by
        // a one-time message from Hangar, queued here.
        return true;
      },

      unpin(workspaceId, paneId) {
        const ws = workspace(workspaceId);
        if (ws?.panes.find((pane) => pane.id === paneId)?.role !== "orchestrator") return;
        updatePane(workspaceId, paneId, { role: undefined });
        // orchestration: announce role (agent D)
        // Its system prompt keeps the role until the next compaction: the
        // session is told once that it no longer orchestrates.
      },

      create(workspaceId, near) {
        const ws = workspace(workspaceId);
        if (!ws || !canCreateOrchestrator(ws)) return null;
        const id = addPane(workspaceId, { agent: "claude", near, dir: preferredDir(near) });
        if (!id) return null;
        // Same tick as the add: the pane mounts, and launches, as the
        // orchestrator it was created to be.
        updatePane(workspaceId, id, {
          role: "orchestrator",
          model: orchestrationOf(ws).orchestratorModel,
        });
        return id;
      },
    };
  }, [snapshot, addPane, updatePane]);
}
