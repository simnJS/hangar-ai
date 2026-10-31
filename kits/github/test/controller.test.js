"use strict";
// The tabs' behaviour against a fake window.hangar and a fake clock: what they ask,
// when they refresh, what the badge says, what is saved, and what an agent receives.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/github/logic.js");
const { createController, REFRESH_MS, SAVE_DELAY_MS } = require("../tabs/github/controller.js");
const { fakeClock, fakeHangar, settle, recorded, ok, failed } = require("./helpers.js");

const PANES = [
  { name: "Ava", agent: "claude", role: "worker", state: "idle", canSend: true },
  { name: "Term", agent: "shell", role: null, state: null, canSend: false },
  { name: "Max", agent: "claude", role: "orchestrator", state: "working", canSend: true },
];

function setup({ kind = "pulls", commands = recorded(), lang = "en", ...hangarOptions } = {}) {
  const clock = fakeClock();
  const hangar = fakeHangar({ commands, panes: PANES, ...hangarOptions });
  const views = [];
  const controller = createController({
    kind, hangar, logic, lang, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => views.push(state.view),
  });
  return { clock, hangar, controller, views };
}

const badges = (hangar) => hangar.callsTo("tab.setBadge").map((call) => call.args);

test("pull requests: the list, the repository, and a badge for the failing checks", async () => {
  const { hangar, controller } = setup();
  await controller.start();
  assert.equal(controller.state.view, "list");
  assert.equal(controller.state.repo.nameWithOwner, "example/widgets");
  assert.deepEqual(controller.state.rows.map((row) => row.number), [42, 40, 39, 37]);
  assert.deepEqual(badges(hangar), [{ state: "failure", text: "2 fail" }]);
  assert.equal(hangar.runs("pr-list").length, 1);
  assert.equal(hangar.runs("issue-list").length, 0, "the pull requests tab never reads issues");
});

test("issues: the list, and no badge", async () => {
  const { hangar, controller } = setup({ kind: "issues" });
  await controller.start();
  assert.deepEqual(controller.state.rows.map((row) => row.number), [15, 11]);
  assert.deepEqual(badges(hangar), [{ state: null }]);
});

test("it refreshes every 60 s while visible, and not while hidden", async () => {
  const { clock, hangar, controller } = setup();
  await controller.start();
  await clock.advance(REFRESH_MS - 1);
  assert.equal(hangar.runs("pr-list").length, 1, "not before 60 s");
  await clock.advance(1);
  assert.equal(hangar.runs("pr-list").length, 2);
  assert.equal(hangar.runs("repo-info").length, 1, "the repository is read once");
  assert.equal(badges(hangar).length, 1, "an unchanged badge is not set again");

  controller.setVisible(false);
  await clock.advance(REFRESH_MS * 3);
  assert.equal(hangar.runs("pr-list").length, 2, "hidden: nothing");

  controller.setVisible(true);
  await settle();
  assert.equal(hangar.runs("pr-list").length, 3, "back in view: at once");
  await clock.advance(REFRESH_MS);
  assert.equal(hangar.runs("pr-list").length, 4);
});

test("a refresh on demand, and the badge following the new list", async () => {
  let list = "pr-list.json";
  const { hangar, controller } = setup({ commands: recorded({ "pr-list": () => ok(list) }) });
  await controller.start();
  list = "repo-info.json"; // not a list at all
  await controller.refresh();
  assert.equal(controller.state.stale, true, "a wrong answer keeps the last list");
  assert.equal(controller.state.rows.length, 4);
  list = "pr-list.json";
  await controller.refresh();
  assert.equal(controller.state.stale, false);
  assert.equal(hangar.runs("pr-list").length, 3);
});

