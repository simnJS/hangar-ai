import { fileURLToPath } from "node:url";
import type { NextConfig } from "next";

const config: NextConfig = {
  // Nothing exotic on purpose. The API is a set of Node route handlers; the
  // only rendered page is static and must build with no environment at all,
  // so there is no config here that could reach for a secret at build time.
  reactStrictMode: true,
  turbopack: {
    // This app lives inside the Hangar repository but is its own Vercel
    // project. Without this, Turbopack walks up, finds the desktop app's
    // lockfile and takes the repository root as the workspace — which drags
    // unrelated files into the trace and makes the build depend on a sibling
    // project's dependencies.
    root: fileURLToPath(new URL(".", import.meta.url)),
  },
};

export default config;
