#!/usr/bin/env node
/**
 * Writes what Hangar UI takes from Hangar: the generated part of
 * crates/hangar-core/kit-sdk/kit.css and the Inter file beside it (docs/kit-ui.md).
 *
 *   node scripts/gen-hangar-ui.mjs           write them
 *   node scripts/gen-hangar-ui.mjs --check   fail when either is stale
 *
 * What it takes, and from where:
 *
 * - The colours are the themes' own. `cargo run -p hangar-theme --example
 *   kit-tokens` prints every chrome token of the two themes kit-sdk/tokens.json
 *   names, and tokens.json says which chrome token each `--kit-*` colour is.
 *   hangar-theme's `kit` tests fail when kit.css no longer has those colours.
 * - The icons are the window's: their drawings are read out of hangar-ui's
 *   icon table (crates/hangar-ui/src/icon/data.rs), on the same grid and stroke.
 * - The face is the one Hangar Mobile loads, from @fontsource-variable/inter,
 *   so `pnpm install` comes first.
 * - The measures, the type scale and the font stacks followed the web app's
 *   stylesheets, which left the repository with it (tag v1.12.1). Their last
 *   values are written below, in ROOT_TOKENS and MEASURES, and a measure is
 *   changed here. The window's own sizes are hangar-theme's `tokens`: a size
 *   moved there should be moved here too, for a kit's tab to keep looking like
 *   the window around it.
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const SDK = "crates/hangar-core/kit-sdk";
export const KIT_CSS = `${SDK}/kit.css`;
const TOKENS = `${SDK}/tokens.json`;
const FONT_PACKAGE = "node_modules/@fontsource-variable/inter";
const FONT_CSS = `${FONT_PACKAGE}/opsz.css`;
const FONT_FILES = [
  [`${FONT_PACKAGE}/files/inter-latin-opsz-normal.woff2`, `${SDK}/inter.woff2`],
  [`${FONT_PACKAGE}/LICENSE`, `${SDK}/inter.LICENSE.txt`],
];
const ICONS_SOURCE = "crates/hangar-ui/src/icon/data.rs";

export const START = "/* hangar-ui:generated:start, by scripts/gen-hangar-ui.mjs: do not edit by hand */";
export const END = "/* hangar-ui:generated:end */";

/** The window's font stacks, radii and motion: the `:root` of the web app's styles.css. */
const ROOT_TOKENS = [
  ["--kit-font", '"Inter Variable", -apple-system, "SF Pro Text", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'],
  ["--kit-mono", '"Cascadia Mono", "SF Mono", ui-monospace, Consolas, monospace'],
  ["--kit-radius-xs", "4px"],
  ["--kit-radius-sm", "5px"],
  ["--kit-radius", "6px"],
  ["--kit-radius-lg", "8px"],
  ["--kit-radius-xl", "10px"],
  ["--kit-dur", "140ms"],
  ["--kit-ease", "cubic-bezier(0.2, 0.7, 0.2, 1)"],
];

/**
 * The lengths of the components they were taken from (the web app's rule and
 * property in the comment). A number scales with the interface: `--kit-scale`.
 */
