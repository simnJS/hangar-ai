"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { model, answer, recorder } = require("./helpers.js");

const W = 1000;
const H = 500;
const p = (x, y) => ({ x, y });

test("a pointer position becomes the image's pixels, kept inside it", () => {
  const rect = { left: 10, top: 20, width: 500, height: 250 };
  assert.deepEqual(model.toImagePoint(260, 145, rect, W, H), p(500, 250));
  assert.deepEqual(model.toImagePoint(-50, 9999, rect, W, H), p(0, 500));
  assert.deepEqual(model.toImagePoint(5, 5, { left: 0, top: 0, width: 0, height: 0 }, W, H), p(0, 0));
});

test("the pen's width follows the image, within limits", () => {
  assert.equal(model.unit(100, 100), 2);
  assert.equal(model.unit(1000, 500), 4);
  assert.equal(model.unit(8000, 4000), 12);
});

test("a drag makes the mark of its tool, and a speck makes nothing", () => {
  assert.deepEqual(model.fromDrag("arrow", p(10, 10), p(210, 110), [], "red", W, H), { type: "arrow", x1: 10, y1: 10, x2: 210, y2: 110, color: "red" });
  assert.deepEqual(model.fromDrag("rect", p(300, 200), p(100, 50), [], "blue", W, H), { type: "rect", x: 100, y: 50, w: 200, h: 150, color: "blue" });
  assert.deepEqual(model.fromDrag("free", p(0, 0), p(5, 5), [p(0, 0), p(3, 3), p(5, 5)], "green", W, H), { type: "free", points: [[0, 0], [3, 3], [5, 5]], color: "green" });
  assert.equal(model.fromDrag("arrow", p(10, 10), p(12, 11), [], "red", W, H), null);
  assert.equal(model.fromDrag("rect", p(10, 10), p(200, 11), [], "red", W, H), null, "a flat box is no box");
  assert.equal(model.fromDrag("free", p(0, 0), p(0, 0), [p(0, 0)], "red", W, H), null);
  assert.equal(model.fromDrag("note", p(0, 0), p(50, 50), [], "red", W, H), null, "a note is placed by a click");
});

test("a stroke keeps a point only when the pointer moved enough", () => {
  let points = [p(0, 0)];
  points = model.extend(points, p(0.5, 0), W, H);
  assert.equal(points.length, 1);
  points = model.extend(points, p(10, 0), W, H);
  assert.equal(points.length, 2);
});

test("notes are numbered in the order they were placed, and undo renumbers", () => {
  const shapes = [
    { type: "note", x: 1, y: 2, text: "a", color: "red" },
    { type: "arrow", x1: 0, y1: 0, x2: 50, y2: 50, color: "red" },
    { type: "note", x: 3, y: 4, text: "b", color: "blue" },
  ];
  assert.deepEqual(model.notesOf(shapes).map((note) => [note.n, note.index, note.text]), [[1, 0, "a"], [2, 2, "b"]]);
  assert.deepEqual(model.notesOf(shapes.slice(1)).map((note) => note.n), [1]);
  assert.deepEqual(model.counts(shapes), { arrow: 1, rect: 0, free: 0, note: 2 });
});

test("drawing paints the image first, then each mark with its colour, and numbers the notes", () => {
  const ctx = recorder();
  const image = { name: "the image" };
  const shapes = [
    { type: "rect", x: 10, y: 10, w: 50, h: 30, color: "red" },
    { type: "arrow", x1: 0, y1: 0, x2: 100, y2: 0, color: "blue" },
    { type: "free", points: [[0, 0], [5, 5], [9, 2]], color: "green" },
    { type: "note", x: 100, y: 100, text: "too close", color: "orange" },
    { type: "note", x: 200, y: 100, text: "", color: "magenta" },
  ];
  model.draw(ctx, { image, width: W, height: H, shapes }, 0.5);
  const names = ctx.log.map((entry) => entry[0]);
  assert.deepEqual(ctx.log[0], ["setTransform", 0.5, 0, 0, 0.5, 0, 0]);
  assert.deepEqual(ctx.log[2], ["drawImage", image, 0, 0, W, H]);
  assert.ok(names.indexOf("drawImage") < names.indexOf("strokeRect"));
  assert.deepEqual(ctx.log.filter((entry) => entry[0] === "strokeRect"), [["strokeRect", 10, 10, 50, 30]]);
  const numbers = ctx.log.filter((entry) => entry[0] === "fillText").map((entry) => entry[1]);
  assert.deepEqual(numbers, ["1", "too close", "2"]);
  assert.equal(ctx.strokeStyle !== undefined, true);
});

