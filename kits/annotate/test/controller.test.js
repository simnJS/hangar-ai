"use strict";
// The behaviour, on a fake Hangar and a fake browser: what opens, what is sent, what
// is said when something is refused. No test writes a real file.
const test = require("node:test");
const assert = require("node:assert/strict");
const { opened, fakeHangar, fakeDeps, fileOf, PICTURE, PNG_BASE64, settle, logic, refusal } = require("./helpers.js");

const arrow = { type: "arrow", x1: 0, y1: 0, x2: 100, y2: 100, color: "red" };

test("a path param opens that image from the workspace, and nothing else is read", async () => {
  const { hangar, state } = await opened({ params: { path: ".hangar/capture.png" }, images: { ".hangar/capture.png": PICTURE } });
  assert.equal(state.phase, "ready");
  assert.deepEqual(hangar.callsTo("files.readBinary").map((call) => call.args), [{ path: ".hangar/capture.png" }]);
  assert.equal(state.image.url, `data:image/png;base64,${PNG_BASE64}`);
  assert.deepEqual([state.image.width, state.image.height, state.image.name, state.source], [800, 600, "capture.png", ".hangar/capture.png"]);
});

test("with no path the tab waits for a pasted, dropped or chosen image", async () => {
  const { hangar, controller, state } = await opened();
  assert.equal(state.phase, "empty");
  assert.equal(hangar.callsTo("files.readBinary").length, 0);
  await controller.openFile(fileOf("image/png", "pasted.png"));
  assert.equal(state.phase, "ready");
  assert.equal(state.source, null);
  assert.equal(state.image.name, "pasted.png");
});

test("what is not a bitmap is refused in words, and nothing opens", async () => {
  const { controller, state } = await opened();
  await controller.openFile(fileOf("image/svg+xml", "x.svg"));
  assert.equal(state.phase, "empty");
  assert.match(state.notice.text, /not an image/);
  await controller.openFile(fileOf("application/pdf", "x.pdf"));
  assert.equal(state.phase, "empty");
  await controller.openFile(fileOf("image/png", "huge.png", logic.INPUT_MAX_BYTES + 1));
  assert.match(state.notice.text, /30 MB/);
  const undecodable = fakeDeps();
  undecodable.decode = async () => {
    throw new Error("broken");
  };
  const second = await opened({ deps: undecodable });
  await second.controller.openFile(fileOf());
  assert.equal(second.state.phase, "empty");
  assert.match(second.state.notice.text, /not an image/);
});

test("a path that is not found, not an image or an SVG says so and offers another way in", async () => {
  const missing = await opened({ params: { path: "nope.png" } });
  assert.equal(missing.state.phase, "empty");
  assert.match(missing.state.notice.text, /nope\.png was not found/);
  const wrongType = await opened({ params: { path: "a.svg" }, images: { "a.svg": { base64: "AAAA", mime: "image/svg+xml" } } });
  assert.equal(wrongType.state.phase, "empty");
  assert.match(wrongType.state.notice.text, /not a PNG, JPEG, WebP or GIF/);
  const unsupported = await opened({ params: { path: "a.txt" }, fail: { "files.readBinary": "unsupported" } });
  assert.match(unsupported.state.notice.text, /not a PNG/);
  const broken = await opened({ params: { path: "a.png" }, fail: { "files.readBinary": "failed" } });
  assert.equal(broken.state.notice.retry, "a.png");
});

test("denied reading says what to accept, and pasting still works", async () => {
  const { controller, state } = await opened({ params: { path: "a.png" }, deny: ["files.readBinary"] });
  assert.equal(state.notice.kind, "denied");
  assert.match(state.notice.text, /files\.read/);
  assert.match(state.notice.text, /paste or drop/);
  await controller.openFile(fileOf());
  assert.equal(state.phase, "ready");
});

test("the tab can be started with tab.params refused", async () => {
  const { state } = await opened({ deny: ["tab.params"] });
  assert.equal(state.phase, "empty");
  assert.equal(state.caller, null);
});

test("tools, colours and zoom change only to what exists", async () => {
  const { controller, state } = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE } });
  controller.setTool("rect");
  assert.equal(state.tool, "rect");
  controller.setTool("laser");
  assert.equal(state.tool, "rect");
  controller.setColor("blue");
  controller.setColor("pink");
  assert.equal(state.color, "blue");
  controller.setZoom(9);
  assert.equal(state.zoom, 4);
  controller.setZoom(null);
  assert.equal(state.zoom, null);
});

