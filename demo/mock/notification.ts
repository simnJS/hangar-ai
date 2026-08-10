/** Stands in for `@tauri-apps/plugin-notification`. The demo stays silent
    rather than asking a visitor for the notification permission. */

export async function isPermissionGranted(): Promise<boolean> {
  return false;
}

export async function requestPermission(): Promise<"granted" | "denied"> {
  return "denied";
}

export function sendNotification(_options: { title: string; body?: string }): void {
  /* deliberately nothing */
}
