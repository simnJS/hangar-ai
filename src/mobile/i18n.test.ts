import { describe, expect, it } from "vitest";
import { DICTIONARIES, pickLocale, translator } from "./i18n";

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();

describe("the phone's dictionaries", () => {
  it("fill every placeholder the English does, no more", () => {
    for (const [key, english] of Object.entries(DICTIONARIES.en)) {
      expect(placeholders(DICTIONARIES.fr[key as keyof typeof DICTIONARIES.en]), key).toEqual(placeholders(english));
    }
  });

  it("come in complete plural pairs", () => {
    for (const dictionary of Object.values(DICTIONARIES)) {
      const keys = Object.keys(dictionary);
      for (const key of keys.filter((key) => key.endsWith("_one"))) {
        expect(keys, key).toContain(key.replace(/_one$/, "_other"));
      }
    }
  });

  it("set French punctuation with its narrow no-break space", () => {
    const { t } = translator("fr");
    for (const key of Object.keys(DICTIONARIES.fr) as (keyof typeof DICTIONARIES.fr)[]) {
      const text = t(key);
      expect(text, key).not.toMatch(/[  ][?!;:»]/);
      expect(text, key).not.toMatch(/«[  ]/);
    }
    expect(t("error.refused", { reason: "x" })).toBe("Le PC a refusé : x");
  });
});

describe("translating", () => {
  it("fills placeholders and chooses plurals by language", () => {
    const fr = translator("fr");
    const en = translator("en");
    expect(fr.tn("forYou.count", 1)).toBe("1 chose vous attend");
    expect(fr.tn("forYou.count", 3)).toBe("3 choses vous attendent");
    // French counts 0 as one; English as many.
    expect(fr.tn("project.workers", 0)).toBe("0 worker");
    expect(en.tn("project.workers", 0)).toBe("0 workers");
    expect(en.t("banner.unreachableSince", { time: "2:32 PM" })).toBe("PC unreachable since 2:32 PM");
    expect(fr.t("forYou.empty")).toBe("Rien ne vous attend");
  });

  it("counts agents with the words their states are shown with", () => {
    for (const locale of ["en", "fr"] as const) {
      const { t, tn } = translator(locale);
      for (const state of ["working", "waiting", "yours", "idle"] as const) {
        expect(tn(`fleet.${state}`, 1).toLowerCase(), `${locale} ${state}`).toContain(t(`activity.${state}`).toLowerCase());
      }
    }
  });

  it("leaves what fills a placeholder as it was written", () => {
    expect(translator("fr").t("composer.placeholder", { name: "Ben : test ?" })).toBe("Écrire à Ben : test ?");
  });
});

describe("choosing the language", () => {
  it("follows the phone's first language it knows", () => {
    expect(pickLocale(["fr-FR", "en-US"])).toBe("fr");
    expect(pickLocale(["de-DE", "fr-CH"])).toBe("fr");
    expect(pickLocale(["en-GB", "fr"])).toBe("en");
    expect(pickLocale(["ja-JP"])).toBe("en");
    expect(pickLocale([])).toBe("en");
  });

  it("lets ?lang= decide", () => {
    expect(pickLocale(["fr-FR"], "en")).toBe("en");
    expect(pickLocale(["en-US"], "fr")).toBe("fr");
    expect(pickLocale(["fr-FR"], "xx")).toBe("fr");
  });
});
