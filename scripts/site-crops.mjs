// Cuts the site's images out of full-window captures of the app, as site/img/crops.json
// says, and writes what the page needs to know about them back into site/index.html.
//
//   node scripts/site-crops.mjs --sources <folder of captures> [--only hero,hero-m] [--html-only]
//
// (HANGAR_CAPTURES can name the folder instead of --sources.)
//
// For each image, crops.json names a capture, a rectangle in it (in units of "scale" capture
// pixels), the widths to write and, optionally, a fade over an edge the crop cuts through.
// Every width is written as AVIF and WebP, as img/<name>-<width>.<ext>. The page then gets, on
// each <source> and <img> carrying data-crop="<name>", the srcset, the src and the width and
// height those files have, and on each <span data-of="<name>"> the position of the next outline
// listed under that image's "marks". Change the captures or the rectangles, run this again, and
// nothing else needs editing by hand.
//
// A rectangle is [x, y, width, height], or a reference into the rectangle index that ships with
// the captures (crops.json "rects"): { "ref": "path/to/rect" }, where a step through a list is
// a position or an entry's "name", optionally with "pad" (one number, or [top, right, bottom,
// left]) and with "x", "y", "w" or "h" to override one side of what the index says. { "union":
// [ref, ...] } spans several. A rectangle in the index can be {x, y, w, h}, {x, y, width,
// height}, {left, top, right, bottom} or [x, y, w, h].
//
// The pixels are Pillow's job (pip install pillow), run through Python so the repository
// needs no image library of its own. --html-only skips them and only rewrites the page.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = parseArgs(process.argv.slice(2));
const configPath = resolve(root, args.config || "site/img/crops.json");
const configDir = dirname(configPath);
const config = JSON.parse(readFileSync(configPath, "utf8"));

const outDir = resolve(configDir, config.out || ".");
const pagePath = resolve(configDir, config.page || "../index.html");
const href = config.href || "img/";
const defaultScale = config.scale || 1;
const defaultFormats = config.formats || { avif: { quality: 62 }, webp: { quality: 82 } };
const images = config.images || {};
const only = args.only ? new Set(args.only.split(",")) : null;
if (only) for (const name of only) if (!images[name]) fail(`crops.json has no image named "${name}"`);

for (const name of Object.keys(images)) {
  if (!/^[a-z0-9][a-z0-9-]*$/.test(name)) fail(`"${name}": image names are lowercase letters, digits and dashes`);
}

// ── rectangles ───────────────────────────────────────────────────────

const sourcesDir = args.sources || process.env.HANGAR_CAPTURES || (config.sources && resolve(configDir, config.sources));
let index = null;
function rectIndex() {
  if (index) return index;
  if (!config.rects) fail('a rectangle uses "ref", but crops.json names no "rects" index');
  if (!sourcesDir) fail("--sources is needed to read the rectangle index");
  index = JSON.parse(readFileSync(resolve(sourcesDir, config.rects), "utf8"));
  return index;
}

function asRect(value, where) {
  if (Array.isArray(value) && value.length === 4 && value.every((n) => typeof n === "number")) {
    const [x, y, w, h] = value;
    return { x, y, w, h };
  }
  if (value && typeof value === "object") {
    const { x, y, left, top, right, bottom } = value;
    const w = value.w ?? value.width;
    const h = value.h ?? value.height;
    if ([x, y, w, h].every((n) => typeof n === "number")) return { x, y, w, h };
    if ([left, top, right, bottom].every((n) => typeof n === "number")) {
      return { x: left, y: top, w: right - left, h: bottom - top };
    }
  }
  fail(`${where}: not a rectangle: ${JSON.stringify(value)}`);
}

// In a list, a key is a position or the "name" of an entry; a name may itself hold slashes
// ("stills/en/hero/rects/workers/Zoe/box").
function lookup(path, where) {
  let node = rectIndex();
  const keys = String(path).split("/");
  for (let i = 0; i < keys.length && node != null; i++) {
    if (!Array.isArray(node)) node = node[keys[i]];
    else if (/^\d+$/.test(keys[i])) node = node[Number(keys[i])];
    else {
      let found = null;
      for (let j = i; j < keys.length && !found; j++) {
        const name = keys.slice(i, j + 1).join("/");
        found = node.find((entry) => entry && entry.name === name);
        if (found) i = j;
      }
      node = found;
    }
  }
  if (node == null) fail(`${where}: "${path}" is not in ${config.rects}`);
  return asRect(node, `${where} (${path})`);
}

