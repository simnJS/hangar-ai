import { describe, expect, it } from "vitest";
import { contrastRatio, hueAndSaturation, hueDistance, perceivedLightness } from "./lib/contrast";
import {
  FILL_CONTRAST,
  TEXT_CONTRAST,
  THEMES,
  deriveChromeTokens,
  getTheme,
  mix,
} from "./themes";

/** The surfaces the chrome writes on; every token is held to all of them. */
const SURFACES = ["--chrome", "--chrome-bar", "--raised", "--pane-head"];

/** Tokens that colour text: 4.5:1 on every surface. */
const TEXT_TOKENS = [
  "--fg",
  "--fg-secondary",
  "--fg-muted",
  "--fg-subtle",
  "--accent-hi",
  "--state-working",
  "--state-waiting",
  "--state-yours",
  "--state-idle",
  "--danger",
  "--success",
  "--warning",
];

/**
 * Everything that can also be written on a selected row, a chip or the
 * toolbar's capsule, all of them a step further out, at --surface-active.
 * The two quietest greys are kept off those.
 */
const SELECTION_TEXT_TOKENS = TEXT_TOKENS.filter(
  (token) => token !== "--fg-muted" && token !== "--fg-subtle",
);

/** A state's glyph, a bar, a frame: 3:1 on every surface. */
const FILL_TOKENS = [
  "--state-working-fill",
  "--state-waiting-fill",
  "--state-yours-fill",
  "--state-idle-fill",
];

const hue = (color: string) => hueAndSaturation(color).hue;

/** `rgba(r, g, b, a)` laid over an opaque `#rrggbb`, as the page composites it. */
function over(rgba: string, base: string): string {
  const [r, g, b, a] = (rgba.match(/[\d.]+/g) ?? []).map(Number);
  const top = "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
  return mix(base, top, a);
}

/** The fills a label or state word can sit on beyond the plain surfaces. */
const fills = (vars: Record<string, string>) => [
  vars["--surface-active"],
  // A hairline's tint over a raised group: a chip, the toolbar's capsule.
  over(vars["--line"], vars["--raised"]),
];

describe("deriveChromeTokens, on every bundled theme", () => {
  it("covers every theme", () => {
    expect(THEMES.length).toBeGreaterThanOrEqual(33);
  });

  describe.each(THEMES.map((theme) => [theme.id, theme] as const))("%s", (_id, theme) => {
    const vars = deriveChromeTokens(theme);

    it("writes every text token at 4.5:1 on every surface", () => {
      for (const token of TEXT_TOKENS) {
        for (const surface of SURFACES) {
          const ratio = contrastRatio(vars[token], vars[surface]);
          expect(ratio, `${token} on ${surface}`).toBeGreaterThanOrEqual(TEXT_CONTRAST);
        }
      }
    });

    it("writes labels, state words and links at 4.5:1 on the selection and on chips", () => {
      for (const token of SELECTION_TEXT_TOKENS) {
        for (const fill of fills(vars)) {
          const ratio = contrastRatio(vars[token], fill);
          expect(ratio, `${token} on ${fill}`).toBeGreaterThanOrEqual(TEXT_CONTRAST);
        }
      }
    });

    it("draws every state fill at 3:1 on every surface and fill", () => {
      for (const token of FILL_TOKENS) {
        for (const surface of [...SURFACES.map((name) => vars[name]), ...fills(vars)]) {
          const ratio = contrastRatio(vars[token], surface);
          expect(ratio, `${token} on ${surface}`).toBeGreaterThanOrEqual(FILL_CONTRAST);
        }
      }
    });

    it("keeps the chosen segment's label at 4.5:1", () => {
      expect(contrastRatio(vars["--fg"], vars["--seg-thumb"])).toBeGreaterThanOrEqual(
        TEXT_CONTRAST,
      );
    });

    it("writes on the accent at 4.5:1", () => {
      expect(contrastRatio(vars["--accent-fg"], vars["--accent"])).toBeGreaterThanOrEqual(
        TEXT_CONTRAST,
      );
    });

    it("gives 'your turn' a hue apart from the accent and from working", () => {
      // 30 degrees, give or take the rounding to 8-bit channels.
      const yours = hue(vars["--state-yours-fill"]);
      expect(hueDistance(yours, hue(theme.accent))).toBeGreaterThanOrEqual(29);
      expect(hueDistance(yours, hue(vars["--state-working-fill"]))).toBeGreaterThanOrEqual(29);
    });
  });
});