const scaled = (px) => `calc(${px}px * var(--kit-scale))`;
const MEASURES = [
  // Heights.
  ["--kit-h-control", scaled(26)], // .btn min-height
  ["--kit-h-small", scaled(22)], // .btn--tiny min-height
  ["--kit-h-row", scaled(28)], // .sidebar --nav-row
  ["--kit-h-rich-row", scaled(42)], // .set-row min-height
  ["--kit-h-search", scaled(28)], // .settings__search height
  ["--kit-h-filter", scaled(24)], // .board-filter__button height
  ["--kit-h-head", scaled(48)], // .settings__head height
  ["--kit-h-bar", scaled(32)], // .model-panel__bar min-height
  ["--kit-h-menu-item", scaled(26)], // .board-filter__option height
  ["--kit-h-notice", scaled(30)], // .notice min-height
  ["--kit-h-tabs", scaled(26)], // .seg height
  // The type scale.
  ["--kit-fs-title", scaled(15)], // .settings__title font-size
  ["--kit-fs-body", scaled(13)], // body font-size
  ["--kit-fs-row", scaled(12.5)], // .nav-ws__name font-size
  ["--kit-fs-control", scaled(12)], // .set-input font-size
  ["--kit-fs-small", scaled(11.5)], // .btn--tiny font-size
  ["--kit-fs-meta", scaled(11)], // .nav-ws__hint font-size
  ["--kit-fs-label", scaled(10.5)], // .nav-section font-size
  ["--kit-fs-chip", scaled(10)], // .chip font-size
  ["--kit-fs-code", scaled(11.5)], // .set-text font-size
  ["--kit-title-tracking", "-0.01em"], // .settings__title letter-spacing
  ["--kit-caps-tracking", "0.06em"], // .nav-section letter-spacing
  ["--kit-font-features", '"cv11", "ss01"'], // body font-feature-settings
  // Parts of a control.
  ["--kit-switch-w", scaled(30)], // .switch__track width
  ["--kit-switch-h", scaled(18)], // .switch__track height
  ["--kit-switch-knob", scaled(14)], // .switch__knob width
  ["--kit-dot", scaled(8)], // .glyph --glyph-size
  ["--kit-glyph", scaled(10)], // .model-tab__spinner width
  ["--kit-progress-h", "3px"], // .model-progress height
  ["--kit-scrollbar", "9px"], // ::-webkit-scrollbar width
];

/** Hangar's stylesheets had no spacing scale: these are the steps they used. */
const SPACES = [2, 4, 6, 8, 10, 12, 16, 20, 24];

/** Icons of the window's set a tab may want, by the name the set gives them. */
const ICONS = [
  "alert",
  "check",
  "chevronDown",
  "close",
  "copy",
  "info",
  "lock",
  "more",
  "plus",
  "restart",
  "search",
  "updown",
];

/** The stroke every icon of the set is drawn with (hangar-ui's `document`). */
const ICON_STROKE = "1.35";

/**
 * The chrome tokens the web app typed as constants rather than computed, and
 * wrote without spaces: `rgba(0,0,0,0.45)`. hangar-theme spells every colour
 * one way; these keep their old spelling, so the stylesheet changes when a
 * colour does and not for a space.
 */
const TYPED_TOGETHER = new Set(["--shadow", "--float-ring"]);
const spelled = (source, value) => (TYPED_TOGETHER.has(source) ? value.replace(/,\s+/g, ",") : value);

const stripComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, "");

function block(selector, declarations, indent = "") {
  const lines = declarations.map(([name, value]) => `${indent}  ${name}: ${value};`);
  return `${indent}${selector} {\n${lines.join("\n")}\n${indent}}`;
}

/**
 * The drawings of hangar-ui's icon table, by name: `Icon::Check => "check"`
 * names one, `Icon::Check => r##"<path …/>"##` draws it. The table paints in
 * `#000`, since the window tints every pixel anyway; a kit's mask keeps the
 * `currentColor` the set was drawn with.
 */
