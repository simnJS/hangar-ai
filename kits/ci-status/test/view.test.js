"use strict";
// What the page draws for each state, on a fake DOM: Hangar UI's classes, text and
// no markup from gh, both languages, every state designed.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/ci/logic.js");
const view = require("../tabs/ci/view.js");
const { createController } = require("../tabs/ci/controller.js");
const { fixture, fakeClock, fakeHangar, fakeUi, fakeDocument, FakeNode, settle } = require("./helpers.js");

const NOW = Date.parse("2026-10-31T18:48:37Z");

async function drawn({ commands, params = { id: "1" }, lang = "en", keep = false }) {
  const clock = fakeClock(NOW);
  const hangar = fakeHangar({ params, commands });
  const root = new FakeNode("div");
  const opened = [];
  const ui = fakeUi(lang);
  const t = ui.i18n(logic.STRINGS);
  const actions = { open: (id) => opened.push(id), back: () => undefined, retry: () => undefined };
  const draw = (state) => view.render(root, state, { doc: fakeDocument, logic, ui, t, now: clock.now, actions });
  const controller = createController({ hangar, logic, t, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer, onChange: draw });
  // Not awaited: a test may be holding gh's answer back, to see the page wait for it.
  controller.start();
  await settle();
  if (!keep) controller.dispose();
  return { root, opened, controller, clock, ui, draw };
}
const reply = (name) => ({ exitCode: 0, stdout: fixture(name) });
const denied = () => {
  throw Object.assign(new Error("x"), { code: "denied" });
};
const noGh = () => {
  throw Object.assign(new Error("x"), { code: "not_found" });
};
const failedRun = { "ci-run": () => reply("run-failure.json"), "ci-log": () => reply("log-failed.txt") };
const one = (nodes) => {
  assert.equal(nodes.length, 1);
  return nodes[0];
};

test("the run's title and state stand on one line, what it took under them", async () => {
  const { root } = await drawn({ commands: failedRun });
  const line = one(root.all("ci-headline"));
  const [title, state] = line.children;
  assert.equal(title.tag, "h1");
  assert.equal(title.text, "Release 1.7.0");
  assert.equal(title.title, "Release 1.7.0", "the whole of it on hover");
  assert.match(state.className, /kit-state--failure/);
  assert.equal(state.text, "Failed");
  assert.equal(root.all("kit-header__state")[0].text, "<256000 ms>, <104 min ago>, CI on main");
});

test("a running run: jobs as rows with their glyph and time, the running step in its row", async () => {
  const { root } = await drawn({ commands: { "ci-run": () => reply("run-running.json") } });
  assert.match(one(root.all("ci-headline")).children[1].className, /kit-state--running/);
  const rows = root.all("kit-row");
  assert.equal(rows.length, 4);
  assert.deepEqual(rows.map((row) => row.all("kit-row__title")[0].text), [
    "rust (windows-latest)",
    "rust (macos-latest)",
    "frontend",
    "rust (ubuntu-22.04)",
  ]);
  assert.deepEqual(rows.map((row) => row.all("kit-glyph")[0].className.replace("kit-glyph kit-glyph--", "")), ["success", "running", "waiting", "waiting"]);
  assert.deepEqual(rows.map((row) => row.all("kit-glyph")[0].attributes["aria-label"]), ["Passed", "Running", "Queued", "Queued"]);

  const going = rows[1];
  assert.equal(going.all("kit-row__sub")[0].text, "Keep Defender out of the build, step 3 of 13");
  assert.match(going.all("kit-row__end")[0].text, /^<\d+ ms>$/);
  assert.equal(rows[2].all("kit-row__sub").length, 0, "a queued job has no step");
  assert.equal(rows[2].all("kit-row__end")[0].text, "Queued", "and no time");
  assert.equal(root.find((node) => node.tag === "progress").length, 0);
  assert.equal(root.all("ci-log").length, 0, "no log while it runs");
});

