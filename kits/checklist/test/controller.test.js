"use strict";
// The tab's behaviour against a fake window.hangar: reading the list, keeping answers
// within data.write's rate, the badge, sending, and reopening to test a fix.
const test = require("node:test");
const assert = require("node:assert/strict");
const { logic, LIST, LIST_PATH, opened, fakeHangar, fakeClock, settle } = require("./helpers.js");
const { createController, SAVE_DELAY_MS, SAVE_SPACING_MS } = require("../tabs/checklist/controller.js");
const { t } = require("./helpers.js");

const badges = (hangar) => hangar.callsTo("tab.setBadge").map((call) => call.args);
const saved = (hangar) => JSON.parse(hangar.data.get(logic.dataPath(LIST_PATH)));

test("it reads the list, names the tab after it, and starts with a running badge", async () => {
  const { hangar, state } = await opened();
  assert.equal(state.view, "list");
  assert.equal(state.list.steps.length, 3);
  assert.deepEqual(hangar.callsTo("files.read").map((call) => call.args), [{ path: LIST_PATH }]);
  assert.deepEqual(hangar.callsTo("tab.setTitle").map((call) => call.args), [{ title: "Login and logout" }]);
  assert.deepEqual(badges(hangar), [{ state: "running", text: "0/3" }]);
  assert.deepEqual(state.caller, { pane: "Ava" });
});

test("each answer moves the badge: running, then success", async () => {
  const { hangar, controller } = await opened();
  controller.answer(0, "ok");
  controller.answer(1, "skip");
  await settle();
  assert.deepEqual(badges(hangar).at(-1), { state: "running", text: "2/3" });
  controller.answer(2, "ok");
  await settle();
  assert.deepEqual(badges(hangar).at(-1), { state: "success", text: "3/3" });
});

test("a failed step makes the badge a failure once everything is answered, and un-answering goes back", async () => {
  const { hangar, controller, state } = await opened();
  controller.answer(0, "ok");
  controller.answer(1, "fail");
  controller.answer(2, "ok");
  await settle();
  assert.deepEqual(badges(hangar).at(-1), { state: "failure", text: "1 failed" });
  controller.clear(1);
  await settle();
  assert.deepEqual(badges(hangar).at(-1), { state: "running", text: "2/3" });
  assert.equal(logic.statusOf(state.saved, state.keys[1]), "todo");
});

test("answers are written after a pause, together, and survive a restart", async () => {
  const { hangar, controller, clock } = await opened();
  controller.answer(0, "ok");
  controller.answer(1, "fail");
  controller.setNote(1, "The dashboard is blank.");
  assert.equal(hangar.callsTo("data.write").length, 0, "nothing at once");
  await clock.advance(SAVE_DELAY_MS);
  assert.equal(hangar.callsTo("data.write").length, 1);
  const written = saved(hangar);
  assert.equal(written.answers[logic.stepKeys(LIST.steps)[1]].note, "The dashboard is blank.");

  const again = await opened({ hangar: fakeHangar({ params: { path: LIST_PATH }, caller: null, files: { [LIST_PATH]: JSON.stringify(LIST) }, data: Object.fromEntries(hangar.data) }) });
  assert.equal(logic.statusOf(again.state.saved, again.state.keys[0]), "ok");
  assert.deepEqual(again.state.saved.answers[again.state.keys[1]], { status: "fail", note: "The dashboard is blank." });
});

test("typing a note never writes faster than data.write allows", async () => {
  const clock = fakeClock();
  const hangar = fakeHangar({ params: { path: LIST_PATH }, caller: { pane: "Ava" }, files: { [LIST_PATH]: JSON.stringify(LIST) }, now: clock.now });
  const { controller } = await opened({ hangar, clock });
  const times = [];
  const original = hangar.call.bind(hangar);
  hangar.call = (method, args) => {
    if (method === "data.write") times.push(clock.now());
    return original(method, args);
  };
  controller.answer(0, "fail");
  for (let second = 0; second < 180; second++) {
    controller.setNote(0, `note ${second}`);
    await clock.advance(1000);
  }
  await clock.advance(SAVE_SPACING_MS);
  for (let i = 1; i < times.length; i++) assert.ok(times[i] - times[i - 1] >= SAVE_SPACING_MS, "writes are spaced");
  for (let i = 10; i < times.length; i++) assert.ok(times[i] - times[i - 10] >= 60000, "never 11 in a minute");
  assert.ok(times.length >= 10, "but it does keep up");
  assert.equal(saved(hangar).answers[logic.stepKeys(LIST.steps)[0]].note, "note 179");
});

