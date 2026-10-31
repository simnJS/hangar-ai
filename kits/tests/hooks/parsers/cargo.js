"use strict";
const { splitLines, messageOf, failure, toMs, capFailures } = require("./common.js");

// `cargo test` runs one binary after another (unit tests, each integration test, the doc
// tests) and prints a result for each: the totals are their sum.
const RESULT = /^test result: (ok|FAILED)\.\s+(\d+) passed;\s+(\d+) failed;\s+(\d+) ignored;\s+(\d+) measured;\s+(\d+) filtered out(?:;\s+finished in ([\d.]+s))?/;
const FAILED_TEST = /^test (.+?) \.\.\. FAILED$/;
const STDOUT_HEADER = /^---- (.+?) (?:stdout|stderr) ----$/;
const PANIC = /panicked at (.+?):(\d+):\d+:?\s*$/;

function parse(lines) {
  let results = 0;
  const totals = { passed: 0, failed: 0, skipped: 0 };
  let duration = null;
  for (const line of lines) {
    const match = RESULT.exec(line);
    if (!match) continue;
    results += 1;
    totals.passed += Number(match[2]);
    totals.failed += Number(match[3]);
    totals.skipped += Number(match[4]);
    const ms = toMs(match[7]);
    if (ms !== null) duration = (duration || 0) + ms;
  }

  const names = [];
  for (const line of lines) {
    const match = FAILED_TEST.exec(line);
    if (match) names.push(match[1]);
  }

  // "---- name stdout ----" up to the next one, or the list "failures:" that closes them.
  const blocks = new Map();
  for (let i = 0; i < lines.length; i++) {
    const match = STDOUT_HEADER.exec(lines[i]);
    if (!match) continue;
    const body = [];
    for (i += 1; i < lines.length && !STDOUT_HEADER.test(lines[i]) && !/^failures:\s*$/.test(lines[i]) && !RESULT.test(lines[i]); i++) body.push(lines[i]);
    i -= 1;
    blocks.set(match[1], body);
  }

  const failures = [];
  for (const name of names) {
    const body = blocks.get(name) || [];
    let file;
    let line;
    const message = [];
    for (const row of body) {
      const panic = PANIC.exec(row);
      if (panic) {
        // Before Rust 1.73 the message sat on this line: panicked at 'message', src/lib.rs:7:5
        const old = /^'(.*)', (.+)$/.exec(panic[1]);
        file = old ? old[2] : panic[1];
        line = Number(panic[2]);
        if (old) message.push(old[1]);
        continue;
      }
      if (/^note: run with `RUST_BACKTRACE/.test(row)) continue;
      message.push(row);
    }
    failures.push(failure(name, messageOf(message), file, line));
  }

  // One "Running" or "Doc-tests" line opens each binary, and one result closes it: fewer results
  // than binaries is an output cut before the end, whose totals would be too good.
  const binaries = lines.filter((line) => /^\s+(?:Running|Doc-tests)\s/.test(line)).length;
  const cut = results < binaries;
  if (cut && !failures.length) return null;
  if (!results && !failures.length) return null;
  if (!results || cut) return { framework: "cargo", passed: null, failed: failures.length, skipped: null, duration: null, failures: capFailures(failures), partial: true };
  return { framework: "cargo", passed: totals.passed, failed: totals.failed, skipped: totals.skipped, duration, failures: capFailures(failures), partial: false };
}

module.exports = { framework: "cargo", parse: (text) => parse(splitLines(text)) };
