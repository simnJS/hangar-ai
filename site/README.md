# site

The Hangar.AI landing page, served at https://hangar.simnjs.fr. Plain HTML, CSS
and JavaScript, no framework, no build step for the page itself, and nothing
loaded from another origin.

```
index.html            the page
styles.css            every colour is a token; Graphite unless main.js swaps them
main.js               themes, section bar, download per OS, the delivery, folds
assets/motion.js      the scroll-driven motion (see Motion below)
assets/mobile.css     the Hangar Mobile section: the phones drawn around their screens
vendor/               GSAP 3.15.0 and its ScrollTrigger, unchanged, and their licence
img/                  crops of captures of the app, AVIF + WebP, cut as img/crops.json says
fonts/                Inter (latin, variable, with optical sizes) and its OFL licence
og.png                the 1200×630 share image for X, Open Graph and the rest
favicon.ico           16/32/48 px, for whatever still asks for /favicon.ico
apple-touch-icon.png  180 px
logo.svg              copied from ../assets/logo.svg
```

## Preview

```bash
npx serve site           # or: python -m http.server -d site 8080
```

The page used to carry a browser demo, the React app built for the web with a
scripted backend. The app is native now, so there is no web build of it to show:
the demo went with the React sources (it is at tag `v1.12.1`, under `demo/`).

## The page

