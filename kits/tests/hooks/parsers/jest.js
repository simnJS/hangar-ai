"use strict";
const { splitLines, messageOf, failure, toMs, locate, capFailures } = require("./common.js");

const SUITES = /^Test Suites:\s+(.+)$/;
const TESTS = /^Tests:\s+(.+)$/;
const TIME = /^Time:\s+(.+)$/;
const FILE_LINE = /^(?:FAIL|PASS)\s+(?:\S+\s+)?(\S+\.\w+)\s*(?:\([\d.]+\s*m?s\))?\s*$/;
const BULLET = /^ {2}● (.+)$/;
const CODE_FRAME = /^\s+>?\s*\d+\s*\|/;
const STACK = /^\s+at\s/;

function counts(text) {
  const out = { passed: 0, failed: 0, skipped: 0 };
  for (const match of text.matchAll(/(\d+)\s+(failed|passed|skipped|todo)/g)) {
    out[match[2] === "failed" ? "failed" : match[2] === "passed" ? "passed" : "skipped"] += Number(match[1]);
  }
  return out;
}

function parse(lines) {
  let tests = null;
  let suites = null;
  let duration = null;
  for (const line of lines) {
    let match;
    if ((match = TESTS.exec(line))) tests = counts(match[1]);
    else if ((match = SUITES.exec(line))) suites = counts(match[1]);
    else if ((match = TIME.exec(line))) duration = toMs(match[1]);
  }

  // Jest prints each failure where it happened, and again under "Summary of all failing
  // tests" when several files ran: the first of a name is kept.
  const failures = [];
  let suiteFailures = 0; // suites that did not run: no test of theirs is counted
  const seen = new Set();
  let file = null;
  for (let i = 0; i < lines.length; i++) {
    const fileLine = FILE_LINE.exec(lines[i]);
    if (fileLine) file = fileLine[1];
    const match = BULLET.exec(lines[i]);
    if (!match || /^Console\b/.test(match[1]) || /^Validation Warning/.test(match[1])) continue;
    const body = [];
    let where = null;
    for (i += 1; i < lines.length && !BULLET.test(lines[i]) && !/^(Test Suites:|Summary of all failing tests|Snapshot Summary)/.test(lines[i]) && !/^(FAIL|PASS)\s/.test(lines[i]); i++) {
      if (!where && STACK.test(lines[i])) where = locate(lines[i]);
      body.push(lines[i]);
    }
    i -= 1;
    const message = [];
    for (const line of body) {
      if (CODE_FRAME.test(line) || STACK.test(line)) break;
      message.push(line);
    }
    const notRun = /^Test suite failed to run/.test(match[1]);
    const name = notRun && file ? file : match[1].trim();
    const key = `${file}\0${name}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (notRun) suiteFailures += 1;
    failures.push(failure(name, messageOf(message), where ? where.file : file, where ? where.line : undefined));
  }

  if (!tests && !suites && !failures.length) return null;
  if (!tests) {
    // "Test suite failed to run" ends with no line of tests: the suites say what happened.
    if (suites && suites.failed > 0) return { framework: "jest", passed: suites.passed, failed: suites.failed, skipped: suites.skipped, duration, failures: capFailures(failures), partial: false };
    return { framework: "jest", passed: null, failed: failures.length, skipped: null, duration, failures: capFailures(failures), partial: true };
  }
  return { framework: "jest", passed: tests.passed, failed: tests.failed + suiteFailures, skipped: tests.skipped, duration, failures: capFailures(failures), partial: false };
}

module.exports = { framework: "jest", parse: (text) => parse(splitLines(text)) };
