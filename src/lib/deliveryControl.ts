import type { Caller, ControlDeps } from "./control";
import type { Workspace } from "../types";

/**
 * The delivery_* control operations: a worker handing in its branch, which
 * lands on its workspace's branch once checked and reviewed (delivery.rs).
 */
export async function deliveryControl(
  op: string,
  _ws: Workspace,
  _args: Record<string, unknown>,
  _caller: Caller,
  _deps: ControlDeps,
): Promise<unknown> {
  throw new Error(`Unknown operation "${op}".`);
}

/**
 * Lands a delivery that passed its check and its review but waits for the
 * person, as it does with automatic landing off.
 */
export async function landDelivery(_ws: Workspace, _taskId: string, _deps: ControlDeps): Promise<void> {
  throw new Error("Nothing to land yet.");
}
