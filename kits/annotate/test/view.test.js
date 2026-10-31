"use strict";
// What the page draws for each state, on a fake DOM: Hangar UI's classes, both
// languages, the canvas driven by pointer events, text never parsed as markup.
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const { TAB, lib, logic, model, createController, fakeHangar, fakeDeps, fakeDocument, FakeNode, fileOf, PICTURE, settle } = require("./helpers.js");
const view = require(path.join(TAB, "view.js"));

async function drawn(options = {}) {
  const hangar = options.hangar || fakeHangar(options);
  const lang = options.lang || "en";
  const t = (key, vars) => logic.t(lang, key, vars);
  const root = new FakeNode("div");
  const asked = [];
  const picks = [];
  let refresh = () => undefined;
  const image = { name: "the decoded image" };
  const ctx = {
    doc: fakeDocument,
    win: { innerHeight: 900, devicePixelRatio: 2 },
    t,
    logic,
    model,
    images: () => image,
    pickFile: () => picks.push("pick"),
    confirm: async (...args) => (asked.push(args), options.confirmAnswer ?? false),
  };
  const controller = createController({
    hangar,
    t,
    lib,
    deps: options.deps || fakeDeps(),
    onChange: (state) => {
      refresh = view.render(root, state, ctx);
    },
    onTyping: () => refresh(),
  });
  ctx.actions = controller;
  await controller.start();
  await settle();
  const buttonNamed = (label) => root.find((node) => node.tag === "button" && node.text === label)[0];
  const canvas = () => root.find((node) => node.tag === "canvas")[0];
  const stage = () => root.find((node) => node.className === "annotate-stage")[0];
  return { root, hangar, controller, state: controller.state, buttonNamed, canvas, stage, asked, picks, image, refresh: () => refresh() };
}

const withImage = { params: { path: "a.png" }, images: { "a.png": PICTURE } };
const classed = (root, name) => root.find((node) => node.className.split(" ").includes(name));
const pointer = (x, y) => ({ clientX: x, clientY: y, button: 0, pointerId: 1, preventDefault() {} });

test("with no image: an empty state that says how to open one, and a button that picks a file", async () => {
  const { root, buttonNamed, picks } = await drawn();
  assert.equal(classed(root, "kit-empty__title")[0].text, "No image yet");
  assert.match(root.text, /Ctrl\+V/);
  buttonNamed("Choose an image").onclick();
  assert.deepEqual(picks, ["pick"]);
  assert.equal(classed(root, "annotate-stage").length, 0);
  assert.equal(classed(root, "annotate-foot").length, 0, "nothing to send yet");
});

test("in French, with accents", async () => {
  const { root, buttonNamed } = await drawn({ ...withImage, lang: "fr" });
  assert.equal(classed(root, "kit-header__title")[0].text, "Annoter");
  for (const label of ["Flèche", "Rectangle", "Main levée", "Note", "Annuler", "Ajuster", "Autre image"]) {
    assert.ok(root.find((node) => node.tag === "button" && node.text === label).length, label);
  }
  assert.ok(buttonNamed("Enregistrer"), "no pane to send to yet");
  assert.match(root.text, /Commentaire \(facultatif\)/);
  assert.match(root.text, /Sans nom de pane/);
});

test("with an image: tools as toggles, five colours, undo, zoom, the canvas, and one primary action", async () => {
  const { root, buttonNamed, state, canvas } = await drawn(withImage);
  const chips = classed(root, "kit-filter");
  assert.deepEqual(chips.map((chip) => chip.text), ["Arrow", "Rectangle", "Freehand", "Note"]);
  assert.deepEqual(chips.map((chip) => chip.getAttribute("aria-pressed")), ["true", "false", "false", "false"]);
  const swatches = classed(root, "annotate-swatch");
  assert.equal(swatches.length, 5);
  assert.deepEqual(swatches.map((swatch) => swatch.getAttribute("aria-label")), ["Red", "Orange", "Green", "Blue", "Magenta"]);
  assert.deepEqual(swatches.map((swatch) => swatch.style.background), model.COLORS.map((color) => color.value));
  assert.equal(swatches[0].getAttribute("aria-pressed"), "true");
  assert.equal(buttonNamed("Undo").disabled, true, "nothing to undo");
  assert.ok(buttonNamed("Fit"));
  assert.equal(classed(root, "kit-btn--primary").length, 1);
  assert.equal(classed(root, "kit-btn--primary")[0].text, "Save");
  assert.match(classed(root, "kit-header__state")[0].text, /a\.png · 800 × 600 px/);
  // Fitted to the stage (a fake with no width: the image at its own size), with the
  // backing store sized for the screen's pixel ratio.
  assert.equal(canvas().width, 1600);
  assert.equal(canvas().height, 1200);
  assert.equal(state.zoom, null);
});

