import { mix } from "../contrast";
import type { ViewerTheme } from "./engine";

/**
 * The viewport's colours, taken from the chrome tokens the active theme set
 * on the document (themes.ts › deriveChromeTokens), so the floor, the axes
 * and the wireframe sit in the same palette as the panel around them.
 *
 * Main bundle: no three.js here, only the shape the engine takes.
 */

const HEX = /^#[0-9a-f]{6}$/i;

function token(style: CSSStyleDeclaration, name: string, fallback: string): string {
  const value = style.getPropertyValue(name).trim();
  return HEX.test(value) ? value : fallback;
}

export function readViewerTheme(root: HTMLElement = document.documentElement): ViewerTheme {
  const style = getComputedStyle(root);
  const dark = (root.style.colorScheme || style.colorScheme) !== "light";
  const bg = token(style, "--bg", dark ? "#1b1c1f" : "#ffffff");
  const fg = token(style, "--fg", dark ? "#e6e6e6" : "#1f2328");
  const accent = token(style, "--accent", "#4f8cff");
  return {
    dark,
    background: bg,
    // Lines a little off the background, the major ones a step further:
    // present, never louder than the model.
    gridMinor: mix(bg, fg, dark ? 0.1 : 0.09),
    gridMajor: mix(bg, fg, dark ? 0.2 : 0.18),
    // Red, green and blue for X, Y and Z, as in every DCC tool: the theme's
    // own danger, success and accent, which are those hues on every theme.
    axisX: token(style, "--danger", "#e5484d"),
    axisY: token(style, "--success", "#46a758"),
    axisZ: accent,
    wire: mix(fg, bg, dark ? 0.1 : 0.25),
    normals: accent,
  };
}