test("flush writes at once, for a page that is leaving", async () => {
  const { hangar, controller } = await opened();
  controller.answer(0, "ok");
  await controller.flush();
  assert.equal(hangar.callsTo("data.write").length, 1);
  await controller.flush();
  assert.equal(hangar.callsTo("data.write").length, 1, "nothing new, nothing written");
});

test("data denied: the tab works, says answers are not kept, and stops trying", async () => {
  const { hangar, controller, state, clock } = await opened({ deny: ["data.read", "data.write"] });
  assert.equal(state.view, "list");
  assert.equal(state.persist, false);
  controller.answer(0, "ok");
  await clock.advance(SAVE_DELAY_MS + SAVE_SPACING_MS);
  assert.equal(hangar.callsTo("data.write").length, 0);
  assert.equal(state.counts.ok, 1);

  const writeOnly = await opened({ deny: ["data.write"] });
  writeOnly.controller.answer(0, "ok");
  await writeOnly.clock.advance(SAVE_DELAY_MS);
  assert.equal(writeOnly.state.persist, false, "a refused write turns saving off, once");
  assert.equal(writeOnly.hangar.callsTo("data.write").length, 1);
});

test("a rate-limited write is tried again, not lost", async () => {
  const clock = fakeClock();
  const hangar = fakeHangar({ params: { path: LIST_PATH }, files: { [LIST_PATH]: JSON.stringify(LIST) }, now: clock.now, writeLimit: 1 });
  const { controller, state } = await opened({ hangar, clock });
  controller.answer(0, "ok");
  await clock.advance(SAVE_DELAY_MS);
  controller.answer(1, "ok");
  await clock.advance(SAVE_SPACING_MS + 1000);
  assert.equal(state.persist, true, "a limit is not a refusal");
  assert.equal(saved(hangar).answers[state.keys[1]], undefined, "not written yet");
  await clock.advance(70000);
  assert.equal(saved(hangar).answers[state.keys[1]].status, "ok", "written once the minute is over");
});

test("send gives the agent that asked a plain summary, and marks the result as sent", async () => {
  const { hangar, controller, state } = await opened();
  controller.answer(0, "ok");
  controller.answer(1, "fail");
  controller.setNote(1, "Blank dashboard.");
  controller.answer(2, "skip");
  assert.equal(await controller.send(), true);
  const [call] = hangar.callsTo("panes.send");
  assert.equal(call.args.pane, "Ava");
  assert.match(call.args.text, /FAILED:\n- Step 2: .*\n {2}Expected: .*\n {2}Note: Blank dashboard\./);
  assert.match(call.args.text, /SKIPPED:\n- Step 3:/);
  assert.equal(state.send.status, "sent");
  assert.equal(hangar.callsTo("notify").length, 1);
  assert.ok(typeof state.saved.sentAt === "number");
  assert.equal(saved(hangar).sentAt, state.saved.sentAt, "and that is kept");
});

test("an answer after sending is a result not yet sent", async () => {
  const { controller, state } = await opened();
  controller.answer(0, "ok");
  await controller.send();
  assert.equal(state.send.status, "sent");
  controller.answer(1, "ok");
  assert.equal(state.send.status, "idle");
  assert.equal(state.saved.sentAt, null);
});

test("nothing to send, or nobody to send to: it does not call panes.send", async () => {
  const empty = await opened();
  assert.equal(await empty.controller.send(), false);
  const person = await opened({ caller: null });
  assert.equal(person.state.caller, null);
  person.controller.answer(0, "ok");
  assert.equal(await person.controller.send(), false);
  assert.equal(person.hangar.callsTo("panes.send").length, 0);
});

