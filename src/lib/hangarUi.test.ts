import { describe, expect, it } from "vitest";
import tokenMap from "../../src-tauri/kit-sdk/tokens.json";
// @ts-expect-error A plain script: no types to have.
import * as generator from "../../scripts/gen-hangar-ui.mjs";
import { deriveChromeTokens, FILL_CONTRAST, getTheme, TEXT_CONTRAST, THEMES } from "../themes";
import { DEFAULT_SETTINGS } from "../types";
import { contrastRatio, mix, toHex } from "./contrast";
import { typeset } from "./typography";

/** Hangar UI (docs/kit-ui.md), held to the sources it is generated from. */

// Read by the generator's own reader, as the sheet is what it writes: Vitest
// empties a `.css` import, `?raw` or not.
const sheet: string = generator.readText(generator.KIT_CSS);
const sdkScript: string = generator.readText("src-tauri/kit-sdk/kit.js");
const uiScript: string = generator.readText("src-tauri/kit-sdk/kit-ui.js");
const gallery: string = generator.readText("src-tauri/kit-sdk/gallery.html");
const docs: string = generator.readText("docs/kit-ui.md");
const generated: string = generator.generatedIn(sheet);
const chromeTokens = (id: string) => deriveChromeTokens(getTheme(id));

/** The declarations of a block of the generated part, as a map. */
function declarations(body: string): Map<string, string> {
  return new Map(
    [...body.matchAll(/(--kit-[a-z0-9-]+):\s*([^;]+);/g)].map(([, name, value]) => [name, value]),
  );
}

function blockOf(pattern: RegExp): Map<string, string> {
  const found = generated.match(pattern);
  if (!found) throw new Error(`no block ${pattern}`);
  return declarations(found[1]);
}

const light = blockOf(/:root,\n:root\[data-kit-scheme="light"\] \{([^}]*)\}/);
const darkForced = blockOf(/\n:root\[data-kit-scheme="dark"\] \{([^}]*)\}/);
const darkBySystem = blockOf(
  /@media \(prefers-color-scheme: dark\) \{\s*:root:not\(\[data-kit-scheme\]\) \{([^}]*)\}/,
);
const fixed = blockOf(/\n:root \{([^}]*)\}/);

const colorTokens = Object.keys(tokenMap.colors);

/** Every class a stylesheet's selectors name. */
const classesIn = (css: string) =>
  new Set([...css.replace(/url\([^)]*\)/g, "").matchAll(/\.(kit-[a-z0-9_-]+)/g)].map(([, name]) => name));

const cssClasses = classesIn(sheet);
/** What a class belongs to: `kit-row` for `kit-row__title` and `kit-row--wrap`. */
const blockName = (name: string) => name.replace(/(__|--).*$/, "");

describe("the generated part of kit.css", () => {
  it("is what Hangar's sources give, or run `node scripts/gen-hangar-ui.mjs`", () => {
    expect(generator.renderGenerated(generator.readSources(chromeTokens))).toBe(generated);
  });

  it("carries the font the app loads, byte for byte", () => {
    expect(generator.staleFontFiles()).toEqual([]);
  });

  it("is made from the themes the app is designed in, its default among them", () => {
    expect(tokenMap.themes.dark).toBe(DEFAULT_SETTINGS.themeId);
    expect(getTheme(tokenMap.themes.dark).appearance).toBe("dark");
    expect(getTheme(tokenMap.themes.light).appearance).toBe("light");
    expect(getTheme(tokenMap.themes.light).id).toBe(tokenMap.themes.light);
  });

  it("follows the chrome: a colour moved in Hangar is a colour moved here", () => {
    const moved = (id: string) => ({ ...chromeTokens(id), "--fg": "#123456" });
    const output: string = generator.renderGenerated({
      ...generator.readSources(moved),
    });
    expect(output).toContain("--kit-text: #123456;");
    expect(output).not.toBe(generated);
  });

  it("follows the stylesheets: a measure moved in Hangar is a measure moved here", () => {
    const sources = generator.readSources(chromeTokens);
    const moved = sources.css.replace(
      /(\.btn \{[^}]*min-height: calc\()26px/,
      "$128px",
    );
    expect(moved).not.toBe(sources.css);
    expect(generator.renderGenerated({ ...sources, css: moved })).toContain(
      "--kit-h-control: calc(28px * var(--kit-scale));",
    );
  });

  it("stops rather than keep a number whose rule is gone", () => {
    const sources = generator.readSources(chromeTokens);
    const gone = sources.css.replace(/\.btn--tiny \{[^}]*\}/, "");
    expect(() => generator.renderGenerated({ ...sources, css: gone })).toThrow(/\.btn--tiny/);
    expect(() =>
      generator.renderGenerated({
        ...sources,
        tokenMap: { ...tokenMap, colors: { "--kit-bg": "--no-such-token" } },
      }),
    ).toThrow(/--no-such-token/);
  });
});

