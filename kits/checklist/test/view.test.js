"use strict";
// What the page draws on a fake DOM: Hangar UI's classes, text and never markup, both
// languages, every state designed, and the presses that reach the controller.
const test = require("node:test");
const assert = require("node:assert/strict");
const { drawn, settle, LIST_PATH } = require("./helpers.js");

const buttons = (root) => root.find((node) => node.tag === "button");
const byText = (root, text) => buttons(root).find((node) => node.text.trim() === text);
const texts = (root, cls) => root.find((node) => node.has(cls)).map((node) => node.text.trim());
const steps = (root) => root.find((node) => node.has("chk-step"));

test("a list draws its title, a progress bar, each step with three choices", async () => {
  const { root } = await drawn();
  assert.deepEqual(texts(root, "kit-header__title"), ["Login and logout"]);
  assert.match(texts(root, "kit-state")[0], /0 of 3 answered/);
  const bar = root.find((node) => node.getAttribute("role") === "progressbar")[0];
  assert.equal(bar.getAttribute("aria-valuemax"), "3");
  assert.equal(steps(root).length, 3);
  assert.equal(texts(root, "kit-row__sub")[0], "Expected: An error shows under the field.");
  const choices = root.find((node) => node.has("kit-filter"));
  assert.equal(choices.length, 9);
  assert.ok(choices.every((node) => node.getAttribute("aria-pressed") === "false"));
});

test("pressing a choice answers, a second press clears it, Problem opens a note", async () => {
  const { root, controller } = await drawn();
  byText(steps(root)[0], "OK").onclick();
  assert.equal(controller.state.counts.ok, 1);
  assert.equal(root.find((node) => node.has("kit-filter") && node.getAttribute("aria-pressed") === "true").length, 1);
  byText(steps(root)[0], "OK").onclick();
  assert.equal(controller.state.counts.ok, 0, "pressing OK again clears it");
  byText(steps(root)[1], "Problem").onclick();
  assert.equal(root.find((node) => node.tag === "textarea").length, 1, "a failed step asks what went wrong");
  assert.equal(controller.state.counts.fail, 1);
});

test("the note of a failed step is what the agent will read", async () => {
  const { root, hangar, controller } = await drawn();
  byText(steps(root)[1], "Problem").onclick();
  const note = root.find((node) => node.tag === "textarea")[0];
  assert.equal(note.getAttribute("data-focus"), "note-1");
  note.value = "Blank page.";
  note.oninput();
  assert.equal(root.find((node) => node.tag === "textarea")[0], note, "typing redraws nothing");
  await controller.send();
  assert.match(hangar.callsTo("panes.send")[0].args.text, /Note: Blank page\./);
});

test("the send button is for the agent that asked, primary once everything is answered", async () => {
  const { root, controller } = await drawn();
  const send = () => byText(root, "Send to agent");
  assert.equal(send().disabled, true);
  assert.match(texts(root, "chk-foot__hint")[0], /Answer at least one step/);
  controller.answer(0, "ok");
  assert.equal(send().disabled, false);
  assert.doesNotMatch(send().className, /primary/);
  assert.match(texts(root, "chk-foot__hint")[0], /2 steps are still unanswered/);
  controller.answer(1, "ok");
  controller.answer(2, "ok");
  assert.match(send().className, /primary/);
  assert.equal(texts(root, "chk-foot__hint")[0], "");
  await send().onclick();
  await settle();
  assert.match(texts(root, "kit-banner")[0], /Sent to Ava/);
  assert.ok(byText(root, "Send again"));
});

test("a refused send is an alert with what to do", async () => {
  const { root, controller } = await drawn({ fail: { "panes.send": "not_found" } });
  controller.answer(0, "ok");
  await controller.send();
  const alert = root.find((node) => node.getAttribute("role") === "alert")[0];
  assert.match(alert.text, /no longer in this workspace/);
  assert.ok(byText(root, "Send to agent"), "and it can be tried again");
});

