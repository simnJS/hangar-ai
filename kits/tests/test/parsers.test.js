"use strict";
// The parsers, on outputs recorded from the real tools (see fixtures/): vitest 5.0.3, jest 30,
// pytest 9.2, cargo 1.9x, node 24's test runner. `go` was not installed on the machine that
// recorded them: its fixtures are written by hand in the format `go test` documents, one
// file per shape (plain, -v, -json, build failure), and say so in the test names.
const test = require("node:test");
const assert = require("node:assert/strict");
const { parseRun } = require("../hooks/parse.js");
const { fixture } = require("./helpers.js");

const read = (name, frameworks = []) => parseRun(fixture(name), frameworks, null);
const names = (run) => run.failures.map((item) => item.name);

test("vitest: a failing run, its failures with their place and their message", () => {
  const run = read("vitest.fail.txt", ["vitest"]);
  assert.equal(run.status, "failed");
  assert.equal(run.framework, "vitest");
  assert.deepEqual([run.passed, run.failed, run.skipped, run.duration, run.partial], [1, 2, 1, 274, false]);
  assert.deepEqual(names(run), ["math > adds wrong", "math > throws"]);
  assert.deepEqual([run.failures[0].file, run.failures[0].line], ["a.test.js", 4]);
  assert.match(run.failures[0].message, /expected \{ a: 1, b: \[ 1, 2 \] \} to deeply equal/);
  assert.match(run.failures[0].message, /^- Expected$/m);
  assert.equal(run.failures[1].message, "Error: kaboom");
  assert.doesNotMatch(run.failures[0].message, /❯|2\|/, "the code frame is not the message");
});

test("vitest: a passing run, and the verbose reporter", () => {
  const pass = read("vitest.pass.txt");
  assert.deepEqual([pass.status, pass.passed, pass.failed, pass.skipped, pass.duration], ["passed", 1, 0, 3, 211]);
  const verbose = read("vitest.verbose.fail.txt");
  assert.deepEqual([verbose.failed, verbose.passed], [2, 1]);
  assert.deepEqual(names(verbose), ["math > adds wrong", "math > throws"]);
  const repo = read("pnpm-test.txt");
  assert.deepEqual([repo.status, repo.passed, repo.failed, repo.duration], ["passed", 2224, 0, 53250], "the real `pnpm test` of this repository, through pnpm's header");
});

test("jest: failures, one per bullet, with the line of the stack", () => {
  const run = read("jest.fail.txt");
  assert.equal(run.status, "failed");
  assert.equal(run.framework, "jest");
  assert.deepEqual([run.passed, run.failed, run.skipped, run.duration], [1, 2, 1, 545]);
  assert.deepEqual(names(run), ["add › adds wrong", "add › throws"]);
  assert.deepEqual([run.failures[0].file, run.failures[0].line], ["sum.test.js", 4]);
  assert.match(run.failures[0].message, /Expected: 4\nReceived: 3$/);
  assert.equal(run.failures[1].message, "kaboom");
  assert.deepEqual([read("jest.pass.txt").status, read("jest.pass.txt").passed, read("jest.pass.txt").skipped], ["passed", 1, 3]);
  assert.deepEqual(names(read("jest.verbose.fail.txt")), ["add › adds wrong", "add › throws"]);
});

test("pytest: failures from the short summary, messages from the E lines", () => {
  const run = read("pytest.fail.txt");
  assert.equal(run.status, "failed");
  assert.deepEqual([run.passed, run.failed, run.skipped, run.duration], [3, 2, 1, 70]);
  assert.deepEqual(names(run), ["test_math.py::test_add_wrong", "test_math.py::test_param[2]"]);
  assert.deepEqual([run.failures[0].file, run.failures[0].line], ["test_math.py", 10]);
  assert.equal(run.failures[0].message, "assert 3 == 4\n +  where 3 = add(1, 2)");
  assert.equal(run.failures[1].message, "assert 2 < 2");
  const pass = read("pytest.pass.txt");
  assert.deepEqual([pass.status, pass.passed, pass.failed], ["passed", 1, 0], "deselected tests are neither");
});