function resolveRect(spec, where) {
  if (Array.isArray(spec) || (spec && typeof spec === "object" && !spec.ref && !spec.union)) {
    return asRect(spec, where);
  }
  if (!spec || typeof spec !== "object") fail(`${where}: no rectangle`);
  let r;
  if (spec.union) {
    const parts = spec.union.map((part) =>
      typeof part === "string" ? lookup(part, where) : resolveRect(part, where),
    );
    const x0 = Math.min(...parts.map((p) => p.x));
    const y0 = Math.min(...parts.map((p) => p.y));
    const x1 = Math.max(...parts.map((p) => p.x + p.w));
    const y1 = Math.max(...parts.map((p) => p.y + p.h));
    r = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
  } else {
    r = lookup(spec.ref, where);
  }
  if (spec.pad != null) {
    const [t, rt, b, l] = typeof spec.pad === "number" ? [spec.pad, spec.pad, spec.pad, spec.pad] : spec.pad;
    r = { x: r.x - l, y: r.y - t, w: r.w + l + rt, h: r.h + t + b };
  }
  for (const k of ["x", "y", "w", "h"]) if (typeof spec[k] === "number") r[k] = spec[k];
  return r;
}

// Everything the page and the encoder need, per image.
const plans = {};
for (const [name, image] of Object.entries(images)) {
  const rect = resolveRect(image.rect, name);
  if (rect.w <= 0 || rect.h <= 0) fail(`${name}: empty rectangle`);
  const widths = [...new Set(image.widths || [])].sort((a, b) => a - b);
  if (!widths.length) fail(`${name}: no "widths"`);
  const formats = {};
  for (const [fmt, opts] of Object.entries(image.formats || defaultFormats)) {
    formats[fmt] = { ...(defaultFormats[fmt] || {}), ...opts };
  }
  const size = (w) => ({ w, h: Math.round((w * rect.h) / rect.w) });
  const marks = (image.marks || []).map((m, i) => {
    const r = resolveRect(m, `${name} mark ${i + 1}`);
    const pct = (v) => `${+(v * 100).toFixed(2)}%`;
    return `--x:${pct((r.x - rect.x) / rect.w)};--y:${pct((r.y - rect.y) / rect.h)};--w:${pct(r.w / rect.w)};--h:${pct(r.h / rect.h)}`;
  });
  plans[name] = { name, image, rect, widths, formats, size, marks, scale: image.scale || defaultScale };
}

// ── pixels ───────────────────────────────────────────────────────────

const PYTHON = String.raw`
import json, sys
from PIL import Image, ImageStat

def ramp(n, size, at_end):
    out = []
    for i in range(n):
        d = i - (n - size) + 1 if at_end else size - i
        t = min(1.0, max(0.0, d / size))
        out.append(round(255 * t * t * (3 - 2 * t)))
    return out

def fade(img, side, size):
    w, h = img.size
    across = side in ("left", "right")
    size = max(1, min(int(size), w if across else h))
    edge = {"left": (0, 0, 1, h), "right": (w - 1, 0, w, h), "top": (0, 0, w, 1), "bottom": (0, h - 1, w, h)}[side]
    colour = tuple(int(v) for v in ImageStat.Stat(img.crop(edge)).median)
    line = Image.new("L", (w, 1) if across else (1, h))
    line.putdata(ramp(w if across else h, size, side in ("right", "bottom")))
    return Image.composite(Image.new("RGB", img.size, colour), img, line.resize((w, h), Image.NEAREST))

opened = {}
report = []
for job in json.load(sys.stdin):
    if job["src"] not in opened:
        opened[job["src"]] = Image.open(job["src"]).convert("RGB")
    full = opened[job["src"]]
    x0, y0, x1, y1 = job["box"]
    clipped = (max(0, x0), max(0, y0), min(full.width, x1), min(full.height, y1))
    crop = full.crop(clipped)
    for side, size in job.get("fade", {}).items():
        crop = fade(crop, side, size)
    files = []
    for w in job["widths"]:
        h = round(crop.height * w / crop.width)
        img = crop if (w, h) == crop.size else crop.resize((w, h), Image.LANCZOS)
        for fmt, opts in job["formats"].items():
            path = job["out"] + "-" + str(w) + "." + fmt
            if fmt == "avif":
                img.save(path, "AVIF", quality=opts.get("quality", 62), speed=opts.get("speed", 4))
            elif fmt == "webp":
                img.save(path, "WEBP", quality=opts.get("quality", 82), method=opts.get("method", 6))
            else:
                raise SystemExit("unknown format " + fmt)
            files.append(path)
    report.append({"name": job["name"], "crop": list(crop.size), "clipped": list(clipped) != [x0, y0, x1, y1], "files": files})
print(json.dumps(report))
`;

