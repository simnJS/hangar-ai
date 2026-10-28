/**
 * What the phone keeps between launches, in localStorage. It can be missing
 * or refuse writes (private browsing, cleared data): every read falls back to
 * nothing, and the app works without it.
 *
 * The fake PC keeps its own, so trying it never leaves fake projects behind
 * for the real app to show while the PC is away.
 */

let prefix = "hangar.mobile.";

export function keepMockApart(): void {
  prefix = "hangar.mobile.mock.";
}

export function load(key: string): string | null {
  try {
    return localStorage.getItem(prefix + key);
  } catch {
    return null;
  }
}

export function save(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(prefix + key);
    else localStorage.setItem(prefix + key, value);
  } catch {
    // Nowhere to keep it: the next launch starts without it.
  }
}

export function loadNumber(key: string): number | null {
  const raw = load(key);
  const value = raw === null ? NaN : Number(raw);
  return Number.isFinite(value) ? value : null;
}

export function loadJson<T>(key: string): T | null {
  try {
    return JSON.parse(load(key) ?? "null") as T | null;
  } catch {
    return null;
  }
}
