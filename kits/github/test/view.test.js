"use strict";
// What the view draws from the controller's state, with a fake DOM: Hangar UI's classes,
// no colour of its own, text from GitHub only ever as text, and the problems said with their fix.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const logic = require("../tabs/github/logic.js");
const { createController } = require("../tabs/github/controller.js");
const view = require("../tabs/github/view.js");
const { fakeDocument, FakeNode, fakeHangar, recorded, failed } = require("./helpers.js");

const NOW = Date.parse("2026-10-31T17:00:00Z");

async function draw(kind, { lang = "en", commands = recorded(), open = null, actions = {} } = {}) {
  const controller = createController({ kind, hangar: fakeHangar({ commands }), logic, lang, now: () => NOW });
  await controller.start();
  if (open) await controller.open(open);
  controller.dispose(); // no 60 s timer left to keep the process alive
  const container = new FakeNode("div");
  const calls = [];
  const ctx = {
    doc: fakeDocument, logic, lang, kind, now: () => NOW,
    actions: {
      refresh: () => calls.push("refresh"), open: (n) => calls.push(`open ${n}`), back: () => calls.push("back"),
      filters: (patch) => calls.push(["filters", patch]), clearFilters: () => calls.push("clear"),
      hand: (anchor, n) => calls.push(`hand ${n}`), copy: (b, text) => calls.push(`copy ${text}`), ...actions,
    },
  };
  view.render(container, controller.state, ctx);
  return { container, calls, controller, again: () => view.render(container, controller.state, ctx) };
}

const buttons = (container) => container.find((node) => node.tag === "button");
const byText = (container, text) => buttons(container).find((node) => node.text.includes(text));