export function iconDrawings(table) {
  const names = new Map([...table.matchAll(/Icon::(\w+) => "(\w+)",/g)].map(([, variant, name]) => [variant, name]));
  const drawings = new Map();
  for (const [, variant, svg] of table.matchAll(/Icon::(\w+) => r##"(.*)"##,/g)) {
    const name = names.get(variant);
    if (name) drawings.set(name, svg.replaceAll("#000", "currentColor"));
  }
  return drawings;
}

/** An icon as the data URL a mask takes: its own drawing, on the set's grid, stroke and caps. */
function iconUrl(drawings, name) {
  const drawing = drawings.get(name);
  if (!drawing) throw new Error(`the icon "${name}" is gone from ${ICONS_SOURCE}`);
  const svg =
    `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='black' ` +
    `stroke-width='${ICON_STROKE}' stroke-linecap='round' stroke-linejoin='round'>${drawing.replace(/"/g, "'")}</svg>`;
  return `url("data:image/svg+xml,${svg.replace(/</g, "%3C").replace(/>/g, "%3E").replace(/#/g, "%23")}")`;
}

const kebab = (name) => name.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`);

function fontFace(fontCss) {
  const face = stripComments(fontCss)
    .match(/@font-face\s*\{[^}]*\}/g)
    ?.find((candidate) => candidate.includes("inter-latin-opsz-normal.woff2"));
  if (!face) throw new Error("the Latin face of Inter is gone from @fontsource-variable/inter/opsz.css");
  return face.replace(/url\([^)]*\)/, "url(inter.woff2)");
}

/**
 * The generated part of kit.css. `sources` holds what Hangar is made of:
 * `{ tokenMap, chromeTokens(themeId), fontCss, iconTable }`.
 */
export function renderGenerated({ tokenMap, chromeTokens, fontCss, iconTable }) {
  const colorsOf = (themeId) => {
    const chrome = chromeTokens(themeId);
    return Object.entries(tokenMap.colors).map(([token, source]) => {
      if (!(source in chrome)) throw new Error(`${token} follows ${source}, which hangar-theme no longer has`);
      return [token, spelled(source, chrome[source])];
    });
  };
  const drawings = iconDrawings(iconTable);
  const fixed = [
    ["--kit-scale", "1"],
    ...ROOT_TOKENS,
    ...MEASURES,
    ...SPACES.map((space) => [`--kit-space-${space}`, `${space}px`]),
    ...ICONS.map((name) => [`--kit-icon-${kebab(name)}`, iconUrl(drawings, name)]),
  ];
  const dark = [["color-scheme", "dark"], ...colorsOf(tokenMap.themes.dark)];
  const light = [["color-scheme", "light"], ...colorsOf(tokenMap.themes.light)];
  return [
    START,
    fontFace(fontCss),
    block(":root", fixed),
    block(':root,\n:root[data-kit-scheme="light"]', light),
    `@media (prefers-color-scheme: dark) {\n${block(":root:not([data-kit-scheme])", dark, "  ")}\n}`,
    block(':root[data-kit-scheme="dark"]', dark),
    END,
  ].join("\n");
}

/** Reads a file of the repository as text, with this checkout's line ends gone. */
export const readText = (path) => readFileSync(join(ROOT, path), "utf8").replace(/\r\n/g, "\n");

/** The chrome tokens of the themes tokens.json names, from hangar-theme: `{ id: { "--fg": "#…" } }`. */
export function themeTokens(tokenMap) {
  const output = execFileSync(
    "cargo",
    ["run", "--quiet", "-p", "hangar-theme", "--example", "kit-tokens", "--", tokenMap.themes.dark, tokenMap.themes.light],
    { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  );
  return JSON.parse(output);
}

/** Hangar's side of `renderGenerated`. */
export function readSources() {
  const tokenMap = JSON.parse(readText(TOKENS));
  const themes = themeTokens(tokenMap);
  return {
    tokenMap,
    chromeTokens: (themeId) => themes[themeId],
    fontCss: readText(FONT_CSS),
    iconTable: readText(ICONS_SOURCE),
  };
}

/** kit.css with its generated part replaced. */
export function withGenerated(sheet, generated) {
  const from = sheet.indexOf(START);
  const to = sheet.indexOf(END);
  if (from < 0 || to < from) throw new Error(`${KIT_CSS} has lost its generated part's markers`);
  return sheet.slice(0, from) + generated + sheet.slice(to + END.length);
}

/** The font files whose copy under kit-sdk differs from the package's. */
export function staleFontFiles() {
  return FONT_FILES.filter(([from, to]) => {
    try {
      return !readFileSync(join(ROOT, from)).equals(readFileSync(join(ROOT, to)));
    } catch {
      return true;
    }
  }).map(([, to]) => to);
}

function main() {
  const sheet = readText(KIT_CSS);
  const next = withGenerated(sheet, renderGenerated(readSources()));
  if (process.argv.includes("--check")) {
    const stale = [...(next === sheet ? [] : [KIT_CSS]), ...staleFontFiles()];
    if (stale.length) {
      console.error(`Stale: ${stale.join(", ")}. Run node scripts/gen-hangar-ui.mjs.`);
      process.exit(1);
    }
    console.log(`${KIT_CSS} and the font beside it are up to date.`);
    return;
  }
  writeFileSync(join(ROOT, KIT_CSS), next);
  for (const [from, to] of FONT_FILES) copyFileSync(join(ROOT, from), join(ROOT, to));
  console.log(`Wrote ${KIT_CSS} and the font beside it.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