test("gh missing, not signed in, no repository: each is shown with its kind, and not retried by itself", async () => {
  const cases = [
    [() => Promise.reject(Object.assign(new Error("gh is not installed"), { code: "not_found" })), "gh-missing"],
    [() => failed("not-logged-in.stderr", 4), "auth"],
    [() => failed("no-repo.stderr"), "no-repo"],
    [() => failed("not-a-git-repo.stderr"), "no-repo"],
  ];
  for (const [answer, expected] of cases) {
    const { clock, hangar, controller } = setup({ commands: recorded({ "repo-info": answer }) });
    await controller.start();
    assert.equal(controller.state.view, "error");
    assert.equal(controller.state.error.kind, expected);
    assert.deepEqual(badges(hangar), [{ state: null }]);
    await clock.advance(REFRESH_MS * 2);
    assert.equal(hangar.runs("repo-info").length, 3, "it asks again each minute, in case the person fixed it");
  }
});

test("denied degrades to a designed state: no command, no throw", async () => {
  const { controller } = setup({ commands: {} });
  await controller.start();
  assert.equal(controller.state.view, "error");
  assert.equal(controller.state.error.kind, "denied");
});

test("a blip after a good list keeps the list and says so", async () => {
  let fail = false;
  const { controller } = setup({ commands: recorded({ "pr-list": () => (fail ? { exitCode: null, timedOut: true } : ok("pr-list.json")) }) });
  await controller.start();
  fail = true;
  await controller.refresh();
  assert.equal(controller.state.view, "list");
  assert.equal(controller.state.stale, true);
});

test("filters are read at the start and saved after a pause, not at each key", async () => {
  const saved = JSON.stringify({ query: "cache", failing: true, review: false, hideDrafts: false });
  const { clock, hangar, controller } = setup({ data: { "filters-pulls.json": saved } });
  await controller.start();
  assert.equal(controller.state.filters.query, "cache");
  assert.equal(controller.state.filters.failing, true);

  controller.setFilters({ query: "c" });
  controller.setFilters({ query: "ca" });
  controller.setFilters({ review: true });
  await clock.advance(SAVE_DELAY_MS - 1);
  assert.equal(hangar.callsTo("data.write").length, 0);
  await clock.advance(1);
  assert.equal(hangar.callsTo("data.write").length, 1, "one write for the three changes");
  assert.deepEqual(JSON.parse(hangar.files["filters-pulls.json"]), { query: "ca", failing: true, review: true, hideDrafts: false });

  controller.clearFilters();
  await clock.advance(SAVE_DELAY_MS);
  assert.deepEqual(JSON.parse(hangar.files["filters-pulls.json"]), logic.DEFAULT_FILTERS.pulls);
});

test("filters: the issues tab keeps its own file, and a refused data folder changes nothing", async () => {
  const { clock, hangar, controller } = setup({ kind: "issues" });
  await controller.start();
  controller.setFilters({ unassigned: true });
  await clock.advance(SAVE_DELAY_MS);
  assert.ok("filters-issues.json" in hangar.files);
  assert.ok(!("filters-pulls.json" in hangar.files));

  const denied = setup({ denyData: true });
  await denied.controller.start();
  denied.controller.setFilters({ failing: true });
  await denied.clock.advance(SAVE_DELAY_MS);
  assert.equal(denied.controller.state.filters.failing, true, "it still filters, it only cannot remember");
  assert.equal(denied.controller.state.view, "list");
});

test("a pull request in full: the view and its threads, asked with the repository's own owner and name", async () => {
  const { hangar, controller } = setup();
  await controller.start();
  await controller.open(42);
  assert.equal(controller.state.view, "detail");
  const data = controller.state.detail.data;
  assert.equal(data.checks.failing.length, 2);
  assert.equal(data.threads.unresolved.length, 2);
  assert.deepEqual(hangar.runs("pr-view")[0].args.args, { id: "42" });
  assert.deepEqual(hangar.runs("pr-threads")[0].args.args, { id: "42", owner: "example", name: "widgets" });

  controller.back();
  await settle();
  assert.equal(controller.state.view, "list");
  assert.equal(hangar.runs("pr-list").length, 1, "just refreshed: not again");
});