test("cargo: totals are the sum of every binary's result; messages come from the panic", () => {
  const run = read("cargo.fail.txt");
  assert.equal(run.status, "failed");
  assert.deepEqual([run.passed, run.failed, run.skipped], [1, 2, 1]);
  assert.deepEqual(names(run), ["tests::adds_wrong", "tests::explodes"]);
  assert.deepEqual([run.failures[0].file, run.failures[0].line], ["src\\lib.rs", 7]);
  assert.equal(run.failures[0].message, "assertion `left == right` failed: one plus two is not four\n  left: 3\n right: 4");
  assert.equal(run.failures[1].message, "index out of bounds: the len is 0 but the index is 3");
  assert.deepEqual([read("cargo.pass.txt").status, read("cargo.pass.txt").passed], ["passed", 1]);

  const two = `${fixture("cargo.pass.txt")}\n   Doc-tests demo\n\nrunning 2 tests\ntest src/lib.rs - add (line 1) ... ok\ntest src/lib.rs - sub (line 9) ... ok\n\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.50s\n`;
  const summed = parseRun(two, ["cargo"], null);
  assert.deepEqual([summed.passed, summed.duration], [3, 500]);
});

test("cargo: a filter that matches nothing ran no test, which is not a pass", () => {
  const run = read("cargo.none.txt");
  assert.equal(run.status, "not-understood");
  assert.equal(run.reason, "no-tests");
});

test("cargo: the message on the panic line, as Rust wrote it before 1.73", () => {
  const old = "running 1 test\ntest t::a ... FAILED\n\nfailures:\n\n---- t::a stdout ----\nthread 't::a' panicked at 'boom', src/lib.rs:7:5\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\nfailures:\n    t::a\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
  const run = parseRun(old, ["cargo"], null);
  assert.deepEqual([run.failures[0].message, run.failures[0].file, run.failures[0].line], ["boom", "src/lib.rs", 7]);
});

test("node:test: the spec reporter, with its failing tests section", () => {
  const run = read("node.spec.fail.txt");
  assert.equal(run.status, "failed");
  assert.equal(run.framework, "node");
  assert.deepEqual([run.passed, run.failed, run.skipped, run.duration], [1, 2, 1, 116]);
  assert.deepEqual(names(run), ["compares objects", "throws"]);
  assert.deepEqual([run.failures[0].file, run.failures[0].line], ["a.test.js", 4]);
  assert.match(run.failures[0].message, /^AssertionError \[ERR_ASSERTION\]: Expected values to be strictly deep-equal:/);
  assert.doesNotMatch(run.failures[0].message, /generatedMessage|at Test\./, "neither the stack nor the error's properties");
  assert.equal(run.failures[1].message, "Error: kaboom\nsecond line");
  assert.equal(read("node.fail.txt").failed, 2, "what a pipe gets by default is the same reporter");
  assert.deepEqual([read("node.spec.pass.txt").status, read("node.spec.pass.txt").passed], ["passed", 2]);
});

test("node:test: TAP, its YAML blocks and their quoting", () => {
  const run = read("node.tap.fail.txt");
  assert.equal(run.status, "failed");
  assert.deepEqual([run.passed, run.failed, run.skipped], [1, 2, 1]);
  assert.deepEqual(names(run), ["compares objects", "throws"]);
  assert.deepEqual([run.failures[0].line, run.failures[1].line], [4, 6]);
  assert.match(run.failures[0].file, /a\.test\.js$/);
  assert.match(run.failures[0].message, /^Expected values to be strictly deep-equal:\n\+ actual - expected/);
  assert.equal(run.failures[1].message, "Error: kaboom\nsecond line".replace("Error: ", ""), "the TAP error is the message, without the class");
  assert.equal(read("node.tap.pass.txt").status, "passed");
});

test("go (hand-written outputs): a plain run counts packages, not tests", () => {
  const run = read("go.fail.txt");
  assert.equal(run.status, "failed");
  assert.deepEqual([run.passed, run.failed], [null, 2]);
  assert.deepEqual(names(run), ["TestAddWrong", "TestTable/negative"], "a parent test is not listed beside its failing subtest");
  assert.deepEqual([run.failures[0].file, run.failures[0].line], ["add_test.go", 12]);
  assert.equal(run.failures[1].message, "add_test.go:27: add(-1, -2) = -3, want -4");
  assert.equal(run.duration, 6);
  const pass = read("go.pass.txt");
  assert.deepEqual([pass.status, pass.passed, pass.failed], ["passed", null, 0], "the count of passing tests is not in the output");
});

