import type { Caller, ControlDeps } from "./control";
import type { Workspace } from "../types";

/**
 * The worker_* control operations: an orchestrator starting, listing and
 * stopping the workers of its workspace (lib/orchestration).
 */
export async function workerControl(
  op: string,
  _ws: Workspace,
  _args: Record<string, unknown>,
  _caller: Caller,
  _deps: ControlDeps,
): Promise<unknown> {
  throw new Error(`Unknown operation "${op}".`);
}
