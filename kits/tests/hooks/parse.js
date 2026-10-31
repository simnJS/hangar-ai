"use strict";
// The output of a test run, turned into the kit's record. Pure: no file, no clock.
const { clean } = require("./parsers/common.js");

const PARSERS = {
  vitest: () => require("./parsers/vitest.js"),
  jest: () => require("./parsers/jest.js"),
  pytest: () => require("./parsers/pytest.js"),
  cargo: () => require("./parsers/cargo.js"),
  go: () => require("./parsers/go.js"),
  node: () => require("./parsers/node.js"),
};

// Hangar hands a tab 1 MB at most, and a run's failures are capped well under it. What is
// read from the output has its own bound: the beginning (the first failures) and the end
// (the summary).
const OUTPUT_HEAD = 1024 * 1024;
const OUTPUT_TAIL = 2 * 1024 * 1024;

const bound = (text) => (text.length > OUTPUT_HEAD + OUTPUT_TAIL ? `${text.slice(0, OUTPUT_HEAD)}\n${text.slice(-OUTPUT_TAIL)}` : text);

/**
 * Reads `output` with the parsers of `frameworks` (all of them when the command does not
 * say which one runs, as `pnpm test` does), and merges what they understand: one command
 * line can run two suites, and their outputs come as one.
 *
 * Answers `{ status, framework, passed, failed, skipped, duration, failures, partial }`, where `status`
 * is "passed", "failed" or "not-understood". Nothing is ever "passed" without a
 * framework's own closing summary, a run with no test is not "passed", and a run whose
 * exit code is not 0 is not either.
 */
function parseRun(output, frameworks, exitCode) {
  const text = clean(bound(String(output || "")));
  const names = frameworks && frameworks.length ? frameworks : Object.keys(PARSERS);
  const understood = [];
  for (const name of names) {
    if (!PARSERS[name]) continue;
    let result = null;
    try {
      result = PARSERS[name]().parse(text);
    } catch {
      result = null; // a parser that trips on an odd output says nothing, rather than something wrong
    }
    if (result) understood.push(result);
  }
  if (!understood.length) return notUnderstood(frameworks);

  const sum = (key) => (understood.every((item) => item[key] === null) ? null : understood.reduce((total, item) => total + (item[key] || 0), 0));
  const failed = sum("failed") || 0;
  const passed = sum("passed");
  const skipped = sum("skipped");
  const partial = understood.some((item) => item.partial);
  const nothingRan = !partial && passed === 0 && failed === 0 && !skipped;
  if (nothingRan) return { ...notUnderstood(frameworks), framework: understood.map((item) => item.framework).join(", "), reason: "no-tests" };

  const exitFailed = Number.isFinite(exitCode) && exitCode !== 0;
  return {
    status: failed > 0 || exitFailed || partial ? "failed" : "passed",
    framework: understood.map((item) => item.framework).join(", "),
    passed,
    failed,
    skipped,
    duration: sum("duration"),
    failures: understood.flatMap((item) => item.failures),
    partial,
  };
}

function notUnderstood(frameworks) {
  return {
    status: "not-understood",
    framework: frameworks && frameworks.length === 1 ? frameworks[0] : null,
    passed: null,
    failed: null,
    skipped: null,
    duration: null,
    failures: [],
    partial: false,
  };
}

module.exports = { parseRun, PARSERS };
