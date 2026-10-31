"use strict";
// Lines compared, for the diff before saving and the conflict view, and the line
// endings a textarea takes away, given back. No DOM, no Hangar.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorDiff = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const LCS_CELLS = 4000000; // beyond this the changed block is shown as replaced whole
  const CONTEXT = 3;

  /** What a textarea does to a text: its `value` only ever holds \n. */
  const normalize = (text) => text.replace(/\r\n?/g, "\n");

  function middle(a, b, offsetA, offsetB, out) {
    const n = a.length;
    const m = b.length;
    const push = (op, i, j) =>
      out.push(
        op === "add"
          ? { op, text: b[j], a: null, b: offsetB + j + 1 }
          : { op, text: a[i], a: offsetA + i + 1, b: null },
      );
    if (!n || !m || n * m > LCS_CELLS) {
      for (let i = 0; i < n; i++) push("del", i);
      for (let j = 0; j < m; j++) push("add", 0, j);
      return;
    }
    const width = m + 1;
    const table = new Uint32Array((n + 1) * width);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        table[i * width + j] =
          a[i] === b[j] ? table[(i + 1) * width + j + 1] + 1 : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && a[i] === b[j]) {
        out.push({ op: "same", text: a[i], a: offsetA + i + 1, b: offsetB + j + 1 });
        i++;
        j++;
      } else if (j >= m || (i < n && table[(i + 1) * width + j] >= table[i * width + j + 1])) {
        push("del", i++);
      } else {
        push("add", 0, j++);
      }
    }
  }

  /** Every line of `before` and `after` (both \n only), in order: same, del or add, with its numbers. */
  function diffLines(before, after) {
    const a = before.split("\n");
    const b = after.split("\n");
    let head = 0;
    while (head < a.length && head < b.length && a[head] === b[head]) head++;
    let tail = 0;
    while (tail < a.length - head && tail < b.length - head && a[a.length - 1 - tail] === b[b.length - 1 - tail]) tail++;
    const out = [];
    for (let i = 0; i < head; i++) out.push({ op: "same", text: a[i], a: i + 1, b: i + 1 });
    middle(a.slice(head, a.length - tail), b.slice(head, b.length - tail), head, head, out);
    for (let i = tail; i > 0; i--) out.push({ op: "same", text: a[a.length - i], a: a.length - i + 1, b: b.length - i + 1 });
    return out;
  }

  /** The same, unchanged stretches folded into `{ op: "skip", count }`. */
  function hunks(ops, context = CONTEXT) {
    const near = new Uint8Array(ops.length);
    ops.forEach((op, index) => {
      if (op.op === "same") return;
      const end = Math.min(ops.length - 1, index + context);
      for (let i = Math.max(0, index - context); i <= end; i++) near[i] = 1;
    });
    const out = [];
    let skipped = 0;
    ops.forEach((op, index) => {
      if (near[index]) {
        if (skipped) out.push({ op: "skip", count: skipped });
        skipped = 0;
        out.push(op);
      } else {
        skipped++;
      }
    });
    if (skipped) out.push({ op: "skip", count: skipped });
    return out;
  }

  const hasChanges = (ops) => ops.some((op) => op.op !== "same");

  /**
   * `edited` (\n only) with the line endings of `original` back: each line that did
   * not change keeps its own, and a new one takes the file's most common.
   */
  function restoreEols(original, edited) {
    const pieces = original.split(/(\r\n|\n|\r)/);
    const eols = [];
    for (let i = 1; i < pieces.length; i += 2) eols.push(pieces[i]);
    const count = (eol) => eols.filter((other) => other === eol).length;
    const counts = ["\n", "\r\n", "\r"].map((eol) => [eol, count(eol)]);
    if (counts[0][1] === eols.length) return edited;
    if (counts[1][1] === eols.length) return edited.replace(/\n/g, "\r\n");
    const common = counts.reduce((best, each) => (each[1] > best[1] ? each : best))[0];
    const lines = edited.split("\n");
    const kept = new Array(lines.length).fill(null);
    for (const op of diffLines(normalize(original), edited)) {
      if (op.op === "same") kept[op.b - 1] = eols[op.a - 1] || null;
    }
    return lines.map((line, index) => (index === lines.length - 1 ? line : line + (kept[index] || common))).join("");
  }

  return { normalize, diffLines, hunks, hasChanges, restoreEols };
});