test("a refused send says why, keeps the answers, and can be tried again", async () => {
  for (const code of ["denied", "not_found", "unavailable", "rate_limited"]) {
    const { hangar, controller, state } = await opened({ fail: { "panes.send": code } });
    controller.answer(0, "fail");
    assert.equal(await controller.send(), false, code);
    assert.deepEqual([state.send.status, state.send.error.code], ["error", code]);
    assert.equal(state.counts.fail, 1);
    assert.equal(state.saved.sentAt, null, "not sent, not marked");
    hangar.callsTo("panes.send");
    assert.notEqual(logic.t("en", `send.${code}`), `send.${code}`);
  }
  const { controller, state } = await opened({ fail: { "panes.send": "weird" } });
  controller.answer(0, "ok");
  await controller.send();
  assert.equal(state.send.error.code, "error");
});

test("notify denied does not undo a send", async () => {
  const { controller, state } = await opened({ deny: ["notify"] });
  controller.answer(0, "ok");
  assert.equal(await controller.send(), true);
  assert.equal(state.send.status, "sent");
});

test("asking for the same list again keeps the answers and marks the failed ones to redo", async () => {
  const { hangar, controller, state } = await opened();
  controller.answer(0, "ok");
  controller.answer(1, "fail");
  controller.setNote(1, "Blank dashboard.");
  controller.answer(2, "skip");
  await controller.send();
  await controller.flush();
  const keys = state.keys;

  hangar.emit("tab.params", { params: { path: LIST_PATH }, caller: { pane: "Ava" } });
  await settle();
  assert.equal(state.view, "list");
  assert.equal(logic.statusOf(state.saved, keys[0]), "ok", "kept");
  assert.equal(logic.statusOf(state.saved, keys[2]), "skip", "kept");
  assert.equal(logic.statusOf(state.saved, keys[1]), "redo");
  assert.equal(state.saved.redo[keys[1]], "Blank dashboard.");
  assert.equal(state.counts.state, "running");
  assert.deepEqual(badges(hangar).at(-1), { state: "running", text: "2/3" });
  assert.equal(state.send.status, "idle");

  controller.answer(1, "ok");
  await controller.send();
  assert.equal(hangar.callsTo("panes.send").length, 2);
  assert.match(hangar.callsTo("panes.send")[1].args.text, /Nothing failed/);
  assert.deepEqual(badges(hangar).at(-1), { state: "success", text: "3/3" });
});

test("a failure that was not sent stays a failure when the tab is asked for again", async () => {
  const { hangar, controller, state } = await opened();
  controller.answer(1, "fail");
  hangar.emit("tab.params", { params: { path: LIST_PATH }, caller: { pane: "Ava" } });
  await settle();
  assert.equal(logic.statusOf(state.saved, state.keys[1]), "fail");
});

test("the failed-to-redo mark survives a restart too", async () => {
  const first = await opened();
  first.controller.answer(1, "fail");
  first.controller.setNote(1, "bad");
  await first.controller.send();
  await first.controller.flush();
  const second = await opened({ hangar: fakeHangar({ params: { path: LIST_PATH }, caller: { pane: "Ava" }, files: { [LIST_PATH]: JSON.stringify(LIST) }, data: Object.fromEntries(first.hangar.data) }) });
  assert.equal(logic.statusOf(second.state.saved, second.state.keys[1]), "redo");
  await second.controller.flush();
  assert.equal(JSON.parse(second.hangar.data.get(logic.dataPath(LIST_PATH))).redo[second.state.keys[1]], "bad");
});

