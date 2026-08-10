/**
 * Stands in for `@tauri-apps/api/event` in the browser demo.
 *
 * The app only ever listens, to `pty:output`, `pty:exit` and `board:changed`,
 * so this is a plain in-process emitter. `dispatch` is the door the fake
 * backend pushes through; the app never sees it.
 */

export interface Event<T> {
  event: string;
  id: number;
  payload: T;
}

export type EventCallback<T> = (event: Event<T>) => void;
export type UnlistenFn = () => void;

const subscribers = new Map<string, Set<EventCallback<any>>>();
let nextId = 1;

export async function listen<T>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  let set = subscribers.get(event);
  if (!set) {
    set = new Set();
    subscribers.set(event, set);
  }
  set.add(handler);
  return () => {
    set!.delete(handler);
  };
}

export async function once<T>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  let off: UnlistenFn = () => undefined;
  off = await listen<T>(event, (payload) => {
    off();
    handler(payload);
  });
  return off;
}

export async function emit<T>(event: string, payload?: T): Promise<void> {
  dispatch(event, payload as T);
}

export async function emitTo<T>(_target: unknown, event: string, payload?: T): Promise<void> {
  dispatch(event, payload as T);
}

/** Backend → app. Copied before iterating so a handler may unsubscribe itself. */
export function dispatch<T>(event: string, payload: T): void {
  const set = subscribers.get(event);
  if (!set || !set.size) return;
  const id = nextId++;
  for (const handler of Array.from(set)) handler({ event, id, payload });
}