test("opened by the person, there is no agent to send to: the summary is copied instead", async () => {
  const { root, copied, controller } = await drawn({ caller: null });
  assert.equal(byText(root, "Send to agent"), undefined);
  assert.match(texts(root, "chk-foot__hint")[0], /No agent asked for this list/);
  assert.equal(byText(root, "Copy the summary").disabled, true);
  controller.answer(0, "fail");
  assert.equal(byText(root, "Copy the summary").disabled, false);
  assert.ok(copied.at(-1).source().includes("FAILED"));
});

test("a step to redo shows what failed last time", async () => {
  const { root, controller, hangar } = await drawn();
  controller.answer(1, "fail");
  controller.setNote(1, "Blank page.");
  await controller.send();
  hangar.emit("tab.params", { params: { path: LIST_PATH }, caller: { pane: "Ava" } });
  await settle();
  assert.deepEqual(texts(root, "kit-state--waiting"), ["To redo"]);
  assert.ok(texts(root, "kit-row__sub").includes("It failed last time: Blank page."));
});

test("the header says a failure, a clean list, and a list with nothing failed but skips", async () => {
  const { root, controller } = await drawn();
  for (let i = 0; i < 3; i++) controller.answer(i, i === 1 ? "fail" : "ok");
  assert.equal(texts(root, "kit-state")[0], "1 step failed");
  assert.equal(texts(root, "kit-state--failure").length, 1);
  controller.answer(1, "ok");
  assert.equal(texts(root, "kit-state")[0], "Everything passed");
  controller.answer(2, "skip");
  assert.equal(texts(root, "kit-state")[0], "Nothing failed");
});

test("every refusal is an empty state that says what is wrong and what to do", async () => {
  const cases = [
    [{ params: {} }, "No list to show"],
    [{ params: { path: "../x" } }, "This path cannot be used"],
    [{ params: { path: "nope.json" } }, "List not found"],
    [{ params: { path: "a.json" }, files: { "a.json": "{x" } }, "The list is not valid JSON"],
    [{ params: { path: "a.json" }, files: { "a.json": '{"steps":[{"text":"ok"},{}]}' } }, "The list is not in the expected shape"],
    [{ params: { path: "a.json" }, files: { "a.json": "[]" }, deny: ["files.read"] }, "This tab may not read files"],
  ];
  for (const [options, title] of cases) {
    const { root } = await drawn(options);
    assert.deepEqual(texts(root, "kit-empty__title"), [title]);
    assert.ok(root.find((node) => node.getAttribute("role") === "alert").length);
    const retry = root.find((node) => node.has("kit-btn")).length > 0;
    assert.equal(retry, !["No list to show", "This path cannot be used"].includes(title), title);
  }
  const shape = await drawn(cases[4][0]);
  assert.match(texts(shape.root, "kit-empty__text")[0], /Step 2 has no text\./);
});

test("saving denied is a warning above the list, and the list still works", async () => {
  const { root, controller } = await drawn({ deny: ["data.read", "data.write"] });
  assert.match(texts(root, "kit-banner--warning")[0], /cannot be saved/);
  controller.answer(0, "ok");
  assert.equal(controller.state.counts.ok, 1);
});

test("French, with its accents and the longer words", async () => {
  const { root } = await drawn({ lang: "fr" });
  assert.match(texts(root, "kit-state")[0], /0 sur 3 traitées/);
  assert.ok(byText(root, "Problème") && byText(root, "Passer"));
  assert.ok(texts(root, "kit-row__sub")[0].startsWith("Attendu : "));
  assert.match(texts(root, "chk-foot__hint")[0], /Répondez à au moins une étape/);
  assert.ok(byText(root, "Envoyer à l'agent"));
});

test("nothing from the list or an answer is ever turned into markup", async () => {
  const hostile = { title: "<img src=x onerror=alert(1)>", steps: [{ text: "<b>bold</b>", expect: "<script>x</script>" }] };
  const { root } = await drawn({ files: { [LIST_PATH]: JSON.stringify(hostile) } });
  assert.equal(root.find((node) => ["img", "b", "script"].includes(node.tag)).length, 0);
  assert.equal(texts(root, "kit-header__title")[0], hostile.title);
});