if (!args["html-only"]) {
  if (!sourcesDir) fail("say where the captures are: --sources <folder>, or HANGAR_CAPTURES");
  const jobs = [];
  for (const plan of Object.values(plans)) {
    if (only && !only.has(plan.name)) continue;
    const src = isAbsolute(plan.image.src) ? plan.image.src : join(sourcesDir, plan.image.src);
    if (!existsSync(src)) fail(`${plan.name}: ${src} does not exist`);
    const s = plan.scale;
    const { x, y, w, h } = plan.rect;
    const fadePx = {};
    for (const [side, css] of Object.entries(plan.image.fade || {})) fadePx[side] = Math.round(css * s);
    jobs.push({
      name: plan.name,
      src,
      box: [Math.round(x * s), Math.round(y * s), Math.round((x + w) * s), Math.round((y + h) * s)],
      widths: plan.widths,
      formats: plan.formats,
      fade: fadePx,
      out: join(outDir, plan.name),
    });
  }
  const python = process.env.PYTHON || (process.platform === "win32" ? "python" : "python3");
  const run = spawnSync(python, ["-c", PYTHON], { input: JSON.stringify(jobs), encoding: "utf8", maxBuffer: 1 << 26 });
  if (run.error) fail(`could not run ${python}: ${run.error.message}`);
  if (run.status !== 0) fail(`Pillow failed:\n${run.stderr}`);
  for (const done of JSON.parse(run.stdout)) {
    const plan = plans[done.name];
    const [cw, ch] = done.crop;
    const sizes = done.files.map((f) => `${f.split(/[\\/]/).pop()} ${Math.round(readFileSync(f).length / 1024)} kB`);
    console.log(`${done.name}: ${cw}×${ch} px from ${plan.image.src} → ${sizes.join(", ")}`);
    if (done.clipped) console.log(`  warning: the rectangle runs off ${plan.image.src}; it was clipped`);
    const over = plan.widths.filter((w) => w > cw);
    if (over.length) {
      console.log(`  warning: ${over.join(", ")} px is wider than the ${cw} px the capture has there, so it is upscaled`);
    }
    // Drop the widths this image no longer has.
    const keep = new Set(done.files.map((f) => f.split(/[\\/]/).pop()));
    const own = new RegExp(`^${plan.name}-\\d+\\.(avif|webp|png|jpe?g)$`);
    for (const file of readdirSync(outDir)) {
      if (own.test(file) && !keep.has(file)) {
        unlinkSync(join(outDir, file));
        console.log(`  removed ${file}`);
      }
    }
  }
}

// ── the page ─────────────────────────────────────────────────────────

// A whole tag, quoted values included, so a ">" inside an alt does not end it.
const TAG = (name) => new RegExp(`<(${name})\\b(?:[^>"']|"[^"]*"|'[^']*')*>`, "g");

