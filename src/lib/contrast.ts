/**
 * Colour arithmetic for the chrome's tokens (themes.ts): mixing, WCAG
 * contrast, and the hue of a colour. Pure functions on `#rrggbb` strings, so
 * every theme's tokens can be derived — and checked — without a DOM.
 */

type Rgb = [number, number, number];

const WHITE = "#ffffff";
const BLACK = "#000000";

export const toRgb = (hex: string): Rgb => {
  const clean = hex.replace("#", "");
  const full =
    clean.length === 3
      ? clean
          .split("")
          .map((c) => c + c)
          .join("")
      : clean;
  return [
    parseInt(full.slice(0, 2), 16),
    parseInt(full.slice(2, 4), 16),
    parseInt(full.slice(4, 6), 16),
  ];
};

export const toHex = ([r, g, b]: Rgb) =>
  "#" +
  [r, g, b]
    .map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0"))
    .join("");

/** Blends two colors; `amount` is how much of `b` to mix in. */
export const mix = (a: string, b: string, amount: number): string => {
  const [r1, g1, b1] = toRgb(a);
  const [r2, g2, b2] = toRgb(b);
  return toHex([
    r1 + (r2 - r1) * amount,
    g1 + (g2 - g1) * amount,
    b1 + (b2 - b1) * amount,
  ]);
};

/** Relative luminance, as WCAG 2 defines it: 0 for black, 1 for white. */
export const relativeLuminance = (hex: string): number => {
  const [r, g, b] = toRgb(hex).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

/**
 * Lightness as the eye reads it, from 0 for black to 1 for white: OKLab's L.
 * WCAG's luminance answers "can it be read"; this answers "do two steps of a
 * ramp look apart", which luminance gets wrong in the darks.
 */
export const perceivedLightness = (hex: string): number => {
  const [r, g, b] = toRgb(hex).map((v) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
};

/** WCAG contrast ratio between two colours, from 1 to 21. */
export const contrastRatio = (a: string, b: string): number => {
  const [hi, lo] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

/** The contrast `color` keeps on the least favourable of `surfaces`. */
export const worstContrast = (color: string, surfaces: readonly string[]): number =>
  Math.min(...surfaces.map((surface) => contrastRatio(color, surface)));

/** White or black, whichever stands further from every one of `surfaces`. */
export const farthestPole = (surfaces: readonly string[]): string =>
  worstContrast(WHITE, surfaces) >= worstContrast(BLACK, surfaces) ? WHITE : BLACK;

/** Steps from `color` to `toward`: fine enough that no token moves further than it must. */
const STEPS = 200;

/**
 * `color` itself when it reads at `min`:1 on every one of `against`, or else
 * the least mix of it toward `toward` that does. `toward` is white on dark
 * surfaces and black on light ones unless given; a caller passes the theme's
 * foreground, say, to keep a grey's tint. When nothing short of `toward`
 * reads, `toward` is the best there is and comes back as it is.
 */
export function ensureContrast(
  color: string,
  against: string | readonly string[],
  min: number,
  toward?: string,
): string {
  const surfaces = typeof against === "string" ? [against] : against;
  const target = toward ?? farthestPole(surfaces);
  if (worstContrast(color, surfaces) >= min) return color;
  for (let step = 1; step < STEPS; step++) {
    const candidate = mix(color, target, step / STEPS);
    if (worstContrast(candidate, surfaces) >= min) return candidate;
  }
  return target;
}

/** Hue (0–360, degrees) and HSL saturation (0–1) of a colour. */
export function hueAndSaturation(hex: string): { hue: number; saturation: number } {
  const [r, g, b] = toRgb(hex).map((v) => v / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const chroma = max - min;
  if (chroma === 0) return { hue: 0, saturation: 0 };
  const sector =
    max === r ? ((g - b) / chroma + 6) % 6 : max === g ? (b - r) / chroma + 2 : (r - g) / chroma + 4;
  const lightness = (max + min) / 2;
  return { hue: sector * 60, saturation: chroma / (1 - Math.abs(2 * lightness - 1)) };
}

/** How far apart two hues sit on the colour wheel, from 0 to 180 degrees. */
export const hueDistance = (a: number, b: number): number => {
  const d = Math.abs(a - b) % 360;
  return d > 180 ? 360 - d : d;
};

/** `hex` turned to `hue`, its saturation and lightness kept. */
export function withHue(hex: string, hue: number): string {
  const [r, g, b] = toRgb(hex).map((v) => v / 255);
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const lightness = (max + min) / 2;
  const chroma = max - min;
  const h = (((hue % 360) + 360) % 360) / 60;
  const x = chroma * (1 - Math.abs((h % 2) - 1));
  const [r1, g1, b1] =
    h < 1
      ? [chroma, x, 0]
      : h < 2
        ? [x, chroma, 0]
        : h < 3
          ? [0, chroma, x]
          : h < 4
            ? [0, x, chroma]
            : h < 5
              ? [x, 0, chroma]
              : [chroma, 0, x];
  const m = lightness - chroma / 2;
  return toHex([(r1 + m) * 255, (g1 + m) * 255, (b1 + m) * 255]);
}
