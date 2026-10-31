#!/usr/bin/env node
/**
 * Writes what Hangar UI takes from Hangar: the generated part of
 * src-tauri/kit-sdk/kit.css and the Inter file beside it (docs/kit-ui.md).
 *
 *   node scripts/gen-hangar-ui.mjs
 *
 * Nothing in that part is copied by hand. The colours come from
 * deriveChromeTokens (src/themes.ts) through kit-sdk/tokens.json, the measures
 * from the rules of Hangar's own stylesheets, the face from the package the app
 * loads it from. src/lib/hangarUi.test.ts runs this same code and fails when the
 * file no longer matches, so a change in Hangar cannot leave the library behind.
 */
import { copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const SDK = "src-tauri/kit-sdk";
export const KIT_CSS = `${SDK}/kit.css`;
const TOKENS = `${SDK}/tokens.json`;
const FONT_PACKAGE = "node_modules/@fontsource-variable/inter";
const FONT_CSS = `${FONT_PACKAGE}/opsz.css`;
const FONT_FILES = [
  [`${FONT_PACKAGE}/files/inter-latin-opsz-normal.woff2`, `${SDK}/inter.woff2`],
  [`${FONT_PACKAGE}/LICENSE`, `${SDK}/inter.LICENSE.txt`],
];
/** The app's stylesheets, as src/stylesheet.ts orders them. */
const STYLESHEETS = ["src/styles.css", "src/styles/model.css"];
const ICONS_SOURCE = "src/components/Icon.tsx";

export const START = "/* hangar-ui:generated:start, by scripts/gen-hangar-ui.mjs: do not edit by hand */";
export const END = "/* hangar-ui:generated:end */";

/** `:root` of src/styles.css, under the name that says what each one is. */
const ROOT_TOKENS = [
  ["--kit-font", "--ui-font"],
  ["--kit-mono", "--mono"],
  ["--kit-radius-xs", "--radius-xs"],
  ["--kit-radius-sm", "--radius-sm"],
  ["--kit-radius", "--radius"],
  ["--kit-radius-lg", "--radius-lg"],
  ["--kit-radius-xl", "--radius-xl"],
  ["--kit-dur", "--dur"],
  ["--kit-ease", "--ease"],
];

/**
 * The same lengths as the components they come from: token, rule, property.
 * A rule that goes away stops the generator rather than leaving a stale number.
 */
const MEASURES = [
  // Heights.
  ["--kit-h-control", ".btn", "min-height"],
  ["--kit-h-small", ".btn--tiny", "min-height"],
  ["--kit-h-row", ".sidebar", "--nav-row"],
  ["--kit-h-rich-row", ".set-row", "min-height"],
  ["--kit-h-search", ".settings__search", "height"],
  ["--kit-h-filter", ".board-filter__button", "height"],
  ["--kit-h-head", ".settings__head", "height"],
  ["--kit-h-bar", ".model-panel__bar", "min-height"],
  ["--kit-h-menu-item", ".board-filter__option", "height"],
  ["--kit-h-notice", ".notice", "min-height"],
  ["--kit-h-tabs", ".seg", "height"],
  // The type scale.
  ["--kit-fs-title", ".settings__title", "font-size"],
  ["--kit-fs-body", "body", "font-size"],
  ["--kit-fs-row", ".nav-ws__name", "font-size"],
  ["--kit-fs-control", ".set-input", "font-size"],
  ["--kit-fs-small", ".btn--tiny", "font-size"],
  ["--kit-fs-meta", ".nav-ws__hint", "font-size"],
  ["--kit-fs-label", ".nav-section", "font-size"],
  ["--kit-fs-chip", ".chip", "font-size"],
  ["--kit-fs-code", ".set-text", "font-size"],
  ["--kit-title-tracking", ".settings__title", "letter-spacing"],
  ["--kit-caps-tracking", ".nav-section", "letter-spacing"],
  ["--kit-font-features", "body", "font-feature-settings"],
  // Parts of a control.
  ["--kit-switch-w", ".switch__track", "width"],
  ["--kit-switch-h", ".switch__track", "height"],
  ["--kit-switch-knob", ".switch__knob", "width"],
  ["--kit-dot", ".glyph", "--glyph-size"],
  ["--kit-glyph", ".model-tab__spinner", "width"],
  ["--kit-progress-h", ".model-progress", "height"],
  ["--kit-scrollbar", "::-webkit-scrollbar", "width"],
];

/** Hangar's stylesheets have no spacing scale: these are the steps they use. */
const SPACES = [2, 4, 6, 8, 10, 12, 16, 20, 24];

/** Icons of components/Icon.tsx a tab may want, by the name it has there. */
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

const stripComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, "");

/**
 * The top-level style rules of a stylesheet. At-rules are left out with what
 * they hold: the base value of a measure is what the library follows.
 */
export function parseRules(css) {
  const source = stripComments(css);
  const rules = [];
  let at = 0;
  while (at < source.length) {
    const open = source.indexOf("{", at);
    if (open < 0) break;
    let depth = 1;
    let end = open + 1;
    while (end < source.length && depth > 0) {
      if (source[end] === "{") depth += 1;
      else if (source[end] === "}") depth -= 1;
      end += 1;
    }
    const prelude = source.slice(at, open).trim();
    if (!prelude.startsWith("@")) {
      rules.push({
        selectors: prelude.split(",").map((part) => part.trim()),
        body: source.slice(open + 1, end - 1),
      });
    }
    at = end;
  }
  return rules;
}

