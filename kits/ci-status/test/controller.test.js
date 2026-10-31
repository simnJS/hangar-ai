"use strict";
// The tab's behaviour against a fake window.hangar and a fake clock: what it asks,
// what it sets, when it polls and stops, when it notifies.
const test = require("node:test");
const assert = require("node:assert/strict");
const logic = require("../tabs/ci/logic.js");
const { createController, POLL_MS } = require("../tabs/ci/controller.js");
const { fixture, fakeClock, fakeHangar, settle } = require("./helpers.js");

const ID = "41222364482";

function setup({ params = { id: ID }, commands, lang = "en", denyNotify } = {}) {
  const clock = fakeClock();
  const hangar = fakeHangar({ params, commands, denyNotify });
  const states = [];
  const controller = createController({
    hangar, logic, lang, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer,
    onChange: (state) => states.push(state.view),
  });
  return { clock, hangar, controller, states };
}

const answer = (name) => ({ exitCode: 0, stdout: fixture(name) });
const badges = (hangar) => hangar.callsTo("tab.setBadge").map((call) => call.args.state);
const runCalls = (hangar, name) => hangar.callsTo("command.run").filter((call) => call.args.name === name);

test("running, then success: title, badge, polling every 5 s, one notification, then silence", async () => {
  const replies = [answer("run-running.json"), answer("run-running.json"), answer("run-success.json")];
  const { clock, hangar, controller } = setup({ commands: { "ci-run": () => replies.shift() } });
  await controller.start();
  await settle();

  assert.deepEqual(hangar.callsTo("tab.setTitle").map((call) => call.args.title), ["CI"]);
  assert.deepEqual(badges(hangar), ["running"]);
  assert.equal(controller.state.view, "run");
  assert.equal(hangar.callsTo("notify").length, 0);
  assert.equal(runCalls(hangar, "ci-run").length, 1);
  assert.deepEqual(runCalls(hangar, "ci-run")[0].args.args, { id: ID });

  await clock.advance(POLL_MS - 1);
  assert.equal(runCalls(hangar, "ci-run").length, 1, "not before 5 s");
  await clock.advance(1);
  assert.equal(runCalls(hangar, "ci-run").length, 2);
  assert.deepEqual(badges(hangar), ["running"], "an unchanged badge is not set again");

  await clock.advance(POLL_MS);
  assert.equal(runCalls(hangar, "ci-run").length, 3);
  assert.deepEqual(badges(hangar), ["running", "success"]);
  const [notice, ...more] = hangar.callsTo("notify");
  assert.equal(more.length, 0, "once");
  assert.match(notice.args.title, /CI: passed/);
  assert.equal(runCalls(hangar, "ci-log").length, 0, "no log for a success");

  await clock.advance(10 * 60 * 1000);
  assert.equal(runCalls(hangar, "ci-run").length, 3, "polling stopped");
  assert.equal(clock.pending(), 0);
  assert.equal(hangar.callsTo("notify").length, 1);
});

test("a failure: badge failure, notification, and the failed step's log", async () => {
  const replies = [answer("run-running.json"), answer("run-failure.json")];
  const { clock, hangar, controller } = setup({
    lang: "fr",
    commands: { "ci-run": () => replies.shift(), "ci-log": () => answer("log-failed.txt") },
  });
  await controller.start();
  await clock.advance(POLL_MS);

  assert.deepEqual(badges(hangar), ["running", "failure"]);
  assert.match(hangar.callsTo("notify")[0].args.title, /CI : échouée/);
  assert.equal(runCalls(hangar, "ci-log").length, 1);
  assert.deepEqual(runCalls(hangar, "ci-log")[0].args.args, { id: ID });
  assert.equal(controller.state.log.shown, 40);
  assert.ok(controller.state.log.lines.some((line) => line.kind === "error"));
  assert.equal(clock.pending(), 0);
});

test("a run already over when opened: its result, no notification, no polling", async () => {
  const { clock, hangar, controller } = setup({ commands: { "ci-run": () => answer("run-success.json") } });
  await controller.start();
  await clock.advance(60000);
  assert.deepEqual(badges(hangar), ["success"]);
  assert.equal(hangar.callsTo("notify").length, 0);
  assert.equal(runCalls(hangar, "ci-run").length, 1);
});

test("a notification the kit may not raise does not break the tab", async () => {
  const replies = [answer("run-running.json"), answer("run-failure.json")];
  const { clock, hangar, controller } = setup({
    denyNotify: true,
    commands: { "ci-run": () => replies.shift(), "ci-log": () => answer("log-failed.txt") },
  });
  await controller.start();
  await clock.advance(POLL_MS);
  assert.equal(controller.state.view, "run");
  assert.ok(controller.state.log);
  assert.equal(hangar.callsTo("notify").length, 1);
});

test("a log that cannot be read leaves the result standing", async () => {
  const { hangar, controller } = setup({
    commands: { "ci-run": () => answer("run-failure.json"), "ci-log": () => ({ exitCode: 1, stderr: "run 1 is still in progress" }) },
  });
  await controller.start();
  await settle();
  assert.equal(controller.state.view, "run");
  assert.deepEqual(controller.state.log, { error: "error" });
  assert.deepEqual(badges(hangar), ["failure"]);
});

