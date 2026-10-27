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