describe("the components, on those tokens", () => {
  const rules = generator.parseRules(sheet);

  it.each([
    [".kit-btn", "min-height", "var(--kit-h-control)"],
    [".kit-btn--small", "min-height", "var(--kit-h-small)"],
    [".kit-btn", "border-radius", "var(--kit-radius)"],
    [".kit-row", "min-height", "var(--kit-h-row)"],
    [".kit-row", "border-radius", "var(--kit-radius)"],
    [".kit-input", "min-height", "var(--kit-h-control)"],
    [".kit-input", "border-radius", "var(--kit-radius-sm)"],
    [".kit-search", "height", "var(--kit-h-search)"],
    [".kit-switch__track", "width", "var(--kit-switch-w)"],
    [".kit-filter", "height", "var(--kit-h-filter)"],
    [".kit-chip", "border-radius", "var(--kit-radius-xs)"],
    [".kit-header", "min-height", "var(--kit-h-head)"],
    [".kit-section__title", "font-size", "var(--kit-fs-label)"],
    [".kit-menu__item", "min-height", "var(--kit-h-menu-item)"],
    [".kit-progress", "height", "var(--kit-progress-h)"],
    [":focus-visible", "outline", "2px solid var(--kit-focus)"],
  ])("%s takes its %s from %s", (selector, property, value) => {
    expect(generator.declaration(rules, selector, property)).toBe(value);
  });

  it("writes no colour of its own, the switch's white knob and shadows aside", () => {
    const written = sheet.slice(sheet.indexOf(generator.END));
    const colours = [...written.matchAll(/#[0-9a-fA-F]{3,8}\b/g)].map(([colour]) => colour);
    expect(colours).toEqual(["#fff"]);
  });

  it("names no token that does not exist, here or in what uses them", () => {
    const known = new Set([...sheet.matchAll(/(--kit-[a-z0-9-]+):/g)].map(([, name]) => name));
    for (const source of [sheet, gallery, uiScript]) {
      for (const [, name] of source.matchAll(/var\((--kit-[a-z0-9-]+)/g)) {
        expect(known, name).toContain(name);
      }
    }
  });

  it("depends on nothing: the font beside it, and the app's icons as data", () => {
    const urls = [...sheet.matchAll(/url\(([^)]*)\)/g)].map(([, url]) => url);
    for (const url of urls) {
      expect(url === "inter.woff2" || url.startsWith('"data:image/svg+xml,')).toBe(true);
    }
    expect(sheet).not.toContain("@import");
    expect(sheet.length).toBeLessThan(50_000);
    expect(uiScript).not.toMatch(/\bfetch\(|XMLHttpRequest|\bimport\(|\beval\(|new Function/);
    expect(uiScript.length).toBeLessThan(32_000);
  });

  it("holds still for whoever asked it to", () => {
    const calm = sheet.slice(sheet.indexOf("@media (prefers-reduced-motion: reduce)"));
    const animated = rules
      .filter((rule: { body: string }) => /\banimation:\s*kit-/.test(rule.body))
      .flatMap((rule: { selectors: string[] }) => rule.selectors);
    expect(animated).toContain(".kit-glyph--running");
    for (const selector of animated) expect(calm, selector).toContain(selector);
  });
});

describe("the tokens", () => {
  it("exist in both themes, the same ones, and whichever way the theme is chosen", () => {
    expect([...light.keys()].sort()).toEqual([...colorTokens].sort());
    expect([...darkForced.keys()].sort()).toEqual([...colorTokens].sort());
    expect([...darkBySystem]).toEqual([...darkForced]);
    for (const token of colorTokens) {
      expect(light.get(token), token).toBeTruthy();
      expect(darkForced.get(token), token).toBeTruthy();
    }
  });

  it("name what a kit is told to use", () => {
    const asked = ["--kit-bg", "--kit-surface", "--kit-text", "--kit-text-dim", "--kit-border", "--kit-accent", "--kit-ok", "--kit-warn", "--kit-danger"];
    for (const token of asked) expect(light.has(token), token).toBe(true);
    for (const token of ["--kit-space-8", "--kit-radius", "--kit-fs-body", "--kit-mono", "--kit-font", "--kit-scale"]) {
      expect(fixed.has(token), token).toBe(true);
    }
  });

  it("have a source in every theme, so the host can send them all", () => {
    for (const theme of THEMES) {
      const chrome = deriveChromeTokens(theme);
      for (const source of Object.values(tokenMap.colors)) {
        expect(chrome[source], `${theme.id} ${source}`).toBeTruthy();
      }
    }
  });
});

/** A colour as it reads over `on`: an rgba token is flattened onto it. */
function flat(colour: string, on: string): string {
  const rgba = colour.match(/^rgba\((\d+),\s*(\d+),\s*(\d+),\s*([\d.]+)\)$/);
  if (!rgba) return colour;
  const [, r, g, b, a] = rgba;
  return mix(on, toHex([Number(r), Number(g), Number(b)]), Number(a));
}

describe("contrast, by the rule of contrast.ts and themes.ts", () => {
  /** A token's values in a theme, as the sheet writes them or the host sends them. */
  const themeTokens = (id: string) => {
    const chrome = chromeTokens(id);
    return Object.fromEntries(
      Object.entries(tokenMap.colors).map(([token, source]) => [token, chrome[source]]),
    ) as Record<string, string>;
  };

  /** Every surface anything is written on, each flattened on the page. */
  const surfaces = (t: Record<string, string>) => {
    const bg = t["--kit-bg"];
    return {
      plain: [bg, t["--kit-surface"], t["--kit-raised"], t["--kit-float"], t["--kit-field"], t["--kit-well"]],
      underPointer: [t["--kit-hover"], t["--kit-selected"]],
    };
  };

  function textPairs(t: Record<string, string>): [string, string, string][] {
    const { plain, underPointer } = surfaces(t);
    const bg = t["--kit-bg"];
    const well = t["--kit-well"];
    const pairs: [string, string, string][] = [];
    const on = (token: string, background: string, where: string) =>
      pairs.push([`${token} on ${where}`, t[token], background]);
    for (const background of [...plain, ...underPointer]) {
      for (const token of ["--kit-text", "--kit-text-dim", "--kit-accent-text", "--kit-ok", "--kit-warn", "--kit-danger", "--kit-neutral"]) {
        on(token, background, background);
      }
    }
    // The quietest greys answer to the plain surfaces, as they do in Hangar.
    for (const background of plain) {
      for (const token of ["--kit-text-muted", "--kit-text-faint"]) on(token, background, background);
    }
    pairs.push(["the primary button's label", t["--kit-on-accent"], t["--kit-accent"]]);
    const chip = flat(t["--kit-line"], bg);
    on("--kit-text-dim", chip, "a chip");
    on("--kit-text-dim", chip, "a filter");
    on("--kit-danger", chip, "a filter's count");
    const pressed = flat(t["--kit-accent-soft"], bg);
    on("--kit-text", pressed, "a filter left on");
    on("--kit-text", chip, "a filter under the pointer");
    on("--kit-text-dim", flat(t["--kit-line"], t["--kit-well"]), "inline code in a block");
    on("--kit-text-dim", t["--kit-seg-track"], "a tab");
    on("--kit-text", t["--kit-seg-thumb"], "the chosen tab");
    on("--kit-text", mix(t["--kit-raised"], t["--kit-danger"], 0.08), "a danger button under the pointer");
    const errorBanner = mix(bg, t["--kit-danger"], 0.08);
    const warningBanner = mix(bg, t["--kit-warn-fill"], 0.08);
    on("--kit-text", errorBanner, "an error banner");
    on("--kit-danger", errorBanner, "an error banner's glyph");
    on("--kit-text", warningBanner, "a warning banner");
    on("--kit-warn", warningBanner, "a warning banner's glyph");
    const errorLine = mix(well, t["--kit-danger"], 0.11);
    const warningLine = mix(well, t["--kit-warn-fill"], 0.1);
    on("--kit-text", errorLine, "an error line");
    on("--kit-text-dim", errorLine, "an error line's number");
    on("--kit-text", warningLine, "a warning line");
    on("--kit-text-dim", warningLine, "a warning line's number");
    return pairs;
  }

  function fillPairs(t: Record<string, string>): [string, string, string][] {
    const { plain, underPointer } = surfaces(t);
    const pairs: [string, string, string][] = [];
    for (const background of [...plain, ...underPointer]) {
      for (const token of ["--kit-ok-fill", "--kit-warn-fill", "--kit-neutral-fill", "--kit-focus", "--kit-danger"]) {
        pairs.push([`${token} on ${background}`, t[token], background]);
      }
    }
    return pairs;
  }

  it("the sheet writes the default themes' values", () => {
    for (const [block, id] of [
      [light, tokenMap.themes.light],
      [darkForced, tokenMap.themes.dark],
    ] as const) {
      const expected = themeTokens(id);
      for (const token of colorTokens) expect(block.get(token), token).toBe(expected[token]);
    }
  });

  describe.each([
    ["the default dark theme", tokenMap.themes.dark],
    ["the default light theme", tokenMap.themes.light],
    ...THEMES.filter((theme) => !Object.values(tokenMap.themes).includes(theme.id)).map(
      (theme) => [theme.name, theme.id] as [string, string],
    ),
  ])("%s", (_name, id) => {
    it(`keeps text at ${TEXT_CONTRAST}:1`, () => {
      const failing = textPairs(themeTokens(id))
        .map(([what, text, background]) => ({ what, ratio: contrastRatio(text, background) }))
        .filter(({ ratio }) => ratio < TEXT_CONTRAST - 0.005);
      expect(failing).toEqual([]);
    });

    it(`keeps a state's shape and the focus ring at ${FILL_CONTRAST}:1`, () => {
      const failing = fillPairs(themeTokens(id))
        .map(([what, fill, background]) => ({ what, ratio: contrastRatio(fill, background) }))
        .filter(({ ratio }) => ratio < FILL_CONTRAST - 0.005);
      expect(failing).toEqual([]);
    });
  });
});

describe("the documentation", () => {
  const named = new Set([
    ...[...docs.matchAll(/class="([^"]*)"/g)].flatMap(([, value]) => value.split(/\s+/)),
    ...[...docs.matchAll(/`\.(kit-[a-z0-9_-]+)/g)].map(([, name]) => name),
  ].filter((name) => name.startsWith("kit-")));

  it("names no class kit.css does not have", () => {
    expect(named.size).toBeGreaterThan(30);
    const missing = [...named].filter((name) => !cssClasses.has(name));
    expect(missing).toEqual([]);
  });

  it("gives an example of every part of every component", () => {
    const script = classesIn(uiScript.replace(/["'`]/g, " .").replace(/\skit-/g, " .kit-"));
    const undocumented = [...cssClasses].filter((name) => !named.has(name) && !script.has(name));
    expect(undocumented).toEqual([]);
  });

  it("says since when each component exists", () => {
    const table = docs.slice(docs.indexOf("## Versions"));
    const versions = table.slice(0, table.indexOf("\n## ", 5));
    for (const block of new Set([...cssClasses].map(blockName))) {
      expect(versions, block).toMatch(
        new RegExp(`\\.${block}(?:__[a-z]+)?(?![a-z-])[^\\n]*\\|\\s*\\d+\\.\\d+\\.\\d+\\s*\\|`),
      );
    }
    const ui = loadSdk().ui;
    for (const name of Object.keys(ui)) {
      expect(versions, name).toMatch(new RegExp(`hangar\\.ui\\.${name}\\b[^\\n]*\\|\\s*\\d+\\.\\d+\\.\\d+\\s*\\|`));
    }
  });

  it("states the rules a kit follows", () => {
    for (const rule of ["glyph", "one primary action", "empty", "loading", "permission", "right-aligned", "tabular", "truncat", "French and English"]) {
      expect(docs.toLowerCase()).toContain(rule.toLowerCase());
    }
  });

  it("says what the host must send, with every token it must send", () => {
    expect(docs).toContain('"event": "theme"');
    expect(docs).toContain("tokens.json");
    expect(docs).toContain("--kit-scale");
  });
});

describe("the gallery", () => {
  it("shows every class of the sheet", () => {
    const shown = new Set(
      [...gallery.matchAll(/class="([^"]*)"/g)].flatMap(([, value]) => value.split(/\s+/)),
    );
    const script = classesIn(uiScript.replace(/["'`]/g, " .").replace(/\skit-/g, " .kit-"));
    const missing = [...cssClasses].filter((name) => !shown.has(name) && !script.has(name));
    expect(missing).toEqual([]);
  });

  it("is not something a kit can reach: it uses relative addresses only", () => {
    expect(gallery).not.toContain("/_hangar/");
  });
});

/** The two scripts in a stand-in for a tab's page: nothing here needs a real DOM. */
function loadSdk(options: { language?: string; withSdk?: boolean } = {}) {
  const handlers: ((event: unknown) => void)[] = [];
  const styles = new Map<string, string>();
  const attributes = new Map<string, string>();
  const errors: unknown[][] = [];
  const parent = { postMessage() {} };
  const documentElement = {
    style: {
      colorScheme: "",
      setProperty: (name: string, value: string) => void styles.set(name, value),
      removeProperty: (name: string) => void styles.delete(name),
    },
    setAttribute: (name: string, value: string) => void attributes.set(name, value),
    getAttribute: (name: string) => attributes.get(name) ?? null,
  };
  const page: Record<string, unknown> = {
    parent,
    addEventListener: (_type: string, handler: (event: unknown) => void) => handlers.push(handler),
  };
  const globals = {
    window: page,
    document: { documentElement },
    navigator: { language: options.language ?? "en-US" },
    console: { error: (...args: unknown[]) => errors.push(args) },
  };
  const run = (code: string) => new Function(...Object.keys(globals), code)(...Object.values(globals));
  if (options.withSdk !== false) run(sdkScript);
  run(uiScript);
  const hangar = page.hangar as {
    on: (name: string, handler: (data: unknown) => void) => () => void;
    ui: UiForTest;
    version: number;
  };
  return {
    hangar,
    ui: hangar?.ui,
    styles,
    attributes,
    errors,
    documentElement,
    send: (data: unknown, source: unknown = parent) =>
      handlers.forEach((handler) => handler({ source, data })),
    run,
  };
}

interface UiForTest {
  locale: () => string;
  i18n: (dictionaries: Record<string, Record<string, string>>) => (key: string, vars?: Record<string, unknown>) => string;
  isErrorLine: (text: string) => boolean;
  format: {
    duration: (ms: number) => string;
    clock: (at: number, options?: { seconds?: boolean }) => string;
    date: (at: number, now?: number) => string;
    since: (at: number, now?: number) => string;
    bytes: (size: number) => string;
    number: (value: number) => string;
  };
}

describe("kit.js, the theme", () => {
  const theme = (data: unknown) => ({ hangar: 1, event: "theme", data });

  it("sets nothing by itself: prefers-color-scheme decides until Hangar speaks", () => {
    const { styles, attributes } = loadSdk();
    expect(styles.size).toBe(0);
    expect(attributes.size).toBe(0);
  });

  it("applies the tokens and the scheme on the root element", () => {
    const { send, styles, attributes, documentElement } = loadSdk();
    send(theme({ scheme: "dark", tokens: { "--kit-bg": "#101010", "--kit-text": "#eeeeee" } }));
    expect(styles.get("--kit-bg")).toBe("#101010");
    expect(styles.get("--kit-text")).toBe("#eeeeee");
    expect(attributes.get("data-kit-scheme")).toBe("dark");
    expect(documentElement.style.colorScheme).toBe("dark");
  });

  it("takes back what the next theme leaves out, so nothing of the last one lingers", () => {
    const { send, styles } = loadSdk();
    send(theme({ scheme: "dark", tokens: { "--kit-bg": "#101010", "--kit-text": "#eeeeee" } }));
    send(theme({ scheme: "light", tokens: { "--kit-bg": "#fafafa" } }));
    expect(styles.get("--kit-bg")).toBe("#fafafa");
    expect(styles.has("--kit-text")).toBe(false);
  });

  it("listens to its parent only, and to nothing but kit tokens", () => {
    const { send, styles, attributes } = loadSdk();
    send(theme({ scheme: "dark", tokens: { "--kit-bg": "#101010" } }), { postMessage() {} });
    expect(styles.size).toBe(0);
    send(theme({ scheme: "sepia", tokens: { "--app-secret": "1", color: "red", "--kit-bg": 7 } }));
    expect(styles.size).toBe(0);
    expect(attributes.has("data-kit-scheme")).toBe(false);
    send(theme("nonsense"));
    send(theme(null));
    expect(styles.size).toBe(0);
  });

  it("still tells the page's own listeners", () => {
    const { send, hangar } = loadSdk();
    const heard: unknown[] = [];
    hangar.on("theme", (data) => heard.push(data));
    send(theme({ scheme: "light", tokens: {} }));
    expect(heard).toEqual([{ scheme: "light", tokens: {} }]);
  });

  it("carries Hangar's language to the library", () => {
    const { send, ui } = loadSdk({ language: "en-US" });
    expect(ui.locale()).toBe("en");
    send(theme({ scheme: "dark", tokens: {}, locale: "fr" }));
    expect(ui.locale()).toBe("fr");
    send(theme({ scheme: "dark", tokens: {}, locale: "<script>" }));
    expect(ui.locale()).toBe("fr");
  });
});

describe("kit-ui.js", () => {
  it("fills window.hangar.ui once, on an object that stays frozen", () => {
    const { hangar, ui, run } = loadSdk();
    expect(ui).toBeTruthy();
    run("window.hangar.ui = {}; ");
    expect(hangar.ui).toBe(ui);
    expect(Object.isFrozen(hangar)).toBe(true);
    expect(() => {
      "use strict";
      (hangar as unknown as Record<string, unknown>).call = null;
    }).toThrow();
    expect(hangar.version).toBe(1);
  });

  it("says so when it is loaded before the SDK", () => {
    const { errors, hangar } = loadSdk({ withSdk: false });
    expect(hangar).toBeUndefined();
    expect(String(errors[0][0])).toContain("/_hangar/kit.js");
  });

  it("sets text as lib/typography does", () => {
    const t = (lang: string, template: string) =>
      loadSdk({ language: lang }).ui.i18n({ [lang]: { key: template } })("key");
    for (const template of ["Terminé : 12 min, 1 h 05 ?", "« Oui » à 100 %", "< 1 min", "{n} sessions", "5 s", "voilà ; fini !", "3 ms 4"]) {
      expect(t("fr", template)).toBe(typeset(template, "fr"));
      expect(t("en", template)).toBe(typeset(template, "en"));
    }
  });

  it("picks the language, English filling in what is missing, and the plural", () => {
    const dictionaries = {
      en: { hello: "Hello", only: "Only here", files_one: "{n} file", files_other: "{n} files", named: "Hello {who}" },
      fr: { hello: "Bonjour", files_one: "{n} fichier", files_other: "{n} fichiers" },
    };
    const fr = loadSdk({ language: "fr-FR" }).ui.i18n(dictionaries);
    expect(fr("hello")).toBe("Bonjour");
    expect(fr("only")).toBe("Only here");
    expect(fr("files", { n: 0 })).toBe("0 fichier");
    expect(fr("files", { n: 2 })).toBe("2 fichiers");
    expect(fr("nothing")).toBe("nothing");
    const en = loadSdk({ language: "en-GB" }).ui.i18n(dictionaries);
    expect(en("files", { n: 1 })).toBe("1 file");
    expect(en("files", { n: 0 })).toBe("0 files");
    expect(en("named", { who: "<b>Ana {n}</b>" })).toBe("Hello <b>Ana {n}</b>");
    expect(en("named")).toBe("Hello {who}");
  });

  it("falls back to English in a language it does not know", () => {
    const de = loadSdk({ language: "de-DE" }).ui.i18n({ en: { hello: "Hello" } });
    expect(de("hello")).toBe("Hello");
  });

  describe.each([
    ["fr-FR", ["900 ms", "2,3 s", "59 s", "1 min 28 s", "10 min", "2 h 05 min", "3 j 4 h"]],
    ["en-US", ["900 ms", "2.3 s", "59 s", "1 min 28 s", "10 min", "2 h 05 min", "3 d 4 h"]],
  ])("formats, in %s", (language, durations) => {
    const { format } = loadSdk({ language }).ui;
    const french = language === "fr-FR";
    const now = new Date(2026, 9, 31, 21, 38, 12).getTime();

    it("writes a duration as Hangar does, its parts never split", () => {
      expect([900, 2300, 59000, 88000, 600000, 7500000, 273600000].map(format.duration)).toEqual(durations);
      expect(format.duration(-1)).toBe("");
      expect(format.duration(Number.NaN)).toBe("");
    });

    it("writes a time in a column on the 24 hours", () => {
      expect(format.clock(now)).toBe("21:38");
      expect(format.clock(now, { seconds: true })).toBe("21:38:12");
      expect(format.clock(new Date(2026, 0, 1, 0, 5).getTime())).toBe("00:05");
    });

    it("gives a moment the year only when it is not this one", () => {
      expect(format.date(now, now)).not.toContain("2026");
      expect(format.date(new Date(2025, 2, 4).getTime(), now)).toContain("2025");
    });

    it("says how long ago as the inspector does", () => {
      expect(format.since(now - 5000, now)).toBe(french ? "à l'instant" : "just now");
      expect(format.since(now - 300000, now)).toBe(french ? "il y a 5 min" : "5 min ago");
      expect(format.since(now - 7200000, now)).toContain(french ? "aujourd'hui à 19 h 38" : "today at");
      expect(format.since(now - 86400000, now)).toContain(french ? "hier à 21 h 38" : "yesterday at");
      expect(format.since(now - 86400000 * 5, now)).toContain(french ? "le 26" : "at");
    });

    it("writes a size in the language's own unit", () => {
      expect(format.bytes(512)).toBe(french ? "512 o" : "512 B");
      expect(format.bytes(15360)).toBe(french ? "15 Ko" : "15 KB");
      expect(format.bytes(1572864)).toBe(french ? "1,5 Mo" : "1.5 MB");
    });
  });

  it("finds what reads as an error in a log, and what only mentions one", () => {
    const { isErrorLine } = loadSdk().ui;
    for (const line of ["error: could not compile", "error[E0599]: no method", "test x ... FAILED", "thread 'main' panicked at src/a.rs", "FATAL: out of memory", "Erreur : introuvable", "1 failed; 3 passed"]) {
      expect(isErrorLine(line), line).toBe(true);
    }
    for (const line of ["test a_refusing_error ... ok", "0 errors", "no errors found", "Compiling errors-kit v1.0", "sans erreur", "test handles_failure_mode ... ok", "errors: 0"]) {
      expect(isErrorLine(line), line).toBe(false);
    }
  });
});
