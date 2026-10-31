"use strict";
// What the page draws for each state, on a fake DOM: values masked until revealed,
// text never parsed as markup, both languages, Hangar UI's classes.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { KIT, TAB, lib, logic, createController, fakeHangar, fakeDocument, FakeNode, row, settle } = require("./helpers.js");
const view = require(path.join(TAB, "view.js"));

const SECRET = "s3cr3t-value-XqZ9";

async function drawn(options = {}) {
  const hangar = options.hangar || fakeHangar(options);
  const lang = options.lang || "en";
  const t = (key, vars) => logic.t(lang, key, vars);
  const root = new FakeNode("div");
  const asked = [];
  let refresh = () => undefined;
  const ctx = { doc: fakeDocument, t, logic, gitignore: lib.gitignore, confirm: async (...args) => (asked.push(args), options.confirmAnswer ?? false) };
  const controller = createController({
    hangar, lib, t,
    onChange: (state) => {
      refresh = view.render(root, state, ctx);
    },
    onTyping: () => refresh(),
  });
  ctx.actions = controller;
  await controller.start();
  await settle();
  const buttonNamed = (label) => root.find((node) => node.tag === "button" && node.text.includes(label))[0];
  return { root, hangar, controller, state: controller.state, buttonNamed, asked, refresh: () => refresh() };
}

const inputOf = (root, key) => root.find((node) => (node.tag === "input" || node.tag === "textarea") && node.getAttribute("aria-label") === key)[0];
const classed = (root, name) => root.find((node) => node.className.split(" ").includes(name));

test("a value is masked until its row is revealed, and shows nowhere else", async () => {
  const { root, controller, state } = await drawn({
    params: { path: ".env" }, files: { ".env": `# about it\nAPI_KEY=${SECRET}\n# OLD=${SECRET}\n`, ".gitignore": ".env\n" },
  });
  const input = inputOf(root, "API_KEY");
  assert.equal(input.type, "password");
  assert.equal(input.value, SECRET, "the field holds it, and the browser masks it");
  assert.equal(input.getAttribute("autocomplete"), "new-password");
  assert.equal(root.text.includes(SECRET), false, "no text node holds it");
  const shown = root.everything().filter((item) => item.includes(SECRET));
  assert.deepEqual(shown, [SECRET], "only that field's value, not a title, a hint or a comment");

  controller.toggleReveal("API_KEY");
  assert.equal(inputOf(root, "API_KEY").type, "text");
  assert.equal(row(state, "API_KEY").revealed, true);
});

test("a value over several lines stays hidden, and is edited whole once shown", async () => {
  const { root, controller } = await drawn({ params: { path: ".env" }, files: { ".env": `KEY='first\n${SECRET}'\n`, ".gitignore": ".env\n" } });
  const hidden = inputOf(root, "KEY");
  assert.equal(hidden.disabled, true);
  assert.equal(hidden.value, undefined, "a password field would drop its line breaks");
  assert.ok(!root.everything().some((item) => item.includes(SECRET)));
  controller.toggleReveal("KEY");
  const area = inputOf(root, "KEY");
  assert.equal(area.tag, "textarea");
  assert.equal(area.value, `first\n${SECRET}`);
});