test("pull requests: a row per item, with its checks and review in words, and a button to hand it over", async () => {
  const { container, calls } = await draw("pulls");
  assert.match(container.text, /example\/widgets/);
  assert.match(container.text, /2 checks failing/);
  assert.match(container.text, /4 open/);
  const rows = container.find((node) => node.className === "kit-row");
  assert.equal(rows.length, 4);
  assert.match(rows[0].text, /Fix the cache key for nested widgets/);
  assert.match(rows[0].text, /#42 · octocat · fix\/cache-key/);
  assert.match(rows[0].text, /2 checks failing/);
  assert.match(rows[0].text, /Review required/);
  assert.match(container.find((node) => node.tag === "li")[2].text, /Draft/);
  assert.equal(container.find((node) => node.className.startsWith("kit-glyph kit-glyph--failure")).length >= 2, true);

  const hands = buttons(container).filter((node) => node.text === "Hand to an agent");
  assert.equal(hands.length, 4, "each item has one");
  hands[0].onclick();
  rows[1].onclick();
  assert.deepEqual(calls, ["hand 42", "open 40"]);
});

test("in French, with its accents", async () => {
  const { container } = await draw("pulls", { lang: "fr" });
  assert.match(container.text, /2 checks en échec/);
  assert.match(container.text, /Revue requise/);
  assert.ok(byText(container, "Confier à un agent"));
  assert.ok(byText(container, "Actualiser"));
  assert.match(container.text, /4 ouvertes/);
});

test("the filters: chips that are pressed, counts, a search that does not lose a word", async () => {
  const { container, calls } = await draw("pulls");
  const chips = container.find((node) => node.className === "kit-filter");
  assert.deepEqual(chips.map((chip) => chip.text.replace(/\s+\d+$/, "")), ["Failing checks", "Needs review", "Hide drafts"]);
  assert.equal(chips[0].attributes["aria-pressed"], "false");
  chips[0].onclick();
  assert.deepEqual(calls.pop(), ["filters", { failing: true }]);
  const input = container.find((node) => node.tag === "input")[0];
  assert.equal(input.id, "filter-query");
  input.value = "cache";
  input.oninput();
  assert.deepEqual(calls.pop(), ["filters", { query: "cache" }]);
});

test("filters that match nothing say so and offer to clear them", async () => {
  const { container, calls, controller, again } = await draw("pulls");
  controller.state.filters = logic.sanitizeFilters("pulls", { query: "zzz" });
  again();
  assert.match(container.text, /Nothing matches these filters/);
  assert.match(container.text, /the 4 open/);
  byText(container, "Clear filters").onclick();
  assert.deepEqual(calls, ["clear"]);
});

test("no item at all is an empty state that says what fills it", async () => {
  const { container } = await draw("pulls", { commands: recorded({ "pr-list": () => ({ exitCode: 0, stdout: "[]" }) }) });
  assert.match(container.text, /No open pull request/);
  assert.match(container.text, /opened on this repository/);
});

test("issues: assignee and labels, and the unassigned chip", async () => {
  const { container } = await draw("issues");
  const rows = container.find((node) => node.className === "kit-row");
  assert.equal(rows.length, 2);
  assert.match(rows[0].text, /bug, ui/);
  assert.match(rows[1].text, /Assigned to: octocat/);
  assert.ok(container.find((node) => node.className === "kit-filter").some((chip) => chip.text.startsWith("Unassigned")));
});

test("a pull request in full: checks, the review asked for, the unresolved comments, the diff stat", async () => {
  const { container, calls } = await draw("pulls", { open: 42 });
  assert.match(container.text, /#42 Fix the cache key/);
  assert.match(container.text, /3 files, \+58 −12/);
  assert.match(container.text, /fix\/cache-key → main/);
  assert.match(container.text, /test \(ubuntu\)/);
  assert.match(container.text, /Unresolved comments/);
  assert.match(container.text, /src\/cache\.js:14/);
  assert.doesNotMatch(container.text, /nit: rename/);
  assert.match(container.text, /https:\/\/github\.com\/example\/widgets\/pull\/42/);
  const primary = container.find((node) => node.tag === "button" && node.className.includes("kit-btn--primary"));
  assert.equal(primary.length, 1, "one primary action: hand it over");
  assert.equal(primary[0].text, "Hand to an agent");
  byText(container, "Back").onclick();
  byText(container, "Copy link").onclick();
  assert.deepEqual(calls, ["back", "copy https://github.com/example/widgets/pull/42"]);
});

test("text from GitHub is only ever a text node, never markup", async () => {
  const hostile = JSON.parse(fs.readFileSync(path.join(__dirname, "fixtures", "pr-view-failing.json"), "utf8"));
  hostile.title = "<img src=x onerror=alert(1)> title";
  hostile.body = "<script>alert(1)</script>";
  const { container } = await draw("pulls", { open: 42, commands: recorded({ "pr-view": () => ({ exitCode: 0, stdout: JSON.stringify(hostile) }) }) });
  assert.match(container.text, /<img src=x onerror=alert\(1\)> title/);
  assert.match(container.text, /<script>alert\(1\)<\/script>/);
  assert.equal(container.find((node) => node.tag === "img" || node.tag === "script").length, 0);
});

test("problems are said with what to do, and the command that fixes them", async () => {
  const gone = await draw("pulls", { commands: recorded({ "repo-info": () => Promise.reject(Object.assign(new Error("x"), { code: "not_found" })) }) });
  assert.match(gone.container.text, /GitHub CLI \(gh\) is not installed/);
  assert.match(gone.container.text, /cli\.github\.com/);
  assert.ok(gone.container.find((node) => node.tag === "code" && node.text === "gh auth login").length);

  const auth = await draw("issues", { commands: recorded({ "repo-info": () => failed("not-logged-in.stderr", 4) }) });
  assert.match(auth.container.text, /gh is not signed in to GitHub/);
  assert.ok(auth.container.find((node) => node.text === "gh auth login").length);
  assert.ok(byText(auth.container, "Try again"));

  const noRepo = await draw("pulls", { lang: "fr", commands: recorded({ "repo-info": () => failed("not-a-git-repo.stderr") }) });
  assert.match(noRepo.container.text, /Ce dossier n’est pas un dépôt GitHub/);
  assert.match(noRepo.container.text, /not a git repository/, "gh's own words are shown too");

  const denied = await draw("pulls", { commands: {} });
  assert.match(denied.container.text, /may not run this command/);
  assert.equal(byText(denied.container, "Try again"), undefined, "retrying a refusal does nothing");
  assert.ok(denied.container.find((node) => node.className.includes("kit-icon--lock")).length);
});

test("the view holds no colour of its own, and loads nothing from outside", () => {
  for (const name of ["view.js", "app.js", "pulls.html", "issues.html"]) {
    const source = fs.readFileSync(path.join(__dirname, "..", "tabs", "github", name), "utf8");
    assert.doesNotMatch(source, /#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(|style\s*=/, `${name}: no colour, no inline style`);
    assert.doesNotMatch(source, /innerHTML|insertAdjacentHTML|document\.write/, `${name}: no markup from text`);
    assert.doesNotMatch(source, /https?:\/\//, `${name}: no external address`);
  }
  for (const name of ["pulls.html", "issues.html"]) {
    const html = fs.readFileSync(path.join(__dirname, "..", "tabs", "github", name), "utf8");
    const addresses = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);
    for (const address of addresses) assert.match(address, /^(\/_hangar\/(kit\.css|kit\.js|kit-ui\.js)|[a-z]+\.(js|css))$/, address);
  }
  const sheet = fs.readFileSync(path.join(__dirname, "..", "tabs", "github", "style.css"), "utf8");
  assert.doesNotMatch(sheet, /#[0-9a-fA-F]{3,8}|rgba?\(|hsla?\(|color\s*:|background|url\(|@import/, "style.css: layout only, in tokens");
});
