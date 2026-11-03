import { defineConfig } from "vitest/config";
import { fileURLToPath, URL } from "node:url";

/**
 * The phone app's unit tests (`pnpm test`): its pure logic and the modules it
 * shares with the desktop's statistics, under Node rather than a DOM. Nothing
 * tested here renders anything, and the modules that reach for `window` get a
 * stub in their own tests.
 */
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "lib/**/*.test.ts", "remote/**/*.test.ts"],
  },
});
