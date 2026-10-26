import { describe, expect, it } from "vitest";
import {
  contrastRatio,
  ensureContrast,
  farthestPole,
  hueAndSaturation,
  hueDistance,
  mix,
  relativeLuminance,
  withHue,
  worstContrast,
} from "./contrast";

describe("relativeLuminance", () => {
  it("runs from 0 for black to 1 for white", () => {
    expect(relativeLuminance("#000000")).toBe(0);
    expect(relativeLuminance("#ffffff")).toBeCloseTo(1, 10);
  });

  it("weighs green far above blue, as the eye does", () => {
    expect(relativeLuminance("#00ff00")).toBeGreaterThan(relativeLuminance("#0000ff") * 9);
  });
});

describe("contrastRatio", () => {
  it("runs from 1 to 21 whichever colour comes first", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 5);
    expect(contrastRatio("#777777", "#777777")).toBe(1);
  });

  it("matches the published WCAG figures", () => {
    // #767676 is the classic lightest grey that passes AA on white.
    expect(contrastRatio("#767676", "#ffffff")).toBeCloseTo(4.54, 2);
    expect(contrastRatio("#777777", "#ffffff")).toBeLessThan(4.5);
  });
});

describe("worstContrast and farthestPole", () => {
  it("measures a colour on the least favourable surface", () => {
    const ratio = worstContrast("#ffffff", ["#000000", "#555555"]);
    expect(ratio).toBeCloseTo(contrastRatio("#ffffff", "#555555"), 10);
  });

  it("goes to white from dark surfaces and to black from light ones", () => {
    expect(farthestPole(["#121316", "#232428"])).toBe("#ffffff");
    expect(farthestPole(["#ffffff", "#ededed"])).toBe("#000000");
  });
});

describe("ensureContrast", () => {
  it("leaves a colour that already reads as it is", () => {
    expect(ensureContrast("#c8c9ce", "#121316", 4.5)).toBe("#c8c9ce");
  });

  it("lightens on a dark surface, just enough", () => {
    const color = ensureContrast("#555555", "#222222", 4.5);
    expect(contrastRatio(color, "#222222")).toBeGreaterThanOrEqual(4.5);
    expect(relativeLuminance(color)).toBeGreaterThan(relativeLuminance("#555555"));
    // The least mix that reads: half a step less does not.
    const lighter = relativeLuminance(color);
    expect(lighter).toBeLessThan(relativeLuminance(mix("#555555", "#ffffff", 0.6)));
  });

  it("darkens on a light surface", () => {
    const color = ensureContrast("#f2ae49", "#ececec", 4.5);
    expect(contrastRatio(color, "#ececec")).toBeGreaterThanOrEqual(4.5);
    expect(relativeLuminance(color)).toBeLessThan(relativeLuminance("#f2ae49"));
  });

  it("keeps the hue while it moves", () => {
    const color = ensureContrast("#f2ae49", "#ececec", 4.5);
    expect(hueDistance(hueAndSaturation(color).hue, hueAndSaturation("#f2ae49").hue)).toBeLessThan(2);
  });

  it("reads on every surface it is given", () => {
    const surfaces = ["#1b1c1f", "#202124", "#232428"];
    const color = ensureContrast("#6d6e72", surfaces, 4.5);
    for (const surface of surfaces) {
      expect(contrastRatio(color, surface)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("leans toward the colour it is given instead of white or black", () => {
    const color = ensureContrast("#3a4060", "#1a1b26", 4.5, "#c0caf5");
    expect(contrastRatio(color, "#1a1b26")).toBeGreaterThanOrEqual(4.5);
    // On the way to the lavender foreground, blue stays the strongest channel.
    const [r, , b] = [color.slice(1, 3), color.slice(3, 5), color.slice(5, 7)].map((h) =>
      parseInt(h, 16),
    );
    expect(b).toBeGreaterThan(r);
  });

  it("falls back on the target when nothing short of it reads", () => {
    expect(ensureContrast("#808080", "#808080", 4.5, "#999999")).toBe("#999999");
  });
});

describe("hues", () => {
  it("reads the hue and saturation of a colour", () => {
    expect(hueAndSaturation("#ff0000")).toEqual({ hue: 0, saturation: 1 });
    expect(hueAndSaturation("#00ff00").hue).toBe(120);
    expect(hueAndSaturation("#0000ff").hue).toBe(240);
    expect(hueAndSaturation("#777777").saturation).toBe(0);
  });

  it("measures the short way round the wheel", () => {
    expect(hueDistance(350, 10)).toBe(20);
    expect(hueDistance(10, 350)).toBe(20);
    expect(hueDistance(0, 180)).toBe(180);
  });

  it("turns a colour to a hue and keeps the rest", () => {
    expect(withHue("#ff0000", 120)).toBe("#00ff00");
    const turned = withHue("#6fd3df", 200);
    expect(hueAndSaturation(turned).hue).toBeCloseTo(200, 0);
    expect(hueAndSaturation(turned).saturation).toBeCloseTo(hueAndSaturation("#6fd3df").saturation, 1);
  });
});
