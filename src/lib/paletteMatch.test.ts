import { describe, expect, it } from "vitest";
import {
  filterPalette,
  fold,
  highlight,
  matchEntry,
  matchText,
  SCORE_KEYWORD,
  SCORE_PREFIX,
  SCORE_SUBSTRING,
  SCORE_TOKENS,
  SCORE_WORD,
  SCORE_WORDS,
} from "./paletteMatch";

/** The highlighted part of `text`, as the palette would bold it. */
const marked = (text: string, query: string) => {
  const match = matchText(text, query);
  return match ? highlight(text, match.ranges).filter((part) => part.hit).map((part) => part.text) : null;
};

describe("fold", () => {
  it("drops accents and case", () => {
    expect(fold("Mémoire Œuvre ÉCRAN")).toBe("memoire œuvre ecran");
  });
});

describe("matchText", () => {
  it("ignores accents and case on both sides", () => {
    expect(matchText("Mémoire", "memoire")?.score).toBe(SCORE_PREFIX);
    expect(matchText("Memoire", "MÉMOIRE")?.score).toBe(SCORE_PREFIX);
  });

  it("ranks a prefix over a word start over a substring", () => {
    expect(matchText("Relancer tous les panneaux", "rel")?.score).toBe(SCORE_PREFIX);
    expect(matchText("Tout relancer", "rel")?.score).toBe(SCORE_WORD);
    expect(matchText("Panneau précédent", "rec")?.score).toBe(SCORE_SUBSTRING);
  });

  it("counts a word start after punctuation, not only after a space", () => {
    expect(matchText("Aller à « Ben »", "ben")?.score).toBe(SCORE_WORD);
    expect(matchText("src/lib/keys.ts", "keys")?.score).toBe(SCORE_WORD);
  });

  it("finds every word of the query, in any order", () => {
    expect(matchText("Tout relancer", "relancer tout")?.score).toBe(SCORE_WORDS);
    expect(matchText("Diviser le panneau actif", "div ctif")?.score).toBe(SCORE_TOKENS);
    expect(matchText("Tout relancer", "relancer rien")).toBeNull();
  });

  it("matches everything, unmarked, when nothing is typed", () => {
    expect(matchText("Anything", "   ")).toEqual({ score: 0, ranges: [] });
  });

  it("returns null when the text does not hold the query", () => {
    expect(matchText("Afficher le tableau", "memoire")).toBeNull();
  });
});

describe("highlight", () => {
  it("marks the letters as typed in the original, accents included", () => {
    expect(marked("Afficher la mémoire", "memo")).toEqual(["mémo"]);
    expect(marked("Réorganiser", "reo")).toEqual(["Réo"]);
  });

  it("keeps decomposed accents with their letter", () => {
    const decomposed = "Mémoire";
    expect(marked(decomposed, "mem")).toEqual(["Mém"]);
  });

  it("marks each word of a multi-word query, merging neighbours", () => {
    expect(marked("Tout relancer", "relancer tout")).toEqual(["Tout", "relancer"]);
    expect(highlight("abcdef", [
      { start: 1, end: 3 },
      { start: 2, end: 4 },
    ])).toEqual([
      { text: "a", hit: false },
      { text: "bcd", hit: true },
      { text: "ef", hit: false },
    ]);
  });

  it("returns the text whole when nothing matched", () => {
    expect(highlight("Tableau", [])).toEqual([{ text: "Tableau", hit: false }]);
  });
});

describe("matchEntry", () => {
  it("falls back to the keywords, below any label match", () => {
    expect(matchEntry("Aller à Ben", ["Codex", "attend ta réponse"], "codex")).toEqual({
      score: SCORE_KEYWORD,
      ranges: [],
    });
    expect(matchEntry("Aller à Ben", ["Codex"], "ben")?.score).toBe(SCORE_WORD);
    expect(matchEntry("Aller à Ben", [], "codex")).toBeNull();
  });
});

describe("filterPalette", () => {
  const entries = [
    { group: "Panneaux", label: "Aller à Ava" },
    { group: "Panneaux", label: "Tout relancer" },
    { group: "Vues", label: "Afficher le tableau" },
    { group: "Workspaces", label: "Ouvrir storefront" },
    { group: "Vues", label: "Relancer la vue" },
  ];

  it("keeps the given order and groups when nothing is typed", () => {
    const groups = filterPalette(entries, "");
    expect(groups.map((group) => group.group)).toEqual(["Panneaux", "Vues", "Workspaces"]);
    expect(groups[1].entries.map(({ entry }) => entry.label)).toEqual([
      "Afficher le tableau",
      "Relancer la vue",
    ]);
  });

  it("puts the group holding the best match first", () => {
    const groups = filterPalette(entries, "relancer");
    expect(groups.map((group) => group.group)).toEqual(["Vues", "Panneaux"]);
    expect(groups[0].entries[0].entry.label).toBe("Relancer la vue");
  });

  it("sorts inside a group and keeps ties in the given order", () => {
    const list = [
      { group: "G", label: "Le tableau" },
      { group: "G", label: "Tableau" },
      { group: "G", label: "Autre tableau" },
    ];
    expect(filterPalette(list, "tab")[0].entries.map(({ entry }) => entry.label)).toEqual([
      "Tableau",
      "Le tableau",
      "Autre tableau",
    ]);
  });

  it("drops groups left empty", () => {
    expect(filterPalette(entries, "storefront").map((group) => group.group)).toEqual([
      "Workspaces",
    ]);
    expect(filterPalette(entries, "zzz")).toEqual([]);
  });
});
