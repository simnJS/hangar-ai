/**
 * Stands in for `@tauri-apps/api/core`. Every command the app sends to Rust
 * lands in the demo backend instead. See demo/backend.
 */
import { route } from "../backend";

export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return route(command, args ?? {}) as Promise<T>;
}

export const convertFileSrc = (path: string) => path;
export const isTauri = false;
