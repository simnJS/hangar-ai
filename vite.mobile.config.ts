import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";

/**
 * Hangar Mobile, the phone app: its own entry (mobile.html, src/mobile/),
 * built into dist-mobile/, which the desktop binary embeds and serves at / to
 * the phone through Tailscale Serve.
 *
 * `pnpm mobile:dev` runs it against the fake PC of src/mobile/mock (the dev
 * server always does); a build talks to the real one, or to the fake one with
 * `?mock` in the address.
 */
const here = (path: string) => fileURLToPath(new URL(path, import.meta.url));
const publicDir = here("./public/mobile");

/**
 * The page is written as mobile.html, beside the desktop's index.html, and
 * served as the site's index: the dev server answers / with it, and the build
 * writes it as index.html.
 *
 * The service worker (src/mobile/sw.js, a file of its own: push handlers join
 * it later) is written at the root with the list of files that make the app
 * shell and a version drawn from them, so a new build replaces the cache.
 */
function mobileShell(): Plugin {
  return {
    name: "hangar-mobile-shell",
    // After Vite has written the page into the bundle.
    enforce: "post",
    configureServer(server) {
      server.middlewares.use((req, _res, next) => {
        if (req.url === "/" || req.url?.startsWith("/?")) req.url = `/mobile.html${req.url.slice(1)}`;
        next();
      });
    },
    generateBundle(_options, bundle) {
      const page = bundle["mobile.html"];
      if (page?.type === "asset") {
        delete bundle["mobile.html"];
        this.emitFile({ type: "asset", fileName: "index.html", source: page.source });
      }
      // Fonts in other scripts than Latin load only when a page needs them:
      // they stay out of the shell and are cached the first time they are.
      const built = Object.keys(bundle).filter(
        (file) => !file.endsWith(".map") && (!file.endsWith(".woff2") || /inter-latin-opsz-normal/.test(file)),
      );
      const statics = readdirSync(publicDir);
      const shell = ["./", ...[...built, ...statics].sort().map((file) => `./${file}`)];
      const version = createHash("sha256")
        .update(shell.join("\n"))
        .update(statics.map((file) => readFileSync(`${publicDir}/${file}`)).join(""))
        .update(readFileSync(here("./src/mobile/sw.js")))
        .digest("hex")
        .slice(0, 12);
      const source = readFileSync(here("./src/mobile/sw.js"), "utf8")
        .replace("/*SHELL*/[]", JSON.stringify(shell))
        .replace('/*VERSION*/"dev"', JSON.stringify(version));
      this.emitFile({ type: "asset", fileName: "sw.js", source });
    },
  };
}

export default defineConfig({
  // Relative, so the build works wherever it is served from.
  base: "./",
  publicDir,
  plugins: [react(), mobileShell()],
  build: {
    outDir: here("./dist-mobile"),
    emptyOutDir: true,
    rollupOptions: { input: here("./mobile.html") },
  },
  // `pnpm mobile:dev --host` to try it from a phone on the same network.
  server: { port: 4310 },
});