test("go (hand-written outputs): -v, -json and a build that failed", () => {
  const verbose = read("go.verbose.fail.txt");
  assert.deepEqual([verbose.passed, verbose.failed, verbose.skipped], [1, 1, 1]);
  assert.equal(verbose.failures[0].message, "add_test.go:12: add(1, 2) = 3, want 4", "the lines before `--- FAIL` are the test's");
  const json = read("go.json.fail.txt");
  assert.deepEqual([json.status, json.passed, json.failed], ["failed", 1, 1]);
  assert.deepEqual([json.failures[0].name, json.failures[0].file, json.failures[0].line], ["TestAddWrong", "add_test.go", 12]);
  const build = read("go.build-failed.txt");
  assert.equal(build.status, "failed");
  assert.deepEqual([build.failures[0].name, build.failures[0].file, build.failures[0].line], ["example.com/demo", "./add_test.go", 9]);
  assert.match(build.failures[0].message, /undefined: addd/);
});

test("colours change nothing: the same run with ANSI escapes reads the same", () => {
  for (const [plain, coloured] of [
    ["vitest.fail.txt", "vitest.fail.ansi.txt"],
    ["jest.fail.txt", "jest.fail.ansi.txt"],
    ["pytest.fail.txt", "pytest.fail.ansi.txt"],
    ["node.spec.fail.txt", "node.spec.fail.ansi.txt"],
  ]) {
    assert.match(fixture(coloured), /\u001b\[/, `${coloured} does hold colours`);
    const a = read(plain);
    const b = read(coloured);
    assert.deepEqual([b.status, b.framework, b.passed, b.failed, b.skipped], [a.status, a.framework, a.passed, a.failed, a.skipped], coloured);
    assert.deepEqual(b.failures.map((item) => [item.name, item.file, item.line, item.message.replace(/\s+/g, " ")]), a.failures.map((item) => [item.name, item.file, item.line, item.message.replace(/\s+/g, " ")]), coloured);
  }
});

test("a line a progress bar overwrote does not count, and Windows line ends read like Unix's", () => {
  const text = fixture("pytest.fail.txt").replace(/\n/g, "\r\n");
  assert.equal(parseRun(text, ["pytest"], null).failed, 2);
  const redrawn = `collecting 1 ...\rcollecting 6 items\r\n${fixture("pytest.fail.txt")}`;
  assert.equal(parseRun(redrawn, ["pytest"], null).failed, 2);
});

test("the command says which parser reads; without it, all of them look", () => {
  assert.equal(parseRun(fixture("pytest.fail.txt"), ["vitest"], null).status, "not-understood", "a vitest command whose output is pytest's is not understood");
  assert.equal(parseRun(fixture("pytest.fail.txt"), [], null).framework, "pytest");
  assert.equal(parseRun(fixture("jest.fail.txt"), [], null).framework, "jest", "jest's FAIL lines are not read as vitest's");
  assert.equal(parseRun(fixture("node.tap.pass.txt"), [], null).framework, "node", "TAP's `ok 1 - x` is not a Go package line");
});

test("two suites in one command line: totals add up, failures stay", () => {
  const output = `${fixture("pnpm-test.txt")}\n${fixture("node.spec.fail.txt")}`;
  const run = parseRun(output, [], 1);
  assert.equal(run.framework, "vitest, node");
  assert.deepEqual([run.status, run.passed, run.failed, run.skipped], ["failed", 2225, 2, 1]);
  assert.deepEqual(names(run), ["compares objects", "throws"]);
});

test("a truncated output is never read as a pass", () => {
  for (const name of ["vitest.pass.txt", "jest.pass.txt", "pytest.pass.txt", "cargo.pass.txt", "node.spec.pass.txt", "node.tap.pass.txt", "pnpm-test.txt"]) {
    const lines = fixture(name).split("\n");
    const summary = lines.findIndex((line) => /Test Files|^Tests:|=+ \d+ passed|^[\d]+ passed|^1 passed|test result:|^ℹ tests|^# tests|^ok /.test(line));
    assert.ok(summary > 0, `${name} has a summary to cut before`);
    for (let keep = 0; keep < summary; keep++) {
      const run = parseRun(lines.slice(0, keep).join("\n"), [], null);
      assert.notEqual(run.status, "passed", `${name} cut after ${keep} lines`);
    }
  }
});

test("a truncated failing output still says what failed, and says it is partial", () => {
  for (const name of ["vitest.fail.txt", "pytest.fail.txt", "cargo.fail.txt", "node.spec.fail.txt", "node.tap.fail.txt", "jest.fail.txt"]) {
    const text = fixture(name);
    const whole = parseRun(text, [], null);
    const lines = text.split("\n");
    let sawFailure = false;
    for (let keep = 1; keep < lines.length; keep++) {
      const run = parseRun(lines.slice(0, keep).join("\n"), [], null);
      assert.notEqual(run.status, "passed", `${name} cut after ${keep} lines`);
      if (run.status === "failed" && run.partial) {
        sawFailure = true;
        assert.ok(run.failures.length >= 1, `${name}: partial means failures seen`);
        assert.equal(run.failed, run.failures.length);
        assert.equal(run.passed, null, "and no count of passes it never saw");
      }
    }
    assert.ok(whole.partial === false, name);
    // Cut mid-message: the failure's name is there, its message shorter.
    const cut = parseRun(text.slice(0, Math.floor(text.length * 0.6)), [], null);
    assert.notEqual(cut.status, "passed", name);
    if (name !== "jest.fail.txt") assert.ok(sawFailure || cut.status === "not-understood", name);
  }
});

test("a summary alone, as `| tail` leaves it, is a result without detail", () => {
  const run = parseRun("\n Test Files  1 failed | 2 passed (3)\n      Tests  4 failed | 20 passed (24)\n   Duration  3.1s\n", [], 1);
  assert.deepEqual([run.status, run.failed, run.passed, run.duration, run.failures], ["failed", 4, 20, 3100, []]);
});

test("not understood: output with no results, a command that failed to start, nothing at all", () => {
  for (const output of ["", "   ", "Error: Cannot find module 'vitest'\n", "error[E0425]: cannot find value `x` in this scope\n", "npm ERR! missing script: test\n", "All good! 3 tests failed successfully\n", "\u001b[31m\u001b[0m"]) {
    const run = parseRun(output, [], 1);
    assert.equal(run.status, "not-understood", JSON.stringify(output));
    assert.deepEqual([run.passed, run.failed, run.failures], [null, null, []]);
  }
});

test("a run with no test is not a pass", () => {
  const run = parseRun("============================ no tests ran in 0.01s =============================\n", ["pytest"], 5);
  assert.deepEqual([run.status, run.reason], ["not-understood", "no-tests"]);
  assert.equal(parseRun(" Test Files  no tests\n      Tests  no tests\n", [], 1).status, "not-understood");
});

test("a green summary under a non-zero exit code is not a pass", () => {
  const run = parseRun(fixture("pnpm-test.txt"), ["vitest"], 1);
  assert.deepEqual([run.status, run.failed, run.passed], ["failed", 0, 2224]);
});

test("a file that did not load fails the run, by the line of files", () => {
  const output = " RUN  v1.6.0 /work\n\n FAIL  src/a.test.ts [ src/a.test.ts ]\nError: Failed to resolve import \"./nope\" from \"src/a.test.ts\". Does the file exist?\n  Plugin: vite:import-analysis\n\n⎯⎯⎯⎯⎯⎯ Failed Suites 1 ⎯⎯⎯⎯⎯⎯⎯\n\n Test Files  1 failed (1)\n      Tests  no tests\n   Duration  1.2s\n";
  const run = parseRun(output, ["vitest"], 1);
  assert.deepEqual([run.status, run.failed], ["failed", 1]);
  assert.equal(run.failures[0].name, "src/a.test.ts");
  assert.match(run.failures[0].message, /Failed to resolve import/);
});

test("jest: a suite that failed to run is a failure named by its file", () => {
  const output = "FAIL ./broken.test.js\n  ● Test suite failed to run\n\n    Cannot find module './nope' from 'broken.test.js'\n\n      at Resolver.resolveModule (node_modules/jest-resolve/build/resolver.js:324:11)\n\nTest Suites: 1 failed, 1 total\nTests:       0 total\nTime:        0.3 s\n";
  const run = parseRun(output, ["jest"], 1);
  assert.equal(run.status, "failed");
  assert.deepEqual([run.failures[0].name, run.failures[0].message], ["./broken.test.js", "Cannot find module './nope' from 'broken.test.js'"]);
});

test("limits: failures and messages are capped, and a huge output is still read", () => {
  const many = Array.from({ length: 80 }, (_, i) => ` FAIL  a.test.js > t${i}\nError: ${"x".repeat(5000)}\n ❯ a.test.js:${i + 1}:1\n\n⎯⎯⎯⎯⎯⎯⎯⎯[${i + 1}/80]⎯\n`).join("\n");
  const run = parseRun(`${many}\n Test Files  1 failed (1)\n      Tests  80 failed (80)\n`, ["vitest"], 1);
  assert.equal(run.failed, 80);
  assert.equal(run.failures.length, 50);
  assert.ok(run.failures[0].message.length <= 2002);
  const big = `${"noise line\n".repeat(400000)}${fixture("vitest.fail.txt")}`;
  assert.ok(big.length > 3 * 1024 * 1024);
  assert.equal(parseRun(big, ["vitest"], 1).failed, 2, "the summary at the end of a huge output is read");
});
