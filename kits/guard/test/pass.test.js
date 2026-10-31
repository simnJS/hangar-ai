"use strict";
// One-time passes, from the tab's click to the hook's retry: exact command, exact
// agent, two minutes, once. The tab's real code writes them, the hook's real code
// takes them, on a real folder.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { decide } = require("../hooks/decide.js");
const store = require("../hooks/store.js");
const logic = require("../tabs/guard/logic.js");
const { createController } = require("../tabs/guard/controller.js");
const { tempFolder, fakeClock, fakeHangar, bashEvent, readLog, runHook } = require("./helpers.js");

function setup(t) {
  const root = tempFolder(t);
  const dir = path.join(root, "guard");
  const project = path.join(root, "project");
  fs.mkdirSync(project);
  const env = (pane = "pane-1", name = "Ava") => ({ HANGAR_PANE_ID: pane, HANGAR_PANE_NAME: name, CLAUDE_PROJECT_DIR: project });
  const run = (command, { now = 1000, pane, name, opened, extra } = {}) =>
    decide(bashEvent(command, project, extra), { env: env(pane, name), now, openTab: opened, context: { dataDir: dir } });
  return { dir, project, run };
}

const lastRefusal = (dir) => readLog(dir).filter((event) => event.type === "refusal").pop();

function writePass(dir, event, now, change = {}) {
  const pass = { ...logic.passFor({ ...event }, now), ...change };
  const file = path.join(dir, logic.passPath(event.key));
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(pass));
  return file;
}

test("a refusal asks for the tab; an allowed call does not", (t) => {
  const { run } = setup(t);
  let opened = 0;
  assert.equal(run("npm test", { opened: () => opened++ }), null);
  assert.equal(opened, 0);
  assert.ok(run("git reset --hard", { opened: () => opened++ }));
  assert.equal(opened, 1);
});

test("the pass lets that command through once, then it is refused again", (t) => {
  const { dir, run } = setup(t);
  assert.ok(run("git reset --hard", { now: 1000 }));
  const file = writePass(dir, lastRefusal(dir), 2000);
  assert.equal(run("git reset --hard", { now: 3000 }), null, "through, with no answer: Claude Code's own flow goes on");
  assert.equal(fs.existsSync(file), false, "used up");
  const used = readLog(dir).pop();
  assert.equal(used.type, "pass-used");
  assert.equal(used.key, lastRefusal(dir).key);
  assert.ok(run("git reset --hard", { now: 4000 }), "once");
});

test("a pass never answers allow: it only stops refusing", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000 });
  writePass(dir, lastRefusal(dir), 2000);
  assert.equal(run("git reset --hard", { now: 3000 }), null);
  for (const name of fs.readdirSync(path.join(__dirname, "..", "hooks"))) {
    assert.doesNotMatch(fs.readFileSync(path.join(__dirname, "..", "hooks", name), "utf8"), /permissionDecision:\s*"allow"/, name);
  }
});

test("it is for that exact command", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000 });
  const file = writePass(dir, lastRefusal(dir), 2000);
  for (const other of ["git reset --hard HEAD~1", "git  reset --hard", "git reset --hard ", "cd . && git reset --hard"]) {
    assert.ok(run(other, { now: 3000 }), JSON.stringify(other));
  }
  assert.ok(fs.existsSync(file), "and still there for it");
  assert.equal(run("git reset --hard", { now: 3000 }), null);
});

test("it is for that agent", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000, pane: "pane-1" });
  const file = writePass(dir, lastRefusal(dir), 2000);
  assert.ok(run("git reset --hard", { now: 3000, pane: "pane-2", name: "Ben" }));
  assert.ok(fs.existsSync(file));
  // A subagent of that pane is not that agent.
  assert.ok(run("git reset --hard", { now: 3000, extra: { agent_id: "sub-1", agent_type: "Explore" } }));
  assert.equal(lastRefusal(dir).agent, "Ava · Explore");
  assert.ok(fs.existsSync(file));
  assert.equal(run("git reset --hard", { now: 3000, pane: "pane-1" }), null);
});

test("it holds for two minutes, and no longer", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000 });
  const event = lastRefusal(dir);
  writePass(dir, event, 2000);
  assert.equal(run("git reset --hard", { now: 2000 + 2 * 60 * 1000 - 1 }), null, "just in time");
  const file = writePass(dir, event, 2000);
  assert.ok(run("git reset --hard", { now: 2000 + 2 * 60 * 1000 }), "too late");
  assert.equal(fs.existsSync(file), false, "and gone");
});

test("a pass the tab could not have written is worth nothing", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000 });
  const event = lastRefusal(dir);
  const now = 5000;
  const cases = [
    ["set too far ahead", (file) => writePass(dir, event, now, { expiresAt: now + 3 * 60 * 1000 })],
    ["for ever", () => writePass(dir, event, now, { expiresAt: Number.MAX_SAFE_INTEGER })],
    ["no expiry", () => writePass(dir, event, now, { expiresAt: undefined })],
    ["another key inside", () => writePass(dir, event, now, { key: "0".repeat(64) })],
    ["another agent inside", () => writePass(dir, event, now, { agentId: "pane-2" })],
    ["not JSON", () => {
      const file = writePass(dir, event, now);
      fs.writeFileSync(file, "{ nope");
      return file;
    }],
    ["not an object", () => {
      const file = writePass(dir, event, now);
      fs.writeFileSync(file, "42");
      return file;
    }],
  ];
  for (const [what, write] of cases) {
    const file = write();
    assert.ok(run("git reset --hard", { now }), what);
    assert.equal(fs.existsSync(file), false, `${what}: removed`);
  }
});

