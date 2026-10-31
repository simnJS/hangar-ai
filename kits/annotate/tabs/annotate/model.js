"use strict";
// The marks on an image: what a drag makes of a tool, how a mark is drawn on a canvas
// context (the screen's and the exported PNG's alike), and what the marks add up to.
// Coordinates are the image's own pixels. No DOM: `draw` takes any 2D context.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.AnnotateModel = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const TOOLS = ["arrow", "rect", "free", "note"];
  /** The only colours of the tab: the person's to choose, painted on the image. */
  const COLORS = [
    { id: "red", value: "#e5484d" },
    { id: "orange", value: "#f08c00" },
    { id: "green", value: "#2f9e44" },
    { id: "blue", value: "#3b82f6" },
    { id: "magenta", value: "#d6409f" },
  ];
  const LABEL_MAX_CHARS = 48;

  const colorValue = (id) => (COLORS.find((color) => color.id === id) || COLORS[0]).value;

  /** The pen's width in image pixels: marks keep their look on a 4K capture and a thumbnail. */
  function unit(width, height) {
    return Math.min(12, Math.max(2, Math.round(Math.max(width, height) / 250)));
  }

  const round = (value) => Math.round(value * 10) / 10;
  const clamp = (value, low, high) => Math.min(high, Math.max(low, value));

  /** A pointer position, from the page's pixels to the image's, kept inside it. */
  function toImagePoint(clientX, clientY, rect, width, height) {
    if (!(rect.width > 0) || !(rect.height > 0)) return { x: 0, y: 0 };
    return {
      x: round(clamp(((clientX - rect.left) * width) / rect.width, 0, width)),
      y: round(clamp(((clientY - rect.top) * height) / rect.height, 0, height)),
    };
  }

  /** The mark a finished drag makes, or null when it is too small to mean anything. */
  function fromDrag(tool, start, end, points, color, width, height) {
    const min = unit(width, height) * 2;
    const dx = end.x - start.x;
    const dy = end.y - start.y;
    if (tool === "arrow") {
      return Math.hypot(dx, dy) < min * 2 ? null : { type: "arrow", x1: start.x, y1: start.y, x2: end.x, y2: end.y, color };
    }
    if (tool === "rect") {
      if (Math.abs(dx) < min || Math.abs(dy) < min) return null;
      return { type: "rect", x: Math.min(start.x, end.x), y: Math.min(start.y, end.y), w: Math.abs(dx), h: Math.abs(dy), color };
    }
    if (tool === "free") {
      return points.length < 2 ? null : { type: "free", points: points.map((p) => [p.x, p.y]), color };
    }
    return null;
  }

  /** Adds a point to a stroke only when it moved enough to show. */
  function extend(points, point, width, height) {
    const last = points[points.length - 1];
    if (last && Math.hypot(point.x - last.x, point.y - last.y) < unit(width, height) / 2) return points;
    return [...points, point];
  }

  /** The notes among the marks, numbered in the order they were placed. */
  function notesOf(shapes) {
    let n = 0;
    const notes = [];
    shapes.forEach((shape, index) => {
      if (shape.type === "note") {
        n += 1;
        notes.push({ n, index, x: shape.x, y: shape.y, text: shape.text || "", color: shape.color });
      }
    });
    return notes;
  }

  /** How many of each, for one sentence. */
  function counts(shapes) {
    const total = { arrow: 0, rect: 0, free: 0, note: 0 };
    for (const shape of shapes) if (shape.type in total) total[shape.type] += 1;
    return total;
  }

  /** Black or white, whichever reads on this colour. */
  function ink(hex) {
    const n = parseInt(hex.slice(1), 16);
    const luminance = 0.299 * ((n >> 16) & 255) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255);
    return luminance > 150 ? "#000000" : "#ffffff";
  }

  function clipLabel(text) {
    const chars = Array.from(String(text).replace(/\s+/g, " ").trim());
    return chars.length > LABEL_MAX_CHARS ? `${chars.slice(0, LABEL_MAX_CHARS - 1).join("")}…` : chars.join("");
  }

  function drawArrow(ctx, shape, line) {
    const head = Math.max(line * 5, 14);
    const angle = Math.atan2(shape.y2 - shape.y1, shape.x2 - shape.x1);
    ctx.beginPath();
    ctx.moveTo(shape.x1, shape.y1);
    ctx.lineTo(shape.x2, shape.y2);
    for (const side of [-1, 1]) {
      ctx.moveTo(shape.x2, shape.y2);
      ctx.lineTo(shape.x2 - head * Math.cos(angle + side * 0.5), shape.y2 - head * Math.sin(angle + side * 0.5));
    }
    ctx.stroke();
  }

  function drawNote(ctx, shape, n, line, width) {
    const radius = line * 3.4;
    ctx.beginPath();
    ctx.arc(shape.x, shape.y, radius, 0, Math.PI * 2);
    ctx.fillStyle = colorValue(shape.color);
    ctx.fill();
    ctx.lineWidth = Math.max(1, line / 2);
    ctx.strokeStyle = ink(colorValue(shape.color));
    ctx.stroke();
    ctx.fillStyle = ink(colorValue(shape.color));
    ctx.font = `bold ${round(radius * 1.25)}px sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(n), shape.x, shape.y);
    const label = clipLabel(shape.text || "");
    if (!label) return;
    const size = radius * 1.05;
    ctx.font = `${round(size)}px sans-serif`;
    ctx.textAlign = "left";
    const pad = size * 0.35;
    const boxWidth = ctx.measureText(label).width + pad * 2;
    const boxHeight = size + pad * 2;
    const right = shape.x + radius + pad;
    const left = right + boxWidth > width ? shape.x - radius - pad - boxWidth : right;
    ctx.fillStyle = colorValue(shape.color);
    ctx.fillRect(left, shape.y - boxHeight / 2, boxWidth, boxHeight);
    ctx.fillStyle = ink(colorValue(shape.color));
    ctx.fillText(label, left + pad, shape.y);
  }

  /**
   * Paints the image, when given, and the marks, on `ctx`, scaled by `k` from image
   * pixels to the context's own. `draft` is a mark still being dragged.
   */
  function draw(ctx, spec, k) {
    const { image, width, height, shapes, draft } = spec;
    ctx.setTransform(k, 0, 0, k, 0, 0);
    ctx.clearRect(0, 0, width, height);
    if (image) ctx.drawImage(image, 0, 0, width, height);
    const line = unit(width, height);
    ctx.lineWidth = line;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    let n = 0;
    for (const shape of draft ? [...shapes, draft] : shapes) {
      ctx.strokeStyle = colorValue(shape.color);
      ctx.lineWidth = line;
      if (shape.type === "arrow") drawArrow(ctx, shape, line);
      else if (shape.type === "rect") ctx.strokeRect(shape.x, shape.y, shape.w, shape.h);
      else if (shape.type === "free") {
        ctx.beginPath();
        shape.points.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
        ctx.stroke();
      } else if (shape.type === "note") {
        n += 1;
        drawNote(ctx, shape, n, line, width);
      }
    }
  }

  /** The marks as the JSON file gives them, scaled and rounded to the written image. */
  function describe(shapes, scale) {
    const px = (value) => Math.round(value * scale);
    return shapes.map((shape) => {
      if (shape.type === "arrow") return { type: "arrow", color: shape.color, from: [px(shape.x1), px(shape.y1)], to: [px(shape.x2), px(shape.y2)] };
      if (shape.type === "rect") return { type: "rectangle", color: shape.color, x: px(shape.x), y: px(shape.y), width: px(shape.w), height: px(shape.h) };
      if (shape.type === "free") {
        const xs = shape.points.map((p) => p[0]);
        const ys = shape.points.map((p) => p[1]);
        const x = px(Math.min(...xs));
        const y = px(Math.min(...ys));
        return { type: "freehand", color: shape.color, x, y, width: px(Math.max(...xs)) - x, height: px(Math.max(...ys)) - y };
      }
      return { type: "note", color: shape.color, x: px(shape.x), y: px(shape.y) };
    });
  }

  return { TOOLS, COLORS, colorValue, unit, toImagePoint, fromDrag, extend, notesOf, counts, ink, clipLabel, draw, describe };
});
