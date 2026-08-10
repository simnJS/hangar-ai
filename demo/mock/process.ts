/** Stands in for `@tauri-apps/plugin-process`. Reloading the frame is the
    closest honest equivalent to relaunching the app. */
export async function relaunch(): Promise<void> {
  window.location.reload();
}

export async function exit(): Promise<void> {
  /* nothing to exit in a browser tab */
}