test("choosing a tool or a colour redraws with it pressed, and the tip follows the tool", async () => {
  const { root, buttonNamed, state } = await drawn(withImage);
  buttonNamed("Rectangle").onclick();
  assert.equal(state.tool, "rect");
  assert.equal(classed(root, "kit-filter").find((chip) => chip.text === "Rectangle").getAttribute("aria-pressed"), "true");
  assert.match(classed(root, "annotate-tip")[0].text, /frame/);
  classed(root, "annotate-swatch")[3].onclick();
  assert.equal(state.color, "blue");
  assert.equal(classed(root, "annotate-swatch")[3].getAttribute("aria-pressed"), "true");
});

test("dragging with the arrow tool draws a draft, then keeps the mark; a speck keeps nothing", async () => {
  const { canvas, state, root } = await drawn(withImage);
  const target = canvas();
  target.style.width = "800px";
  target.style.height = "600px";
  target.onpointerdown(pointer(10, 10));
  target.onpointermove(pointer(200, 120));
  assert.ok(target.context.log.some((entry) => entry[0] === "moveTo"), "the draft shows while dragging");
  assert.equal(state.shapes.length, 0);
  target.onpointerup(pointer(210, 130));
  assert.deepEqual(state.shapes.map((shape) => [shape.type, shape.x1, shape.y1, shape.x2, shape.y2]), [["arrow", 10, 10, 210, 130]]);

  const again = canvas();
  again.onpointerdown(pointer(50, 50));
  again.onpointerup(pointer(51, 51));
  assert.equal(state.shapes.length, 1, "too small to mean anything");
  assert.equal(root.find((node) => node.tag === "button" && node.text === "Undo")[0].disabled, false);
});

test("the freehand tool keeps the stroke", async () => {
  const { canvas, state, buttonNamed } = await drawn(withImage);
  buttonNamed("Freehand").onclick();
  const target = canvas();
  target.style.width = "800px";
  target.style.height = "600px";
  target.onpointerdown(pointer(0, 0));
  for (const [x, y] of [[20, 5], [40, 30], [60, 10]]) target.onpointermove(pointer(x, y));
  target.onpointerup(pointer(80, 40));
  assert.equal(state.shapes[0].type, "free");
  assert.ok(state.shapes[0].points.length >= 4);
});

test("a click with the note tool places a numbered note, and its word is typed in the list", async () => {
  const { root, canvas, state, buttonNamed } = await drawn(withImage);
  assert.match(root.text, /Notes you place on the image are listed here/);
  buttonNamed("Note").onclick();
  const target = canvas();
  target.style.width = "800px";
  target.style.height = "600px";
  target.onpointerdown(pointer(300, 200));
  assert.deepEqual([state.shapes[0].type, state.shapes[0].x, state.shapes[0].y], ["note", 300, 200]);
  const [field] = root.find((node) => node.tag === "input" && node.getAttribute("aria-label") === "Note 1");
  assert.ok(field, "a field for the note");
  assert.equal(fakeDocument.activeElement, field, "it takes the focus, because the person just clicked");
  field.value = "<b>too close</b>";
  field.oninput();
  assert.equal(state.shapes[0].text, "<b>too close</b>");
  assert.equal(canvas().context.log.filter((entry) => entry[0] === "fillText").map((entry) => entry[1]).includes("<b>too close</b>"), true, "redrawn with its word");
  const rendered = root.find((node) => node.tag === "input" && node.getAttribute("aria-label") === "Note 1")[0];
  assert.equal(rendered, field, "typing does not rebuild the field");
});

