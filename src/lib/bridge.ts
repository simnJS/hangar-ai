import { listen } from "@tauri-apps/api/event";
import type { QueuedPrompt } from "./ipc";

/**
 * What the hangar-bridge mod reports from inside a pane's Claude Code (see
 * src-tauri/src/bridge.rs and src-tauri/claude-mod/hangar-bridge).
 */
export interface RateLimit {
  /** `five_hour`, `seven_day`, or a gateway's own window. */
  kind: string;
  percentUsed: number;
  resetsAt: string | null;
}

export type BridgeEvent =
  | { kind: "hello"; sessionId: string; model: string; version: string }
  | { kind: "session"; sessionId: string; source: string }
  | { kind: "turn"; phase: "start" }
  | {
      kind: "turn";
      phase: "end";
      reason: "answer" | "aborted" | "refusal" | "error";
      durationMs: number;
      /** First line of the final answer, possibly empty. */
      answer: string;
    }
  | {
      kind: "permission";
      waiting: boolean;
      tool?: string;
      /** What the call would do, as the dialog opens: its input, long texts cut. */
      input?: unknown;
    }
  /**
   * A call of the main loop, as it starts — its input, long texts cut — and
   * as it ends, however it ends. The transcript has a call only once it has
   * its result; an AskUserQuestion's input is the questions the person has.
   */
  | { kind: "tool"; phase: "start"; toolUseId: string; tool: string; input: Record<string, unknown> }
  | { kind: "tool"; phase: "end"; toolUseId: string }
  | {
      kind: "usage";
      context: { tokens: number | null; window: number; percent: number | null };
      rateLimits: RateLimit[];
      costUsd: number | null;
    }
  | {
      kind: "edit";
      file: string;
      /** Another pane edited this file lately: its name… */
      overlap?: string;
      /** …and its id, so that pane can show it. */
      overlapPane?: string;
    }
  /** A queued prompt the session refused (a hook dropped it, or it failed). */
  | { kind: "dropped"; text: string; reason: string }
  | { kind: "end"; reason: string };

type EventHandler = (event: BridgeEvent) => void;
type QueueHandler = (pending: QueuedPrompt[]) => void;

const eventHandlers = new Map<string, EventHandler>();
const queueHandlers = new Map<string, QueueHandler>();

let ready: Promise<void> | null = null;

/** One pair of window listeners for every pane, as the PTY bus does. */
function ensureBus(): Promise<void> {
  if (!ready) {
    ready = (async () => {
      await listen<{ paneId: string; event: BridgeEvent }>("bridge:event", (event) => {
        eventHandlers.get(event.payload.paneId)?.(event.payload.event);
      });
      await listen<{ paneId: string; pending: QueuedPrompt[] }>("bridge:queue", (event) => {
        queueHandlers.get(event.payload.paneId)?.(event.payload.pending);
      });
    })().catch((err) => {
      ready = null;
      throw err;
    });
  }
  return ready;
}

/**
 * Resolves with the unsubscribe function. Outside the desktop shell there is
 * no event bus, and the pane simply never hears from a mod.
 */
export async function subscribeBridge(
  paneId: string,
  onEvent: EventHandler,
  onQueue: QueueHandler,
): Promise<() => void> {
  try {
    await ensureBus();
  } catch {
    return () => undefined;
  }
  eventHandlers.set(paneId, onEvent);
  queueHandlers.set(paneId, onQueue);
  return () => {
    eventHandlers.delete(paneId);
    queueHandlers.delete(paneId);
  };
}
