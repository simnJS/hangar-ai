import { invoke } from "@tauri-apps/api/core";
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
 *
 * A notification that names a pane goes out through the Rust side, the only
 * half that can make the toast clickable. Platforms it has no backend for
 * answer with an error, and the plugin takes the notification as before —
 * minus the click, which is worth less than losing the notification.
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

/** The pane a click on the toast has to bring back to the front. */
export interface NotifyTarget {
  workspaceId: string;
  paneId: string;
}

export async function notify(
  title: string,
  body: string,
  target?: NotifyTarget,
): Promise<void> {
  if (!(await ensurePermission())) return;
  if (target) {
    try {
      await invoke("notify_send", {
        title,
        body,
        workspaceId: target.workspaceId,
        paneId: target.paneId,
      });
      return;
    } catch {
      /* no clickable backend here — fall through to the plain notification */
    }
  }
  try {
    sendNotification({ title, body });
  } catch {
    /* notification backend unavailable; the pane badge still shows it */
  }
}