test("marks are added, notes are numbered, typing changes the word, and undo takes the last", async () => {
  const { controller, state, typing } = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE } });
  controller.addMark(arrow);
  controller.setColor("green");
  controller.addNote(10, 20);
  assert.equal(state.focusNote, 1, "the note's field takes the focus once");
  controller.setNoteText(1, "too close\nto the edge");
  assert.equal(state.shapes[1].text, "too close to the edge", "one line");
  assert.equal(state.shapes[1].color, "green");
  controller.addNote(30, 40);
  assert.equal(state.focusNote, 2);
  controller.undo();
  assert.equal(state.shapes.length, 2);
  controller.undo();
  controller.undo();
  controller.undo();
  assert.equal(state.shapes.length, 0);
  assert.ok(typing.length >= 1, "typing is told apart from a change");
});

test("a marked image is not replaced by a paste, and Another image drops it", async () => {
  const { controller, state } = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE } });
  controller.addMark(arrow);
  await controller.openFile(fileOf("image/png", "other.png"));
  assert.equal(state.image.name, "a.png");
  assert.match(state.status.text, /Another image/);
  controller.close();
  assert.equal(state.phase, "empty");
  assert.equal(state.shapes.length, 0);
  await controller.openFile(fileOf("image/png", "other.png"));
  assert.equal(state.image.name, "other.png");
});

test("Send writes the PNG, then the notes, then tells the agent that asked", async () => {
  const { hangar, controller, state, deps } = await opened({
    params: { path: ".hangar/capture.png" },
    caller: { pane: "Ava" },
    images: { ".hangar/capture.png": PICTURE },
  });
  assert.equal(state.target, "Ava");
  controller.addMark(arrow);
  controller.addNote(120, 240);
  controller.setNoteText(1, "too close");
  controller.setComment("Fix the spacing.");
  await controller.send();

  const png = ".hangar/annotations/20261031-231512.png";
  const json = ".hangar/annotations/20261031-231512.json";
  assert.deepEqual(hangar.callsTo("files.writeBinary").map((call) => call.args), [{ path: png, base64: PNG_BASE64 }]);
  assert.deepEqual(hangar.callsTo("files.write").map((call) => call.args.path), [json]);
  assert.deepEqual(deps.exported, [{ scale: 1, shapes: 2 }]);
  const record = JSON.parse(hangar.store.get(json));
  assert.equal(record.image, "20261031-231512.png");
  assert.deepEqual([record.width, record.height, record.source, record.comment], [800, 600, ".hangar/capture.png", "Fix the spacing."]);
  assert.deepEqual(record.notes, [{ n: 1, x: 120, y: 240, text: "too close" }]);
  assert.equal(record.marks.length, 2);

  const [sent] = hangar.callsTo("panes.send");
  assert.equal(sent.args.pane, "Ava");
  assert.match(sent.args.text, new RegExp(`Image: ${png.replace(/\./g, "\\.")}`));
  assert.match(sent.args.text, /Comment: Fix the spacing\./);
  assert.match(sent.args.text, /1\. too close \(x 120, y 240\)/);
  assert.equal(state.status.kind, "success");
  assert.equal(state.status.text, `Sent to Ava: ${png}`);
  assert.equal(state.busy, false);
  // The order is the order of the files: image, notes, message.
  assert.deepEqual(hangar.calls.map((call) => call.method).slice(-3), ["files.writeBinary", "files.write", "panes.send"]);
});

test("the pane the person types is the one told, and without one the files are only saved", async () => {
  const typed = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE } });
  typed.controller.setTarget("  Jack ");
  await typed.controller.send();
  assert.equal(typed.hangar.callsTo("panes.send")[0].args.pane, "Jack");

  const alone = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE } });
  await alone.controller.send();
  assert.equal(alone.hangar.callsTo("panes.send").length, 0);
  assert.equal(alone.hangar.callsTo("files.writeBinary").length, 1);
  assert.equal(alone.state.status.text, "Saved: .hangar/annotations/20261031-231512.png");
});

test("a taken name gets a counter instead of replacing anything", async () => {
  const { hangar, controller, state } = await opened({
    params: { path: "a.png" },
    images: { "a.png": PICTURE },
    texts: { ".hangar/annotations/20261031-231512.png": "taken" },
  });
  await controller.send();
  assert.deepEqual([...hangar.written.keys()], [".hangar/annotations/20261031-231512-2.png"]);
  assert.equal(hangar.store.get(".hangar/annotations/20261031-231512.png"), "taken");
  assert.ok(hangar.store.has(".hangar/annotations/20261031-231512-2.json"));
  assert.match(state.status.text, /-2\.png/);
});