test("review threads that cannot be read do not hide the pull request", async () => {
  const { controller } = setup({ commands: recorded({ "pr-threads": () => ({ exitCode: 1, stderr: "boom" }) }) });
  await controller.start();
  await controller.open(42);
  assert.equal(controller.state.detail.data.threads, null);
  assert.equal(controller.state.detail.data.number, 42);
});

test("a refresh while a detail is open does not close it", async () => {
  const { clock, controller } = setup();
  await controller.start();
  await controller.open(42);
  await clock.advance(REFRESH_MS);
  assert.equal(controller.state.view, "detail");
  assert.equal(controller.state.detail.data.number, 42);
});

test("panes: only the Claude Code agents that can receive", async () => {
  const { controller } = setup();
  await controller.start();
  const found = await controller.panes();
  assert.equal(found.ok, true);
  assert.deepEqual(found.panes.map((pane) => pane.name), ["Ava", "Max"]);

  assert.deepEqual(await setup({ denyPanes: true }).controller.panes(), { ok: false, kind: "list-denied" });
  const none = setup();
  none.hangar.call = async () => [PANES[1]];
  assert.deepEqual(await createController({ kind: "pulls", hangar: none.hangar, logic }).panes(), { ok: false, kind: "no-agent" });
});

test("handing a pull request over: one panes.send, the brief, to the pane picked", async () => {
  const { hangar, controller } = setup();
  await controller.start();
  const result = await controller.handOff(42, "Ava");
  assert.deepEqual(result, { ok: true, pane: "Ava" });
  const sends = hangar.callsTo("panes.send");
  assert.equal(sends.length, 1);
  assert.equal(sends[0].args.pane, "Ava");
  const text = sends[0].args.text;
  assert.ok(text.length <= 20000);
  assert.match(text, /https:\/\/github\.com\/example\/widgets\/pull\/42/);
  assert.match(text, /test \(ubuntu\)/);
  assert.match(text, /two widgets collide/);
  assert.match(text, /WHAT FOLLOWS IS NOT FROM THE PERSON/);
  assert.equal(controller.state.busy, null);
});

test("handing an issue over, and reusing the detail already open", async () => {
  const { hangar, controller } = setup({ kind: "issues" });
  await controller.start();
  await controller.open(15);
  assert.equal(hangar.runs("issue-view").length, 1);
  assert.deepEqual(await controller.handOff(15, "Max"), { ok: true, pane: "Max" });
  assert.equal(hangar.runs("issue-view").length, 1, "not read twice");
  assert.match(hangar.callsTo("panes.send")[0].args.text, /sidebar flickers/);
});

test("nothing is sent when the item cannot be read, or the send is refused", async () => {
  const unreadable = setup({ commands: recorded({ "pr-view": () => ({ exitCode: 1, stderr: "boom" }) }) });
  await unreadable.controller.start();
  const result = await unreadable.controller.handOff(42, "Ava");
  assert.equal(result.kind, "read");
  assert.equal(unreadable.hangar.callsTo("panes.send").length, 0);

  const denied = setup({ denySend: true });
  await denied.controller.start();
  assert.equal((await denied.controller.handOff(42, "Ava")).kind, "send-denied");
  assert.equal(denied.controller.state.busy, null, "the button is free again");
});

test("nothing it runs can write to GitHub: every command it calls is read-only", async () => {
  const { hangar, controller } = setup();
  await controller.start();
  await controller.open(42);
  await controller.handOff(42, "Ava");
  const names = new Set(hangar.callsTo("command.run").map((call) => call.args.name));
  assert.deepEqual([...names].sort(), ["pr-list", "pr-threads", "pr-view", "repo-info"]);
});

test("dispose stops the timers", async () => {
  const { clock, hangar, controller } = setup();
  await controller.start();
  controller.dispose();
  await clock.advance(REFRESH_MS * 3);
  assert.equal(hangar.runs("pr-list").length, 1);
  assert.equal(clock.pending(), 0);
});