let html = readFileSync(pagePath, "utf8");
const used = new Set();
html = html.replace(TAG("source|img"), (tag, kind) => {
  const name = (/\sdata-crop="([^"]+)"/.exec(tag) || [])[1];
  if (!name) return tag;
  const plan = plans[name];
  if (!plan) fail(`index.html: data-crop="${name}" is not in crops.json`);
  used.add(name);
  const first = plan.size(plan.widths[0]);
  if (kind === "source") {
    const fmt = (/\btype="image\/([a-z]+)"/.exec(tag) || [])[1];
    if (!fmt || !plan.formats[fmt]) fail(`index.html: a <source> of "${name}" needs type="image/avif" or "image/webp"`);
    tag = setAttr(
      tag,
      "srcset",
      plan.widths.map((w) => `${href}${stamped(`${name}-${w}.${fmt}`)} ${w}w`).join(", "),
    );
  } else {
    const fmt = plan.formats.webp ? "webp" : Object.keys(plan.formats)[0];
    tag = setAttr(tag, "src", `${href}${stamped(`${name}-${plan.widths[0]}.${fmt}`)}`);
  }
  tag = setAttr(tag, "width", String(first.w));
  return setAttr(tag, "height", String(first.h));
});

const markCount = {};
html = html.replace(TAG("span"), (tag) => {
  const name = (/\sdata-of="([^"]+)"/.exec(tag) || [])[1];
  if (!name) return tag;
  const plan = plans[name];
  if (!plan) fail(`index.html: data-of="${name}" is not in crops.json`);
  const i = (markCount[name] = (markCount[name] || 0) + 1) - 1;
  if (!plan.marks[i]) fail(`index.html: ${name} has more outlines than crops.json lists under "marks"`);
  return setAttr(tag, "style", plan.marks[i]);
});
for (const [name, plan] of Object.entries(plans)) {
  if (plan.marks.length && plan.marks.length !== (markCount[name] || 0)) {
    fail(`crops.json lists ${plan.marks.length} marks for ${name}, index.html has ${markCount[name] || 0}`);
  }
  if (!used.has(name)) console.log(`note: ${name} is in crops.json but no tag in index.html uses it`);
}
// The page's own stylesheet, script and share image get the same stamp: the
// site sends them with a four-hour browser cache, and a page fresh from the
// server must never be drawn with the styles, script or images of the one
// before it.
const siteDir = dirname(pagePath);
html = html
  .replace(/(href=")styles\.css(?:\?v=[0-9a-f]+)?(")/g, (_, a, b) => a + stampedIn(siteDir, "styles.css") + b)
  .replace(/(src=")main\.js(?:\?v=[0-9a-f]+)?(")/g, (_, a, b) => a + stampedIn(siteDir, "main.js") + b)
  .replace(/(https:\/\/hangar\.simnjs\.fr\/)og\.png(?:\?v=[0-9a-f]+)?/g, (_, a) => a + stampedIn(siteDir, "og.png"));
writeFileSync(pagePath, html);
console.log(`${pagePath.slice(root.length + 1)}: sizes and srcsets of ${used.size} images written`);

// ── helpers ──────────────────────────────────────────────────────────

/**
 * `file` with `?v=` and the start of its content's hash: a file that changes
 * changes address, so no browser or CDN can pair a new page with an old copy
 * still in its cache under the same name.
 */
function stamped(file) {
  return stampedIn(outDir, file);
}

function stampedIn(dir, file) {
  const path = join(dir, file);
  if (!existsSync(path)) fail(`${file} is referenced but missing from ${dir}`);
  const hash = createHash("sha1").update(readFileSync(path)).digest("hex").slice(0, 10);
  return `${file}?v=${hash}`;
}

function setAttr(tag, name, value) {
  const re = new RegExp(`(\\s${name}=")[^"]*(")`);
  if (re.test(tag)) return tag.replace(re, (_, a, b) => a + value + b);
  return tag.replace(/\s*(\/?)>$/, ` ${name}="${value}"$1>`);
}

function parseArgs(list) {
  const out = {};
  for (let i = 0; i < list.length; i++) {
    const m = /^--([a-z-]+)(?:=(.*))?$/.exec(list[i]);
    if (!m) fail(`unexpected argument ${list[i]}`);
    if (m[2] !== undefined) out[m[1]] = m[2];
    else if (list[i + 1] && !list[i + 1].startsWith("--")) out[m[1]] = list[++i];
    else out[m[1]] = true;
  }
  return out;
}

function fail(message) {
  console.error(`site-crops: ${message}`);
  process.exit(1);
}
