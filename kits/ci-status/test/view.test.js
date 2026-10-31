"use strict";
// What the page draws for each state, on a fake DOM: text, no markup from gh, both languages.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/ci/logic.js");
const view = require("../tabs/ci/view.js");
const { createController } = require("../tabs/ci/controller.js");
const { fixture, fakeClock, fakeHangar, fakeDocument, FakeNode, settle } = require("./helpers.js");

async function drawn({ commands, params = { id: "1" }, lang = "en" }) {
  const clock = fakeClock(Date.parse("2026-10-31T18:48:37Z"));
  const hangar = fakeHangar({ params, commands });
  const root = new FakeNode("div");
  const opened = [];
  const controller = createController({
    hangar, logic, lang, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => view.render(root, state, {
      doc: fakeDocument, logic, lang, now: clock.now,
      actions: { open: (id) => opened.push(id), back: () => undefined, retry: () => undefined },
    }),
  });
  await controller.start();
  await settle();
  controller.dispose();
  return { root, opened, controller };
}
const reply = (name) => ({ exitCode: 0, stdout: fixture(name) });

test("a running run shows its jobs, progress and running step", async () => {
  const { root } = await drawn({ commands: { "ci-run": () => reply("run-running.json") } });
  const text = root.text;
  assert.match(text, /Running/);
  assert.match(text, /rust \(macos-latest\)/);
  assert.match(text, /2\/13 steps/);
  assert.match(text, /▸ /);
  assert.match(text, /Elapsed1m 44s/);
  const bars = root.find((node) => node.tag === "progress");
  assert.equal(bars.length, 4);
  assert.deepEqual([bars[1].max, bars[1].value], [13, 2]);
});

test("a failure shows the failed job's log excerpt", async () => {
  const { root } = await drawn({
    commands: { "ci-run": () => reply("run-failure.json"), "ci-log": () => reply("log-failed.txt") },
  });
  assert.match(root.text, /Failed/);
  assert.match(root.text, /Log of the failed steps/);
  assert.match(root.text, /last 40 of \d+ lines/);
  const pre = root.find((node) => node.tag === "pre")[0];
  assert.ok(pre.children.length > 40);
  assert.ok(pre.find((node) => node.className === "line error").length > 0);
});

test("in French, with accents", async () => {
  const { root } = await drawn({ lang: "fr", commands: { "ci-run": () => reply("run-failure.json"), "ci-log": () => reply("log-failed.txt") } });
  assert.match(root.text, /Échouée/);
  assert.match(root.text, /Journal des étapes en échec/);
  assert.match(root.text, /Durée4m 16s/);
});

test("not logged in: the problem and the fix, in both languages", async () => {
  const commands = { "ci-run": () => ({ exitCode: 4, stderr: fixture("not-logged-in.stderr") }) };
  const en = await drawn({ commands });
  assert.match(en.root.text, /gh is not signed in/);
  assert.match(en.root.text, /gh auth login/);
  assert.equal(en.root.find((node) => node.attributes.role === "alert").length, 1);
  const fr = await drawn({ commands, lang: "fr" });
  assert.match(fr.root.text, /gh n’est pas connecté/);
  assert.match(fr.root.text, /Réessayer/);
});

test("gh absent: tells where to get it", async () => {
  const { root } = await drawn({
    commands: { "ci-run": () => { throw Object.assign(new Error("x"), { code: "not_found" }); } },
  });
  assert.match(root.text, /GitHub CLI \(gh\) not found/);
  assert.match(root.text, /cli\.github\.com/);
});

test("the picker lists runs as buttons that open them", async () => {
  const { root, opened } = await drawn({ params: {}, commands: { "ci-list": () => reply("run-list.json") } });
  assert.match(root.text, /Recent runs/);
  const rows = root.find((node) => node.tag === "button" && /\brun\b/.test(node.className));
  assert.equal(rows.length, 15);
  rows[0].onclick();
  assert.deepEqual(opened, ["41231065926"]);
});

test("what gh says is text, never markup", async () => {
  const hostile = JSON.parse(fixture("run-success.json"));
  hostile.workflowName = "<img src=x onerror=alert(1)>";
  hostile.displayTitle = "<script>alert(1)</script>";
  const { root } = await drawn({ commands: { "ci-run": () => ({ exitCode: 0, stdout: JSON.stringify(hostile) }) } });
  assert.match(root.text, /<script>alert\(1\)<\/script>/);
  assert.equal(root.find((node) => ["img", "script"].includes(node.tag)).length, 0);
});
