import { describe, expect, it, vi } from "vitest";
import source from "./strings.ts?raw";

// useLocale reaches for the app's store, which talks to Tauri; the
// dictionaries and translateAccounts() need neither.
vi.mock("../../store", () => ({ useStore: () => ({}) }));

const { ACCOUNT_DICTIONARIES, en, fr, translateAccounts } = await import("./strings");

const NNBSP = " ";

/** Every French string that breaks `rule`, as "key: text", for a readable failure. */
function offenders(rule: (text: string) => boolean): string[] {
  return Object.entries(ACCOUNT_DICTIONARIES.fr)
    .filter(([, text]) => rule(text))
    .map(([key, text]) => `${key}: ${JSON.stringify(text)}`);
}

describe("accounts strings", () => {
  it("has the same keys in both languages", () => {
    expect(Object.keys(fr).sort()).toEqual(Object.keys(en).sort());
  });

  it("keeps the same placeholders in both languages", () => {
    const holes = (text: string) => (text.match(/\{\w+\}/g) ?? []).sort().join();
    const wrong = Object.keys(en).filter(
      (key) => holes(en[key as keyof typeof en]) !== holes(fr[key as keyof typeof fr]),
    );
    expect(wrong).toEqual([]);
  });
});

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
    const french = source.slice(source.indexOf("export const fr:"), source.indexOf("function typesetAll"));
    expect(french).not.toMatch(/[  ]|\u00a0|\u202f/);
  });
});

describe("translateAccounts", () => {
  it("picks the plural and fills placeholders", () => {
    expect(translateAccounts("en", "switcher.running", { n: 1 })).toBe("1 agent running.");
    expect(translateAccounts("fr", "switcher.running", { n: 3 })).toBe("3 agents en cours.");
  });

  it("sets a percentage the French way", () => {
    expect(translateAccounts("fr", "switcher.percent", { n: 40 })).toBe(`40${NNBSP}%`);
    expect(translateAccounts("en", "switcher.percent", { n: 40 })).toBe("40%");
  });

  it("never touches what fills a placeholder", () => {
    expect(translateAccounts("fr", "settings.removeAsk", { name: "Perso : 2 ?" })).toBe(
      `Retirer «${NNBSP}Perso : 2 ?${NNBSP}»${NNBSP}?`,
    );
  });
});
