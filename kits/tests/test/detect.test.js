"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const { detect } = require("../hooks/detect.js");

const frameworks = (command) => (detect(command) ? detect(command).frameworks : null);

test("each framework, by the command that starts it", () => {
  assert.deepEqual(frameworks("vitest run"), ["vitest"]);
  assert.deepEqual(frameworks("npx vitest run src/lib"), ["vitest"]);
  assert.deepEqual(frameworks("pnpm exec vitest --reporter=verbose"), ["vitest"]);
  assert.deepEqual(frameworks("npx jest --ci"), ["jest"]);
  assert.deepEqual(frameworks("pytest -x tests/"), ["pytest"]);
  assert.deepEqual(frameworks("python -m pytest"), ["pytest"]);
  assert.deepEqual(frameworks("python3 -m pytest -k add"), ["pytest"]);
  assert.deepEqual(frameworks("uv run pytest"), ["pytest"]);
  assert.deepEqual(frameworks("poetry run pytest -q"), ["pytest"]);
  assert.deepEqual(frameworks("cargo test"), ["cargo"]);
  assert.deepEqual(frameworks("cargo +nightly test --workspace"), ["cargo"]);
  assert.deepEqual(frameworks("go test ./..."), ["go"]);
  assert.deepEqual(frameworks("node --test"), ["node"]);
  assert.deepEqual(frameworks("node --test kits/guard/"), ["node"]);
  assert.deepEqual(frameworks("node --experimental-test-coverage --test test/"), ["node"]);
});

test("a package script's framework is not in the command: all parsers look", () => {
  for (const command of ["pnpm test", "npm test", "npm t", "yarn test", "bun run test", "pnpm run test:unit", "npm run test -- --watch=false", "pnpm -r test", "pnpm --filter web test", "pnpm -C kits/x test", "yarn run test:e2e"]) {
    assert.deepEqual(frameworks(command), [], command);
  }
});

test("a chain is read by each of its commands, and a wrapper does not hide one", () => {
  assert.deepEqual(frameworks("cd app && npx vitest run"), ["vitest"]);
  assert.deepEqual(frameworks("pnpm test && node --test kits/tests/"), [], "a script and a named suite: the output is read by all");
  assert.deepEqual(frameworks("cargo test; go test ./..."), ["cargo", "go"]);
  assert.deepEqual(frameworks("CI=1 NODE_ENV=test npx vitest run"), ["vitest"]);
  assert.deepEqual(frameworks("env FORCE_COLOR=0 pytest"), ["pytest"]);
  assert.deepEqual(frameworks("cross-env CI=1 jest"), ["jest"]);
  assert.deepEqual(frameworks("& cargo test"), ["cargo"], "PowerShell's call operator");
  assert.deepEqual(frameworks("pnpm test 2>&1 | tail -30"), []);
  assert.deepEqual(frameworks("vitest run | tee out.txt"), ["vitest"]);
  assert.deepEqual(frameworks("$env:CI='1'; pnpm test"), []);
});

test("a command that merely mentions a test runner is not a run", () => {
  for (const command of [
    'git commit -m "make vitest faster"',
    "git commit -m 'run cargo test' && git push",
    'echo "pnpm test" > notes.txt',
    "grep -rn 'jest' src/",
    "cat vitest.config.ts",
    "ls tests/",
    "npm install vitest",
    "pnpm add -D jest",
    "cargo build",
    "cargo check --tests",
    "go build ./... && go vet ./...",
    "node script.js --test-mode",
    "node build.js",
    "pnpm build",
    "pnpm run lint",
    "pnpm testing-library-docs",
    "python script.py",
    "vitest --version",
    "pytest --help",
    "",
    "   ",
  ]) {
    assert.equal(detect(command), null, JSON.stringify(command));
  }
});

test("what is not a command line is no run either", () => {
  assert.equal(detect(undefined), null);
  assert.equal(detect(null), null);
  assert.equal(detect({ command: "pnpm test" }), null);
  assert.equal(detect(`pnpm test ${"x".repeat(30000)}`), null, "a command line of 30 000 characters is not one an agent typed to run tests");
});
