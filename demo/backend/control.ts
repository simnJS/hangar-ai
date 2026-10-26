/**
 * The MCP control channel, faked.
 *
 * A demo agent "calling" a Hangar tool sends the `control:request` the Rust
 * side would send, so the app carries it out through lib/control exactly as
 * it does for a real agent (the store, the 3D preview, the announcement on
 * screen) and answers through `control_reply`, which lands in `reply` below.
 */
import { dispatch } from "../mock/event";

export interface ControlAnswer {
  ok: boolean;
  value: unknown;
  error: string | null;
}

/** The app answers a model_show once the model has loaded, 45 s at most. */
const ANSWER_MS = 60_000;

const pending = new Map<string, (answer: ControlAnswer) => void>();
let asked = 0;

export function ask(op: string, args: Record<string, unknown>): Promise<ControlAnswer> {
  const id = `demo-control-${++asked}`;
  return new Promise((resolve) => {
    // Nobody listening yet (the app is still loading) must not leave the
    // transcript waiting for ever.
    const timer = setTimeout(() => {
      pending.delete(id);
      resolve({ ok: false, value: null, error: "Hangar did not answer" });
    }, ANSWER_MS);
    pending.set(id, (answer) => {
      clearTimeout(timer);
      resolve(answer);
    });
    dispatch("control:request", { id, op, args });
  });
}

export function reply(id: string, ok: boolean, value: unknown, error: string | null) {
  const resolve = pending.get(id);
  if (!resolve) return;
  pending.delete(id);
  resolve({ ok, value, error });
}
