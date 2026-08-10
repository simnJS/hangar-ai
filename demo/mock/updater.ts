/** Stands in for `@tauri-apps/plugin-updater`. The demo is always up to date,
    so the banner never appears. */

export interface Update {
  version: string;
  date?: string;
  body?: string;
  downloadAndInstall: (onEvent?: (e: unknown) => void) => Promise<void>;
}

export async function check(): Promise<Update | null> {
  return null;
}
