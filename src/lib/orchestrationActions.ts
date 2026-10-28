import { useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { preferredDir } from "./layout";
import { hqOf, managerOf, orchestrationOf, orchestratorOf } from "./orchestration";
import { announceRole } from "./orchestratorBrief";
import { useStore } from "../store";
import type { Workspace } from "../types";
import type { StoreValue } from "../store";

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
  | "taken"
  /** It is the project manager: its role is its own, never an orchestrator's. */
  | "manager"
  /** The workspace is the HQ, where the manager lives and no project is planned. */
  | "hq";

export function pinRefusal(
  ws: Pick<Workspace, "panes"> & { hq?: boolean },
  paneId: string,
): PinRefusal | null {
  const pane = ws.panes.find((entry) => entry.id === paneId);
  if (!pane) return "missing";
  if (pane.role === "manager") return "manager";
  if (ws.hq) return "hq";
  if (pane.role === "orchestrator") return "pinned";
  if (pane.role === "worker") return "worker";
  if (pane.agent !== "claude") return "notClaude";
  if (orchestratorOf(ws)) return "taken";
  return null;
}

/** Whether the workspace can have an orchestrator created in it. */
export const canCreateOrchestrator = (ws: Pick<Workspace, "panes"> & { hq?: boolean }) =>
  !ws.hq && !orchestratorOf(ws);

/** What `openManager` did, for the notice that follows. */
export type OpenManager =
  /** The HQ and its manager did not exist: both are there now. */
  | { kind: "created"; workspaceId: string; paneId: string }
  /** The HQ was there without its manager (closed): a new one is in it. */
  | { kind: "restored"; workspaceId: string; paneId: string }
  /** The manager was there already: shown. */
  | { kind: "shown"; workspaceId: string; paneId: string }
  /** The HQ has no room for a pane, or its folder could not be made. */
  | { kind: "failed"; error?: string };

/** What `openManager` reads and does: the store's, and the HQ's folder. */
export interface ManagerDeps
  extends Pick<
    StoreValue,
    "snapshot" | "addWorkspace" | "updateWorkspace" | "setActiveWorkspace" | "addPane" | "updatePane"
  > {
  /** `<app data dir>/hq`, created if missing. */
  hqDir: () => Promise<string>;
  /** The HQ's name, in the language of the window. */
  hqName: string;
}

/** The HQ's folder, from Rust. */
export const hqDir = () => invoke<string>("hq_dir");

let opening: Promise<OpenManager> | null = null;

/**
 * « Ouvrir le chef de projet »: the one way to the manager. Hangar has one
 * manager, in one HQ workspace: with neither, both are made — the pane a
 * Claude Code with the role from the start, on the orchestrator's model, so
 * it launches with Hangar's server and its role — with the HQ only, a new
 * manager is added; with both, they are shown. Never a second manager.
 * Calls made while one is under way share its answer.
 */
export function openManager(deps: ManagerDeps): Promise<OpenManager> {
  opening ??= run(deps).finally(() => {
    opening = null;
  });
  return opening;
}

async function run(deps: ManagerDeps): Promise<OpenManager> {
  const { snapshot, setActiveWorkspace } = deps;
  const existing = managerOf(snapshot());
  if (existing) {
    setActiveWorkspace(existing.ws.id);
    return { kind: "shown", workspaceId: existing.ws.id, paneId: existing.pane.id };
  }

  let hq = hqOf(snapshot());
  let created = false;
  if (!hq) {
    let cwd: string;
    try {
      cwd = await deps.hqDir();
    } catch (err) {
      return { kind: "failed", error: String(err) };
    }
    // Another call may have made it while the folder was.
    hq = hqOf(snapshot());
    if (!hq) {
      const id = deps.addWorkspace({
        name: deps.hqName,
        cwd,
        layout: 1,
        agents: ["claude"],
        shellId: null,
      });
      deps.updateWorkspace(id, { hq: true });
      hq = snapshot().workspaces.find((ws) => ws.id === id) ?? null;
      created = true;
    }
  }
  if (!hq) return { kind: "failed" };

  // The first pane of a new HQ is the Claude Code it was made with; an HQ
  // whose manager was closed gets one again.
  const paneId = created
    ? (hq.panes[0]?.id ?? null)
    : deps.addPane(hq.id, { agent: "claude", near: null, dir: "row" });
  if (!paneId) return { kind: "failed" };
  // Same tick as the add: the pane mounts, and launches, as the manager.
  deps.updatePane(hq.id, paneId, {
    role: "manager",
    model: orchestrationOf(hq).orchestratorModel,
  });
  setActiveWorkspace(hq.id);
  return { kind: created ? "created" : "restored", workspaceId: hq.id, paneId };
}

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
  /** « Ouvrir le chef de projet »: see `openManager`. */
  openManager: (hqName: string) => Promise<OpenManager>;
}

export function useOrchestrationActions(): OrchestrationActions {
  const store = useStore();
  const { snapshot, addPane, updatePane } = store;
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
        // a one-time message from Hangar, queued here — once the store holds
        // the role the briefing is written from.
        window.setTimeout(() => void announceRole(workspaceId, paneId).catch(() => undefined), 0);
        return true;
      },

      unpin(workspaceId, paneId) {
        const ws = workspace(workspaceId);
        if (ws?.panes.find((pane) => pane.id === paneId)?.role !== "orchestrator") return;
        updatePane(workspaceId, paneId, { role: undefined });
        // orchestration: announce role (agent D)
        // Its system prompt keeps the role until the next compaction: the
        // session is told once that it no longer orchestrates.
        window.setTimeout(
          () => void announceRole(workspaceId, paneId, "orchestrator").catch(() => undefined),
          0,
        );
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

      openManager: (hqName) => openManager({ ...store, hqDir, hqName }),
    };
  }, [snapshot, addPane, updatePane, store.addWorkspace, store.updateWorkspace, store.setActiveWorkspace]);
}
