"use strict";
const { splitLines, messageOf, failure, toMs, locate, capFailures } = require("./common.js");

// "ok  <tab>pkg<tab>0.003s", "ok  pkg  (cached)", "FAIL<tab>pkg<tab>0.2s", "FAIL pkg [build failed]"
const PACKAGE = /^(ok|FAIL)\s+(\S+)\s+((?:[\d.]+s|\(cached\)|\[(?:build|setup) failed\]).*)$/;
const RESULT = /^(\s*)--- (PASS|FAIL|SKIP): (\S+) \(([\d.]+s)\)/;

/** `go test -json`: one event a line. */
function parseJson(lines) {
  const tests = new Map();
  let any = false;
  let duration = 0;
  for (const line of lines) {
    if (!line.startsWith("{")) continue;
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    if (!event || typeof event.Action !== "string") continue;
    any = true;
    const key = `${event.Package}\0${event.Test || ""}`;
    if (!tests.has(key)) tests.set(key, { pkg: event.Package, name: event.Test || "", output: [], action: null });
    const test = tests.get(key);
    if (event.Action === "output") test.output.push(String(event.Output || "").replace(/\n$/, ""));
    else if (["pass", "fail", "skip"].includes(event.Action)) {
      test.action = event.Action;
      if (!event.Test && Number.isFinite(event.Elapsed)) duration += event.Elapsed * 1000;
    }
  }
  if (!any) return null;
  const out = { passed: 0, failed: 0, skipped: 0 };
  const failures = [];
  for (const test of tests.values()) {
    if (!test.name || !test.action) continue;
    if (test.action === "pass") out.passed += 1;
    else if (test.action === "skip") out.skipped += 1;
    else {
      out.failed += 1;
      const body = test.output.filter((row) => !/^(=== |--- |\s*--- )/.test(row.trim()) && row.trim());
      const where = locate(body.join("\n"));
      failures.push(failure(test.name, messageOf(body.map((row) => row.replace(/^\s{4}/, ""))), where ? where.file : undefined, where ? where.line : undefined));
    }
  }
  const finished = [...tests.values()].some((test) => !test.name && test.action);
  if (!finished && !failures.length) return null;
  return { framework: "go", passed: out.passed, failed: out.failed, skipped: out.skipped, duration: Math.round(duration), failures: capFailures(failures), partial: !finished };
}

function parse(lines) {
  const json = parseJson(lines);
  if (json) return json;

  const packages = lines.map((line) => PACKAGE.exec(line)).filter(Boolean);
  let duration = null;
  let bad = 0;
  for (const match of packages) {
    if (match[1] !== "ok") bad += 1;
    const ms = toMs((match[3] || "").split(/\s+/)[0]);
    if (ms !== null) duration = (duration || 0) + ms;
  }

  // "--- FAIL: TestX (0.00s)" and the indented lines after it, up to the next result or package line.
  // With -v the lines of a test come between its "=== RUN" and its "---" line instead.
  const ends = (line) => RESULT.test(line) || /^(?:(?:FAIL|ok|\?|PASS|exit status)(?:\s|$)|=== )/.test(line);
  const ran = new Map();
  let running = null;
  for (const line of lines) {
    const start = /^=== (?:RUN|CONT)\s+(\S+)/.exec(line);
    if (start) {
      running = start[1];
      if (!ran.has(running)) ran.set(running, []);
    } else if (ends(line)) running = null;
    else if (running) ran.get(running).push(line);
  }
  const results = [];
  for (let i = 0; i < lines.length; i++) {
    const match = RESULT.exec(lines[i]);
    if (!match) continue;
    const body = [];
    for (i += 1; i < lines.length && !ends(lines[i]); i++) body.push(lines[i]);
    i -= 1;
    results.push({ depth: match[1].length, verdict: match[2], name: match[3], body: body.some((row) => row.trim()) ? body : ran.get(match[3]) || [] });
  }
  const failed = results.filter((item) => item.verdict === "FAIL");
  // A parent fails when a subtest does: the subtest is the failure worth showing.
  const leaves = failed.filter((item) => !failed.some((other) => other !== item && other.name.startsWith(`${item.name}/`)));
  const failures = leaves.map((item) => {
    const body = item.body.map((row) => row.replace(/^\s{4}/, ""));
    const where = locate(body.join("\n"));
    return failure(item.name, messageOf(body), where ? where.file : undefined, where ? where.line : undefined);
  });

  // A package that did not build has no test to name: its compiler output is the failure.
  for (let i = 0; i < lines.length; i++) {
    const match = /^FAIL\s+(\S+)\s+\[(?:build|setup) failed\]/.exec(lines[i]);
    if (!match) continue;
    const body = [];
    for (let j = i - 1; j >= 0 && !/^(FAIL|ok|\?)\s/.test(lines[j]); j--) body.unshift(lines[j]);
    const where = locate(body.join("\n"));
    failures.push(failure(match[1], messageOf(body), where ? where.file : undefined, where ? where.line : undefined));
  }

  const top = results.filter((item) => item.depth === 0);
  if (!packages.length) {
    if (!failures.length) return null;
    return { framework: "go", passed: null, failed: failures.length, skipped: null, duration: null, failures: capFailures(failures), partial: true };
  }
  const verbose = top.some((item) => item.verdict === "PASS");
  const failedCount = failures.length || (bad ? bad : 0);
  return {
    framework: "go",
    // A plain run counts packages, not tests: without -v, the number of passing tests is not in the output.
    passed: verbose ? top.filter((item) => item.verdict === "PASS").length : null,
    failed: failedCount,
    skipped: top.filter((item) => item.verdict === "SKIP").length,
    duration,
    failures: capFailures(failures),
    partial: false,
  };
}

module.exports = { framework: "go", parse: (text) => parse(splitLines(text)) };
