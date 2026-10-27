import type { OrchestrationSettings, Pane, Workspace } from "../types";

/**
 * A workspace's orchestration: one Claude Code pane pinned as its
 * orchestrator, which plans on the board and hands each task to a worker pane
 * of its own, in a worktree, then lands what the workers deliver.
 *
 * The limits here are Hangar's to hold, not the orchestrator's: a prompt can
 * ask a model to start no more than three workers, only code can refuse the
 * fourth.
 */

export const ORCHESTRATION_DEFAULTS: OrchestrationSettings = {
  maxWorkers: 3,
  maxAttempts: 2,
  autoMerge: true,
  checkCommand: null,
  orchestratorModel: "opus",
  workerModel: "sonnet",
  reviewerModel: "sonnet",
  branch: null,
  permissionMode: null,
};

/** The permission modes offered for an orchestration's agents (null: theirs). */
export const ORCHESTRATION_PERMISSION_MODES = ["auto", "acceptEdits", "bypassPermissions"] as const;

/**
 * What an orchestration pane may use without asking, whatever its mode: the
 * tools of Hangar's own server — the board, workers, deliveries. Each of them
 * checks who calls it, so this lets nothing else through.
 */
export const ORCHESTRATION_ALLOWED_TOOLS = ["mcp__hangar"];

/** A workspace's settings, each one absent or malformed read as its default. */
export function orchestrationOf(ws: Pick<Workspace, "orchestration">): OrchestrationSettings {
  const saved = ws.orchestration;
  if (!saved || typeof saved !== "object") return ORCHESTRATION_DEFAULTS;
  const count = (value: unknown, fallback: number) =>
    typeof value === "number" && Number.isInteger(value) && value >= 1 ? value : fallback;
  const name = (value: unknown, fallback: string) =>
    typeof value === "string" && value.trim() ? value.trim() : fallback;
  const optional = (value: unknown) =>
    typeof value === "string" && value.trim() ? value.trim() : null;
  return {
    maxWorkers: count(saved.maxWorkers, ORCHESTRATION_DEFAULTS.maxWorkers),
    maxAttempts: count(saved.maxAttempts, ORCHESTRATION_DEFAULTS.maxAttempts),
    autoMerge: typeof saved.autoMerge === "boolean" ? saved.autoMerge : ORCHESTRATION_DEFAULTS.autoMerge,
    checkCommand: optional(saved.checkCommand),
    orchestratorModel: name(saved.orchestratorModel, ORCHESTRATION_DEFAULTS.orchestratorModel),
    workerModel: name(saved.workerModel, ORCHESTRATION_DEFAULTS.workerModel),
    reviewerModel: name(saved.reviewerModel, ORCHESTRATION_DEFAULTS.reviewerModel),
    branch: optional(saved.branch),
    permissionMode: (ORCHESTRATION_PERMISSION_MODES as readonly string[]).includes(
      saved.permissionMode as string,
    )
      ? (saved.permissionMode as string)
      : null,
  };
}

/** The workspace's orchestrator pane, if it has one. */
export const orchestratorOf = (ws: Pick<Workspace, "panes">): Pane | null =>
  ws.panes.find((pane) => pane.role === "orchestrator") ?? null;

/** The workspace's worker panes. */
export const workersOf = (ws: Pick<Workspace, "panes">): Pane[] =>
  ws.panes.filter((pane) => pane.role === "worker");
