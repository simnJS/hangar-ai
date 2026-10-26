import { describe, expect, it } from "vitest";
import { bindUnits, frenchSpacing, typeset } from "./typography";

const NNBSP = "\u202f";
const NBSP = "\u00a0";

describe("frenchSpacing", () => {
  it("puts a narrow no-break space before high punctuation", () => {
    expect(frenchSpacing("Réinitialiser {n} agents ?")).toBe(`Réinitialiser {n} agents${NNBSP}?`);
    expect(frenchSpacing("Shell : {program}")).toBe(`Shell${NNBSP}: {program}`);
    expect(frenchSpacing("garde-la ; les autres")).toBe(`garde-la${NNBSP}; les autres`);
    expect(frenchSpacing("{pct} %")).toBe(`{pct}${NNBSP}%`);
  });

  it("puts one inside the guillemets", () => {
    expect(frenchSpacing("Rien ne correspond à « {query} ».")).toBe(
      `Rien ne correspond à «${NNBSP}{query}${NNBSP}».`,
    );
    expect(frenchSpacing("«{title}»")).toBe(`«${NNBSP}{title}»`);
  });

  it("replaces whatever space was typed, and only one", () => {
    expect(frenchSpacing(`Compactage imminent${NBSP}: bientôt`)).toBe(
      `Compactage imminent${NNBSP}: bientôt`,
    );
    expect(frenchSpacing("Confirmer  ?")).toBe(`Confirmer${NNBSP}?`);
  });

  it("does not invent a space before a colon nothing preceded", () => {
    expect(frenchSpacing("C:\\dev à 12:30")).toBe("C:\\dev à 12:30");
  });
});

describe("bindUnits", () => {
  it("keeps a number with its unit", () => {
    expect(bindUnits("il y a {n} min")).toBe(`il y a {n}${NBSP}min`);
    expect(bindUnits("5 h")).toBe(`5${NBSP}h`);
    expect(bindUnits("toutes les 15 s — la présence")).toBe(`toutes les 15${NBSP}s — la présence`);
  });

  it("binds compound durations and a leading sign", () => {
    expect(bindUnits("{h} h {m}")).toBe(`{h}${NBSP}h${NBSP}{m}`);
    expect(bindUnits("{d} d {h} h")).toBe(`{d}${NBSP}d${NBSP}{h}${NBSP}h`);
    expect(bindUnits("< 1 min")).toBe(`<${NBSP}1${NBSP}min`);
  });

  it("leaves a word that merely starts like a unit alone", () => {
    expect(bindUnits("{n} sessions")).toBe("{n} sessions");
    expect(bindUnits("{n} heures")).toBe("{n} heures");
    expect(bindUnits("{name} default")).toBe("{name} default");
  });
});

describe("typeset", () => {
  it("only spaces French punctuation in French", () => {
    expect(typeset("Reset {n} agents?", "en")).toBe("Reset {n} agents?");
    expect(typeset("{pct}%", "en")).toBe("{pct}%");
    expect(typeset("{n} min ago", "en")).toBe(`{n}${NBSP}min ago`);
    expect(typeset("Retirer {n} min ?", "fr")).toBe(`Retirer {n}${NBSP}min${NNBSP}?`);
  });
});