test("a draft is drawn with the rest, and no image means only the marks (for tests)", () => {
  const ctx = recorder();
  model.draw(ctx, { image: null, width: W, height: H, shapes: [], draft: { type: "rect", x: 0, y: 0, w: 10, h: 10, color: "red" } }, 1);
  assert.ok(!ctx.log.some((entry) => entry[0] === "drawImage"));
  assert.equal(ctx.log.filter((entry) => entry[0] === "strokeRect").length, 1);
});

test("a label that would leave the image goes to the other side of its badge", () => {
  const ctx = recorder();
  model.draw(ctx, { image: null, width: 200, height: 100, shapes: [{ type: "note", x: 190, y: 50, text: "long label here", color: "red" }] }, 1);
  const box = ctx.log.find((entry) => entry[0] === "fillRect");
  assert.ok(box[1] < 190, "to the left of the badge");
});

test("black or white ink, whichever reads", () => {
  assert.equal(model.ink("#ffffff"), "#000000");
  assert.equal(model.ink("#000000"), "#ffffff");
  assert.equal(model.ink(model.colorValue("blue")), "#ffffff");
  assert.equal(model.colorValue("nope"), model.COLORS[0].value, "an unknown colour is the first");
});

test("the JSON describes each mark in the written image's pixels", () => {
  const shapes = [
    { type: "arrow", x1: 10, y1: 20, x2: 110, y2: 120, color: "red" },
    { type: "rect", x: 100, y: 200, w: 40, h: 60, color: "blue" },
    { type: "free", points: [[10, 10], [30, 50], [20, 5]], color: "green" },
    { type: "note", x: 11, y: 13, text: "x", color: "orange" },
  ];
  assert.deepEqual(model.describe(shapes, 0.5), [
    { type: "arrow", color: "red", from: [5, 10], to: [55, 60] },
    { type: "rectangle", color: "blue", x: 50, y: 100, width: 20, height: 30 },
    { type: "freehand", color: "green", x: 5, y: 3, width: 10, height: 22 },
    { type: "note", color: "orange", x: 6, y: 7 },
  ]);
});

test("the message names the image, the counts, the comment and each note", () => {
  const text = answer.message({
    pngPath: ".hangar/annotations/20261031-231512.png",
    jsonPath: ".hangar/annotations/20261031-231512.json",
    jsonSaved: true,
    width: 800,
    height: 600,
    source: ".hangar/capture.png",
    comment: "Fix the spacing.",
    notes: [{ n: 1, x: 10, y: 20, text: "too close" }, { n: 2, x: 30, y: 40, text: "" }],
    counts: { arrow: 1, rect: 2, free: 0, note: 2 },
    reduced: false,
  });
  assert.equal(
    text,
    [
      "The person annotated an image for you.",
      "Image: .hangar/annotations/20261031-231512.png (800 × 600 px). Read it to see the marks.",
      "Source: .hangar/capture.png",
      "Marks: 1 arrow, 2 rectangles, 2 notes.",
      "Notes file: .hangar/annotations/20261031-231512.json",
      "Comment: Fix the spacing.",
      "Numbered notes (x, y in the image's pixels):",
      "1. too close (x 10, y 20)",
      "2. (no text) (x 30, y 40)",
    ].join("\n"),
  );
});

test("the message omits what is not there, says when the image was reduced, and stays under the limit", () => {
  const base = { pngPath: "a.png", jsonPath: "a.json", jsonSaved: false, width: 1, height: 1, source: null, comment: "", notes: [], counts: { arrow: 0, rect: 0, free: 0, note: 0 }, reduced: true };
  const text = answer.message(base);
  assert.match(text, /\(1 × 1 px, reduced from the original\)/);
  assert.match(text, /Marks: none\./);
  assert.doesNotMatch(text, /Notes file|Comment|Source|Numbered/);
  const long = answer.message({ ...base, comment: "x".repeat(50000) });
  assert.ok(long.length <= answer.MESSAGE_MAX);
});
