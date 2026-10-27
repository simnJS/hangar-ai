# site

The Hangar.AI landing page, served at https://hangar.simnjs.fr. Plain HTML, CSS
and JavaScript, no framework, no build step for the page itself, and nothing
loaded from another origin.

```
index.html            the page
styles.css            every colour is a token; Graphite unless main.js swaps them
main.js               themes, the demo frame, prompt chips, section bar, download per OS
img/                  real captures of the app, AVIF + WebP, two or three widths each
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
folder, Inter included. It is ignored by git and built by Vercel
(`vercel.json`) and by the `Dockerfile`.

It is a single file on purpose. The demo loads in an iframe, and relative asset
paths depend on how the host spells the URL: `serve` alone rewrites
`/demo/index.html` → `/demo/index` → `/demo`, after which `./assets/x.js`
resolves against the root and 404s. With nothing to resolve, `/demo`, `/demo/`
and `/demo/index.html` all work, on any host, under any base path. The frame
needs `X-Frame-Options: SAMEORIGIN`, which `vercel.json` and `sws.toml` send.

## The page

One story, in order: what Hangar.AI is (hero), the orchestrator in four steps,
the live demo, everything else, downloads, questions.

Every claim on it comes from the code. When a feature changes, check the copy
against `src/i18n.ts` (the English strings say what the app does in plain
words) and against the file the claim names: `src/lib/orchestration.ts` for the
orchestration defaults (3 workers, 2 attempts, merge by itself, Opus plans,
Sonnet works and reviews), `src-tauri/src/delivery.rs` for the delivery
stages, `src/lib/orchestratorBrief.ts` for what the orchestrator is told.

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

Everything in `img/` is the demo build, captured with headless Edge through the
DevTools protocol:

- window 1440 × 900, `--force-device-scale-factor=2` (emulating the scale
  instead makes xterm draw its text twice too large), English forced through
  `navigator.language`;
- Graphite, the demo's default;
- the hero and most crops are the **checkout-v2** workspace, once the opening
  scene has Jade working: Iris orchestrates, Jade and Kai are its workers, and
  its board holds a question and a delivery ready to merge;
- the menu with "Pin as orchestrator" is Ava's pane in hangar-ai; the 3D
  preview is Ava after the prompt `show the model`, with its pane zoomed.

Crops are encoded with Pillow: AVIF at quality 62, WebP at 82, at 1× and 2×
(the hero at 1280, 1920 and 2560 px). Each `<img>` carries its width and
height, and everything below the hero is `loading="lazy"`.

The outlines numbered on the hero (`.mark`) are positioned in percent of that
1440 × 900 capture. Recapture at another size and they need moving.

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
Forgetting to bump is not fatal: the app updates itself after install, and
"Every file of the latest release" always points at `/releases/latest`.

## Deploy

Static. Vercel builds it with `pnpm demo:build` and serves `site/`
(`vercel.json`); the `Dockerfile` does the same with static-web-server. The
canonical address is https://hangar.simnjs.fr, which the canonical link and the
Open Graph tags name.
