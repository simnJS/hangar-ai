import { describe, expect, it, vi } from "vitest";
import source from "./i18n.ts?raw";

// The hooks reach for the app's store, which talks to Tauri; the dictionaries
// and translate() need neither.
vi.mock("./store", () => ({ useStore: () => ({}) }));

const { DICTIONARIES, translate } = await import("./i18n");

const NNBSP = " ";

/** Every French string that breaks `rule`, as "key: text", for a readable failure. */
function offenders(rule: (text: string) => boolean): string[] {
  return Object.entries(DICTIONARIES.fr)
    .filter(([, text]) => rule(text))
    .map(([key, text]) => `${key}: ${JSON.stringify(text)}`);
}

describe("French typography", () => {
  it("has a narrow no-break space before ? ! ; : » and %", () => {
    expect(offenders((text) => /[^ ][?!;:»%]/.test(text))).toEqual([]);
  });

  it("has a narrow no-break space after «", () => {
    expect(offenders((text) => /«[^ ]/.test(text))).toEqual([]);
  });

  it("closes every guillemet it opens", () => {
    const count = (text: string, mark: string) => text.split(mark).length - 1;
    expect(offenders((text) => count(text, "«") !== count(text, "»"))).toEqual([]);
  });

  it("quotes with guillemets, never with English quotes", () => {
    expect(offenders((text) => /["“”]/.test(text))).toEqual([]);
  });

  it("writes an ellipsis as one character", () => {
    expect(offenders((text) => text.includes("..."))).toEqual([]);
  });

  it("leaves the no-break spaces to the typesetter", () => {
    // Written by hand, they drift: some strings had them and most did not.
    const french = source.slice(source.indexOf("const fr: Record<Key, string> = {"));
    expect(french).not.toMatch(/[  ]|\\u00a0|\\u202f/);
  });

  it("never touches what fills a placeholder", () => {
    expect(translate("fr", "palette.empty", { query: "fix : tests ?" })).toBe(
      `Rien ne correspond à «${NNBSP}fix : tests ?${NNBSP}».`,
    );
  });
});
