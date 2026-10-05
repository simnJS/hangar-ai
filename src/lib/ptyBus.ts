import { listen } from "@tauri-apps/api/event";

/** `seq` is how many bytes the pane has printed once this chunk is in. */
type Handler = (data: string, seq: number) => void;

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
      await listen<{ id: string; data: string; seq: number }>("pty:output", (event) => {
        outputHandlers.get(event.payload.id)?.(event.payload.data, event.payload.seq);
      });
      await listen<{ id: string }>("pty:exit", (event) => {
        exitHandlers.get(event.payload.id)?.();
      });
    })().catch((err) => {
      // A cached rejection would make every pane opened afterwards fail on a
      // problem that was over long ago. Forget it so the next one retries.
      ready = null;
      throw err;
    });
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