test("asked keys come first with their state, then the file's, and why at the top", async () => {
  const { root } = await drawn({
    params: { path: "sub/.env", keys: "MISSING,DONE", why: "For the <b>tests</b>." },
    caller: { pane: "Asker" },
    files: { "sub/.env": "OTHER=1\nDONE=x\n", "sub/.env.example": "# Offer hint\nOFFERED=y\n", ".gitignore": ".env\n" },
  });
  const text = root.text;
  assert.match(text, /Asker asks:/);
  assert.match(text, /For the <b>tests<\/b>\./, "shown as written, not as markup");
  assert.equal(classed(root, "kit-header__title")[0].text, ".env");
  assert.match(classed(root, "kit-header__state")[0].text, /1 missing.*sub\//);
  const titles = classed(root, "kit-section__title").map((node) => node.text);
  assert.deepEqual(titles, ["Asked for", "In this file", "In .env.example, missing here", "Add a key"]);
  const keys = classed(root, "kit-row__title").map((node) => node.text);
  assert.deepEqual(keys, ["MISSING", "DONE", "OTHER", "OFFERED"]);
  assert.match(text, /Offer hint/);
  assert.equal(classed(root, "kit-state--waiting").filter((node) => node.text === "missing").length, 1);
  assert.equal(classed(root, "kit-state--success").filter((node) => node.text === "set").length, 1);
});

test("the warning says it plainly, and the button is what writes", async () => {
  const { root, hangar, buttonNamed } = await drawn({ params: { path: ".env" }, files: { ".env": "A=1\n", ".gitignore": "x\n" } });
  assert.match(root.text, /\.env is not ignored by git: git would commit it, secrets included\./);
  assert.equal(classed(root, "kit-banner--warning").length, 1);
  assert.equal(hangar.callsTo("files.write").length, 0);
  await buttonNamed("Add .env to .gitignore").onclick();
  assert.equal(hangar.store.get(".gitignore"), "x\n.env\n");
  assert.equal(classed(root, "kit-banner--warning").length, 0);
});

test("typing refreshes what depends on it and leaves the field alone", async () => {
  const { root, controller, buttonNamed } = await drawn({ params: { path: ".env", keys: "A" }, files: { ".env": "A=\n", ".gitignore": ".env\n" } });
  const input = inputOf(root, "A");
  assert.equal(buttonNamed("Save").disabled, true, "nothing to save");
  assert.match(classed(root, "kit-header__state")[0].text, /1 missing/);
  input.value = "typed";
  input.oninput();
  assert.equal(inputOf(root, "A"), input, "the same element: the caret stays where it is");
  assert.equal(buttonNamed("Save").disabled, false);
  assert.doesNotMatch(classed(root, "kit-header__state")[0].text, /missing/);
  assert.equal(classed(root, "kit-chip")[0].text, "changed");
  await buttonNamed("Save").onclick();
  await settle();
  assert.equal(controller.state.saved, true);
  assert.match(classed(root, "kit-header__state")[0].text, /Saved/);
});

test("one primary action at a time: save, or tell the agent there is nothing to change", async () => {
  const { root, controller } = await drawn({ params: { path: ".env" }, caller: { pane: "Asker" }, files: { ".env": "A=1\n", ".gitignore": ".env\n" } });
  const primary = () => classed(root, "kit-btn--primary");
  assert.deepEqual(primary().map((node) => node.text), ["Tell Asker: nothing to change"]);
  controller.editRow("A", "2");
  assert.deepEqual(primary().map((node) => node.text), ["Save"]);
  await primary()[0].onclick();
  await settle();
  assert.match(root.text, /Asker was told, without any value\./);
  assert.equal(root.text.includes("nothing to change"), false);
});

test("an agent that could not be told is said, with a way to try again", async () => {
  const { root, hangar, buttonNamed } = await drawn({ params: { path: ".env" }, caller: { pane: "Asker" }, files: { ".env": "A=1\n", ".gitignore": ".env\n" }, sendError: "not_found" });
  const input = inputOf(root, "A");
  input.value = "2";
  input.oninput();
  await buttonNamed("Save").onclick();
  await settle();
  assert.match(root.text, /Saved, but Asker could not be told: that pane is gone\. No value was sent\./);
  hangar.sendError = null;
  await buttonNamed("Tell Asker again").onclick();
  await settle();
  assert.match(root.text, /Asker was told/);
});

test("the editor: line numbers, the line asked for marked, and the check above it", async () => {
  const body = Array.from({ length: 12 }, (_, i) => `"k${i}": ${i},`).join("\n");
  const { root, controller, state } = await drawn({ params: { path: "conf.json", line: "5", why: "Check the timeout." }, files: { "conf.json": `{\n${body}\n}\n` } });
  const gutter = classed(root, "editor-gutter")[0];
  assert.match(gutter.text, /^1\n2\n3\n4\n5\n6/);
  assert.equal(classed(gutter, "editor-gutter__mark")[0].text, "5");
  assert.equal(gutter.attributes["aria-hidden"], "true");
  const area = classed(root, "editor-area")[0];
  assert.equal(area.wrap, "off");
  assert.equal(area.scrollTop, 18, "scrolled to it, three lines above");
  assert.equal(state.text.jump, null);
  assert.equal(fakeDocument.activeElement === area, false, "and the keyboard stays where it was");
  assert.match(root.text, /Broken JSON, line \d+/);
  assert.match(root.text, /Check the timeout\./);

  area.value = '{"a": 1}\n';
  area.oninput();
  assert.match(root.text, /Valid JSON/);
  assert.equal(classed(root, "editor-area")[0], area, "the same element");
  assert.match(gutter.text, /^1\n2\n$/);
  assert.ok(controller.isDirty());
});

test("the review: signs as well as colours, then a broken file asks", async () => {
  const { root, controller, buttonNamed, hangar } = await drawn({ params: { path: "a.json" }, files: { "a.json": '{"a": 1}\n' } });
  const area = classed(root, "editor-area")[0];
  area.value = '{"a": 2,}\n';
  area.oninput();
  await buttonNamed("Review changes").onclick();
  assert.match(root.text, /− \{"a": 1\}/);
  assert.match(root.text, /\+ \{"a": 2,\}/);
  assert.equal(classed(root, "kit-log__line--error").length, 1);
  assert.equal(classed(root, "editor-add").length, 1);
  assert.equal(buttonNamed("Save").disabled, true, "broken, and not said so");
  const toggle = root.find((node) => node.tag === "input" && node.type === "checkbox")[0];
  assert.match(root.text, /I know this file is broken: save it anyway\./);
  toggle.checked = true;
  toggle.onchange();
  assert.equal(buttonNamed("Save").disabled, false);
  await buttonNamed("Save").onclick();
  await settle();
  assert.equal(hangar.store.get("a.json"), '{"a": 2,}\n');
  assert.equal(controller.state.text.stage, "edit");
});

test("the text conflict shows both sides and three ways out", async () => {
  const { root, hangar, controller, buttonNamed } = await drawn({ params: { path: "a.txt" }, files: { "a.txt": "base\n" } });
  controller.editText("mine\n");
  controller.review();
  hangar.store.set("a.txt", "theirs\n");
  await controller.saveText();
  await settle();
  assert.match(root.text, /This file changed while you were editing it\. Nothing was written\./);
  assert.match(root.text, /− theirs/);
  assert.match(root.text, /\+ mine/);
  assert.match(classed(root, "kit-state--failure")[0].text, /Changed on disk/);
  await buttonNamed("Keep what is on disk").onclick();
  await settle();
  assert.equal(controller.state.text.value, "theirs\n");
  assert.equal(root.text.includes("Replace it with mine"), false);
});

test("the dotenv conflict keeps values hidden until asked, on both sides", async () => {
  const { root, hangar, controller, buttonNamed, state } = await drawn({ params: { path: ".env" }, files: { ".env": "A=1\n", ".gitignore": ".env\n" } });
  controller.editRow("A", `${SECRET}-mine`);
  hangar.store.set(".env", `A=${SECRET}-theirs\nB=2\n`);
  await controller.saveForm();
  await settle();
  assert.ok(!root.everything().some((item) => item.includes(SECRET)), "both masked");
  assert.match(root.text, new RegExp(view.MASK));
  assert.match(root.text, /Yours/);
  assert.match(root.text, /On disk/);
  assert.match(root.text, /1 other key changed on disk/);

  await buttonNamed("Show").onclick();
  assert.match(root.text, new RegExp(`${SECRET}-mine`));
  assert.match(root.text, new RegExp(`${SECRET}-theirs`));
  const radios = root.find((node) => node.tag === "input" && node.type === "radio");
  assert.deepEqual(radios.map((node) => node.checked), [true, false]);
  radios[1].onchange();
  assert.equal(state.conflict.rows[0].choice, "theirs");
  await buttonNamed("Save with these choices").onclick();
  await settle();
  assert.equal(hangar.store.get(".env"), `A=${SECRET}-theirs\nB=2\n`);
});

test("the picker groups, filters as you type, and opens what is clicked", async () => {
  const { root, controller, state } = await drawn({ files: { ".env.local": "A=1\n", "app.yaml": "a: 1\n", "src/x.ts": "", ".gitignore": ".env.local\n" } });
  assert.deepEqual(classed(root, "kit-section__title").map((node) => node.text), ["Environment files", "Configuration files", "Other files"]);
  assert.match(root.text, /new/, ".env is offered though it is not there");
  const search = root.find((node) => node.tag === "input" && node.type === "search")[0];
  search.value = "app";
  search.oninput();
  assert.equal(root.find((node) => node.tag === "input" && node.type === "search")[0], search, "the same element");
  assert.deepEqual(classed(root, "kit-section__title").map((node) => node.text), ["Configuration files"]);
  search.value = "nothing like it";
  search.oninput();
  assert.match(root.text, /No file matches\./);
  search.value = "";
  search.oninput();
  const row = root.find((node) => node.tag === "button" && node.title === ".env.local")[0];
  await row.onclick();
  await settle();
  assert.equal(state.view, "dotenv");
  assert.ok(root.text.includes("← Files"));
  assert.ok(controller);
});

test("back with edits asks in place, without one it just goes", async () => {
  const stay = await drawn({ files: { "a.txt": "x\n" }, confirmAnswer: false });
  await stay.root.find((node) => node.title === "a.txt")[0].onclick();
  await settle();
  stay.controller.editText("changed\n");
  await stay.buttonNamed("← Files").onclick({ currentTarget: "the button" });
  await settle();
  assert.equal(stay.asked[0][0], "the button");
  assert.equal(stay.asked[0][1].message, "Your changes are not saved.");
  assert.equal(stay.state.view, "text");
  assert.equal(stay.state.text.value, "changed\n");

  const leave = await drawn({ files: { "a.txt": "x\n" }, confirmAnswer: true });
  await leave.root.find((node) => node.title === "a.txt")[0].onclick();
  await settle();
  leave.controller.editText("changed\n");
  await leave.buttonNamed("← Files").onclick({ currentTarget: "the button" });
  await settle();
  assert.equal(leave.state.view, "picker");

  const clean = await drawn({ files: { "a.txt": "x\n" }, confirmAnswer: false });
  await clean.root.find((node) => node.title === "a.txt")[0].onclick();
  await settle();
  await clean.buttonNamed("← Files").onclick();
  await settle();
  assert.equal(clean.state.view, "picker");
  assert.equal(clean.asked.length, 0);
});

test("denied and failed are designed states, each saying what to do", async () => {
  const denied = await drawn({ params: { path: ".env" }, deny: ["files.read"] });
  assert.match(denied.root.text, /This tab may not read or write here/);
  assert.match(denied.root.text, /Accept files\.read and files\.write/);
  assert.equal(classed(denied.root, "kit-icon--lock").length, 1);
  assert.equal(classed(denied.root, "kit-empty").length, 1);
  assert.ok(denied.buttonNamed("Try again"));

  const bad = await drawn({ params: { path: "../x" } });
  assert.match(bad.root.text, /This path cannot be opened/);
  assert.equal(bad.buttonNamed("Try again"), undefined, "trying again would change nothing");
  await bad.buttonNamed("Choose a file").onclick();
  await settle();
  assert.equal(bad.state.view, "picker", "the picker is the way on");
});

test("in French, with accents", async () => {
  const { root, controller } = await drawn({
    lang: "fr", params: { path: ".env", keys: "A,B" }, caller: { pane: "Asker" }, files: { ".env": "C=1\n", ".gitignore": "x\n" },
  });
  const text = root.text;
  assert.match(text, /Dans ce fichier/);
  assert.match(text, /Demandées/);
  assert.match(text, /manquante/);
  assert.match(text, /n'est pas ignoré par git/);
  controller.editRow("A", "x");
  assert.match(root.text, /Enregistrer/);
  assert.match(root.text, /modifiée/);
});

test("every word the page says exists in both languages", () => {
  const { en, fr } = logic.STRINGS;
  assert.deepEqual(Object.keys(fr).sort(), Object.keys(en).sort());
  const accents = /[àâçéèêëîïôûùüœ]/i;
  assert.ok(Object.values(fr).filter((text) => accents.test(text)).length > 60, "written with its accents");
  for (const file of ["view.js", "controller.js"]) {
    const source = fs.readFileSync(path.join(TAB, file), "utf8");
    for (const [, key] of source.matchAll(/\bt\("([\w.-]+)"/g)) {
      assert.ok(en[key] !== undefined || en[`${key}_one`] !== undefined, `${file}: ${key}`);
    }
  }
  for (const code of ["json", "yaml-tab", "yaml-quote", "yaml-bracket", "yaml-reserved", "toml-trailing", "toml-string", "toml-escape", "toml-key", "toml-equals", "toml-value", "toml-header", "toml-duplicate"]) {
    assert.ok(en[`check.${code}`], code);
  }
  const validateSource = fs.readFileSync(path.join(TAB, "validate.js"), "utf8");
  for (const [, code] of validateSource.matchAll(/["'`](yaml-[a-z]+|toml-[a-z]+)["'`]/g)) assert.ok(en[`check.${code}`], code);
  for (const kind of ["denied", "invalid", "bad-path", "failed", "unavailable", "timeout", "error"]) {
    assert.ok(en[`err.${kind}.title`] && en[`err.${kind}.hint`], kind);
  }
});

test("Hangar UI's classes, and no colour of its own", () => {
  const css = fs.readFileSync(path.join(TAB, "style.css"), "utf8");
  assert.doesNotMatch(css, /#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(|\boklch\(|(?<![-\w])(?:white|black|red|green|blue|gray|grey)(?![-\w])/i, "a colour of its own");
  assert.doesNotMatch(css, /\bCanvas(Text)?\b/);
  for (const [, token] of css.matchAll(/var\((--[\w-]+)/g)) assert.match(token, /^--kit-/, token);
  const html = fs.readFileSync(path.join(TAB, "index.html"), "utf8");
  assert.match(html, /href="\/_hangar\/kit\.css"/);
  assert.match(html, /src="\/_hangar\/kit\.js"/);
  assert.match(html, /src="\/_hangar\/kit-ui\.js"/);
  assert.ok(html.indexOf("/_hangar/kit.js") < html.indexOf("/_hangar/kit-ui.js"), "the SDK first");
  const sheet = path.join(KIT, "..", "..", "src-tauri", "kit-sdk", "kit.css");
  if (!fs.existsSync(sheet)) return; // a kit installed on its own has no sources beside it
  const known = fs.readFileSync(sheet, "utf8");
  const source = fs.readFileSync(path.join(TAB, "view.js"), "utf8");
  for (const [name] of new Set(source.matchAll(/\bkit-[a-z]+(?:[_-]+[a-z]+)*/g))) {
    if (name === "kit-ui") continue; // docs/kit-ui.md, in a comment
    assert.ok(known.includes(`.${name}`), `${name} is not a class of Hangar UI`);
  }
  for (const state of ["waiting", "success", "neutral", "failure", "running"]) assert.ok(known.includes(`.kit-state--${state}`), state);
  for (const icon of ["lock", "alert"]) assert.ok(known.includes(`.kit-icon--${icon}`), icon);
});
