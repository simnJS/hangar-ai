# site

The Hangar.AI landing page. Plain HTML, CSS and JavaScript. No build step for
the page itself.

```
index.html   the page
styles.css   every colour is a token, because the theme switcher repaints them
main.js      the theme switcher, and the bridge that tells the demo to follow
logo.svg     copied from ../assets/logo.svg
demo/        built output, see ../demo. Do not edit by hand
```

## Preview

```bash
pnpm demo:build          # fills site/demo
npx serve site           # or: python -m http.server -d site 8080
```

`site/demo/index.html` is build output: one self-contained file, no assets
folder. Regenerate it with `pnpm demo:build`, and either commit it (GitHub Pages
needs it in the repo) or build it in CI before deploying. Nothing else on the
page needs building.

It is a single file on purpose. The demo loads in an iframe, and relative asset
paths depend on how the host spells the URL: `serve` alone rewrites
`/demo/index.html` → `/demo/index` → `/demo`, after which `./assets/x.js`
resolves against the root and 404s. With nothing to resolve, `/demo`, `/demo/`
and `/demo/index.html` all work, on any host, under any base path.

## The demo

The frame under the hero is **the app**, compiled for the browser. Same React,
same xterm terminals, same board, same settings. What is faked is everything
that was Rust: the pseudo-terminals, the board file, the machine probing. See
[`../demo/README.md`](../demo/README.md).

The agents in it answer from a table of canned transcripts. The page says so
under the prompt chips, and each agent repeats it in its own banner. Keep both.

## Themes

`THEMES` in `main.js` holds eight palettes copied verbatim from
`src/themes.ts`. Clicking one recolours the whole document, and posts the theme
id into the demo frame so the embedded app switches with it, which is exactly
what the setting does inside the real app.

Every other colour is derived from the palette's background, foreground and
accent, so adding one of the remaining 24 means pasting six hex values.

## Typography

System stacks, so the page has nothing to download. To move to the real faces,
self-host them and swap `--ui` / `--mono` in `styles.css`. The intended pairing
is Geist for the interface and JetBrains Mono for anything that came out of a
terminal.

## Deploy

Static. For GitHub Pages, point the source at this folder. For Vercel or
Netlify, set the output directory to `site` and the build command to
`pnpm demo:build`.