test("a cancelled run and a skipped job say so, rather than « ended »", async () => {
  const run = JSON.parse(fixture("run-success.json"));
  run.conclusion = "cancelled";
  run.jobs[0].conclusion = "skipped";
  const { root } = await drawn({ commands: { "ci-run": () => ({ exitCode: 0, stdout: JSON.stringify(run) }) } });
  const state = one(root.all("ci-headline")).children[1];
  assert.match(state.className, /kit-state--neutral/);
  assert.equal(state.text, "Cancelled");
  assert.equal(root.all("kit-row")[0].all("kit-row__end")[0].text, "Skipped");
});

test("a failure: what failed first, its log right under it, then the rest", async () => {
  const { root } = await drawn({ commands: failedRun });
  const titles = root.all("kit-section__title").map((node) => node.text);
  assert.deepEqual(titles, ["Failed jobs", "Log of the failed steps", "Other jobs"]);
  const [failed, others] = root.all("kit-rows");
  const row = one(failed.all("kit-row"));
  assert.equal(row.all("kit-row__title")[0].text, "rust (macos-latest)");
  assert.equal(row.all("kit-row__sub")[0].text, "Failed at the step Tests");
  assert.equal(row.all("kit-row__end")[0].text, "<143000 ms>");
  assert.equal(others.all("kit-row").length, 3);
  assert.deepEqual(root.all("kit-section__count").map((node) => node.text), ["1", "3"]);
});

test("the log is built by the library, told which lines are errors", async () => {
  const { root } = await drawn({ commands: failedRun });
  const { logged } = one(root.all("ci-log"));
  assert.equal(logged.isError, logic.isErrorLine);
  assert.equal(logged.label, "Log of the failed steps");
  const first = logged.lines.findIndex((line) => (line.kind || (logged.isError(line.text) ? "error" : "")) === "error");
  assert.match(logged.lines[first].text, /panicked at src\/accounts\.rs:688:9/);
});

test("the log, every way it can go", async () => {
  const run = { "ci-run": () => reply("run-failure.json") };
  let reading;
  const held = new Promise((resolve) => (reading = resolve));
  const waiting = await drawn({ commands: { ...run, "ci-log": async () => (await held, reply("log-failed.txt")) } });
  assert.match(waiting.root.shown, /Reading the log…/);
  assert.equal(waiting.root.all("ci-log").length, 0);
  assert.equal(one(waiting.root.all("kit-state--running")).tag, "span");
  reading();

  const unreadable = await drawn({ commands: { ...run, "ci-log": () => ({ exitCode: 1, stderr: "run 1 is still in progress" }) } });
  const banner = one(unreadable.root.all("kit-banner--error"));
  assert.match(banner.text, /The log could not be read: gh failed\./);
  assert.match(banner.text, /Try again/);

  const blank = await drawn({ commands: { ...run, "ci-log": () => ({ exitCode: 0, stdout: "  \n" }) } });
  assert.match(blank.root.shown, /The log is empty/);

  const cut = await drawn({ commands: { ...run, "ci-log": () => ({ exitCode: 0, stdout: "a line\n", truncated: true }) } });
  assert.match(one(cut.root.all("kit-banner--warning").filter((node) => !node.hidden)).text, /first megabyte/);
});

test("the link is a quiet line at the foot, and can be copied", async () => {
  const { root } = await drawn({ commands: failedRun });
  const foot = one(root.all("ci-foot"));
  assert.match(foot.className, /kit-bar--foot/);
  assert.equal(foot.hidden, false);
  const url = "https://github.com/simnJS/hangar-ai/actions/runs/41222364482";
  const text = one(foot.all("kit-truncate"));
  assert.equal(text.text, url);
  assert.equal(text.title, url);
  const copy = one(foot.find((node) => node.tag === "button"));
  assert.equal(copy.copies, url);
  assert.equal(copy.attributes["aria-label"], "Copy the link");
});

test("a run that fell over hides what it said of itself, and the link", async () => {
  const replies = [reply("run-running.json")];
  const { root, clock, controller } = await drawn({
    keep: true,
    commands: { "ci-run": () => replies.shift() || { exitCode: 4, stderr: fixture("not-logged-in.stderr") } },
  });
  assert.match(root.shown, /Running/);
  await clock.advance(5000);
  controller.dispose();
  assert.equal(controller.state.view, "error");
  assert.doesNotMatch(root.shown, /Running/);
  assert.equal(one(root.all("ci-foot")).hidden, true);
  assert.match(root.shown, /gh is not signed in/);
});