test("the send button says Send once there is a pane to tell, and Sending while it works", async () => {
  const { root, controller, state } = await drawn({ ...withImage, caller: { pane: "Ava" } });
  const button = () => classed(root, "kit-btn--primary")[0];
  assert.equal(button().text, "Send");
  assert.match(root.text, /Ava asked for this image/);
  controller.setTarget("");
  assert.equal(button().text, "Save", "typing a pane name changes it without a redraw");
  controller.setTarget("Jack");
  assert.equal(button().text, "Send");
  assert.equal(state.busy, false);
});

test("the sent answer shows in the foot, an error in the danger colour's class", async () => {
  const { root, controller } = await drawn({ ...withImage, caller: { pane: "Ava" } });
  await controller.send();
  assert.match(classed(root, "annotate-foot__status")[0].text, /^Sent to Ava: \.hangar\/annotations\/\d{8}-\d{6}\.png$/);
  const denied = await drawn({ ...withImage, deny: ["files.writeBinary"] });
  await denied.controller.send();
  const status = classed(denied.root, "annotate-foot__status")[0];
  assert.ok(status.className.includes("annotate-foot__status--bad"));
  assert.equal(status.getAttribute("role"), "status");
});

test("denied reading is a designed state with a way to pick a file anyway", async () => {
  const { root, picks } = await drawn({ params: { path: "a.png" }, deny: ["files.readBinary"] });
  assert.equal(classed(root, "kit-empty__title")[0].text, "This tab may not read files");
  assert.ok(classed(root, "kit-icon--lock").length);
  classed(root, "kit-btn--primary")[0].onclick();
  assert.deepEqual(picks, ["pick"]);
});

test("a path not found is an error banner, with the empty state below it", async () => {
  const { root } = await drawn({ params: { path: "nope.png" } });
  const banner = classed(root, "kit-banner--error")[0];
  assert.match(banner.text, /nope\.png was not found/);
  assert.equal(banner.getAttribute("role"), "alert");
  assert.ok(classed(root, "kit-empty").length);
});

test("Another image asks before dropping marks, and not otherwise", async () => {
  const bare = await drawn(withImage);
  await bare.buttonNamed("Another image").onclick();
  assert.equal(bare.state.phase, "empty");
  assert.equal(bare.asked.length, 0);

  const marked = await drawn({ ...withImage, confirmAnswer: false });
  marked.controller.addMark({ type: "arrow", x1: 0, y1: 0, x2: 90, y2: 90, color: "red" });
  await marked.buttonNamed("Another image").onclick();
  assert.equal(marked.asked.length, 1);
  assert.equal(marked.state.phase, "ready", "kept");
  const yes = await drawn({ ...withImage, confirmAnswer: true });
  yes.controller.addMark({ type: "arrow", x1: 0, y1: 0, x2: 90, y2: 90, color: "red" });
  await yes.buttonNamed("Another image").onclick();
  assert.equal(yes.state.phase, "empty");
});

test("zoom steps from the fitted zoom, and Fit goes back", async () => {
  const { buttonNamed, state, root } = await drawn(withImage);
  const zoomIn = root.find((node) => node.tag === "button" && node.getAttribute("aria-label") === "Zoom in")[0];
  zoomIn.onclick();
  assert.equal(state.zoom, 1.5);
  assert.equal(classed(root, "annotate-zoom__value")[0].text, "150 %");
  root.find((node) => node.tag === "button" && node.getAttribute("aria-label") === "Zoom out")[0].onclick();
  assert.equal(state.zoom, 1);
  buttonNamed("Fit").onclick();
  assert.equal(state.zoom, null);
});

test("every control can be reached by keyboard: real buttons and inputs, named", async () => {
  const { root } = await drawn(withImage);
  const controls = root.find((node) => ["button", "input", "textarea"].includes(node.tag));
  assert.ok(controls.length >= 15);
  for (const control of controls) {
    const name = control.text || control.getAttribute("aria-label") || control.title;
    assert.ok(name, `${control.tag} ${control.className} has no name`);
    if (control.tag === "button") assert.equal(control.type, "button");
  }
  assert.equal(classed(root, "annotate-stage")[0].getAttribute("tabindex"), "0");
});

test("a pasted image from the person, not a path, shows its own name", async () => {
  const { controller, root } = await drawn();
  await controller.openFile(fileOf("image/png", "shot <1>.png"));
  assert.match(classed(root, "kit-header__state")[0].text, /shot <1>\.png · 800 × 600 px/);
});
