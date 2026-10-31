"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { logic, model, answer } = require("./helpers.js");

test("English and French have the same keys, and French keeps its accents", () => {
  assert.deepEqual(Object.keys(logic.STRINGS.fr).sort(), Object.keys(logic.STRINGS.en).sort());
  for (const key of Object.keys(logic.STRINGS.en)) {
    const vars = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
    assert.deepEqual(vars(logic.STRINGS.fr[key]), vars(logic.STRINGS.en[key]), key);
  }
  assert.equal(logic.t("fr", "empty_title"), "Pas encore d’image");
  assert.equal(logic.t("fr", "tool_arrow"), "Flèche");
  assert.equal(logic.t("fr", "tool_free"), "Main levée");
  assert.match(logic.t("fr", "denied_text"), /Réglages › Kits/);
});

test("the language is French for any French locale, English otherwise", () => {
  for (const locale of ["fr", "fr-FR", "FR-ca", "fr_BE"]) assert.equal(logic.pickLang(locale), "fr", locale);
  for (const locale of ["en", "en-US", "de", "frisian", "", undefined, null]) assert.equal(logic.pickLang(locale), "en", String(locale));
});

test("a missing key or variable shows itself rather than nothing", () => {
  assert.equal(logic.t("en", "nope"), "nope");
  assert.equal(logic.t("fr", "loading", {}), "Lecture de {path}…");
  assert.equal(logic.t("en", "loading", { path: "a.png" }), "Reading a.png…");
});

test("the path param: a short string, else nothing", () => {
  assert.equal(logic.cleanPath(" .hangar/a.png "), ".hangar/a.png");
  for (const bad of [undefined, null, 3, "", "   ", "x".repeat(1025), {}, ["a"]]) assert.equal(logic.cleanPath(bad), null, String(bad));
  assert.equal(logic.baseName(".hangar/a.png"), "a.png");
  assert.equal(logic.baseName("sub\\b.png"), "b.png");
});

test("a name is the local time, and a taken one gets a counter", () => {
  assert.equal(logic.stamp(new Date(2026, 9, 31, 23, 5, 9)), "20261031-230509");
  assert.deepEqual(logic.names("20261031-230509", 3), ["20261031-230509", "20261031-230509-2", "20261031-230509-3"]);
});

test("only bitmaps open: never an SVG or a PDF", () => {
  for (const mime of ["image/png", "image/JPEG", "image/webp", "image/gif"]) assert.ok(logic.openableMime(mime), mime);
  for (const mime of ["image/svg+xml", "application/pdf", "text/html", "", undefined]) assert.ok(!logic.openableMime(mime), String(mime));
  assert.equal(logic.dataUrl("IMAGE/PNG", "AAAA"), "data:image/png;base64,AAAA");
});

test("fitting never enlarges, and zoom steps move by one", () => {
  assert.equal(logic.fitZoom(1000, 500, 500, 1000), 0.5);
  assert.equal(logic.fitZoom(1000, 4000, 500, 400), 0.1);
  assert.equal(logic.fitZoom(100, 100, 500, 500), 1);
  assert.equal(logic.fitZoom(100, 100, 0, 0), 1);
  assert.equal(logic.stepZoom(0.5, 1), 0.67);
  assert.equal(logic.stepZoom(0.5, -1), 0.33);
  assert.equal(logic.stepZoom(0.47, 1), 0.5);
  assert.equal(logic.stepZoom(4, 1), 4);
  assert.equal(logic.stepZoom(0.1, -1), 0.1);
});

test("text is clipped by characters, and a note stays on one line", () => {
  assert.equal(logic.clip("a\nb\r\nc", 10), "a b c");
  assert.equal(logic.clip("a\nb", 10, true), "a\nb");
  assert.equal(logic.clip("éééé", 2), "éé");
  assert.equal(Array.from(logic.clip("😀".repeat(10), 3)).length, 3);
});

test("the export scales go down, and the base64 limit is 10 MB of bytes", () => {
  assert.deepEqual([...logic.EXPORT_SCALES].sort((a, b) => b - a), logic.EXPORT_SCALES);
  assert.equal(logic.EXPORT_SCALES[0], 1);
  assert.equal(logic.BASE64_MAX, Math.ceil((10 * 1024 * 1024) / 3) * 4);
  assert.equal(answer.MESSAGE_MAX, 20000);
  assert.equal(model.COLORS.length, 5);
});
