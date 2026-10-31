"use strict";
// The hook as Claude Code starts it: events on stdin, a data folder on disk, an exit code.
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { fixture, tempFolder, runHook, successEvent, failureEvent } = require("./helpers.js");
const { recordFor } = require("../hooks/capture.js");

const dataOf = (root) => path.join(root, "tests");
const readIndex = (root) => JSON.parse(fs.readFileSync(path.join(dataOf(root), "index.json"), "utf8"));
const readRun = (root, key) => JSON.parse(fs.readFileSync(path.join(dataOf(root), "runs", `${key}.json`), "utf8"));
const envFor = (root, extra = {}) => ({ HANGAR_KITS_DATA: root, HANGAR_PANE_NAME: "Emma", CLAUDE_PROJECT_DIR: "/work/project", ...extra });

test("a failing run arrives as a failed call: it is read, and kept for the tab", async (t) => {
  const root = tempFolder(t);
  const { code, stdout, stderr } = await runHook(failureEvent("pnpm vitest run", fixture("vitest.fail.txt")), envFor(root));
  assert.equal(code, 0);
  assert.equal(stdout, "", "it says nothing to the agent");
  assert.equal(stderr, "");
  const [entry] = readIndex(root).runs;
  assert.deepEqual([entry.status, entry.framework, entry.passed, entry.failed, entry.skipped, entry.project, entry.agent], ["failed", "vitest", 1, 2, 1, "project", "Emma"]);
  const run = readRun(root, entry.key);
  assert.deepEqual(run.failures.map((item) => item.name), ["math > adds wrong", "math > throws"]);
  assert.deepEqual([run.command, run.cwd, run.exitCode, run.version], ["pnpm vitest run", "/work/project", 1, 1]);
  assert.ok(Math.abs(run.at - Date.now()) < 10000);
});

test("a passing run arrives as a successful call", async (t) => {
  const root = tempFolder(t);
  assert.equal((await runHook(successEvent("cargo test", fixture("cargo.pass.txt")), envFor(root))).code, 0);
  const [entry] = readIndex(root).runs;
  assert.deepEqual([entry.status, entry.framework, entry.passed, entry.failed], ["passed", "cargo", 1, 0]);
});

test("PowerShell calls are read like Bash's", async (t) => {
  const root = tempFolder(t);
  await runHook(successEvent("pytest", fixture("pytest.pass.txt"), { tool: "PowerShell" }), envFor(root));
  assert.equal(readIndex(root).runs[0].framework, "pytest");
});

test("an output the hook cannot read is recorded as not understood, never as passed", async (t) => {
  const root = tempFolder(t);
  await runHook(successEvent("pnpm test", "> app@1.0.0 test\n> echo skipping\nskipping\n"), envFor(root));
  const [entry] = readIndex(root).runs;
  assert.equal(entry.status, "not-understood");
  assert.deepEqual([entry.passed, entry.failed], [null, null]);
  assert.deepEqual(readRun(root, entry.key).failures, []);
});

test("a command that is not a test run writes nothing", async (t) => {
  const root = tempFolder(t);
  for (const command of ["ls -la", "git status", 'git commit -m "fix vitest"', "pnpm build"]) {
    assert.equal((await runHook(successEvent(command, fixture("vitest.fail.txt")), envFor(root))).code, 0);
  }
  assert.equal(fs.existsSync(dataOf(root)), false, "not even the folder");
});

test("the same agent in the same project replaces its last run; another agent, or another project, has its own", async (t) => {
  const root = tempFolder(t);
  await runHook(failureEvent("vitest run", fixture("vitest.fail.txt")), envFor(root));
  await runHook(successEvent("vitest run", fixture("vitest.pass.txt")), envFor(root));
  assert.deepEqual(readIndex(root).runs.map((item) => item.status), ["passed"]);
  await runHook(failureEvent("vitest run", fixture("vitest.fail.txt")), envFor(root, { HANGAR_PANE_NAME: "Jack" }));
  await runHook(failureEvent("vitest run", fixture("vitest.fail.txt")), envFor(root, { CLAUDE_PROJECT_DIR: "/work/other" }));
  const runs = readIndex(root).runs;
  assert.equal(runs.length, 3);
  assert.equal(new Set(runs.map((item) => item.key)).size, 3);
  assert.equal(fs.readdirSync(path.join(dataOf(root), "runs")).length, 3, "one file each");
});

test("hooks of several agents at once lose none of their runs", async (t) => {
  const root = tempFolder(t);
  const agents = Array.from({ length: 8 }, (_, i) => `Agent${i}`);
  const results = await Promise.all(agents.map((name) => runHook(failureEvent("vitest run", fixture("vitest.fail.txt")), envFor(root, { HANGAR_PANE_NAME: name }))));
  assert.deepEqual(results.map((item) => item.code), agents.map(() => 0));
  assert.deepEqual(readIndex(root).runs.map((item) => item.agent).sort(), [...agents].sort());
  assert.equal(fs.existsSync(path.join(dataOf(root), ".lock")), false, "the lock is let go");
  assert.deepEqual(fs.readdirSync(dataOf(root)).filter((name) => name.endsWith(".tmp")), []);
});

test("it always exits 0, and says nothing, whatever it is handed", async (t) => {
  const root = tempFolder(t);
  const garbage = ["", "not json", "{", "[]", "null", '{"tool_name":"Bash"}', '{"tool_name":"Bash","tool_input":{"command":42}}', '{"tool_name":"Bash","tool_input":{"command":"pnpm test"}}', '{"tool_name":"Bash","tool_input":{"command":"pnpm test"},"tool_response":{"stdout":{"a":1}}}', '{"tool_name":"Bash","tool_input":{"command":"pnpm test"},"tool_response":null,"error":{"x":1}}'];
  for (const input of garbage) {
    const result = await runHook(input, envFor(root));
    assert.equal(result.code, 0, JSON.stringify(input));
    assert.equal(result.stdout, "");
  }
});