describe("deriveChromeTokens, on the themes the app is designed in", () => {
  it("keeps Graphite's own state colours where they already read", () => {
    const vars = deriveChromeTokens(getTheme("graphite"));
    expect(vars["--state-working"]).toBe("#5fd27f");
    expect(vars["--state-working-fill"]).toBe("#5fd27f");
    expect(vars["--state-waiting"]).toBe("#e7c06b");
    // Graphite's cyan, not its blue accent.
    expect(hueDistance(hue(vars["--state-yours"]), hue("#6fd3df"))).toBeLessThan(2);
  });

  it("keeps the secondary text the stylesheets mixed for Graphite and GitHub Light", () => {
    for (const id of ["graphite", "github-light"]) {
      const theme = getTheme(id);
      const vars = deriveChromeTokens(theme);
      const fg = theme.xterm.foreground as string;
      const bg = theme.xterm.background as string;
      expect(vars["--fg"]).toBe(fg);
      expect(vars["--fg-secondary"]).toBe(mix(fg, bg, 0.24));
    }
  });

  it("keeps the three secondary steps in order, brightest first", () => {
    const vars = deriveChromeTokens(getTheme("graphite"));
    const on = (token: string) => contrastRatio(vars[token], vars["--raised"]);
    expect(on("--fg")).toBeGreaterThan(on("--fg-secondary"));
    expect(on("--fg-secondary")).toBeGreaterThan(on("--fg-muted"));
    expect(on("--fg-muted")).toBeGreaterThanOrEqual(on("--fg-subtle"));
  });

  it("darkens a light theme's state colours into words, and keeps them vivid as fills", () => {
    const vars = deriveChromeTokens(getTheme("ayu-light"));
    const theme = getTheme("ayu-light");
    expect(vars["--state-working-fill"]).not.toBe(vars["--state-working"]);
    expect(contrastRatio(vars["--state-working"], "#ffffff")).toBeGreaterThan(
      contrastRatio(theme.xterm.green as string, "#ffffff"),
    );
  });

  it("puts a card on the page the way the appearance wants it", () => {
    const dark = deriveChromeTokens(getTheme("graphite"));
    expect(dark["--card"]).toBe(dark["--raised"]);
    const light = deriveChromeTokens(getTheme("github-light"));
    expect(light["--card"]).toBe(light["--bg"]);
  });
});

describe("the statistics' colours", () => {
  it("step away from the card in order on every theme, the faintest still at 2:1", () => {
    for (const theme of THEMES) {
      const vars = deriveChromeTokens(theme);
      const card = vars["--card"];
      const steps = ["--viz-1", "--viz-2", "--viz-3", "--viz-4"].map((token) => vars[token]);
      const away = steps.map((color) =>
        Math.abs(perceivedLightness(color) - perceivedLightness(card)),
      );
      // Far enough apart to tell neighbours in a stacked column at a glance.
      for (let step = 1; step < steps.length; step++) {
        expect(away[step] - away[step - 1], `${theme.id} step ${step}`).toBeGreaterThanOrEqual(0.06);
      }
      expect(contrastRatio(steps[0], card), theme.id).toBeGreaterThanOrEqual(2);
      expect(contrastRatio(steps[3], card), theme.id).toBeGreaterThanOrEqual(3);
    }
  });

  it("darken or brighten the heatmap with the value, from an empty cell up", () => {
    for (const theme of THEMES) {
      const vars = deriveChromeTokens(theme);
      const card = vars["--card"];
      const away = [1, 2, 3, 4, 5].map((level) =>
        Math.abs(perceivedLightness(vars[`--viz-heat-${level}`]) - perceivedLightness(card)),
      );
      for (let level = 1; level < away.length; level++) {
        expect(away[level], `${theme.id} level ${level + 1}`).toBeGreaterThan(away[level - 1]);
      }
    }
  });
});