test("the agent rewording a step loses that step's answer and no other's", async () => {
  const { hangar, controller, state } = await opened();
  controller.answer(0, "ok");
  controller.answer(1, "ok");
  await controller.flush();
  const edited = { ...LIST, steps: [LIST.steps[0], { ...LIST.steps[1], text: "Sign in with the other account." }, LIST.steps[2]] };
  hangar.files.set(LIST_PATH, JSON.stringify(edited));
  hangar.emit("tab.params", { params: { path: LIST_PATH }, caller: null });
  await settle();
  assert.equal(logic.statusOf(state.saved, state.keys[0]), "ok");
  assert.equal(logic.statusOf(state.saved, state.keys[1]), "todo");
});

test("two lists keep their answers apart", async () => {
  const hangar = fakeHangar({ params: { path: "a.json" }, files: { "a.json": JSON.stringify(LIST), "b.json": JSON.stringify(LIST) } });
  const { controller, state } = await opened({ hangar });
  controller.answer(0, "ok");
  await controller.flush();
  hangar.emit("tab.params", { params: { path: "b.json" }, caller: null });
  await settle();
  assert.equal(state.counts.answered, 0);
});

test("a missing, malformed or unreadable list is said, and nothing else breaks", async () => {
  const cases = [
    [{ params: {} }, "missing"],
    [{ params: { path: "../x.json" } }, "path"],
    [{ params: { path: "nope.json" } }, "not_found"],
    [{ params: { path: "bad.json" }, files: { "bad.json": "{oops" } }, "json"],
    [{ params: { path: "empty.json" }, files: { "empty.json": "" } }, "empty"],
    [{ params: { path: "shape.json" }, files: { "shape.json": '{"steps":[{}]}' } }, "shape"],
    [{ params: { path: "a.json" }, files: { "a.json": "[]" }, deny: ["files.read"] }, "denied"],
  ];
  for (const [options, kind] of cases) {
    const { hangar, controller, state } = await opened({ hangar: fakeHangar({ caller: { pane: "Ava" }, ...options }) });
    assert.equal(state.view, "error", kind);
    assert.equal(state.error.kind, kind);
    assert.equal(hangar.callsTo("data.write").length, 0);
    assert.deepEqual(badges(hangar).at(-1) || { state: null }, { state: null }, "no stale badge");
    controller.answer(0, "ok");
    assert.equal(await controller.send(), false);
    assert.equal(hangar.callsTo("panes.send").length, 0);
  }
});

test("retry reads the list again, and a list written meanwhile shows", async () => {
  const hangar = fakeHangar({ params: { path: LIST_PATH }, caller: { pane: "Ava" } });
  const { controller, state } = await opened({ hangar });
  assert.equal(state.error.kind, "not_found");
  hangar.files.set(LIST_PATH, JSON.stringify(LIST));
  await controller.retry();
  await settle();
  assert.equal(state.view, "list");
});

test("a later request makes an earlier slow answer stale", async () => {
  const hangar = fakeHangar({ params: { path: "a.json" }, files: { "a.json": JSON.stringify(LIST), "b.json": JSON.stringify({ title: "B", steps: [{ text: "only" }] }) } });
  const release = {};
  const original = hangar.call.bind(hangar);
  hangar.call = (method, args) => (method === "files.read" && args.path === "a.json" ? new Promise((resolve) => (release.a = () => resolve(original(method, args)))) : original(method, args));
  const controller = createController({ hangar, logic, t });
  controller.start(); // waits on a.json
  await settle();
  assert.equal(controller.state.view, "loading");
  hangar.emit("tab.params", { params: { path: "b.json" }, caller: null });
  await settle();
  release.a();
  await settle();
  assert.equal(controller.state.list.title, "B");
  controller.dispose();
});

test("a badge refused by Hangar never breaks the tab", async () => {
  const { state, controller } = await opened({ fail: { "tab.setBadge": "denied", "tab.setTitle": "invalid" } });
  assert.equal(state.view, "list");
  controller.answer(0, "ok");
  assert.equal(state.counts.ok, 1);
});

test("dispose stops everything", async () => {
  const { hangar, controller, clock } = await opened();
  controller.answer(0, "ok");
  controller.dispose();
  await clock.advance(60000);
  assert.equal(hangar.callsTo("data.write").length, 0);
  assert.equal(clock.pending(), 0);
});
