import { describe, expect, it } from "vitest";
import { THEMES, contrastRatio, getTheme, legibleOn, mix, themeVars } from "./themes";

describe("contrastRatio", () => {
  it("runs from 1 to 21 whichever colour comes first", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 5);
    expect(contrastRatio("#777777", "#777777")).toBe(1);
  });
});

describe("legibleOn", () => {
  it("keeps the first step when it already reads", () => {
    const first = mix("#000000", "#ffffff", 0.76);
    expect(legibleOn(["#000000"], "#000000", "#ffffff", 0.76)).toBe(first);
  });

  it("leans further toward the target until it reads at 4.5:1", () => {
    const color = legibleOn(["#555555"], "#555555", "#ffffff", 0.1);
    expect(contrastRatio(color, "#555555")).toBeGreaterThanOrEqual(4.5);
    expect(color).not.toBe("#ffffff");
  });

  it("falls back on the target when nothing short of it reads", () => {
    expect(legibleOn(["#808080"], "#808080", "#999999", 0.5)).toBe("#999999");
  });
});

describe("themeVars", () => {
  const surfaces = (vars: Record<string, string>) => [
    vars["--chrome"],
    vars["--chrome-bar"],
    vars["--raised"],
  ];

  it("keeps the colours the stylesheets mixed for Graphite and GitHub Light", () => {
    for (const id of ["graphite", "github-light"]) {
      const theme = getTheme(id);
      const vars = themeVars(theme);
      const fg = theme.xterm.foreground as string;
      const bg = theme.xterm.background as string;
      expect(vars["--fg-secondary"]).toBe(mix(fg, bg, 0.24));
      expect(vars["--state-yours-text"]).toBe(mix(vars["--accent-hi"], fg, 0.22));
    }
  });

  it("gives secondary text and the 'your turn' word 4.5:1 on every chrome surface", () => {
    for (const theme of THEMES) {
      const vars = themeVars(theme);
      const fg = theme.xterm.foreground as string;
      for (const token of ["--fg-secondary", "--state-yours-text"]) {
        const color = vars[token];
        // Only a theme whose own foreground falls short gets the foreground.
        const reads = surfaces(vars).every((surface) => contrastRatio(color, surface) >= 4.5);
        expect(reads || color === fg, `${theme.id} ${token}`).toBe(true);
      }
    }
  });

  it("puts a card on the page the way the appearance wants it", () => {
    const dark = themeVars(getTheme("graphite"));
    expect(dark["--card"]).toBe(dark["--raised"]);
    const light = themeVars(getTheme("github-light"));
    expect(light["--card"]).toBe(light["--bg"]);
  });
});
