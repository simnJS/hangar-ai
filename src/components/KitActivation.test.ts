import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
// @ts-expect-error A plain script: no types to have.
import * as generator from "../../scripts/gen-hangar-ui.mjs";
import type { KitInfo } from "../lib/kits";

// The icon is drawn from its props; the store and Tauri are not needed.
vi.mock("../store", () => ({ useStore: () => ({}) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const { KitIcon } = await import("./KitActivation");

// Vitest empties a `.css` import, `?raw` or not.
const sheet: string = generator.readText("src/components/KitActivation.css");

/** The declarations of one rule of the sheet. */
function rule(selector: string): string {
  const start = sheet.indexOf(`\n${selector} {`);
  if (start < 0) throw new Error(`no rule ${selector}`);
  return sheet.slice(start, sheet.indexOf("}", start));
}

const kit = (icon?: string): KitInfo => ({
  id: "guard",
  source: { kind: "folder", path: "C:\\kits\\guard" },
  dir: "C:\\kits\\guard",
  installedVersion: "0.1.0",
  addedAt: 0,
  error: null,
  update: null,
  manifest: { schema: 1, id: "guard", name: "Guardrails", version: "0.1.0", description: "", icon, tabs: [], setup: [] },
});

const draw = (icon?: string) => renderToStaticMarkup(createElement(KitIcon, { kit: kit(icon) }));

describe("KitIcon", () => {
  // Found in WebView2: `stroke="currentColor"` in an `<img>` is black, and every
  // kit's icon disappeared on a dark theme, on a tile of rgb(12, 12, 14).
  it("draws an SVG as a mask, which takes the colour around it", () => {
    const html = draw("icon.svg");
    expect(html).toContain("kit-icon--glyph");
    expect(html).toContain("mask-image:url(");
    expect(html).toContain("/guard/icon.svg");
    expect(rule(".kit-icon--glyph")).toContain("background-color: currentColor");
  });

  it("still loads the SVG in an image, to hear that it failed", () => {
    expect(draw("icon.svg")).toMatch(/<img [^>]*hidden/);
  });

  it("leaves a PNG alone: it has colours of its own", () => {
    const html = draw("icon.png");
    expect(html).not.toContain("kit-icon--glyph");
    expect(html).toMatch(/<img [^>]*class="kit-icon"/);
  });

  it("falls back on the plug without an icon", () => {
    const html = draw();
    expect(html).not.toContain("<img");
    expect(html).toContain("<svg");
  });
});

describe("the activation view's width", () => {
  // Found in WebView2 on Guardrails: one hook's matcher, `Bash|PowerShell|…`, has
  // nowhere to break, and pushed the whole view past the dialog, its description
  // cut off on the right and a scrollbar under everything.
  it("breaks a hook's matcher anywhere", () => {
    expect(rule(".kit-step__title")).toContain("overflow-wrap: anywhere");
  });

  it("holds its column to the dialog's width", () => {
    expect(rule(".kit-act")).toContain("grid-template-columns: minmax(0, 1fr)");
  });
});
