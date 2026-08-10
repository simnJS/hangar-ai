/**
 * Folds the demo build into one self-contained site/demo/index.html.
 *
 * Why: the demo is loaded in an iframe, and relative asset paths depend on how
 * the host spells the URL. `serve` alone rewrites /demo/index.html → /demo/index
 * → /demo, after which `./assets/x.js` resolves against the root and 404s. Other
 * hosts have their own opinions about trailing slashes and clean URLs.
 *
 * A document with no sub-resources has nothing to resolve, so it loads the same
 * way at /demo, /demo/ or /demo/index.html, on any host and under any base path.
 * The whole thing gzips to about the same size the separate bundle did.
 *
 * Run by `pnpm demo:build`, after vite.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const outDir = path.join(root, "site", "demo");
const assetDir = path.join(outDir, "assets");

if (!fs.existsSync(assetDir)) {
  console.error("no site/demo/assets, run vite build first");
  process.exit(1);
}

const assets = fs.readdirSync(assetDir);
const cssFile = assets.find((f) => f.endsWith(".css"));
const jsFile = assets.find((f) => f.endsWith(".js"));

if (!jsFile) {
  console.error("no bundle in site/demo/assets");
  process.exit(1);
}

let css = cssFile ? fs.readFileSync(path.join(assetDir, cssFile), "utf8") : "";
let js = fs.readFileSync(path.join(assetDir, jsFile), "utf8");

const MEDIA = {
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".webp": "image/webp",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

const dataUri = (file) => {
  const type = MEDIA[path.extname(file)];
  if (!type) return null;
  return `data:${type};base64,` + fs.readFileSync(path.join(assetDir, file)).toString("base64");
};

// Plain references, as they appear in CSS url() and in string literals.
for (const file of assets) {
  const uri = dataUri(file);
  if (!uri) continue;
  for (const ref of [`./assets/${file}`, `assets/${file}`, `/assets/${file}`]) {
    if (js.includes(ref)) js = js.split(ref).join(uri);
    if (css.includes(ref)) css = css.split(ref).join(uri);
  }
}

// Vite emits imported assets as `new URL("file.png", import.meta.url)`. Inlined
// the module resolves that against the document, which is not where the file
// is, so the whole expression is replaced by the data URI.
js = js.replace(/new URL\("([^"]+)",\s*import\.meta\.url\)/g, (whole, file) => {
  if (!fs.existsSync(path.join(assetDir, file))) return whole;
  return JSON.stringify(dataUri(file) ?? whole);
});

const page = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Hangar.AI demo</title>
    <meta name="robots" content="noindex" />
    <style>
html, body, #root { height: 100%; margin: 0; }
body { background: #16161e; }
${css}</style>
  </head>
  <body>
    <div id="root"></div>
    <script type="module">
${js}</script>
  </body>
</html>
`;

fs.writeFileSync(path.join(outDir, "index.html"), page);
fs.rmSync(assetDir, { recursive: true, force: true });

const left = page.match(/["'(]\.{0,2}\/?assets\/[A-Za-z0-9._-]+/g);
if (left) console.warn("still referencing:", [...new Set(left)].join(", "));

console.log(`site/demo/index.html, ${(page.length / 1024).toFixed(0)} kB, no sub-resources`);