/** A property's value in the first rule naming exactly `selector` that sets it. */
export function declaration(rules, selector, property) {
  const pattern = new RegExp(`(?:^|[;{\\s])${property.replace(/[-]/g, "\\-")}\\s*:\\s*([^;]+);`);
  for (const rule of rules) {
    if (!rule.selectors.includes(selector)) continue;
    const found = rule.body.match(pattern);
    if (found) return found[1].replace(/\s+/g, " ").trim();
  }
  throw new Error(`Hangar UI follows ${selector} { ${property} }, which is gone from Hangar's stylesheets`);
}

/**
 * A Hangar value in the library's own terms: its interface size follows
 * `--kit-scale`. A size Hangar scales when it uses it, like a glyph's, is
 * scaled here, where it is used through a token.
 */
function measureValue(value, property) {
  const named = value.replaceAll("--ui-scale", "--kit-scale");
  return property === "--glyph-size" ? `calc(${named} * var(--kit-scale))` : named;
}

function block(selector, declarations, indent = "") {
  const lines = declarations.map(([name, value]) => `${indent}  ${name}: ${value};`);
  return `${indent}${selector} {\n${lines.join("\n")}\n${indent}}`;
}

/**
 * An icon of Icon.tsx as the data URL a mask takes: its own drawing, on the
 * grid, stroke and caps that file gives every icon.
 */
function iconUrl(iconSource, name) {
  const found = iconSource.match(
    new RegExp(String.raw`\n\s*${name}:\s*\(?\s*(?:<>)?([\s\S]*?)(?:</>\s*)?\)?,\n`),
  );
  const stroke = iconSource.match(/strokeWidth=\{([\d.]+)\}/)?.[1];
  if (!found || !stroke) throw new Error(`the icon "${name}" is gone from ${ICONS_SOURCE}`);
  const drawing = found[1].trim().replace(/\s*\n\s*/g, "").replace(/\s*\/>/g, "/>").replace(/"/g, "'");
  const svg =
    `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='black' ` +
    `stroke-width='${stroke}' stroke-linecap='round' stroke-linejoin='round'>${drawing}</svg>`;
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
 * `{ tokenMap, chromeTokens(themeId), css, fontCss, iconSource }`.
 */
export function renderGenerated({ tokenMap, chromeTokens, css, fontCss, iconSource }) {
  const rules = parseRules(css);
  const colorsOf = (themeId) => {
    const chrome = chromeTokens(themeId);
    return Object.entries(tokenMap.colors).map(([token, source]) => {
      if (!(source in chrome)) throw new Error(`${token} follows ${source}, which themes.ts no longer derives`);
      return [token, chrome[source]];
    });
  };
  const fixed = [
    ["--kit-scale", "1"],
    ...ROOT_TOKENS.map(([token, source]) => [token, declaration(rules, ":root", source)]),
    ...MEASURES.map(([token, selector, property]) => [
      token,
      measureValue(declaration(rules, selector, property), property),
    ]),
    ...SPACES.map((space) => [`--kit-space-${space}`, `${space}px`]),
    ...ICONS.map((name) => [`--kit-icon-${kebab(name)}`, iconUrl(iconSource, name)]),
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

/** Hangar's side of `renderGenerated`, but for the theme function the caller imports. */
export function readSources(chromeTokens) {
  return {
    tokenMap: JSON.parse(readText(TOKENS)),
    chromeTokens,
    css: STYLESHEETS.map(readText).join("\n"),
    fontCss: readText(FONT_CSS),
    iconSource: readText(ICONS_SOURCE),
  };
}

/** kit.css with its generated part replaced. */
export function withGenerated(sheet, generated) {
  const from = sheet.indexOf(START);
  const to = sheet.indexOf(END);
  if (from < 0 || to < from) throw new Error(`${KIT_CSS} has lost its generated part's markers`);
  return sheet.slice(0, from) + generated + sheet.slice(to + END.length);
}

/** The generated part as it stands in kit.css. */
export function generatedIn(sheet) {
  const from = sheet.indexOf(START);
  const to = sheet.indexOf(END);
  if (from < 0 || to < from) throw new Error(`${KIT_CSS} has lost its generated part's markers`);
  return sheet.slice(from, to + END.length);
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

async function main() {
  // Vite is already what reads the app's TypeScript, on every Node version.
  const { runnerImport } = await import("vite");
  const { module: themes } = await runnerImport(join(ROOT, "src/themes.ts"), {
    configFile: false,
    logLevel: "silent",
  });
  const chromeTokens = (themeId) => themes.deriveChromeTokens(themes.getTheme(themeId));
  const sheet = readText(KIT_CSS);
  writeFileSync(join(ROOT, KIT_CSS), withGenerated(sheet, renderGenerated(readSources(chromeTokens))));
  for (const [from, to] of FONT_FILES) copyFileSync(join(ROOT, from), join(ROOT, to));
  console.log(`Wrote ${KIT_CSS} and the font beside it.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
  process.exit(0);
}