test("a hiccup while following a run is a banner over the run", async () => {
  const replies = [reply("run-running.json"), { exitCode: 1, stderr: "error connecting" }];
  const { root, clock, controller } = await drawn({ keep: true, commands: { "ci-run": () => replies.shift() || reply("run-running.json") } });
  assert.equal(one(root.all("kit-banner--warning")).hidden, true);
  await clock.advance(5000);
  assert.equal(one(root.all("kit-banner--warning")).hidden, false);
  assert.match(root.shown, /The last refresh failed/);
  assert.match(root.shown, /rust \(macos-latest\)/);
  controller.dispose();
});

test("in French, with accents", async () => {
  const { root } = await drawn({ lang: "fr", commands: failedRun });
  assert.equal(one(root.all("ci-headline")).children[1].text, "Échouée");
  assert.match(root.shown, /Journal des étapes en échec/);
  assert.match(root.shown, /Jobs en échec/);
  assert.match(root.shown, /Échec à l'étape Tests/);
  assert.match(root.all("kit-header__state")[0].text, /CI sur main/);
  assert.equal(root.all("kit-glyph")[0].attributes["aria-label"], "Échoué", "a job agrees as a job");
});

test("a run with no job has no count of them", async () => {
  const queued = JSON.parse(fixture("run-running.json"));
  queued.status = "queued";
  queued.jobs = [];
  const { root } = await drawn({ commands: { "ci-run": () => ({ exitCode: 0, stdout: JSON.stringify(queued) }) } });
  assert.match(root.shown, /No job yet\./);
  assert.equal(root.all("kit-section__count").length, 0);
});

test("loading says what it reads, under a head that will not move", async () => {
  let release;
  const held = new Promise((resolve) => (release = resolve));
  const run = await drawn({ commands: { "ci-run": async () => (await held, reply("run-success.json")) } });
  assert.match(run.root.shown, /Reading the run…/);
  assert.equal(one(run.root.all("ci-headline")).children[0].text, "Run #1");
  assert.equal(one(run.root.all("kit-empty")).children[0].className, "kit-state kit-state--running");
  const list = await drawn({ params: {}, commands: { "ci-list": async () => (await held, reply("run-list.json")) } });
  assert.match(list.root.shown, /Reading the runs…/);
  assert.equal(one(list.root.all("ci-headline")).children[0].text, "Recent runs");
  const refresh = one(list.root.find((node) => node.tag === "button"));
  assert.equal(refresh.attributes["aria-busy"], "true");
  assert.equal(refresh.children[0].className, "kit-icon kit-icon--restart", "one ring: the page's own");
  release();
});

test("the head of a run loading already holds the height of its second line", async () => {
  const { root } = await drawn({ commands: { "ci-run": () => new Promise(() => undefined) } });
  const line = one(root.all("kit-header__state"));
  assert.equal(line.hidden, false);
  assert.equal(line.text, " ");
});

test("refreshing keeps the rows and the keyboard, and says it is working", async () => {
  let release = () => undefined;
  const replies = [() => reply("run-list.json"), async () => (await new Promise((resolve) => (release = resolve)), reply("run-list.json"))];
  const clock = fakeClock(NOW);
  const hangar = fakeHangar({ params: {}, commands: { "ci-list": () => replies.shift()() } });
  const root = new FakeNode("div");
  const ui = fakeUi("en");
  const t = ui.i18n(logic.STRINGS);
  let retried = 0;
  const controller = createController({ hangar, logic, t, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer, onChange: (state) => draw(state) });
  const actions = { open: () => undefined, back: () => undefined, retry: () => ((retried += 1), controller.retry()) };
  const draw = (state) => view.render(root, state, { doc: fakeDocument, logic, ui, t, now: clock.now, actions });
  controller.start();
  await settle();
  const refresh = root.find((node) => node.tag === "button")[0];
  const row = root.all("kit-row")[0];
  assert.equal(refresh.attributes["aria-busy"], "false");

  refresh.onclick();
  await settle();
  assert.equal(retried, 1);
  assert.equal(refresh.attributes["aria-busy"], "true");
  assert.equal(refresh.disabled, undefined, "not disabled: that would take the keyboard away");
  assert.equal(refresh.children[0].className, "kit-glyph kit-glyph--running");
  assert.equal(root.all("kit-row")[0], row, "the rows stay");

  refresh.onclick();
  assert.equal(retried, 1, "once is enough");

  release();
  await settle();
  assert.equal(refresh.attributes["aria-busy"], "false");
  assert.equal(root.all("kit-row")[0], row, "the same runs: nothing is rebuilt");
  controller.dispose();
});

const STATES = [
  ["not logged in", { "ci-run": () => ({ exitCode: 4, stderr: fixture("not-logged-in.stderr") }) }, "gh is not signed in", "alert", "gh auth login"],
  ["gh absent", { "ci-run": noGh }, "gh is not installed", "alert", "https://cli.github.com"],
  ["permission denied", { "ci-run": denied }, "This tab may not read the CI", "lock", null],
  ["no repository", { "ci-run": () => ({ exitCode: 1, stderr: "fatal: not a git repository" }) }, "No GitHub repository here", "alert", null],
  ["no such run", { "ci-run": () => ({ exitCode: 1, stderr: "HTTP 404: not found" }) }, "Run not found", "alert", null],
  ["too long", { "ci-run": () => ({ exitCode: null, timedOut: true }) }, "gh took too long", "alert", null],
];
for (const [name, commands, title, icon, toCopy] of STATES) {
  test(`${name}: a designed state, what it is, what to do, and one action`, async () => {
    const { root } = await drawn({ commands });
    const box = one(root.all("kit-empty"));
    assert.equal(box.attributes.role, "alert");
    assert.equal(one(box.all("kit-empty__title")).text, title);
    assert.ok(one(box.all("kit-empty__text")).text.length > 20);
    assert.equal(one(box.all(`kit-icon--${icon}`)).tag, "span");
    const primary = one(box.all("kit-btn--primary"));
    assert.equal(primary.text, "Try again");
    if (toCopy) {
      assert.equal(one(box.all("kit-code")).text, toCopy);
      assert.equal(one(box.all("ci-copy")).find((node) => node.tag === "button")[0].copies, toCopy);
    } else {
      assert.equal(box.all("ci-copy").length, 0);
    }
    assert.equal(one(root.all("ci-foot")).hidden, true);
    // In French as well.
    const fr = await drawn({ commands, lang: "fr" });
    assert.notEqual(one(fr.root.all("kit-empty__title")).text, title);
    assert.equal(one(fr.root.all("kit-btn--primary")).text, "Réessayer");
  });
}

test("what gh said when nothing else explains it is shown, as text", async () => {
  const { root } = await drawn({ commands: { "ci-run": () => ({ exitCode: 1, stderr: "something <b>odd</b>" }) } });
  assert.equal(one(root.all("kit-empty__title")).text, "gh failed");
  const said = one(root.all("kit-pre"));
  assert.equal(said.text, "something <b>odd</b>");
  assert.equal(said.tabIndex, 0, "it scrolls, so it takes the keyboard");
  assert.equal(root.find((node) => node.tag === "b").length, 0);
  // « This is what it said: » comes right before what it said, the action after.
  const parts = one(root.all("kit-empty")).children.map((node) => node.className);
  assert.deepEqual(parts.slice(-3), ["kit-empty__text", "kit-pre ci-said", "kit-empty__actions"]);
});

test("the picker lists runs as rows, with branch and age, that open them", async () => {
  const { root, opened } = await drawn({ params: {}, commands: { "ci-list": () => reply("run-list.json") } });
  assert.equal(one(root.all("ci-headline")).children[0].text, "Recent runs");
  const rows = root.find((node) => node.tag === "button" && /\bkit-row\b/.test(node.className));
  assert.equal(rows.length, 15);
  const first = rows[0];
  assert.equal(first.all("kit-row__title")[0].text, "Point the site's downloads to 1.8.0");
  const line = one(first.all("kit-row__sub"));
  assert.equal(one(line.all("kit-truncate")).text, "CI on main");
  assert.equal(one(line.all("ci-age")).text, "<2 min ago>");
  assert.equal(rows[1].all("kit-truncate")[0].text, "Release on v1.8.0");
  assert.equal(first.all("kit-glyph")[0].attributes["aria-label"], "Passed");
  first.onclick();
  assert.deepEqual(opened, ["41231065926"]);
  assert.equal(one(root.all("ci-foot")).hidden, true);
});

test("no run: says so, and what will bring one", async () => {
  const { root } = await drawn({ params: {}, commands: { "ci-list": () => ({ exitCode: 0, stdout: "[]" }) } });
  const box = one(root.all("kit-empty"));
  assert.equal(one(box.all("kit-empty__title")).text, "No run yet");
  assert.match(one(box.all("kit-empty__text")).text, /A push to this repository/);
  assert.equal(box.attributes.role, undefined, "not an error");
  const refresh = one(root.find((node) => node.tag === "button"));
  assert.equal(refresh.attributes["aria-label"], "Refresh");
  assert.equal(refresh.attributes["aria-busy"], "false");
});

test("a run opened from the picker has a way back; one opened by a hook has none", async () => {
  const { root, controller, draw } = await drawn({ params: {}, keep: true, commands: { "ci-list": () => reply("run-list.json"), "ci-run": () => reply("run-success.json") } });
  assert.equal(root.all("ci-back").length, 0);
  await controller.openRun("41231065926");
  draw(controller.state);
  assert.equal(one(root.all("ci-back")).attributes["aria-label"], "Back to the recent runs");
  controller.dispose();
  const direct = await drawn({ commands: { "ci-run": () => reply("run-success.json") } });
  assert.equal(direct.root.all("ci-back").length, 0);
});

test("a redraw leaves alone what did not change: only the clocks move", async () => {
  const { root, clock, controller, draw } = await drawn({ keep: true, commands: { "ci-run": () => reply("run-running.json") } });
  const copy =one(one(root.all("ci-foot")).find((node) => node.tag === "button"));
  const row = root.all("kit-row")[1];
  const end = row.all("kit-row__end")[0];
  const before = end.text;
  const elapsedBefore = root.all("kit-header__state")[0].text;
  await clock.advance(1000);
  draw(controller.state);
  assert.equal(one(one(root.all("ci-foot")).find((node) => node.tag === "button")), copy, "the same button, focus and all");
  assert.equal(root.all("kit-row")[1], row, "the same row");
  assert.notEqual(end.text, before, "its time moved");
  assert.notEqual(root.all("kit-header__state")[0].text, elapsedBefore);
  controller.dispose();
});

test("a line that did not change is not written again: a selection in it stays", async () => {
  const { root, clock, controller, draw } = await drawn({ keep: true, commands: { "ci-run": () => reply("run-running.json") } });
  const [title] = one(root.all("ci-headline")).children;
  const text = title.children[0];
  title.children[0] = Object.assign(new String(text), { marked: true });
  await clock.advance(1000);
  draw(controller.state);
  assert.equal(title.children[0].marked, true);
  controller.dispose();
});

test("a log is built once: a redraw never scrolls it back", async () => {
  const { root, controller, draw } = await drawn({ commands: failedRun, keep: true });
  const lines = one(root.all("ci-log"));
  const options = lines.logged;
  draw(controller.state);
  assert.equal(one(root.all("ci-log")), lines);
  assert.equal(one(root.all("ci-log")).logged, options);
  controller.dispose();
});

test("Hangar's language changing redraws the page in it", async () => {
  const { root, controller, ui, draw } = await drawn({ commands: failedRun, keep: true });
  assert.match(root.shown, /Failed jobs/);
  ui.lang = "fr";
  draw(controller.state);
  assert.match(root.shown, /Jobs en échec/);
  assert.doesNotMatch(root.shown, /Failed jobs/);
  assert.equal(root.all("ci-log").length, 1);
  controller.dispose();
});

test("what gh says is text, never markup", async () => {
  const hostile = JSON.parse(fixture("run-success.json"));
  hostile.workflowName = "<img src=x onerror=alert(1)>";
  hostile.displayTitle = "<script>alert(1)</script>";
  const { root } = await drawn({ commands: { "ci-run": () => ({ exitCode: 0, stdout: JSON.stringify(hostile) }) } });
  assert.match(root.text, /<script>alert\(1\)<\/script>/);
  assert.equal(root.find((node) => ["img", "script"].includes(node.tag)).length, 0);
});
