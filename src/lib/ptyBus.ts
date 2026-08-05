import { listen } from "@tauri-apps/api/event";

type Handler = (data: string) => void;

const outputHandlers = new Map<string, Handler>();
const exitHandlers = new Map<string, () => void>();

let ready: Promise<void> | null = null;

/**
 * A single pair of window-level listeners fans out to every pane, instead of
 * each terminal subscribing to the same global event stream.
 */
function ensureBus(): Promise<void> {
  if (!ready) {
    ready = (async () => {
      await listen<{ id: string; data: string }>("pty:output", (event) => {
        outputHandlers.get(event.payload.id)?.(event.payload.data);
      });
      await listen<{ id: string }>("pty:exit", (event) => {
        exitHandlers.get(event.payload.id)?.();
      });
    })();
  }
  return ready;
}

/** Must be awaited before spawning, so no early output is dropped. */
export async function subscribePty(
  id: string,
  onOutput: Handler,
  onExit: () => void,
): Promise<() => void> {
  await ensureBus();
  outputHandlers.set(id, onOutput);
  exitHandlers.set(id, onExit);
  return () => {
    outputHandlers.delete(id);
    exitHandlers.delete(id);
  };
}
