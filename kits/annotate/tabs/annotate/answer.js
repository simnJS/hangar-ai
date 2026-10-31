"use strict";
// What leaves the tab for the agent: the message `panes.send` carries, and the small
// JSON file beside the image. In English whatever the tab's language, because an
// agent reads it; the person's own words (notes, comment) go through as they wrote them.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.AnnotateAnswer = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  /** `panes.send` takes 20 000 characters at most. */
  const MESSAGE_MAX = 20000;

  const plural = (n, one, other) => `${n} ${n === 1 ? one : other}`;

  function marksSentence(counts) {
    const parts = [];
    if (counts.arrow) parts.push(plural(counts.arrow, "arrow", "arrows"));
    if (counts.rect) parts.push(plural(counts.rect, "rectangle", "rectangles"));
    if (counts.free) parts.push(plural(counts.free, "freehand stroke", "freehand strokes"));
    if (counts.note) parts.push(plural(counts.note, "note", "notes"));
    return parts.length ? parts.join(", ") : "none";
  }

  /**
   * The JSON file: the image's name and size, the comment, the numbered notes and a
   * line per mark, all in the written image's pixels.
   */
  function record({ png, width, height, source, comment, notes, marks, createdAt }) {
    return {
      version: 1,
      image: png,
      width,
      height,
      source: source || null,
      createdAt,
      comment: comment || "",
      notes: notes.map((note) => ({ n: note.n, x: note.x, y: note.y, text: note.text })),
      marks,
    };
  }

  /** @param {{ pngPath, jsonPath, jsonSaved, width, height, source, comment, notes, counts, reduced }} data */
  function message(data) {
    const lines = [
      "The person annotated an image for you.",
      `Image: ${data.pngPath} (${data.width} × ${data.height} px${data.reduced ? ", reduced from the original" : ""}). Read it to see the marks.`,
    ];
    if (data.source) lines.push(`Source: ${data.source}`);
    lines.push(`Marks: ${marksSentence(data.counts)}.`);
    if (data.jsonSaved) lines.push(`Notes file: ${data.jsonPath}`);
    if (data.comment) lines.push(`Comment: ${data.comment}`);
    if (data.notes.length) {
      lines.push("Numbered notes (x, y in the image's pixels):");
      for (const note of data.notes) {
        lines.push(`${note.n}. ${note.text ? note.text : "(no text)"} (x ${note.x}, y ${note.y})`);
      }
    }
    const text = lines.join("\n");
    return text.length > MESSAGE_MAX ? `${text.slice(0, MESSAGE_MAX - 1)}…` : text;
  }

  return { MESSAGE_MAX, marksSentence, record, message };
});
