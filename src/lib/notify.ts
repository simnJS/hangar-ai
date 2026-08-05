import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/**
 * Desktop notifications, resolved once and never retried.
 *
 * Outside the Tauri shell — `pnpm dev` in a plain browser — the plugin call
 * throws rather than returning a verdict, so every entry point swallows its
 * error and the app simply goes quiet instead of breaking a pane.
 */
let permission: Promise<boolean> | null = null;

function ensurePermission(): Promise<boolean> {
  if (!permission) {
    permission = (async () => {
      try {
        if (await isPermissionGranted()) return true;
        return (await requestPermission()) === "granted";
      } catch {
        return false;
      }
    })();
  }
  return permission;
}

export async function notify(title: string, body: string): Promise<void> {
  if (!(await ensurePermission())) return;
  try {
    sendNotification({ title, body });
  } catch {
    /* notification backend unavailable; the pane badge still shows it */
  }
}
