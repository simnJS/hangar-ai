import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import type { BridgeEvent } from "../../lib/bridge";
import { launchedAccount } from "../../lib/accounts";
import { mergeUsage, usageByAccount, type AccountUsage } from "../../lib/accountSwitch";

/**
 * The last usage each account reported, from the hangar-bridge's `usage`
 * events: a session counts for the account its pane was launched on. Kept in
 * memory and in localStorage, so a restart still has the last known figures
 * (the interface shows their age through `resetsAt`, and « unknown » for a
 * window never reported).
 */
const KEY = "hangar.accountUsage";

type Usage = Record<string, AccountUsage>;

function load(): Usage {
  try {
    const raw = localStorage.getItem(KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : {};
    return parsed && typeof parsed === "object" ? (parsed as Usage) : {};
  } catch {
    return {};
  }
}

let usage: Usage = load();
const listeners = new Set<() => void>();
let started = false;

function start() {
  if (started) return;
  started = true;
  listen<{ paneId: string; event: BridgeEvent }>("bridge:event", ({ payload }) => {
    if (payload.event.kind !== "usage" || payload.event.rateLimits.length === 0) return;
    const account = launchedAccount(payload.paneId);
    if (!account) return;
    const fresh = usageByAccount(
      [{ paneId: payload.paneId, at: Date.now(), rateLimits: payload.event.rateLimits }],
      () => account,
    );
    usage = mergeUsage(usage, fresh);
    try {
      localStorage.setItem(KEY, JSON.stringify(usage));
    } catch {
      // Private window or blocked storage: the figures just last until closed.
    }
    for (const listener of listeners) listener();
  }).catch(() => undefined);
}

function subscribe(listener: () => void) {
  start();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useAccountUsage(): Usage {
  return useSyncExternalStore(subscribe, () => usage);
}