test("two calls racing for one pass: one goes through", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000 });
  const event = lastRefusal(dir);
  writePass(dir, event, 2000);
  assert.equal(store.takePass(dir, event.key, event.agentId, 3000), true);
  assert.equal(store.takePass(dir, event.key, event.agentId, 3000), false);
});

test("passes nobody came back for are swept at the next refusal", (t) => {
  const { dir, run } = setup(t);
  run("git reset --hard", { now: 1000 });
  const event = lastRefusal(dir);
  const old = writePass(dir, event, 2000);
  const longAgo = new Date(Date.now() - 60 * 60 * 1000);
  fs.utimesSync(old, longAgo, longAgo);
  const fresh = path.join(path.dirname(old), `${"a".repeat(64)}.json`);
  fs.writeFileSync(fresh, "{}");
  run("git clean -fd", { now: Date.now() });
  assert.equal(fs.existsSync(old), false);
  assert.equal(fs.existsSync(fresh), true);
});

test("the file tools get a pass of their own: that tool, that file", (t) => {
  const { dir, project } = setup(t);
  const env = { HANGAR_PANE_ID: "pane-1", HANGAR_PANE_NAME: "Ava", CLAUDE_PROJECT_DIR: project };
  const file_path = path.join(project, ".env");
  const event = (tool_name, tool_input) => JSON.stringify({ session_id: "s", cwd: project, tool_name, tool_input });
  const run = (tool, input, now) => decide(event(tool, input), { env, now, context: { dataDir: dir } });
  assert.ok(run("Read", { file_path }, 1000));
  const refused = lastRefusal(dir);
  assert.equal(refused.command, `Read ${file_path}`);
  writePass(dir, refused, 2000);
  assert.ok(run("Write", { file_path, content: "x" }, 3000), "another tool");
  assert.ok(run("Read", { file_path: path.join(project, ".env.local") }, 3000), "another file");
  assert.equal(run("Read", { file_path }, 3000), null);
  assert.ok(run("Read", { file_path }, 4000));
});

test("a long command: cut for the log, whole for the pass", (t) => {
  const { dir, run } = setup(t);
  const long = `git reset --hard # ${"x".repeat(5000)}`;
  run(long, { now: 1000 });
  const event = lastRefusal(dir);
  assert.equal(event.command.length, store.COMMAND_MAX);
  assert.equal(event.length, long.length);
  writePass(dir, event, 2000);
  assert.ok(run(`git reset --hard # ${"x".repeat(5001)}`, { now: 3000 }), "another command with the same start");
  assert.equal(run(long, { now: 3000 }), null);
});

test("the log stays small enough for the tab to read", (t) => {
  const { dir, run } = setup(t);
  const huge = `git reset --hard # ${"x".repeat(1900)}`;
  for (let i = 0; i < 400; i++) run(`${huge}${i}`, { now: 1000 + i });
  const { size } = fs.statSync(path.join(dir, store.LOG));
  assert.ok(size < 1024 * 1024, `${size} bytes: a tab reads 1 MB at most`);
  const events = readLog(dir);
  assert.ok(events.length > 50, "every line left is whole");
  assert.equal(events.at(-1).at, 1000 + 399, "the newest stay");
});

test("from refusal to retry: the hook logs, the tab lists, the click writes, the hook takes", async (t) => {
  const { dir, run } = setup(t);
  const clock = fakeClock(Date.parse("2026-10-31T18:00:00Z"));
  run("git reset --hard", { now: clock.now() - 3000 });
  run("git reset --hard", { now: clock.now() - 2000 });
  run("git clean -fd", { now: clock.now() - 1000 });

  const hangar = fakeHangar({ folder: dir });
  const controller = createController({ hangar, logic, now: clock.now, setTimer: clock.setTimer, clearTimer: clock.clearTimer });
  t.after(() => controller.dispose());
  await controller.start();
  assert.equal(controller.state.rows.length, 2);
  const row = controller.state.rows.find((candidate) => candidate.command === "git reset --hard");
  assert.equal(row.count, 2);
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: "failure", text: "2" });

  await controller.allow(row.key);
  assert.equal(logic.passState(row, controller.state.grants[row.key], clock.now()).state, "active");
  assert.ok(fs.existsSync(path.join(dir, "passes", `${row.key}.json`)));

  await clock.advance(10 * 1000);
  assert.equal(run("git reset --hard", { now: clock.now() }), null, "the retry goes through");
  assert.ok(run("git clean -fd", { now: clock.now() }), "and only that one");

  await clock.advance(2000);
  const updated = controller.state.rows.find((candidate) => candidate.key === row.key);
  assert.equal(logic.passState(updated, controller.state.grants[row.key], clock.now()).state, "used");
  assert.deepEqual(hangar.callsTo("tab.setBadge").pop().args, { state: "failure", text: "1" });
});

test("the real hook process takes a pass the tab's code wrote", async (t) => {
  const { dir, project } = setup(t);
  const env = { HANGAR_KITS_DATA: path.dirname(dir), HANGAR_PANE_ID: "pane-1", HANGAR_PANE_NAME: "Ava", CLAUDE_PROJECT_DIR: project };
  const refused = await runHook(bashEvent("git reset --hard", project), env);
  assert.ok(refused.stdout);
  writePass(dir, lastRefusal(dir), Date.now());
  const passed = await runHook(bashEvent("git reset --hard", project), env);
  assert.equal(passed.code, 0);
  assert.equal(passed.stdout, "");
  assert.ok((await runHook(bashEvent("git reset --hard", project), env)).stdout, "once");
});
