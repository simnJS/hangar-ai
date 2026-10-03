import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    // The suite covers pure logic only: no database, no network, no Clerk.
    // Anything that needs those lives in a route handler, which stays a thin
    // shell around the functions tested here.
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