test("a heavy image is written at a smaller scale, and the notes follow the scale", async () => {
  const deps = fakeDeps({ exportBase64: (scale) => (scale > 0.5 ? "A".repeat(logic.BASE64_MAX + 1) : PNG_BASE64) });
  const { hangar, controller, state } = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE }, deps });
  controller.setTarget("Ava");
  controller.addNote(400, 200);
  await controller.send();
  assert.deepEqual(deps.exported.map((entry) => entry.scale), [1, 0.75, 0.5]);
  const record = JSON.parse(hangar.store.get(".hangar/annotations/20261031-231512.json"));
  assert.deepEqual([record.width, record.height], [400, 300]);
  assert.deepEqual([record.notes[0].x, record.notes[0].y], [200, 100]);
  assert.match(hangar.callsTo("panes.send")[0].args.text, /400 × 300 px, reduced from the original/);
  assert.equal(state.status.kind, "success");

  const tooHeavy = fakeDeps({ exportBase64: () => "A".repeat(logic.BASE64_MAX + 1) });
  const never = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE }, deps: tooHeavy });
  await never.controller.send();
  assert.equal(never.hangar.callsTo("files.writeBinary").length, 0);
  assert.match(never.state.status.text, /too heavy/);
});

test("denied writing says what to accept, and nothing is sent", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.png" }, caller: { pane: "Ava" }, images: { "a.png": PICTURE }, deny: ["files.writeBinary"] });
  await controller.send();
  assert.equal(state.status.kind, "error");
  assert.match(state.status.text, /files\.write in Settings › Kits/);
  assert.equal(hangar.callsTo("panes.send").length, 0);
  assert.equal(state.busy, false);
});

test("a Hangar without files.writeBinary is told to update, in French too", async () => {
  const { controller, state } = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE }, fail: { "files.writeBinary": "unknown" }, lang: "fr" });
  await controller.send();
  assert.match(state.status.text, /Mettez Hangar à jour/);
});

test("when the agent cannot be told, the image is still saved and the person is told what to do", async () => {
  const cases = [
    ["denied", /Saved: .*panes\.send in Settings › Kits/],
    ["not_found", /No pane is named Ava/],
    ["unavailable", /Ava cannot take a message/],
    ["rate_limited", /Ava was not told/],
  ];
  for (const [code, pattern] of cases) {
    const { hangar, controller, state } = await opened({ params: { path: "a.png" }, caller: { pane: "Ava" }, images: { "a.png": PICTURE }, fail: { "panes.send": code } });
    await controller.send();
    assert.equal(state.status.kind, "warning", code);
    assert.match(state.status.text, pattern, code);
    assert.equal(hangar.written.size, 1, "the image is saved");
  }
});

test("a notes file that cannot be saved does not lose the image or the message", async () => {
  const { hangar, controller, state } = await opened({ params: { path: "a.png" }, caller: { pane: "Ava" }, images: { "a.png": PICTURE }, fail: { "files.write": "failed" } });
  await controller.send();
  assert.equal(state.status.kind, "warning");
  assert.match(state.status.text, /not its notes file/);
  const [sent] = hangar.callsTo("panes.send");
  assert.doesNotMatch(sent.args.text, /Notes file:/);
});

test("a second Send while one runs does nothing, and there is nothing to send without an image", async () => {
  const empty = await opened();
  await empty.controller.send();
  assert.equal(empty.hangar.callsTo("files.writeBinary").length, 0);

  const { hangar, controller } = await opened({ params: { path: "a.png" }, images: { "a.png": PICTURE } });
  const first = controller.send();
  const second = controller.send();
  await Promise.all([first, second]);
  await settle();
  assert.equal(hangar.callsTo("files.writeBinary").length, 1);
});

test("asked for again, the latest agent is the one to answer, and marks are never lost", async () => {
  const { controller, state, hangar } = await opened({ params: { path: "a.png" }, caller: { pane: "Ava" }, images: { "a.png": PICTURE, "b.png": PICTURE } });
  controller.addMark(arrow);
  await controller.receive({ params: { path: "b.png" }, caller: { pane: "Jack" } });
  assert.equal(state.target, "Jack");
  assert.equal(state.source, "a.png", "marks stay on their image");
  assert.equal(hangar.callsTo("files.readBinary").length, 1);
  controller.undo();
  await controller.receive({ params: { path: "b.png" }, caller: { pane: "Jack" } });
  assert.equal(state.source, "b.png");
  await controller.receive({ params: { path: "b.png" }, caller: { pane: "Jack" } });
  assert.equal(hangar.callsTo("files.readBinary").length, 2, "the same image is not read again");
});

test("an image that arrives late does not replace a newer one", async () => {
  let release;
  const slow = fakeHangar({ params: {}, images: { "a.png": PICTURE } });
  const original = slow.call.bind(slow);
  slow.call = async (method, args) => {
    if (method === "files.readBinary") await new Promise((resolve) => (release = resolve));
    return original(method, args);
  };
  const { controller, state } = await opened({ hangar: slow });
  const loading = controller.loadPath("a.png");
  await settle();
  await controller.openFile(fileOf("image/png", "newer.png"));
  release();
  await loading;
  await settle();
  assert.equal(state.image.name, "newer.png");
  assert.equal(refusal("x").code, "x");
});