test("gh not logged in: says so, takes the badge off, does not poll, retries on request", async () => {
  const stderr = fixture("not-logged-in.stderr");
  let signedIn = false;
  const { clock, hangar, controller } = setup({
    commands: { "ci-run": () => (signedIn ? answer("run-success.json") : { exitCode: 4, stderr }) },
  });
  await controller.start();
  await clock.advance(60000);
  assert.equal(controller.state.view, "error");
  assert.equal(controller.state.error.kind, "no-auth");
  assert.equal(runCalls(hangar, "ci-run").length, 1);
  assert.deepEqual(hangar.callsTo("tab.setBadge").map((call) => call.args.state), [null]);

  signedIn = true;
  await controller.retry();
  assert.equal(controller.state.view, "run");
  assert.deepEqual(badges(hangar), [null, "success"]);
});

test("gh absent: not_found is explained, nothing throws", async () => {
  const { hangar, controller } = setup({
    commands: { "ci-run": () => { throw Object.assign(new Error("gh is not installed"), { code: "not_found" }); } },
  });
  await controller.start();
  await settle();
  assert.equal(controller.state.error.kind, "no-gh");
  assert.equal(hangar.callsTo("notify").length, 0);
});

test("the kit's commands not accepted: denied is explained", async () => {
  const { controller } = setup({ commands: {} });
  await controller.start();
  await settle();
  assert.equal(controller.state.error.kind, "denied");
});

test("a hiccup while following a run is retried; three in a row end the vigil", async () => {
  const replies = [answer("run-running.json")];
  const { clock, hangar, controller } = setup({
    commands: { "ci-run": () => replies.shift() || { exitCode: 1, stderr: "error connecting to api.github.com" } },
  });
  await controller.start();
  await clock.advance(POLL_MS);
  assert.equal(controller.state.view, "run");
  assert.equal(controller.state.warning, true);
  await clock.advance(POLL_MS);
  assert.equal(controller.state.view, "run");
  await clock.advance(POLL_MS);
  assert.equal(controller.state.view, "error");
  assert.equal(runCalls(hangar, "ci-run").length, 4);
  assert.equal(clock.pending(), 0);
});

test("a hiccup that passes clears the warning", async () => {
  const replies = [answer("run-running.json"), { exitCode: 1, stderr: "timeout" }, answer("run-running.json")];
  const { clock, controller } = setup({ commands: { "ci-run": () => replies.shift() } });
  await controller.start();
  await clock.advance(POLL_MS);
  assert.equal(controller.state.warning, true);
  await clock.advance(POLL_MS);
  assert.equal(controller.state.warning, false);
});

test("without an id: a picker of recent runs, and choosing one follows it", async () => {
  const { clock, hangar, controller } = setup({
    params: {},
    commands: { "ci-list": () => answer("run-list.json"), "ci-run": () => answer("run-success.json") },
  });
  await controller.start();
  await settle();
  assert.equal(controller.state.view, "picker");
  assert.equal(controller.state.runs.length, 15);
  assert.equal(runCalls(hangar, "ci-run").length, 0);
  assert.deepEqual(hangar.callsTo("tab.setTitle").map((call) => call.args.title), ["CI"]);

  await controller.openRun("41231065926");
  assert.equal(controller.state.view, "run");
  assert.equal(controller.state.fromPicker, true);
  assert.deepEqual(runCalls(hangar, "ci-run")[0].args.args, { id: "41231065926" });

  await controller.back();
  assert.equal(controller.state.view, "picker");
  assert.equal(hangar.callsTo("tab.setTitle").at(-1).args.title, "CI");
  assert.equal(clock.pending(), 0);
});

test("an id that is not digits is no id: the picker", async () => {
  for (const id of ["abc", "-1", "12 34", ""]) {
    const { hangar, controller } = setup({ params: { id }, commands: { "ci-list": () => answer("run-list.json") } });
    await controller.start();
    await settle();
    assert.equal(controller.state.view, "picker", id);
    assert.equal(runCalls(hangar, "ci-run").length, 0);
  }
});

test("the picker without gh logged in says so", async () => {
  const { controller } = setup({
    params: {},
    commands: { "ci-list": () => ({ exitCode: 4, stderr: fixture("not-logged-in.stderr") }) },
  });
  await controller.start();
  await settle();
  assert.equal(controller.state.view, "error");
  assert.equal(controller.state.error.kind, "no-auth");
});

test("leaving a run mid-poll drops its answers", async () => {
  let release;
  const slow = new Promise((resolve) => (release = resolve));
  const { clock, controller } = setup({
    params: {},
    commands: {
      "ci-list": () => answer("run-list.json"),
      "ci-run": async () => {
        await slow;
        return answer("run-running.json");
      },
    },
  });
  await controller.start();
  await settle();
  const opening = controller.openRun("5");
  await settle();
  await controller.back();
  release();
  await opening;
  await settle();
  assert.equal(controller.state.view, "picker");
  assert.equal(clock.pending(), 0);
});
