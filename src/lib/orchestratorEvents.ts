/**
 * What happens in a workspace that its orchestrator should hear about, posted
 * by the modules that see it happen (workers, deliveries, the board) and
 * handed to the orchestrator pane in batches, as messages from Hangar rather
 * than from the person (lib/orchestration).
 */
export type OrchestratorEvent =
  | { kind: "workerStarted" | "workerStopped" | "workerExited"; worker: string; taskId: string | null }
  | { kind: "workerWaiting"; worker: string; taskId: string | null; forMs: number }
  | { kind: "workerIdle"; worker: string; taskId: string | null; forMs: number }
  | { kind: "taskMoved"; taskId: string; title: string; from: string; to: string; by: string | null }
  | { kind: "taskCommented"; taskId: string; title: string; by: string; text: string }
  | { kind: "taskNeedsYou"; taskId: string; title: string; reason: string }
  | {
      kind: "delivery";
      taskId: string;
      title: string;
      worker: string;
      outcome: "landed" | "ready" | "conflict" | "checkFailed" | "changesRequested" | "dirty" | "error";
      detail: string;
    };

/** Hands `event` to the workspace's orchestrator, if it has one. */
export function postOrchestratorEvent(_workspaceId: string, _event: OrchestratorEvent): void {}