One story, short, in order: what Hangar.AI is (hero: one line, a download for
the visitor's system and one link per OS, the chat view), the chat view, the
orchestrator with its workers and deliveries, Hangar Mobile, kits and
previews, the experiments (nodes, pane scopes), performance, "Also in the
box", downloads, questions. Hangar runs Claude Code only since 2.0, and the
page names no other agent.

Every claim on it comes from the code, `CHANGELOG.md` or the English strings.
When a feature changes, check the copy against `crates/hangar-i18n/dict/en.json`
(the English strings say what the app does in plain words) and against the
file the claim names: `crates/hangar-app/src/state/types.rs` for the defaults
(Opus plans, Sonnet works and reviews; idle workspaces sleep after 10 minutes),
`crates/hangar-core/src/delivery.rs` for the delivery stages,
`crates/hangar-app/src/settings/experiments.rs` for the experiments.

The performance figures are measures only: the 2.2 row is the changelog's
(`[Unreleased]` until 2.2.0 is cut: CPU with eight agents printing, the wave
of resumed sessions, the chat view's start, the executable's size), the 2.0
row is `docs/native/BENCHMARK.md`'s, 1.12.1 against 2.0. Add a figure only
with the measure it comes from.

On a phone (below 600 px) the page keeps every point and drops detail: the
words wrapped in `.wide-only` are left out there, so read them with the rest
when the copy changes. The short facts (under the orchestrator and the kits,
and "Also in the box") are `<details class="fold">`, written open; `main.js`
folds them to their titles on a phone, and above 600 px they simply stay open.

The delivery card is HTML, not a capture. It is written in its final state,
and says it is an illustration. `assets/motion.js` walks it through its four
stages as it scrolls up the screen; if that script did not run, `main.js` plays
them once on a timer when the card comes into view, and with reduced motion it
stays as written.

## Motion

`assets/motion.js`, with GSAP and ScrollTrigger from `vendor/` (3.15.0, the
npm package's `dist/` files as they came; `vendor/GSAP-LICENSE.md` holds the
licence and their checksums). About 46 KB gzipped together; nothing comes from
another origin. To update GSAP, copy the two files from the new package, change
the `?v=3.15.0` of their `<script>` tags, and the version and checksums in the
licence file (the crop script stamps `assets/motion.js`, not `vendor/`).

What moves, top to bottom:

- **Hero.** At load, the headline rises word by word and the rest of the copy
  after it (timed, once). As you scroll, the window leans back in perspective
  and lies flat by the time its top nears the bar, the copy fades back, and the
  accent glow drifts down, slower than the page.
- **Chat view.** The tasks panel comes in part by part (veils of the capture's
  own background lift, top to bottom). On a wide, tall enough screen the
  section then pins: point by point, the part of the panel it is about stays lit
  and the rest dims, and the five progress bars fill, redrawn over the capture
  in its own colours where it has them (`PARTS` and `BARS` in pixels of the
  580 × 1400 crop: move them if `tasks` is cut again).
- **Orchestrator.** The panes window leans back and lies flat as it comes in;
  the delivery's stages run with the scroll, forth and back.
- **Hangar Mobile.** The four phones come in one after the other, from depth,
  fanned out.
- **Kits.** The 3D preview's window comes in turned and leaning, and straightens.
- **Performance.** Each figure counts up as it enters, and a bar under it goes
  from before (the whole bar) to after (its share of it). A figure whose
  "before" is no number gets no bar; the bars are drawn by the script, so a page
  without it has none.
- **Everywhere.** Section titles rise line by line out of a mask; cards, facts
  and short texts fade up as they enter; links within the page glide to their
  section (any wheel, touch or key stops the glide).

How: `gsap.matchMedia()` with three sets. `full` (960 px and wider) does all of
the above; `light` (narrower) drops the pin, the tilts and the phones' depth for
plain rises and fades; `reduce` is next.

**Reduced motion** (`prefers-reduced-motion: reduce`): nothing travels, scales
or counts. Titles, cards and texts only fade in, and every figure, bar and the
delivery show their final state. The inline script in `<head>` does not even set
`motion` then.

Without script everything is visible as written: the only CSS that hides
anything (the hero's copy, before its entrance) applies under `html.motion`,
which the inline script sets before the first paint and takes back after 3 s if
`motion.js` never set `motion-ready`.

Tuning: the numbers live in `TUNE` at the top of `motion.js` (scrub lag, the
hero's tilt, scale, lift and where it lies flat, the glow's drift, how long the
chat stays pinned, the windows' tilt, the reveals' rise, the line stagger, the
counting time); the delivery's stages are `STAGES`, stretches of the card's way
up the screen. Rules to keep: move `transform` and `opacity` only (the
exception is the figures' text while they count, held at its final width so
nothing shifts); give back the final state on the way out of a set (each
returns its cleanups); create the pin before what comes after it. Check with
the page served locally and headless Chrome at 1440 and 390 px, with and
without reduced motion, and a layout-shift observer while scrolling: it should
read 0.

## Design

The page wears the app's own chrome tokens. `styles.css` holds Graphite, the
default theme, exactly as `crates/hangar-theme` has it, and GitHub Light for
a visitor whose system is light (`data-theme="auto"` on `<html>`, a
`prefers-color-scheme` block). `main.js` holds the same tokens for eight
themes, two of them light, and the swatches under "Kits & previews" repaint
the page; picking one sets `data-theme` to its id and `data-scheme` to light
or dark.

The hero has a capture in each scheme (`hero`, `hero-light`): the page shows
the one that matches the theme on screen, and the other, `loading="lazy"`
under `display: none`, is never fetched.

Graphite keeps colour for meaning, and so does the page: green is an agent at
work, amber one waiting on you, cyan your turn, blue the focus and the thing
to click. The one decoration is a soft pool of the theme's accent behind the
hero. Radii follow the app: 4 to 6 px for controls, 8 for cards, 10 for
floating panels and windows. Type is Inter, as in the app, with its optical
sizes doing the display cut of the headlines; monospace is only for what a
terminal would print.

Every text token clears 4.5:1 on every surface it sits on, in all eight themes.

To add a theme to the swatches, take its tokens from the app rather than
picking them by eye. The theme crate prints them:

```bash
cargo run -q -p hangar-theme --example kit-tokens -- kanagawa
```

then map the `--*` names to the keys `TOKENS` lists in `main.js`.

## Captures

Everything in `img/` but the phone screens is the real app: a debug build of
Hangar.AI 2.2 on Linux, on demo states written for the site, with no path,
account or project of the person taking them. The scripts live outside the
repository, in `~/dev/bench/site-shots/`:

- `demo.py` writes a root that a bwrap sandbox mounts at `/home/dev` (HOME,
  projects, data folder, endpoint file): a **Recipes** repository with
  worktrees for its workers, a board with tasks at every stage (progress
  reported, one blocked on a question for you, one ready to land), Iris's
  conversation as a Claude Code transcript, a **landing-pad** repository with
  a GLB model, and one data folder per state;
- `fake_claude.py` stands in for Claude Code in every pane: it prints a
  handcrafted screen (`screens/<pane>.ans`) and tells Hangar through the
  bridge what a real session would (hello, context use, a turn under way, a
  permission waiting, a `model_show` call), so states, the navigator and the
  inspector read as with real agents;
- `shoot.py` starts the debug build per state inside the sandbox
  (`HANGAR_DEV_CAPTURE_DIR`, `kill -USR1` for each capture, `dev_command` to
  switch to the board; `HANGAR_DEV_SETTINGS=nodes` and
  `HANGAR_DEV_NODES_CONNECT=1` for the nodes page, against an sshd container
  of its own on a port of the worker's range).

The captures are lossless PNGs of the window's client area, Graphite unless
named, English, permissions in Auto mode:

- `chat`, `light` (GitHub Light), `panes`, `board`, `model`, `nodes`: a window
  1440 × 900 at scale 2, so 2880 × 1800 px;
- `chatm`, `chatml`: the chat alone (navigator and inspector closed) in a
  window 760 × 900, for the phone's hero, where the wide one would be too small
  to read.

Before a capture goes public, check it frame by frame: no `/home/<you>` or
`C:\Users\…` path, no e-mail address, no other project, and no "bypass
permissions" banner.

`img/crops.json` says what is cut out of the captures, and
`scripts/site-crops.mjs` cuts it:

```bash
node scripts/site-crops.mjs --sources <folder of captures>          # every image
node scripts/site-crops.mjs --sources <folder> --only hero,board     # some of them
```

The pixels are Pillow's, through Python, and it must write AVIF (Pillow 11.2
or later from pip; a distribution's Pillow may lack it): point `PYTHON` at
one that does, such as a virtual environment's.

For each image, `crops.json` gives the capture it comes from, a rectangle in the
capture (`[x, y, width, height]`, in capture pixels), the widths to write, and
optionally a fade over an edge the crop cuts through. The first width sets the
`width` and `height` the page reserves, and an image is never drawn wider than
that, so it must be at least the width the layout gives the image. Every width
becomes `img/<name>-<width>.avif` (quality 62) and `.webp` (quality 82), and
widths an image no longer has are deleted. The script then writes into
`index.html` the `srcset`, `src`, `width` and `height` of each `<source>` and
`<img>` marked `data-crop="<name>"`. It can also place numbered outlines over
an image (`marks`, `<span class="mark" data-of="…">`); the page uses none now.
It warns when a width asks for more pixels than the capture has there, or a
rectangle runs off it.

Every URL it writes carries `?v=` and the start of the file's hash, and so do
`styles.css`, `main.js`, `assets/mobile.css` and the share image `og.png`. The
site sends images, styles and scripts with a four-hour browser cache, so
without it a visitor who came before an update would get the new page with the
old images under it. After changing `styles.css`, `main.js` or `og.png` by
hand, run the script again (`--html-only` is enough) so their stamps follow.

The full-window captures are not kept in the repository; `--sources` names the
folder that holds them. To swap them, put the new ones in a folder, set each
image's `src` and `rect`, and run the script. Nothing in the page is edited by
hand.

Below 600 px each `<picture>` switches, through a `media` source, to the `-m`
crop of the same image, drawn close to the size the app draws it so its text
reads on a phone: the narrow chat, one pane (Eva's permission), two board
columns, the 3D preview, the node's card. Everything below the hero is
`loading="lazy"`.

The phone screens (`mobile-*`) are Hangar Mobile in its mock mode, unchanged
since 2.0; `crops.json` keeps their entries, so cut them with `--only` naming
the others when their sources are not at hand.

`og.png` is an HTML page rendered at 1200 × 630 with a headless Chrome: the
brand, the headline, one line on the orchestrator, and the hero capture
bleeding off the bottom (`~/dev/bench/site-shots/og/og.html`).

## Releasing a new version

The version is written into `index.html` by hand, so the download buttons point
straight at the files. Replace the version (`2.2.0` now) everywhere in the page
(the brand, the download heading, the hero's three links, the five asset links and the JSON-LD). The
asset names are the ones the release workflow uploads, kept from the Tauri
builds (`docs/native/PACKAGING.md`):

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
Cloudflare. The `Dockerfile` at the root copies `site/` as it is into the
server's image; `sws.toml` is its configuration: compression, and on every
response `X-Content-Type-Options: nosniff`, `X-Frame-Options: SAMEORIGIN` (no
other site may frame the page), a referrer policy and a permissions policy.

static-web-server picks the `Content-Type` from the extension, and gets every one
the page uses right (`image/avif`, `image/webp`, `font/woff2`, `image/x-icon`,
`image/svg+xml`), which `nosniff` makes load-bearing: a wrong one and the browser
refuses the file. After a change of server or of image format, check them:

```bash
curl -sI https://hangar.simnjs.fr/img/hero-1280.avif | grep -i content-type
```

The canonical address is https://hangar.simnjs.fr, which the canonical link and
the Open Graph tags name.
