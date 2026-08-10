import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

/**
 * The browser build of the app, for the demo embedded in the site.
 *
 * `src/` is compiled unmodified. Every `@tauri-apps/*` import is redirected to
 * a stand-in under `demo/mock`, which talks to the fake backend in
 * `demo/backend`: an in-memory board and a pseudo-terminal that pretends to
 * be a shell. No Rust, no PTY, no model.
 */
const mock = (file: string) => fileURLToPath(new URL(`./demo/mock/${file}`, import.meta.url));

export default defineConfig({
  root: fileURLToPath(new URL("./demo", import.meta.url)),
  // Relative, so the build works from site/demo/ whatever the host is.
  base: "./",
  plugins: [react()],
  resolve: {
    alias: [
      { find: "@tauri-apps/api/core", replacement: mock("core.ts") },
      { find: "@tauri-apps/api/event", replacement: mock("event.ts") },
      { find: "@tauri-apps/api/window", replacement: mock("window.ts") },
      { find: "@tauri-apps/api/app", replacement: mock("app.ts") },
      { find: "@tauri-apps/plugin-dialog", replacement: mock("dialog.ts") },
      { find: "@tauri-apps/plugin-process", replacement: mock("process.ts") },
      { find: "@tauri-apps/plugin-updater", replacement: mock("updater.ts") },
      { find: "@tauri-apps/plugin-notification", replacement: mock("notification.ts") },
    ],
  },
  build: {
    outDir: fileURLToPath(new URL("./site/demo", import.meta.url)),
    emptyOutDir: true,
    // One request for the app, one for its stylesheet: the demo loads inside
    // an iframe that is already below the fold.
    chunkSizeWarningLimit: 1200,
  },
  server: {
    port: 4300,
    fs: {
      // src/ lives outside the demo root.
      allow: [fileURLToPath(new URL("./", import.meta.url))],
    },
  },
});