test("a data folder it cannot write is no reason to bother the agent", async (t) => {
  const root = tempFolder(t);
  const blocked = path.join(root, "blocked");
  fs.writeFileSync(blocked, "a file, where a folder should be");
  const result = await runHook(failureEvent("vitest run", fixture("vitest.fail.txt")), envFor(root, { HANGAR_KITS_DATA: blocked }));
  assert.equal(result.code, 0);
  assert.equal(result.stdout, "");
  assert.equal(result.stderr, "");
});

test("it does not slow the agent: a run of 100 000 lines is read in well under a second of its own", async (t) => {
  const root = tempFolder(t);
  const noise = "    ✓ some test name that is long enough to look like a real one 1ms\n".repeat(100000);
  const input = successEvent("vitest run", `${noise}${fixture("vitest.fail.txt")}`);
  await runHook(successEvent("ls", ""), envFor(root)); // a first start warms the disk cache
  const { code, ms } = await runHook(input, envFor(root));
  assert.equal(code, 0);
  assert.ok(ms < 3000, `${ms} ms`);
  const idle = await runHook(successEvent("ls", "x"), envFor(root));
  assert.ok(idle.ms < 1500, `a call that is no test run costs a Node start: ${idle.ms} ms`);
  assert.equal(readIndex(root).runs[0].failed, 2);
});

test("an event larger than the hook reads is left alone", async (t) => {
  const root = tempFolder(t);
  const result = await runHook(successEvent("vitest run", "x".repeat(25 * 1024 * 1024)), envFor(root));
  assert.equal(result.code, 0);
  assert.equal(fs.existsSync(dataOf(root)), false);
});

test("the record: the agent, the project and the exit code, from where Hangar puts them", () => {
  const now = Date.parse("2026-10-31T17:02:00Z");
  const event = JSON.parse(failureEvent("vitest run", fixture("vitest.fail.txt"), { cwd: "C:\\code\\my-app\\packages\\ui", exitCode: 2 }));
  const run = recordFor(event, { HANGAR_PANE_NAME: "Ava", CLAUDE_PROJECT_DIR: "C:\\code\\my-app\\" }, now);
  assert.deepEqual([run.project, run.agent, run.cwd, run.exitCode, run.at], ["my-app", "Ava", "C:\\code\\my-app\\packages\\ui", 2, now]);
  assert.match(run.key, /^[0-9a-f]{16}$/);

  const byPane = recordFor(event, { HANGAR_PANE_ID: "94bc62aa-6521-4ec1-82d6-11f878c231e7" }, now);
  assert.equal(byPane.agent, "94bc62aa", "without a name, the start of the pane's id");
  const bySession = recordFor(event, {}, now);
  assert.equal(bySession.agent, "session-", "without Hangar, the session's");
  assert.equal(recordFor({ ...event, tool_name: "Read" }, {}, now), null);
});

test("an exit code in the response, and a long command line", () => {
  const event = { tool_name: "Bash", cwd: "/p", tool_input: { command: `pnpm test # ${"x".repeat(2000)}` }, tool_response: { stdout: fixture("pnpm-test.txt"), stderr: "", exit_code: 3 } };
  const run = recordFor(event, {}, 1);
  assert.equal(run.exitCode, 3);
  assert.equal(run.status, "failed", "green summary, non-zero exit");
  assert.ok(run.command.length <= 600);
  assert.ok(run.command.endsWith("…"));
});

test("a tool response as a plain string, or as content blocks, is read too", () => {
  const base = { tool_name: "Bash", cwd: "/p", tool_input: { command: "pytest" } };
  assert.equal(recordFor({ ...base, tool_response: fixture("pytest.fail.txt") }, {}, 1).failed, 2);
  assert.equal(recordFor({ ...base, tool_response: { content: [{ type: "text", text: fixture("pytest.fail.txt") }] } }, {}, 1).failed, 2);
  assert.equal(recordFor({ ...base, tool_response: { stdout: "", stderr: fixture("pytest.fail.txt") } }, {}, 1).failed, 2, "stderr counts");
});

test("old and stale runs are forgotten", async (t) => {
  const root = tempFolder(t);
  const { saveRun, KEEP_RUNS, KEEP_MS } = require("../hooks/store.js");
  const dir = dataOf(root);
  const base = recordFor(JSON.parse(successEvent("vitest run", fixture("vitest.pass.txt"))), { CLAUDE_PROJECT_DIR: "/a" }, 1000);
  saveRun(dir, { ...base, key: "0".repeat(16), at: 1000 });
  saveRun(dir, { ...base, key: "1".repeat(16), at: 1000 + KEEP_MS + 5 });
  assert.deepEqual(readIndex(root).runs.map((item) => item.key), ["1".repeat(16)], "three days later, the first is gone");
  assert.equal(fs.existsSync(path.join(dir, "runs", `${"0".repeat(16)}.json`)), false);
  for (let i = 0; i < KEEP_RUNS + 5; i++) saveRun(dir, { ...base, key: i.toString(16).padStart(16, "a"), at: 2000 + KEEP_MS + i });
  assert.equal(readIndex(root).runs.length, KEEP_RUNS);
  assert.equal(fs.readdirSync(path.join(dir, "runs")).length, KEEP_RUNS);
});
