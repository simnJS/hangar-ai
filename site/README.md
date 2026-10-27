# site

The Hangar.AI landing page, served at https://hangar.simnjs.fr. Plain HTML, CSS
and JavaScript, no framework, no build step for the page itself, and nothing
loaded from another origin.

```
index.html            the page
styles.css            every colour is a token; Graphite unless main.js swaps them
main.js               themes, the demo frame, prompt chips, section bar, download per OS
img/                  crops of captures of the app, AVIF + WebP, cut as img/crops.json says
fonts/                Inter (latin, variable, with optical sizes) and its OFL licence
og.png                the 1200×630 share image for X, Open Graph and the rest
favicon.ico           16/32/48 px, for whatever still asks for /favicon.ico
apple-touch-icon.png  180 px
logo.svg              copied from ../assets/logo.svg
demo/                 built output, see ../demo. Do not edit by hand
```

## Preview

```bash
pnpm demo:build          # fills site/demo
npx serve site           # or: python -m http.server -d site 8080
```

`site/demo/index.html` is build output: one self-contained file, no assets
folder, Inter included. It is ignored by git and built by the `Dockerfile`.

It is a single file on purpose. The demo loads in an iframe, and relative asset
paths depend on how the host spells the URL: `serve` alone rewrites
`/demo/index.html` → `/demo/index` → `/demo`, after which `./assets/x.js`
resolves against the root and 404s. With nothing to resolve, `/demo`, `/demo/`
and `/demo/index.html` all work, on any host, under any base path. The frame
needs `X-Frame-Options: SAMEORIGIN`, which `sws.toml` sends.

## The page

One story, in order: what Hangar.AI is (hero), the orchestrator in four steps,
the live demo, everything else, downloads, questions.

Every claim on it comes from the code. When a feature changes, check the copy
against `src/i18n.ts` (the English strings say what the app does in plain
words) and against the file the claim names: `src/lib/orchestration.ts` for the
orchestration defaults (3 workers, 2 attempts, merge by itself, Opus plans,
Sonnet works and reviews), `src-tauri/src/delivery.rs` for the delivery
stages, `src/lib/orchestratorBrief.ts` for what the orchestrator is told.

On a phone (below 600 px) the page keeps every point and drops detail: the
words wrapped in `.wide-only` are left out there, so read them with the rest
when the copy changes. The facts under the orchestrator and the list under
"Everything else" are `<details class="fold">`, written open; `main.js` folds
them to their titles on a phone.

The delivery card in step 4 is HTML, not a capture. It is written in its final
state; `main.js` resets it and plays the four stages once when it scrolls into
view, unless the visitor asked for reduced motion. It is the only thing on the
page that moves by itself, and it says it is an illustration.

## Design

The page wears the app's own chrome tokens. `styles.css` holds Graphite, the
default theme, exactly as `deriveChromeTokens()` in `src/themes.ts` computes
it; `main.js` holds the same tokens for seven more themes, two of them light,
and the swatches in "An interface built like an IDE" repaint the page and post
the theme to the demo, which switches with it.

Graphite keeps colour for meaning, and so does the page: green is an agent at
work, amber one waiting on you, cyan your turn, blue the focus and the thing to
click. Radii follow the app: 4 to 6 px for controls, 8 for cards, 10 for
floating panels and windows. Type is Inter, as in the app, with its optical
sizes doing the display cut of the headlines; monospace is only for what a
terminal would print.

Every text token clears 4.5:1 on every surface it sits on, in all eight themes.

To add a theme to the swatches, take its tokens from the app rather than
picking them by eye. A throwaway vitest file in `src/` can print them:

```ts
import { THEMES, deriveChromeTokens } from "./themes";
import { test } from "vitest";
test("tokens", () => {
  const t = THEMES.find((x) => x.id === "kanagawa")!;
  console.log(JSON.stringify(deriveChromeTokens(t), null, 1));
});
```

then map the `--*` names to the keys `TOKENS` lists in `main.js`.

## Captures

Everything in `img/` is the real app, not the demo: Hangar.AI 1.0 on Windows,
with real Claude Code agents (the orchestrator on Opus, workers and reviewer on
Sonnet) orchestrating **checkout-v2**, a small sample repository, over one
afternoon. The captures are lossless PNGs of the window's client area:

- window 1280 × 720 at scale 2, so each capture is 2560 × 1440 px;
- Graphite, the interface in English, permissions in Auto mode;
- a run of its own (another data folder, another identifier), so no other
  project or path of the person taking them appears;
- the hero is round 2: Iris, the orchestrator, reads Zoe's delivery on the
  coupon task while Eli writes the order receipt; the board, Needs you and the
  orchestration settings are the same minutes; the grid of four panes is round
  4, with Ava, Max and Leo at work;
