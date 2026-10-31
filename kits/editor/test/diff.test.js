"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { diff } = require("./helpers.js");

const changes = (before, after) => diff.diffLines(before, after).filter((op) => op.op !== "same").map((op) => [op.op, op.text]);

test("what changed, line by line, with both numberings", () => {
  assert.deepEqual(changes("a\nb\nc", "a\nb\nc"), []);
  assert.deepEqual(changes("a\nb\nc", "a\nX\nc"), [["del", "b"], ["add", "X"]]);
  assert.deepEqual(changes("a\nc", "a\nb\nc"), [["add", "b"]]);
  assert.deepEqual(changes("a\nb\nc", "a\nc"), [["del", "b"]]);
  assert.deepEqual(changes("", "a\n"), [["add", "a"]]);
  const ops = diff.diffLines("a\nb\nc", "a\nX\nY\nc");
  assert.deepEqual(ops.map((op) => [op.op, op.a, op.b]), [
    ["same", 1, 1], ["del", 2, null], ["add", null, 2], ["add", null, 3], ["same", 3, 4],
  ]);
});

test("lines moved in the middle of a block are found", () => {
  assert.deepEqual(changes("1\n2\n3\n4\n5\n6", "1\n3\n4\nnew\n5\n6"), [["del", "2"], ["add", "new"]]);
});

test("a change too large to compare is shown replaced, not lost", () => {
  const big = (prefix) => Array.from({ length: 3000 }, (_, i) => `${prefix}${i}`).join("\n");
  const ops = diff.diffLines(big("a"), big("b"));
  assert.equal(ops.filter((op) => op.op === "del").length, 3000);
  assert.equal(ops.filter((op) => op.op === "add").length, 3000);
});

test("a small change in a large file is cheap", () => {
  const lines = Array.from({ length: 50000 }, (_, i) => `line ${i}`);
  const changed = [...lines];
  changed[25000] = "changed";
  const started = Date.now();
  const ops = diff.diffLines(lines.join("\n"), changed.join("\n"));
  assert.ok(Date.now() - started < 1000);
  assert.deepEqual(ops.filter((op) => op.op !== "same").map((op) => op.text), ["line 25000", "changed"]);
});

test("unchanged stretches fold around the changes", () => {
  const lines = Array.from({ length: 20 }, (_, i) => String(i)).join("\n");
  const changed = lines.replace("\n10\n", "\nten\n");
  const hunks = diff.hunks(diff.diffLines(lines, changed));
  assert.deepEqual(hunks.map((hunk) => (hunk.op === "skip" ? hunk.count : hunk.text)), [7, "7", "8", "9", "10", "ten", "11", "12", "13", 6]);
  assert.equal(diff.hasChanges(diff.diffLines("a", "a")), false);
  assert.equal(diff.hasChanges(diff.diffLines("a", "b")), true);
  assert.deepEqual(diff.hunks(diff.diffLines("a\nb", "a\nb")), [{ op: "skip", count: 2 }]);
});

test("what a textarea takes away comes back", () => {
  assert.equal(diff.normalize("a\r\nb\rc\n"), "a\nb\nc\n");
  assert.equal(diff.restoreEols("a\nb\n", "a\nc\n"), "a\nc\n");
  assert.equal(diff.restoreEols("a\r\nb\r\n", "a\nc\nd\n"), "a\r\nc\r\nd\r\n");
  assert.equal(diff.restoreEols("no end of line", "no end\nof line"), "no end\nof line");
  assert.equal(diff.restoreEols("", "new\n"), "new\n");
  assert.equal(diff.restoreEols("a\r\nb\nc\r\n", "a\nb\nc\n"), "a\r\nb\nc\r\n", "mixed, untouched");
  assert.equal(diff.restoreEols("a\r\nb\nc\r\n", "a\nb\nnew\nc\n"), "a\r\nb\nnew\r\nc\r\n", "a new line takes the common one");
  assert.equal(diff.restoreEols("a\rb\r", "a\nb\nc\n"), "a\rb\rc\r");
  assert.equal(diff.restoreEols("a\r\nb", "a\nb\nc"), "a\r\nb\r\nc", "the old last line had none of its own");
});
