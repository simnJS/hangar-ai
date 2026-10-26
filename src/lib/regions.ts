import { focusablesIn } from "./focusable";

/**
 * The window's regions, in the order F6 walks them, which is the order they
 * are read in: the navigator down the left, the toolbar, the panes (or the
 * board, memory or statistics view in their place), the 3D preview, the
 * board panel and the inspector on the right, and the status bar along the
 * bottom.
 *
 * Tab cannot do this: a terminal keeps Tab for the shell, so without F6 the
 * keyboard has no way out of a pane and into the panels around it.
 */
export type Region = "navigator" | "toolbar" | "main" | "model" | "dock" | "inspector" | "status";

export const REGION_ORDER: Region[] = [
  "navigator",
  "toolbar",
  "main",
  "model",
  "dock",
  "inspector",
  "status",
];

/**
 * The region after (`step` 1) or before (-1) `current` among those on
 * screen, wrapping around. From outside every region, F6 starts at the first
 * and Shift+F6 at the last.
 */
export function nextRegion(available: Region[], current: Region | null, step: 1 | -1): Region | null {
  const regions = REGION_ORDER.filter((region) => available.includes(region));
  if (!regions.length) return null;
  const at = current ? regions.indexOf(current) : -1;
  if (at < 0) return step > 0 ? regions[0] : regions[regions.length - 1];
  return regions[(at + step + regions.length) % regions.length];
}

/** The element each region is drawn in. */
const ROOTS: Record<Region, string> = {
  navigator: ".sidebar",
  toolbar: ".toolbar",
  main: ".workarea",
  model: ".model-panel",
  dock: ".board-dock",
  inspector: ".inspector__panel",
  status: ".status",
};

/** The 3D preview and the board panel sit inside the work area, so they are asked first. */
const LOOKUP: Region[] = ["model", "dock", "navigator", "toolbar", "inspector", "status", "main"];

/** The region an element belongs to, if any. */
export function regionOf(element: Element | null): Region | null {
  if (!element) return null;
  return LOOKUP.find((region) => element.closest(ROOTS[region])) ?? null;
}

/**
 * Puts the keyboard in a region: on its one control that stands for it where
 * there is one — the navigator's current row, the broadcast field — and on
 * the panel itself for the 3D preview, the board panel and the inspector,
 * which a screen reader then names, and from which Tab walks in. The panes
 * are App's, which knows which terminal is the focused one. Returns false
 * when the region is not on screen.
 */
export function focusRegion(region: Region): boolean {
  const root = document.querySelector<HTMLElement>(ROOTS[region]);
  if (!root || !root.getClientRects().length) return false;
  let target: HTMLElement | null = null;
  if (region === "navigator") {
    target = root.querySelector<HTMLElement>('[role="tree"] [tabindex="0"]');
  } else if (region === "status") {
    target = root.querySelector<HTMLElement>("input:not([disabled])");
  } else if (region === "model" || region === "dock" || region === "inspector") {
    target = root;
  }
  target ??= focusablesIn(root)[0] ?? null;
  if (!target) return false;
  target.focus({ preventScroll: true });
  return true;
}
