import { defineConfig } from "vitest/config";

/**
 * Unit tests for the frontend's pure logic.
 *
 * Apart from vite.config.ts, whose dev server is tuned for Tauri, and under
 * Node rather than a DOM: nothing tested here renders anything, and the one
 * module that reaches for `window` gets a stub in its own test.
 */
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
