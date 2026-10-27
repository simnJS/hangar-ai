import { useSyncExternalStore } from "react";

/**
 * The deliveries being landed right now, by workspace and task: checked,
 * reviewed and merged one at a time (lib/deliveryControl), which takes a
 * while, and must not be cut short by the workspace going to sleep
 * (lib/hibernate) or answered twice by a second « Fusionner ».
 *
 * Whoever runs one says so for as long as it runs: « Fusionner » in the
 * « Besoin de toi » list, and a worker's own `delivery_submit`.
 */

const running = new Map<string, Set<string>>();
const listeners = new Set<() => void>();

function changed() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Marks a delivery as running until the returned call; calling it twice is harmless. */
export function beginDelivery(workspaceId: string, taskId: string): () => void {
  const tasks = running.get(workspaceId) ?? new Set<string>();
  tasks.add(taskId);
  running.set(workspaceId, tasks);
  changed();
  let ended = false;
  return () => {
    if (ended) return;
    ended = true;
    const held = running.get(workspaceId);
    if (!held?.delete(taskId)) return;
    if (!held.size) running.delete(workspaceId);
    changed();
  };
}

/** Whether a delivery of this workspace is running. */
export const isDelivering = (workspaceId: string) => Boolean(running.get(workspaceId)?.size);

/** Whether this task's delivery is running, kept current. */
export function useDelivering(workspaceId: string, taskId: string): boolean {
  return useSyncExternalStore(subscribe, () => Boolean(running.get(workspaceId)?.has(taskId)));
}