- the pane menu, the palette filtered on "orchestrat" and the 3D preview are
  Eve's pane in storefront; the statistics are filtered on checkout-v2, the only
  project on screen.

Before a capture goes public, check it frame by frame: no `C:\Users\…` or
`%APPDATA%` path (a worker's launch line shows one for a second or two), no
e-mail address, no other project, and no "bypass permissions" banner.

`img/crops.json` says what is cut out of the captures, and
`scripts/site-crops.mjs` cuts it:

```bash
node scripts/site-crops.mjs --sources <folder of captures>          # every image
node scripts/site-crops.mjs --sources <folder> --only hero,board     # some of them
```

For each image, `crops.json` gives the capture it comes from, a rectangle in the
capture (`[x, y, width, height]`, in units of `scale` capture pixels: 1 for
these captures, so plain capture pixels), the widths to write, and optionally a
fade over an edge the crop cuts through. The first width sets the `width` and
`height` the page reserves, and an image is never drawn wider than that, so it
must be at least the width the layout gives the image. Every width becomes
`img/<name>-<width>.avif` (quality 62) and `.webp` (quality 82), encoded by
Pillow through Python (`pip install pillow`), and widths an image no longer has
are deleted. The script then writes into `index.html` the `srcset`, `src`,
`width` and `height` of each `<source>` and `<img>` marked `data-crop="<name>"`,
and places the hero's numbered outlines (`<span class="mark" data-of="hero">`,
in the order of the hero's `marks`) in percent of its crop. It warns when a
width asks for more pixels than the capture has there, or a rectangle runs off
it.

A rectangle can also point into the index of element rectangles that comes
with the captures (`rects`, a path from the captures folder; here
`../../index.json`, in capture pixels): `{ "ref":
"stills/en/s-grid/rects/workers/Max/box" }`, where a step through a list is a
position or an entry's `name`, with an optional `pad` and `x`, `y`, `w` or `h`
to override, or `{ "union": [...] }` over several.

The full-window captures are not kept in the repository; `--sources` names the
folder that holds them. To swap them, put the new ones in a folder, set each
image's `src` and `rect` (or `rects` and the refs) and `scale`, and run the
script. Nothing in the page is edited by hand.

Below 600 px each `<picture>` switches, through a `media` source, to the `-m`
crop of the same image: the orchestrator's pane, two board columns, one
worker, the Needs you popover, the pane menu, two statistics cards, the 3D
preview, drawn close to the size the app draws them so their text reads on a
phone. There the hero's outlines give way to the one line of its legend
that matches its crop, and the palette capture is left out. Everything below
the hero is `loading="lazy"`.

The README's images (`../.github/assets`) are cut from the same captures, at
full capture size: the hero without the navigator, so the panes read at the
width of a README, then the Needs you popover, the orchestration settings, the
board and the statistics cards. `../.github/social-preview.png` (1280 × 640) is
rendered the way `og.png` is, with the hero capture.

`og.png` is an HTML page rendered at 1200 × 630: the brand, the headline, one
line on the orchestrator, and the hero capture bleeding off the bottom.

## Releasing a new version

The version is written into `index.html` by hand, so the download buttons point
straight at the files. Replace `1.0.0` everywhere in the page (the brand, the
release link, the download heading, the five asset links and the JSON-LD). The
asset names follow what tauri-action uploads:

```
Hangar.AI_<v>_x64-setup.exe
Hangar.AI_<v>_universal.dmg
Hangar.AI_<v>_amd64.AppImage
Hangar.AI_<v>_amd64.deb
Hangar.AI-<v>-1.x86_64.rpm
```

`gh release view v<v> --json assets` lists them once the release is out.
Forgetting to bump is not fatal: the app offers each new version once installed,
and "Every file of the latest release" always points at `/releases/latest`.

## Deploy

Static, served by [static-web-server](https://static-web-server.net) behind
Cloudflare. The `Dockerfile` at the root runs `pnpm demo:build` and copies
`site/` into the server's image; `sws.toml` is its configuration: compression,
and on every response `X-Content-Type-Options: nosniff`, `X-Frame-Options:
SAMEORIGIN` (the demo's frame), a referrer policy and a permissions policy.

static-web-server picks the `Content-Type` from the extension, and gets every one
the page uses right (`image/avif`, `image/webp`, `font/woff2`, `image/x-icon`,
`image/svg+xml`), which `nosniff` makes load-bearing: a wrong one and the browser
refuses the file. After a change of server or of image format, check them:

```bash
curl -sI https://hangar.simnjs.fr/img/hero-1280.avif | grep -i content-type
```

The canonical address is https://hangar.simnjs.fr, which the canonical link and
the Open Graph tags name.
