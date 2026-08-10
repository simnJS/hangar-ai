/** Stands in for `@tauri-apps/api/window`. The demo runs in an iframe, so
    fullscreen is the browser's business and everything here is a no-op. */

let fullscreen = false;

export function getCurrentWindow() {
  return {
    label: "demo",
    isFullscreen: async () => fullscreen,
    setFullscreen: async (on: boolean) => {
      fullscreen = on;
    },
    setTitle: async () => undefined,
    minimize: async () => undefined,
    maximize: async () => undefined,
    close: async () => undefined,
    isMaximized: async () => false,
  };
}

export const getCurrent = getCurrentWindow;
